//! Admission and lifetime tracking for shutdown. A detached blocking closure
//! retains its guard until actual completion, even if its async waiter is gone.
use super::{Result, ServerError};
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, watch};

#[derive(Default)]
struct State {
    closed: bool,
    active: usize,
}
pub(crate) struct Activities {
    state: Mutex<State>,
    changed: Notify,
    stopping: watch::Sender<bool>,
}
impl Activities {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            changed: Notify::new(),
            stopping: watch::channel(false).0,
        })
    }
    pub(crate) fn enter(self: &Arc<Self>) -> Result<Activity> {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return Err(ServerError::new(503, "shutdown", "Server is shutting down"));
        }
        state.active = state
            .active
            .checked_add(1)
            .ok_or_else(|| ServerError::new(503, "activity-budget", "Activity count overflow"))?;
        Ok(Activity(self.clone()))
    }
    pub(crate) fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        self.stopping.send_replace(true);
    }
    pub(crate) fn is_closed(&self) -> bool {
        self.state.lock().unwrap().closed
    }
    pub(crate) async fn closed(&self) {
        let mut stopping = self.stopping.subscribe();
        let _ = stopping.wait_for(|value| *value).await;
    }
    pub(crate) async fn drained(&self) {
        loop {
            // Register before checking the count, so the final guard cannot
            // notify between the check and waiter registration.
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.state.lock().unwrap().active == 0 {
                return;
            }
            changed.await;
        }
    }
}
pub(crate) struct Activity(Arc<Activities>);
impl Drop for Activity {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.active -= 1;
        if state.active == 0 {
            self.0.changed.notify_waiters();
        }
    }
}
