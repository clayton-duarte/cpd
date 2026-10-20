//! The engine, `cpd watch` (contract §3.7, §6.6; commands.md §12): one per
//! machine, running E1–E5 and E7–E9 (E6 was removed) every `tick_seconds` under `locks/watch.lock`.
//!
//! - [`engine`]: the loop, its signals, `watch-started` and `watch-stopped`, and E7
//!   (the PR start, which calls `pr::start`);
//! - [`distill`] E9: starts `cpd context distill` for a Context whose digest is behind;
//! - [`escalations`] E1, [`liveness`] E2, [`schedule`] E3, [`completion`] E4,
//!   [`pr`] E5, [`inbox`] E8: one module per step (E6 was removed, its number stays);
//! - [`workers`]: the `@cpd_workers` step, which sets the StatusBar's `workers N/M` text;
//! - [`report`]: an unexpected failure as an `error` Event, once until it clears;
//! - [`ensure`]: `watch --ensure`, which starts the engine in `cpd-work:watch`;
//! - [`restart`]: the tmux server's restart: E3's hold while it is pending, the drain, the detached run;
//! - [`lock`]: `locks/watch.lock`; [`log`]: stderr and `run/watch.log`.

mod completion;
mod distill;
mod engine;
mod ensure;
mod escalations;
mod inbox;
mod liveness;
mod lock;
mod log;
mod pr;
mod report;
pub mod restart;
mod schedule;
mod workers;

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Child;

use crate::error::CpdError;
use crate::output::Output;

/// The `txns.command` of the engine's transactions and the `source` of its
/// `error` Events (commands.md §12).
const SOURCE: &str = "watch";

/// The arguments of `cpd watch` (commands.md §12).
#[derive(clap::Args, Debug)]
pub struct Args {
    /// Start the engine in cpd-work:watch when it is not running, and return at once
    #[arg(long)]
    ensure: bool,
    /// Restart the tmux server onto the installed tmux (internal: the engine starts it, contract §6.1)
    #[arg(long, hide = true, conflicts_with = "ensure")]
    restart_tmux: bool,
}

/// `cpd watch [--ensure]`.
pub fn run(args: Args) -> Result<Output, CpdError> {
    if args.ensure {
        ensure::ensure()
    } else if args.restart_tmux {
        restart::detached()
    } else {
        engine::run()
    }
}

/// Records `child`, a detached child the engine started (`setsid`, so it is in no process group of
/// the engine's), by writing its pid and a newline to `pid_file`: the record of a process that is
/// nowhere else (contract §1.4). Reaps it on a thread of its own while this process lives, then
/// removes the file. An engine that dies first leaves the file behind, with a pid that may since
/// have been reused. Returns the pid; a file that cannot be written is an error, and the child is
/// still reaped.
fn record_detached(mut child: Child, pid_file: PathBuf) -> io::Result<u32> {
    let pid = child.id();
    let written = (|| {
        let dir = pid_file.parent().expect("a pid file lives in a directory");
        fs::create_dir_all(dir)?;
        // Renamed into place, so a reader never sees half a pid.
        let tmp = pid_file.with_extension(format!("pid.{pid}.tmp"));
        fs::write(&tmp, format!("{pid}\n"))?;
        fs::rename(&tmp, &pid_file)
    })();
    std::thread::spawn(move || {
        let _ = child.wait();
        let _ = fs::remove_file(&pid_file);
    });
    written.map(|()| pid)
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::{Duration, Instant};

    use super::*;

    fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !ready() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_detached_childs_pid_is_in_its_file_until_it_is_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("run/x/child.pid");
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = record_detached(child, file.clone()).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), format!("{pid}\n"));
        assert_eq!(fs::read_dir(file.parent().unwrap()).unwrap().count(), 1, "no temporary file is left");
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGKILL).unwrap();
        wait_until("the file to be removed", || !file.exists());
    }
}
