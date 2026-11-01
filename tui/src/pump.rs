//! The loop's one wait point: input, commit and read signals on one channel, the schedule of reads and the clock needs.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use cpd_core::clock::Clock;
use cpd_core::error::CpdError;
use ratatui::crossterm::event::Event;

use crate::read::watch::Watch;
use crate::view_ports::Input;
use cpd_core::ports::Ports;

pub const FRAME: Duration = Duration::from_millis(crate::palette::FRAME_MS);
pub const SAFETY: Duration = Duration::from_secs(2);

/// What a driven loop with nothing due polls its input for.
const IDLE: Duration = Duration::from_secs(60);

pub enum Signal {
    Input(Event),
    InputFailed(std::io::Error),
    Commit,
    Read,
    Due,
}

#[derive(Clone)]
pub struct Waker {
    send: Sender<Signal>,
}

impl Waker {
    pub fn commit(&self) {
        let _ = self.send.send(Signal::Commit);
    }
    /// A result is ready to take: a read's or a background job's.
    pub fn read_done(&self) {
        let _ = self.send.send(Signal::Read);
    }
}

/// How many `Due` answers in a row a scripted pump gives before it says the loop does not end: a script that does not end the
/// loop must fail loudly, not spin.
const DUE_LIMIT: usize = 10_000;

/// How the pump waits: on a thread and the real clock, on the loop's own input ([`Pump::undriven`]), or on a script and the
/// `Clock` of the ports.
enum Mode {
    Thread,
    Driven,
    Script {
        clock: Arc<dyn Clock>,
        signals: RefCell<VecDeque<Signal>>,
        dues: Cell<usize>,
    },
}

/// The one wait of a view loop. In production it reads input on its own thread and waits on the real clock. A scripted pump
/// ([`Pump::scripted`]) has no thread and never waits on the wall clock: it answers with what the test scripted.
pub struct Pump {
    send: Sender<Signal>,
    recv: Receiver<Signal>,
    mode: Mode,
}

