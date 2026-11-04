//! Write-time curation: the backoff after a failed judge call.

use std::time::{Duration, Instant};

const BASE: Duration = Duration::from_secs(60);
const CAP: Duration = Duration::from_secs(60 * 60);

pub struct Backoff {
    failures: u32,
    next_at: Option<Instant>,
}

impl Backoff {
    pub fn new() -> Self {
        Self {
            failures: 0,
            next_at: None,
        }
    }
    pub fn due(&self, now: Instant) -> bool {
        self.next_at.is_none_or(|at| now >= at)
    }
    pub fn failed(&mut self, now: Instant) {
        self.failures = self.failures.saturating_add(1);
        self.next_at = Some(now + self.delay());
    }
    pub fn succeeded(&mut self) {
        self.failures = 0;
        self.next_at = None;
    }
    pub fn delay(&self) -> Duration {
        match self.failures {
            0 => Duration::ZERO,
            n => BASE.saturating_mul(1u32 << (n - 1).min(16)).min(CAP),
        }
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}
