//! The start half of the Worker runner seam for `durable` (contract §3.4 "The Worker runner and its seam": Start, Liveness and
//! restart, Stop; §6.12), through the driver in this process: with `worker_runner = "durable"` the driver starts the runner as a
//! detached child of its own (no tmux window), watches it by pid, starts it again in place, ends the Attempt `setup_failed` or
//! `crashed` where the contract says, stops it with SIGTERM then SIGKILL, and a reloaded driver follows the Attempt's stored
//! runner, never today's flag.
//!
//! The runner is a process of the `FakeSpawn`: a `Launch` it records and a pid (the driver is pid 1000, so the runner is the next
//! one). Nothing runs: the test is the process, with `FakeSpawn::exit` (its exit status), `FakeSpawn::signals` (what the driver
//! sent) and `FakeSpawn::set_cmdline` (what the guard of a signal reads, which the real `Spawn` reads from the OS). Time is the
//! `ManualClock`: a nap is instant, and the clock says how long the driver waited.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cpd_core::config::catalog;
use cpd_core::home::CpdHome;
use cpd_core::host::spawn::{Launch, Pid, Signal, Spawn};
use cpd_core::ids::{AttemptId, PlanId};
use serde_json::{Value, json};

use super::driver::{JOB, PLAN, seed_home};
use super::driver_open::{Live, live, submit};
use super::driver_reload::{reload, run_until_exec};

const ATTEMPT: &str = "api-a1";
/// The grace of a stop (`timeouts.pi_stop_seconds`), long enough that a driver that does not wait for it cannot be mistaken for one that does.
const GRACE_SECONDS: u64 = 30;
const GRACE: &str = "[timeouts]\npi_stop_seconds = 30";

// ---------- helpers the other test Jobs of this PR reuse ----------

/// A pi adapter directory of this env: `<root>/adapter`, with the runner file `durable-worker/main.ts` when `with_runner`. The
/// runner is never run (the `FakeSpawn` starts nothing): the file only has to exist.
pub(super) fn adapter(live: &Live, with_runner: bool) -> PathBuf {
    let dir = live.fx.env.root().join("adapter");
    fs::create_dir_all(&dir).unwrap();
    // `pi_adapter` must be a package: a directory with a package.json.
    fs::write(dir.join("package.json"), "{}\n").unwrap();
    if with_runner {
        let main = runner_file(&dir);
        fs::create_dir_all(main.parent().unwrap()).unwrap();
        fs::write(main, "// the runner of the test: nothing runs it\n").unwrap();
    }
    dir
}

/// The runner file of an adapter directory (contract §3.4 "Start, `durable`").
pub(super) fn runner_file(adapter: &Path) -> PathBuf {
    adapter.join("durable-worker").join("main.ts")
}

/// The config of `live` from now on: `worker_runner = <runner>` and `pi_adapter = <adapter>`, then `tables` (`[timeouts]` and the
/// like). An Attempt takes its runner from this file when it is dispatched.
pub(super) fn use_runner(live: &Live, runner: &str, adapter: &Path, tables: &str) {
    seed_home(&live.fx.env, &format!("worker_runner = \"{runner}\"\npi_adapter = \"{}\"\n{tables}", adapter.display()));
}

/// Plan `pay` with Job `api` queued, `worker_runner = "durable"` and an adapter directory that holds the runner file. `tables` is
/// more TOML after the keys (`[timeouts]`...).
pub(super) fn durable_live(tables: &str) -> Live {
    let live = live("");
    let adapter = adapter(&live, true);
    use_runner(&live, "durable", &adapter, tables);
    live
}

