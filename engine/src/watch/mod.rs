//! The engine, `cpd watch` (contract §3.7, §6.6; commands.md §12): one per
//! machine, running E1–E5, E7–E11 and E13 (E6 was removed) every `tick_seconds` under `locks/watch.lock`.
//!
//! - [`engine`]: the loop, its signals, `watch-started` and `watch-stopped`, and E7
//!   (the PR start, which calls `pr::start`);
//! - [`distill`] E9: starts `cpd context distill` for a Context whose digest is behind;
//! - [`titles`] E10: starts `cpd session retitle` for idle Lead and Manager Sessions whose turn ended after their title;
//! - [`sleep`] E13: puts finished Lead and Manager Sessions to sleep;
//! - [`archive_leads`] E14: archives the done Plan of a Lead asleep past `timeouts.archive_sleep_seconds`;
//! - [`team`] E11: reads the team Slack channel through the detached helper into `run/team-review.json`;
//! - [`models`] E15: lists the models pi can offer into `run/models-<provider>.json` (7 days);
//! - [`janitor`] E16: starts `cpd janitor` detached every `watch.janitor_minutes`;
//! - [`escalations`] E1, [`liveness`] E2, [`schedule`] E3, [`open`] E4,
//!   [`pr`] E5, [`inbox`] E8: one module per step (E6 was removed, its number stays);
//! - [`workers`]: the `@cpd_workers` step, which sets the StatusBar's `workers N/M` text;
//! - [`host_alert`] E6: the `@cpd_host_alert` step, which turns the version pill red (hs-d618);
//! - [`report`]: an unexpected failure as an `error` Event, once until it clears;
//! - [`ensure`]: `watch --ensure`, which starts the engine in `cpd-work:watch`, and `watch --check`, which verifies what that start depends on without touching anything;
//! - [`restart`]: the tmux server's restart: E3's hold while it is pending, the drain, the detached run;
//! - [`lock`]: `locks/watch.lock`; [`log`]: stderr and `run/watch.log`.

mod archive;
mod archive_leads;
mod open;
mod distill;
mod engine;
mod ensure;
mod escalations;
pub mod host_alert;
mod inbox;
mod janitor;
mod react;
mod liveness;
mod lock;
mod log;
mod models;
mod poll_run;
mod pr;
mod provider_hold;
mod report;
pub mod restart;
mod schedule;
mod sleep;
pub mod team;
mod titles;
mod wake;
mod workers;
#[cfg(test)]
mod local_time_tests;
#[cfg(test)]
mod incident_tests;
#[cfg(test)]
mod provider_hold_e3_tests;
#[cfg(test)]
mod janitor_tests;
#[cfg(test)]
mod inbox_tests;
#[cfg(test)]
mod open_tests;
#[cfg(test)]
mod archive_leads_tests;
#[cfg(test)]
mod archive_tests;
#[cfg(test)]
mod schedule_tests;
#[cfg(test)]
mod testkit;
#[cfg(test)]
mod pill_texts;
#[cfg(test)]
mod poll_run_tests;
#[cfg(test)]
mod incident_option_tests;
#[cfg(test)]
mod runner_tests;
#[cfg(test)]
mod ports_tests;
#[cfg(test)]
mod provider_hold_tests;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::spawn::{Launch, Pid};
use cpd_core::output::Output;

/// The `txns.command` of the engine's transactions and the `source` of its
/// `error` Events (commands.md §12).
const SOURCE: &str = "watch";

/// The arguments of `cpd watch` (commands.md §12).
#[derive(clap::Args, Debug)]
pub struct Args {
    /// Check, read-only, what starting the engine depends on (config, store, tmux, its listing, the home) and print what was checked
    #[arg(long, conflicts_with_all = ["ensure", "restart_tmux"])]
    check: bool,
    /// Start the engine in cpd-work:watch when it is not running, and return at once
    #[arg(long)]
    ensure: bool,
    /// With `--ensure`, as the supervising tmux hooks call it (contract §6.6): wait for a dying engine's lock, one start at a time, backing off after a failed start
    #[arg(long, hide = true, requires = "ensure")]
    wait: bool,
    /// Restart the tmux server onto the installed tmux (internal: the engine starts it, contract §6.1)
    #[arg(long, hide = true, conflicts_with = "ensure")]
    restart_tmux: bool,
    /// Write run/install-restart.json for the engine about to be retired (internal: scripts/install-cpd.sh, contract §3.7 "A planned restart")
    #[arg(long, hide = true, conflicts_with_all = ["ensure", "restart_tmux"])]
    mark_restart: bool,
}

