//! The Worker teardown `cpd attempt abort` triggers (contract §3.4 "attempt abort", "The Worker runner and its seam"
//! A9 Stop, §3.7 E2, §7.4 R): a `durable` Worker is stopped by the existing stop seam (`runner::stop`: SIGTERM, then
//! SIGKILL after `timeouts.pi_stop_seconds`, the cmdline guard honoured) and its A4 restart is suppressed; a `pi`
//! Worker has its tmux window killed and its worktree cleaned through the existing crash-recovery/A9 path, never a new
//! one. The `cpd attempt abort` core command already exists and ends the Attempt `failed` / `aborted` (the core impl
//! Job); these tests are red only because the driver/engine do not yet honour that stop.
//!
//! Everything runs in this process on the fakes: the durable runner and the tmux server are the existing `FakeSpawn`
//! and fake tmux, the worktrees are the fake git's, and no real process is ever started. The live driver is the one
//! [`Live::drive`] runs, with the driver lock's flock held the way a real driver holds it.

use crate::common;

use std::fs::OpenOptions;
use std::time::Duration;

use common::Run;
use cpd_core::git::{Setup, create_attempt_worktree};
use cpd_core::home::CpdHome;
use cpd_core::host::spawn::{Launch, Pid, Signal, Spawn};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::ports::Ports;
use serde_json::{Value, json};

use super::driver::{JOB, PLAN};
use super::driver_open::{Live, live};
use super::worker_runner_durable::durable_live;

const ATTEMPT: &str = "api-a1";
/// The Lead Session of Plan `pay`, as the driver fixtures seed it.
const LEAD: &str = "lead1";
/// The grace of a stop (`timeouts.pi_stop_seconds`), long enough that a driver that does not wait for it cannot be
/// mistaken for one that does.
const GRACE_SECONDS: u64 = 30;
const GRACE: &str = "[timeouts]\npi_stop_seconds = 30";

// ---------- helpers ----------

fn home(live: &Live) -> CpdHome {
    CpdHome::resolve(Some(live.fx.env.cpd_home().into()), None).unwrap()
}

/// The pid the `FakeSpawn` gave its `i`th launch: it counts from 1000 and every launch of these tests got one.
fn pid_of_launch(i: usize) -> Pid {
    Pid(1000 + u32::try_from(i).unwrap())
}

/// The pid stored as the Worker's (`attempt show`'s `worker_pid`); the test fails when none is stored.
fn stored_pid(live: &Live, id: &str) -> Pid {
    let attempt = live.attempt(id);
    let pid = attempt["worker_pid"].as_u64().unwrap_or_else(|| panic!("no worker_pid is stored: {attempt}"));
    Pid(u32::try_from(pid).unwrap())
}

/// What a process of `launch` shows as its command line.
fn cmdline_of(launch: &Launch) -> String {
    std::iter::once(launch.program.as_str()).chain(launch.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
}

/// The runner's `Launch`, the second of the `FakeSpawn` (after the driver's), when the driver started it; the test fails otherwise.
fn the_runners_launch(live: &Live) -> Launch {
    let launches = live.fx.fakes.spawn.launches();
    assert_eq!(launches.len(), 2, "the driver's launch and the runner's: {launches:#?}");
    launches[1].clone()
}

/// The runner the driver started, as the reader of the Attempt's `worker_pid` finds it: its pid is stored, and it shows its
/// own command line (what the guard of a signal compares, from the OS in production).
fn the_runner(live: &Live, id: &str) -> Pid {
    let launch = the_runners_launch(live);
    let pid = stored_pid(live, id);
    assert_eq!(pid, pid_of_launch(1), "the stored pid is the runner's");
    live.fx.fakes.spawn.set_cmdline(pid, &cmdline_of(&launch));
    pid
}

/// `cpd attempt abort <attempt> --plan pay`, as the Lead's Session runs it, in this process.
fn abort(live: &Live, id: &str) -> Value {
    live.fx.cpd_with(&live.fx.env.root(), &["attempt", "abort", id, "--plan", PLAN], &[("CPD_SESSION", LEAD)]).ok()
}

fn assert_ended(live: &Live, id: &str, outcome: &str, cause: &str) {
    let attempt = live.attempt(id);
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!(outcome), &json!(cause)), "{attempt}");
}

/// The state of the Worker Session of Attempt `id`.
fn session_state(live: &Live, id: &str) -> String {
    live.value(&format!("SELECT state FROM sessions WHERE plan = '{PLAN}' AND attempt = '{id}'"))
}

/// [`Live::drive`] with the driver lock's flock held by the test, as a live driver holds it in production. The in-process
/// driver only checks fd 3's identity (it does not flock the file), so without this the abort's liveness probe would read
/// the driver as dead. `hook(live, id, n)` runs after the driver's `n`th nap.
fn drive_with_the_lock(live: &Live, key: &str, mut hook: impl FnMut(&Live, &str, usize)) -> (String, Run) {
    let out = live.dispatch(key).ok();
    let id = out["attempt"].as_str().unwrap_or_else(|| panic!("no attempt in {out}")).to_owned();
    let launch = live.fx.take_launch(live.fx.fakes.spawn.launches().len() - 1);
    let path = home(live).attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(&id).unwrap());
    // fd 3 first (it must still be free), then the flock on another description, as a real driver's starter holds it, so
    // the abort's liveness probe sees the driver as live.
    let _fd3 = live.fx.hand_over(std::fs::File::open(&path).unwrap());
    let held = OpenOptions::new().read(true).write(true).open(&path).unwrap();
    held.try_lock().expect("the test holds the driver lock");
    let ran = live.fx.run_launch_with(&launch, |_, nap| hook(live, &id, nap));
    drop(held);
    (id, ran)
}

