//! The process seam: [`Runner`] runs one `Command` under a deadline, like [`deadline::output`].
//! [`SystemRunner`] is the real one; [`FakeRunner`] answers from a script, in-process, on virtual time.

use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::Arc;
use std::time::Duration;

use super::deadline::{self, Failed};
use crate::clock::ManualClock;

pub trait Runner: Send + Sync {
    /// Runs `cmd` for at most `deadline` and returns what [`deadline::output`] returns: the same `Output`,
    /// `Failed::Io` or `Failed::Timeout(tool_timeout)`.
    fn output(&self, cmd: &mut Command, tool: &str, deadline: Duration) -> Result<Output, Failed>;
}

/// The real runner: [`deadline::output`] and nothing else.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn output(&self, cmd: &mut Command, tool: &str, deadline: Duration) -> Result<Output, Failed> {
        deadline::output(cmd, tool, deadline)
    }
}

/// What a `Command` carries, as the fake saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub tool: String,
    pub program: String,
    pub args: Vec<String>,
    /// `Command::get_envs`: `None` is a removed variable.
    pub envs: Vec<(String, Option<String>)>,
    pub cwd: Option<PathBuf>,
    pub deadline: Duration,
}

/// How a scripted call ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Exit { code: i32, stdout: Vec<u8>, stderr: Vec<u8>, takes: Duration },
    Io(ErrorKind),
}

impl Reply {
    pub fn ok(_stdout: &str) -> Reply {
        todo!()
    }
    pub fn exit(_code: i32, _stdout: &str, _stderr: &str) -> Reply {
        todo!()
    }
    pub fn io(_kind: ErrorKind) -> Reply {
        todo!()
    }
    /// The reply takes `d` of virtual time.
    pub fn takes(self, _d: Duration) -> Reply {
        todo!()
    }
}

/// An in-process fake of one program (tmux, git), answering every call of it that no rule matched.
pub trait Handler: Send + Sync {
    fn handle(&self, call: &Call) -> Reply;
}

/// A scripted [`Runner`]; a clone shares the script and the record.
#[derive(Clone)]
pub struct FakeRunner {
    _shared: Arc<()>,
}

impl FakeRunner {
    pub fn new(_clock: ManualClock) -> FakeRunner {
        todo!()
    }
    /// A rule: the first match answers. `program` is compared with the program's file name.
    pub fn on(self, _program: &str, _matches: impl Fn(&Call) -> bool + Send + Sync + 'static, _reply: Reply) -> FakeRunner {
        todo!()
    }
    /// Like [`FakeRunner::on`], used up after one answer.
    pub fn once(self, _program: &str, _matches: impl Fn(&Call) -> bool + Send + Sync + 'static, _reply: Reply) -> FakeRunner {
        todo!()
    }
    /// Answers every call of `program` that no rule matched.
    pub fn route(self, _program: &str, _handler: Arc<dyn Handler>) -> FakeRunner {
        todo!()
    }
    /// Every call in order, answered or not.
    pub fn calls(&self) -> Vec<Call> {
        todo!()
    }
    /// The calls no rule matched.
    pub fn unscripted(&self) -> Vec<Call> {
        todo!()
    }
}

impl Runner for FakeRunner {
    fn output(&self, _cmd: &mut Command, _tool: &str, _deadline: Duration) -> Result<Output, Failed> {
        todo!()
    }
}
