//! `Pane` and `Wrapper`: `cpd session run` (the program of a Lead's pane) on a thread of the test process, on virtual
//! time, stepped one nap of its clock at a time (docs/implementing.md, Tests, "Virtual-time loops").
//!
//! `session run` loops on its clock: a poll, then a nap. [`Wrapper::step`] lets it run to its next nap and returns, so a
//! test does something (installs a file, changes the store, [`Fx::advance`]) between two polls and the wrapper sees it at
//! the next one. The two threads never run together: the wrapper runs while the test waits in `step`, the test runs
//! while the wrapper waits for the next `step`, so nothing is a race and no time is waited for.
//!
//! What a `Pane` stands for is the pane of the real thing, and what is outside this process is fake:
//! - pi is a pid of the [`PaneSpawn`] (a `FakeSpawn` that also logs the signals, can ignore `SIGTERM` and never returns
//!   from an `exec`, as the real one does not: the wrapper ends there as [`Ended::Exec`], and the test runs the launch
//!   again with [`Pane::resume`], which is the new process);
//! - the installed `cpd` is a file the test writes (never run); `<installed> --version` is a route of the `FakeRunner`
//!   that answers the real verdict of this env's store (`cpd::run_with` of `--version`) or what the test says;
//! - the pane's screen is a file the process's stdout and stderr point at while the `Pane` lives (what `print!` and
//!   `eprintln!` write in the wrapper), and its keyboard is a [`Keys`] (`cpd_core::host::input`): the wrapper that waits
//!   for Enter pauses there, as at a nap, and reads what the test typed with [`Wrapper::enter`] since, else the end of
//!   input (the User gave up). The screen is read from the descriptors, so it holds what the wrapper printed only where the harness does
//!   not capture `print!` itself: under nextest (`--nocapture`, a process per test), not under a plain `cargo test`.

use std::any::Any;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::os::fd::OwnedFd;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nix::unistd::{dup, dup2_stderr, dup2_stdin, dup2_stdout};

use cpd_core::clock::{Clock, ManualClock};
use cpd_core::error::CpdError;
use cpd_core::host::runner::{Call, Handler, Reply, Runner};
use cpd_core::host::spawn::{FakeSpawn, Launch, Pid, Signal, Spawn};
use cpd_core::ports::Ports;
use time::OffsetDateTime;

use super::fx::Fx;

/// The virtual time a wait of the wrapper's is followed for.
const LIMIT: Duration = Duration::from_secs(600);

/// What `exec` was asked for; the payload the wrapper's thread ends with, as a real `exec` never returns.
struct Exec(Launch);

/// The payload of a wrapper the test stopped (its `Wrapper` dropped).
struct Stopped;

/// The record of a [`PaneSpawn`].
#[derive(Default)]
struct Log {
    started: Vec<(Pid, Launch)>,
    signals: Vec<(Pid, Signal)>,
    execs: Vec<Launch>,
}

/// The `Spawn` of a pane: a `FakeSpawn` whose `exec` records and then does not return, whose signals are logged, and
/// whose pids can ignore `SIGTERM`.
pub struct PaneSpawn {
    inner: Arc<FakeSpawn>,
    log: Mutex<Log>,
    ignore_term: AtomicBool,
    term_exits: Mutex<Option<i32>>,
}

impl PaneSpawn {
    fn log(&self) -> MutexGuard<'_, Log> {
        self.log.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The pid and `Launch` of each process started (pi), in order.
    pub fn started(&self) -> Vec<(Pid, Launch)> {
        self.log().started.clone()
    }

    /// The pid of each process started (each pi), in order.
    pub fn pids(&self) -> Vec<Pid> {
        self.log().started.iter().map(|(pid, _)| *pid).collect()
    }

    /// The `Launch` of each process started (each pi), in order.
    pub fn pis(&self) -> Vec<Launch> {
        self.log()
            .started
            .iter()
            .map(|(_, launch)| launch.clone())
            .collect()
    }

