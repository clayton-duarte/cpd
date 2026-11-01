//! `cpd events follow` in the test process, on the fake ports of an [`Fx`] (docs/implementing.md, Tests): `events::run`
//! prints on the process's stdout and naps on the system clock, which a test can neither read nor drive, so these call
//! `cpd_core::events::follow_loop` with a writer of the test's and a clock of the test's. Include it with
//! `#[path = "../common/follower.rs"] mod follower;`.
//!
//! - [`follow_once`] is `fx.cpd(["events", "follow", ...])` for a command that ends by itself (`--until-idle`, a refusal):
//!   its `Run` has the stdout, the exit status and the stderr of the call.
//! - [`Follower`] is a follower that does not end by itself: it runs on a thread of its own and takes its poll naps on a
//!   clock that hands each nap to the test, so the test decides when it polls again (`next`, `exit`). The thread and
//!   the test pass messages over channels, never time: nothing sleeps, nothing is polled, and the follower polls only
//!   after the test said so.
#![allow(dead_code)] // Each test file that includes it uses a subset of it.

use std::ffi::OsString;
use std::io::{self, Seek, SeekFrom, Write};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use clap::Parser;
use cpd::cli::{Cli, Command};
use cpd_core::clock::{Clock, ManualClock};
use cpd_core::env::Env;
use cpd_core::error::CpdError;
use cpd_core::events::{self, FollowArgs};
use cpd_core::output::{Output, emit};
use cpd_core::ports::Ports;
use serde_json::Value;
use time::OffsetDateTime;

use crate::common::fx::{Fx, exit_code};
use crate::common::Run;

/// The `FollowArgs` of `cpd events follow <words>`, or the failure of parsing them (`usage`, `invalid_plan_id`, ...).
fn parse(words: &[&str]) -> Result<FollowArgs, CpdError> {
    let argv = ["cpd", "events", "follow"].into_iter().chain(words.iter().copied());
    match Cli::try_parse_from(argv) {
        Ok(Cli { command: Some(Command::Events(events::Command::Follow(args))) }) => Ok(args),
        Ok(other) => panic!("`cpd events follow {words:?}` parsed as {other:?}"),
        Err(err) => Err(cpd::cli::clap_result(err).expect_err("a parse failure is a refusal")),
    }
}

/// Runs `cpd events follow <words>` to its end on `ports`, in the process's environment and directory (those of `fx`),
/// as `Fx::cpd` runs a command: stdout is what it printed, stderr what it said on the process's descriptor 2 meanwhile
/// (read back from a temporary file: nextest runs a test with `--nocapture`, so `eprintln!` reaches the descriptor).
/// A command that does not end by itself never returns: use [`Follower`].
pub fn follow_once_on(ports: &Ports, words: &[&str]) -> Run {
    let mut stdout = Vec::new();
    let (code, stderr) = capturing_stderr(|| {
        let result = parse(words).and_then(|args| {
            let mut env = Env::open_with(ports)?;
            events::follow_in(&mut env, args, &mut stdout)
        });
        emit(result, &mut stdout)
    });
    Run { exit: exit_code(code), stdout: String::from_utf8(stdout).expect("stdout is UTF-8"), stderr }
}

/// [`follow_once_on`] the fake ports of `fx`.
pub fn follow_once(fx: &Fx, words: &[&str]) -> Run {
    follow_once_on(&fx.fakes.ports, words)
}

/// Runs `f` with descriptor 2 pointing at a temporary file; returns what it returned and what the file got. The
/// descriptor is put back after, also when `f` panics.
fn capturing_stderr<T>(f: impl FnOnce() -> T) -> (T, String) {
    use nix::unistd::{dup, dup2_stderr};
    struct Back(std::os::fd::OwnedFd);
    impl Drop for Back {
        fn drop(&mut self) {
            let _ = dup2_stderr(&self.0);
        }
    }
    let mut file = tempfile::tempfile().expect("a temporary file");
    let saved = dup(io::stderr()).expect("dup stderr");
    dup2_stderr(&file).expect("point stderr at the file");
    let back = Back(saved);
    let value = f();
    drop(back);
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut text = String::new();
    io::Read::read_to_string(&mut file, &mut text).unwrap();
    (value, text)
}

// ------------------------------------------------------------------------------------------------ a running follower

