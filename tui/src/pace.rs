//! The core the view loops share (commands.md §17, rule R4): the reader thread's one-at-a-time, newest-wins queue.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ports::Ports;
use cpd_core::store::Store;

type Job<T> = Box<dyn FnOnce() -> T + Send + 'static>;

/// What the requester and the reader thread share: the newest waiting request, and whether the requester is gone.
struct Queue<T> {
    waiting: Option<Job<T>>,
    closed: bool,
}

/// Reads run on one thread, one at a time; a request made while one runs waits, and only the newest waiting one is kept.
///
/// A scripted pump's `Reads` is the inline form ([`Reads::on`]): `request` runs the job in the caller, so a read is finished
/// when `request` returns and `take` yields it; no thread, no race. The newest-wins rule holds by construction: no request
/// waits.
pub struct Reads<T> {
    run: Run<T>,
    results: Receiver<T>,
}

/// Where the jobs run.
enum Run<T> {
    /// On the reader thread.
    Thread(Arc<(Mutex<Queue<T>>, Condvar)>),
    /// In the caller of `request`; the result is sent to `take` and the pump is woken.
    Inline {
        send: Sender<T>,
        waker: crate::pump::Waker,
    },
}

impl<T: Send + 'static> Reads<T> {
    /// The reads of the loop `pump` drives: the reader thread's, or inline for a scripted pump.
    pub(crate) fn on(pump: &crate::pump::Pump) -> Self {
        if pump.inline() {
            let (send, results) = mpsc::channel();
            Reads {
                run: Run::Inline {
                    send,
                    waker: pump.waker(),
                },
                results,
            }
        } else {
            Self::with_waker(pump.waker())
        }
    }

    /// The reader thread calls `waker.read_done()` after each result it sends.
    pub(crate) fn with_waker(waker: crate::pump::Waker) -> Self {
        let queue = Arc::new((
            Mutex::new(Queue {
                waiting: None,
                closed: false,
            }),
            Condvar::new(),
        ));
        let (send, results): (Sender<T>, Receiver<T>) = mpsc::channel();
        let shared = Arc::clone(&queue);
        // The jobs read `Ports::current()`: this thread's scope is not the reader thread's.
        let ports = Ports::current();
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
                    if send.send(Ports::scope(&ports, job)).is_err() {
                        return;
                    }
                    waker.read_done();
                }
            })
            .expect("spawn the reader thread");
        Reads {
            run: Run::Thread(queue),
            results,
        }
    }

    /// Ask for `job` to run on the reader thread.
    pub fn request(&mut self, job: impl FnOnce() -> T + Send + 'static) {
        match &self.run {
            Run::Thread(queue) => {
                let (lock, wake) = &**queue;
                lock.lock().unwrap_or_else(|e| e.into_inner()).waiting = Some(Box::new(job));
                wake.notify_one();
            }
            Run::Inline { send, waker } => {
                // the receiver is `self.results`: it is there
                let _ = send.send(job());
                waker.read_done();
            }
        }
    }

    /// A finished result, if there is one. Never blocks.
    pub fn take(&mut self) -> Option<T> {
        self.results.try_recv().ok()
    }
}

impl<T> Drop for Reads<T> {
    fn drop(&mut self) {
        match &self.run {
            Run::Thread(queue) => {
                let (lock, wake) = &**queue;
                let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
                q.closed = true;
                q.waiting = None;
                wake.notify_one();
            }
            // the reader thread's store dies with the thread; the inline form's is this thread's: it closes with the reads
            Run::Inline { .. } => drop(STORE.with(|cell| cell.borrow_mut().take())),
        }
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
        Arc::new(Mutex::new(Owned {
            home,
            busy_ms,
            state,
        }))
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
