//! The test slots (contract §7.6 "The test slot", hs-d336, hs-d301): at most `test_concurrency` full test runs at
//! once on a machine, whether a Gate's `pre-start` and `pre-merge` or a command run by `cpd slot` (commands.md §23).
//!
//! A slot is a `flock(2)` on `$CPD_HOME/run/test-<i>.lock`, taken through [`crate::lock`] and held until the [`Slot`]
//! is dropped; the kernel frees it when its holder dies, so there is no stale slot. [`Slots`] is the one routine
//! both callers use; `cpd slot` is [`run`].
//!
//! [`Slots::take`] tries the slots in order without blocking and, when all are busy, polls: it never blocks on one
//! slot (another might free first, and a blocked `flock` cannot see a raised `test_concurrency`). The wait has no cap;
//! its lines go to a caller-given writer.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::ErrorKind;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, kill, sigaction};
use nix::unistd::Pid;

use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::lock;
use crate::ports::Ports;

/// How long a caller that found every slot busy sleeps before it tries them all again (`GATE_SLOT_POLL_MS`).
pub const POLL: Duration = Duration::from_millis(1000);

/// How often a caller that keeps waiting says so.
pub const STILL_WAITING: Duration = Duration::from_secs(60);

/// The environment variable that names the held slot (contract §7.6, hs-d301): its value is the slot number.
pub const SLOT_VARIABLE: &str = "CPD_TEST_SLOT";

/// The slots of one run: `test_concurrency` lock files, read once when the run begins.
#[derive(Clone, Debug)]
pub struct Slots {
    paths: Vec<PathBuf>,
    poll: Duration,
    still_waiting: Duration,
}

/// A slot taken: held until this is dropped (or the process dies).
#[derive(Debug)]
pub struct Slot {
    _file: File,
    index: usize,
}

impl Slot {
    /// `i` of `run/test-<i>.lock`.
    pub fn index(&self) -> usize {
        self.index
    }
}

impl Slots {
    /// The slots `0..concurrency` of `home`, polled as the contract says.
    pub fn new(home: &CpdHome, concurrency: u32) -> Slots {
        Slots::in_files((0..concurrency).map(|i| home.test_slot(i)).collect(), POLL, STILL_WAITING)
    }

    /// Slots in `paths` (in the order they are tried), with their own timing.
    pub fn in_files(paths: Vec<PathBuf>, poll: Duration, still_waiting: Duration) -> Slots {
        assert!(!paths.is_empty(), "test_concurrency is at least 1 (config validation)");
        Slots { paths, poll, still_waiting }
    }

    /// Takes the first free slot, waiting for one when every slot is busy. Each line goes to `report` as
    /// `report(line, waited)`, `waited` being true for every line of a run that had to wait; a caller shows those
    /// to the person running it. `prefix` begins every line (`gate`, `slot`):
    /// - when the first try finds every slot busy, `<prefix>: waiting for a slot (<N> of <N> busy; test_concurrency <N>)`;
    ///   then, every `still_waiting`, `<prefix>: still waiting for a slot (<s> s)`;
    /// - when taken, `<prefix>: slot <i> taken after <s> s` (also at once: `after 0 s`, with `waited` false).
    pub fn take(&self, prefix: &str, report: &mut dyn FnMut(&str, bool) -> Result<(), CpdError>) -> Result<Slot, CpdError> {
        self.take_with(&Ports::real(), prefix, report)
    }

    /// [`Slots::take`] with `ports.clock` for the elapsed time and the polls' naps.
    pub fn take_with(&self, ports: &Ports, prefix: &str, report: &mut dyn FnMut(&str, bool) -> Result<(), CpdError>) -> Result<Slot, CpdError> {
        let begun = ports.clock.now();
        let elapsed = || ports.clock.now().saturating_duration_since(begun);
        let mut waiting = false;
        let mut next_notice = self.still_waiting;
        loop {
            if let Some(slot) = self.try_all()? {
                report(&format!("{prefix}: slot {} taken after {} s", slot.index, elapsed().as_secs()), waiting)?;
                return Ok(slot);
            }
            let waited = elapsed();
            if !waiting {
                waiting = true;
                let n = self.paths.len();
                report(&format!("{prefix}: waiting for a slot ({n} of {n} busy; test_concurrency {n})"), true)?;
            } else if waited >= next_notice {
                next_notice += self.still_waiting;
                report(&format!("{prefix}: still waiting for a slot ({} s)", waited.as_secs()), true)?;
            }
            // time-claim: the poll of the contract (GATE_SLOT_POLL_MS); there is no event to wait for, as a blocked
            // flock cannot see another slot free or a raised test_concurrency.
            ports.clock.sleep(self.poll);
        }
    }

