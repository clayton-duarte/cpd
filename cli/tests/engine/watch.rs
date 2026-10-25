//! The engine, `cpd watch` (contract §3.7, §6.6; commands.md §12): the lock
//! and its refusal, the signals, `--ensure`, and each step E1–E6 (E7, the PR start, is in pr_start.rs) on a
//! prepared database, with a private tmux server and fake `pi` and `cpd`.

use crate::common;
#[path = "../common/checkout.rs"]
mod checkout;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fx::Fx as Ip;
use common::{CPD, PR_POLL_EVERY_TICK, Run, SLEEP_SECONDS, TICK_SECONDS, TestEnv, copy_executable, wait_until, wait_until_for};
use cpd_core::clock;
use cpd_core::error::CpdError;
use cpd_core::git::{self, Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::{CpdHome, Infra};
use cpd_core::host::runner::{Call, Reply};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::output::Output;
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::store::{Store, WriteTxn};
use nix::fcntl::{Flock, FlockArg};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use rusqlite::Connection;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::format_description;

const PLAN: &str = "pay";

/// O supports efforts 0–5, S 1–5, H 0–3.
const EFFORTS: [(&str, &str, &str); 3] =
    [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")];

/// A time `minutes` ago, as cpd writes times (contract §1.3).
fn ago(minutes: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::minutes(minutes))
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .unwrap()
}

/// SQL for Attempt `<key>-a<number>` ended `failed` / `crashed` at `ended_at`; `driver_pid` is null when its driver never started.
fn ended_attempt(key: &str, number: u32, tier: &str, driver_pid: Option<i64>, ended_at: &str) -> String {
    let pid = driver_pid.map_or("NULL".to_owned(), |p| p.to_string());
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, driver_pid)
           VALUES ('pay', '{key}-a{number}', '{key}', {number}, '{tier}', 'ladder', 'ended', 'failed', 'crashed',
                   'cpd/pay--{key}-a{number}', '/w', '{ended_at}', '{ended_at}', {pid});"
    )
}

fn parse_time(text: &str) -> OffsetDateTime {
    OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A running `cpd watch`; SIGKILLed if a test panics before it stops it.
struct Engine {
    child: Option<Child>,
}

impl Engine {
    fn pid(&self) -> u32 {
        self.child.as_ref().unwrap().id()
    }

    fn signal(&self, signal: Signal) {
        kill(Pid::from_raw(self.pid() as i32), signal).unwrap();
    }

    /// Stops it with `signal` and returns what it printed.
    fn stop_with(mut self, signal: Signal) -> Run {
        self.signal(signal);
        let out = self.child.take().unwrap().wait_with_output().unwrap();
        Run {
            exit: out.status.code().expect("watch exits, it is not killed"),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    fn stop(self) -> Run {
        self.stop_with(Signal::SIGTERM)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A repo `pay` with the Lead worktree of Plan `pay` (phase `dispatch`), a
/// config on this env's tmux server with a one-second tick, a fake pi install
/// whose Tier catalog is seeded in the cache, a Context `ctx` and the Plan's
/// Lead Session (`stopped`, so E2 leaves it alone).
struct Fx {
    env: TestEnv,
    repo: PathBuf,
    lead: PathBuf,
}

/// What the fake `gh` answers to the PR poller's search when a test does not care: no pull requests.
const EMPTY_SEARCH: &str = r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#;

fn fx(config_extra: &str) -> Fx {
    let fx = fx_bare(config_extra);
    fx.fixture(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'main', 'dispatch', 'cpd/pay', '{lead}', '{PR_TITLE}', 'config', cpd_txn(), 't', 't');",
        repo = fx.repo.display(),
        lead = fx.lead.display()
    ));
    fx
}

/// The stored title of PR `repo`, and the shape it is given (`config`: its Jobs are `work` Jobs, as these fixtures add them).
const PR_TITLE: &str = "Charge the card at checkout";

/// `fx` without the Plan: Context `ctx`, the repo `pay` and its Lead worktree, no Plan. E5 runs `cpd pr poll` on every
/// engine, through a fake `gh` that finds no pull requests: no test runs the real one.
fn fx_bare(config_extra: &str) -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(&repo, &["config", key, value]);
    }
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("lead.log") };
    let lead = create_lead_worktree(&repo, &PlanId::parse(PLAN).unwrap(), "main", &setup).unwrap().path;
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n{config_extra}\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [watch]\ntick_seconds = {TICK_SECONDS}\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env.root().join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    env.fake_pi("exit 0");
    env.fake_gh(&format!("echo '{EMPTY_SEARCH}'"));
    // A `cpd` on `PATH` that records its calls: E5 runs the engine's own executable, so it must see none.
    env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
    let fx = Fx { env, repo, lead };
    let catalog: String = EFFORTS
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    fx.fixture(&format!(
        "{catalog}
         INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');"
    ));
    fx
}

impl Fx {
    fn home(&self) -> CpdHome {
        CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap()
    }

    fn store(&self) -> Store {
        Store::open(&self.home(), 5000).unwrap()
    }

    fn fixture(&self, sql: &str) {
        self.store().write("fixture", |tx| exec(tx, sql)).unwrap();
    }

    /// Job `key` of Plan `pay`, `queued` with its Locked-Job fingerprint, then
    /// moved to `status` by `then` (SQL run after the fingerprint is set).
    fn add_job(&self, key: &str, needs: &[&str], then: &str) {
        self.add_job_in("repo", key, needs, then);
    }

    /// `add_job` in PR `pr` of Plan `pay`.
    fn add_job_in(&self, pr: &str, key: &str, needs: &[&str], then: &str) {
        self.add_job_on("S2", pr, key, needs, then);
    }

    /// `add_job_in` with the Job's Tier `tier`.
    fn add_job_on(&self, tier: &str, pr: &str, key: &str, needs: &[&str], then: &str) {
        let mut sql = format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', '{pr}', '{key}', 'Title of {key}', '{tier}', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        );
        for need in needs {
            sql += &format!("INSERT INTO dependencies (plan, job, needs) VALUES ('pay', '{key}', '{need}');");
        }
        self.fixture(&sql);
        let plan = PlanId::parse(PLAN).unwrap();
        let all = self.store().read(|conn| jobs::load(conn, &plan)).unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.fixture(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = '{key}'; {then}",
            locked_fingerprint(job)
        ));
    }

    /// Job `key` `running` with Attempt `<key>-a1` in `status`, started at `started_at`, its driver (pid 4242) started.
    /// A `running` Attempt has its `starting → running` `attempt-status` Event at `started_at` too.
    fn add_running(&self, key: &str, status: &str, started_at: &str) {
        self.add_running_since(key, status, started_at, (status == "running").then_some(started_at));
    }

    /// `add_running` with the `starting → running` Event at `running_at` (none: the Attempt is still `starting`).
    fn add_running_since(&self, key: &str, status: &str, started_at: &str, running_at: Option<&str>) {
        let worktree = self.repo.parent().unwrap().join(format!("pay.pay--{key}-a1"));
        let event = running_at
            .map(|at| {
                format!(
                    "INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
                       VALUES (cpd_txn(), '{at}', 'attempt-status', 'ctx', 'pay', '{key}', '{key}-a1', 'running',
                               '{{\"from\":\"starting\",\"to\":\"running\"}}');"
                )
            })
            .unwrap_or_default();
        self.add_job(
            key,
            &[],
            &format!(
                "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = '{key}';
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, driver_pid)
                   VALUES ('pay', '{key}-a1', '{key}', 1, 'S2', 'ladder', '{status}', 'cpd/pay--{key}-a1', '{}', '{started_at}', 4242);
                 {event}",
                worktree.display()
            ),
        );
    }

    fn request_cancel(&self, key: &str) {
        self.fixture(&format!(
            "UPDATE jobs SET cancel_reason = 'not needed', cancel_requested_at = '{}' WHERE plan = 'pay' AND key = '{key}';",
            ago(0)
        ));
    }

    /// Holds Attempt `attempt`'s driver lock, as a live driver does.
    fn live_driver(&self, attempt: &str) -> Flock<File> {
        let home = self.home();
        let path = home.attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(attempt).unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
        Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap()
    }

    /// Starts `cpd watch` and waits for its `watch-started`.
    fn start(&self) -> Engine {
        self.start_from(Path::new(CPD))
    }

    /// `start` with the engine run from the binary at `exe`; E5 polls every tick and the engine and its drivers look
    /// every 50 ms ([`common::PLAN_POLLS`]).
    fn start_from(&self, exe: &Path) -> Engine {
        self.start_with(exe, &common::PLAN_POLLS)
    }

    /// `start_from` with the variables `vars` added to the engine's environment.
    fn start_with(&self, exe: &Path, vars: &[(&str, &str)]) -> Engine {
        let mut command = self.env.command(exe);
        command.envs(vars.iter().copied());
        let child = command.arg("watch").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let mut engine = Engine { child: Some(child) };
        let pid = engine.pid();
        let started = common::poll(common::WAIT, || {
            let seen = self.env.query::<i64>(&format!(
                "SELECT count(*) FROM events WHERE kind = 'watch-started' AND json_extract(data, '$.pid') = {pid}"
            )) == 1;
            // An engine that exited (`watch_running`, a failed start) never says it started: stop waiting for it.
            (seen || engine.child.as_mut().unwrap().try_wait().unwrap().is_some()).then_some(seen)
        });
        if started != Some(true) {
            let mut child = engine.child.take().unwrap();
            let exited = child.try_wait().unwrap();
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!(
                "engine {pid} did not append watch-started (exited: {exited:?})\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        engine
    }

    /// Every Event of `kind`, in order: `(plan, job, attempt, data)`.
    fn events(&self, kind: &str) -> Vec<(Option<String>, Option<String>, Option<String>, Value)> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT plan, job, attempt, data FROM events WHERE kind = ?1 ORDER BY id")?;
            let rows = stmt
                .query_map([kind], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.into_iter().map(|(p, j, a, d)| (p, j, a, serde_json::from_str(&d).unwrap())).collect())
        })
    }

    fn count(&self, kind: &str) -> usize {
        self.events(kind).len()
    }

    fn texts(&self, kind: &str) -> Vec<String> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT text FROM events WHERE kind = ?1 ORDER BY id")?;
            Ok(stmt.query_map([kind], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    fn attempt(&self, id: &str) -> (String, Option<String>, Option<String>, Option<String>) {
        self.env.read(|conn| {
            Ok(conn.query_row(
                "SELECT status, outcome, cause, stop_request FROM attempts WHERE plan = 'pay' AND id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
    }

    fn job_status(&self, key: &str) -> String {
        self.env.query(&format!("SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"))
    }
}

// ---------- the lock, the signals, the log ----------

#[test]
fn watch_holds_the_lock_refuses_a_second_and_stops_on_sigterm() {
    let fx = fx("");
    let engine = fx.start();
    let pid = engine.pid();

    let second = fx.env.cpd(&["watch"]);
    assert_eq!(second.exit, 3, "{second:?}");
    assert_eq!(second.error()["code"], "watch_running");

    let out = engine.stop();
    assert_eq!(out.json(), json!({"ok": true, "stopped": true}), "{out:?}");
    assert_eq!(out.exit, 0);
    assert_eq!(fx.events("watch-started"), [(None, None, None, json!({"pid": pid}))]);
    assert_eq!(fx.events("watch-stopped"), [(None, None, None, json!({"pid": pid}))]);
    let log = fs::read_to_string(fx.env.cpd_home().join("run/watch.log")).unwrap();
    assert!(log.contains(&format!("started (pid {pid})")), "{log}");
    assert!(out.stderr.contains(&format!("started (pid {pid})")), "{out:?}");

    // The lock went with it: a new engine starts, and SIGINT stops it too.
    let again = fx.start();
    let out = again.stop_with(Signal::SIGINT);
    assert_eq!(out.json(), json!({"ok": true, "stopped": true}));
    assert_eq!(fx.count("watch-stopped"), 2);
}

/// `tmux kill-window` (the install's restart of the engine) sends SIGHUP: it is a stop, not a crash. The signal's one job,
/// setting the stop flag, is `catch_signals` (the smoke test above sends real SIGTERM and SIGINT); here the flag is `until`.
#[test]
fn watch_stops_on_sighup_with_one_watch_stopped() {
    let fx = engine("");

    let out = fx.watch_first_tick().unwrap();

    assert_eq!(out, Output::success(json!({"stopped": true})), "{out:?}");
    assert_eq!(fx.events("watch-stopped"), [(None, None, None, json!({"pid": std::process::id()}))]);
}

#[test]
fn watch_refuses_unknown_flags_and_needs_its_config() {
    let fx = fx("");
    let run = fx.env.cpd(&["watch", "--bogus"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (2, json!("usage")));
    fs::remove_file(fx.env.cpd_home().join("config.toml")).unwrap();
    let run = fx.env.cpd(&["watch"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (1, json!("config_missing")));
}

// ---------- --ensure ----------

/// engine-W10: both calls probe the lock before either engine has taken it (the fake `cpd` is slow to start), so both create
/// a window and one engine loses the lock (`watch_running`, its pane dies). Both calls succeed, one engine runs, no dead window.
/// hygiene.md 3c: the late supervising `ensure` of a test whose env is being removed must not make a home again. The
/// home it finds is the one `rm -rf` leaves for a moment: `config.toml` still readable, `cpd.db` (which every set-up
/// home has: the store is opened before anything runs `watch --ensure`) already gone, as `run/` and `locks/`. (With no
/// `$CPD_HOME` at all `ensure` fails already: `config_missing`.) The marker the fix keys on is `cpd.db`.
#[test]
fn ensure_in_a_home_that_does_not_exist_fails_and_creates_nothing() {
    let env = TestEnv::new();
    let home = env.root().join("home-being-removed");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("config.toml"), format!("version = 1\nmachine = \"m1\"\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket())).unwrap();
    let entries = || -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&home).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    };

    // The server, if it starts, is the env's own (the config's socket): the teardown stops it.
    let run = env.cpd_with(env.root(), &["watch", "--ensure"], &[("CPD_HOME", home.to_str().unwrap())]);

    assert_ne!(run.exit, 0, "ensure succeeded in a home with no cpd.db: {run:?}");
    assert_eq!(entries(), ["config.toml"], "ensure made something in the home: {run:?}");
}

#[test]
fn ensure_fails_when_the_engine_does_not_start() {
    let fx = fx("");
    fx.env.fake("cpd", "echo 'cannot start' >&2; exit 7");
    let run = fx.env.cpd(&["watch", "--ensure"]);
    assert_eq!(run.exit, 1, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "watch_start_failed");
    assert_eq!(error["details"]["exit"], 7);
    assert_eq!(error["details"]["next"], "cpd watch --ensure");
    assert!(error["details"]["pane"].as_str().unwrap().starts_with('%'));
}

// ---------- supervision: the engine comes back whatever stopped it (contract §6.6, hs-d328) ----------

impl Fx {
    /// A Manager Session of Context `ctx`.
    fn manager(&self, id: &str) {
        self.fixture(&format!(
            "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
               VALUES ('{id}', 'manager', 'ctx', 'Manager {id}', '/', 'O3', 'stopped', 't', 't');"
        ));
    }

    /// The messages the engine sent: `(session, context, data)`.
    fn messages(&self) -> Vec<(String, Option<String>, Value)> {
        self.store()
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT session, context, data FROM events WHERE kind = 'session-message' ORDER BY id")?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows.into_iter().map(|(s, c, d)| (s, c, serde_json::from_str(&d).unwrap())).collect())
            })
            .unwrap()
    }
}

// ---------- the engine in-process: `watch::run_with(ports, until)` on a `ManualClock` ----------

/// A home for the engine on fake ports ([`Fx::watch`]): its config, the routes it needs, Context `ctx`. No tmux, no git, no
/// process, no wait: the clock is the `ManualClock`, so a time a test sets is exact.
fn engine(config_extra: &str) -> Ip {
    let fx = Ip::with_ports();
    fx.engine_config(config_extra);
    fx.engine_routes();
    fx.env.sql("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');");
    fx
}

fn engine_log(fx: &Ip) -> String {
    fs::read_to_string(fx.home().watch_log()).unwrap_or_default()
}

/// What the engine tests read of, and put in, an in-process home.
trait Home {
    fn home(&self) -> CpdHome;
    /// Every Event of `kind`, in order: `(plan, job, attempt, data)`.
    fn events(&self, kind: &str) -> Vec<(Option<String>, Option<String>, Option<String>, Value)>;
    fn texts(&self, kind: &str) -> Vec<String>;
    /// The messages the engine sent: `(session, context, data)`.
    fn messages(&self) -> Vec<(String, Option<String>, Value)>;
    fn manager(&self, id: &str);
    fn marker_path(&self) -> PathBuf;
    /// The `watch-started` text of the engine under test (the last).
    fn started_text(&self) -> String;
    /// A home whose previous engine (pid [`PREVIOUS_ENGINE`]) started an hour ago and stopped `down_ms` before the clock's
    /// now, saying so (`watch-stopped`) when `said`, else leaving nothing (a crash: it was last alive at its start).
    fn previous_engine(&self, down_ms: i64, said: bool);
    /// The previous engine's lock file as it last marked it: `ms` before the clock's now.
    fn last_marked_alive(&self, ms: i64);
    /// The install's marker, written `age_ms` before the clock's now for the engine `pid`.
    fn marker(&self, pid: i64, age_ms: i64);
    fn raw_marker(&self, text: &str);
    /// The engine starts, ticks once and stops: what the Managers were told is then settled.
    fn start_and_stop(&self);
    /// No Manager got a downtime message, and the engine's `watch-started` says `how`.
    fn told_with(&self, how: &str);
}

impl Home for Ip {
    fn home(&self) -> CpdHome {
        CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap()
    }

    fn events(&self, kind: &str) -> Vec<(Option<String>, Option<String>, Option<String>, Value)> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT plan, job, attempt, data FROM events WHERE kind = ?1 ORDER BY id")?;
            let rows = stmt
                .query_map([kind], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.into_iter().map(|(p, j, a, d)| (p, j, a, serde_json::from_str(&d).unwrap())).collect())
        })
    }

    fn texts(&self, kind: &str) -> Vec<String> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT text FROM events WHERE kind = ?1 ORDER BY id")?;
            Ok(stmt.query_map([kind], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    fn messages(&self) -> Vec<(String, Option<String>, Value)> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT session, context, data FROM events WHERE kind = 'session-message' ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.into_iter().map(|(s, c, d)| (s, c, serde_json::from_str(&d).unwrap())).collect())
        })
    }

    fn manager(&self, id: &str) {
        self.env.sql(&format!(
            "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
               VALUES ('{id}', 'manager', 'ctx', 'Manager {id}', '/', 'O3', 'stopped', 't', 't');"
        ));
    }

    fn marker_path(&self) -> PathBuf {
        self.home().run_dir().join("install-restart.json")
    }

    fn started_text(&self) -> String {
        self.texts("watch-started").pop().unwrap()
    }

    fn previous_engine(&self, down_ms: i64, said: bool) {
        let stopped = if said {
            format!(
                "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '{}', 'watch-stopped', 'The engine stopped.', '{{\"pid\":{PREVIOUS_ENGINE}}}');",
                self.stamp_ago(down_ms)
            )
        } else {
            String::new()
        };
        // Without a `watch-stopped` the engine is dated by its `watch-started`.
        let started_ago = if said { 3_600_000 } else { down_ms };
        self.env.sql(&format!(
            "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '{}', 'watch-started', 'The engine started.', '{{\"pid\":{PREVIOUS_ENGINE}}}');{stopped}",
            self.stamp_ago(started_ago)
        ));
    }

    fn last_marked_alive(&self, ms: i64) {
        let path = self.home().watch_lock();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = File::create(&path).unwrap();
        file.set_modified(self.utc_ago(ms).into()).unwrap();
    }

    fn marker(&self, pid: i64, age_ms: i64) {
        self.raw_marker(&json!({"by": "install-cpd.sh", "at": self.stamp_ago(age_ms), "engine_pid": pid}).to_string());
    }

    fn raw_marker(&self, text: &str) {
        fs::create_dir_all(self.marker_path().parent().unwrap()).unwrap();
        fs::write(self.marker_path(), text).unwrap();
    }

    fn start_and_stop(&self) {
        self.watch_first_tick().unwrap();
    }

    fn told_with(&self, how: &str) {
        assert_eq!(self.messages(), [], "a restart sends no session-message (hs-d514): the Managers are not told");
        assert!(self.started_text().contains(how), "{how:?} not in {}", self.started_text());
    }
}