    /// Every `(pid, signal)` the wrapper sent, in order, also the ones pi ignored.
    pub fn signals(&self) -> Vec<(Pid, Signal)> {
        self.log().signals.clone()
    }

    /// Every `Launch` of an `exec`, in order.
    pub fn execs(&self) -> Vec<Launch> {
        self.log().execs.clone()
    }

    /// From now on a pi ignores `SIGTERM`: the signal is logged and nothing happens to it, as a pi that traps it.
    pub fn ignore_term(&self) {
        self.ignore_term.store(true, Ordering::SeqCst);
    }

    /// From now on a pi exits with `code` at `SIGTERM`, as pi 1.2.0 does (`shutdown({fromSignal})` ends in
    /// `process.exit(0)`), instead of being ended by the signal.
    pub fn exit_on_term(&self, code: i32) {
        *self
            .term_exits
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(code);
    }

    /// Whether `pid` runs.
    pub fn alive(&self, pid: Pid) -> bool {
        self.inner.alive(pid)
    }

    /// Ends `pid` as `SIGKILL` would, from outside (the User, the OOM killer): not a signal of the wrapper's.
    pub fn killed(&self, pid: Pid) {
        self.inner
            .signal(pid, Signal::Kill)
            .expect("a pid the spawn started");
    }
}

impl Spawn for PaneSpawn {
    fn spawn(&self, l: &Launch) -> io::Result<Pid> {
        let pid = self.inner.spawn(l)?;
        self.log().started.push((pid, l.clone()));
        Ok(pid)
    }
    fn try_wait(&self, p: Pid) -> io::Result<Option<ExitStatus>> {
        self.inner.try_wait(p)
    }
    fn signal(&self, p: Pid, s: Signal) -> io::Result<()> {
        self.log().signals.push((p, s));
        if s == Signal::Term && self.ignore_term.load(Ordering::SeqCst) {
            return Ok(());
        }
        if s == Signal::Term
            && let Some(code) = *self
                .term_exits
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
        {
            if self.inner.alive(p) {
                self.inner.exit(p, code);
            }
            return Ok(());
        }
        self.inner.signal(p, s)
    }
    fn alive(&self, p: Pid) -> bool {
        self.inner.alive(p)
    }
    fn exe_of(&self, p: Pid) -> Option<PathBuf> {
        self.inner.exe_of(p)
    }
    fn cmdline(&self, p: Pid) -> Option<String> {
        self.inner.cmdline(p)
    }
    fn with_env(&self, key: &str, value: &str) -> Vec<Pid> {
        self.inner.with_env(key, value)
    }
    fn ppid(&self, p: Pid) -> Option<Pid> {
        self.inner.ppid(p)
    }
    fn env_of(&self, p: Pid, key: &str) -> Option<String> {
        self.inner.env_of(p, key)
    }
    fn descendants(&self, root: Pid) -> Vec<Pid> {
        self.inner.descendants(root)
    }
    fn exec(&self, l: &Launch) -> io::Error {
        let _ = self.inner.exec(l);
        self.log().execs.push(l.clone());
        resume_unwind(Box::new(Exec(l.clone())))
    }
    fn stream(&self, l: &Launch) -> io::Result<(Pid, Box<dyn BufRead + Send>)> {
        self.inner.stream(l)
    }
    fn self_exe(&self) -> Result<PathBuf, CpdError> {
        self.inner.self_exe()
    }
}

/// What `<installed> --version` answers.
enum Verdict {
    /// The real verdict: `cpd --version` run in this process on this env's store.
    Real,
    /// This JSON line.
    Says(String),
}

/// The route of the installed `cpd`'s program name: it answers `--version` only.
struct VersionRoute {
    ports: Ports,
    verdict: Mutex<Verdict>,
}

