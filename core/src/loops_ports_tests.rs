//! The rest of the core loops on virtual time (docs/implementing.md, "Time in tests"): each wait of this file reads
//! its time and takes its nap on the `Clock` of a `Ports`, and runs its processes on the `FakeRunner` and the
//! `FakeSpawn` of that `Ports`. No test sleeps, starts a process or reads the wall clock: a nap is a line of
//! `ManualClock::naps`, and what happens "between two polls" is a hook the test hangs on the clock (`Hooked`), run
//! after the n-th nap, which writes through a second handle on the same store or lets go of a lock.
//!
//! The stubs these tests call are `todo!()`: `events::follow_in`, `ask::question::ask_in`,
//! `plan::cancel::wait_until_ended_with`, `pr::rebase::settle_in`, `pr::poll::take_lock_with`,
//! `inbox::rules::conditions_on`, `inbox::cmd::snooze_until_on`, `secret::grants::{approve_on, take_approval_on}`,
//! `context::distill::run_pi_timed_with`. The old names call them on `Ports::real()` once implemented.
//!
//! Ported assertions (old -> new): `cli/tests/flow/ask.rs` (an `ask` child the test answers, and the one that times
//! out) -> `ask_wakes_on_an_answer_that_lands_between_polls` and `ask_expires_at_its_deadline_in_half_second_polls`;
//! `cli/tests/flow/plan_cancel.rs` (the wait for the drivers) -> the `plan_cancel_*` tests.
//!
//! The memo of GitHub's answer (`inbox::rules::GITHUB`) is process-wide: each rules test uses a pull request of its
//! own, so none reads another's answer, under nextest (a process per test) or not.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::datetime;

use crate::clock::{Clock, ManualClock};
use crate::context::distill::{Waited, run_pi_timed_with};
use crate::driver::lock::take_free_driver_lock;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{FollowArgs, NewEvent, Scope, append, highest_id};
use crate::home::CpdHome;
use crate::host::pi::Launch as PiLaunch;
use crate::host::runner::{FakeRunner, Reply};
use crate::host::spawn::FakeSpawn;
use crate::ids::{AttemptId, PlanId, WorkflowKey};
use crate::inbox::cmd::{Until, snooze_until_on};
use crate::inbox::model::Kind;
use crate::inbox::rules::conditions_on;
use crate::lock;
use crate::output::Output;
use crate::plan::cancel::wait_until_ended_with;
use crate::ports::Ports;
use crate::pr::poll::take_lock_with;
use crate::pr::rebase::settle_in;
use crate::secret::grants::{Approval, approve_on, take_approval_on};
use crate::store::Store;

const MS: fn(u64) -> Duration = Duration::from_millis;
const SECS: fn(u64) -> Duration = Duration::from_secs;

// ---- the rig: virtual time with a hook after each nap ----------------------------------------------------------------

/// A clock that is a `ManualClock` and calls `hook(n)` after its n-th nap (1-based): what a test does "between two
/// polls". The hook runs on the thread that napped, so there is no thread, no race and no real time.
struct Hooked {
    inner: ManualClock,
    hook: Mutex<Box<dyn FnMut(usize) + Send>>,
}

impl Clock for Hooked {
    fn now(&self) -> Instant {
        self.inner.now()
    }
    fn sleep(&self, d: Duration) {
        self.inner.sleep(d);
        let n = self.inner.naps().len();
        (self.hook.lock().expect("the hook never panics while held"))(n);
    }
    fn utc(&self) -> OffsetDateTime {
        self.inner.utc()
    }
}

/// The `Ports` of a test and the typed handles on what it fakes.
struct Rig {
    ports: Ports,
    clock: ManualClock,
    runner: Arc<FakeRunner>,
    spawn: Arc<FakeSpawn>,
}

fn rig() -> Rig {
    rig_hooked(|_| {})
}

