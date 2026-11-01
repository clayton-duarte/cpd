//! The engine on virtual time and `FakeSpawn` (docs/implementing.md, "Ports, Spawn and run_with", "Virtual-time loops").
//!
//! Time is read only through the `ManualClock` (`advance`, `naps`, `utc`); a pid only through the `FakeSpawn`; a
//! tmux or `cpd` answer only through the `FakeRunner`. No test sleeps, starts a process or reads the wall clock. What
//! stays real is the temp home, the bundled SQLite and `flock` on files of that home.
//!
//! The decided shapes these tests pin (the stubs are `todo!()` or plain fields until the implementation Job):
//! - `Engine.ports`: the seams the steps read; a step test sets it (`fx.engine.ports = ports`) and calls the step as
//!   before (`distill::run`, `titles::run`, `escalations::run`, `restart::drain`);
//! - `engine::run_with(&Ports, &CpdHome, until: &AtomicBool)`: the loop of `cpd watch` on `ports.clock`, ended when
//!   `until` is set; `run()` is `run_with(&Ports::real(), ..)`;
//! - `Downtime::since_previous_with`, `consume_marker_with`, `WatchLock::touch_with` and `schedule::backing_off_with`:
//!   the `_with(clock, ..)` forms of what read `now_utc()` or `SystemTime::now()`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use rusqlite::Connection;
use serde_json::json;
use time::OffsetDateTime;

use super::engine::{Downtime, consume_marker_with, run_with};
use super::lock::WatchLock;
use super::log::Log;
use super::schedule::backing_off_with;
use super::testkit::Fx;
use super::{distill, escalations, restart, titles};
use cpd_core::clock::{Clock, ManualClock, timestamp};
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::host::runner::{FakeRunner, Reply};
use cpd_core::host::spawn::{FakeSpawn, Launch, Pid, Spawn};
use cpd_core::ids::{JobKey, PlanId};
use cpd_core::ports::Ports;
use cpd_core::store::Store;

const EXE: &str = "/opt/cpd/bin/cpd";
const SECS: fn(u64) -> Duration = Duration::from_secs;
const MILLIS: fn(u64) -> Duration = Duration::from_millis;

// ---- the world: a clock, a runner and a spawn, and the flag that stops the loop ------------------------------------

/// A `ManualClock` that also runs a hook on each nap (the nap's number, 1-based, before it passes) and sets `until` once
/// `stop_after` naps have passed: how a test ends the loop and acts between two ticks on one thread, with no second
/// reader of the time. It reads and advances time only through the `ManualClock` it holds.
struct Stepped {
    inner: ManualClock,
    until: Arc<AtomicBool>,
    stop_after: usize,
    hook: Mutex<Box<dyn FnMut(usize) + Send>>,
}

impl Clock for Stepped {
    fn now(&self) -> Instant {
        self.inner.now()
    }
    fn sleep(&self, d: Duration) {
        let nap = self.inner.naps().len() + 1;
        (self.hook.lock().unwrap())(nap);
        self.inner.sleep(d);
        if nap >= self.stop_after {
            self.until.store(true, Ordering::SeqCst);
        }
    }
    fn utc(&self) -> OffsetDateTime {
        self.inner.utc()
    }
}

struct World {
    clock: ManualClock,
    runner: Arc<FakeRunner>,
    spawn: Arc<FakeSpawn>,
    ports: Ports,
    until: Arc<AtomicBool>,
    /// The UTC time of the clock when the world was made.
    start: OffsetDateTime,
}

fn poll_ok() -> String {
    json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}}).to_string()
}

/// `front` adds the rules that must win; then no tmux server runs and `cpd pr poll` succeeds.
fn world_with(stop_after: usize, hook: impl FnMut(usize) + Send + 'static, front: impl FnOnce(FakeRunner) -> FakeRunner) -> World {
    let clock = ManualClock::new();
    let runner = Arc::new(
        front(FakeRunner::new(clock.clone()))
            .on("tmux", |_| true, Reply::exit(1, "", "no server running on /tmp/tmux-1/testkit\n"))
            .on("cpd", |_| true, Reply::ok(&poll_ok())),
    );
    let spawn = Arc::new(FakeSpawn::new());
    spawn.set_self_exe(EXE);
    let until = Arc::new(AtomicBool::new(false));
    let stepped = Stepped { inner: clock.clone(), until: until.clone(), stop_after, hook: Mutex::new(Box::new(hook)) };
    let ports = Ports { clock: Arc::new(stepped), runner: runner.clone(), spawn: spawn.clone(), ..Ports::real() };
    let start = clock.utc();
    World { clock, runner, spawn, ports, until, start }
}