/// Writes the end report of Attempt `id`'s runner, `run/pay/attempts/<id>.report.json` (contract §6.12 `EndReport`): what the real
/// runner leaves after each settle. `report` is the JSON of the file.
pub(super) fn write_report(live: &Live, id: &str, report: &Value) {
    let path = live.fx.env.cpd_home().join("run").join(PLAN).join("attempts").join(format!("{id}.report.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, report.to_string()).unwrap();
}

/// An `EndReport` that stopped with `stop_reason`, with `turns` assistant messages and no error.
pub(super) fn report(stop_reason: &str, turns: u64) -> Value {
    json!({
        "stop_reason": stop_reason, "error_message": null, "turns": turns,
        "tokens_in": 1, "tokens_out": 1, "cache_read": 0, "cache_write": 0, "cost_usd": 0.0, "model": "p/sonnet"
    })
}

// ---------- what the tests read ----------

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

/// The runner the driver started, as the reader of the Attempt's `worker_pid` finds it: its pid is stored, and it shows its own
/// command line (what the guard of a signal compares, from the OS in production).
fn the_runner(live: &Live, id: &str) -> Pid {
    let launch = the_runners_launch(live);
    let pid = stored_pid(live, id);
    assert_eq!(pid, pid_of_launch(1), "the stored pid is the runner's");
    live.fx.fakes.spawn.set_cmdline(pid, &cmdline_of(&launch));
    pid
}

/// `job cancel` as the Worker's pi runs it, and the engine's E2 that sets the stop request of the Attempt it finds with a cancel.
fn cancel(live: &Live, id: &str) {
    live.fx.cpd_with(&live.worktree(id), &["job", "cancel", JOB, "--reason", "no longer needed"], &[("CPD_PLAN", PLAN), ("CPD_ATTEMPT", id)]).ok();
    live.fixture(&format!("UPDATE attempts SET stop_request = 'cancelled' WHERE plan = 'pay' AND id = '{id}';"));
}

/// A Worker that commits `api.txt` and submits.
fn work(live: &Live, id: &str) -> Value {
    live.commit(&live.worktree(id), "api.txt", "api\n", "api");
    submit(live, id)
}

/// The detail an Attempt's end gave: `attempt-retry`'s `feedback` for a cause of the ladder, else `job-failed`'s `detail`.
fn end_detail(live: &Live, id: &str) -> String {
    let events = live.events(id);
    events
        .iter()
        .find_map(|e| match e["kind"].as_str()? {
            "attempt-retry" => e["data"]["feedback"].as_str(),
            "job-failed" => e["data"]["detail"].as_str(),
            _ => None,
        })
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("no end detail among {events:#?}"))
}

fn assert_ended(live: &Live, id: &str, outcome: &str, cause: &str) {
    let attempt = live.attempt(id);
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!(outcome), &json!(cause)), "{attempt}");
}

/// The Attempt never got a Worker: no `starting -> running` transition, no pid, no window.
fn assert_never_ran(live: &Live, id: &str) {
    let attempt = live.attempt(id);
    assert_eq!(attempt["worker_pid"], Value::Null, "{attempt}");
    let running = live.events(id).into_iter().filter(|e| e["kind"] == "attempt-status" && e["data"]["to"] == "running").count();
    assert_eq!(running, 0, "the Attempt ran: {attempt}");
    assert_eq!(new_windows(live), 0);
}

/// How many windows the driver asked tmux for.
fn new_windows(live: &Live) -> usize {
    live.fx.calls("tmux").iter().filter(|c| c.args.iter().any(|a| a == "new-window")).count()
}

/// The driver's log of Attempt `id`.
fn driver_log(live: &Live, id: &str) -> String {
    fs::read_to_string(home(live).driver_log(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(id).unwrap())).unwrap_or_default()
}

/// The txn of the one `history` row that satisfies `where_`.
fn txn_of(live: &Live, where_: &str) -> i64 {
    live.value(&format!("SELECT txn FROM history WHERE {where_} ORDER BY id LIMIT 1"))
}

/// pi's Node as the driver reads it: the one `config::catalog::node()` finds under this env's `$HOME` (no `XDG_DATA_HOME`).
fn pi_node(live: &Live) -> String {
    let home = live.fx.env.home();
    let (home_s, data_s) = (home.display().to_string(), home.join(".local").join("share").display().to_string());
    live.fx.with_vars(&[("HOME", home_s.as_str()), ("XDG_DATA_HOME", data_s.as_str())], || catalog::node().display().to_string())
}