impl Handler for VersionRoute {
    fn handle(&self, call: &Call) -> Reply {
        assert_eq!(
            call.args,
            ["--version"],
            "the wrapper asks the installed cpd for its version, nothing else"
        );
        match &*self.verdict.lock().unwrap_or_else(PoisonError::into_inner) {
            Verdict::Says(json) => Reply::ok(json),
            Verdict::Real => {
                let mut out = Vec::new();
                let code = cpd::run_with(
                    &self.ports,
                    ["cpd", "--version"].map(OsString::from),
                    &mut out,
                );
                Reply::exit(exit_code(code), &String::from_utf8_lossy(&out), "")
            }
        }
    }
}

/// The path a wrapper runs from in `fx`'s env: an empty file `installed-bin/cpd` that is never run, made the first time
/// (a wrapper that finds it written again takes it for a new install and reloads).
pub fn installed_cpd(fx: &Fx) -> PathBuf {
    let installed = fx.env.root().join("installed-bin/cpd");
    if !installed.exists() {
        std::fs::create_dir_all(installed.parent().expect("a parent"))
            .expect("create installed-bin");
        std::fs::write(&installed, "").expect("write the installed cpd");
    }
    installed
}

/// The pane of a Lead's wrapper: its screen and its `Spawn`, the installed `cpd`'s verdict and the process's stdin.
pub struct Pane {
    /// The pids and signals of the processes the wrapper starts and signals.
    pub spawn: Arc<PaneSpawn>,
    /// The tmux pane id (`%n`): `TMUX_PANE` of the wrapper.
    pub pane: String,
    clock: Arc<ManualClock>,
    runner: Arc<dyn Runner>,
    screen: PathBuf,
    verdict: Arc<VersionRoute>,
    /// What stdin, stdout and stderr were, put back on drop.
    saved: [OwnedFd; 3],
}

impl Pane {
    /// A pane for `session`, in the cockpit of `fx` (a new window whose `@cpd_session` is `session`, as the command that
    /// creates a Lead's pane sets it); `installed` is the path the wrapper runs from (`FakeSpawn::self_exe`).
    /// Exports `TMUX` and `TMUX_PANE` for the wrapper's `wrong_pane` check until `fx` drops. The stdin and the output of the process
    /// point at `/dev/null` and the screen file until the `Pane` drops.
    pub fn new(fx: &Fx, session: &str, installed: &Path) -> Pane {
        if fx.tmux_state().is_none() {
            fx.cockpit();
        }
        let command = format!("{} session run {session}", installed.display());
        let out = fx.tmux(&[
            "new-window",
            "-d",
            "-t",
            "=cpd:",
            "-P",
            "-F",
            "#{pane_id}",
            &command,
        ]);
        assert!(
            out.status.success(),
            "tmux new-window: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let pane = String::from_utf8(out.stdout)
            .expect("tmux's stdout is UTF-8")
            .trim()
            .to_owned();
        let set = fx.tmux(&["set", "-p", "-t", &pane, "@cpd_session", session]);
        assert!(
            set.status.success(),
            "tmux set: {}",
            String::from_utf8_lossy(&set.stderr)
        );
        Pane::at(fx, &pane, installed)
    }

    /// The pane `pane` of the cockpit of `fx`, which a command already made (`session open`, `session fork`): as
    /// [`Pane::new`] for the rest.
    pub fn at(fx: &Fx, pane: &str, installed: &Path) -> Pane {
        let pane = pane.to_owned();
        fx.export(
            "TMUX",
            &format!(
                "{}/tmux-0/{},1,0",
                fx.env.tmux_tmpdir().display(),
                fx.env.tmux_socket()
            ),
        );
        fx.export("TMUX_PANE", &pane);
        fx.fakes.spawn.set_self_exe(installed);

        let spawn = Arc::new(PaneSpawn {
            inner: fx.fakes.spawn.clone(),
            log: Mutex::default(),
            ignore_term: AtomicBool::new(false),
            term_exits: Mutex::new(None),
        });
        let runner = fx.fakes.runner.weak();
        let ports = Ports {
            clock: fx.fakes.clock.clone(),
            runner: runner.clone(),
            spawn: fx.fakes.spawn.clone(),
            ..Ports::real()
        };
        let verdict = Arc::new(VersionRoute {
            ports,
            verdict: Mutex::new(Verdict::Real),
        });
        let program = installed
            .file_name()
            .expect("the installed cpd has a name")
            .to_string_lossy()
            .into_owned();
        let _ = (*fx.fakes.runner).clone().route(&program, verdict.clone());

        let screen = fx.env.root().join("screen");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&screen)
            .expect("open the screen file");
        let null = OpenOptions::new()
            .read(true)
            .open("/dev/null")
            .expect("open /dev/null");
        let saved = [
            dup(io::stdin()).expect("dup stdin"),
            dup(io::stdout()).expect("dup stdout"),
            dup(io::stderr()).expect("dup stderr"),
        ];
        dup2_stdin(&null).expect("point stdin at /dev/null");
        dup2_stdout(&file).expect("point stdout at the screen");
        dup2_stderr(&file).expect("point stderr at the screen");
        Pane {
            spawn,
            pane,
            clock: fx.fakes.clock.clone(),
            runner,
            screen,
            verdict,
            saved,
        }
    }

