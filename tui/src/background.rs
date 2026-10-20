//! Work a view starts for a key or a click (a `cpd` subprocess, a tmux call, a popup that returns when it closes)
//! runs off the view's thread (tui-W7): the view keeps drawing and reading input, and takes each result when it
//! ends. Jobs of one [`Background`] run one at a time, in the order they were submitted, so two clicks never race;
//! a view that must not wait for another kind of job (a popup) has a second [`Background`].

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};

type Job<T> = Box<dyn FnOnce() -> T + Send>;

pub(crate) struct Background<T> {
    tx: Sender<T>,
    rx: Receiver<T>,
    queued: VecDeque<Job<T>>,
    running: bool,
}

impl<T: Send + 'static> Background<T> {
    pub(crate) fn new() -> Background<T> {
        let (tx, rx) = channel();
        Background { tx, rx, queued: VecDeque::new(), running: false }
    }

    /// Runs `job` on a thread, after the jobs submitted before it.
    pub(crate) fn submit(&mut self, job: impl FnOnce() -> T + Send + 'static) {
        self.queued.push_back(Box::new(job));
        self.start();
    }

    /// The result of a job that ended since the last call, if any (the next queued job starts now). Never blocks.
    pub(crate) fn finished(&mut self) -> Option<T> {
        let result = self.rx.try_recv().ok()?;
        self.running = false;
        self.start();
        Some(result)
    }

    fn start(&mut self) {
        if self.running {
            return;
        }
        let Some(job) = self.queued.pop_front() else { return };
        self.running = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // the receiver is gone only when the view ended
            let _ = tx.send(job());
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn next(background: &mut Background<u32>) -> u32 {
        let start = Instant::now();
        loop {
            if let Some(n) = background.finished() {
                return n;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "no result");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn submit_does_not_wait_and_jobs_run_one_at_a_time_in_order() {
        let mut background = Background::new();
        let start = Instant::now();
        background.submit(|| {
            std::thread::sleep(Duration::from_millis(200));
            1
        });
        background.submit(|| 2);
        assert!(start.elapsed() < Duration::from_millis(100), "submit returns at once");
        assert_eq!(background.finished(), None, "the first has not ended");
        assert_eq!([next(&mut background), next(&mut background)], [1, 2]);
    }
}
