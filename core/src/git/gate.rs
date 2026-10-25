//! The Gate's use of the test slot (contract §7.6 "The test slot", hs-d336): a Gate takes one of the machine's
//! `test_concurrency` slots, through the routine it shares with `cpd slot` ([`crate::slot`]), and the wait is
//! logged on stderr and at the head of the `pre-merge` log.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{CpdError, ErrorCode};
use crate::ports::Ports;
pub use crate::slot::{Slot, Slots};

/// Takes a slot for a Gate (waiting for one when every slot is busy) and logs it (contract §7.6):
/// - `log` is truncated, and holds the lines of [`Slots::take`] (prefix `gate`), to which the `pre-merge` hook's
///   output is appended;
/// - the lines of a Gate that waited are on stderr too; the line of a Gate that found a slot free is in `log` only.
pub fn take(slots: &Slots, log: &Path) -> Result<Slot, CpdError> {
    take_with(&Ports::real(), slots, log)
}

/// [`take`] with `ports.clock` for the elapsed time and the polls' naps ([`Slots::take_with`]).
pub fn take_with(ports: &Ports, slots: &Slots, log: &Path) -> Result<Slot, CpdError> {
    let mut log = HeadLog::begin(log)?;
    slots.take_with(ports, "gate", &mut |line, waited| log.line(line, waited))
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
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use time::OffsetDateTime;

    use super::*;
    use crate::clock::{Clock, ManualClock};
    use crate::ports::Fakes;

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

    /// A `ManualClock` that lets go of `held` in its `at`-th nap: what another process does "between two polls", on
    /// the thread that waits, with no thread, no race and no real time.
    struct Frees {
        inner: ManualClock,
        at: usize,
        held: Mutex<Option<Slot>>,
    }

    impl Clock for Frees {
        fn now(&self) -> Instant {
            self.inner.now()
        }
        fn sleep(&self, d: Duration) {
            self.inner.sleep(d);
            if self.inner.naps().len() == self.at {
                self.held.lock().unwrap().take();
            }
        }
        fn utc(&self) -> OffsetDateTime {
            self.inner.utc()
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
        // The second slot frees, not the first, in the sixth nap (30 ms): after the line of the 20 ms.
        let fakes = Fakes::new();
        let clock = Arc::new(Frees { inner: (*fakes.clock).clone(), at: 6, held: Mutex::new(Some(second)) });
        let ports = Ports { clock: clock.clone(), ..fakes.ports.clone() };

        let taken = take_with(&ports, &slots, &log).unwrap();

        // The Gate takes the slot that freed, and does not block on slot 0.
        assert_eq!(taken.index(), 1);
        assert_eq!(fakes.clock.naps(), vec![Duration::from_millis(5); 6], "it waited its poll, 6 times, on the clock");
        assert!(text(&log).starts_with("gate: waiting for a slot (2 of 2 busy; test_concurrency 2)\ngate: still waiting for a slot ("), "{}", text(&log));
        assert!(text(&log).trim_end().ends_with(" s") && text(&log).contains("\ngate: slot 1 taken after "), "{}", text(&log));
        drop(first);
    }
}