// ---------- (1) a durable Worker stops through the existing stop seam ----------

#[test]
fn durable_worker_is_stopped_through_the_existing_path() {
    let live = durable_live(GRACE);
    let spawn = live.fx.fakes.spawn.clone();
    let (mut pid, mut asked_at) = (None, None);

    // The runner ignores SIGTERM: the driver waits `pi_stop_seconds`, then SIGKILLs it. The abort is the stop request.
    let (id, ran) = drive_with_the_lock(&live, JOB, |live, id, nap| {
        if nap == 1 {
            let runner = the_runner(live, id);
            spawn.survive(runner, Signal::Term);
            pid = Some(runner);
            asked_at = Some(live.fx.fakes.clock.elapsed());
            abort(live, id);
        }
        // During the grace: SIGTERM was sent, the runner lives on, and nothing else is sent yet.
        if nap > 1 {
            let pid = pid.expect("the abort is the first nap");
            assert_eq!(spawn.signals(), [(pid, Signal::Term)], "nap {nap}");
            assert!(spawn.alive(pid), "nap {nap}");
        }
    });

    let pid = pid.unwrap();
    assert_eq!(ran.exit, 0, "the driver honours the abort and ends cleanly: {}", ran.stdout);
    assert_ended(&live, &id, "failed", "aborted");
    assert_eq!(spawn.signals(), [(pid, Signal::Term), (pid, Signal::Kill)], "SIGTERM, then SIGKILL");
    assert!(!spawn.alive(pid), "the runner is stopped");
    let waited = live.fx.fakes.clock.elapsed() - asked_at.unwrap();
    assert!(
        waited >= Duration::from_secs(GRACE_SECONDS),
        "SIGKILL came after {waited:?} of clock, before the {GRACE_SECONDS} s of timeouts.pi_stop_seconds"
    );
    assert_eq!(session_state(&live, &id), "stopped", "the worker Session is stopped");
    assert_eq!(spawn.launches().len(), 2, "nothing started the runner again");
}

// ---------- (2) the A4 restart is suppressed after the abort ----------

/// The live evidence (Manager, 19:18 EDT, up-p1-9-a1): a SIGKILL alone is absorbed — the driver's A4 restart bound
/// respawns the worker in ~2 s and the Attempt resumes. The abort must end the Attempt instead of restarting it.
#[test]
fn durable_restart_is_suppressed_after_abort() {
    let live = durable_live("");
    let spawn = live.fx.fakes.spawn.clone();
    let mut pid = None;

    let (id, ran) = drive_with_the_lock(&live, JOB, |live, id, nap| {
        if nap == 1 {
            let runner = the_runner(live, id);
            pid = Some(runner);
            // The SIGKILL the A4 restart bound would absorb.
            spawn.signal(runner, Signal::Kill).unwrap();
            abort(live, id);
        }
    });

    let pid = pid.unwrap();
    assert_eq!(ran.exit, 0, "the driver honours the abort and ends cleanly: {}", ran.stdout);
    assert_ended(&live, &id, "failed", "aborted");
    assert_eq!(spawn.launches().len(), 2, "the worker was not respawned: no second Launch for the Attempt");
    assert_eq!(live.attempt(&id)["worker_pid"], pid.0, "no new worker_pid was stored");
    assert_eq!(session_state(&live, &id), "stopped", "the worker Session is stopped");
}

// ---------- (3) a pi Worker's window and worktree go through crash recovery ----------

/// A `pi` Attempt whose driver is dead: the abort reuses the existing crash-recovery/A9 path — the Worker's tmux window
/// is killed and its parallel worktree and branch are removed, exactly as a crash's end does.
#[test]
fn pi_window_and_worktree_are_cleaned_through_crash_recovery() {
    let live = live("");
    let plan = PlanId::parse(PLAN).unwrap();
    let attempt = AttemptId::parse(ATTEMPT).unwrap();
    // As a driver leaves a running parallel pi Attempt: its own worktree, cut from the Lead Branch.
    let setup = Setup { timeout: Duration::from_secs(30), log: live.fx.env.root().join("attempt.log") };
    let made = Ports::scope(&live.fx.fakes.ports, || create_attempt_worktree(&live.repo, &live.lead, &plan, &attempt, &setup)).unwrap();
    assert!(made.path.exists(), "the worktree exists before the abort: {}", made.path.display());
    assert_ne!(made.branch, "cpd/pay/repo", "a parallel Attempt, whose worktree a crash's end removes");
    // The Attempt first (its Session is a deferred foreign key), then the Session, then the Attempt's worker_session:
    // the order A3 uses, so both deferred foreign keys resolve.
    live.fixture(&format!(
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'api';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at, runner)
           VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', '{}', '{}', '{}', 't', 'pi');",
        made.branch,
        made.path.display(),
        made.base_sha
    ));
    live.fixture(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
           VALUES ('ws-api-a1', 'worker', 'ctx', 'pay', 'api-a1', 'pay api a1', '/', 'S2', 'working', 't', 't');
         UPDATE attempts SET worker_session = 'ws-api-a1' WHERE plan = 'pay' AND id = 'api-a1';",
    );
    let window = live.fx.tmux(&["new-window", "-t", "cpd-work", "-n", "pay--api-a1", "sleep", "600"]);
    assert!(window.status.success(), "{}", String::from_utf8_lossy(&window.stderr));
    assert!(live.fx.panes().iter().any(|p| p.window_name == "pay--api-a1"), "the Worker window exists before the abort");

    abort(&live, ATTEMPT);

    assert_ended(&live, ATTEMPT, "failed", "aborted");
    assert!(live.fx.panes().iter().all(|p| p.window_name != "pay--api-a1"), "the Worker window is killed");
    assert!(!made.path.exists(), "the worktree is removed: {}", made.path.display());
}
