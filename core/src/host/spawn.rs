//! The process seam of what [`Runner`](super::runner::Runner) is not: a child that outlives the call, is waited for
//! later, is signalled, or replaces the process (docs/implementing.md, "Ports, Spawn and run_with").
//! [`ProcessSpawn`] is the real one; [`FakeSpawn`] starts nothing and records.
//!
//! STUBS of PR ports2's red Job: every body is `todo!()`; Job ports-2 implements them.

use std::io::{self, BufRead};
use std::os::fd::RawFd;
use std::path::PathBuf;
use std::process::ExitStatus;

use crate::error::CpdError;

/// A process id, as `Spawn` hands it out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(pub u32);

/// A signal `Spawn::signal` sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Term,
    Kill,
}

/// What to start: the program, its argv, its environment and how it is detached.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub env_set: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub setsid: bool,
    pub stdout_to: Option<PathBuf>,
    pub hand_over: Option<RawFd>,
    pub idle: bool,
}

pub trait Spawn: Send + Sync {
    /// A detached process or a child; the real one reaps it.
    fn spawn(&self, l: &Launch) -> io::Result<Pid>;
    fn try_wait(&self, p: Pid) -> io::Result<Option<ExitStatus>>;
    fn signal(&self, p: Pid, s: Signal) -> io::Result<()>;
    /// Zombie-aware.
    fn alive(&self, p: Pid) -> bool;
    fn exe_of(&self, p: Pid) -> Option<PathBuf>;
    fn cmdline(&self, p: Pid) -> Option<String>;
    /// Returns only on failure.
    fn exec(&self, l: &Launch) -> io::Error;
    fn stream(&self, l: &Launch) -> io::Result<(Pid, Box<dyn BufRead + Send>)>;
    /// The path of cpd's own executable.
    fn self_exe(&self) -> Result<PathBuf, CpdError>;
    /// Which implementation this is, by name, for a test of `Ports` (`"ProcessSpawn"` for the real one).
    fn kind(&self) -> &'static str {
        "other"
    }
}

/// The real one: `Command`, `setsid`, `kill(2)`, `/proc` or `libproc`, `exec`.
pub struct ProcessSpawn;

impl Spawn for ProcessSpawn {
    fn spawn(&self, _l: &Launch) -> io::Result<Pid> {
        todo!("ProcessSpawn::spawn (Job ports-2)")
    }
    fn try_wait(&self, _p: Pid) -> io::Result<Option<ExitStatus>> {
        todo!("ProcessSpawn::try_wait (Job ports-2)")
    }
    fn signal(&self, _p: Pid, _s: Signal) -> io::Result<()> {
        todo!("ProcessSpawn::signal (Job ports-2)")
    }
    fn alive(&self, _p: Pid) -> bool {
        todo!("ProcessSpawn::alive (Job ports-2)")
    }
    fn exe_of(&self, _p: Pid) -> Option<PathBuf> {
        todo!("ProcessSpawn::exe_of (Job ports-2)")
    }
    fn cmdline(&self, _p: Pid) -> Option<String> {
        todo!("ProcessSpawn::cmdline (Job ports-2)")
    }
    fn exec(&self, _l: &Launch) -> io::Error {
        todo!("ProcessSpawn::exec (Job ports-2)")
    }
    fn stream(&self, _l: &Launch) -> io::Result<(Pid, Box<dyn BufRead + Send>)> {
        todo!("ProcessSpawn::stream (Job ports-2)")
    }
    fn self_exe(&self) -> Result<PathBuf, CpdError> {
        todo!("ProcessSpawn::self_exe (Job ports-2)")
    }
    fn kind(&self) -> &'static str {
        "ProcessSpawn"
    }
}

/// The fake: pids from a counter, a table of launches and their state, the record of signals. Starts nothing.
/// Configured through `&self`, so a test can hold it in an `Arc` beside the `Ports` that share it.
#[derive(Default)]
pub struct FakeSpawn {}

impl FakeSpawn {
    pub fn new() -> FakeSpawn {
        todo!("FakeSpawn::new (Job ports-2)")
    }
    /// Every `Launch` that `spawn` and `stream` received, in order.
    pub fn launches(&self) -> Vec<Launch> {
        todo!("FakeSpawn::launches (Job ports-2)")
    }
    /// Every `(pid, signal)` that `signal` received, in order.
    pub fn signals(&self) -> Vec<(Pid, Signal)> {
        todo!("FakeSpawn::signals (Job ports-2)")
    }
    /// Marks `pid` exited with `code`: `try_wait` then returns the status and `alive` is false.
    pub fn exit(&self, _pid: Pid, _code: i32) {
        todo!("FakeSpawn::exit (Job ports-2)")
    }
    /// What `self_exe` returns; until it is called `self_exe` is an `Err`.
    pub fn set_self_exe(&self, _path: impl Into<PathBuf>) {
        todo!("FakeSpawn::set_self_exe (Job ports-2)")
    }
    /// What `exe_of(pid)` returns.
    pub fn set_exe_of(&self, _pid: Pid, _path: impl Into<PathBuf>) {
        todo!("FakeSpawn::set_exe_of (Job ports-2)")
    }
    /// What `cmdline(pid)` returns.
    pub fn set_cmdline(&self, _pid: Pid, _line: &str) {
        todo!("FakeSpawn::set_cmdline (Job ports-2)")
    }
}

impl Spawn for FakeSpawn {
    fn spawn(&self, _l: &Launch) -> io::Result<Pid> {
        todo!("FakeSpawn::spawn (Job ports-2)")
    }
    fn try_wait(&self, _p: Pid) -> io::Result<Option<ExitStatus>> {
        todo!("FakeSpawn::try_wait (Job ports-2)")
    }
    fn signal(&self, _p: Pid, _s: Signal) -> io::Result<()> {
        todo!("FakeSpawn::signal (Job ports-2)")
    }
    fn alive(&self, _p: Pid) -> bool {
        todo!("FakeSpawn::alive (Job ports-2)")
    }
    fn exe_of(&self, _p: Pid) -> Option<PathBuf> {
        todo!("FakeSpawn::exe_of (Job ports-2)")
    }
    fn cmdline(&self, _p: Pid) -> Option<String> {
        todo!("FakeSpawn::cmdline (Job ports-2)")
    }
    fn exec(&self, _l: &Launch) -> io::Error {
        todo!("FakeSpawn::exec (Job ports-2)")
    }
    fn stream(&self, _l: &Launch) -> io::Result<(Pid, Box<dyn BufRead + Send>)> {
        todo!("FakeSpawn::stream (Job ports-2)")
    }
    fn self_exe(&self) -> Result<PathBuf, CpdError> {
        todo!("FakeSpawn::self_exe (Job ports-2)")
    }
}