/// A world whose loop never stops by itself.
fn world() -> World {
    world_with(usize::MAX, |_| {}, |r| r)
}

impl World {
    /// The time `ms` milliseconds from where the world began, as the store writes it.
    fn stamp(&self, ms: i64) -> String {
        timestamp(self.start + time::Duration::milliseconds(ms))
    }

    fn advance(&self, d: Duration) {
        self.clock.advance(d);
    }

    /// `fx.engine` on this world's ports, its tmux a `Tmux` through this world's runner.
    fn attach(&self, fx: &mut Fx) {
        fx.engine.ports = self.ports.clone();
        fx.engine.tmux = Tmux::new("testkit").with_runner(self.runner.clone());
    }

    fn launches(&self) -> Vec<Launch> {
        self.spawn.launches()
    }

    /// The only `Launch` so far.
    fn launch(&self) -> Launch {
        let launches = self.launches();
        assert_eq!(launches.len(), 1, "one launch: {launches:?}");
        launches.into_iter().next().unwrap()
    }
}

fn fx_on(w: &World, config_extra: &str) -> Fx {
    let mut fx = Fx::new(config_extra);
    w.attach(&mut fx);
    fx
}

/// `fx_on` with Plan `pay` (PR `repo`, its Lead `lead1`) in the store.
fn plan_on(w: &World, config_extra: &str) -> Fx {
    let mut fx = Fx::with_plan(config_extra);
    w.attach(&mut fx);
    fx
}

fn texts(fx: &mut Fx, sql: &str) -> Vec<String> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare(sql)?;
            Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
        })
        .unwrap()
}

/// The modification time of `home`'s lock file, as the store writes times.
fn lock_mark(home: &CpdHome) -> String {
    timestamp(OffsetDateTime::from(std::fs::metadata(home.watch_lock()).unwrap().modified().unwrap()))
}

// ---- run_with: the loop on virtual time ---------------------------------------------------------------------------

/// One tick a second, no PR poll in the way, a store that is not waited for in real time.
const LOOP: &str = "[watch]\ntick_seconds = 1\npr_poll_seconds = 3600\n[timeouts]\nstore_busy_ms = 1\n";

#[test]
fn the_engine_waits_between_ticks_in_naps_of_a_tenth_of_a_second() {
    let w = world_with(25, |_| {}, |r| r);
    let fx = fx_on(&w, LOOP);

    run_with(&w.ports, &fx.engine.home, &w.until).unwrap();

    assert_eq!(w.clock.naps(), vec![MILLIS(100); 25]);
    assert_eq!(w.clock.elapsed(), MILLIS(2500));
}

#[test]
fn a_tick_begins_when_tick_seconds_of_the_clock_have_passed_since_the_last_one_began() {
    // Ticks at 0 s, 1 s and 2 s; the 25th nap ends at 2.5 s and stops the engine before the tick of 3 s.
    let w = world_with(25, |_| {}, |r| r);
    let fx = fx_on(&w, LOOP);

    run_with(&w.ports, &fx.engine.home, &w.until).unwrap();

    assert_eq!(lock_mark(&fx.engine.home), w.stamp(2000), "the engine marks the lock alive at the start of each tick");
}

#[test]
fn a_commit_by_another_process_brings_the_next_tick_forward_to_half_a_second_after_the_last_began() {
    // The hook commits on a connection of its own during the first nap: the engine sees `data_version` change at the
    // next one. Ticks at 0 s and 0.5 s (not 5 s); the sixth nap, 0.5 s to 0.6 s, stops it.
    let home = Arc::new(Mutex::new(None::<CpdHome>));
    let shared = home.clone();
    let w = world_with(
        6,
        move |nap| {
            if nap == 1 {
                let home = shared.lock().unwrap().clone().unwrap();
                let mut other = Store::open(&home, 5000).unwrap();
                other
                    .write("fixture", |tx| {
                        Ok(tx.execute_batch("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('c2', 'C2', 'acme', 't', 'm1');")?)
                    })
                    .unwrap();
            }
        },
        |r| r,
    );
    let fx = fx_on(&w, "[watch]\ntick_seconds = 5\npr_poll_seconds = 3600\n");
    *home.lock().unwrap() = Some(fx.engine.home.clone());

    run_with(&w.ports, &fx.engine.home, &w.until).unwrap();

    assert_eq!(lock_mark(&fx.engine.home), w.stamp(500));
}

#[test]
fn the_engine_stops_between_ticks_when_until_is_set_and_says_so() {
    let w = world_with(3, |_| {}, |r| r);
    let mut fx = fx_on(&w, LOOP);

    run_with(&w.ports, &fx.engine.home, &w.until).unwrap();

    assert_eq!(w.clock.naps().len(), 3, "it naps no more once `until` is set");
    assert_eq!(fx.count("watch-started"), 1);
    assert_eq!(fx.count("watch-stopped"), 1);
}