/// What a follower tells its test.
enum Message {
    /// A line it wrote.
    Line(String),
    /// It is about to nap and waits to be told to go on.
    Napped,
    /// It ended with this exit status.
    Ended(i32),
}

/// Naps on virtual time and hands each nap to the test: `sleep` says `Napped` and waits for the test's go. When the
/// test is gone (its `Follower` dropped) the wait ends with an unwind that carries [`Stopped`], so the thread ends.
struct Napper {
    time: ManualClock,
    said: Mutex<Sender<Message>>,
    go: Mutex<Receiver<()>>,
}

/// The payload of the unwind that ends a follower whose test is gone: not a failure.
struct Stopped;

impl Clock for Napper {
    fn now(&self) -> Instant {
        self.time.now()
    }
    fn sleep(&self, d: Duration) {
        self.time.sleep(d);
        if self.said.lock().unwrap().send(Message::Napped).is_err() {
            resume_unwind(Box::new(Stopped));
        }
        if self.go.lock().unwrap().recv().is_err() {
            resume_unwind(Box::new(Stopped));
        }
    }
    fn utc(&self) -> OffsetDateTime {
        self.time.utc()
    }
}

/// The follower's stdout: each line it writes goes to the test; after [`Follower::close_stdout`] a write fails with
/// `BrokenPipe`, as a closed pipe's does, and the follower finds its reader gone with no write (the answer of
/// `reader_gone` on a real pipe is tested in `core/src/events/mod.rs`).
struct Pipe {
    said: Sender<Message>,
    closed: Arc<AtomicBool>,
}

impl Write for Pipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        for line in String::from_utf8_lossy(bytes).split_inclusive('\n') {
            self.said.send(Message::Line(line.to_owned())).map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// How many naps `next` and `exit` let pass before they give up: the follower polls every `poll_ms` of virtual time, so
/// this is a bound on a hang, not a wait.
const NAPS: usize = 200;

/// A `cpd events follow` on a thread of its own, polling on the test's say-so.
pub struct Follower {
    said: Receiver<Message>,
    go: Option<Sender<()>>,
    closed: Arc<AtomicBool>,
    /// Lines the follower wrote before the test asked.
    lines: std::collections::VecDeque<Value>,
    ended: Option<i32>,
    thread: Option<JoinHandle<()>>,
}

impl Follower {
    /// Starts `cpd events follow <words>` on the fake ports of `fx` (a clock that is the follower's own) and returns
    /// once it has done its first poll and is about to nap: what it has written so far is in the queue, and what the
    /// store held when it started is what "before it started" means.
    pub fn start(fx: &Fx, words: &[&str]) -> Follower {
        let argv: Vec<OsString> = ["events", "follow"].into_iter().chain(words.iter().copied()).map(OsString::from).collect();
        let args = parse(words).unwrap_or_else(|e| panic!("`cpd events follow {words:?}`: {e:?}"));
        let (said_tx, said) = channel();
        let (go, go_rx) = channel();
        let closed = Arc::new(AtomicBool::new(false));
        let ports = Ports {
            clock: Arc::new(Napper { time: (*fx.fakes.clock).clone(), said: Mutex::new(said_tx.clone()), go: Mutex::new(go_rx) }),
            runner: fx.fakes.runner.clone(),
            spawn: fx.fakes.spawn.clone(),
            ..Ports::real()
        };
        let pipe_closed = Arc::clone(&closed);
        let thread = std::thread::spawn(move || {
            let mut pipe = Pipe { said: said_tx.clone(), closed: Arc::clone(&pipe_closed) };
            let ended = catch_unwind(AssertUnwindSafe(|| {
                let result: Result<Output, CpdError> = Env::open_with(&ports)
                    .and_then(|mut env| events::follow_loop(&mut env, args, &mut pipe, &|| pipe_closed.load(Ordering::SeqCst), &argv));
                exit_code(emit(result, &mut pipe))
            }));
            match ended {
                Ok(code) => {
                    let _ = said_tx.send(Message::Ended(code));
                }
                Err(payload) if payload.is::<Stopped>() => {}
                Err(payload) => resume_unwind(payload),
            }
        });
        let mut follower = Follower { said, go: Some(go), closed, lines: Default::default(), ended: None, thread: Some(thread) };
        follower.until(|m| matches!(m, Message::Napped));
        follower
    }