    /// From now on `<installed> --version` answers `json`, a line of what `cpd --version` prints.
    pub fn version_says(&self, json: &str) {
        *self
            .verdict
            .verdict
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Verdict::Says(json.to_owned());
    }

    /// From now on `<installed> --version` answers what the built cpd answers on this env's store.
    pub fn version_is_real(&self) {
        *self
            .verdict
            .verdict
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Verdict::Real;
    }

    /// What the pane shows: what the wrapper printed on stdout and stderr, and what it ended with.
    pub fn screen(&self) -> String {
        let _ = io::stdout().flush();
        std::fs::read_to_string(&self.screen).unwrap_or_default()
    }

    /// Starts `cpd <args>` (a `session run`) as the pane's process; it runs to its first nap at the first `step`. Its
    /// `TMUX_PANE` is this pane's at each `step`.
    pub fn run(&self, args: &[&str]) -> Wrapper {
        let argv: Vec<OsString> = std::iter::once("cpd")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect();
        let (nap_tx, nap_rx) = channel();
        let (go_tx, go_rx) = channel();
        let stepped = Arc::new(Stepped {
            inner: self.clock.clone(),
            nap: Mutex::new(nap_tx.clone()),
            go: Mutex::new(go_rx),
        });
        let typed = Arc::new(Mutex::new(Vec::new()));
        let keys = Keys {
            stepped: stepped.clone(),
            typed: typed.clone(),
            buffer: Vec::new(),
            at: 0,
        };
        let ports = Ports {
            clock: stepped.clone(),
            runner: self.runner.clone(),
            spawn: self.spawn.clone(),
            ..Ports::real()
        };
        let screen = self.screen.clone();
        let thread = std::thread::spawn(move || {
            // The first `step` starts it; a `Wrapper` dropped before that never runs it.
            if !stepped.wait_for_step() {
                return;
            }
            let mut out = Vec::new();
            let run = || {
                cpd_core::host::input::scope(Box::new(keys), || {
                    cpd::run_with(&ports, argv, &mut out)
                })
            };
            let ended = match catch_unwind(AssertUnwindSafe(run)) {
                Ok(code) => Ended::Returned {
                    code: exit_code(code),
                    stdout: String::from_utf8_lossy(&out).into_owned(),
                },
                Err(payload) => match payload.downcast::<Exec>() {
                    Ok(exec) => Ended::Exec(exec.0),
                    Err(payload) => match payload.downcast::<Stopped>() {
                        Ok(_) => Ended::Stopped,
                        Err(payload) => Ended::Panicked(Mutex::new(Some(payload))),
                    },
                },
            };
            if let Ended::Returned { stdout, .. } = &ended {
                let _ = OpenOptions::new()
                    .append(true)
                    .open(&screen)
                    .and_then(|mut file| file.write_all(stdout.as_bytes()));
            }
            let _ = nap_tx.send(Event::Done(ended));
        });
        Wrapper {
            go: Some(go_tx),
            events: nap_rx,
            thread: Some(thread),
            ended: None,
            clock: self.clock.clone(),
            pane: self.pane.clone(),
            typed,
        }
    }