#[test]
fn the_engine_runs_no_tick_when_until_is_set_before_it_starts() {
    let w = world();
    let mut fx = fx_on(&w, LOOP);
    w.until.store(true, Ordering::SeqCst);

    run_with(&w.ports, &fx.engine.home, &w.until).unwrap();

    assert!(w.clock.naps().is_empty());
    assert_eq!((fx.count("watch-started"), fx.count("watch-stopped")), (1, 1));
}

#[test]
fn a_busy_store_is_waited_out_on_the_clock_and_logged_when_it_clears() {
    // Another connection holds the write lock when the engine writes `watch-started`; the hook lets go during the
    // first nap of the wait. The wait is virtual: the first back-off is 500 ms.
    let held = Arc::new(Mutex::new(None::<Connection>));
    let release = held.clone();
    let w = world_with(
        40,
        move |nap| {
            if nap == 1 {
                release.lock().unwrap().take().expect("held").execute_batch("ROLLBACK").unwrap();
            }
        },
        |r| r,
    );
    let mut fx = fx_on(&w, LOOP);
    let other = Connection::open(fx.engine.home.db()).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    *held.lock().unwrap() = Some(other);

    run_with(&w.ports, &fx.engine.home, &w.until).unwrap();

    assert_eq!(fx.count("watch-started"), 1, "the write went through once the store was free");
    assert!(w.clock.naps()[0] <= MILLIS(500), "the first back-off is at most 500 ms: {:?}", w.clock.naps());
    let log = std::fs::read_to_string(fx.engine.home.watch_log()).unwrap();
    assert!(log.contains("the store is clear again after 0.5 s"), "{log}");
}

// ---- the downtime and the marker, exact at the boundary ------------------------------------------------------------

/// A home whose store holds `watch-started` of pid 77 at 0 s of the clock, and `watch-stopped` at 0 s when `stopped`.
fn after_an_engine(w: &World, stopped: bool) -> (Fx, WatchLock) {
    let mut fx = fx_on(w, "");
    let at = w.stamp(0);
    let mut sql = format!(
        "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '{at}', 'watch-started', 'started', '{{\"pid\":77}}');"
    );
    if stopped {
        sql += &format!("INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '{at}', 'watch-stopped', 'stopped', '{{\"pid\":77}}');");
    }
    fx.fixture(&sql);
    let lock = WatchLock::take(&fx.engine.home).unwrap();
    (fx, lock)
}

fn downtime(w: &World, fx: &mut Fx, lock: &WatchLock, lock_existed: bool, marker: Option<i64>) -> Downtime {
    Downtime::since_previous_with(&*w.ports.clock, &mut fx.engine.store, lock, lock_existed, marker).unwrap().expect("a previous engine")
}

#[test]
fn an_engine_that_stopped_is_down_from_its_stop_to_the_clock() {
    let w = world();
    let (mut fx, lock) = after_an_engine(&w, true);
    w.advance(MILLIS(3200));

    let down = downtime(&w, &mut fx, &lock, true, None);

    assert!(down.said);
    assert_eq!(down.down, time::Duration::milliseconds(3200));
    assert_eq!(down.since, w.start);
    assert_eq!(down.sentence(&w.ports.clock.zone()), format!("The engine was down for 3.2 s: it stopped at {}.", cpd_core::clock::prose(w.start, &w.ports.clock.zone())));
}

#[test]
fn a_restart_back_in_exactly_ten_seconds_on_the_clock_is_a_planned_one() {
    let w = world();
    let (mut fx, lock) = after_an_engine(&w, true);
    w.advance(SECS(10));

    let down = downtime(&w, &mut fx, &lock, true, Some(77));

    assert_eq!((down.planned, down.overdue), (true, false));
    assert!(down.sentence(&w.ports.clock.zone()).ends_with("(an install's planned restart)."), "{}", down.sentence(&w.ports.clock.zone()));
}

#[test]
fn a_restart_back_a_millisecond_after_ten_seconds_on_the_clock_is_overdue() {
    let w = world();
    let (mut fx, lock) = after_an_engine(&w, true);
    w.advance(MILLIS(10_001));

    let down = downtime(&w, &mut fx, &lock, true, Some(77));

    assert_eq!((down.planned, down.overdue), (false, true));
    assert!(down.sentence(&w.ports.clock.zone()).contains("it was an install's restart that took longer than 10 s."), "{}", down.sentence(&w.ports.clock.zone()));
}

