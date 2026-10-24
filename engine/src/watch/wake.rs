//! Tick timing: when the next tick starts (contract §3.7).

use std::time::{Duration, Instant};

/// The least time between the start of a tick and an early one.
pub const EARLY_GAP: Duration = Duration::from_millis(500);

/// The time the next tick starts: the earliest of `began + tick`, `poll_due`, and (when `committed`)
/// `max(now, began + EARLY_GAP)` capped at `began + tick`.
pub fn next_tick(began: Instant, now: Instant, tick: Duration, poll_due: Option<Instant>, committed: bool) -> Instant {
    let _ = (began, now, tick, poll_due, committed);
    todo!()
}