    /// Starts the follower that `launch` (the restart a follower recorded on the `FakeSpawn`) describes: its arguments
    /// after the program, as `cpd` would parse them.
    pub fn start_launch(fx: &Fx, launch: &cpd_core::host::spawn::Launch) -> Follower {
        let words: Vec<&str> = launch.args.iter().map(String::as_str).collect();
        assert_eq!(words[..2], ["events", "follow"], "{launch:?}");
        Follower::start(fx, &words[2..])
    }

    /// Reads messages until `stop` says one is the one wanted (it is consumed), queueing the lines before it. An end of
    /// the follower is recorded and also stops the read. When the follower's thread panicked, so does this.
    fn until(&mut self, stop: impl Fn(&Message) -> bool) {
        loop {
            match self.said.recv() {
                Ok(Message::Line(line)) => self.lines.push_back(serde_json::from_str(&line).unwrap_or_else(|e| panic!("{line:?}: {e}"))),
                Ok(Message::Ended(code)) => {
                    self.ended = Some(code);
                    return;
                }
                Ok(message) if stop(&message) => return,
                Ok(_) => {}
                Err(_) => {
                    let thread = self.thread.take().expect("the follower's thread");
                    match thread.join() {
                        Err(payload) => resume_unwind(payload),
                        Ok(()) => panic!("the follower's thread ended without an exit status"),
                    }
                }
            }
        }
    }

    /// The next line it wrote, letting it poll again (a nap of virtual time) while it has none; `None` when it ended.
    pub fn next(&mut self) -> Option<Value> {
        for _ in 0..NAPS {
            if let Some(line) = self.lines.pop_front() {
                return Some(line);
            }
            if self.ended.is_some() {
                return None;
            }
            self.go_on();
        }
        panic!("the follower wrote no line in {NAPS} polls")
    }

    /// Lets the follower poll once more: it ran from its nap to the next one (or its end).
    fn go_on(&mut self) {
        self.go.as_ref().expect("the follower is running").send(()).expect("the follower's thread is waiting");
        self.until(|m| matches!(m, Message::Napped));
    }

    /// From now on a write of the follower fails with `BrokenPipe`: its reader went away.
    pub fn close_stdout(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    /// Lets it poll until it ends; its exit status. Lines it writes meanwhile stay queued for `next`.
    pub fn exit(&mut self) -> i32 {
        for _ in 0..NAPS {
            if let Some(code) = self.ended {
                return code;
            }
            self.go_on();
        }
        panic!("the follower did not end in {NAPS} polls")
    }
}

impl Drop for Follower {
    fn drop(&mut self) {
        // The thread's next nap or write finds the test gone and ends it; wait for it, so no thread outlives the test.
        self.go = None;
        drop(std::mem::replace(&mut self.said, channel().1));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// ---------------------------------------------------------------------------------------------------------- tests

#[test]
fn a_follower_hands_over_its_lines_and_its_exit_status_with_no_time_passing() {
    use cpd_core::events::{Kind, NewEvent, append};
    use cpd_core::store::Store;
    let fx = Fx::new();
    fx.env.write_config("version = 1\nmachine = \"m1\"\n[events]\npoll_ms = 20\n");
    let home = cpd_core::home::CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let mut watch = |pid: i64| store.write("t", |tx| append(tx, NewEvent::new(Kind::WatchStarted, "start", serde_json::json!({ "pid": pid })))).unwrap();
    let mut follower = Follower::start(&fx, &[]);
    let (a, b) = (watch(1), watch(2));
    assert_eq!(follower.next().unwrap()["id"], a);
    assert_eq!(follower.next().unwrap()["id"], b);
    follower.close_stdout();
    watch(3);
    assert_eq!(follower.exit(), 0, "a write after the reader went away ends the follower, as an EPIPE does");
    assert_eq!(follower.next(), None);
    assert_eq!(fx.fakes.clock.elapsed(), Duration::from_millis(20) * fx.fakes.clock.naps().len() as u32, "only naps passed time");
}

#[test]
fn a_refusal_is_the_one_line_the_follower_ends_with() {
    let fx = Fx::new();
    fx.env.write_config("version = 1\nmachine = \"m1\"\n");
    let run = follow_once(&fx, &["--plan", "nope"]);
    assert_eq!(run.exit, 4, "{run:?}");
    assert_eq!(run.json()["error"]["code"], "plan_not_found");
}