#[test]
fn an_engine_that_left_no_stop_was_last_alive_when_it_marked_the_lock_on_the_clock() {
    let w = world();
    let (mut fx, lock) = after_an_engine(&w, false);
    w.advance(SECS(30));
    lock.touch_with(&*w.ports.clock).unwrap();
    w.advance(MILLIS(3200));

    let down = downtime(&w, &mut fx, &lock, true, None);

    assert!(!down.said);
    assert_eq!(down.since, w.start + time::Duration::seconds(30), "the lock file's mtime is the clock's time of the mark");
    assert_eq!(down.down, time::Duration::milliseconds(3200));
}

#[test]
fn marking_the_lock_alive_writes_the_time_of_the_clock_as_its_mtime() {
    let w = world();
    let fx = fx_on(&w, "");
    let lock = WatchLock::take(&fx.engine.home).unwrap();
    w.advance(MILLIS(1500));

    lock.touch_with(&*w.ports.clock).unwrap();

    let mtime = std::fs::metadata(fx.engine.home.watch_lock()).unwrap().modified().unwrap();
    assert_eq!(timestamp(OffsetDateTime::from(mtime)), w.stamp(1500));
    assert_eq!(SystemTime::from(w.start + time::Duration::milliseconds(1500)), mtime);
}

/// The marker file of `engine_pid`, written `age_ms` before the clock's now (negative: after it).
fn write_marker(w: &World, home: &CpdHome, age_ms: i64) -> std::path::PathBuf {
    let path = home.run_dir().join("install-restart.json");
    std::fs::create_dir_all(home.run_dir()).unwrap();
    let at = timestamp(w.clock.utc() - time::Duration::milliseconds(age_ms));
    std::fs::write(&path, json!({"by": "install-cpd.sh", "at": at, "engine_pid": 77}).to_string()).unwrap();
    path
}

#[test]
fn a_marker_exactly_120_seconds_old_on_the_clock_is_a_marker() {
    let w = world();
    let fx = fx_on(&w, "");
    let path = write_marker(&w, &fx.engine.home, 120_000);
    let mut log = Log::open(&fx.engine.home).unwrap();

    assert_eq!(consume_marker_with(&*w.ports.clock, &fx.engine.home, &mut log), Some(77));
    assert!(!path.exists(), "it is deleted when read");
}

#[test]
fn a_marker_a_millisecond_older_than_120_seconds_on_the_clock_is_ignored_and_deleted() {
    let w = world();
    let fx = fx_on(&w, "");
    let path = write_marker(&w, &fx.engine.home, 120_001);
    let mut log = Log::open(&fx.engine.home).unwrap();

    assert_eq!(consume_marker_with(&*w.ports.clock, &fx.engine.home, &mut log), None);
    assert!(!path.exists());
}

#[test]
fn a_marker_dated_after_the_clock_is_ignored() {
    let w = world();
    let fx = fx_on(&w, "");
    write_marker(&w, &fx.engine.home, -1);
    let mut log = Log::open(&fx.engine.home).unwrap();

    assert_eq!(consume_marker_with(&*w.ports.clock, &fx.engine.home, &mut log), None);
}

// ---- E3's back-off: 2^k seconds from the last `ended_at` to the clock ------------------------------------------------

/// SQL for Attempt `api-a<number>`, ended at `ended_at`, whose driver never started.
fn ended_attempt(number: u32, ended_at: &str) -> String {
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, driver_pid)
           VALUES ('pay', 'api-a{number}', 'api', {number}, 'S2', 'ladder', 'ended', 'failed', 'crashed',
                   'cpd/pay--api-a{number}', '/w', '{ended_at}', '{ended_at}', NULL);"
    )
}

/// Job `api` with `k` Attempts in a row whose driver never started, the last ended at 0 s of the clock.
fn failed_to_start(w: &World, k: u32) -> Fx {
    let mut fx = plan_on(w, "");
    let ended: String = (1..=k).map(|n| ended_attempt(n, &w.stamp(-1000 * i64::from(k - n)))).collect();
    fx.add_job("api", &[], &ended);
    fx
}

fn backing_off(w: &World, fx: &mut Fx) -> bool {
    let (plan, key) = (PlanId::parse("pay").unwrap(), JobKey::parse("api").unwrap());
    fx.engine.store.read(|conn| backing_off_with(conn, &plan, &key, &*w.ports.clock)).unwrap()
}

#[test]
fn a_job_backs_off_for_2_to_the_k_seconds_from_its_last_ended_at_to_the_clock() {
    let w = world();
    let mut fx = failed_to_start(&w, 2);
    w.advance(MILLIS(3999));
    assert!(backing_off(&w, &mut fx), "4 s are not over at 3.999 s");
    w.advance(MILLIS(1));
    assert!(!backing_off(&w, &mut fx), "they are over at 4.000 s");
}

