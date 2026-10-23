//! The core the tree and plan loops share (commands.md §17, rules R3 and R4): clock-aligned pacing and the reader
//! thread's one-at-a-time, newest-wins queue.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// One frame of the spinner, in ms (the clock of [`crate::palette::frame`]).
const FRAME_MS: u64 = 100;

/// How long to wait from `now_ms` (Unix ms) to the next 100 ms frame boundary of the clock: 1..=100 ms.
pub fn wait_for_next_frame(now_ms: u64) -> Duration {
    Duration::from_millis(FRAME_MS - now_ms % FRAME_MS)
}

/// [`wait_for_next_frame`] from the system clock now.
pub fn wait_now() -> Duration {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    wait_for_next_frame(now)
}

type Job<T> = Box<dyn FnOnce() -> T + Send + 'static>;

/// What the requester and the reader thread share: the newest waiting request, and whether the requester is gone.
struct Queue<T> {
    waiting: Option<Job<T>>,
    closed: bool,
}

/// Reads run on one thread, one at a time; a request made while one runs waits, and only the newest waiting one is kept.
pub struct Reads<T> {
    queue: Arc<(Mutex<Queue<T>>, Condvar)>,
    results: Receiver<T>,
}

impl<T: Send + 'static> Reads<T> {
    pub fn new() -> Self {
        let queue = Arc::new((Mutex::new(Queue { waiting: None, closed: false }), Condvar::new()));
        let (send, results): (Sender<T>, Receiver<T>) = mpsc::channel();
        let shared = Arc::clone(&queue);
        std::thread::Builder::new()
            .name("cpd-reads".into())
            .spawn(move || {
                let (lock, wake) = &*shared;
                loop {
                    let job = {
                        let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
                        loop {
                            if let Some(job) = q.waiting.take() {
                                break job;
                            }
                            if q.closed {
                                return;
                            }
                            q = wake.wait(q).unwrap_or_else(|e| e.into_inner());
                        }
                    };
                    if send.send(job()).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn the reader thread");
        Reads { queue, results }
    }

    /// Ask for `job` to run on the reader thread.
    pub fn request(&mut self, job: impl FnOnce() -> T + Send + 'static) {
        let (lock, wake) = &*self.queue;
        lock.lock().unwrap_or_else(|e| e.into_inner()).waiting = Some(Box::new(job));
        wake.notify_one();
    }

    /// A finished result, if there is one. Never blocks.
    pub fn take(&mut self) -> Option<T> {
        self.results.try_recv().ok()
    }
}

impl<T> Drop for Reads<T> {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
        q.closed = true;
        q.waiting = None;
        wake.notify_one();
    }
}