    /// The new process of an `exec` the wrapper asked for: `cpd` with the launch's arguments, which the old one
    /// replaced itself with.
    pub fn resume(&self, launch: &Launch) -> Wrapper {
        let args: Vec<&str> = launch.args.iter().map(String::as_str).collect();
        self.run(&args)
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        let [stdin, stdout, stderr] = &self.saved;
        let _ = dup2_stdin(stdin);
        let _ = dup2_stdout(stdout);
        let _ = dup2_stderr(stderr);
        // A failed test's own message went to the screen file with the rest of stderr: say it where it is read.
        if std::thread::panicking() {
            eprintln!(
                "--- the pane's screen (the wrapper's output and the test's panic message) ---\n{}",
                self.screen()
            );
        }
    }
}

/// How a wrapper ended.
pub enum Ended {
    /// `run_with` returned: the exit code and what it wrote to its writer (the result or the error JSON).
    Returned { code: i32, stdout: String },
    /// It asked the `Spawn` to `exec` this: the process was replaced.
    Exec(Launch),
    /// The test dropped it.
    Stopped,
    /// It panicked.
    Panicked(Mutex<Option<Box<dyn Any + Send>>>),
}

impl std::fmt::Debug for Ended {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ended::Returned { code, stdout } => write!(f, "returned {code}: {stdout}"),
            Ended::Exec(launch) => write!(f, "exec {launch:?}"),
            Ended::Stopped => write!(f, "stopped"),
            Ended::Panicked(_) => write!(f, "panicked"),
        }
    }
}

enum Event {
    Napped,
    Done(Ended),
}

/// The wrapper's clock: the `ManualClock`, whose `sleep` moves it forward and then waits for the test's next `step`.
struct Stepped {
    inner: Arc<ManualClock>,
    nap: Mutex<Sender<Event>>,
    go: Mutex<Receiver<()>>,
}

impl Stepped {
    /// Waits for the test's next `step`; false when the `Wrapper` is gone.
    fn wait_for_step(&self) -> bool {
        self.go
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv()
            .is_ok()
    }
}

impl Stepped {
    /// The wrapper stops here until the test's next `step`; unwinds when the `Wrapper` is gone.
    fn pause(&self) {
        let _ = self
            .nap
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(Event::Napped);
        if !self.wait_for_step() {
            resume_unwind(Box::new(Stopped));
        }
    }
}

impl Clock for Stepped {
    fn now(&self) -> Instant {
        self.inner.now()
    }
    fn sleep(&self, d: Duration) {
        self.inner.sleep(d);
        self.pause();
    }
    fn utc(&self) -> OffsetDateTime {
        self.inner.utc()
    }
}

/// The keyboard of a wrapper: when it asks for a line it pauses, as at a nap (no time passes), so the test may type
/// ([`Wrapper::enter`]) after it sees what the wrapper printed; what was typed by the next `step` is the line, and
/// nothing typed is the end of input.
struct Keys {
    stepped: Arc<Stepped>,
    typed: Arc<Mutex<Vec<u8>>>,
    buffer: Vec<u8>,
    at: usize,
}

impl io::Read for Keys {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.fill_buf()?.read(out)?;
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for Keys {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.at >= self.buffer.len() {
            self.stepped.pause();
            self.buffer =
                std::mem::take(&mut *self.typed.lock().unwrap_or_else(PoisonError::into_inner));
            self.at = 0;
        }
        Ok(&self.buffer[self.at..])
    }