#[test]
fn the_backoff_of_a_job_is_capped_at_300_seconds() {
    let w = world();
    let mut fx = failed_to_start(&w, 12);
    w.advance(MILLIS(299_999));
    assert!(backing_off(&w, &mut fx), "300 s are not over at 299.999 s");
    w.advance(MILLIS(1));
    assert!(!backing_off(&w, &mut fx), "they are over at 300.000 s");
}

// ---- E1: an escalation whose asker is gone is decided by the spawn -----------------------------------------------------

fn escalation(fx: &mut Fx, id: &str, asker_pid: u32) {
    fx.fixture(&format!(
        "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
           VALUES ('pay', '{id}', 'api-a1', {}, 'Why?', 'open', {asker_pid}, 't');",
        id.rsplit('q').next().unwrap()
    ));
}

fn status(fx: &mut Fx, id: &str) -> String {
    fx.query(&format!("SELECT status FROM escalations WHERE id = '{id}'"))
}

fn running_attempt() -> impl Fn(&mut Fx) {
    |fx| fx.add_running("api", "running", "t")
}

#[test]
fn e1_expires_an_escalation_whose_asker_is_no_process_of_the_spawn() {
    let w = world();
    let mut fx = plan_on(&w, "");
    running_attempt()(&mut fx);
    // The test's own process is alive for the system, and unknown to the fake spawn.
    escalation(&mut fx, "api-a1-q1", std::process::id());

    escalations::run(&mut fx.engine);

    assert_eq!(status(&mut fx, "api-a1-q1"), "expired");
    let expired = fx.events("escalation-expired");
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].3["reason"], "asker_dead");
}

#[test]
fn e1_keeps_an_escalation_whose_asker_runs_in_the_spawn_and_expires_it_when_it_exits() {
    let w = world();
    let mut fx = plan_on(&w, "");
    running_attempt()(&mut fx);
    let asker = w.spawn.spawn(&Launch { program: "cpd".into(), args: vec!["ask".into()], ..Launch::default() }).unwrap();
    escalation(&mut fx, "api-a1-q1", asker.0);

    escalations::run(&mut fx.engine);
    assert_eq!(status(&mut fx, "api-a1-q1"), "open", "its asker runs");

    w.spawn.exit(asker, 0);
    escalations::run(&mut fx.engine);
    assert_eq!(status(&mut fx, "api-a1-q1"), "expired", "its asker exited");
}

// ---- E1: the asking Worker's Session goes back to working -------------------------------------------------------------

fn worker_session(fx: &mut Fx, session: &str, job: &str, state: &str, label: Option<&str>) {
    let label = label.map_or("NULL".to_string(), |l| format!("'{l}'"));
    fx.fixture(&format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_label, state_at, created_at)
           VALUES ('{session}', 'worker', 'ctx', 'pay', '{job}-a1', 'Worker', '/', 'S1', '{state}', {label}, 't', 't');
         UPDATE attempts SET worker_session = '{session}' WHERE plan = 'pay' AND id = '{job}-a1';"
    ));
}

fn waiting_worker(fx: &mut Fx) {
    worker_session(fx, "w1", "api", "waiting", Some("φ: question to the Lead"));
}

fn session_state(fx: &mut Fx, id: &str) -> String {
    fx.query(&format!("SELECT state FROM sessions WHERE id = '{id}'"))
}

fn session_label(fx: &mut Fx, id: &str) -> Option<String> {
    fx.query(&format!("SELECT state_label FROM sessions WHERE id = '{id}'"))
}

fn asker_that_runs(w: &World) -> Pid {
    w.spawn.spawn(&Launch { program: "cpd".into(), args: vec!["ask".into()], ..Launch::default() }).unwrap()
}

#[test]
fn e1_puts_the_worker_session_back_to_working_when_it_expires_an_escalation_whose_asker_is_gone() {
    let w = world();
    let mut fx = plan_on(&w, "");
    running_attempt()(&mut fx);
    waiting_worker(&mut fx);
    escalation(&mut fx, "api-a1-q1", std::process::id());

    escalations::run(&mut fx.engine);

    assert_eq!(status(&mut fx, "api-a1-q1"), "expired");
    assert_eq!(session_state(&mut fx, "w1"), "working");
    assert_eq!(session_label(&mut fx, "w1"), None);
    let changes = fx.events("session-state");
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].0.as_deref(), Some("pay"));
    assert_eq!(changes[0].3["from"], "waiting");
    assert_eq!(changes[0].3["to"], "working");
    assert!(changes[0].3["label"].is_null());
    assert_eq!(fx.events("escalation-expired").len(), 1);
}