// ---------- the downtime message (hs-d328) ----------

/// `Fx::watch` ends the engine at the nap that follows the first tick: one tick, one nap of a tenth of a second, no wait.
#[test]
fn the_engine_in_process_runs_until_the_first_tick_and_naps_once_on_virtual_time() {
    let fx = engine("");

    fx.watch_first_tick().unwrap();

    assert_eq!(fx.fakes.clock.naps(), [Duration::from_millis(100)]);
    assert_eq!((fx.events("watch-started").len(), fx.events("watch-stopped").len()), (1, 1));
}

#[test]
fn the_engine_in_process_runs_until_the_nap_it_is_told_and_the_hook_sees_each_nap_before_it_passes() {
    let fx = engine("");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();

    fx.watch(3, move |nap| log.lock().unwrap().push(nap)).unwrap();

    assert_eq!(*seen.lock().unwrap(), [1, 2, 3]);
    assert_eq!(fx.fakes.clock.naps(), [Duration::from_millis(100); 3]);
}

#[test]
fn the_engine_in_process_runs_until_its_step_says_the_nap_is_the_last_and_the_step_sees_the_state_after_each_tick() {
    let fx = engine("");
    let clock = fx.fakes.clock.clone();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();

    // The engine is quiet, so the state the step reads is the virtual time: it stops at the nap that passes the 2 s mark.
    fx.watch_steps(move |nap| {
        log.lock().unwrap().push((nap, clock.elapsed()));
        clock.elapsed() >= Duration::from_secs(2)
    })
    .unwrap();

    let seen = seen.lock().unwrap();
    assert_eq!(seen.first(), Some(&(1, Duration::ZERO)));
    assert_eq!(seen.last(), Some(&(21, Duration::from_secs(2))));
    assert_eq!(fx.fakes.clock.naps().len(), 21);
}

#[test]
fn a_plan_and_a_queued_job_in_process_have_the_locked_fingerprint_the_engine_checks() {
    let fx = engine("");
    fx.engine_plan();
    fx.add_job("api", &[], "");
    fx.add_job("web", &["api"], "");

    assert_eq!(fx.env.query::<String>("SELECT status FROM jobs WHERE plan = 'pay' AND key = 'api'"), "queued");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs WHERE plan = 'pay' AND fingerprint IS NOT NULL"), 2);
    assert_eq!(fx.env.query::<String>("SELECT needs FROM dependencies WHERE plan = 'pay' AND job = 'web'"), "api");
    assert_eq!(fx.env.query::<String>("SELECT phase FROM plans WHERE id = 'pay'"), "dispatch");
}

#[test]
fn a_restart_sends_no_downtime_notice() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.watch_first_tick().unwrap();
    let stopped_at = fx.env.query::<String>("SELECT at FROM events WHERE kind = 'watch-stopped' ORDER BY id LIMIT 1");
    fx.watch_first_tick().unwrap();
    // The record is the Event's text, not a message (hs-d514: the Managers are not told of a restart).
    assert!(fx.texts("watch-started")[1].contains(&format!("it stopped at {stopped_at}")), "{:?}", fx.texts("watch-started"));
    assert_eq!(fx.messages(), [], "no Manager is sent the downtime");
}

/// A SIGKILLed engine leaves no `watch-stopped`, and its lock file as it last marked it (the engine marks it at each tick):
/// the fixture is what the kill leaves, the engine reading it is the subject.
#[test]
fn an_engine_that_died_without_a_watch_stopped_sends_no_notice() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(60_000, false);
    fx.last_marked_alive(500);
    fx.watch(2, |_| {}).unwrap();
    assert_eq!(fx.messages(), []);
    let text = fx.started_text();
    assert!(text.contains("it was last alive"), "{:?}", fx.texts("watch-started"));
    assert!(text.contains("The engine was down for 0.5 s"), "down from the lock's last mark, not from its start: {text}");
}

// ---------- @cpd_workers ----------

// Each fixture's dispatch PR has its `open` Job, which E4 adds at the first tick and which waits for the PR's other Jobs: one queued Job.
const NO_WORKERS: &str = "workers 0/3 · 0 waiting · 1 queued";
const ONE_WORKER: &str = "workers 1/3 · 0 waiting · 1 queued";

impl Fx {
    /// A tmux server with one session, `keepalive`.
    fn tmux_server(&self) {
        assert!(self.env.tmux(&["new-session", "-d", "-s", "keepalive", "sleep", "600"]).status.success());
    }

    fn workers(&self) -> String {
        self.option("@cpd_workers")
    }

    fn option(&self, name: &str) -> String {
        let out = self.env.tmux(&["show", "-gv", name]);
        String::from_utf8_lossy(&out.stdout).trim_end().to_owned()
    }
}

#[test]
fn without_a_tmux_server_the_workers_step_does_nothing_and_sets_the_text_once_one_appears() {
    let fx = fx("");
    let engine = fx.start();
    // time-claim: no error over more than one engine tick without a tmux server; no per-poll signal from the engine,
    // which records nothing at a tick that does nothing.
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
    assert!(!fx.env.tmux(&["has-session"]).status.success());

    fx.tmux_server();
    wait_until("@cpd_workers", || fx.workers() == NO_WORKERS);
    let out = engine.stop();
    assert_eq!(out.exit, 0, "{out:?}");
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

#[test]
fn a_tmux_failure_of_the_workers_step_is_one_error_and_the_engine_goes_on() {
    let fx = fx("");
    fx.tmux_server();
    // A tmux that fails `set` and is the real one otherwise, first on the engine's PATH.
    let real = std::env::var("PATH")
        .unwrap()
        .split(':')
        .map(|dir| Path::new(dir).join("tmux"))
        .find(|p| p.is_file())
        .expect("tmux on PATH");
    fx.env.fake("tmux", &format!("case \" $*\" in *\" set -g @cpd_workers\"*) echo boom >&2; exit 1;; esac\nexec '{}' \"$@\"", real.display()));
    let engine = fx.start();
    let workers_errors = || fx.texts("error").into_iter().filter(|t| t.contains("@cpd_workers")).collect::<Vec<_>>();
    wait_until("the error", || workers_errors().len() == 1);
    // time-claim: the failed set is not reported again over more than one engine tick; no per-poll signal from the
    // engine, which records nothing at a tick that sets nothing new.
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(workers_errors().len(), 1, "{:?}", workers_errors());
    assert!(workers_errors()[0].contains("tmux set failed: boom"), "{:?}", workers_errors());
    assert_eq!(fx.workers(), "");

    // It kept running, and sets the text once tmux works again.
    fx.env.fake("tmux", &format!("exec '{}' \"$@\"", real.display()));
    wait_until("@cpd_workers", || fx.workers() == NO_WORKERS);
    let out = engine.stop();
    assert_eq!(out.exit, 0, "{out:?}");
}

#[test]
fn ensure_says_the_exit_status_is_unknown_when_tmux_has_none() {
    let fx = fx("");
    // Killed by a signal: the pane is dead and `#{pane_dead_status}` is empty.
    fx.env.fake("cpd", "kill -KILL $$");
    let run = fx.env.cpd(&["watch", "--ensure"]);
    let error = run.error();
    assert_eq!(error["code"], "watch_start_failed");
    assert_eq!(error["details"]["exit"], Value::Null);
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("its exit status is unknown"), "{message}");
}

// ---------- E1 ----------

#[test]
fn e1_expires_open_escalations_whose_asker_is_dead() {
    let fx = fx("");
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let dead = gone.id();
    gone.wait().unwrap();
    let alive = std::process::id();
    let lock = fx.live_driver("api-a1");
    fx.add_running("api", "running", &ago(1));
    fx.fixture(&format!(
        "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
           VALUES ('pay', 'api-a1-q1', 'api-a1', 1, 'Why?', 'open', {dead}, '{t}'),
                  ('pay', 'api-a1-q2', 'api-a1', 2, 'How?', 'open', {alive}, '{t}');",
        t = ago(1)
    ));

    let engine = fx.start();
    wait_until("escalation-expired", || fx.count("escalation-expired") == 1);
    engine.stop();
    drop(lock);

    assert_eq!(
        fx.events("escalation-expired"),
        [(Some("pay".into()), Some("api".into()), Some("api-a1".into()), json!({"escalation": "api-a1-q1", "reason": "asker_dead"}))]
    );
    let status = |id: &str| fx.env.query::<String>(&format!("SELECT status FROM escalations WHERE id = '{id}'"));
    assert_eq!((status("api-a1-q1"), status("api-a1-q2")), ("expired".into(), "open".into()));
    assert!(fx.env.query::<Option<String>>("SELECT expired_at FROM escalations WHERE id = 'api-a1-q1'").is_some());
}

