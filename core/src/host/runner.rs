//! The process seam: [`Runner`] runs one `Command` under a deadline, like [`deadline::output`].
//! [`SystemRunner`] is the real one; [`FakeRunner`] answers from a script, in-process, on virtual time.

use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use super::deadline::{self, Failed};
use crate::clock::{Clock, ManualClock};

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
    /// Exit 0 with `stdout`.
    pub fn ok(stdout: &str) -> Reply {
        Reply::exit(0, stdout, "")
    }
    pub fn exit(code: i32, stdout: &str, stderr: &str) -> Reply {
        Reply::Exit { code, stdout: stdout.as_bytes().to_vec(), stderr: stderr.as_bytes().to_vec(), takes: Duration::ZERO }
    }
    /// The program could not be started: `Failed::Io(kind)`.
    pub fn io(kind: ErrorKind) -> Reply {
        Reply::Io(kind)
    }
    /// The reply takes `d` of virtual time.
    pub fn takes(self, d: Duration) -> Reply {
        match self {
            Reply::Exit { code, stdout, stderr, .. } => Reply::Exit { code, stdout, stderr, takes: d },
            io @ Reply::Io(_) => io,
        }
    }
}

/// An in-process fake of one program (tmux, git), answering every call of it that no rule matched.
pub trait Handler: Send + Sync {
    fn handle(&self, call: &Call) -> Reply;
}

type Matches = Box<dyn Fn(&Call) -> bool + Send + Sync>;

struct Rule {
    program: String,
    matches: Matches,
    reply: Reply,
    once: bool,
    used: bool,
}

#[derive(Default)]
struct Script {
    rules: Vec<Rule>,
    routes: Vec<(String, Arc<dyn Handler>)>,
    calls: Vec<Call>,
    unscripted: Vec<Call>,
}

/// A scripted [`Runner`]; a clone shares the script and the record.
#[derive(Clone)]
pub struct FakeRunner {
    clock: ManualClock,
    script: Arc<Mutex<Script>>,
}

/// The program's file name: `/x/bin/tmux` is `tmux`.
fn file_name(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

impl FakeRunner {
    /// A fake with no rule; `Reply::takes` passes virtual time on `clock`.
    pub fn new(clock: ManualClock) -> FakeRunner {
        FakeRunner { clock, script: Arc::default() }
    }
    fn script(&self) -> MutexGuard<'_, Script> {
        self.script.lock().expect("the script is never held across a panic")
    }
    fn rule(self, program: &str, matches: Matches, reply: Reply, once: bool) -> FakeRunner {
        self.script().rules.push(Rule { program: program.to_owned(), matches, reply, once, used: false });
        self
    }
    /// A rule: the first match answers. `program` is compared with the program's file name.
    pub fn on(self, program: &str, matches: impl Fn(&Call) -> bool + Send + Sync + 'static, reply: Reply) -> FakeRunner {
        self.rule(program, Box::new(matches), reply, false)
    }
    /// Like [`FakeRunner::on`], used up after one answer.
    pub fn once(self, program: &str, matches: impl Fn(&Call) -> bool + Send + Sync + 'static, reply: Reply) -> FakeRunner {
        self.rule(program, Box::new(matches), reply, true)
    }
    /// Answers every call of `program` that no rule matched.
    pub fn route(self, program: &str, handler: Arc<dyn Handler>) -> FakeRunner {
        self.script().routes.push((program.to_owned(), handler));
        self
    }
    /// Every call in order, answered or not.
    pub fn calls(&self) -> Vec<Call> {
        self.script().calls.clone()
    }
    /// The calls no rule matched.
    pub fn unscripted(&self) -> Vec<Call> {
        self.script().unscripted.clone()
    }

    /// Records `call` and finds its answer; the script's lock is not held while a handler runs.
    fn answer(&self, call: &Call) -> Option<Reply> {
        let name = file_name(&call.program);
        let handler = {
            let mut script = self.script();
            script.calls.push(call.clone());
            let hit = script.rules.iter_mut().find(|r| !r.used && file_name(&r.program) == name && (r.matches)(call));
            if let Some(rule) = hit {
                rule.used = rule.once;
                return Some(rule.reply.clone());
            }
            let route = script.routes.iter().find(|(program, _)| file_name(program) == name).map(|(_, h)| h.clone());
            if route.is_none() {
                script.unscripted.push(call.clone());
            }
            route
        };
        handler.map(|h| h.handle(call))
    }
}

impl Runner for FakeRunner {
    fn output(&self, cmd: &mut Command, tool: &str, deadline: Duration) -> Result<Output, Failed> {
        let program = cmd.get_program().to_string_lossy().into_owned();
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let envs = cmd
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned())))
            .collect();
        let call = Call {
            tool: tool.to_owned(),
            program,
            args,
            envs,
            cwd: cmd.get_current_dir().map(PathBuf::from),
            deadline,
        };
        match self.answer(&call) {
            None => Err(Failed::Io(ErrorKind::NotFound.into())),
            Some(Reply::Io(kind)) => Err(Failed::Io(kind.into())),
            Some(Reply::Exit { takes, .. }) if takes >= deadline => {
                self.clock.sleep(deadline);
                let argv = std::iter::once(call.program).chain(call.args).collect();
                Err(Failed::Timeout(deadline::timeout(tool, argv, deadline)))
            }
            Some(Reply::Exit { code, stdout, stderr, takes }) => {
                self.clock.sleep(takes);
                Ok(Output { status: ExitStatus::from_raw(code << 8), stdout, stderr })
            }
        }
    }
}
