//! Bounded class-specific admission. Blocking tasks retain their permits until
//! actual completion even when an HTTP caller disappears. Cancellation remains
//! cooperative; no abandoned task frees capacity before its allocations drain.
use super::*;
use crate::{OperationContext, OperationLimits};
use std::{
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
#[derive(Debug, Clone, Copy)]
pub enum WorkClass {
    Http,
    Animation,
    Background,
}
struct Class {
    queued: Arc<Semaphore>,
    active: Arc<Semaphore>,
}
pub struct Scheduler {
    http: Class,
    animation: Class,
    background: Class,
    global: OperationContext,
    profile: ServerProfile,
}
struct CancelOnDrop(Option<OperationContext>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(c) = &self.0 {
            c.cancel();
        }
    }
}
impl Scheduler {
    pub fn new(profile: ServerProfile) -> Self {
        let class = |n| Class {
            queued: Arc::new(Semaphore::new(n + profile.queued_per_class)),
            active: Arc::new(Semaphore::new(n)),
        };
        Self {
            http: class(profile.active_http),
            animation: class(profile.active_animation),
            background: class(profile.active_background),
            global: OperationContext::new(
                OperationLimits {
                    max_live_bytes: profile.max_live_bytes,
                    max_work: u64::MAX,
                    max_commands: profile.max_commands,
                    max_diagnostics: 32,
                },
                None,
            ),
            profile,
        }
    }
    pub fn context(&self) -> OperationContext {
        self.global.child(
            OperationLimits {
                max_live_bytes: self.profile.max_live_bytes,
                max_work: self.profile.max_work,
                max_commands: self.profile.max_commands,
                max_diagnostics: 32,
            },
            Instant::now().checked_add(Duration::from_millis(self.profile.operation_timeout_ms)),
        )
    }
    pub fn live_bytes(&self) -> u64 {
        self.global.live_bytes()
    }
    async fn admit(
        &self,
        class: WorkClass,
        ctx: &OperationContext,
    ) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit)> {
        let c = match class {
            WorkClass::Http => &self.http,
            WorkClass::Animation => &self.animation,
            WorkClass::Background => &self.background,
        };
        let queue = c
            .queued
            .clone()
            .try_acquire_owned()
            .map_err(|_| ServerError::new(503, "queue-full", "Work queue is full"))?;
        loop {
            ctx.check()?;
            match tokio::time::timeout(Duration::from_millis(10), c.active.clone().acquire_owned())
                .await
            {
                Ok(Ok(active)) => return Ok((queue, active)),
                Ok(Err(_)) => return Err(ServerError::new(503, "shutdown", "Scheduler is closed")),
                Err(_) => {}
            }
        }
    }
    pub async fn blocking<T: Send + 'static>(
        &self,
        class: WorkClass,
        ctx: OperationContext,
        f: impl FnOnce(OperationContext) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let mut guard = CancelOnDrop(Some(ctx.clone()));
        let permits = self.admit(class, &ctx).await?;
        let result = tokio::task::spawn_blocking(move || {
            let _permits = permits;
            ctx.check()?;
            f(ctx)
        })
        .await
        .map_err(|_| ServerError::new(500, "worker", "Worker failed"))?;
        guard.0 = None;
        result
    }
    pub async fn asynchronous<T>(
        &self,
        class: WorkClass,
        ctx: OperationContext,
        f: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let mut guard = CancelOnDrop(Some(ctx.clone()));
        let _permits = self.admit(class, &ctx).await?;
        ctx.check()?;
        let result = f.await;
        guard.0 = None;
        result
    }
}
