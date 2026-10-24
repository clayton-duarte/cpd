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
    waker: Option<crate::pump::Waker>,
}

impl<T: Send + 'static> Background<T> {
    pub(crate) fn new() -> Background<T> {
        let (tx, rx) = channel();
        Background { tx, rx, queued: VecDeque::new(), running: false, waker: None }
    }

    // not called until the loops convert
    #[allow(dead_code)]
    pub(crate) fn with_waker(waker: crate::pump::Waker) -> Background<T> {
        Background { waker: Some(waker), ..Background::new() }
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
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            // the receiver is gone only when the view ended
            let _ = tx.send(job());
            if let Some(waker) = waker {
                waker.read_done();
            }
        });
    }
}
