//! The core the tree and plan loops share (commands.md §17, rules R3 and R4): clock-aligned pacing and the reader
//! thread's one-at-a-time, newest-wins queue. Stubs: the implementation Job fills them in.

use std::time::Duration;

/// How long to wait from `now_ms` (Unix ms) to the next 100 ms frame boundary of the clock: 1..=100 ms.
pub fn wait_for_next_frame(_now_ms: u64) -> Duration {
    todo!("R3")
}

/// Reads run on one thread, one at a time; a request made while one runs waits, and only the newest waiting one is kept.
pub struct Reads<T> {
    _result: std::marker::PhantomData<T>,
}

impl<T: Send + 'static> Reads<T> {
    pub fn new() -> Self {
        todo!("R4")
    }

    /// Ask for `job` to run on the reader thread.
    pub fn request(&mut self, _job: impl FnOnce() -> T + Send + 'static) {
        todo!("R4")
    }

    /// A finished result, if there is one. Never blocks.
    pub fn take(&mut self) -> Option<T> {
        todo!("R4")
    }
}