// ---------- E2 ----------

#[test]
fn e2_ends_attempts_whose_driver_died_crashed_or_cancelled() {
    let fx = fx("retry_cap = 0");
    fx.add_running("api", "running", &ago(1));
    fx.add_running("web", "starting", &ago(1));
    fx.request_cancel("web");

    let engine = fx.start();
    wait_until("both ended", || fx.attempt("api-a1").0 == "ended" && fx.attempt("web-a1").0 == "ended");
    engine.stop();

    assert_eq!(fx.attempt("api-a1"), ("ended".into(), Some("failed".into()), Some("crashed".into()), None));
    assert_eq!(fx.job_status("api"), "failed");
    let failed = fx.events("job-failed");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].3["cause"], "crashed");
    assert_eq!(failed[0].2.as_deref(), Some("api-a1"));
    assert_eq!(fx.attempt("web-a1"), ("ended".into(), Some("cancelled".into()), Some("cancel_requested".into()), None));
    assert_eq!(fx.job_status("web"), "cancelled");
    assert_eq!(fx.events("job-cancelled")[0].3, json!({"reason": "not needed"}));
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

#[test]
fn e2_removes_the_lock_of_a_dead_driver_it_ends_and_does_not_recreate_it() {
    let fx = fx("retry_cap = 0");
    // `api-a1`: a dead driver left its lock file, free. `web-a1`: the file is missing, as an ended
    // Attempt's is (E2's probe must not create it: no driver).
    drop(fx.live_driver("api-a1"));
    fx.add_running("api", "running", &ago(1));
    fx.add_running("web", "running", &ago(1));
    let lock = |attempt: &str| fx.home().attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(attempt).unwrap());
    assert!(lock("api-a1").exists() && !lock("web-a1").exists());

    let engine = fx.start();
    wait_until("both ended", || fx.attempt("api-a1").0 == "ended" && fx.attempt("web-a1").0 == "ended");
    // The lock goes with the end, and the next ticks do not bring it back.
    fx.env.wait_whole_ticks(2);
    engine.stop();

    assert!(!lock("api-a1").exists() && !lock("web-a1").exists());
    let left = fs::read_dir(fx.home().locks_dir()).unwrap().map(|e| e.unwrap().file_name()).collect::<Vec<_>>();
    assert!(left.iter().all(|n| !n.to_string_lossy().starts_with("attempt-")), "{left:?}");
}

#[test]
fn e2_climbs_the_ladder_for_a_dead_driver() {
    let fx = fx("concurrency = 1");
    // A live Attempt holds the only slot, so E3 does not start the next one.
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago(1));
    fx.add_running("api", "running", &ago(1));

    let engine = fx.start();
    wait_until("api ended", || fx.attempt("api-a1").0 == "ended");
    engine.stop();

    assert_eq!(fx.job_status("api"), "queued");
    let retry = fx.events("attempt-retry");
    assert_eq!(retry.len(), 1);
    assert_eq!((&retry[0].3["cause"], &retry[0].3["next_tier"]), (&json!("crashed"), &json!("S3")));
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts WHERE job = 'api'"), 1);
}

/// hs-n465, hs-d632: the catalog refuses the Tier of an Attempt whose driver died (`S2` when
/// `S` supports 0,1,3,5). E2 ends it and requeues the Job at the next valid Tier in the Worker
/// range at or above its effort (`S3`), never wedging with `tier_invalid` and leaving it `starting`.
#[test]
fn e2_ends_an_attempt_whose_tier_the_catalog_refuses_and_requeues_at_the_next_valid_tier() {
    let fx = fx("concurrency = 1");
    fx.fixture("UPDATE tier_catalog SET efforts = '[0,1,3,5]' WHERE letter = 'S';");
    // A live Attempt holds the only slot, so E3 does not start the next one after E2 queues `api`.
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago(1));
    fx.add_running("api", "starting", &ago(1));

    let engine = fx.start();
    wait_until("api ended", || fx.attempt("api-a1").0 == "ended");
    engine.stop();

    assert_ne!(fx.attempt("api-a1").0, "starting", "the Attempt stayed starting");
    assert_eq!(fx.job_status("api"), "queued");
    let retry = fx.events("attempt-retry");
    assert_eq!(retry.len(), 1, "{retry:?}");
    assert_eq!(
        (retry[0].1.as_deref(), retry[0].2.as_deref(), &retry[0].3["cause"], &retry[0].3["next_tier"]),
        (Some("api"), Some("api-a1"), &json!("crashed"), &json!("S3"))
    );
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'error' AND json_extract(data, '$.code') = 'tier_invalid'"),
        0,
        "{:?}",
        fx.texts("error")
    );
}

/// `attempt-status` check Events of `<key>-a1`: `(at, "start" | "end")`, in order.
fn check_events(fx: &Fx, key: &str, edges: &[(&str, &str)]) {
    for (at, edge) in edges {
        fx.fixture(&format!(
            "INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
               VALUES (cpd_txn(), '{at}', 'attempt-status', 'ctx', 'pay', '{key}', '{key}-a1', 'check',
                       '{{\"from\":\"running\",\"to\":\"running\",\"check\":\"{edge}\",\"what\":\"tests\"}}');"
        ));
    }
}

#[test]
fn e2_does_not_count_a_check_span_toward_the_attempts_timeout() {
    // hs-d497, contract §7.7: 31 minutes since `running`, 25 of them in a Gate (one closed span, one still open): not over 30 active.
    let fx = fx("");
    let _locks: Vec<_> = ["spent-a1", "over-a1"].iter().map(|a| fx.live_driver(a)).collect();
    fx.add_running("spent", "running", &ago(31));
    check_events(&fx, "spent", &[(&ago(30), "start"), (&ago(20), "end"), (&ago(15), "start")]);
    // The same time with a span of one minute: 30.x minutes active, which is over.
    fx.add_running("over", "running", &ago(31));
    check_events(&fx, "over", &[(&ago(10), "start"), (&ago(9), "end")]);

    let engine = fx.start();
    wait_until("the stop request", || fx.attempt("over-a1").3.is_some());
    fx.env.wait_whole_ticks(1);
    engine.stop();

    assert_eq!(fx.attempt("over-a1").3.as_deref(), Some("timed_out"));
    assert_eq!(fx.attempt("spent-a1"), ("running".into(), None, None, None), "a span still open is not active time either");
}

#[test]
fn e2_closes_a_check_span_whose_submit_died_and_leaves_one_that_is_held() {
    let fx = fx("");
    let _drivers: Vec<_> = ["dead-a1", "held-a1", "done-a1"].iter().map(|a| fx.live_driver(a)).collect();
    for key in ["dead", "held", "done"] {
        fx.add_running(key, "running", &ago(5));
    }
    check_events(&fx, "dead", &[(&ago(4), "start")]);
    check_events(&fx, "held", &[(&ago(4), "start")]);
    check_events(&fx, "done", &[(&ago(4), "start"), (&ago(3), "end")]);
    // `held`'s submit is alive: it holds the check lock.
    let path = fx.home().check_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse("held-a1").unwrap());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
    let _submit = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();

    let engine = fx.start();
    wait_until("the missing end", || fx.events("attempt-status").iter().any(|e| e.3["died"] == json!(true)));
    fx.env.wait_whole_ticks(2);
    engine.stop();

    let ends: Vec<_> = fx.events("attempt-status").into_iter().filter(|e| e.3["died"] == json!(true)).collect();
    assert_eq!(ends.len(), 1, "once, and only for the span nobody holds: {ends:?}");
    assert_eq!(
        (ends[0].1.as_deref(), ends[0].2.as_deref(), &ends[0].3),
        (Some("dead"), Some("dead-a1"), &json!({"from": "running", "to": "running", "check": "end", "what": "tests", "died": true}))
    );
}

/// Attempt `api-a1` of Plan `pay` in `merging`, its worktree cut from the Lead Branch with one commit, its driver
/// dead. With `moved` the driver had moved the Lead Branch (step 8) and synced the Lead worktree before it died.
/// Returns the worktree and the tip.
fn merging_api(fx: &Fx, moved: bool) -> (git::Created, String) {
    let attempt = AttemptId::parse("api-a1").unwrap();
    let setup = Setup { timeout: Duration::from_secs(30), log: fx.env.root().join("attempt.log") };
    let made = create_attempt_worktree(&fx.repo, &fx.lead, &PlanId::parse(PLAN).unwrap(), &attempt, &setup).unwrap();
    fs::write(made.path.join("api.txt"), "api\n").unwrap();
    fx.env.git(&made.path, &["add", "-A"]);
    fx.env.git(&made.path, &["commit", "-q", "-m", "api"]);
    let tip = fx.env.git(&made.path, &["rev-parse", "HEAD"]);
    if moved {
        fx.env.git(&fx.lead, &["merge", "-q", "--ff-only", &made.branch]);
    }
    fx.add_job(
        "api",
        &[],
        &format!(
            "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'api';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, submitted_sha, submitted_at, started_at)
               VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'merging', '{}', '{}', '{}', '{tip}', '{t}', '{t}');",
            made.branch,
            made.path.display(),
            made.base_sha,
            t = ago(1)
        ),
    );
    (made, tip)
}

#[test]
fn e2_recovers_a_merge_whose_driver_died_after_the_compare_and_swap() {
    let fx = fx("");
    // The driver moved the Lead Branch (step 8) and synced the Lead worktree, then died.
    let (made, tip) = merging_api(&fx, true);

    let engine = fx.start();
    wait_until("the merge recorded", || fx.attempt("api-a1").0 == "ended");
    engine.stop();

    assert_eq!(fx.attempt("api-a1"), ("ended".into(), Some("merged".into()), Some("merged".into()), None));
    assert_eq!(fx.job_status("api"), "merged");
    assert_eq!(fx.events("job-merged")[0].3, json!({"sha": tip, "tier": "S2", "kind": "work", "turn": false}));
    assert!(!made.path.exists());
}

// ---------- E3 ----------

#[test]
fn e3_one_tick_schedules_a_ready_job_within_the_concurrency() {
    let fx = fx("concurrency = 1\nretry_cap = 0");
    // The work session as tui leaves it, with remain-on-exit (§6.1).
    for args in [&["new-session", "-d", "-s", "cpd-work", "-n", "x", "sleep", "600"][..], &["set", "-g", "remain-on-exit", "on"]] {
        assert!(fx.env.tmux(args).status.success(), "tmux {args:?}");
    }
    fx.add_job("api", &[], "");
    fx.add_job("web", &[], "");
    fx.add_job("ui", &["api"], "");

    let engine = fx.start();
    wait_until("attempt-started", || fx.count("attempt-started") == 1);
    engine.stop();

    let started = fx.events("attempt-started");
    assert_eq!(started, [(Some("pay".into()), Some("api".into()), Some("api-a1".into()), json!({"number": 1, "tier": "S2", "origin": "ladder"}))]);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts"), 1);
    assert_eq!((fx.job_status("web"), fx.job_status("ui")), ("queued".into(), "queued".into()));
    assert!(fx.env.query::<Option<i64>>("SELECT driver_pid FROM attempts WHERE id = 'api-a1'").is_some());
    // Its driver runs detached; the Worker exits at once, so it ends crashed.
    wait_until_for("the driver to end api-a1", Duration::from_secs(60), || fx.attempt("api-a1").0 == "ended");
}

/// hs-n465, hs-d632: a stored Job whose Tier the catalog refuses (`S2` when `S` supports
/// 0,1,3,5) is started at the next valid Tier in the Worker range at or above its effort
/// (`S3`), never wedging with `tier_invalid`.
#[test]
fn e3_starts_a_job_whose_stored_tier_the_catalog_refuses_at_the_next_valid_tier() {
    let fx = fx("concurrency = 1\nretry_cap = 0");
    fx.fixture("UPDATE tier_catalog SET efforts = '[0,1,3,5]' WHERE letter = 'S';");
    // The work session as tui leaves it, with remain-on-exit (§6.1).
    for args in [&["new-session", "-d", "-s", "cpd-work", "-n", "x", "sleep", "600"][..], &["set", "-g", "remain-on-exit", "on"]] {
        assert!(fx.env.tmux(args).status.success(), "tmux {args:?}");
    }
    fx.add_job_on("S2", "repo", "api", &[], "");

    let engine = fx.start();
    wait_until("attempt-started", || fx.count("attempt-started") == 1);
    engine.stop();

    let started = fx.events("attempt-started");
    assert_eq!(started, [(Some("pay".into()), Some("api".into()), Some("api-a1".into()), json!({"number": 1, "tier": "S3", "origin": "ladder"}))]);
    assert_eq!(fx.env.query::<String>("SELECT tier FROM attempts WHERE id = 'api-a1'"), "S3");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts"), 1);
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'error' AND json_extract(data, '$.code') = 'tier_invalid'"),
        0,
        "{:?}",
        fx.texts("error")
    );
    // Its driver runs detached; the Worker exits at once, so it ends crashed.
    wait_until_for("the driver to end api-a1", Duration::from_secs(60), || fx.attempt("api-a1").0 == "ended");
}

/// SQL that marks Job `key` urgent, for `add_job`'s `then`.
fn urgent(key: &str) -> String {
    format!("UPDATE jobs SET urgent = 1 WHERE plan = 'pay' AND key = '{key}';")
}

/// The work session as tui leaves it, with remain-on-exit (§6.1), and a Worker that stays alive.
fn work_session_with_a_worker_that_stays(fx: &Fx) {
    fx.env.fake_pi(&format!("sleep {SLEEP_SECONDS}"));
    for args in [&["new-session", "-d", "-s", "cpd-work", "-n", "x", "sleep", &SLEEP_SECONDS.to_string()][..], &["set", "-g", "remain-on-exit", "on"]] {
        assert!(fx.env.tmux(args).status.success(), "tmux {args:?}");
    }
}