/// `Live::drive` of Job `api`, with `before` run between `job dispatch` and the driver (the one place a test can change the fakes after
/// the starter spawned the driver): `hook(live, id, n)` is called after the driver's `n`th nap.
fn drive_after(live: &Live, before: impl FnOnce(), mut hook: impl FnMut(&Live, &str, usize)) -> String {
    let id = live.dispatch(JOB).ok()["attempt"].as_str().unwrap().to_owned();
    let launch = live.fx.take_launch(0);
    let lock = home(live).attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(&id).unwrap());
    let _fd3 = live.fx.hand_over(fs::File::open(lock).unwrap());
    before();
    let ran = live.fx.run_launch_with(&launch, |_, nap| hook(live, &id, nap));
    assert_eq!(ran.exit, 0, "{}", ran.stdout);
    id
}

// ---------- start (R5) ----------

#[test]
fn a_durable_attempt_runs_the_runner_as_a_detached_child_and_not_in_a_tmux_window() {
    let live = durable_live("");
    let adapter = adapter(&live, true);
    let cpd_home = live.fx.env.cpd_home();
    let cpd_home_s = cpd_home.display().to_string();
    let mut checked = false;

    let (id, _) = live.drive(JOB, |live, id, nap| {
        if nap != 1 {
            return;
        }
        let attempt = live.attempt(id);
        let session = attempt["worker_session"].as_str().unwrap_or_else(|| panic!("no Session registered: {attempt}")).to_owned();

        // The runner is the only process besides the driver, and the whole `Launch` is the contract's.
        let launch = the_runners_launch(live);
        let spec = home(live).worker_spec(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(id).unwrap());
        assert!(spec.exists(), "the Spec is rendered before the runner starts: {}", spec.display());
        let log = cpd_home.join("run").join(PLAN).join("logs").join(format!("worker-{id}.log"));
        assert_eq!(launch.program, pi_node(live), "pi's Node runs the runner");
        assert_eq!(
            launch.args,
            [runner_file(&adapter).display().to_string(), "--model".into(), "p/sonnet:medium".into(), "--spec".into(), spec.display().to_string()],
            "the argv is the runner file, the Tier's pi model pattern and the Spec"
        );
        assert_eq!(launch.cwd, Some(live.worktree(id)), "the cwd is the Attempt's worktree");
        assert!(launch.setsid, "detached from the driver's session");
        assert!(launch.idle, "at idle priority like a pi Worker");
        assert_eq!(launch.stdout_to, Some(log), "stdout and stderr are appended to the Worker's log");
        assert_eq!((launch.hand_over, launch.foreground), (None, false), "no driver lock, no terminal");
        let env: BTreeMap<String, String> = launch.env_set.iter().cloned().collect();
        let keys: BTreeSet<&str> = env.keys().map(String::as_str).collect();
        assert_eq!(keys, BTreeSet::from(["CPD_HOME", "CPD_SESSION", "CPD_PLAN", "CPD_ATTEMPT", "PI_SKIP_VERSION_CHECK", "PATH"]), "{env:?}");
        assert_eq!((env["CPD_HOME"].as_str(), env["CPD_PLAN"].as_str(), env["CPD_ATTEMPT"].as_str()), (cpd_home_s.as_str(), PLAN, id));
        assert_eq!((env["CPD_SESSION"].as_str(), env["PI_SKIP_VERSION_CHECK"].as_str()), (session.as_str(), "1"));
        assert!(env["PATH"].starts_with(&format!("{cpd_home_s}/bin:")), "$CPD_HOME/bin is first on PATH: {}", env["PATH"]);
        assert!(cpd_home.join("bin/git").exists(), "the git wrapper is installed as for pi");

        // No window, no pane, no `session-moved`: the Session has no pane by design.
        assert_eq!(new_windows(live), 0, "a window was started for the Worker");
        let windows: Vec<String> = live.fx.panes().into_iter().map(|p| p.window_name).collect();
        assert_eq!(windows, ["watch"], "the work session holds no Worker window");
        let pane: Option<String> = live.value(&format!("SELECT tmux_pane FROM sessions WHERE id = '{session}'"));
        assert_eq!(pane, None, "the runner's Session has no pane");
        let moved: i64 = live.value(&format!("SELECT count(*) FROM events WHERE kind = 'session-moved' AND session = '{session}'"));
        assert_eq!(moved, 0, "no `session-moved` Event");

        // `starting -> running` and the pid in one transaction, after the Session's own.
        assert_eq!((attempt["status"].as_str(), attempt["runner"].as_str()), (Some("running"), Some("durable")), "{attempt}");
        let pid = the_runner(live, id);
        let running = txn_of(live, &format!("tbl = 'attempts' AND op = 'update' AND json_extract(after, '$.status') = 'running' AND pk LIKE '%\"{id}\"%'"));
        let stored = txn_of(live, &format!("tbl = 'attempts' AND op = 'update' AND json_extract(after, '$.worker_pid') = {} AND pk LIKE '%\"{id}\"%'", pid.0));
        assert_eq!(running, stored, "`running` and `worker_pid` are one transaction");
        let registered = txn_of(live, &format!("tbl = 'sessions' AND op = 'insert' AND pk LIKE '%{session}%'"));
        assert!(registered < stored, "the Session is registered before the runner starts: txn {registered}, then {stored}");
        checked = true;

        work(live, id);
    });

    assert!(checked, "the driver napped");
    assert_ended(&live, &id, "merged", "merged");
    // The runner ended on the SIGTERM of the stop before the merge: no SIGKILL for a process that is gone.
    assert_eq!(live.fx.fakes.spawn.signals(), [(pid_of_launch(1), Signal::Term)]);
    assert_eq!(live.fx.fakes.spawn.launches().len(), 2, "nothing started the runner again");
}

