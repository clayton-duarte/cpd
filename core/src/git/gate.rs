//! The Gate slot (contract §7.6 "The Gate slot", hs-d336): at most `gate_concurrency` Gates run at once on a
//! machine. A slot is a `flock(2)` on `$CPD_HOME/run/gate-<i>.lock`, taken through [`crate::lock`] and held until
//! the [`Slot`] is dropped; the kernel frees it when its holder dies, so there is no stale slot.
//!
//! [`Slots::take`] tries the slots in order without blocking and, when all are busy, polls: it never blocks on one
//! slot (another might free first, and a blocked `flock` cannot see a raised `gate_concurrency`). The wait is
//! logged on stderr and at the head of the `pre-merge` log; it has no cap.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::lock;

/// How long a Gate that found every slot busy sleeps before it tries them all again (`GATE_SLOT_POLL_MS`).
pub const POLL: Duration = Duration::from_millis(1000);

/// How often a Gate that keeps waiting says so.
pub const STILL_WAITING: Duration = Duration::from_secs(60);

/// The slots of one Gate run: `gate_concurrency` lock files, read once when the run begins.
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
    /// `i` of `run/gate-<i>.lock`.
    pub fn index(&self) -> usize {
        self.index
    }
}

impl Slots {
    /// The slots `0..concurrency` of `home`, polled as the contract says.
    pub fn new(home: &CpdHome, concurrency: u32) -> Slots {
        Slots::in_files((0..concurrency).map(|i| home.gate_slot(i)).collect(), POLL, STILL_WAITING)
    }

    /// Slots in `paths` (in the order they are tried), with their own timing.
    pub fn in_files(paths: Vec<PathBuf>, poll: Duration, still_waiting: Duration) -> Slots {
        assert!(!paths.is_empty(), "gate_concurrency is at least 1 (config validation)");
        Slots { paths, poll, still_waiting }
    }

    /// Takes the first free slot, waiting for one when every slot is busy, and logs it (contract §7.6):
    /// - `log` is truncated, and holds the lines below, to which the `pre-merge` hook's output is appended;
    /// - when the first try finds every slot busy, `gate: waiting for a slot (<N> of <N> busy; gate_concurrency <N>)`
    ///   on stderr and in `log`; then, every `still_waiting`, `gate: still waiting for a slot (<s> s)`;
    /// - `gate: slot <i> taken after <s> s` in `log`, and on stderr only when the Gate had to wait.
    pub fn take(&self, log: &Path) -> Result<Slot, CpdError> {
        let begun = Instant::now();
        let mut log = HeadLog::begin(log)?;
        let mut waiting = false;
        let mut next_notice = self.still_waiting;
        loop {
            if let Some(slot) = self.try_all()? {
                log.line(&format!("gate: slot {} taken after {} s", slot.index, begun.elapsed().as_secs()), waiting)?;
                return Ok(slot);
            }
            let waited = begun.elapsed();
            if !waiting {
                waiting = true;
                let n = self.paths.len();
                log.line(&format!("gate: waiting for a slot ({n} of {n} busy; gate_concurrency {n})"), true)?;
            } else if waited >= next_notice {
                next_notice += self.still_waiting;
                log.line(&format!("gate: still waiting for a slot ({} s)", waited.as_secs()), true)?;
            }
            // time-claim: the poll of the contract (GATE_SLOT_POLL_MS); there is no event to wait for, as a blocked
            // flock cannot see another slot free or a raised gate_concurrency.
            std::thread::sleep(self.poll);
        }
    }

    /// One pass over the slots in order, without blocking: the first that is free is taken.
    fn try_all(&self) -> Result<Option<Slot>, CpdError> {
        for (index, path) in self.paths.iter().enumerate() {
            let failed = |why: &dyn std::fmt::Display| {
                CpdError::new(ErrorCode::GateError, format!("Cannot take the Gate slot {}: {why}.", path.display()))
                    .with("lock", path.display().to_string())
            };
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|e| failed(&e))?;
            }
            // Close-on-exec (std's default), so no hook the Gate runs keeps the slot alive.
            let file = File::options().read(true).write(true).create(true).truncate(false).open(path).map_err(|e| failed(&e))?;
            if lock::try_lock(&file).map_err(|e| failed(&format!("flock failed: {e}")))? {
                return Ok(Some(Slot { _file: file, index }));
            }
        }
        Ok(None)
    }
}

/// The `pre-merge` log, which the wait's lines head.
struct HeadLog {
    path: PathBuf,
    file: File,
}

