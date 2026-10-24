//! The core the tree and plan loops share (commands.md §17, rules R3 and R4): clock-aligned pacing and the reader
//! thread's one-at-a-time, newest-wins queue.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;

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
        Self::spawn(None)
    }

    fn spawn(waker: Option<crate::pump::Waker>) -> Self {
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
                    if let Some(w) = &waker {
                        w.read_done();
                    }
                }
            })
            .expect("spawn the reader thread");
        Reads { queue, results }
    }

    /// Like [`Reads::new`]; the reader thread calls `waker.read_done()` after each result it sends.
    pub(crate) fn with_waker(waker: crate::pump::Waker) -> Self {
        Self::spawn(Some(waker))
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

thread_local! {
    /// The reader thread's own store ([`Store`] is not `Send`: it is opened on the thread that reads with it).
    static STORE: std::cell::RefCell<Option<Store>> = const { std::cell::RefCell::new(None) };
}

/// What the reader thread reads with: its own [`Store`], opened on that thread at the first read (and again at the next
/// after a failed open), and `S`, the state the reads keep (a `Follow`). One job runs at a time, so the lock is never
/// contended; a view shares one `Arc<Mutex<Owned<S>>>` with every job it requests.
pub struct Owned<S> {
    home: CpdHome,
    busy_ms: u64,
    state: S,
}

impl<S> Owned<S> {
    pub fn new(home: CpdHome, busy_ms: u64, state: S) -> Arc<Mutex<Owned<S>>> {
        Arc::new(Mutex::new(Owned { home, busy_ms, state }))
    }

    /// Runs `read` with the store and the state; call it from the reader thread only.
    pub fn read<T>(
        shared: &Mutex<Owned<S>>,
        read: impl FnOnce(&mut Store, &mut S) -> Result<T, CpdError>,
    ) -> Result<T, CpdError> {
        let mut guard = shared.lock().unwrap_or_else(|e| e.into_inner());
        let owned = &mut *guard;
        STORE.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                *slot = Some(Store::open(&owned.home, owned.busy_ms)?);
            }
            read(slot.as_mut().expect("opened above"), &mut owned.state)
        })
    }
}