impl Pump {
    pub fn new(mut input: impl FnMut() -> std::io::Result<Event> + Send + 'static) -> Pump {
        let (send, recv) = mpsc::channel();
        let tx = send.clone();
        std::thread::Builder::new()
            .name("cpd-input".into())
            .spawn(move || {
                loop {
                    let signal = match input() {
                        Ok(event) => Signal::Input(event),
                        Err(e) => {
                            let _ = tx.send(Signal::InputFailed(e));
                            return;
                        }
                    };
                    if tx.send(signal).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn the input thread");
        Pump {
            send,
            recv,
            mode: Mode::Thread,
        }
    }
    /// A pump with no thread: `wait` answers the signals of `signals` in order, each at once. Signals sent through its
    /// [`Waker`] (a commit the test announces, a read that finished inline) come before them. When none is left and the
    /// loop waits for a time, the pump advances `clock` to that time and never past it, and answers `Due`; a wait with no
    /// time and no signal left is a script that did not end the loop: it panics.
    pub fn scripted(clock: Arc<dyn Clock>, signals: Vec<Signal>) -> Pump {
        let (send, recv) = mpsc::channel();
        let mode = Mode::Script {
            clock,
            signals: RefCell::new(signals.into()),
            dues: Cell::new(0),
        };
        Pump { send, recv, mode }
    }
    /// Whether the jobs of this pump's loop run in the caller (a scripted pump) and not on their own threads.
    pub(crate) fn inline(&self) -> bool {
        matches!(self.mode, Mode::Script { .. })
    }
    /// A pump with no input thread: the loop that owns it waits through [`Pump::wait_driven`], on the input it is given.
    pub fn undriven() -> Pump {
        let (send, recv) = mpsc::channel();
        Pump {
            send,
            recv,
            mode: Mode::Driven,
        }
    }
    /// Whether the loop that owns this pump waits through [`Pump::wait_driven`] (it was made by [`Pump::undriven`]).
    pub fn is_driven(&self) -> bool {
        matches!(self.mode, Mode::Driven)
    }
    pub fn waker(&self) -> Waker {
        Waker {
            send: self.send.clone(),
        }
    }
    /// [`Pump::wait`] for a loop that is driven: no input thread, and no real time. A signal a background job posted is
    /// returned at once. While work is `busy` the wait blocks until it posts its signal (the work ends by itself, in no
    /// time of the clock's), unless `input` says an event is there meanwhile. Else nothing is under way: `input` is polled for the time left until `until` (a poll that
    /// finds nothing naps it on the `ports`' clock and says so), so the input decides what happens next.
    pub fn wait_driven(
        &self,
        ports: &Ports,
        input: &mut dyn Input,
        until: Option<Instant>,
        busy: bool,
    ) -> Signal {
        if let Ok(signal) = self.recv.try_recv() {
            return signal;
        }
        if busy {
            return match input.ready_while_busy() {
                Err(e) => Signal::InputFailed(e),
                Ok(true) => input.read().map_or_else(Signal::InputFailed, Signal::Input),
                Ok(false) => self.recv.recv().unwrap_or(Signal::Due),
            };
        }
        let wait = until.map_or(IDLE, |at| at.saturating_duration_since(ports.clock.now()));
        match input.poll(wait) {
            Err(e) => Signal::InputFailed(e),
            Ok(false) => Signal::Due,
            Ok(true) => input.read().map_or_else(Signal::InputFailed, Signal::Input),
        }
    }
    pub fn wait(&self, until: Option<Instant>) -> Signal {
        if let Mode::Script {
            clock,
            signals,
            dues,
        } = &self.mode
        {
            let next = self
                .recv
                .try_recv()
                .ok()
                .or_else(|| signals.borrow_mut().pop_front());
            if let Some(signal) = next {
                dues.set(0);
                return signal;
            }
            let at = until.expect("the script is over and the loop waits for a signal: end it with Signal::InputFailed");
            assert!(
                dues.get() < DUE_LIMIT,
                "the script is over and the loop still waits: end it with Signal::InputFailed"
            );
            dues.set(dues.get() + 1);
            let wait = at.saturating_duration_since(clock.now());
            if !wait.is_zero() {
                clock.sleep(wait);
            }
            return Signal::Due;
        }
        // The pump holds a `Sender` itself, so the channel never disconnects.
        match until {
            None => self.recv.recv().unwrap_or(Signal::Due),
            Some(at) => self
                .recv
                .recv_timeout(at.saturating_duration_since(Instant::now()))
                .unwrap_or(Signal::Due),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadKind {
    pub force: bool,
}

pub struct Schedule {
    every: Duration,
    /// The start of the last read, or of the schedule before any.
    since: Instant,
    started: bool,
    wanted: bool,
    force: bool,
    out: bool,
}

impl Schedule {
    pub fn new(now: Instant, every: Duration) -> Schedule {
        Schedule {
            every,
            since: now,
            started: false,
            wanted: false,
            force: false,
            out: false,
        }
    }
    pub fn commit(&mut self) {
        self.wanted = true;
    }
    pub fn force(&mut self) {
        self.wanted = true;
        self.force = true;
    }
    pub fn take(&mut self, now: Instant) -> Option<ReadKind> {
        if self.out {
            return None;
        }
        let frame_over = !self.started || now >= self.since + FRAME;
        if !((self.wanted && frame_over) || now >= self.since + self.every) {
            return None;
        }
        let kind = ReadKind { force: self.force };
        self.since = now;
        self.started = true;
        self.wanted = false;
        self.force = false;
        self.out = true;
        Some(kind)
    }
    pub fn done(&mut self) {
        self.out = false;
    }
    /// Whether a read is out: its result is still to come.
    pub fn is_out(&self) -> bool {
        self.out
    }
    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        if self.out {
            None
        } else if self.wanted {
            Some(if self.started {
                self.since + FRAME
            } else {
                now
            })
        } else {
            Some(self.since + self.every)
        }
    }
}

pub struct Needs {
    pub frame: bool,
    pub second: bool,
    pub at: Option<Instant>,
}

pub fn wake(
    now: Instant,
    wall_ms: u64,
    needs: &Needs,
    deadline: Option<Instant>,
) -> Option<Instant> {
    let next = |period: u64| now + Duration::from_millis(period - wall_ms % period);
    [
        deadline,
        needs.at,
        needs.frame.then(|| next(crate::palette::FRAME_MS)),
        needs.second.then(|| next(1000)),
    ]
    .into_iter()
    .flatten()
    .min()
}

/// What a loop that follows the store waits on: the pump, and the commit watcher whose signals the pump carries. The watcher is
/// an injectable: production starts it ([`Feed::real`]; its failure is the loop's to show), a test has none and says the store
/// changed with [`Waker::commit`].
pub struct Feed {
    pub pump: Pump,
    pub watch: Result<Option<Watch>, CpdError>,
}

impl Feed {
    /// The input thread on the terminal, and a watcher of the store's directory `dir`.
    pub fn real(dir: &Path) -> Feed {
        let pump = Pump::new(ratatui::crossterm::event::read);
        let watch = Watch::start(dir, pump.waker()).map(Some);
        Feed { pump, watch }
    }
    /// A scripted pump over `clock` and no watcher.
    pub fn scripted(clock: Arc<dyn Clock>, signals: Vec<Signal>) -> Feed {
        Feed {
            pump: Pump::scripted(clock, signals),
            watch: Ok(None),
        }
    }
    /// A pump with no input thread and no watcher: the loop waits on the input it is given ([`Pump::wait_driven`]) and polls.
    pub fn driven() -> Feed {
        Feed {
            pump: Pump::undriven(),
            watch: Ok(None),
        }
    }
}