#[test]
fn a_missing_runner_file_ends_the_attempt_setup_failed_before_any_spawn() {
    let live = live("");
    // An adapter directory with no `durable-worker/main.ts`.
    let adapter = adapter(&live, false);
    use_runner(&live, "durable", &adapter, "");

    let (id, _) = live.drive(JOB, |_, _, nap| panic!("the driver waited for a Worker at nap {nap}: it started one"));

    assert_ended(&live, &id, "failed", "setup_failed");
    assert_eq!(live.fx.fakes.spawn.launches().len(), 1, "the driver's own launch is the only one: nothing was spawned");
    assert_never_ran(&live, &id);
}

#[test]
fn a_spawn_that_fails_ends_the_attempt_setup_failed() {
    let live = durable_live("");

    let id = drive_after(
        &live,
        || live.fx.fakes.spawn.fail_spawn(io::ErrorKind::PermissionDenied),
        |_, _, nap| panic!("the driver waited for a Worker at nap {nap}: its spawn failed"),
    );

    assert_ended(&live, &id, "failed", "setup_failed");
    let launches = live.fx.fakes.spawn.launches();
    assert_eq!(launches.len(), 2, "the driver's launch, and the runner's that failed: {launches:#?}");
    assert!(launches[1].args.iter().any(|a| a.ends_with("durable-worker/main.ts")), "{:?}", launches[1]);
    assert_never_ran(&live, &id);
}

// ---------- liveness and restart (R6) ----------

#[test]
fn a_durable_runner_that_exits_1_is_started_again_up_to_three_times_then_the_attempt_crashes() {
    let live = durable_live("");
    let spawn = live.fx.fakes.spawn.clone();
    let mut first: Option<Launch> = None;

    // Every end but exit 2 is started again: exit 1, exit 143 (SIGTERM handled), death by a signal; the 4th death is the 4th end.
    let (id, _) = live.drive(JOB, |live, id, nap| {
        let launches = spawn.launches();
        let n = nap;
        // The runner of this nap is the (nap)th started: the driver's own launch is the first.
        assert_eq!(launches.len(), n + 1, "nap {nap}: the runner was started again in place: {launches:#?}");
        let pid = pid_of_launch(n);
        assert_eq!(stored_pid(live, id), pid, "nap {nap}: the new pid is stored");
        match &first {
            None => first = Some(launches[1].clone()),
            Some(first) => assert_eq!(&launches[n], first, "nap {nap}: the same launch again, so the store and the conversation resume"),
        }
        assert!(spawn.alive(pid));
        match nap {
            1 | 4 => spawn.exit(pid, 1),
            2 => spawn.exit(pid, 143),
            3 => spawn.signal(pid, Signal::Kill).unwrap(),
            _ => panic!("nap {nap}: the 4th death ended the Attempt"),
        }
    });

    assert_ended(&live, &id, "failed", "crashed");
    assert_eq!(spawn.launches().len(), 5, "the driver and the runner four times: three restarts");
    let log = driver_log(&live, &id);
    assert!(log.lines().filter(|l| l.to_lowercase().contains("restart")).count() >= 3, "each restart is logged: {log}");
}