/// `cpd watch [--ensure]`.
pub fn run(args: Args) -> Result<Output, CpdError> {
    if args.check {
        ensure::check()
    } else if args.ensure {
        ensure::ensure(args.wait)
    } else if args.restart_tmux {
        restart::detached()
    } else if args.mark_restart {
        engine::mark_restart()
    } else {
        engine::run()
    }
}

/// `cpd watch` on `ports` in `CPD_HOME`: the engine runs its ticks, waiting through `ports.clock`, until `until` is set.
pub fn run_with(ports: &cpd_core::ports::Ports, until: &std::sync::atomic::AtomicBool) -> Result<Output, CpdError> {
    engine::run_with(ports, &CpdHome::from_env()?, until)
}

/// What the engine starts detached (`setsid`, so the child is in no process group of the engine's; stdin from
/// `/dev/null`): `program` with `args`, in the home, `CPD_HOME` set and the caller's own identity removed, its stdout
/// and stderr appended to `log`.
fn detached_launch(program: &Path, args: &[&str], home: &CpdHome, log: PathBuf) -> Launch {
    Launch {
        program: program.to_string_lossy().into_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        env_set: vec![("CPD_HOME".to_owned(), home.root().to_string_lossy().into_owned())],
        env_remove: ["CPD_PLAN", "CPD_SESSION", "CPD_ATTEMPT", "TMUX", "TMUX_PANE"].map(str::to_owned).to_vec(),
        cwd: Some(home.root().to_owned()),
        setsid: true,
        stdout_to: Some(log),
        ..Launch::default()
    }
}

/// Records `pid`, a detached child the engine started, by writing its pid and a newline to `pid_file`: the record of a
/// process that is nowhere else (contract §1.4). The engine removes the file when it sees the child end
/// ([`sweep_detached`]); an engine that dies first leaves it, with a pid that may since have been reused. Returns the
/// pid; a file that cannot be written is an error, and the child is still tracked.
fn record_detached(engine: &mut engine::Engine, pid: Pid, pid_file: PathBuf) -> io::Result<u32> {
    engine.detached.push((pid, pid_file.clone()));
    let dir = pid_file.parent().expect("a pid file lives in a directory");
    fs::create_dir_all(dir)?;
    // Renamed into place, so a reader never sees half a pid.
    let tmp = pid_file.with_extension(format!("pid.{}.tmp", pid.0));
    fs::write(&tmp, format!("{}\n", pid.0))?;
    fs::rename(&tmp, &pid_file)?;
    Ok(pid.0)
}

/// Removes the pid file of each detached child that has ended (`Spawn::try_wait`).
fn sweep_detached(engine: &mut engine::Engine) {
    let spawn = engine.ports.spawn.clone();
    engine.detached.retain(|(pid, file)| {
        let running = matches!(spawn.try_wait(*pid), Ok(None));
        if !running {
            let _ = fs::remove_file(file);
        }
        running
    });
}

#[cfg(test)]
mod tests {
    use cpd_core::host::spawn::{FakeSpawn, Spawn};

    use super::*;
    use super::testkit::Fx;

    #[test]
    fn a_detached_childs_pid_is_in_its_file_until_the_engine_sees_it_end() {
        let mut fx = Fx::new("");
        let spawn = std::sync::Arc::new(FakeSpawn::new());
        fx.engine.ports.spawn = spawn.clone();
        let file = fx.engine.home.root().join("run/x/child.pid");
        let pid = spawn.spawn(&Launch::default()).unwrap();

        assert_eq!(record_detached(&mut fx.engine, pid, file.clone()).unwrap(), pid.0);
        assert_eq!(fs::read_to_string(&file).unwrap(), format!("{}\n", pid.0));
        assert_eq!(fs::read_dir(file.parent().unwrap()).unwrap().count(), 1, "no temporary file is left");
        sweep_detached(&mut fx.engine);
        assert!(file.exists(), "it runs");

        spawn.exit(pid, 0);
        sweep_detached(&mut fx.engine);
        assert!(!file.exists(), "it ended");
    }
}
#[cfg(test)]
mod wake_tests;