/// An urgent Job may start one Attempt beyond `concurrency`, and no more; the extra slot is never a non-urgent Job's.
#[test]
fn e3_starts_one_urgent_job_beyond_the_concurrency_and_not_a_second() {
    let fx = fx("concurrency = 1\nretry_cap = 0");
    work_session_with_a_worker_that_stays(&fx);
    // One Attempt is running: the only slot is taken.
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago(1));
    fx.add_job("old", &[], "");
    fx.add_job("hot1", &[], &urgent("hot1"));
    fx.add_job("hot2", &[], &urgent("hot2"));

    let engine = fx.start();
    wait_until("attempt-started", || fx.count("attempt-started") >= 1);
    // Two more ticks: the second urgent Job and the older one still wait.
    fx.env.wait_whole_ticks(2);
    engine.stop();

    // The first urgent Job by `seq` started (running Attempts: 2 = concurrency + 1); the second urgent one and the older Job wait.
    let started: Vec<Option<String>> = fx.events("attempt-started").into_iter().map(|e| e.1).collect();
    assert_eq!(started, [Some("hot1".to_owned())]);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts WHERE status <> 'ended'"), 2);
    assert_eq!((fx.job_status("hot1"), fx.job_status("hot2"), fx.job_status("old")), ("running".into(), "queued".into(), "queued".into()));
}

/// The 00:02Z incident (hs-d151): the file the engine runs from is replaced, then
/// E3 starts an Attempt. The driver starts from the path recorded at start
/// (`cannot run <path> (deleted)` before).
#[test]
fn e3_starts_the_driver_after_the_engines_binary_is_replaced() {
    let fx = fx("concurrency = 1\nretry_cap = 0");
    for args in [&["new-session", "-d", "-s", "cpd-work", "-n", "x", "sleep", "600"][..], &["set", "-g", "remain-on-exit", "on"]] {
        assert!(fx.env.tmux(args).status.success(), "tmux {args:?}");
    }
    let installed = fx.env.root().join("cpd-installed");
    copy_executable(CPD, &installed);
    let engine = fx.start_from(&installed);

    let new = fx.env.root().join("cpd-new");
    copy_executable(CPD, &new);
    fs::rename(&new, &installed).unwrap();
    fx.add_job("api", &[], "");

    wait_until("attempt-started", || fx.count("attempt-started") == 1);
    engine.stop();
    wait_until_for("the driver to end api-a1", Duration::from_secs(60), || fx.attempt("api-a1").0 == "ended");
    assert!(fx.env.query::<Option<i64>>("SELECT driver_pid FROM attempts WHERE id = 'api-a1'").is_some(), "the driver was not started");
    let texts = fx.texts("attempt-ended");
    assert!(texts.iter().all(|t| !t.contains("cannot run") && !t.contains("deleted")), "{texts:?}");
    assert!(!fx.texts("error").iter().any(|t| t.contains("deleted")));
}

/// The 00:02Z incident, reproduced: the engine cannot run the driver (its file is gone), so every Attempt
/// ends `failed` / `crashed` with `driver_pid` null. No Tier is climbed, no try is used up (`retry_cap = 0`
/// would fail the Job at the first), and E3 waits `2^k` seconds after the k-th failure: `2^k` units, the unit a
/// tenth of a second here (`CPD_TEST_BACKOFF_UNIT`, contract §1.2 "Test overrides"), over 0.1 s ticks.
#[test]
fn e3_spawn_failures_requeue_at_the_same_tier_never_fail_the_job_and_back_off() {
    let fx = fx("concurrency = 1\nretry_cap = 0");
    let unit = Duration::from_millis(50);
    let installed = fx.env.root().join("cpd-installed");
    copy_executable(CPD, &installed);
    let engine = fx.start_with(&installed, &[("CPD_TEST_BACKOFF_UNIT", "0.05")]);
    fs::remove_file(&installed).unwrap();
    fx.add_job("api", &[], "");

    wait_until("three spawn failures", || fx.env.query::<i64>("SELECT count(*) FROM attempts WHERE status = 'ended'") >= 3);
    engine.stop();

    assert_ne!(fx.job_status("api"), "failed");
    assert_eq!(fx.count("job-failed"), 0);
    let attempts: Vec<(String, String, String, Option<i64>, String, String)> = fx.env.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, tier, origin, driver_pid, started_at, ended_at FROM attempts WHERE job = 'api' AND status = 'ended' ORDER BY number",
        )?;
        Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect::<rusqlite::Result<_>>()?)
        });
    assert!(attempts.len() >= 3, "{attempts:?}");
    for (id, tier, origin, pid, _, _) in &attempts {
        assert_eq!((tier.as_str(), origin.as_str(), *pid), ("S2", "ladder", None), "{id}");
    }
    for (i, (id, ..)) in attempts.iter().enumerate() {
        assert_eq!(fx.attempt(id), ("ended".into(), Some("failed".into()), Some("crashed".into()), None));
        let retry = &fx.events("attempt-retry")[i];
        assert_eq!((&retry.3["cause"], &retry.3["next_tier"]), (&json!("crashed"), &json!("S2")));
    }
    // After the k-th failure the next start waits 2^k units.
    for (k, pair) in attempts.windows(2).enumerate() {
        let waited = parse_time(&pair[1].4) - parse_time(&pair[0].5);
        let k = (k + 1) as u32;
        assert!(waited >= unit * 2u32.pow(k), "after failure {k}: waited {waited}");
    }
}

/// A started driver resets the back-off, and only Attempts whose driver started use up a try: `web`
/// has a spawn failure (`a1`) and a started, crashed `a2` before `a3` dies at `retry_cap = 1`.
#[test]
fn e2_counts_only_attempts_whose_driver_started() {
    let fx = fx("concurrency = 1\nretry_cap = 1");
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago(1));
    let running = |key: &str, number: u32, tier: &str| {
        format!(
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = '{key}';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, driver_pid)
           VALUES ('pay', '{key}-a{number}', '{key}', {number}, '{tier}', 'ladder', 'running', 'cpd/pay--{key}-a{number}', '/w', '{}', 4242);",
        ago(1)
        )
    };
    // `web`: spawn failure, then a started driver that died: one try used, the ladder climbs.
    fx.add_job("web", &[], &(ended_attempt("web", 1, "S2", None, &ago(2)) + &running("web", 2, "S2")));
    // `api`: a started driver that crashed, then another that died: both tries used, the Job fails.
    fx.add_job("api", &[], &(ended_attempt("api", 1, "S2", Some(4242), &ago(2)) + &running("api", 2, "S3")));

    let engine = fx.start();
    wait_until("both ended", || fx.attempt("web-a2").0 == "ended" && fx.attempt("api-a2").0 == "ended");
    engine.stop();

    assert_eq!(fx.job_status("web"), "queued");
    let retry = fx.events("attempt-retry");
    assert_eq!(retry.len(), 1, "{retry:?}");
    assert_eq!((retry[0].1.as_deref(), &retry[0].3["next_tier"]), (Some("web"), &json!("S3")));
    assert_eq!(fx.job_status("api"), "failed");
    let failed = fx.events("job-failed");
    assert_eq!((failed.len(), failed[0].1.as_deref(), &failed[0].3["cause"]), (1, Some("api"), &json!("crashed")));
}

/// hs-n278: a driver that an install or a store migration ended (it left its `.infra` marker) is not its Worker's
/// failure: E2 ends it as one whose driver never started, so the Job is queued at the same Tier and `retry_cap`
/// does not count it. A driver that died with no marker still climbs the ladder and counts.
#[test]
fn e2_ends_a_driver_ended_by_the_store_at_the_same_tier_uncounted_and_other_crashes_still_climb() {
    let fx = fx("concurrency = 1\nretry_cap = 1");
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago(1));
    let running = |key: &str, number: u32, tier: &str| {
        format!(
            "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = '{key}';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, driver_pid)
               VALUES ('pay', '{key}-a{number}', '{key}', {number}, '{tier}', 'ladder', 'running', 'cpd/pay--{key}-a{number}', '/w', '{}', 4242);",
            ago(1)
        )
    };
    // `web`: two Attempts that never had a driver, then a driver the store ended: three that are not the
    // Worker's failures, more than `retry_cap = 1` allows if they counted.
    let before = ended_attempt("web", 1, "S2", None, &ago(5)) + &ended_attempt("web", 2, "S2", None, &ago(4));
    fx.add_job("web", &[], &(before + &running("web", 3, "S2")));
    // `api`: the same Attempts, but its last driver died with no marker: the ladder counts it.
    fx.add_job("api", &[], &(ended_attempt("api", 1, "S2", Some(4242), &ago(2)) + &running("api", 2, "S3")));
    let plan = PlanId::parse(PLAN).unwrap();
    fx.home().leave_infra(&plan, &AttemptId::parse("web-a3").unwrap(), Infra::StoreTooNew, "The store is newer than this cpd.").unwrap();

    let engine = fx.start();
    wait_until("both ended", || fx.attempt("web-a3").0 == "ended" && fx.attempt("api-a2").0 == "ended");
    engine.stop();

    // `web-a3`: same Tier S2, not counted, and the marker went with the Attempt.
    assert_eq!(fx.job_status("web"), "queued");
    assert_eq!(fx.attempt("web-a3"), ("ended".into(), Some("failed".into()), Some("crashed".into()), None));
    assert_eq!(fx.env.query::<Option<i64>>("SELECT driver_pid FROM attempts WHERE id = 'web-a3'"), None);
    assert!(!fx.home().attempt_infra(&plan, &AttemptId::parse("web-a3").unwrap()).exists());
    let retry: Vec<_> = fx.events("attempt-retry").into_iter().filter(|e| e.1.as_deref() == Some("web")).collect();
    assert_eq!((retry.len(), &retry[0].3["next_tier"]), (1, &json!("S2")), "{retry:?}");
    assert!(retry[0].3["feedback"].as_str().unwrap().contains("store_too_new"), "{retry:?}");
    // `api-a2`: a crash that counts: both tries used, the Job fails.
    assert_eq!(fx.job_status("api"), "failed");
    assert_eq!(fx.events("job-failed").len(), 1);
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

// ---------- a busy store (contract §3.7, §4.2) ----------

/// A connection that is not cpd's holding the write lock until [`Held::release`] or its drop.
struct Held(rusqlite::Connection);

fn hold_write_lock(fx: &Fx) -> Held {
    let conn = rusqlite::Connection::open(fx.home().db()).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    Held(conn)
}

impl Held {
    fn release(self) {
        self.0.execute_batch("ROLLBACK").unwrap();
    }
}

fn watch_log(fx: &Fx) -> String {
    fs::read_to_string(fx.env.cpd_home().join("run/watch.log")).unwrap_or_default()
}

/// `store_busy_ms` of 200 ms: far shorter than the locks these tests hold.
const SHORT_BUSY: &str = "[timeouts]\nstore_busy_ms = 200\n";

/// `cpd watch` started and not waited for.
fn spawn_engine(fx: &Fx) -> Engine {
    let child = fx.env.command(CPD).arg("watch").envs([PR_POLL_EVERY_TICK]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    Engine { child: Some(child) }
}

/// A store that stays busy for longer than `store_busy_ms` does not end the engine, at its start or in a
/// step, and no Event is lost: it waits, logs, and goes on when the lock is released.
#[test]
fn a_store_busy_for_longer_than_store_busy_ms_never_ends_the_engine_or_loses_an_event() {
    let fx = fx(SHORT_BUSY);
    // E5's poll says when it runs, and lasts until the test lets it end (`go`): the engine is then in its first tick, past E4.
    let (marker, go) = (fx.env.root().join("gh-ran"), fx.env.root().join("gh-go"));
    fx.env.fake_gh(&format!(
        "touch '{}'; until [ -e '{go}' ]; do sleep 0.01; done; echo '{EMPTY_SEARCH}'",
        marker.display(),
        go = go.display()
    ));
    fx.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api';");

    // The start: `watch-started` waits for the lock.
    let held = hold_write_lock(&fx);
    let mut engine = spawn_engine(&fx);
    // Its first `store_busy` is logged once the lock outlasted `store_busy_ms`; an engine that ended on it never appends `watch-started`.
    wait_until("the engine to meet the lock at its start", || watch_log(&fx).contains("store_busy: "));
    assert_eq!(engine.child.as_mut().unwrap().try_wait().unwrap(), None, "the engine exited");
    assert_eq!(fx.count("watch-started"), 0, "{}", watch_log(&fx));
    held.release();
    wait_until("watch-started", || fx.count("watch-started") == 1);
    wait_until("E5's poll", || marker.exists());

    // A step: the second tick's E4 meets the lock, taken while E5 of the first tick is still running. The first tick added `open-repo`;
    // an open Job that merged on a PR still `dispatch` counts for nothing, so E4 has a Job to add (a write) at the second tick.
    assert_eq!(fx.jobs_of_kind("open").len(), 1);
    fx.fixture("UPDATE jobs SET status = 'running' WHERE key = 'open-repo'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'open-repo';");
    let busy = watch_log(&fx).matches("store_busy: ").count();
    let held = hold_write_lock(&fx);
    fs::write(&go, "").unwrap();
    wait_until("the engine to meet the lock", || watch_log(&fx).matches("store_busy: ").count() > busy);
    assert_eq!(fx.jobs_of_kind("open").len(), 1);
    held.release();
    wait_until("the fresh open Job", || fx.jobs_of_kind("open").len() == 2);
    assert_eq!(engine.child.as_mut().unwrap().try_wait().unwrap(), None, "the engine exited");
    engine.stop();

    let log = watch_log(&fx);
    assert!(log.contains("store_busy: "), "{log}");
    assert!(log.contains("the store is clear again after "), "{log}");
    assert_eq!(fx.count("error"), 0, "{log}");
    assert_eq!(fx.count("job-added"), 2, "each open Job's Event was appended once");
}

/// `watch-stopped` is appended when the engine stops while the store is busy, however long that lasts.
#[test]
fn watch_stopped_waits_for_a_busy_store() {
    let fx = engine(SHORT_BUSY);
    let home = fx.home();
    let (db, log) = (home.db(), home.watch_log());
    let held = Arc::new(Mutex::new(None::<Connection>));
    // What the test saw while the stop waited: (the `watch-stopped` Events, the log after "stopping on a signal").
    let seen = Arc::new(Mutex::new(None::<(i64, String)>));
    let (taken, looked) = (held.clone(), seen.clone());
    let out = fx
        .watch(1, move |nap| match nap {
            // The stop is requested (`until`, set when this nap passes) while a connection that is not cpd's holds the write lock.
            1 => {
                let other = Connection::open(&db).unwrap();
                other.busy_timeout(Duration::ZERO).unwrap();
                other.execute_batch("BEGIN IMMEDIATE").unwrap();
                *taken.lock().unwrap() = Some(other);
            }
            // The stop has met the lock for longer than `store_busy_ms`: its `store_busy` is logged after the stop. Let go.
            3 => {
                let count = Connection::open(&db).unwrap().query_row("SELECT count(*) FROM events WHERE kind = 'watch-stopped'", [], |r| r.get(0)).unwrap();
                let text = fs::read_to_string(&log).unwrap_or_default();
                *looked.lock().unwrap() = Some((count, text));
                taken.lock().unwrap().take().expect("held").execute_batch("ROLLBACK").unwrap();
            }
            _ => {}
        })
        .unwrap();

    let (count, text) = seen.lock().unwrap().take().expect("the stop waited for the store");
    assert_eq!(count, 0, "the engine exited without watch-stopped");
    assert!(text.split("stopping on a signal").nth(1).is_some_and(|after| after.contains("store_busy: ")), "{text}");
    assert_eq!(out, Output::success(json!({"stopped": true})));
    assert_eq!(fx.events("watch-stopped"), [(None, None, None, json!({"pid": std::process::id()}))]);
    assert!(!engine_log(&fx).contains("cannot append watch-stopped"), "{}", engine_log(&fx));
}

// ---------- E3: a draft ----------

// ---------- E4: the open Job (contract §3.7 E4, §3.2 Ready, §2.2 "Job kinds"; hs-d340) ----------

impl Fx {
    /// Job `key` of PR `repo`, a draft: the PR's open Job waits for it, and E3 never starts it.
    fn add_draft(&self, key: &str) {
        self.add_draft_in("repo", key);
    }

    /// `add_draft` in PR `pr`.
    fn add_draft_in(&self, pr: &str, key: &str) {
        self.fixture(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', '{pr}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 1, cpd_txn(), 't');"
        ));
    }

    /// Job `key` of PR `repo` as cpd adds an `open` or `update` Job (`kind`): `appended`, no Dependencies, `queued` with its
    /// fingerprint, then moved by `then` (SQL).
    fn add_cpd_job(&self, key: &str, kind: &str, then: &str) {
        self.add_cpd_job_in("repo", key, kind, then);
    }

    /// `add_cpd_job` in PR `pr`.
    fn add_cpd_job_in(&self, pr: &str, key: &str, kind: &str, then: &str) {
        self.fixture(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, created_txn, created_at)
             VALUES ('pay', '{pr}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 1, '{kind}', cpd_txn(), 't');"
        ));
        let plan = PlanId::parse(PLAN).unwrap();
        let all = self.store().read(|conn| jobs::load(conn, &plan)).unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.fixture(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = '{key}'; {then}",
            locked_fingerprint(job)
        ));
    }

    /// `(key, status)` of every Job of `kind`, by key.
    fn jobs_of_kind(&self, kind: &str) -> Vec<(String, String)> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT key, status FROM jobs WHERE kind = ?1 ORDER BY key")?;
            Ok(stmt.query_map([kind], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }
}