#[test]
fn e1_keeps_the_worker_session_waiting_while_another_escalation_of_the_attempt_is_open() {
    let w = world();
    let mut fx = plan_on(&w, "");
    running_attempt()(&mut fx);
    waiting_worker(&mut fx);
    escalation(&mut fx, "api-a1-q1", std::process::id());
    let asker = asker_that_runs(&w);
    escalation(&mut fx, "api-a1-q2", asker.0);

    escalations::run(&mut fx.engine);

    assert_eq!(status(&mut fx, "api-a1-q1"), "expired");
    assert_eq!(status(&mut fx, "api-a1-q2"), "open");
    assert_eq!(session_state(&mut fx, "w1"), "waiting");
    assert_eq!(session_label(&mut fx, "w1").as_deref(), Some("φ: question to the Lead"));
    assert!(fx.events("session-state").is_empty());

    w.spawn.exit(asker, 0);
    escalations::run(&mut fx.engine);

    assert_eq!(status(&mut fx, "api-a1-q2"), "expired");
    assert_eq!(session_state(&mut fx, "w1"), "working");
    assert_eq!(fx.events("session-state").len(), 1);
}

#[test]
fn e1_puts_the_session_back_to_working_once_when_it_expires_every_open_escalation_of_the_attempt_in_one_run() {
    let w = world();
    let mut fx = plan_on(&w, "");
    running_attempt()(&mut fx);
    waiting_worker(&mut fx);
    escalation(&mut fx, "api-a1-q1", std::process::id());
    escalation(&mut fx, "api-a1-q2", std::process::id());

    escalations::run(&mut fx.engine);

    assert_eq!(status(&mut fx, "api-a1-q1"), "expired");
    assert_eq!(status(&mut fx, "api-a1-q2"), "expired");
    assert_eq!(session_state(&mut fx, "w1"), "working");
    assert_eq!(fx.events("session-state").len(), 1);
}

#[test]
fn e1_puts_a_waiting_session_back_to_working_and_leaves_an_idle_one_as_it_is() {
    let w = world();
    let mut fx = plan_on(&w, "");
    running_attempt()(&mut fx);
    fx.add_running("web", "running", "t");
    waiting_worker(&mut fx);
    worker_session(&mut fx, "w2", "web", "idle", None);
    escalation(&mut fx, "api-a1-q1", std::process::id());
    fx.fixture(&format!(
        "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
           VALUES ('pay', 'web-a1-q1', 'web-a1', 1, 'Why?', 'open', {}, 't');",
        std::process::id()
    ));

    escalations::run(&mut fx.engine);

    assert_eq!(status(&mut fx, "api-a1-q1"), "expired");
    assert_eq!(status(&mut fx, "web-a1-q1"), "expired");
    assert_eq!(session_state(&mut fx, "w1"), "working");
    assert_eq!(session_state(&mut fx, "w2"), "idle");
    assert_eq!(fx.events("session-state").len(), 1);
}

// ---- E9: the distill run is started through the spawn ---------------------------------------------------------------

/// A Context `ctx` whose newest uncurated record was written at `at`; one record is enough (`context_curation_min_records`).
fn with_a_record(fx: &mut Fx, at: &str) {
    fx.fixture(&format!("INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('ctx', 'm1-n1', 'note', '{at}', 'm1', 'A note');"));
}

const CURATION: &str = "context_curation_min_records = 1\n";

/// A world and an engine with a record that is quiet: written 11 minutes before the clock's now.
fn quiet_context() -> (World, Fx) {
    let w = world();
    let mut fx = fx_on(&w, CURATION);
    w.advance(SECS(11 * 60));
    with_a_record(&mut fx, &w.stamp(0));
    (w, fx)
}

fn tick_e9(fx: &mut Fx) {
    distill::run(&mut fx.engine);
}

#[test]
fn e9_starts_cpd_context_distill_through_the_spawn_with_the_recorded_executable() {
    let (w, mut fx) = quiet_context();

    tick_e9(&mut fx);

    let launch = w.launch();
    assert_eq!(launch.program, EXE);
    assert_eq!(launch.args, ["context", "distill", "--context", "ctx", "--batch", "60"]);
}

#[test]
fn e9_starts_the_distill_run_detached_in_the_home_with_its_log_and_without_the_callers_identity() {
    let (w, mut fx) = quiet_context();
    let home = fx.engine.home.clone();

    tick_e9(&mut fx);

    let launch = w.launch();
    assert!(launch.setsid);
    assert_eq!(launch.cwd.as_deref(), Some(home.root()));
    assert_eq!(launch.stdout_to, Some(cpd_core::context::distill::log_path(&home, &"ctx".parse().unwrap())));
    assert!(launch.env_set.contains(&("CPD_HOME".to_owned(), home.root().to_string_lossy().into_owned())), "{:?}", launch.env_set);
    for removed in ["CPD_PLAN", "CPD_SESSION", "CPD_ATTEMPT", "TMUX", "TMUX_PANE"] {
        assert!(launch.env_remove.iter().any(|r| r == removed), "{removed}: {:?}", launch.env_remove);
    }
}

