//! The Gate's use of the test slot (contract §7.6 "The test slot", hs-d336): a Gate takes one of the machine's
//! `test_concurrency` slots, through the routine it shares with `cpd slot` ([`crate::slot`]), and the wait is
//! logged on stderr and at the head of the `pre-merge` log.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{CpdError, ErrorCode};
pub use crate::slot::{Slot, Slots};

/// Takes a slot for a Gate (waiting for one when every slot is busy) and logs it (contract §7.6):
/// - `log` is truncated, and holds the lines of [`Slots::take`] (prefix `gate`), to which the `pre-merge` hook's
///   output is appended;
/// - the lines of a Gate that waited are on stderr too; the line of a Gate that found a slot free is in `log` only.
pub fn take(slots: &Slots, log: &Path) -> Result<Slot, CpdError> {
    let mut log = HeadLog::begin(log)?;
    slots.take("gate", &mut |line, waited| log.line(line, waited))
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
        let paths = (0..n).map(|i| dir.path().join("run").join(format!("test-{i}.lock"))).collect();
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

        let first = take(&slots, &log).unwrap();
        assert_eq!(first.index(), 0);
        assert_eq!(text(&log), "gate: slot 0 taken after 0 s\n", "the log is truncated, with no wait line");
        let second = take(&slots, &log).unwrap();
        assert_eq!(second.index(), 1);
        drop(first);
        assert_eq!(take(&slots, &log).unwrap().index(), 0, "the first slot is free again and tried first");
    }

    #[test]
    fn with_every_slot_busy_a_gate_waits_logs_it_and_takes_the_slot_that_frees() {
        let (_dir, slots, log) = slots(2);
        let (first, second) = (take(&slots, &log).unwrap(), take(&slots, &log).unwrap());

        let waiter = {
            let (slots, log) = (slots.clone(), log.clone());
            std::thread::spawn(move || take(&slots, &log).unwrap().index())
        };
        wait_for(&log, "gate: still waiting for a slot (");
        assert!(!waiter.is_finished());
        assert!(text(&log).starts_with("gate: waiting for a slot (2 of 2 busy; test_concurrency 2)\ngate: still waiting for a slot ("), "{}", text(&log));
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
        let path = dir.path().join("run").join("test-0.lock");
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

        assert_eq!(take(&slots, &log).unwrap().index(), 0);
        assert_eq!(text(&log), "gate: slot 0 taken after 0 s\n", "no wait");
    }
}