/// PR `repo` (an `open` Job the Lead cancelled) beside PR `web`, which has none: `web`'s own `open-web` is what shows the engine has
/// looked at both.
fn fx_cancelled_open_job() -> Fx {
    let fx = fx("");
    fx.add_merged_api();
    fx.add_cpd_job("open-repo", "open", "UPDATE jobs SET status = 'cancelled', cancel_reason = 'never to be opened', cancel_requested_at = 't' WHERE key = 'open-repo';");
    fx.fixture(&format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'web', '{}', 'main', 'dispatch', 'cpd/pay/web', '{}', 'Show the receipt', 'config', cpd_txn(), 't', 't');",
        fx.repo.display(),
        fx.lead.display()
    ));
    fx.add_draft_in("web", "later-web");
    fx
}

/// The ways on after that cancel (§3.8, §7.6): `pr open --pr repo` is the repair and is not refused `open_job_live` (no `open` Job is
/// live); E4 still adds nothing, before or after it.
#[test]
fn the_way_back_from_a_cancelled_open_job_is_pr_open_which_is_not_refused_open_job_live() {
    let fx = fx_cancelled_open_job();
    let engine = fx.start();
    wait_until("the open Job of PR web", || fx.jobs_of_kind("open").iter().any(|(key, _)| key == "open-web"));
    fx.env.wait_whole_ticks(2);

    let run = fx.env.cpd(&["pr", "open", "--pr", "repo", "--plan", "pay"]);
    fx.env.wait_whole_ticks(2);
    engine.stop();

    let json = run.json();
    assert_ne!(json["error"]["code"], "open_job_live", "{run:?}");
    let open: Vec<String> = fx.jobs_of_kind("open").into_iter().map(|(key, _)| key).collect();
    assert_eq!(open, ["open-repo", "open-web"], "no fresh open Job for PR repo");
}

/// The other way on is `pr cancel`: it cancels the PR's own queued `open` Job with the PR, and E4 re-adds nothing (the PR is no longer
/// `dispatch`), over several ticks. PR `repo` is the one the engine is seen to look at.
#[test]
fn pr_cancel_cancels_the_prs_own_open_job_with_it_and_e4_adds_none() {
    let fx = fx("");
    fx.add_merged_api();
    fx.fixture(&format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'web', '{}', 'main', 'dispatch', 'cpd/pay/web', '{}', 'Show the receipt', 'config', cpd_txn(), 't', 't');",
        fx.repo.display(),
        fx.lead.display()
    ));
    fx.add_cpd_job_in("web", "open-web", "open", "");
    fx.env.cpd(&["pr", "cancel", "web", "--plan", "pay"]).ok();

    let engine = fx.start();
    wait_until("the open Job of PR repo", || fx.jobs_of_kind("open").iter().any(|(key, _)| key == "open-repo"));
    fx.env.wait_whole_ticks(3);
    engine.stop();

    let web: Vec<_> = fx.jobs_of_kind("open").into_iter().filter(|(key, _)| key.starts_with("open-web")).collect();
    assert_eq!(web, [("open-web".to_owned(), "cancelled".to_owned())]);
    assert_eq!(fx.env.query::<String>("SELECT status FROM prs WHERE plan = 'pay' AND key = 'web'"), "cancelled");
}

// ---------- E5: the update Job on a conflict (contract §3.7 E5, §7.6 step 6; hs-d340, hs-d497) ----------

/// Plan `pay` in `deploy` with PR `repo` `open` (pull request `acme/pay` #812, stored `mergeable`, `clean`), Job `api` merged, and a queued
/// Job `blocked` that waits for a draft, so no other Job of the PR could run: E3 starts nothing. The fake `gh`'s search answers
/// what `search.json` says; the test writes it.
fn fx_open_pr() -> Fx {
    let fx = fx_bare("");
    fx.fixture(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'deploy', 'lead1', 't', 't');
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, mergeable, merge_state, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 812, 'https://github.com/acme/pay/pull/812', 'Pay the bills', 0, 'open', 'none', 'none', 'mergeable', 'clean', 'opened', NULL, 'opened');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at, pr_number, pr_url, pr_opened_at, pull_request)
           VALUES ('pay', 'repo', '{repo}', 'main', 'open', 'cpd/pay', '{lead}', '{PR_TITLE}', 'config', cpd_txn(), 't', 't', 812, 'https://github.com/acme/pay/pull/812', 'opened', (SELECT id FROM pull_requests));",
        repo = fx.repo.display(),
        lead = fx.lead.display()
    ));
    fx.add_merged_api();
    fx.add_draft("later");
    fx.add_job("blocked", &["later"], "");
    let search = fx.env.root().join("search.json");
    fs::write(&search, common::pay_search("MERGEABLE", "CLEAN")).unwrap();
    fx.env.fake_gh(&format!("cat '{}'", search.display()));
    fx
}

impl Fx {
    /// What the fake `gh`'s search says of pull request 812.
    fn github_says(&self, mergeable: &str, merge_state: &str) {
        fs::write(self.env.root().join("search.json"), common::pay_search(mergeable, merge_state)).unwrap();
    }
}

