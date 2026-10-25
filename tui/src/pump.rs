//! The loop's one wait point: input, commit and read signals on one channel, the schedule of reads and the clock needs.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::Event;

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

pub struct Pump {
    send: Sender<Signal>,
    recv: Receiver<Signal>,
}

impl Pump {
    pub fn new(mut input: impl FnMut() -> std::io::Result<Event> + Send + 'static) -> Pump {
        let (send, recv) = mpsc::channel();
        let tx = send.clone();
        std::thread::Builder::new()
            .name("cpd-input".into())
            .spawn(move || loop {
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
            })
            .expect("spawn the input thread");
        Pump { send, recv }
    }
    /// A pump with no input thread: the loop that owns it waits through [`Pump::wait_driven`], on the input it is given.
    pub fn undriven() -> Pump {
        let (send, recv) = mpsc::channel();
        Pump { send, recv }
    }
    pub fn waker(&self) -> Waker {
        Waker { send: self.send.clone() }
    }
    /// [`Pump::wait`] for a loop that is driven: no input thread, and no real time. A signal a background job posted is
    /// returned at once. While work is `busy` the wait blocks until it posts its signal (the work ends by itself, in no
    /// time of the clock's). Else nothing is under way: `input` is polled for the time left until `until` (a poll that
    /// finds nothing naps it on the `ports`' clock and says so), so the input decides what happens next.
    pub fn wait_driven(&self, ports: &Ports, input: &mut dyn Input, until: Option<Instant>, busy: bool) -> Signal {
        if let Ok(signal) = self.recv.try_recv() {
            return signal;
        }
        if busy {
            return self.recv.recv().unwrap_or(Signal::Due);
        }
        let wait = until.map_or(IDLE, |at| at.saturating_duration_since(ports.clock.now()));
        match input.poll(wait) {
            Err(e) => Signal::InputFailed(e),
            Ok(false) => Signal::Due,
            Ok(true) => input.read().map_or_else(Signal::InputFailed, Signal::Input),
        }
    }
    pub fn wait(&self, until: Option<Instant>) -> Signal {
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
        Schedule { every, since: now, started: false, wanted: false, force: false, out: false }
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
            Some(if self.started { self.since + FRAME } else { now })
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

pub fn wake(now: Instant, wall_ms: u64, needs: &Needs, deadline: Option<Instant>) -> Option<Instant> {
    let next = |period: u64| now + Duration::from_millis(period - wall_ms % period);
    [deadline, needs.at, needs.frame.then(|| next(crate::palette::FRAME_MS)), needs.second.then(|| next(1000))]
        .into_iter()
        .flatten()
        .min()
}

/// What a test does with a pump: no input thread, and the wait on a clock.
#[cfg(test)]
impl Pump {
    /// A pump with no input thread: `signals` come out of [`Pump::wait_with`] in order, then `Due` (a test's pump).
    pub fn scripted(signals: Vec<Signal>) -> Pump {
        let (send, recv) = mpsc::channel();
        for signal in signals {
            let _ = send.send(signal);
        }
        Pump { send, recv }
    }
    /// [`Pump::wait`] on `clock`: a queued signal is returned at once; else the wait naps on the clock up to `until` (none
    /// when it has passed) and returns `Due`.
    pub fn wait_with(&self, clock: &dyn cpd_core::clock::Clock, until: Option<Instant>) -> Signal {
        if let Ok(signal) = self.recv.try_recv() {
            return signal;
        }
        if let Some(wait) = until.map(|at| at.saturating_duration_since(clock.now())).filter(|d| !d.is_zero()) {
            clock.sleep(wait);
        }
        Signal::Due
    }
}