#[test]
fn e9_records_the_pid_the_spawn_handed_out_in_the_pid_file() {
    let (w, mut fx) = quiet_context();
    let pid_file = fx.engine.home.distill_pid(&"ctx".parse().unwrap());

    tick_e9(&mut fx);

    assert_eq!(w.launches().len(), 1);
    assert_eq!(std::fs::read_to_string(pid_file).unwrap(), "1000\n");
}

#[test]
fn e9_starts_no_second_run_while_the_first_has_not_exited() {
    let (w, mut fx) = quiet_context();

    tick_e9(&mut fx);
    tick_e9(&mut fx);
    tick_e9(&mut fx);

    assert_eq!(w.launches().len(), 1);
}

#[test]
fn e9_takes_the_next_smaller_batch_when_a_run_exited_and_the_state_did_not_change() {
    let (w, mut fx) = quiet_context();
    tick_e9(&mut fx);
    w.spawn.exit(Pid(1000), 1);

    tick_e9(&mut fx);

    let launches = w.launches();
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert_eq!(launches[1].args.last().map(String::as_str), Some("30"));
}

#[test]
fn e9_waits_until_the_newest_record_is_ten_minutes_old_on_the_clock() {
    let w = world();
    let mut fx = fx_on(&w, CURATION);
    with_a_record(&mut fx, &w.stamp(0));
    w.advance(MILLIS(599_999));
    tick_e9(&mut fx);
    assert!(w.launches().is_empty(), "9 min 59.999 s is not quiet yet");

    w.advance(MILLIS(1));
    tick_e9(&mut fx);

    assert_eq!(w.launches().len(), 1, "10 min is quiet");
}

// ---- the tmux restart: started detached through the spawn, not again within a minute ------------------------------------

fn pending_restart() -> (World, Fx) {
    let w = world();
    let fx = fx_on(&w, "");
    restart::mark(&fx.engine.home, "3.4", "3.8").unwrap();
    (w, fx)
}

#[test]
fn the_drain_starts_cpd_watch_restart_tmux_through_the_spawn_with_the_recorded_executable() {
    let (w, mut fx) = pending_restart();

    restart::drain(&mut fx.engine);

    let launch = w.launch();
    assert_eq!(launch.program, EXE);
    assert_eq!(launch.args, ["watch", "--restart-tmux"]);
}

#[test]
fn the_drain_starts_the_restart_detached_in_the_home_with_its_log() {
    let (w, mut fx) = pending_restart();
    let home = fx.engine.home.clone();

    restart::drain(&mut fx.engine);

    let launch = w.launch();
    assert!(launch.setsid);
    assert_eq!(launch.cwd.as_deref(), Some(home.root()));
    assert_eq!(launch.stdout_to, Some(home.tmux_restart_log()));
    assert!(launch.env_set.contains(&("CPD_HOME".to_owned(), home.root().to_string_lossy().into_owned())), "{:?}", launch.env_set);
}

#[test]
fn the_drain_records_the_pid_the_spawn_handed_out_in_the_pid_file() {
    let (w, mut fx) = pending_restart();
    let pid_file = fx.engine.home.tmux_restart_pid();

    restart::drain(&mut fx.engine);

    assert_eq!(w.launches().len(), 1);
    assert_eq!(std::fs::read_to_string(pid_file).unwrap(), "1000\n");
}

#[test]
fn the_drain_does_not_start_the_restart_again_before_a_minute_of_the_clock_has_passed() {
    let (w, mut fx) = pending_restart();
    restart::drain(&mut fx.engine);
    w.advance(MILLIS(59_999));
    restart::drain(&mut fx.engine);
    assert_eq!(w.launches().len(), 1, "59.999 s are not a minute");

    w.advance(MILLIS(1));
    restart::drain(&mut fx.engine);

    assert_eq!(w.launches().len(), 2, "a minute is");
}

// ---- E10: the retitle run is started through the spawn ----------------------------------------------------------------

/// tmux answers a chat slot pane of the cockpit, 90 columns wide.
fn with_a_chat_slot(r: FakeRunner) -> FakeRunner {
    r.on("tmux", |c| c.args.iter().any(|a| a == "list-panes"), Reply::ok("%1\t@1\tcpd\tmain\t0\t\t4242\t\tchat\n"))
        .on("tmux", |c| c.args.iter().any(|a| a == "display-message"), Reply::ok("90\n"))
}