/// The poll queues the PR's `update` Job in the transaction that stores `conflicting`, once per transition, and a base that merely moved
/// (`BEHIND`, still `MERGEABLE`) queues nothing (hs-d497: no `behind` detection). The conflict is no `pr-feedback`.
#[test]
fn e5_queues_one_update_job_when_the_pull_request_conflicts_and_none_for_a_base_that_merely_moved() {
    let fx = fx_open_pr();
    let engine = fx.start();

    fx.github_says("MERGEABLE", "BEHIND");
    wait_until("the poll to store that the base moved", || fx.env.query::<String>("SELECT merge_state FROM pull_requests WHERE number = 812") == "behind");
    fx.env.wait_whole_ticks(3);
    assert_eq!(fx.jobs_of_kind("update"), Vec::<(String, String)>::new(), "a base that merely moved queues nothing");

    fx.github_says("CONFLICTING", "DIRTY");
    wait_until("the update Job", || !fx.jobs_of_kind("update").is_empty());
    fx.env.wait_whole_ticks(3);
    engine.stop();

    assert_eq!(fx.jobs_of_kind("update"), [("up-repo".to_owned(), "queued".to_owned())], "once per conflict, however many polls see it");
    let row: (String, String, i64, bool) = fx.env.read(|conn| {
        Ok(conn.query_row(
            "SELECT title, tier, appended, fingerprint IS NOT NULL FROM jobs WHERE plan = 'pay' AND key = 'up-repo'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?)
    });
    assert_eq!(row, (format!("Bring the pull request up to date: {PR_TITLE}"), "S1".into(), 1, true));
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM dependencies WHERE plan = 'pay' AND job = 'up-repo'"), 0);
    let added = fx.events("job-added");
    assert_eq!(added.len(), 1, "{added:?}");
    let data = &added[0].3;
    assert_eq!((&data["kind"], &data["by"], &data["reason"], &data["pr"]), (&json!("update"), &json!("poll"), &json!("conflict"), &json!("repo")));
    assert_eq!(fx.count("pr-feedback"), 0, "a conflict is no pr-feedback reason any more");
    assert_eq!(fx.env.query::<String>("SELECT status FROM prs WHERE plan = 'pay'"), "open", "the PR's status does not change");
}

/// The update Job that ended is a cause that ended: a conflict that comes after it is a new transition, and a new Job, `up-repo-2`.
#[test]
fn e5_queues_a_new_update_job_for_a_conflict_that_comes_after_the_first_job_ended() {
    let fx = fx_open_pr();
    let engine = fx.start();
    fx.github_says("CONFLICTING", "DIRTY");
    wait_until("the update Job", || !fx.jobs_of_kind("update").is_empty());

    // The Job ran and merged; GitHub recomputes (`unknown`), then says the pull request is clean, then conflicts again.
    fx.fixture("UPDATE jobs SET status = 'running' WHERE key = 'up-repo'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'up-repo';");
    fx.github_says("UNKNOWN", "UNKNOWN");
    fx.env.wait_whole_ticks(2);
    fx.github_says("MERGEABLE", "CLEAN");
    fx.env.wait_whole_ticks(2);
    assert_eq!(fx.jobs_of_kind("update").len(), 1);
    fx.github_says("CONFLICTING", "DIRTY");
    wait_until("the second update Job", || fx.jobs_of_kind("update").len() == 2);
    engine.stop();

    assert_eq!(fx.jobs_of_kind("update"), [("up-repo".to_owned(), "merged".to_owned()), ("up-repo-2".to_owned(), "queued".to_owned())]);
}

impl Fx {
    /// The Context's live Manager, Session `mgr1`.
    fn add_manager(&self) {
        self.fixture(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('mgr1', 'manager', 'ctx', NULL, 'Manager', '/', 'O5', 'idle', 't', 't');",
        );
    }

    /// Job `api` merged: the PR's open Job can be Ready once nothing else of the PR is left.
    fn add_merged_api(&self) {
        self.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");
    }
}

// ---------- E5 ----------

/// Plan `pay` in phase `deploy`.
fn in_pr(fx: &Fx) {
    fx.fixture(
        "UPDATE plans SET phase = 'deploy' WHERE id = 'pay';
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
           VALUES ('acme/pay', 7, 'https://github.com/acme/pay/pull/7', 'Pay', 0, 'open', 'none', 'none', 't', 't');
         UPDATE prs SET status = 'open', pr_number = 7, pr_url = 'https://github.com/acme/pay/pull/7', pr_opened_at = 't',
            pull_request = (SELECT id FROM pull_requests WHERE number = 7) WHERE plan = 'pay';",
    );
}

/// What the fake `gh` answers to the search when it found no pull request, with `remaining` requests left until `reset_at`.
fn search_reply(remaining: u32, reset_at: &str) -> String {
    format!(r#"{{"data":{{"search":{{"issueCount":0,"nodes":[]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":{remaining},"resetAt":"{reset_at}"}}}}}}"#)
}

/// Milliseconds since the epoch, as the fake `gh` writes them.
fn epoch_ms(at: &str) -> i128 {
    OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).unwrap().unix_timestamp_nanos() / 1_000_000
}

#[test]
fn e5_polls_every_pr_poll_seconds_after_the_previous_poll_ended_whatever_the_plans() {
    // No Plan at all: E5 polls anyway.
    let fx = fx_bare("");
    let log = fx.env.root().join("poll.log");
    // A poll that lasts 0.2 s, so that "after the end" differs from "after the start", and that notes both.
    fx.env.fake_gh(&format!(
        r#"echo "start $({now})" >> '{log}'; sleep 0.2; echo "end $({now})" >> '{log}'; echo '{}'; exit 0"#,
        search_reply(4987, "2099-01-01T00:00:00Z"),
        now = NOW_NS,
        log = log.display()
    ));

    // The poll period, `pr_poll_seconds`, is 0.3 s here (its test override): longer than a poll and than a tick.
    let engine = fx.start_with(Path::new(CPD), &[("CPD_TEST_PR_POLL", "0.3")]);
    wait_until("three polls", || fs::read_to_string(&log).map_or(0, |t| t.matches("end").count()) >= 3);
    engine.stop();

    // start, end, start, end, …: a poll never starts before the previous one ended, and `pr_poll_seconds` after its end.
    let notes: Vec<(String, i128)> = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|l| {
        let (what, ns) = l.split_once(' ').unwrap();
        (what.to_owned(), ns.parse::<i128>().unwrap() / 1_000_000)
    })
        .collect();
    for (i, pair) in notes.windows(2).enumerate() {
        match (i % 2, pair[0].0.as_str(), pair[1].0.as_str()) {
            (0, "start", "end") => assert!(pair[1].1 - pair[0].1 >= 200, "{notes:?}"),
            (1, "end", "start") => assert!(pair[1].1 - pair[0].1 >= 300, "the next poll starts 0.3 s after the end of the last: {notes:?}"),
            _ => panic!("polls overlap or are out of order: {notes:?}"),
        }
    }
    assert_eq!(fx.count("error"), 0);
}

#[test]
fn e5_reports_a_failing_poll_once() {
    let fx = fx("");
    in_pr(&fx);
    fx.env.fake_gh("echo 'gh: boom' >&2; exit 1");

    let engine = fx.start();
    wait_until("three polls", || fx.env.calls("gh").len() >= 3);
    engine.stop();

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!((plan, job, attempt), (&None, &None, &None));
    assert_eq!((&data["source"], &data["code"], &data["details"]["exit"]), (&json!("watch"), &json!("gh_failed"), &json!(1)));
}

// ---------- E5: a gh HTTP 5xx or timeout is transient (hs-d354) ----------

/// What GitHub's 502 looks like through `gh api graphql`: nginx's HTML page on stdout and `gh`'s message on stderr, exit 1.
const GH_502: &str = "echo '<!DOCTYPE html><html><head><title>502 Bad Gateway</title></head><body><center><h1>502 Bad Gateway</h1></center><hr><center>nginx</center></body></html>'; echo 'gh: HTTP 502: Bad Gateway (https://api.github.com/graphql)' >&2; exit 1";

fn failing_file(fx: &Fx) -> PathBuf {
    fx.env.cpd_home().join("run/pr-poll-failing.json")
}

/// `run/pr-poll-failing.json` as the engine keeps it: the `call` that failed and `since` when it first did, `minutes` ago.
fn fail_since(fx: &Fx, call: &str, minutes: i64) {
    fs::create_dir_all(failing_file(fx).parent().unwrap()).unwrap();
    fs::write(failing_file(fx), json!({"call": call, "since": ago(minutes)}).to_string()).unwrap();
}

fn failing(fx: &Fx) -> Option<Value> {
    fs::read_to_string(failing_file(fx)).ok().map(|t| serde_json::from_str(&t).unwrap())
}

/// Sets `watch.pr_poll_transient_minutes`.
fn transient_minutes(fx: &Fx, minutes: u32) {
    let config = fx.env.cpd_home().join("config.toml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(&config, text.replace("[watch]\n", &format!("[watch]\npr_poll_transient_minutes = {minutes}\n"))).unwrap();
}

/// What the Manager was sent that is not E4's: `data.text` of every `session-message`.
fn messages(fx: &Fx) -> Vec<String> {
    fx.events("session-message").into_iter().map(|e| e.3["text"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn e5_a_gh_502_raises_no_error_and_no_message_logs_a_line_and_the_next_poll_heals() {
    let fx = fx("");
    fx.add_manager();
    let healed = fx.env.root().join("healed");
    fx.env.fake_gh(&format!("if [ -e '{}' ]; then echo '{EMPTY_SEARCH}'; else {GH_502}; fi", healed.display()));

    let engine = fx.start();
    wait_until("three failing polls", || fx.env.calls("gh").len() >= 3);
    wait_until("the engine's record of the failure", || failing(&fx).is_some());
    let record = failing(&fx).unwrap();
    assert_eq!(record["call"], "search");
    let since = parse_time(record["since"].as_str().unwrap());
    assert!((OffsetDateTime::now_utc() - since).whole_seconds() < 60, "the first failure was just now: {record}");
    assert_eq!((fx.count("error"), messages(&fx)), (0, Vec::<String>::new()));

    // The next poll heals it: the record is gone, and nothing was ever raised.
    fs::write(&healed, "").unwrap();
    wait_until("the record to be deleted by a poll that succeeded", || failing(&fx).is_none());
    fx.env.wait_whole_ticks(2);
    engine.stop();
    assert_eq!((fx.count("error"), messages(&fx)), (0, Vec::<String>::new()));
    let log = watch_log(&fx);
    assert!(log.contains("pr poll: gh search failed transiently (HTTP 502), failing since "), "{log}");
    assert!(log.contains("): retrying at the next poll"), "{log}");
}

#[test]
fn e5_a_gh_timeout_is_transient_too() {
    let fx = fx("");
    fx.add_manager();
    // `gh` hangs; the poll's 15 s kill is shortened (contract §1.2, `CPD_TEST_DEADLINE_GH`).
    fx.env.fake_gh(&format!("exec sleep {SLEEP_SECONDS}"));
    let mut vars = common::PLAN_POLLS.to_vec();
    vars.push(("CPD_TEST_DEADLINE_GH", "0.3"));

    let engine = fx.start_with(Path::new(CPD), &vars);
    wait_until("the first failure to be handled", || fx.count("error") >= 1 || watch_log(&fx).contains("failed transiently"));
    engine.stop();
    assert_eq!((fx.count("error"), messages(&fx)), (0, Vec::<String>::new()));
    assert!(watch_log(&fx).contains("pr poll: gh search failed transiently (timeout), failing since "), "{}", watch_log(&fx));
    assert_eq!(failing(&fx).unwrap()["call"], "search");
}

/// A `fx` with a Manager, a `gh` that always answers 502 and `run/pr-poll-failing.json` for `minutes` minutes of failing.
fn fx_with_manager_failing(minutes: i64) -> Fx {
    let fx = fx("");
    fx.add_manager();
    fx.env.fake_gh(GH_502);
    fail_since(&fx, "search", minutes);
    fx
}

#[test]
fn e5_counts_the_failing_window_from_pr_poll_transient_minutes() {
    for (minutes, raised) in [(4, 0), (6, 1)] {
        let fx = fx_with_manager_failing(minutes);
        transient_minutes(&fx, 5);
        let engine = fx.start();
        fx.env.wait_whole_ticks(3);
        engine.stop();
        assert_eq!(fx.count("error"), raised, "failing for {minutes} minutes, limit 5");
    }
}

/// `gh`'s own failure to parse a response that GitHub cut off mid-body (hs-d354, 2026-10-20 16:43:13Z): exit 1, this on stderr, an empty stdout.
const GH_CUT: &str = "echo 'unexpected end of JSON input' >&2; exit 1";

#[test]
fn e5_a_cut_gh_response_raises_no_error_and_no_message_logs_a_line_and_the_next_poll_heals() {
    let fx = fx("");
    fx.add_manager();
    let healed = fx.env.root().join("healed");
    fx.env.fake_gh(&format!("if [ -e '{}' ]; then echo '{EMPTY_SEARCH}'; else {GH_CUT}; fi", healed.display()));

    let engine = fx.start();
    wait_until("three failing polls", || fx.env.calls("gh").len() >= 3);
    wait_until("the engine's record of the failure", || failing(&fx).is_some());
    assert_eq!(failing(&fx).unwrap()["call"], "search");
    assert_eq!((fx.count("error"), messages(&fx)), (0, Vec::<String>::new()));

    // The next poll heals it: the record is gone, and nothing was ever raised.
    fs::write(&healed, "").unwrap();
    wait_until("the record to be deleted by a poll that succeeded", || failing(&fx).is_none());
    fx.env.wait_whole_ticks(2);
    engine.stop();
    assert_eq!((fx.count("error"), messages(&fx)), (0, Vec::<String>::new()));
    let log = watch_log(&fx);
    assert!(log.contains("pr poll: gh search failed transiently (unexpected end of JSON input), failing since "), "{log}");
    assert!(log.contains("): retrying at the next poll"), "{log}");
}

/// The shell command that prints the epoch in nanoseconds: BSD `date` has no `%N`.
const NOW_NS: &str = r#"perl -MTime::HiRes=gettimeofday -e '($s,$u)=gettimeofday; printf "%d%06d000\n",$s,$u'"#;

#[test]
fn e5_pauses_below_100_requests_until_reset_and_reports_one_gh_rate_limited() {
    let fx = fx("");
    let log = fx.env.root().join("poll.log");
    let first = fx.env.root().join("first-poll-done");
    // The first poll leaves 50 requests until `reset_at`, 0.5 s after it (written to `reset-at`); the next one, which
    // E5 must not run before that (it polls every tick otherwise), 4987.
    let reset_file = fx.env.root().join("reset-at");
    fx.env.fake_gh(&format!(
        r#"echo "start $({now})" >> '{log}'
if [ ! -e '{first}' ]; then
  touch '{first}'; ms=$(( $({now}) / 1000000 + 500 ))
  reset=$(perl -MPOSIX -e '$ms=shift; print strftime("%Y-%m-%dT%H:%M:%S",gmtime(int($ms/1000))).sprintf(".%03dZ",$ms%1000)' "$ms"); printf %s "$reset" > '{reset_file}'
  echo '{low}' | sed "s/RESET_AT/$reset/"
else echo '{ok}'; fi; exit 0"#,
        now = NOW_NS,
        log = log.display(),
        first = first.display(),
        reset_file = reset_file.display(),
        low = search_reply(50, "RESET_AT"),
        ok = search_reply(4987, "2099-01-01T00:00:00Z"),
    ));

    let engine = fx.start();
    wait_until("the second poll", || fx.env.calls("gh").len() >= 2);
    engine.stop();
    let reset_at = fs::read_to_string(&reset_file).unwrap();

    // The second poll started at or after `reset_at`: the ticks in between ran no poll.
    let starts: Vec<i128> = fs::read_to_string(&log).unwrap().lines().map(|l| l.strip_prefix("start ").unwrap().parse::<i128>().unwrap() / 1_000_000).collect();
    assert_eq!(starts.len(), 2, "{starts:?}");
    assert!(starts[1] >= epoch_ms(&reset_at), "polled {} ms before the reset", epoch_ms(&reset_at) - starts[1]);
    // One Event, for the machine, with what the User needs; resuming raised nothing more.
    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!((plan, job, attempt), (&None, &None, &None));
    assert_eq!((&data["source"], &data["code"]), (&json!("watch"), &json!("gh_rate_limited")));
    assert_eq!(data["details"], json!({"remaining": 50, "reset_at": reset_at}));
}

// ---------- E3's back-off, E5's executable and E6, in-process ----------

/// A Job whose latest 9 Attempts never had a driver, the last just ended: E3 waits 300 s, so no tick starts it.
#[test]
fn e3_does_not_start_a_job_in_its_backoff() {
    let fx = engine("");
    fx.engine_plan();
    let now = fx.stamp_ago(0);
    let attempts: String = (1..=9).map(|n| ended_attempt("api", n, "S2", None, &now)).collect();
    fx.add_job("api", &[], &attempts);

    // Three whole ticks (a tick is ten naps of a tenth of a second).
    fx.watch(30, |_| {}).unwrap();

    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts"), 9);
    assert_eq!(fx.env.query::<String>("SELECT status FROM jobs WHERE plan = 'pay' AND key = 'api'"), "queued");
    assert_eq!(fx.texts("error"), Vec::<String>::new());
}

/// E5 runs `cpd pr poll` from the path recorded at start, not from `PATH` (where the `cpd` route of `engine_routes` stands:
/// it must see no call).
#[test]
fn e5_polls_with_the_engines_recorded_executable_not_the_one_on_path() {
    let fx = engine("");
    let installed = fx.env.root().join("cpd-installed");
    fx.fakes.spawn.set_self_exe(&installed);
    let poll = json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}});
    fx.fake("cpd-installed", Reply::ok(&poll.to_string()));

    fx.watch_first_tick().unwrap();

    let polls = fx.calls("cpd-installed");
    assert_eq!(polls.iter().map(|call| (call.program.as_str(), call.args.as_slice())).collect::<Vec<_>>(), [(installed.to_str().unwrap(), ["pr".to_owned(), "poll".to_owned()].as_slice())]);
    assert_eq!(fx.calls("cpd"), Vec::<Call>::new(), "E5 ran the cpd on PATH");
    assert_eq!(fx.events("error").len(), 0);
}

// ---------- E6 (removed) ----------

#[test]
fn the_engine_neither_imports_nor_syncs_a_context_store() {
    let fx = engine("");
    // An old git store the database does not hold: only migration 0007 reads those, once.
    let store = fx.env.cpd_home().join("contexts/demo");
    fs::create_dir_all(store.join("records")).unwrap();
    fs::write(store.join("context.json"), r#"{"v":1,"id":"demo","title":"Demo","jira":null,"created_at":"2026-10-14T21:00:00.000Z","created_on":"m1"}"#).unwrap();
    fs::write(
        store.join("records/m1.jsonl"),
        "{\"v\":1,\"id\":\"m1-n1\",\"kind\":\"note\",\"at\":\"2026-10-14T21:00:00.000Z\",\"machine\":\"m1\",\"by\":{\"session\":null,\"plan\":null},\"text\":\"Remember this.\"}\n",
    )
    .unwrap();
    // Not a store at all: E6 used to log and skip it; nothing looks now.
    fs::create_dir_all(fx.env.cpd_home().join("contexts/not a context")).unwrap();

    // Two whole ticks (a tick is ten naps of a tenth of a second) and the one that ends the run.
    fx.watch(20, |_| {}).unwrap();

    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM contexts WHERE id = 'demo'"), 0);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM context_records"), 0);
    assert_eq!(fx.events("context-created").len() + fx.events("context-record").len(), 0);
    assert_eq!(fx.events("error").len(), 0);
    assert!(store.join("context.json").is_file(), "the folder is left alone");
    assert!(!engine_log(&fx).contains("E6"), "{}", engine_log(&fx));
}

// ---------- scripts/install-cpd.sh (Finding 5) ----------

/// Runs `scripts/install-cpd.sh` with a fake `cargo` that "builds" a fake
/// `cpd`, in `env`: the fake `cpd` answers `config` with the private tmux
/// socket, and records `watch --ensure` with the windows of the work session
/// at that moment. Returns the script's stderr and the fake's calls.
fn install(env: &TestEnv) -> (String, Vec<Vec<String>>) {
    let (out, calls) = install_with(env, OPENS);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    (String::from_utf8_lossy(&out.stderr).into_owned(), calls)
}

/// The `store` object of a fake build's `--version`: the store opens.
const OPENS: &str = r#"{"opens":true,"refusal":null,"unknown":[],"pending":[]}"#;

/// `install`, where the fake build's `--version` reports `store` (a JSON literal: the verdict
/// object or `null`), run from a checkout whose `origin/main` holds the same migrations as the
/// tree. Returns the output.
fn install_with(env: &TestEnv, store: &str) -> (std::process::Output, Vec<Vec<String>>) {
    let socket = env.tmux_socket();
    let built = env.fake(
        "cpd-built",
        &format!(
            r#"case "$1" in
--version) echo '{{"ok":true,"version":"0.0.0","store":{store}}}' ;;
config) echo '{{"ok":true,"config":{{"tmux":{{"socket":"{socket}","cockpit_session":"cpd","work_session":"cpd-work"}}}}}}' ;;
watch) [ "$2" = --check ] && {{ echo '{{"ok":true}}'; exit 0; }}
  if [ "$2" = --mark-restart ]; then echo '{{"ok":true,"marked":true,"engine_pid":1}}'; exit 0; fi
  tmux -L '{socket}' list-windows -t =cpd-work -F '#{{window_name}}' > '{windows}' 2>&1
  l="$CPD_HOME/locks/watch.lock"; mkdir -p "$CPD_HOME/locks"
  sh -c 'echo $$ > "$CPD_HOME/engine.pid"; exec flock "$1" sleep 120' sh "$l" </dev/null >/dev/null 2>&1 &
  until ! flock -n "$l" true; do :; done
  echo '{{"ok":true,"started":true,"pane":"%9"}}' ;;
esac"#,
            windows = env.root().join("windows-at-ensure").display()
        ),
    );
    let target = env.root().join("target");
    let cargo = env.fake("fake-cargo", &format!("mkdir -p '{t}/release' && cp '{}' '{t}/release/cpd'", built.display(), t = target.display()));
    let checkout = checkout::Checkout::new(env, &["0001-initial.sql"]);
    let out = env
        .command(checkout.script())
        .env("CARGO", cargo)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .unwrap();
    // The fake engine the script's `watch --ensure` started holds the lock for the script's check only.
    if let Ok(pid) = fs::read_to_string(env.cpd_home().join("engine.pid")) {
        let _ = std::process::Command::new("kill").arg(pid.trim()).status();
    }
    (out, env.calls("cpd-built"))
}