fn rig_hooked(hook: impl FnMut(usize) + Send + 'static) -> Rig {
    let clock = ManualClock::new();
    let runner = Arc::new(FakeRunner::new(clock.clone()));
    let spawn = Arc::new(FakeSpawn::new());
    let hooked = Arc::new(Hooked {
        inner: clock.clone(),
        hook: Mutex::new(Box::new(hook)),
    });
    Rig {
        ports: Ports {
            clock: hooked,
            runner: runner.clone(),
            spawn: spawn.clone(),
            ..Ports::real()
        },
        clock,
        runner,
        spawn,
    }
}

impl Rig {
    /// Adds a rule to the runner inside `ports` (a clone of a `FakeRunner` shares its script).
    fn on(&self, program: &str, reply: Reply) {
        let _ = (*self.runner).clone().on(program, |_| true, reply);
    }
}

// ---- fixtures: a temp `$CPD_HOME`, its store, a Plan with a running Attempt ---------------------------------------------

struct Home {
    _dir: tempfile::TempDir,
    home: CpdHome,
}

/// A temporary `$CPD_HOME` whose `config.toml` is `version = 1`, a machine and `config`.
fn home(config: &str) -> Home {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    fs::write(
        home.config(),
        format!("version = 1\nmachine = \"m1\"\n{config}"),
    )
    .unwrap();
    Home { _dir: dir, home }
}

fn store(home: &CpdHome) -> Store {
    Store::open(home, 5000).unwrap()
}

fn env(rig: &Rig, home: &Home) -> Env {
    Env::open_home_with(&rig.ports, home.home.clone()).unwrap()
}

fn sql(store: &mut Store, sql: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok(())
        })
        .unwrap();
}

/// Plan `pl` (Lead `lead-pl`), its open Workflow `repo`, Job `a` running, and Attempt `a-a1` in `status` (`running` or `merging`)
/// with Worker Session `w1` `working`.
fn seed(store: &mut Store, status: &str) {
    let submitted = if status == "merging" {
        ", submitted_sha, submitted_at"
    } else {
        ""
    };
    let submitted_values = if status == "merging" {
        ", 'abc', 't'"
    } else {
        ""
    };
    sql(
        store,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Context', 't', 'm1');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
             VALUES ('pl', 'ctx', 'Plan', 'grounding', 'lead-pl', 't');
             INSERT INTO pull_requests (id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, mergeable, merge_state, authored, review_requested)
             VALUES (7, 'o/r', 7, 'https://example/7', 'Ship', 0, 'open', 'none', 'none', 't', NULL, 't', 'unknown', 'unknown', 0, 0);
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at)
             VALUES ('pl', 'repo', '/repo', 'main', 'open', 'cpd/pl', '/lead', 7, 'https://example/7', 't', 7, cpd_txn(), 't', 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
             VALUES ('lead-pl', 'lead', 'ctx', 'pl', 'pl', '/repo', 'O3', 'idle', 't', 't');
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
             VALUES ('pl', 'repo', 'a', 'A', 'S2', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', 'draft', 0, NULL, cpd_txn(), 't');
             UPDATE jobs SET status = 'queued', fingerprint = '{fp}' WHERE key = 'a';
             UPDATE jobs SET status = 'running' WHERE key = 'a';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at{submitted})
             VALUES ('pl', 'a-a1', 'a', 1, 'S2', 'ladder', '{status}', 'cpd/pl--a-a1', '/w', 'w1', 't'{submitted_values});
             INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
             VALUES ('w1', 'worker', 'ctx', 'pl', 'a-a1', 'a-a1', '/w', 'S2', 'working', 't', 't');",
            fp = "0".repeat(64)
        ),
    );
}

/// What a driver's end writes: `a-a1` is `ended`.
const END_ATTEMPT: &str = "UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE id = 'a-a1'";

fn plan() -> PlanId {
    PlanId::parse("pl").unwrap()
}

fn attempt() -> AttemptId {
    AttemptId::parse("a-a1").unwrap()
}

fn naps_of(rig: &Rig) -> Vec<Duration> {
    rig.clock.naps()
}

fn success(out: Output) -> serde_json::Map<String, Value> {
    match out {
        Output::Success(map) => map,
        Output::Owned => panic!("expected a success envelope, got Owned"),
    }
}

