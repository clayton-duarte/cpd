//! The loop's one wait point: input, commit and read signals on one channel, the schedule of reads and the clock needs.
//! Stubs: the tests of `pump_tests` are red against them.
#![allow(dead_code, unused_variables)]

use std::time::{Duration, Instant};

use ratatui::crossterm::event::Event;

pub const FRAME: Duration = Duration::from_millis(100);
pub const SAFETY: Duration = Duration::from_secs(2);

pub enum Signal {
    Input(Event),
    InputFailed(std::io::Error),
    Commit,
    Read,
    Due,
}

#[derive(Clone)]
pub struct Waker {}

impl Waker {
    pub fn commit(&self) {
        todo!()
    }
    pub fn read_done(&self) {
        todo!()
    }
}

pub struct Pump {}

impl Pump {
    pub fn new(input: impl FnMut() -> std::io::Result<Event> + Send + 'static) -> Pump {
        todo!()
    }
    pub fn waker(&self) -> Waker {
        todo!()
    }
    pub fn wait(&self, until: Option<Instant>) -> Signal {
        todo!()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadKind {
    pub force: bool,
}

pub struct Schedule {}

impl Schedule {
    pub fn new(now: Instant, every: Duration) -> Schedule {
        todo!()
    }
    pub fn commit(&mut self) {
        todo!()
    }
    pub fn force(&mut self) {
        todo!()
    }
    pub fn take(&mut self, now: Instant) -> Option<ReadKind> {
        todo!()
    }
    pub fn done(&mut self) {
        todo!()
    }
    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        todo!()
    }
}

pub struct Needs {
    pub frame: bool,
    pub second: bool,
    pub at: Option<Instant>,
}

pub fn wake(now: Instant, wall_ms: u64, needs: &Needs, deadline: Option<Instant>) -> Option<Instant> {
    todo!()
}