#[test]
fn install_leaves_a_server_without_an_engine_alone() {
    let env = TestEnv::new();
    let out = env.tmux(&["new-session", "-d", "-s", "cpd-work", "-n", "other", "sleep", "600"]);
    assert!(out.status.success());

    let (stderr, calls) = install(&env);

    assert!(!stderr.contains("restarting the running engine"), "{stderr}");
    assert_eq!(calls, [vec!["--version"], vec!["config"], vec!["watch", "--check"]]);
}

#[test]
fn install_refuses_a_build_that_cannot_open_the_store_before_replacing_anything() {
    let env = TestEnv::new();
    let out = env.tmux(&["new-session", "-d", "-s", "cpd-work", "-n", "watch", "sleep", "600"]);
    assert!(out.status.success());

    let refusing = r#"{"opens":false,"refusal":{"code":"store_too_new","message":"The database was migrated by a newer cpd."},"unknown":[],"pending":[]}"#;
    let (out, calls) = install_with(&env, refusing);

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("cpd install refused: this build cannot open "), "{stderr}");
    assert!(stderr.contains("cpd.db: The database was migrated by a newer cpd."), "{stderr}");
    assert!(stderr.contains("merge main into this branch, then install again"), "{stderr}");
    assert!(!env.home().join(".local/bin/cpd").exists());
    assert!(!env.home().join(".local/share/cpd/pi").exists());
    // Only --version ran: no `tui --respawn`, no `watch --ensure`.
    assert_eq!(calls, [vec!["--version"]]);
}

#[test]
fn install_accepts_a_build_that_opens_the_store_or_a_home_without_a_database() {
    for store in [OPENS, "null"] {
        let env = TestEnv::new();
        let (out, _) = install_with(&env, store);
        assert!(out.status.success(), "{store}: {}", String::from_utf8_lossy(&out.stderr));
        assert!(env.home().join(".local/bin/cpd").exists());
    }
}

// ---------- the adapter swap of scripts/install-cpd.sh (pi-ci-W9) ----------

/// The names under `dir`, recursively, relative to it, sorted.
fn tree(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            out.push(path.strip_prefix(base).unwrap().display().to_string());
            if path.is_dir() {
                walk(base, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// The entries directly in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    out.sort();
    out
}

#[test]
fn install_puts_the_adapter_at_share_cpd_pi_without_its_tests() {
    let env = TestEnv::new();
    install(&env);

    // `install` runs the script from the fake checkout (`Checkout::new`): compare with its `pi/`.
    let checkout = env.root().join("repos/install-clone");
    let share = env.home().join(".local/share/cpd");
    let adapter = share.join("pi");
    let expected: Vec<String> = tree(&checkout.join("pi")).into_iter().filter(|p| p != "test" && !p.starts_with("test/")).collect();
    assert!(expected.contains(&"extension.ts".to_string()) && expected.contains(&"lib/cpd.ts".to_string()), "{expected:?}");
    assert!(tree(&checkout.join("pi")).contains(&"test/cpd.test.ts".to_string()));
    // `pi` is a symlink to a `pi.<id>` beside it, which holds the checkout's `pi/` minus `test/`.
    assert!(fs::symlink_metadata(&adapter).unwrap().file_type().is_symlink(), "pi is not a symlink");
    let target = fs::read_link(&adapter).unwrap().to_string_lossy().into_owned();
    assert!(target.starts_with("pi.") && !target.contains('/'), "the target `{target}` is not a relative pi.<id>");
    assert_eq!(tree(&share.join(&target)), expected);
    assert_eq!(tree(&adapter), expected);
    // Nothing else is left beside it: no tmp directory, no scratch link, no legacy.
    assert_eq!(names(&share), ["pi".to_string(), target]);
}

/// Puts a `python3` first on the install's `PATH` that runs the real one with an audit hook recording each
/// rename(2) it makes (`os.rename` and `os.replace` both raise the `os.rename` audit event) as
/// `<pid>\t<src>\t<dst>` in `<root>/renames`. Returns that log.
fn record_renames(env: &TestEnv) -> PathBuf {
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("python3"))
        .find(|p| p.is_file())
        .expect("python3 on PATH (the install requires it)");
    let log = env.root().join("renames");
    let site = env.root().join("python-site");
    fs::create_dir_all(&site).unwrap();
    fs::write(
        site.join("sitecustomize.py"),
        format!(
            r#"import os, sys
def _record(event, args):
    if event == "os.rename":
        with open({log:?}, "a") as f:
            f.write(f"{{os.getpid()}}\t{{os.fsdecode(args[0])}}\t{{os.fsdecode(args[1])}}\n")
sys.addaudithook(_record)
"#,
            log = log.display().to_string()
        ),
    )
    .unwrap();
    env.fake("python3", &format!("PYTHONPATH='{}' exec '{}' \"$@\"", site.display(), real.display()));
    log
}

/// The renames `log` ([`record_renames`]) holds, in order, as `(pid, src, dst)` with the paths relative to
/// `share` and the scratch link `pi.link.<pid of the script>` as `pi.link.$$`.
fn renames(log: &Path, share: &Path) -> Vec<(u32, String, String)> {
    let name = |path: &str| {
        let name = Path::new(path).strip_prefix(share).unwrap_or_else(|_| panic!("{path} is not under {}", share.display())).display().to_string();
        match name.strip_prefix("pi.link.") {
            Some(pid) if pid.parse::<u32>().is_ok() => "pi.link.$$".to_string(),
            _ => name,
        }
    };
    fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let [pid, src, dst] = line.split('\t').collect::<Vec<_>>()[..] else { panic!("a rename record `{line}`") };
            (pid.parse().unwrap(), name(src), name(dst))
        })
        .collect()
}

#[test]
fn install_replaces_an_installed_adapter_as_a_whole_by_one_rename() {
    let env = TestEnv::new();
    let share = env.home().join(".local/share/cpd");
    let adapter = share.join("pi");
    // An installed adapter as an earlier install left it: `pi` -> `pi.<id>`. (One `Checkout` per `TestEnv`, so the
    // first version is laid out by hand; the install swaps the link over it.)
    let first = "pi.20200101T000000-1";
    fs::create_dir_all(share.join(first)).unwrap();
    fs::write(share.join(first).join("package.json"), "{}").unwrap();
    fs::write(share.join(first).join("stale.ts"), "").unwrap();
    std::os::unix::fs::symlink(first, &adapter).unwrap();
    let log = record_renames(&env);

    let (out, _) = install_with(&env, OPENS);

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // `pi` is never missing: the only rename that touches it is one rename(2) of the new link over it
    // (docs/implementing.md "The adapter swap"), which the kernel does atomically.
    let renames: Vec<(String, String)> = renames(&log, &share).into_iter().map(|(_, src, dst)| (src, dst)).collect();
    assert_eq!(renames, [("pi.link.$$".to_string(), "pi".to_string())]);
    assert_ne!(fs::read_link(&adapter).unwrap().to_string_lossy(), first, "pi points to the new version");
    assert!(!adapter.join("stale.ts").exists(), "the old adapter was not replaced as a whole");
    assert!(adapter.join("package.json").exists());
}

#[test]
fn install_over_a_real_adapter_directory_moves_it_aside_and_links_in_one_process() {
    let env = TestEnv::new();
    let share = env.home().join(".local/share/cpd");
    let adapter = share.join("pi");
    fs::create_dir_all(adapter.join("old")).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fs::write(adapter.join("stale.ts"), "").unwrap();
    fs::write(adapter.join("old/f.ts"), "").unwrap();
    let log = record_renames(&env);

    let (out, _) = install_with(&env, OPENS);

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // The migration is two renames of one process, back to back (docs/implementing.md "The adapter swap"): the
    // directory aside as a whole, then the link over it, so the one window `pi` is missing holds no process start-up.
    let renames = renames(&log, &share);
    let pids: Vec<u32> = renames.iter().map(|(pid, _, _)| *pid).collect();
    let moves: Vec<(&str, &str)> = renames.iter().map(|(_, src, dst)| (src.as_str(), dst.as_str())).collect();
    assert_eq!(moves, [("pi", "pi.legacy"), ("pi.link.$$", "pi")]);
    assert_eq!(pids[0], pids[1], "the two renames run in two processes: {renames:?}");
    assert!(fs::symlink_metadata(&adapter).unwrap().file_type().is_symlink(), "pi is a symlink afterwards");
    assert!(!adapter.join("stale.ts").exists() && !adapter.join("old").exists(), "the old adapter was not replaced as a whole");
    assert!(share.join("pi.legacy/stale.ts").exists() && share.join("pi.legacy/old/f.ts").exists(), "pi.legacy holds the old files");
    assert!(adapter.join("package.json").exists());
}

// ---------- a caller with no UTF-8 locale (hs-d463; contract §6.1 "Every tmux call is a UTF-8 client") ----------

// ---------- a planned restart is logged only (hs-d471, contract §3.7 "A planned restart") ----------

/// The pid of the engine the tests below say ran before the one under test: no process.
const PREVIOUS_ENGINE: i64 = 424_242;

/// The sentence a planned restart adds before the final full stop.
const PLANNED: &str = "(an install's planned restart)";

fn secs_ago(secs: f64) -> String {
    clock::timestamp(OffsetDateTime::now_utc() - time::Duration::seconds_f64(secs))
}

impl Fx {
    /// A home whose previous engine (pid [`PREVIOUS_ENGINE`]) started an hour ago and stopped `down` seconds ago, saying so
    /// (`watch-stopped`) when `said`, else leaving nothing (a crash: it was last alive at its start, no lock file).
    /// The new engine's own start comes a moment after `down`.
    fn previous_engine(&self, down: f64, said: bool) {
        let stopped = if said {
            format!(
                "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '{}', 'watch-stopped', 'The engine stopped.', '{{\"pid\":{PREVIOUS_ENGINE}}}');",
                secs_ago(down)
            )
        } else {
            String::new()
        };
        // Without a `watch-stopped` the engine is dated by its `watch-started`.
        let started_ago = if said { 3600.0 } else { down };
        self.fixture(&format!(
            "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '{}', 'watch-started', 'The engine started.', '{{\"pid\":{PREVIOUS_ENGINE}}}');{stopped}",
            secs_ago(started_ago)
        ));
    }

    fn marker_path(&self) -> PathBuf {
        self.env.cpd_home().join("run/install-restart.json")
    }

    /// Starts the engine, lets it finish its start and stops it: what the Managers were told is then settled.
    fn start_and_stop(&self) {
        let _ = self.start().stop();
    }

    /// The `watch-started` text of the engine under test.
    fn started_text(&self) -> String {
        self.texts("watch-started").pop().unwrap()
    }
}

#[test]
fn a_planned_restart_back_within_ten_seconds_is_logged_and_tells_no_manager() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(500, true);
    fx.marker(PREVIOUS_ENGINE, 1_000);
    fx.start_and_stop();
    assert_eq!(fx.messages(), [], "a quiet restart sends no session-message");
    let text = fx.started_text();
    assert!(text.contains("The engine was down for ") && text.contains(&format!("{PLANNED}.")), "{text}");
    assert!(engine_log(&fx).contains(&format!("{PLANNED}.")), "{}", engine_log(&fx));
    assert!(!fx.marker_path().exists(), "the marker is consumed at the start");
}

#[test]
fn a_stop_with_no_marker_tells_each_manager_as_before() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(500, true);
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(!fx.started_text().contains(PLANNED), "{}", fx.started_text());
}

#[test]
fn a_planned_restart_that_stayed_down_past_ten_seconds_tells_each_manager_it_took_longer() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(11_000, true);
    fx.marker(PREVIOUS_ENGINE, 12_000);
    fx.start_and_stop();
    fx.told_with("it was an install's restart that took longer than 10 s");
    assert!(!fx.marker_path().exists(), "the marker is consumed at the start");
}

/// The bound is `downtime <= 10 s` quiet, `> 10 s` tells. On the `ManualClock` the new engine starts exactly when the
/// fixture says: the quiet side is exactly 10.000 s, the other a millisecond over.
#[test]
fn the_bound_is_ten_seconds_quiet_up_to_it_and_told_past_it() {
    {
        let quiet = engine("");
        quiet.manager("mgr1");
        quiet.previous_engine(10_000, true);
        quiet.marker(PREVIOUS_ENGINE, 10_000);
        quiet.start_and_stop();
        assert_eq!(quiet.messages(), [], "up to 10 s is quiet");
        let text = quiet.started_text();
        assert!(text.contains("The engine was down for 10.0 s") && text.contains(&format!("{PLANNED}.")), "{text}");
    }
    // One `Fx` at a time in a process (it holds the process's environment): the first is dropped.
    let told = engine("");
    told.manager("mgr1");
    told.previous_engine(10_001, true);
    told.marker(PREVIOUS_ENGINE, 10_001);
    told.start_and_stop();
    told.told_with("took longer than 10 s");
}

#[test]
fn a_marker_for_another_engine_does_not_silence_the_stop_and_is_deleted() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(500, true);
    fx.marker(PREVIOUS_ENGINE + 1, 1_000);
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(!fx.started_text().contains(PLANNED), "{}", fx.started_text());
    assert!(!fx.marker_path().exists(), "a foreign marker is deleted too");
}