fn refused(result: Result<impl std::fmt::Debug, CpdError>) -> CpdError {
    result.expect_err("expected a refusal")
}

// ---- events follow ---------------------------------------------------------------------------------------------------

/// A writer that keeps its lines and fails with `BrokenPipe` at the write that would be the `keep + 1`-th: the way a
/// reader goes away, so that a loop that never ends by itself ends.
struct Sink {
    lines: Arc<Mutex<String>>,
    keep: usize,
}

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut held = self.lines.lock().unwrap();
        if held.lines().count() >= self.keep {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        held.push_str(&String::from_utf8_lossy(bytes));
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn follow_args(since: Option<i64>, until_idle: bool) -> FollowArgs {
    FollowArgs {
        scope: Scope {
            plan: None,
            context: None,
            session: None,
        },
        kinds: Vec::new(),
        since,
        until_idle,
    }
}

/// Appends an `error` Event whose text is `text`, through `store`.
fn event(store: &mut Store, text: &str) {
    store
        .write("fixture", |tx| {
            append(
                tx,
                NewEvent::error(text, "fixture", "internal", "m", json!({})),
            )?;
            Ok(())
        })
        .unwrap();
}

fn texts(lines: &Arc<Mutex<String>>) -> Vec<String> {
    let held = lines.lock().unwrap();
    held.lines()
        .map(|l| {
            serde_json::from_str::<Value>(l).unwrap()["text"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

/// A follower that started with `old` already in the store; `row A` is committed after the 2nd nap and `row B` after the
/// 3rd, and the sink keeps one line. Returns what it printed and the naps.
fn followed_two_rows() -> (Vec<String>, Vec<Duration>) {
    let h = home("[events]\npoll_ms = 250\n");
    event(&mut store(&h.home), "old");
    // The hook opens its own handle on the store: a `Store` is not `Send`.
    let home = h.home.clone();
    let rig = rig_hooked(move |n| match n {
        2 => event(&mut store(&home), "row A"),
        3 => event(&mut store(&home), "row B"),
        _ => {}
    });
    let mut env = env(&rig, &h);
    let lines = Arc::new(Mutex::new(String::new()));
    let out = crate::events::follow_in(
        &mut env,
        follow_args(None, false),
        &mut Sink {
            lines: lines.clone(),
            keep: 1,
        },
    )
    .unwrap();
    assert!(
        matches!(out, Output::Owned),
        "a reader that goes away ends the follower without an error"
    );
    (texts(&lines), naps_of(&rig))
}

#[test]
fn follow_prints_only_the_rows_committed_after_it_started() {
    let (printed, _) = followed_two_rows();
    assert_eq!(printed, ["row A"]);
}

#[test]
fn follow_naps_the_configured_poll_between_polls_until_the_row_that_ends_it() {
    let (_, naps) = followed_two_rows();
    assert_eq!(naps, vec![MS(250); 3]);
}

#[test]
fn follow_until_idle_prints_what_was_committed_at_the_start_and_never_naps() {
    let h = home("");
    let mut writer = store(&h.home);
    event(&mut writer, "first");
    event(&mut writer, "second");
    let rig = rig();
    let mut env = env(&rig, &h);
    let lines = Arc::new(Mutex::new(String::new()));
    let out = crate::events::follow_in(
        &mut env,
        follow_args(Some(0), true),
        &mut Sink {
            lines: lines.clone(),
            keep: usize::MAX,
        },
    )
    .unwrap();
    assert!(matches!(out, Output::Owned));
    assert_eq!(texts(&lines), ["first", "second"]);
    assert!(
        naps_of(&rig).is_empty(),
        "--until-idle returns once it has printed: {:?}",
        naps_of(&rig)
    );
}

#[test]
fn follow_whose_binary_is_replaced_restarts_through_spawn_from_after_the_last_row() {
    let h = home("[events]\npoll_ms = 250\n");
    let mut writer = store(&h.home);
    event(&mut writer, "old");
    let cursor = writer.read(highest_id).unwrap();
    // The installed cpd: a file that a new install replaces by a rename, so the path holds another inode.
    let bin_dir = tempfile::tempdir().unwrap();
    let exe = bin_dir.path().join("cpd");
    fs::write(&exe, "v1").unwrap();
    let (swap_from, swap_to) = (bin_dir.path().join("cpd.new"), exe.clone());
    let rig = rig_hooked(move |n| {
        if n == 1 {
            fs::write(&swap_from, "v2").unwrap();
            fs::rename(&swap_from, &swap_to).unwrap();
        }
    });
    rig.spawn.set_self_exe(&exe);
    let mut env = env(&rig, &h);
    let lines = Arc::new(Mutex::new(String::new()));
    let err = refused(crate::events::follow_in(
        &mut env,
        follow_args(None, false),
        &mut Sink {
            lines,
            keep: usize::MAX,
        },
    ));
    assert_eq!(
        err.code(),
        ErrorCode::SpawnFailed,
        "the fake exec returns, as an exec that failed does: {err:?}"
    );
    assert_eq!(naps_of(&rig), vec![MS(250)]);
    let launches = rig.spawn.launches();
    assert_eq!(launches.len(), 1, "{launches:?}");
    assert_eq!(Path::new(&launches[0].program), exe);
    assert_eq!(
        launches[0].args[launches[0].args.len() - 2..],
        ["--since".to_owned(), cursor.to_string()]
    );
}

// ---- ask ---------------------------------------------------------------------------------------------------------------

fn ask_env(rig: &Rig, config: &str) -> (Home, Env) {
    let h = home(config);
    seed(&mut store(&h.home), "running");
    let env = env(rig, &h);
    (h, env)
}

fn status_of_escalation(env: &mut Env) -> String {
    env.store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT status FROM escalations WHERE id = 'a-a1-q1'",
                [],
                |r| r.get::<_, String>(0),
            )?)
        })
        .unwrap()
}

#[test]
fn ask_expires_at_its_deadline_in_half_second_polls() {
    let rig = rig();
    let (_h, mut env) = ask_env(&rig, "");
    let err = refused(crate::ask::question::ask_in(
        &mut env,
        &plan(),
        &attempt(),
        "which port?",
        Some(1),
    ));
    assert_eq!(err.code(), ErrorCode::AskTimeout);
    assert_eq!(naps_of(&rig), vec![MS(500); 120]);
}

#[test]
fn an_ask_that_timed_out_leaves_its_escalation_expired() {
    let rig = rig();
    let (_h, mut env) = ask_env(&rig, "");
    let _ = crate::ask::question::ask_in(&mut env, &plan(), &attempt(), "which port?", Some(1));
    assert_eq!(status_of_escalation(&mut env), "expired");
}

#[test]
fn ask_with_a_zero_timeout_expires_at_the_first_poll_without_a_nap() {
    let rig = rig();
    let (_h, mut env) = ask_env(&rig, "");
    let err = refused(crate::ask::question::ask_in(
        &mut env,
        &plan(),
        &attempt(),
        "which port?",
        Some(0),
    ));
    assert_eq!(err.code(), ErrorCode::AskTimeout);
    assert!(naps_of(&rig).is_empty());
}

#[test]
fn ask_without_a_timeout_waits_the_configured_ask_minutes() {
    let rig = rig();
    let (_h, mut env) = ask_env(&rig, "[timeouts]\nask_minutes = 2\n");
    let err = refused(crate::ask::question::ask_in(
        &mut env,
        &plan(),
        &attempt(),
        "which port?",
        None,
    ));
    assert_eq!(err.code(), ErrorCode::AskTimeout);
    assert_eq!(rig.clock.elapsed(), SECS(120));
}

/// `ask` with the answer written by another handle on the store after the 3rd nap.
fn answered_after_three_naps() -> (Output, Vec<Duration>) {
    let h = home("");
    seed(&mut store(&h.home), "running");
    let home = h.home.clone();
    let rig = rig_hooked(move |n| {
        if n == 3 {
            sql(
                &mut store(&home),
                "UPDATE escalations SET status = 'answered', answer = 'go ahead', answered_at = 't' WHERE status = 'open'",
            );
        }
    });
    let mut env = env(&rig, &h);
    let out = crate::ask::question::ask_in(&mut env, &plan(), &attempt(), "which port?", Some(60))
        .unwrap();
    (out, naps_of(&rig))
}

#[test]
fn ask_wakes_on_an_answer_that_lands_between_polls() {
    let (out, _) = answered_after_three_naps();
    assert_eq!(success(out)["answer"], "go ahead");
}

#[test]
fn ask_stops_polling_at_the_poll_that_finds_the_answer() {
    let (_, naps) = answered_after_three_naps();
    assert_eq!(naps, vec![MS(500); 3]);
}

// ---- plan cancel: the wait for the drivers --------------------------------------------------------------------------

fn waiting_store(status: &str) -> (Home, Store) {
    let h = home("");
    let mut s = store(&h.home);
    seed(&mut s, status);
    (h, s)
}

#[test]
fn plan_cancel_does_not_nap_when_no_attempt_is_live() {
    let rig = rig();
    let (_h, mut s) = waiting_store("running");
    sql(&mut s, END_ATTEMPT);
    wait_until_ended_with(&rig.ports, &mut s, &plan(), SECS(60), MS(250)).unwrap();
    assert!(naps_of(&rig).is_empty());
}

#[test]
fn plan_cancel_polls_at_its_cadence_until_the_drivers_have_ended() {
    let h = home("");
    let mut s = store(&h.home);
    seed(&mut s, "running");
    let home = h.home.clone();
    let rig = rig_hooked(move |n| {
        if n == 4 {
            sql(&mut store(&home), END_ATTEMPT);
        }
    });
    wait_until_ended_with(&rig.ports, &mut s, &plan(), SECS(60), MS(250)).unwrap();
    assert_eq!(naps_of(&rig), vec![MS(250); 4]);
}

#[test]
fn plan_cancel_times_out_after_its_limit_in_polls_of_its_cadence() {
    let rig = rig();
    let (_h, mut s) = waiting_store("running");
    let err = refused(wait_until_ended_with(
        &rig.ports,
        &mut s,
        &plan(),
        SECS(2),
        MS(250),
    ));
    assert_eq!(err.code(), ErrorCode::CancelTimeout);
    assert_eq!(naps_of(&rig), vec![MS(250); 8]);
}

#[test]
fn the_plan_cancel_timeout_names_the_attempts_still_live() {
    let rig = rig();
    let (_h, mut s) = waiting_store("running");
    let err = refused(wait_until_ended_with(
        &rig.ports,
        &mut s,
        &plan(),
        SECS(2),
        MS(250),
    ));
    assert_eq!(err.details()["attempts"], json!(["a-a1"]));
}

// ---- pr rebase: guard U's wait for a live driver ------------------------------------------------------------------------

/// Plan `pl` whose Attempt `a-a1` is `status`, with a live driver (its lock held for as long as the value lives).
fn guarded(status: &str) -> (Home, Store, impl Sized) {
    let h = home("");
    let mut s = store(&h.home);
    seed(&mut s, status);
    let driver = take_free_driver_lock(&h.home, &plan(), &attempt())
        .unwrap()
        .expect("no driver holds the lock yet");
    (h, s, driver)
}

fn settle(rig: &Rig, h: &Home) -> Result<(), CpdError> {
    let mut env = env(rig, h);
    settle_in(&mut env, &mut None, &plan(), &WorkflowKey::parse("repo").unwrap(), &attempt())
}

#[test]
fn guard_u_does_not_nap_when_the_attempt_is_no_longer_merging() {
    let rig = rig();
    let (h, mut s, _driver) = guarded("merging");
    sql(&mut s, END_ATTEMPT);
    settle(&rig, &h).unwrap();
    assert!(naps_of(&rig).is_empty());
}

#[test]
fn guard_u_reads_the_store_every_200_ms_until_the_driver_records_its_merge() {
    let h = home("");
    seed(&mut store(&h.home), "merging");
    let _driver = take_free_driver_lock(&h.home, &plan(), &attempt())
        .unwrap()
        .expect("no driver holds the lock yet");
    let home = h.home.clone();
    let rig = rig_hooked(move |n| {
        if n == 3 {
            sql(&mut store(&home), END_ATTEMPT);
        }
    });
    settle(&rig, &h).unwrap();
    assert_eq!(naps_of(&rig), vec![MS(200); 3]);
}

#[test]
fn guard_u_gives_up_after_60_seconds_of_polls() {
    let rig = rig();
    let (h, _s, _driver) = guarded("merging");
    let err = refused(settle(&rig, &h));
    assert_eq!(err.code(), ErrorCode::CasLost);
    assert_eq!(naps_of(&rig), vec![MS(200); 300]);
}

#[test]
fn the_guard_u_timeout_names_the_attempt() {
    let rig = rig();
    let (h, _s, _driver) = guarded("merging");
    let err = refused(settle(&rig, &h));
    assert_eq!(err.details()["attempt"], "a-a1");
}

// ---- pr poll: the lock wait ----------------------------------------------------------------------------------------------

fn poll_lock_path(h: &Home) -> PathBuf {
    h.home.root().join("locks").join("pr-poll.lock")
}

/// Another `pr poll`'s hold on `path`.
fn held(path: &Path) -> File {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap();
    assert!(
        lock::try_lock(&file).unwrap(),
        "the test takes the free lock first"
    );
    file
}

#[test]
fn the_poll_lock_is_taken_without_a_nap_when_it_is_free() {
    let rig = rig();
    let h = home("");
    take_lock_with(&rig.ports, &poll_lock_path(&h)).unwrap();
    assert!(naps_of(&rig).is_empty());
}

#[test]
fn the_poll_lock_is_tried_every_100_ms_until_its_holder_lets_go() {
    let h = home("");
    let path = poll_lock_path(&h);
    let mut holder = Some(held(&path));
    let rig = rig_hooked(move |n| {
        if n == 5 {
            holder.take();
        }
    });
    take_lock_with(&rig.ports, &path).unwrap();
    assert_eq!(naps_of(&rig), vec![MS(100); 5]);
}

#[test]
fn the_poll_lock_times_out_after_60_seconds_of_tries() {
    let rig = rig();
    let h = home("");
    let path = poll_lock_path(&h);
    let _holder = held(&path);
    let err = refused(take_lock_with(&rig.ports, &path));
    assert_eq!(err.code(), ErrorCode::LockTimeout);
    assert_eq!(naps_of(&rig), vec![MS(100); 600]);
}

#[test]
fn the_poll_lock_timeout_says_how_many_seconds_it_waited() {
    let rig = rig();
    let h = home("");
    let path = poll_lock_path(&h);
    let _holder = held(&path);
    let err = refused(take_lock_with(&rig.ports, &path));
    assert_eq!(err.details()["waited_seconds"], 60);
}

// ---- the inbox rules: the review window and the GitHub memo -------------------------------------------------------------

const TEAM: &str = "[inbox]\nteam_channel = \"C02JU6C2Z1D\"\nslack_user_id = \"U02ABCDEF12\"\n";

/// A primary post of `repo#number`, `age` seconds before the clock's UTC time.
fn post(rig: &Rig, repo: &str, number: u64, age: u64) -> Value {
    let ts = rig.clock.utc().unix_timestamp() as f64 - age as f64;
    json!({"repo": repo, "number": number, "ts": ts, "ts_raw": "1.1", "author": "Ryan Case", "primary": true,
           "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false})
}

fn write_cache(h: &Home, posts: &[Value]) {
    let path = h.home.run_dir().join("team-review.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, json!({"version": 1, "polled_at": "t", "channel": "C02JU6C2Z1D", "read": 10, "posts": posts}).to_string()).unwrap();
}

/// What `gh api graphql` prints for one open pull request nobody has reviewed.
fn gh_answer() -> Reply {
    Reply::ok(
        &json!({"data": {"viewer": {"login": "me"},
                         "p0": {"pullRequest": {"title": "Add retries", "isDraft": false, "state": "OPEN", "reviewDecision": null,
                                                "author": {"login": "ryan"}, "latestReviews": {"nodes": []}}}}})
        .to_string(),
    )
}

fn review_items(rig: &Rig, h: &Home, s: &mut Store) -> Vec<String> {
    conditions_on(&rig.ports, s, &h.home)
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == Kind::ReviewRequested)
        .map(|c| c.subject)
        .collect()
}

fn gh_calls(rig: &Rig) -> usize {
    rig.runner
        .calls()
        .iter()
        .filter(|c| Path::new(&c.program).file_name().is_some_and(|n| n == "gh"))
        .count()
}

#[test]
fn a_post_inside_the_window_opens_a_review_item() {
    let rig = rig();
    let h = home(TEAM);
    write_cache(&h, &[post(&rig, "acme/win-in", 1, 3600)]);
    rig.on("gh", gh_answer());
    assert_eq!(
        review_items(&rig, &h, &mut store(&h.home)),
        ["pr:acme/win-in#1"]
    );
}

#[test]
fn a_post_the_clock_has_aged_past_the_window_opens_nothing_and_asks_nothing() {
    let rig = rig();
    let h = home(TEAM);
    write_cache(&h, &[post(&rig, "acme/win-out", 2, 3600)]);
    rig.on("gh", gh_answer());
    // The window is a week: eight days later the same post is outside it.
    rig.clock.advance(SECS(8 * 24 * 3600));
    assert_eq!(
        review_items(&rig, &h, &mut store(&h.home)),
        Vec::<String>::new()
    );
    assert_eq!(gh_calls(&rig), 0);
}

#[test]
fn the_github_answer_is_reused_while_it_is_younger_than_its_ttl() {
    let rig = rig();
    let h = home(TEAM);
    write_cache(&h, &[post(&rig, "acme/ttl-in", 3, 3600)]);
    rig.on("gh", gh_answer());
    let mut s = store(&h.home);
    review_items(&rig, &h, &mut s);
    rig.clock.advance(SECS(299));
    review_items(&rig, &h, &mut s);
    assert_eq!(gh_calls(&rig), 1);
}

#[test]
fn the_github_answer_is_asked_again_once_its_ttl_has_passed() {
    let rig = rig();
    let h = home(TEAM);
    write_cache(&h, &[post(&rig, "acme/ttl-out", 4, 3600)]);
    rig.on("gh", gh_answer());
    let mut s = store(&h.home);
    review_items(&rig, &h, &mut s);
    rig.clock.advance(SECS(300));
    review_items(&rig, &h, &mut s);
    assert_eq!(gh_calls(&rig), 2);
}

// ---- inbox snooze: "tomorrow" ----------------------------------------------------------------------------------------------

#[test]
fn snooze_15_minutes_counts_from_the_clocks_utc_time() {
    let rig = rig();
    rig.clock.advance(SECS(3600));
    assert_eq!(
        snooze_until_on(&rig.ports, Until::Minutes15).unwrap(),
        datetime!(2026-01-01 01:15 UTC)
    );
}

#[test]
fn snooze_an_hour_counts_from_the_clocks_utc_time() {
    let rig = rig();
    assert_eq!(
        snooze_until_on(&rig.ports, Until::Hour1).unwrap(),
        datetime!(2026-01-01 01:00 UTC)
    );
}

#[test]
fn snooze_tomorrow_before_nine_local_is_nine_local_today() {
    let rig = rig();
    rig.on("date", Reply::ok("+0200\n"));
    // 00:00 UTC is 02:00 at +0200: 09:00 local is 07:00 UTC the same day.
    assert_eq!(
        snooze_until_on(&rig.ports, Until::Tomorrow).unwrap(),
        datetime!(2026-01-01 07:00 UTC)
    );
}

#[test]
fn snooze_tomorrow_after_nine_local_is_nine_local_the_next_day() {
    let rig = rig();
    rig.on("date", Reply::ok("+0200\n"));
    // 08:00 UTC is 10:00 at +0200.
    rig.clock.advance(SECS(8 * 3600));
    assert_eq!(
        snooze_until_on(&rig.ports, Until::Tomorrow).unwrap(),
        datetime!(2026-01-02 07:00 UTC)
    );
}

#[test]
fn snooze_tomorrow_asks_date_for_the_offset_through_the_runner() {
    let rig = rig();
    rig.on("date", Reply::ok("+0200\n"));
    snooze_until_on(&rig.ports, Until::Tomorrow).unwrap();
    let calls = rig.runner.calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].args, ["+%z"]);
}

// ---- secret: how long an approval lasts ---------------------------------------------------------------------------------------

fn approval() -> Approval {
    Approval {
        session: "s1".into(),
        action: "get".into(),
        vault: "other".into(),
        item: Some("i".into()),
        field: Some("f".into()),
        at: String::new(),
    }
}

#[test]
fn an_approval_expires_ten_minutes_after_the_clocks_utc_time() {
    let rig = rig();
    let h = home("");
    rig.clock.advance(SECS(3600));
    assert_eq!(
        approve_on(&rig.ports, &h.home, &approval()).unwrap(),
        datetime!(2026-01-01 01:10 UTC)
    );
}

#[test]
fn an_approval_is_taken_while_the_clock_is_inside_its_ten_minutes() {
    let rig = rig();
    let h = home("");
    approve_on(&rig.ports, &h.home, &approval()).unwrap();
    rig.clock.advance(SECS(599));
    assert!(take_approval_on(&rig.ports, &h.home, &approval()).unwrap());
}

#[test]
fn an_approval_is_refused_once_the_clock_reaches_its_ten_minutes() {
    let rig = rig();
    let h = home("");
    approve_on(&rig.ports, &h.home, &approval()).unwrap();
    rig.clock.advance(SECS(600));
    assert!(!take_approval_on(&rig.ports, &h.home, &approval()).unwrap());
}

#[test]
fn an_approval_is_taken_once() {
    let rig = rig();
    let h = home("");
    approve_on(&rig.ports, &h.home, &approval()).unwrap();
    assert!(take_approval_on(&rig.ports, &h.home, &approval()).unwrap());
    assert!(!take_approval_on(&rig.ports, &h.home, &approval()).unwrap());
}

// ---- context distill: the run's time ------------------------------------------------------------------------------------------

const PI_LIMIT: Duration = Duration::from_secs(15 * 60);

fn pi_launch() -> PiLaunch {
    PiLaunch {
        argv: ["pi", "--mode", "json"].map(str::to_owned).to_vec(),
        env: Default::default(),
    }
}

fn run_pi(rig: &Rig) -> crate::context::distill::Timed {
    run_pi_timed_with(
        &rig.ports,
        &pi_launch(),
        Path::new("/digest"),
        Path::new("/digest/distill.log"),
        PI_LIMIT,
        &[],
    )
    .unwrap()
}

#[test]
fn a_distill_run_is_stamped_with_the_clocks_utc_time_at_its_start() {
    let rig = rig();
    rig.on("pi", Reply::ok("{}").takes(SECS(90)));
    rig.clock.advance(SECS(3600));
    assert_eq!(run_pi(&rig).started_at, "2026-01-01T01:00:00.000Z");
}

#[test]
fn a_distill_run_lasts_what_its_reply_took_on_the_clock() {
    let rig = rig();
    rig.on("pi", Reply::ok("{}").takes(SECS(90)));
    assert_eq!(run_pi(&rig).seconds, 90.0);
}

#[test]
fn a_distill_run_past_its_limit_is_timed_out_having_lasted_the_limit() {
    let rig = rig();
    rig.on("pi", Reply::ok("{}").takes(SECS(20 * 60)));
    let timed = run_pi(&rig);
    assert!(
        matches!(timed.waited, Waited::TimedOut),
        "{:?}",
        timed.waited
    );
    assert_eq!(timed.seconds, 900.0);
}
