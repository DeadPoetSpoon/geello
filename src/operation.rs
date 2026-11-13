//! Cooperative cancellation and explicit accounting without an executor dependency.
use crate::GeelloError;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

/// Caller-selected experimental limits, not production deployment defaults.
#[derive(Debug, Clone, Copy)]
pub struct OperationLimits {
    pub max_live_bytes: u64,
    pub max_work: u64,
    pub max_commands: usize,
    pub max_diagnostics: usize,
}
/// Shared operation control. Clones observe the same cancellation and counters.
#[derive(Debug, Clone)]
pub struct OperationContext {
    state: Arc<State>,
}
#[derive(Debug)]
struct State {
    cancelled: AtomicBool,
    deadline: Option<Instant>,
    limits: OperationLimits,
    live: AtomicU64,
    work: AtomicU64,
}
impl OperationContext {
    pub fn new(limits: OperationLimits, deadline: Option<Instant>) -> Self {
        Self {
            state: Arc::new(State {
                cancelled: AtomicBool::new(false),
                deadline,
                limits,
                live: AtomicU64::new(0),
                work: AtomicU64::new(0),
            }),
        }
    }
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Release)
    }
    pub fn limits(&self) -> OperationLimits {
        self.state.limits
    }
    pub fn live_bytes(&self) -> u64 {
        self.state.live.load(Ordering::Acquire)
    }
    pub fn work(&self) -> u64 {
        self.state.work.load(Ordering::Acquire)
    }
    pub fn checkpoint(&self, cost: u64) -> Result<(), GeelloError> {
        self.check()?;
        charge(&self.state.work, cost, self.state.limits.max_work, "work")
    }
    pub fn check(&self) -> Result<(), GeelloError> {
        if self.state.cancelled.load(Ordering::Acquire) {
            return Err(GeelloError::Cancelled);
        }
        if self.state.deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(GeelloError::DeadlineExceeded);
        }
        Ok(())
    }
    pub(crate) fn reserve(&self, bytes: usize) -> Result<Reservation, GeelloError> {
        self.check()?;
        charge(
            &self.state.live,
            bytes as u64,
            self.state.limits.max_live_bytes,
            "memory",
        )?;
        Ok(Reservation {
            context: self.clone(),
            bytes: bytes as u64,
        })
    }
    /// Deadline polling interval for independent GPU completion workers.
    #[cfg(feature = "render-gpu")]
    pub(crate) fn poll_interval() -> std::time::Duration {
        std::time::Duration::from_millis(2)
    }
}
fn charge(counter: &AtomicU64, cost: u64, max: u64, kind: &'static str) -> Result<(), GeelloError> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            n.checked_add(cost).filter(|v| *v <= max)
        })
        .map(|_| ())
        .map_err(|_| GeelloError::BudgetExceeded(kind))
}
#[derive(Debug)]
pub(crate) struct Reservation {
    context: OperationContext,
    bytes: u64,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.context
            .state
            .live
            .fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
