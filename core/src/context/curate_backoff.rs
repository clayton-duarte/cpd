//! Write-time curation: the backoff after a failed judge call.

use std::time::{Duration, Instant};

pub struct Backoff {
    failures: u32,
    next_at: Option<Instant>,
}

impl Backoff {
    pub fn new() -> Self {
        todo!()
    }
    pub fn due(&self, _now: Instant) -> bool {
        todo!()
    }
    pub fn failed(&mut self, _now: Instant) {
        let _ = (self.failures, self.next_at);
        todo!()
    }
    pub fn succeeded(&mut self) {
        todo!()
    }
    pub fn delay(&self) -> Duration {
        todo!()
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}