    fn consume(&mut self, n: usize) {
        self.at += n;
    }
}

/// A running `session run`, stepped by the test.
pub struct Wrapper {
    go: Option<Sender<()>>,
    events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
    ended: Option<Ended>,
    clock: Arc<ManualClock>,
    /// The tmux pane id: `TMUX_PANE` while it runs.
    pane: String,
    typed: Arc<Mutex<Vec<u8>>>,
}

impl Wrapper {
    /// Types Enter: the wrapper that waits for a line at its next `step` (it paused at the prompt) reads it.
    pub fn enter(&self) {
        self.typed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(b'\n');
    }

    /// Lets the wrapper run to its next nap (the clock has moved by it) or to its end. Returns whether it still runs.
    /// A wrapper that panicked panics here.
    pub fn step(&mut self) -> bool {
        if self.ended.is_some() {
            return false;
        }
        // SAFETY: under the `Fx`'s lock; the wrapper's thread is parked in a wait for this step, and no other runs.
        unsafe { std::env::set_var("TMUX_PANE", &self.pane) };
        self.go
            .as_ref()
            .expect("a running wrapper")
            .send(())
            .expect("the wrapper waits for its step");
        match self.events.recv().expect("the wrapper ends with an event") {
            Event::Napped => true,
            Event::Done(Ended::Panicked(payload)) => {
                self.join();
                resume_unwind(
                    payload
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take()
                        .expect("a payload"),
                )
            }
            Event::Done(ended) => {
                self.ended = Some(ended);
                self.join();
                false
            }
        }
    }

    /// `n` steps; panics when the wrapper ends before them.
    pub fn steps(&mut self, n: usize) {
        for i in 0..n {
            assert!(
                self.step(),
                "the wrapper ended at step {i} of {n}: {:?}",
                self.ended
            );
        }
    }

    /// Steps until `cond` holds (looked at before the first step too); panics when the wrapper ends without it, or when
    /// [`LIMIT`] of virtual time passes.
    pub fn until(&mut self, what: &str, mut cond: impl FnMut() -> bool) {
        let start = self.clock.elapsed();
        while !cond() {
            // A step that ends the wrapper may be the one that made `cond` true.
            if !self.step() {
                assert!(cond(), "the wrapper ended ({:?}) before {what}", self.ended);
                return;
            }
            assert!(
                self.clock.elapsed() - start < LIMIT,
                "waited {LIMIT:?} of virtual time for {what}"
            );
        }
    }

    /// Steps until the wrapper ends, and says how.
    pub fn finish(&mut self) -> &Ended {
        let start = self.clock.elapsed();
        while self.step() {
            assert!(
                self.clock.elapsed() - start < LIMIT,
                "the wrapper did not end in {LIMIT:?} of virtual time"
            );
        }
        self.ended.as_ref().expect("ended")
    }

    /// How it ended, once it has.
    pub fn ended(&self) -> Option<&Ended> {
        self.ended.as_ref()
    }

    /// The `Launch` of the `exec` it ended with; panics when it ended otherwise or runs.
    pub fn exec(&self) -> &Launch {
        match &self.ended {
            Some(Ended::Exec(launch)) => launch,
            other => panic!("the wrapper did not end in an exec: {other:?}"),
        }
    }

    fn join(&mut self) {
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .expect("the wrapper's thread ended by its event");
        }
    }
}

impl Drop for Wrapper {
    fn drop(&mut self) {
        // The thread wakes from its wait with an error and unwinds out of the wrapper.
        drop(self.go.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn exit_code(code: ExitCode) -> i32 {
    (0..=u8::MAX)
        .find(|n| ExitCode::from(*n) == code)
        .map(i32::from)
        .expect("an ExitCode is a byte")
}
