//! Driver and Worker priority (contract, "Priority (hs-d528)"): a dispatched Attempt's driver and its Worker's pi run at idle
//! priority (`nice -n 19`, and I/O class idle where the platform has it), and the caller keeps its own nice. In this process:
//! the kernel applying `nice` is not our code, so the tests read what is: the `Launch` of the driver (`idle`) and the command of the
//! Worker's pane, which start with `host::priority::idle_prefix()`.

use cpd_core::host::priority::idle_prefix;
use serde_json::Value;

use super::driver::{JOB, PLAN};
use super::driver_open::{Live, live};

const ATTEMPT: &str = "api-a1";

/// The nice of this process (`getpriority(2)`; `-1` is also a nice, so `errno` is cleared first).
fn nice_of_self() -> i32 {
    nix::errno::Errno::clear();
    // SAFETY: `getpriority` only reads.
    let nice = unsafe { nix::libc::getpriority(nix::libc::PRIO_PROCESS, 0) };
    assert_eq!(nix::errno::Errno::last_raw(), 0, "getpriority");
    nice
}

/// A Worker that commits `api.txt` and submits: what pi does in its pane.
fn work(live: &Live, id: &str) -> Value {
    live.commit(&live.worktree(id), "api.txt", "api\n", "api");
    live.fx
        .cpd_with(
            &live.worktree(id),
            &["attempt", "submit", id],
            &[("CPD_PLAN", PLAN), ("CPD_ATTEMPT", id)],
        )
        .json()
}

#[test]
fn a_dispatched_attempts_driver_runs_at_nice_19_and_the_caller_keeps_its_nice() {
    let live = live("");
    let before = nice_of_self();

    let out = live.dispatch(JOB).ok();
    assert_eq!(out["attempt"], ATTEMPT);
    let launch = live.fx.take_launch(0);
    let after = nice_of_self();

    // The driver is started through the spawn seam with `idle`, which `ProcessSpawn` turns into the prefix: `nice -n 19` first.
    assert!(launch.idle, "the driver's launch is not idle: {launch:?}");
    assert_eq!(&launch.args[..2], ["attempt", "run"], "{launch:?}");
    assert_eq!(
        idle_prefix()[..3],
        ["nice", "-n", "19"],
        "nice 19 is the first of the prefix"
    );
    assert_eq!(after, before, "the caller keeps its nice");
    assert_eq!(
        live.fx.fakes.spawn.launches().len(),
        1,
        "one process started: the driver"
    );
}

#[test]
fn the_worker_pi_runs_at_nice_19_and_io_class_idle() {
    let live = live("");

    let (attempt, _) = live.run_job(JOB, work);

    assert_eq!(attempt["outcome"], "merged", "{attempt}");
    let windows: Vec<Vec<String>> = live
        .fx
        .calls("tmux")
        .into_iter()
        .map(|c| c.args)
        .filter(|a| a.iter().any(|w| w == "new-window"))
        .collect();
    let worker: Vec<&Vec<String>> = windows
        .iter()
        .filter(|a| a.iter().any(|w| w == "pi"))
        .collect();
    assert_eq!(
        worker.len(),
        1,
        "one window runs the Worker's pi: {windows:?}"
    );
    // The pane's command is the argv of the Worker's launch: the idle prefix, then pi.
    let argv = worker[0];
    let at = argv
        .iter()
        .position(|w| w == "pi")
        .expect("pi is in the command");
    assert!(at >= idle_prefix().len(), "{argv:?}");
    assert_eq!(
        argv[at - idle_prefix().len()..at],
        *idle_prefix(),
        "the Worker's command does not start with the idle prefix: {argv:?}"
    );
}