#[test]
fn a_durable_runner_that_exits_2_ends_the_attempt_crashed_without_a_restart() {
    let live = durable_live("");
    let spawn = live.fx.fakes.spawn.clone();

    // Exit status 2 is a `usage` error: the runner refused its arguments, and the same launch would too.
    let (id, _) = live.drive(JOB, |live, id, nap| {
        assert_eq!(nap, 1, "the Attempt ended at the first look");
        assert_eq!(spawn.launches().len(), 2);
        spawn.exit(the_runner(live, id), 2);
    });

    assert_ended(&live, &id, "failed", "crashed");
    assert_eq!(spawn.launches().len(), 2, "the runner was not started again");
    let log = live.fx.env.cpd_home().join("run").join(PLAN).join("logs").join(format!("worker-{id}.log"));
    let detail = end_detail(&live, &id);
    assert!(detail.contains(&log.display().to_string()), "the detail names the Worker's log file {}: {detail}", log.display());
}

// ---------- stop (R8) ----------

#[test]
fn a_cancel_stops_the_runner_with_term_then_kill_after_the_grace() {
    let live = durable_live(GRACE);
    let spawn = live.fx.fakes.spawn.clone();
    let (mut pid, mut asked_at) = (None, None);

    // The runner ignores SIGTERM: the driver waits `pi_stop_seconds` of clock, then SIGKILLs it.
    let (id, _) = live.drive(JOB, |live, id, nap| {
        if nap == 1 {
            let runner = the_runner(live, id);
            spawn.survive(runner, Signal::Term);
            pid = Some(runner);
            asked_at = Some(live.fx.fakes.clock.elapsed());
            cancel(live, id);
            return;
        }
        // A nap of the grace: SIGTERM is sent, the runner lives on, and nothing else is sent yet.
        let pid = pid.expect("the cancel is the first nap");
        assert_eq!(spawn.signals(), [(pid, Signal::Term)], "nap {nap}");
        assert!(spawn.alive(pid), "nap {nap}");
    });

    let pid = pid.unwrap();
    assert_ended(&live, &id, "cancelled", "cancel_requested");
    assert_eq!(spawn.signals(), [(pid, Signal::Term), (pid, Signal::Kill)], "SIGTERM, then SIGKILL");
    assert!(!spawn.alive(pid));
    let waited = live.fx.fakes.clock.elapsed() - asked_at.unwrap();
    assert!(waited >= std::time::Duration::from_secs(GRACE_SECONDS), "SIGKILL came after {waited:?} of clock, before the {GRACE_SECONDS} s of timeouts.pi_stop_seconds");
}

/// A signal goes to a stored `worker_pid` only when its command line holds the runner file and this Attempt's Spec path: the pid may
/// have been reused by another process, or be another Attempt's runner (contract §3.4 "Stop").
#[test]
fn a_stop_sends_no_signal_to_a_pid_that_is_not_the_runner_of_this_attempt() {
    // (what the pid shows as its command line, given the runner file and the Spec of this Attempt)
    type Shown = fn(&str, &str) -> String;
    let others: [(&str, Shown); 3] = [
        ("an unrelated process", |_, _| "sleep 100".to_owned()),
        ("the runner of another Attempt", |runner, spec| format!("node {runner} --model p/sonnet:medium --spec {}", spec.replace("api-a1", "api-a2"))),
        ("another program on this Attempt's Spec", |_, spec| format!("node /elsewhere/main.ts --model p/sonnet:medium --spec {spec}")),
    ];
    for (what, shown) in others {
        let live = durable_live(GRACE);
        let adapter = adapter(&live, true);
        let spawn = live.fx.fakes.spawn.clone();
        let mut pid = None;

        let (id, _) = live.drive(JOB, |live, id, nap| {
            if nap == 1 {
                let runner = stored_pid(live, id);
                let spec = home(live).worker_spec(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(id).unwrap());
                spawn.set_cmdline(runner, &shown(&runner_file(&adapter).display().to_string(), &spec.display().to_string()));
                pid = Some(runner);
                cancel(live, id);
            }
        });

        assert_ended(&live, &id, "cancelled", "cancel_requested");
        assert_eq!(spawn.signals(), [], "{what}: a signal went to pid {:?}", pid);
        drop(live);
    }
}