impl HeadLog {
    /// Truncates the log: this run begins it (contract §7.6).
    fn begin(path: &Path) -> Result<HeadLog, CpdError> {
        let failed = |e: std::io::Error| {
            CpdError::new(ErrorCode::GateError, format!("Cannot write {}: {e}.", path.display())).with("log", path.display().to_string())
        };
        path.parent().map_or(Ok(()), fs::create_dir_all).map_err(failed)?;
        let file = File::create(path).map_err(failed)?;
        Ok(HeadLog { path: path.to_path_buf(), file })
    }

    /// `text` and a newline in the log, and on stderr when `stderr`.
    fn line(&mut self, text: &str, stderr: bool) -> Result<(), CpdError> {
        writeln!(self.file, "{text}").map_err(|e| {
            CpdError::new(ErrorCode::GateError, format!("Cannot write {}: {e}.", self.path.display())).with("log", self.path.display().to_string())
        })?;
        if stderr {
            eprintln!("{text}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// `n` slots in a fresh directory, polled every 5 ms and announced every 20 ms.
    fn slots(n: u32) -> (tempfile::TempDir, Slots, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let paths = (0..n).map(|i| dir.path().join("run").join(format!("gate-{i}.lock"))).collect();
        let log = dir.path().join("logs").join("pr-x.pre-merge.log");
        (dir, Slots::in_files(paths, Duration::from_millis(5), Duration::from_millis(20)), log)
    }

    fn text(log: &Path) -> String {
        fs::read_to_string(log).unwrap_or_default()
    }

    /// Waits until `log` contains `needle`, as long as it takes.
    fn wait_for(log: &Path, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(120);
        while !text(log).contains(needle) {
            assert!(Instant::now() < deadline, "no {needle:?} in {:?}", text(log));
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_free_slot_is_taken_at_once_in_order_and_only_the_log_says_so() {
        let (_dir, slots, log) = slots(2);
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::write(&log, "left by an earlier run\n").unwrap();

        let first = slots.take(&log).unwrap();
        assert_eq!(first.index(), 0);
        assert_eq!(text(&log), "gate: slot 0 taken after 0 s\n", "the log is truncated, with no wait line");
        let second = slots.take(&log).unwrap();
        assert_eq!(second.index(), 1);
        drop(first);
        assert_eq!(slots.take(&log).unwrap().index(), 0, "the first slot is free again and tried first");
    }

    #[test]
    fn with_every_slot_busy_a_gate_waits_logs_it_and_takes_the_slot_that_frees() {
        let (_dir, slots, log) = slots(2);
        let (first, second) = (slots.take(&log).unwrap(), slots.take(&log).unwrap());

        let waiter = {
            let (slots, log) = (slots.clone(), log.clone());
            std::thread::spawn(move || slots.take(&log).unwrap().index())
        };
        wait_for(&log, "gate: still waiting for a slot (");
        assert!(!waiter.is_finished());
        assert!(text(&log).starts_with("gate: waiting for a slot (2 of 2 busy; gate_concurrency 2)\ngate: still waiting for a slot ("), "{}", text(&log));
        // The second slot frees, not the first: the Gate takes it, and does not block on slot 0.
        drop(second);
        assert_eq!(waiter.join().unwrap(), 1);
        assert!(text(&log).trim_end().ends_with(" s") && text(&log).contains("\ngate: slot 1 taken after "), "{}", text(&log));
        drop(first);
    }

    #[test]
    fn a_slot_held_by_a_dead_process_is_free_at_once() {
        // A child that takes slot 0 through its own descriptor and is killed: the kernel frees it.
        let (dir, slots, log) = slots(1);
        let path = dir.path().join("run").join("gate-0.lock");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // `exec sleep` keeps the pid, so the kill below ends the one process that holds the descriptor.
        let mut holder = std::process::Command::new("sh")
            .args(["-c", "exec 9>>\"$0\"; flock --nonblock 9 || exit 1; echo held; exec sleep 600"])
            .arg(&path)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        {
            use std::io::BufRead;
            let mut line = String::new();
            std::io::BufReader::new(holder.stdout.take().unwrap()).read_line(&mut line).unwrap();
            assert_eq!(line, "held\n");
        }
        let probe = File::options().read(true).write(true).open(&path).unwrap();
        assert!(!lock::try_lock(&probe).unwrap(), "the holder has the slot");
        drop(probe);

        nix::sys::signal::kill(nix::unistd::Pid::from_raw(holder.id() as i32), nix::sys::signal::Signal::SIGKILL).unwrap();
        holder.wait().unwrap();

        assert_eq!(slots.take(&log).unwrap().index(), 0);
        assert_eq!(text(&log), "gate: slot 0 taken after 0 s\n", "no wait");
    }
}