    /// One pass over the slots in order, without blocking: the first that is free is taken.
    fn try_all(&self) -> Result<Option<Slot>, CpdError> {
        for (index, path) in self.paths.iter().enumerate() {
            let failed = |why: &dyn std::fmt::Display| {
                CpdError::new(ErrorCode::Internal, format!("Cannot take the test slot {}: {why}.", path.display()))
                    .with("lock", path.display().to_string())
            };
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|e| failed(&e))?;
            }
            // Close-on-exec (std's default), so no command the holder runs keeps the slot alive.
            let file = File::options().read(true).write(true).create(true).truncate(false).open(path).map_err(|e| failed(&e))?;
            if lock::try_lock(&file).map_err(|e| failed(&format!("flock failed: {e}")))? {
                return Ok(Some(Slot { _file: file, index }));
            }
        }
        Ok(None)
    }
}

/// `cpd slot -- <cmd> [args…]` (commands.md §23).
#[derive(clap::Args, Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// The command and its arguments, run as given (looked up on `PATH`, no shell)
    #[arg(last = true, value_name = "CMD")]
    pub command: Vec<OsString>,
}

/// `cpd slot` (commands.md §23): runs the command in a slot and returns its exit status; the slot is free again
/// when this returns. Inside a slot already (`CPD_TEST_SLOT` set and non-empty) it takes none.
pub fn run(args: Args) -> Result<u8, CpdError> {
    let Some((program, rest)) = args.command.split_first() else {
        return Err(CpdError::new(ErrorCode::Usage, "A command is required after `--`: cpd slot -- <cmd> [args…]."));
    };
    let mut command = Command::new(program);
    command.args(rest);
    if std::env::var_os(SLOT_VARIABLE).is_some_and(|v| !v.is_empty()) {
        return Ok(execute(command, program));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let slot = Slots::new(&home, config.test_concurrency).take("slot", &mut |line, waited| {
        if waited {
            eprintln!("{line}");
        }
        Ok(())
    })?;
    command.env(SLOT_VARIABLE, slot.index().to_string());
    let status = execute(command, program);
    drop(slot);
    Ok(status)
}

/// The pid of the command, for [`forward`]; `0` before it has started.
static CHILD: AtomicI32 = AtomicI32::new(0);
/// A signal that arrived before the command had started, `0` for none.
static EARLY: AtomicI32 = AtomicI32::new(0);

extern "C" fn forward(signal: i32) {
    match CHILD.load(Ordering::Relaxed) {
        0 => EARLY.store(signal, Ordering::Relaxed),
        pid => {
            // The signal of a command that has ended is an ESRCH, ignored: the pid is read, not owned.
            let _ = Signal::try_from(signal).map(|s| kill(Pid::from_raw(pid), s));
        }
    }
}

/// Starts `command`, which inherits stdin, stdout and stderr, and waits for it, forwarding `SIGINT`, `SIGTERM` and
/// `SIGHUP` to it, so that a signal never frees a slot under a command that still runs. Returns the exit status of
/// the shell's convention: `128 + n` for signal `n`, `127` for a command not found, `126` for one that cannot run.
fn execute(mut command: Command, program: &OsString) -> u8 {
    let action = SigAction::new(SigHandler::Handler(forward), SaFlags::empty(), SigSet::empty());
    for signal in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGHUP] {
        // SAFETY: the handler only reads and writes atomics and calls kill(2), which are async-signal-safe.
        if let Err(e) = unsafe { sigaction(signal, &action) } {
            eprintln!("cpd slot: cannot forward {signal}: {e}.");
        }
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let name = program.to_string_lossy();
            return match e.kind() {
                ErrorKind::NotFound => {
                    eprintln!("cpd slot: {name}: command not found.");
                    127
                }
                _ => {
                    eprintln!("cpd slot: {name}: cannot execute: {e}.");
                    126
                }
            };
        }
    };
    CHILD.store(child.id() as i32, Ordering::Relaxed);
    if let Some(early) = Some(EARLY.swap(0, Ordering::Relaxed)).filter(|s| *s != 0) {
        forward(early);
    }
    match child.wait() {
        Ok(status) => status.code().map_or_else(|| 128 + status.signal().unwrap_or(0), |c| c) as u8,
        Err(e) => {
            eprintln!("cpd slot: cannot wait for {}: {e}.", program.to_string_lossy());
            126
        }
    }
}