// ---------- reload (R3) ----------

#[test]
fn a_reloaded_driver_resumes_a_pi_attempt_as_pi_when_the_config_now_says_durable() {
    let re = reload();
    let adapter = adapter(&re.live, true);
    use_runner(&re.live, "pi", &adapter, "");
    let old_lead = re.live.lead_sha();
    let launch = re.dispatch();

    let (exec, _fd3) = run_until_exec(&re, &launch, |_, nap| {
        if nap == 1 {
            assert_eq!(re.fx().fakes.spawn.launches().len(), 1, "a pi Worker runs in a window: the driver is the only process");
            assert_eq!(new_windows(&re.live), 1);
            // The flag moves while the driver runs, and a new cpd is installed.
            use_runner(&re.live, "durable", &adapter, "");
            re.install();
        }
    });
    re.assert_exec_of_the_driver(&exec, Pid(1000));

    re.run_new(exec, |_, nap| {
        if nap == 1 {
            re.submit();
        }
    });

    re.assert_merged(&old_lead);
    let attempt = re.live.attempt(ATTEMPT);
    assert_eq!((&attempt["runner"], &attempt["worker_pid"]), (&json!("pi"), &Value::Null), "{attempt}");
    // Still pi: the Worker's window, no runner, no signal.
    assert_eq!(new_windows(&re.live), 1, "the reloaded driver started no other window");
    assert_eq!(re.fx().fakes.spawn.launches().len(), 2, "the driver and its exec: no runner was started");
    assert_eq!(re.fx().fakes.spawn.signals(), []);
}

#[test]
fn a_reloaded_driver_resumes_a_durable_attempt_by_its_stored_pid_when_the_config_now_says_pi() {
    let re = reload();
    let adapter = adapter(&re.live, true);
    use_runner(&re.live, "durable", &adapter, "");
    let old_lead = re.live.lead_sha();
    let launch = re.dispatch();
    let mut runner = None;

    let (exec, _fd3) = run_until_exec(&re, &launch, |_, nap| {
        if nap == 1 {
            runner = Some(the_runner(&re.live, ATTEMPT));
            // The flag moves while the driver runs (the adapter stays: the runner file is where it was), and a new cpd is installed.
            use_runner(&re.live, "pi", &adapter, "");
            re.install();
        }
    });
    let runner = runner.expect("the driver napped");
    re.assert_exec_of_the_driver(&exec, Pid(1000));
    assert!(re.fx().fakes.spawn.alive(runner), "the reload leaves the runner running");
    assert_eq!(re.fx().fakes.spawn.launches().len(), 3, "the driver, the runner and the exec");

    re.run_new(exec, |_, nap| {
        if nap == 1 {
            assert!(re.fx().fakes.spawn.alive(runner), "the new driver found the runner alive by its stored pid");
            re.submit();
        }
    });

    re.assert_merged(&old_lead);
    let attempt = re.live.attempt(ATTEMPT);
    assert_eq!(attempt["runner"], "durable", "{attempt}");
    assert_eq!(attempt["worker_pid"], runner.0, "{attempt}");
    // Still durable: the same runner, stopped by its stored pid (SIGTERM, which it obeyed) before the merge; no window, no second runner.
    assert_eq!(re.fx().fakes.spawn.signals(), [(runner, Signal::Term)]);
    assert_eq!(re.fx().fakes.spawn.launches().len(), 3, "the reloaded driver started no runner");
    assert_eq!(new_windows(&re.live), 0);
}
