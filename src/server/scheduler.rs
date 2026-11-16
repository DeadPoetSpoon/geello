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
    memory: OperationContext,
    profile: ServerProfile,
    activities: Arc<super::activity::Activities>,
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
        let limits = OperationLimits {
            max_live_bytes: profile.max_live_bytes,
            max_work: u64::MAX,
            max_commands: profile.max_commands,
            max_diagnostics: 32,
        };
        let memory = OperationContext::new(limits, None);
        Self {
            http: class(profile.active_http),
            animation: class(profile.active_animation),
            background: class(profile.active_background),
            global: memory.child(limits, None),
            memory,
            profile,
            activities: super::activity::Activities::new(),
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
        self.memory.live_bytes()
    }
    /// Persistent state must remain writable while cancelled workers record
    /// their terminal outcome during draining.
    pub(crate) fn memory_context(&self) -> OperationContext {
        self.memory.clone()
    }
    /// Stop admission and cooperatively cancel existing work. Blocking code is
    /// never forcibly aborted; shutdown waits for its real lifetime to finish.
    pub fn begin_shutdown(&self) {
        self.activities.close();
        self.global.cancel();
        for class in [&self.http, &self.animation, &self.background] {
            class.queued.close();
            class.active.close();
        }
    }
    pub async fn shutdown(&self) {
        self.begin_shutdown();
        self.activities.drained().await;
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
        // Keep one acquire future across cancellation/deadline checks. Recreating
        // it on every timeout would repeatedly surrender the waiter's FIFO place.
        let acquire = c.active.clone().acquire_owned();
        tokio::pin!(acquire);
        loop {
            ctx.check()?;
            match tokio::time::timeout(Duration::from_millis(10), &mut acquire).await {
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
        let activity = self.activities.enter()?;
        let mut guard = CancelOnDrop(Some(ctx.clone()));
        let permits = self.admit(class, &ctx).await?;
        let result = tokio::task::spawn_blocking(move || {
            let _activity = activity;
            let _permits = permits;
            ctx.check()?;
            f(ctx)
        })
        .await
        .map_err(|_| ServerError::new(500, "worker", "Worker failed"))?;
        guard.0 = None;
        result
    }
    /// A foreground subscriber promotes a queued tile without starting a
    /// second encoder. Once admitted, the closure always runs exactly once.
    pub(crate) async fn blocking_tile<T: Send + 'static>(
        &self,
        ctx: OperationContext,
        mut foreground: tokio::sync::watch::Receiver<bool>,
        f: impl FnOnce(OperationContext) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let activity = self.activities.enter()?;
        let mut guard = CancelOnDrop(Some(ctx.clone()));
        let is_foreground = *foreground.borrow_and_update();
        let permits = if is_foreground {
            self.admit(WorkClass::Http, &ctx).await?
        } else {
            tokio::select! {
                biased;
                _ = foreground.changed() => self.admit(WorkClass::Http, &ctx).await?,
                permits = self.admit(WorkClass::Background, &ctx) => permits?,
            }
        };
        let result = tokio::task::spawn_blocking(move || {
            let _activity = activity;
            let _permits = permits;
            ctx.check()?;
            f(ctx)
        })
        .await
        .map_err(|_| ServerError::new(500, "worker", "Worker failed"))?;
        guard.0 = None;
        result
    }
    pub async fn asynchronous<T: Send + 'static>(
        &self,
        class: WorkClass,
        ctx: OperationContext,
        f: impl Future<Output = Result<T>> + Send + 'static,
    ) -> Result<T> {
        let activity = self.activities.enter()?;
        let mut guard = CancelOnDrop(Some(ctx.clone()));
        let permits = self.admit(class, &ctx).await?;
        ctx.check()?;
        // The server owns the driver until it drains, just as it owns blocking
        // closures. An abandoned caller cancels cooperatively, but cannot drop
        // a submitted GPU future and release its slot during device completion.
        let result = tokio::spawn(async move {
            let _activity = activity;
            let _permits = permits;
            f.await
        })
        .await
        .map_err(|_| ServerError::new(500, "worker", "Worker failed"))?;
        guard.0 = None;
        result
    }
    pub(crate) async fn asynchronous_tile<T: Send + 'static>(
        &self,
        ctx: OperationContext,
        mut foreground: tokio::sync::watch::Receiver<bool>,
        f: impl Future<Output = Result<T>> + Send + 'static,
    ) -> Result<T> {
        let activity = self.activities.enter()?;
        let mut guard = CancelOnDrop(Some(ctx.clone()));
        let is_foreground = *foreground.borrow_and_update();
        let permits = if is_foreground {
            self.admit(WorkClass::Http, &ctx).await?
        } else {
            tokio::select! {
                biased;
                _ = foreground.changed() => self.admit(WorkClass::Http,&ctx).await?,
                permits = self.admit(WorkClass::Background,&ctx) => permits?,
            }
        };
        ctx.check()?;
        // The server owns the driver until it drains, just as it owns blocking
        // closures. An abandoned caller cancels cooperatively, but cannot drop
        // a submitted GPU future and release its slot during device completion.
        let result = tokio::spawn(async move {
            let _activity = activity;
            let _permits = permits;
            f.await
        })
        .await
        .map_err(|_| ServerError::new(500, "worker", "Worker failed"))?;
        guard.0 = None;
        result
    }
}
