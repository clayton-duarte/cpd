//! The process seam: [`Runner`] runs one `Command` under a deadline, like [`deadline::output`].
//! [`SystemRunner`] is the real one; [`FakeRunner`] answers from a script, in-process, on virtual time.

use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::ExitStatus;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use super::deadline::{self, Failed};
use crate::clock::{Clock, ManualClock};

/// The `Call.deadline` of a process that has none (pi in a retitle, `cpd watch --ensure`, `date`): a `Duration` the real runner never reaches.
pub const NO_DEADLINE: Duration = Duration::from_secs(60 * 60 * 24 * 365 * 100);

/// The `Call.tool` of a Git hook (`git::hooks::exec`): the `SystemRunner` runs it at idle priority (hs-d528).
pub const HOOK_TOOL: &str = "hook";

pub trait Runner: Send + Sync {
    /// Runs `cmd` for at most `deadline` and returns what [`deadline::output`] returns: the same `Output`,
    /// `Failed::Io` or `Failed::Timeout(tool_timeout)`.
    fn output(&self, cmd: &mut Command, tool: &str, deadline: Duration) -> Result<Output, Failed>;

    /// Runs `cmd` with no deadline, in its own process group, like [`Command::output`]: for a call that returns when
    /// the User says so (`tmux display-popup`, contract §6.3).
    fn output_unbounded(&self, cmd: &mut Command, tool: &str) -> std::io::Result<Output>;

    /// Runs what `call` describes: like [`Runner::output`], and also writes `call.stdin` to the child's stdin
    /// (then closes it) and sends the child's stderr to the file `call.stderr_to`, appending.
    fn run(&self, call: &Call) -> Result<Output, Failed>;

    /// Which implementation this is, by name, for a test of `Ports` (`"SystemRunner"` for the real one).
    fn kind(&self) -> &'static str {
        "other"
    }
}

/// The `Command` that `call` describes: its program, arguments, environment (a `None` value removes the variable)
/// and working directory. Stdin and stderr are wired by the runner, not here.
pub fn command(call: &Call) -> Command {
    let mut cmd = Command::new(&call.program);
    cmd.args(&call.args);
    for (key, value) in &call.envs {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
    if let Some(cwd) = &call.cwd {
        cmd.current_dir(cwd);
    }
    cmd
}

/// The real runner: [`deadline::output`], plus a thread that writes `Call.stdin` and a file for `Call.stderr_to`.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn kind(&self) -> &'static str {
        "SystemRunner"
    }
    fn output(&self, cmd: &mut Command, tool: &str, deadline: Duration) -> Result<Output, Failed> {
        deadline::output(cmd, tool, deadline)
    }
    fn output_unbounded(&self, cmd: &mut Command, _tool: &str) -> std::io::Result<Output> {
        cmd.process_group(0).output()
    }
    fn run(&self, call: &Call) -> Result<Output, Failed> {
        let mut cmd = system_command(call);
        if let Some(path) = &call.stderr_to {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(Failed::Io)?;
            cmd.stderr(file);
        }
        deadline::output_fed(
            &mut cmd,
            &call.tool,
            call.deadline,
            call.stdin.clone(),
            call.stderr_to.is_some(),
            call.tool == HOOK_TOOL,
        )
    }
}

/// The `Command` [`SystemRunner`] starts for `call`: [`command`], and a Git hook (`HOOK_TOOL`) behind the idle priority prefix.
pub fn system_command(call: &Call) -> Command {
    if call.tool == HOOK_TOOL {
        command(&idle(call))
    } else {
        command(call)
    }
}

/// `call` behind the idle priority prefix (hs-d528): its environment and working directory are the same.
fn idle(call: &Call) -> Call {
    let prefix = super::priority::idle_prefix();
    let args = prefix[1..]
        .iter()
        .map(|s| (*s).to_owned())
        .chain([call.program.clone()])
        .chain(call.args.clone())
        .collect();
    Call {
        program: prefix[0].to_owned(),
        args,
        ..call.clone()
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
    /// Bytes written to the child's stdin, which is then closed; `None` is a null stdin.
    pub stdin: Option<Vec<u8>>,
    /// The file the child's stderr is appended to; `None` is captured in the `Output`.
    pub stderr_to: Option<PathBuf>,
}

/// How a scripted call ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Exit {
        code: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        takes: Duration,
    },
    /// The program ended on the signal `signal` (no exit code): `ExitStatus::signal()` says which.
    Signal {
        signal: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    Io(ErrorKind),
}