#[test]
fn a_marker_older_than_120_seconds_does_not_silence_the_stop_and_is_deleted() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(500, true);
    fx.marker(PREVIOUS_ENGINE, 130_000);
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(!fx.started_text().contains(PLANNED), "{}", fx.started_text());
    assert!(!fx.marker_path().exists(), "a stale marker is deleted too");
}

#[test]
fn an_unparseable_marker_does_not_silence_the_stop_is_logged_once_deleted_and_the_engine_runs() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(500, true);
    fx.raw_marker("{not json");
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(!fx.marker_path().exists(), "the unreadable marker is deleted");
    let lines = engine_log(&fx).lines().filter(|l| l.contains("install-restart.json")).count();
    assert_eq!(lines, 1, "{}", engine_log(&fx));
}

#[test]
fn a_marker_with_a_field_of_another_type_is_no_marker() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(500, true);
    fx.raw_marker(&json!({"by": "install-cpd.sh", "at": fx.stamp_ago(1_000), "engine_pid": "424242"}).to_string());
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(!fx.marker_path().exists());
}

#[test]
fn a_crashed_engine_with_a_valid_marker_is_quiet_by_the_time_it_was_last_alive() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(500, false);
    fx.marker(PREVIOUS_ENGINE, 1_000);
    fx.start_and_stop();
    assert_eq!(fx.messages(), []);
    let text = fx.started_text();
    assert!(text.contains("it was last alive") && text.contains(&format!("{PLANNED}.")), "{text}");
}

#[test]
fn a_crashed_engine_with_a_valid_marker_past_ten_seconds_tells() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(11_000, false);
    fx.marker(PREVIOUS_ENGINE, 12_000);
    fx.start_and_stop();
    fx.told_with("took longer than 10 s");
}

#[test]
fn mark_restart_writes_the_marker_of_the_latest_engine_and_twice_leaves_one_file() {
    let fx = fx("");
    fx.previous_engine(0.5, true);
    fx.fixture(&format!(
        "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '{}', 'watch-started', 'The engine started.', '{{\"pid\":{}}}');",
        secs_ago(0.1),
        PREVIOUS_ENGINE + 7
    ));
    let before = OffsetDateTime::now_utc() - time::Duration::seconds(1);
    let out = fx.env.cpd(&["watch", "--mark-restart"]).ok();
    assert_eq!(out, json!({"ok": true, "marked": true, "engine_pid": PREVIOUS_ENGINE + 7}));
    let marker: Value = serde_json::from_str(&fs::read_to_string(fx.marker_path()).unwrap()).unwrap();
    assert_eq!((marker["by"].clone(), marker["engine_pid"].clone()), (json!("install-cpd.sh"), json!(PREVIOUS_ENGINE + 7)), "{marker}");
    let at = parse_time(marker["at"].as_str().unwrap());
    assert!(at >= before && at <= OffsetDateTime::now_utc() + time::Duration::seconds(1), "{marker}");

    fx.env.cpd(&["watch", "--mark-restart"]).ok();
    let files: Vec<_> = fs::read_dir(fx.marker_path().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("install-restart"))
        .collect();
    assert_eq!(files, ["install-restart.json"], "one marker, no temporary file left");
}

#[test]
fn mark_restart_with_no_watch_started_writes_nothing_and_fails_watch_not_started() {
    let fx = fx("");
    let run = fx.env.cpd(&["watch", "--mark-restart"]);
    assert_eq!(run.exit, 3, "{run:?}");
    assert_eq!(run.error()["code"], "watch_not_started");
    assert!(!fx.marker_path().exists());
}

#[test]
fn a_marker_written_by_mark_restart_silences_the_restart_it_marks() {
    let fx = fx("");
    fx.manager("mgr1");
    let first = fx.start();
    let _ = first.stop();
    fx.env.cpd(&["watch", "--mark-restart"]).ok();
    fx.start_and_stop();
    assert_eq!(fx.messages(), [], "the engine that stopped was marked, and the next one is back within a moment");
    assert!(fx.started_text().contains(PLANNED), "{}", fx.started_text());
}

// ---------- `watch --check` is read-only (hs-d463; contract §3.7 step 1, commands.md §12) ----------

/// A store one migration behind this binary, and what the check must leave alone. The store is the template's, whose
/// newest migration's row is removed (the trigger that makes `schema_migrations` append-only is dropped and recreated
/// as it was): the verdict (§4.5) is then `opens` with that one migration `pending`. The migration's effects are
/// already in the schema, so a check that migrated would fail on it or double-record it: either way the checks below fail.
struct BehindStore {
    env: TestEnv,
    /// The newest migration of this binary: the one the store lacks.
    pending: String,
    /// Held open, so that the WAL and its shared memory exist for the whole check: a read-only open must not create them.
    _held: rusqlite::Connection,
}

/// What `watch --check` must leave byte-for-byte unchanged: the database, its WAL, `locks/store.lock`, with their
/// modification times, and the set of files under `$CPD_HOME`. (The shared-memory file is the readers' own scratch,
/// written by any reader, so it is listed but not compared.)
#[derive(Debug, PartialEq)]
struct Files {
    contents: Vec<(String, Option<Vec<u8>>, Option<std::time::SystemTime>)>,
    names: Vec<String>,
}

fn files(env: &TestEnv) -> Files {
    fn walk(dir: &Path, root: &Path, names: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            names.push(path.strip_prefix(root).unwrap().display().to_string());
            if path.is_dir() {
                walk(&path, root, names);
            }
        }
    }
    let home = env.cpd_home();
    let mut names = Vec::new();
    walk(&home, &home, &mut names);
    names.sort();
    let contents = ["cpd.db", "cpd.db-wal", "locks/store.lock"]
        .iter()
        .map(|name| {
            let path = home.join(name);
            (name.to_string(), fs::read(&path).ok(), fs::metadata(&path).and_then(|m| m.modified()).ok())
        })
        .collect();
    Files { contents, names }
}

/// Every table's row count: the check writes no row and no Event.
fn row_counts(env: &TestEnv) -> Vec<(String, i64)> {
    env.read(|conn| {
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        tables
            .into_iter()
            .map(|t| Ok((t.clone(), conn.query_row(&format!("SELECT count(*) FROM \"{t}\""), [], |r| r.get(0))?)))
            .collect()
    })
}

fn applied(env: &TestEnv) -> Vec<String> {
    env.read(|conn| {
        Ok(conn.prepare("SELECT name FROM schema_migrations ORDER BY seq")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    })
}

fn user_version(env: &TestEnv) -> i64 {
    env.query("PRAGMA user_version")
}

fn behind_store() -> BehindStore {
    let env = TestEnv::new();
    env.write_tmux_config("");
    let conn = rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap();
    conn.pragma_update(None, "journal_mode", "wal").unwrap();
    let (pending, trigger): (String, String) = (
        conn.query_row("SELECT name FROM schema_migrations ORDER BY seq DESC LIMIT 1", [], |r| r.get(0)).unwrap(),
        conn.query_row("SELECT sql FROM sqlite_schema WHERE name = 'schema_migrations_no_delete'", [], |r| r.get(0)).unwrap(),
    );
    conn.execute_batch("DROP TRIGGER schema_migrations_no_delete").unwrap();
    conn.execute("DELETE FROM schema_migrations WHERE name = ?1", [&pending]).unwrap();
    conn.execute_batch(&trigger).unwrap();
    // `locks/store.lock` exists, as it does on a machine that has run cpd.
    fs::create_dir_all(env.cpd_home().join("locks")).unwrap();
    OpenOptions::new().create(true).append(true).open(env.cpd_home().join("locks/store.lock")).unwrap();
    assert!(env.cpd_home().join("cpd.db-wal").exists(), "the held connection keeps the WAL");
    BehindStore { env, pending, _held: conn }
}

impl BehindStore {
    /// The stand-in for the old engine (the binary with the earlier list of migrations; none is built here, and a store
    /// whose last row was removed cannot be opened by this binary's `Store::open`, which would migrate it, the thing
    /// that must not happen): what makes the old engine fail `store_too_new` (§4.5) is an applied migration it does
    /// not know. So the old engine still works against the store when no migration the old list lacks is applied, the
    /// store is not marked newer (`user_version` is the one every named store has), and it reads as it did.
    fn assert_the_old_engine_still_works(&self, before_applied: &[String], before_version: i64) {
        let env = &self.env;
        let now = applied(env);
        assert_eq!(now, before_applied, "no migration was applied");
        assert!(!now.contains(&self.pending), "the new binary's migration {} was applied by the check", self.pending);
        assert_eq!(user_version(env), before_version, "user_version is unchanged");
        // The old engine's own read and its own write, from a connection that is not this binary's `Store`.
        let conn = rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        let check: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
        assert_eq!(check, "ok");
        conn.execute_batch("BEGIN IMMEDIATE; COMMIT").expect("the old engine can still take the write lock");
    }
}

/// The tmux the check fails at: a UTF-8-less client's listing, tabs printed as `_` (the mac-pro failure of hs-d463).
fn tmux_printing_underscores(env: &TestEnv) {
    env.fake(
        "tmux",
        r#"case " $*" in
*" -V"*) echo "tmux 3.6";;
*" list-sessions"*) echo '$0';;
*" list-panes"*) echo '%1_cpd_main_0_1234_x';;
esac"#,
    );
}

/// (1) A check that passes against a store one migration behind writes nothing: the database, its WAL and `locks/store.lock`
/// are byte-for-byte and mtime-for-mtime what they were, no file is made, the schema version is unchanged.
#[test]
fn watch_check_against_a_store_one_migration_behind_passes_and_changes_no_file_and_no_version() {
    let s = behind_store();
    let before = (files(&s.env), applied(&s.env), user_version(&s.env));
    let run = s.env.cpd(&["watch", "--check"]);
    let out = run.ok();
    assert_eq!(out["checked"], json!(["config", "store", "tmux", "listing", "home"]), "{out}");
    assert_eq!((files(&s.env), applied(&s.env), user_version(&s.env)), before, "the check passed and wrote nothing");
    s.assert_the_old_engine_still_works(&before.1, before.2);
}

/// (2) The Manager's test: the check fails after the store step (the tmux listing, a tmux that prints `_` for tabs) against the
/// store one migration behind: the store is not migrated, and the old engine still works against it.
#[test]
fn a_watch_check_that_fails_after_the_store_step_leaves_the_store_unmigrated_for_the_old_engine() {
    let s = behind_store();
    tmux_printing_underscores(&s.env);
    let before = (files(&s.env), applied(&s.env), user_version(&s.env));
    let run = s.env.cpd(&["watch", "--check"]);
    let error = run.error();
    assert_eq!(error["code"], "tmux_failed", "the check fails at the listing, after the store step: {error}");
    assert!(error["message"].as_str().unwrap().contains("unexpected output"), "{error}");
    assert!(
        s.env.calls("tmux").iter().any(|c| c.iter().any(|a| a == "list-panes")),
        "the check got as far as the tmux listing: {:?}",
        s.env.calls("tmux")
    );
    assert_eq!((files(&s.env), applied(&s.env), user_version(&s.env)), before, "the failed check left the store as it was");
    s.assert_the_old_engine_still_works(&before.1, before.2);
}

/// (3) The check writes no row and no Event, whether it passes or fails.
#[test]
fn watch_check_writes_no_row_and_no_event_passing_or_failing() {
    let s = behind_store();
    let before = row_counts(&s.env);
    assert!(before.iter().any(|(t, _)| t == "events") && before.iter().any(|(t, _)| t == "txns"));
    s.env.cpd(&["watch", "--check"]).ok();
    assert_eq!(row_counts(&s.env), before, "a passing check wrote a row");
    tmux_printing_underscores(&s.env);
    assert_eq!(s.env.cpd(&["watch", "--check"]).error()["code"], "tmux_failed");
    assert_eq!(row_counts(&s.env), before, "a failing check wrote a row");
    assert_eq!(s.env.query::<i64>("SELECT count(*) FROM events"), before.iter().find(|(t, _)| t == "events").unwrap().1);
}

// ---- E9's retry with a smaller batch (contract §3.7 E9; FX4; hs-d549, hs-d551) ----

/// `count` old notes `m1-n<first>`.. in force of Context `ctx`, the newest hours old: the quiet period is over.
fn old_notes(fx: &Fx, first: i64, count: i64) {
    use cpd_core::context::{By, Draft, Record};
    use cpd_core::ids::{ContextId, RecordId};

    let context = ContextId::parse("ctx").unwrap();
    for n in first..first + count {
        let record = Record {
            id: RecordId::parse(&format!("m1-n{n}")).unwrap(),
            at: ago(600 - n),
            by: By { session: None, plan: None },
            text: format!("Note number {n}."),
            draft: Draft::Note,
        };
        fx.store().write("fixture", |tx| Ok(record.insert(tx, &context).unwrap())).unwrap();
    }
}

/// A fake pi that logs the number of records of the batch it is given, and curates it when that is at most `fits`
/// (the model "cannot handle" a bigger batch: it answers no JSON line).
fn pi_handling_at_most(fx: &Fx, fits: u32) {
    fx.env.fake_pi(&common::pi_says(&format!(
        "n=$(grep -c '^\\[' batch.md)\necho $n >> \"$HOME/batches\"\nif [ $n -le {fits} ]; then\n{}\nelse\necho 'I cannot do this.'\nfi",
        common::CURATES_ALL
    )));
}

/// The batch sizes pi saw, in order, a run of equal ones (the ladder's steps of one run) counted once.
fn batch_sizes(fx: &Fx) -> Vec<u32> {
    let text = fs::read_to_string(fx.env.home().join("batches")).unwrap_or_default();
    let mut sizes: Vec<u32> = text.lines().map(|l| l.trim().parse().unwrap()).collect();
    sizes.dedup();
    sizes
}

#[test]
fn e9_retries_a_failed_run_with_the_next_smaller_batch() {
    let fx = fx_bare("");
    old_notes(&fx, 1, 60);
    pi_handling_at_most(&fx, 15);
    let engine = fx.start();
    common::wait_until_for("a batch of 15 curated", Duration::from_secs(30), || fx.env.query::<i64>("SELECT count(*) FROM record_curations") >= 15);
    engine.stop();
    assert_eq!(batch_sizes(&fx)[..3], [60, 30, 15], "a failed state is retried with 30, then 15 (the model gets a smaller batch)");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM record_curations"), 15);
}

// ---- E9 and the curation floor (hs-d592 R2, R5) ----