fn titled_world() -> World {
    world_with(usize::MAX, |_| {}, with_a_chat_slot)
}

/// Session `id` of `role` in Context `ctx`, idle, whose last turn ended at `turn` and whose title (if any) was written at `titled`.
fn idle_session(fx: &mut Fx, id: &str, context: &str, role: &str, turn: &str, titled: Option<&str>) {
    let titled = titled.map_or("NULL".to_owned(), |t| format!("'{t}'"));
    fx.fixture(&format!(
        "INSERT OR IGNORE INTO contexts (id, title, owner, created_at, created_on) VALUES ('{context}', '{context}', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, titled_at)
           VALUES ('{id}', '{role}', '{context}', NULL, '{id}', '/', 'O3', 'idle', 't', 't', {titled});
         INSERT INTO events (txn, at, kind, context, session, text, data)
           VALUES (cpd_txn(), '{turn}', 'session-state', '{context}', '{id}', 'idle', '{{\"from\":\"working\",\"to\":\"idle\"}}');"
    ));
}

#[test]
fn e10_starts_cpd_session_retitle_through_the_spawn_with_the_recorded_executable_and_the_chat_slot_width() {
    let w = titled_world();
    let mut fx = fx_on(&w, "");
    idle_session(&mut fx, "mgr1", "ctx", "manager", &w.stamp(0), None);

    titles::run(&mut fx.engine);

    let launch = w.launch();
    assert_eq!(launch.program, EXE);
    assert_eq!(launch.args, ["session", "retitle", "mgr1", "--width", "90"]);
}

#[test]
fn e10_starts_the_retitle_detached_in_the_home_with_the_sessions_log() {
    let w = titled_world();
    let mut fx = fx_on(&w, "");
    idle_session(&mut fx, "mgr1", "ctx", "manager", &w.stamp(0), None);
    let home = fx.engine.home.clone();

    titles::run(&mut fx.engine);

    let launch = w.launch();
    assert!(launch.setsid);
    assert_eq!(launch.cwd.as_deref(), Some(home.root()));
    assert_eq!(launch.stdout_to, Some(home.run_dir().join("titles").join("mgr1.log")));
    assert!(launch.env_set.contains(&("CPD_HOME".to_owned(), home.root().to_string_lossy().into_owned())), "{:?}", launch.env_set);
}

#[test]
fn e10_starts_at_most_two_retitle_runs_at_once() {
    let w = titled_world();
    let mut fx = fx_on(&w, "");
    for (i, id) in ["mgr1", "mgr2", "mgr3"].into_iter().enumerate() {
        idle_session(&mut fx, id, &format!("c{i}"), "manager", &w.stamp(0), None);
    }

    titles::run(&mut fx.engine);

    assert_eq!(w.launches().len(), 2);
}

#[test]
fn e10_rewrites_a_title_once_it_is_ten_minutes_old_on_the_clock() {
    let w = titled_world();
    let mut fx = fx_on(&w, "");
    // Titled at 0 s; a newer turn ended at 1 s.
    idle_session(&mut fx, "mgr1", "ctx", "manager", &w.stamp(1000), Some(&w.stamp(0)));
    w.advance(MILLIS(599_999));
    titles::run(&mut fx.engine);
    assert!(w.launches().is_empty(), "the title is 9 min 59.999 s old");

    w.advance(MILLIS(1));
    titles::run(&mut fx.engine);

    assert_eq!(w.launches().len(), 1, "the title is 10 min old");
}

#[test]
fn e10_reports_a_title_that_has_failed_for_ten_minutes_of_the_clock_and_not_before() {
    let w = titled_world();
    let mut fx = fx_on(&w, "");
    idle_session(&mut fx, "mgr1", "ctx", "manager", &w.stamp(0), None);

    // Run 1 starts at 0 s and fails; the failure is first seen by the next tick, which starts run 2.
    titles::run(&mut fx.engine);
    w.spawn.exit(Pid(1000), 1);
    titles::run(&mut fx.engine);
    // Run 2 fails 9 min 59.999 s after the first failure was seen.
    w.advance(MILLIS(599_999));
    w.spawn.exit(Pid(1001), 1);
    titles::run(&mut fx.engine);
    assert_eq!(w.launches().len(), 3, "each failed run is started again");
    assert_eq!(fx.count("error"), 0, "it has failed for less than ten minutes");

    // Run 3 fails exactly ten minutes after.
    w.advance(MILLIS(1));
    w.spawn.exit(Pid(1002), 1);
    titles::run(&mut fx.engine);

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].3["code"], "title_failed");
    assert_eq!(texts(&mut fx, "SELECT count(*) || '' FROM events WHERE kind = 'error'"), ["1"]);
}