impl Reply {
    /// Exit 0 with `stdout`.
    pub fn ok(stdout: &str) -> Reply {
        Reply::exit(0, stdout, "")
    }
    pub fn exit(code: i32, stdout: &str, stderr: &str) -> Reply {
        Reply::Exit {
            code,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
            takes: Duration::ZERO,
        }
    }
    /// The program was killed by `signal` after printing `stdout` and `stderr`.
    pub fn signal(signal: i32, stdout: &str, stderr: &str) -> Reply {
        Reply::Signal {
            signal,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }
    /// The program could not be started: `Failed::Io(kind)`.
    pub fn io(kind: ErrorKind) -> Reply {
        Reply::Io(kind)
    }
    /// The reply takes `d` of virtual time.
    pub fn takes(self, d: Duration) -> Reply {
        match self {
            Reply::Exit {
                code,
                stdout,
                stderr,
                ..
            } => Reply::Exit {
                code,
                stdout,
                stderr,
                takes: d,
            },
            other => other,
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

/// A [`FakeRunner`] held weakly: for a route that lives in the runner's own script and calls it back.
struct WeakRunner {
    clock: ManualClock,
    script: Weak<Mutex<Script>>,
}

impl WeakRunner {
    fn upgrade(&self) -> Option<FakeRunner> {
        self.script.upgrade().map(|script| FakeRunner {
            clock: self.clock.clone(),
            script,
        })
    }
}

impl Runner for WeakRunner {
    fn output(&self, cmd: &mut Command, tool: &str, deadline: Duration) -> Result<Output, Failed> {
        match self.upgrade() {
            Some(runner) => runner.output(cmd, tool, deadline),
            None => Err(Failed::Io(ErrorKind::NotFound.into())),
        }
    }
    fn output_unbounded(&self, cmd: &mut Command, tool: &str) -> std::io::Result<Output> {
        match self.upgrade() {
            Some(runner) => runner.output_unbounded(cmd, tool),
            None => Err(ErrorKind::NotFound.into()),
        }
    }
    fn run(&self, call: &Call) -> Result<Output, Failed> {
        match self.upgrade() {
            Some(runner) => runner.run(call),
            None => Err(Failed::Io(ErrorKind::NotFound.into())),
        }
    }
}

/// The program's file name: `/x/bin/tmux` is `tmux`.
fn file_name(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

impl FakeRunner {
    /// A fake with no rule; `Reply::takes` passes virtual time on `clock`.
    pub fn new(clock: ManualClock) -> FakeRunner {
        FakeRunner {
            clock,
            script: Arc::default(),
        }
    }
    fn script(&self) -> MutexGuard<'_, Script> {
        self.script
            .lock()
            .expect("the script is never held across a panic")
    }
    fn rule(self, program: &str, matches: Matches, reply: Reply, once: bool) -> FakeRunner {
        self.script().rules.push(Rule {
            program: program.to_owned(),
            matches,
            reply,
            once,
            used: false,
        });
        self
    }
    /// A rule: the first match answers. `program` is compared with the program's file name.
    pub fn on(
        self,
        program: &str,
        matches: impl Fn(&Call) -> bool + Send + Sync + 'static,
        reply: Reply,
    ) -> FakeRunner {
        self.rule(program, Box::new(matches), reply, false)
    }
    /// Like [`FakeRunner::on`], used up after one answer.
    pub fn once(
        self,
        program: &str,
        matches: impl Fn(&Call) -> bool + Send + Sync + 'static,
        reply: Reply,
    ) -> FakeRunner {
        self.rule(program, Box::new(matches), reply, true)
    }
    /// Answers every call of `program` that no rule matched.
    pub fn route(self, program: &str, handler: Arc<dyn Handler>) -> FakeRunner {
        self.script().routes.push((program.to_owned(), handler));
        self
    }
    /// This runner as a [`Runner`] that does not keep the script alive.
    pub fn weak(&self) -> Arc<dyn Runner> {
        Arc::new(WeakRunner {
            clock: self.clock.clone(),
            script: Arc::downgrade(&self.script),
        })
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
            let hit = script
                .rules
                .iter_mut()
                .find(|r| !r.used && file_name(&r.program) == name && (r.matches)(call));
            if let Some(rule) = hit {
                rule.used = rule.once;
                return Some(rule.reply.clone());
            }
            let route = script
                .routes
                .iter()
                .find(|(program, _)| file_name(program) == name)
                .map(|(_, h)| h.clone());
            if route.is_none() {
                script.unscripted.push(call.clone());
            }
            route
        };
        handler.map(|h| h.handle(call))
    }
}

impl FakeRunner {
    /// Answers `call` like [`deadline::output`]: its `Output`, `Failed::Io` or `Failed::Timeout`, on virtual time.
    fn reply(&self, call: Call) -> Result<Output, Failed> {
        let deadline = call.deadline;
        match self.answer(&call) {
            None => Err(Failed::Io(ErrorKind::NotFound.into())),
            Some(Reply::Io(kind)) => Err(Failed::Io(kind.into())),
            Some(Reply::Signal {
                signal,
                stdout,
                stderr,
            }) => Ok(Output {
                status: ExitStatus::from_raw(signal),
                stdout,
                stderr,
            }),
            Some(Reply::Exit { takes, .. }) if takes >= deadline => {
                self.clock.sleep(deadline);
                let argv = std::iter::once(call.program).chain(call.args).collect();
                Err(Failed::Timeout(deadline::timeout(
                    &call.tool, argv, deadline,
                )))
            }
            Some(Reply::Exit {
                code,
                stdout,
                stderr,
                takes,
            }) => {
                // A reply that takes no time is no nap: `ManualClock::naps` lists only what a caller waited on.
                if !takes.is_zero() {
                    self.clock.sleep(takes);
                }
                Ok(Output {
                    status: ExitStatus::from_raw(code << 8),
                    stdout,
                    stderr,
                })
            }
        }
    }
}

impl Runner for FakeRunner {
    fn output(&self, cmd: &mut Command, tool: &str, deadline: Duration) -> Result<Output, Failed> {
        let program = cmd.get_program().to_string_lossy().into_owned();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let envs = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        self.reply(Call {
            tool: tool.to_owned(),
            program,
            args,
            envs,
            cwd: cmd.get_current_dir().map(PathBuf::from),
            deadline,
            stdin: None,
            stderr_to: None,
        })
    }
    fn run(&self, call: &Call) -> Result<Output, Failed> {
        self.reply(call.clone())
    }
    /// No deadline: the call is recorded with the longest one, which no reply reaches.
    fn output_unbounded(&self, cmd: &mut Command, tool: &str) -> std::io::Result<Output> {
        match self.output(cmd, tool, Duration::MAX) {
            Ok(out) => Ok(out),
            Err(Failed::Io(e)) => Err(e),
            Err(Failed::Timeout(e)) => Err(std::io::Error::other(e.message().to_owned())),
        }
    }
}
