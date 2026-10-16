//! The engine, `cpd watch` (contract §3.7, §6.6; commands.md §12): the lock
//! and its refusal, the signals, `--ensure`, and each step E1–E6 (E7, the PR start, is in pr_start.rs) on a
//! prepared database, with a private tmux server and fake `pi` and `cpd`.

mod common;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{CPD, Run, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::store::{Store, WriteTxn};
use nix::fcntl::{Flock, FlockArg};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
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

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// Waits until `done` holds, at most 30 s.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
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

fn fx(config_extra: &str) -> Fx {
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
             [watch]\ntick_seconds = 1\n[tmux]\nsocket = \"{}\"\n",
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
    let fx = Fx { env, repo, lead };
    let catalog: String = EFFORTS
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    fx.fixture(&format!(
        "{catalog}
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'main', 'dispatch', 'cpd/pay', '{lead}', cpd_txn(), 't', 't');",
        repo = fx.repo.display(),
        lead = fx.lead.display()
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

    fn query<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.store().read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }

    /// Job `key` of Plan `pay`, `queued` with its Locked-Job fingerprint, then
    /// moved to `status` by `then` (SQL run after the fingerprint is set).
    fn add_job(&self, key: &str, needs: &[&str], then: &str) {
        let mut sql = format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
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

    /// Job `key` `running` with Attempt `<key>-a1` in `status`, started at `started_at`.
    fn add_running(&self, key: &str, status: &str, started_at: &str) {
        let worktree = self.repo.parent().unwrap().join(format!("pay.pay--{key}-a1"));
        self.add_job(
            key,
            &[],
            &format!(
                "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = '{key}';
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                   VALUES ('pay', '{key}-a1', '{key}', 1, 'S2', 'ladder', '{status}', 'cpd/pay--{key}-a1', '{}', '{started_at}');",
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
        let child = self.env.command(CPD).arg("watch").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let engine = Engine { child: Some(child) };
        let pid = engine.pid();
        wait_until("watch-started", || {
            self.query::<i64>(&format!(
                "SELECT count(*) FROM events WHERE kind = 'watch-started' AND json_extract(data, '$.pid') = {pid}"
            )) == 1
        });
        engine
    }

    /// Every Event of `kind`, in order: `(plan, job, attempt, data)`.
    fn events(&self, kind: &str) -> Vec<(Option<String>, Option<String>, Option<String>, Value)> {
        self.store()
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT plan, job, attempt, data FROM events WHERE kind = ?1 ORDER BY id")?;
                let rows = stmt
                    .query_map([kind], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows.into_iter().map(|(p, j, a, d)| (p, j, a, serde_json::from_str(&d).unwrap())).collect())
            })
            .unwrap()
    }

    fn count(&self, kind: &str) -> usize {
        self.events(kind).len()
    }

    fn texts(&self, kind: &str) -> Vec<String> {
        self.store()
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT text FROM events WHERE kind = ?1 ORDER BY id")?;
                Ok(stmt.query_map([kind], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    fn attempt(&self, id: &str) -> (String, Option<String>, Option<String>, Option<String>) {
        self.store()
            .read(|conn| {
                Ok(conn.query_row(
                    "SELECT status, outcome, cause, stop_request FROM attempts WHERE plan = 'pay' AND id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?)
            })
            .unwrap()
    }

    fn job_status(&self, key: &str) -> String {
        self.query(&format!("SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"))
    }

    /// A fake `cpd` on `PATH` that runs `pr` with `pr` (sh code) and
    /// everything else with the built binary.
    fn fake_cpd(&self, pr: &str) {
        self.env.fake("cpd", &format!("if [ \"$1\" = pr ]; then {pr}; fi\nexec '{CPD}' \"$@\""));
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

#[test]
fn ensure_starts_the_engine_in_the_work_session_once() {
    let fx = fx("");
    fx.env.fake("cpd", &format!("exec '{CPD}' \"$@\""));

    let out = fx.env.cpd(&["watch", "--ensure"]).ok();
    assert_eq!(out["started"], true, "{out}");
    let pane = out["pane"].as_str().unwrap().to_owned();
    assert!(pane.starts_with('%'), "{out}");
    let panes = fx.env.tmux(&["list-panes", "-a", "-F", "#{pane_id} #{session_name}:#{window_name} #{pane_dead} #{pane_pid}"]);
    let panes = String::from_utf8_lossy(&panes.stdout).into_owned();
    let line = panes.lines().find(|l| l.starts_with(&format!("{pane} "))).unwrap_or_else(|| panic!("{panes}"));
    let fields: Vec<&str> = line.split(' ').collect();
    assert_eq!(&fields[1..3], ["cpd-work:watch", "0"], "{panes}");
    // Its process took the lock and said so.
    let pid: u32 = fields[3].parse().unwrap();
    assert_eq!(fx.events("watch-started"), [(None, None, None, json!({"pid": pid}))]);
    assert_eq!(fx.env.cpd(&["watch"]).error()["code"], "watch_running");
    assert_eq!(fx.env.calls("cpd"), [vec!["watch".to_owned()]]);

    let again = fx.env.cpd(&["watch", "--ensure"]).ok();
    assert_eq!(again, json!({"ok": true, "started": false, "already": true}));
    assert_eq!(fx.count("watch-started"), 1);

    // A dead engine's window is replaced, not duplicated.
    kill(Pid::from_raw(pid as i32), Signal::SIGTERM).unwrap();
    wait_until("watch-stopped", || fx.count("watch-stopped") == 1);
    wait_until("the dead pane", || {
        let out = fx.env.tmux(&["list-panes", "-t", &pane, "-F", "#{pane_dead}"]);
        String::from_utf8_lossy(&out.stdout).trim() == "1"
    });
    let third = fx.env.cpd(&["watch", "--ensure"]).ok();
    assert_eq!(third["started"], true);
    let windows = fx.env.tmux(&["list-windows", "-t", "=cpd-work", "-F", "#{window_name}"]);
    assert_eq!(String::from_utf8_lossy(&windows.stdout), "watch\n");
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
    let status = |id: &str| fx.query::<String>(&format!("SELECT status FROM escalations WHERE id = '{id}'"));
    assert_eq!((status("api-a1-q1"), status("api-a1-q2")), ("expired".into(), "open".into()));
    assert!(fx.query::<Option<String>>("SELECT expired_at FROM escalations WHERE id = 'api-a1-q1'").is_some());
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
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM attempts WHERE job = 'api'"), 1);
}

#[test]
fn e2_asks_live_drivers_to_stop_on_timeout_or_cancel_with_escalations_pausing_the_clock() {
    let fx = fx("");
    let _locks: Vec<_> = ["late-a1", "gone-a1", "asked-a1", "fresh-a1"].iter().map(|a| fx.live_driver(a)).collect();
    fx.add_running("late", "running", &ago(31));
    fx.add_running("gone", "running", &ago(1));
    fx.request_cancel("gone");
    // 40 minutes, 20 of them waiting for answers (two overlapping Escalations count once).
    fx.add_running("asked", "running", &ago(40));
    fx.fixture(&format!(
        "INSERT INTO escalations (plan, id, attempt, number, question, status, answer, asker_pid, asked_at, answered_at)
           VALUES ('pay', 'asked-a1-q1', 'asked-a1', 1, 'Q?', 'answered', 'A.', 1, '{}', '{}'),
                  ('pay', 'asked-a1-q2', 'asked-a1', 2, 'Q?', 'answered', 'A.', 1, '{}', '{}');",
        ago(30),
        ago(15),
        ago(25),
        ago(10)
    ));
    fx.add_running("fresh", "running", &ago(1));

    let engine = fx.start();
    wait_until("stop requests", || fx.attempt("late-a1").3.is_some() && fx.attempt("gone-a1").3.is_some());
    // One more tick, for the Attempts it must leave alone.
    std::thread::sleep(Duration::from_millis(1500));
    engine.stop();

    assert_eq!(fx.attempt("late-a1"), ("running".into(), None, None, Some("timed_out".into())));
    assert_eq!(fx.attempt("gone-a1"), ("running".into(), None, None, Some("cancelled".into())));
    assert_eq!(fx.attempt("asked-a1"), ("running".into(), None, None, None));
    assert_eq!(fx.attempt("fresh-a1"), ("running".into(), None, None, None));
    // A data write for the driver: no Event of its own.
    assert_eq!(fx.count("job-cancelled") + fx.count("job-failed") + fx.count("error"), 0, "{:?}", fx.texts("error"));
}

#[test]
fn e2_recovers_a_merge_whose_driver_died_after_the_compare_and_swap() {
    let fx = fx("");
    let attempt = AttemptId::parse("api-a1").unwrap();
    let setup = Setup { timeout: Duration::from_secs(30), log: fx.env.root().join("attempt.log") };
    let made = create_attempt_worktree(&fx.repo, &fx.lead, &PlanId::parse(PLAN).unwrap(), &attempt, &setup).unwrap();
    fs::write(made.path.join("api.txt"), "api\n").unwrap();
    fx.env.git(&made.path, &["add", "-A"]);
    fx.env.git(&made.path, &["commit", "-q", "-m", "api"]);
    let tip = fx.env.git(&made.path, &["rev-parse", "HEAD"]);
    // The driver moved the Lead Branch (step 8) and synced the Lead worktree, then died.
    fx.env.git(&fx.lead, &["merge", "-q", "--ff-only", &made.branch]);
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

    let engine = fx.start();
    wait_until("the merge recorded", || fx.attempt("api-a1").0 == "ended");
    engine.stop();

    assert_eq!(fx.attempt("api-a1"), ("ended".into(), Some("merged".into()), Some("merged".into()), None));
    assert_eq!(fx.job_status("api"), "merged");
    assert_eq!(fx.events("job-merged")[0].3, json!({"sha": tip, "tier": "S2"}));
    assert!(!made.path.exists());
}

/// A Session whose pane is dead or gone becomes `stopped` with no pane, and
/// its place `none` (Escalation e2e-a1-q1); a `stopped` Session keeps a live pane.
#[test]
fn e2_stops_sessions_whose_pane_is_dead_or_gone() {
    let fx = fx("");
    let new = |args: &[&str]| {
        let out = fx.env.tmux(args);
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let live = new(&["new-session", "-d", "-s", "x", "-P", "-F", "#{pane_id}", "sleep", "600"]);
    new(&["set", "-g", "remain-on-exit", "on"]);
    let waiting = new(&["new-window", "-d", "-t", "=x:", "-P", "-F", "#{pane_id}", "sleep", "600"]);
    let dead = new(&["new-window", "-d", "-t", "=x:", "-P", "-F", "#{pane_id}", "sh", "-c", "exit 3"]);
    wait_until("the dead pane", || new(&["list-panes", "-t", &dead, "-F", "#{pane_dead}"]) == "1");
    fx.fixture(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
           VALUES ('s-live', 'lead', 'ctx', 'pay', 'Live', '/', 'O3', '{live}', 'idle', NULL, 't', 't'),
                  ('s-dead', 'lead', 'ctx', 'pay', 'Dead', '/', 'O3', '{dead}', 'blocked', 'cpd: dialog', 't', 't'),
                  ('s-gone', 'lead', 'ctx', NULL, 'Gone', '/', 'O3', '%999', 'working', NULL, 't', 't'),
                  ('s-waiting', 'lead', 'ctx', 'pay', 'Waiting', '/', 'O3', '{waiting}', 'stopped', NULL, 't', 't'),
                  ('s-shut', 'lead', 'ctx', 'pay', 'Shut', '/', 'O3', '%998', 'stopped', NULL, 't', 't');"
    ));

    let engine = fx.start();
    wait_until("three without a pane", || fx.count("session-moved") == 3);
    std::thread::sleep(Duration::from_millis(1500));
    engine.stop();

    let row = |id: &str| {
        fx.store()
            .read(|c| Ok(c.query_row("SELECT state, tmux_pane, state_at FROM sessions WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
            .unwrap()
    };
    let rows: Vec<(String, Option<String>, String)> = ["s-live", "s-dead", "s-gone", "s-waiting", "s-shut"].iter().map(|id| row(id)).collect();
    let (states, panes): (Vec<&str>, Vec<Option<&str>>) = rows.iter().map(|r| (r.0.as_str(), r.1.as_deref())).unzip();
    assert_eq!(states, ["idle", "stopped", "stopped", "stopped", "stopped"]);
    assert_eq!(panes, [Some(live.as_str()), None, None, Some(waiting.as_str()), None]);
    // Already stopped: its state did not change, so neither did state_at.
    assert_eq!(rows[4].2, "t");
    assert_eq!(fx.query::<Option<String>>("SELECT state_label FROM sessions WHERE id = 's-dead'"), None);
    let events = fx.events("session-state");
    assert_eq!(events.len(), 2);
    assert!(events.contains(&(Some("pay".into()), None, None, json!({"from": "blocked", "to": "stopped", "label": null}))), "{events:?}");
    assert!(events.contains(&(None, None, None, json!({"from": "working", "to": "stopped", "label": null}))), "{events:?}");
    let moved = fx.events("session-moved");
    let none = json!({"pane": null, "window": null, "place": "none"});
    assert_eq!(moved, [(Some("pay".into()), None, None, none.clone()), (None, None, None, none.clone()), (Some("pay".into()), None, None, none)]);
    let places: Vec<String> = fx.env.cpd(&["session", "list"]).ok()["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| ["s-dead", "s-gone", "s-shut"].contains(&s["id"].as_str().unwrap()))
        .map(|s| s["place"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(places, ["none", "none", "none"]);
}

/// Finding 4: a pane with a `@cpd_view` this binary does not know (made by a
/// newer cpd) is skipped, so E2 still runs and appends no `error`.
#[test]
fn e2_skips_a_pane_with_an_unknown_view_and_reports_no_error() {
    let fx = fx("");
    let new = |args: &[&str]| {
        let out = fx.env.tmux(args);
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let pane = new(&["new-session", "-d", "-s", "x", "-P", "-F", "#{pane_id}", "sleep", "600"]);
    new(&["set", "-p", "-t", &pane, "@cpd_view", "hologram"]);
    fx.fixture(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, tmux_pane, state, state_at, created_at)
           VALUES ('s-gone', 'lead', 'ctx', NULL, 'Gone', '/', 'O3', '%999', 'working', 't', 't');",
    );

    let engine = fx.start();
    wait_until("the missing pane's Session stopped", || fx.count("session-moved") == 1);
    std::thread::sleep(Duration::from_millis(2500));
    let out = engine.stop();

    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
    assert_eq!(fx.query::<String>("SELECT state FROM sessions WHERE id = 's-gone'"), "stopped");
    assert_eq!(out.stderr.matches("hologram").count(), 1, "logged once: {out:?}");
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
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM attempts"), 1);
    assert_eq!((fx.job_status("web"), fx.job_status("ui")), ("queued".into(), "queued".into()));
    assert!(fx.query::<Option<i64>>("SELECT driver_pid FROM attempts WHERE id = 'api-a1'").is_some());
    // Its driver runs detached; the Worker exits at once, so it ends crashed.
    let deadline = Instant::now() + Duration::from_secs(60);
    while fx.attempt("api-a1").0 != "ended" {
        assert!(Instant::now() < deadline, "the driver never ended api-a1");
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn e3_reports_a_tampered_job_once_and_does_not_start_it() {
    let fx = fx("");
    fx.add_job("api", &[], &format!("UPDATE jobs SET fingerprint = '{}' WHERE key = 'api';", "0".repeat(64)));

    let engine = fx.start();
    wait_until("error", || fx.count("error") == 1);
    std::thread::sleep(Duration::from_millis(2500));
    engine.stop();

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!((plan.as_deref(), job.as_deref(), attempt.as_deref()), (Some("pay"), Some("api"), None));
    assert_eq!((&data["source"], &data["code"]), (&json!("watch"), &json!("locked_job_tampered")));
    assert!(fx.texts("error")[0].ends_with("See: cpd job show api --plan pay"), "{:?}", fx.texts("error"));
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM attempts"), 0);
    assert_eq!(fx.job_status("api"), "queued");
}

// ---------- E4 ----------

#[test]
fn e4_appends_all_merged_once_when_no_job_is_left_to_run() {
    let fx = fx("");
    fx.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");
    fx.add_job("web", &[], "UPDATE jobs SET status = 'cancelled', cancel_reason = 'no', cancel_requested_at = 't' WHERE key = 'web';");

    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    std::thread::sleep(Duration::from_millis(2500));
    engine.stop();

    assert_eq!(fx.events("all-merged"), [(Some("pay".into()), None, None, json!({"pr": "repo", "merged": ["api"], "cancelled": ["web"]}))]);
    assert!(fx.texts("all-merged")[0].ends_with("Next: cpd pr open --pr repo --plan pay"));
    assert!(fx.query::<Option<String>>("SELECT all_merged_at FROM prs WHERE plan = 'pay'").is_some());
}

#[test]
fn e4_waits_while_a_job_is_queued() {
    let fx = fx("concurrency = 1");
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago(1));
    fx.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");

    let engine = fx.start();
    std::thread::sleep(Duration::from_millis(2500));
    engine.stop();

    assert_eq!(fx.count("all-merged"), 0);
}

// ---------- E5 ----------

/// Plan `pay` in phase `pr`.
fn in_pr(fx: &Fx) {
    fx.fixture(
        "UPDATE plans SET phase = 'pr' WHERE id = 'pay';
         UPDATE prs SET status = 'open', pr_number = 7, pr_url = 'https://example.invalid/pr/7', pr_opened_at = 't' WHERE plan = 'pay';",
    );
}

#[test]
fn e5_runs_pr_poll_for_each_plan_in_pr_every_pr_poll_seconds() {
    let fx = fx("");
    in_pr(&fx);
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        fs::read_to_string(fx.env.cpd_home().join("config.toml")).unwrap().replace("tick_seconds = 1\n", "tick_seconds = 1\npr_poll_seconds = 2\n"),
    )
    .unwrap();
    fx.fake_cpd(r#"echo '{"ok": true, "plan": "pay", "state": "OPEN", "merged": false, "changes": {"checks": [], "reviews": [], "comments": []}}'; exit 0"#);

    let engine = fx.start();
    wait_until("two polls", || fx.env.calls("cpd").len() >= 2);
    let started = Instant::now();
    engine.stop();

    let calls = fx.env.calls("cpd");
    assert!(calls.iter().all(|c| c == &["pr", "poll", "--plan", "pay"]), "{calls:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(fx.count("error"), 0);
}

#[test]
fn e5_reports_a_failing_poll_once() {
    let fx = fx("");
    in_pr(&fx);
    fx.fake_cpd(r#"echo '{"ok": false, "error": {"code": "gh_failed", "message": "gh failed.", "details": {"exit": 1}}}'; exit 1"#);
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        fs::read_to_string(fx.env.cpd_home().join("config.toml")).unwrap().replace("tick_seconds = 1\n", "tick_seconds = 1\npr_poll_seconds = 1\n"),
    )
    .unwrap();

    let engine = fx.start();
    wait_until("three polls", || fx.env.calls("cpd").len() >= 3);
    engine.stop();

    assert_eq!(
        fx.events("error"),
        [(Some("pay".into()), None, None, json!({"source": "watch", "code": "gh_failed", "message": "gh failed.", "details": {"exit": 1}}))]
    );
}

// ---------- E6 ----------

#[test]
fn e6_imports_the_context_stores_at_start_and_syncs_those_with_a_remote() {
    let fx = fx("");
    // Context `demo` with a note, written on another home of this machine.
    let other = fx.env.root().join("other-home");
    fs::create_dir_all(&other).unwrap();
    fs::copy(fx.env.cpd_home().join("config.toml"), other.join("config.toml")).unwrap();
    let on_other = |args: &[&str]| {
        let out = fx.env.command(CPD).args(args).env("CPD_HOME", &other).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stdout));
    };
    on_other(&["context", "new", "demo", "--title", "Demo"]);
    on_other(&["context", "note", "Remember this.", "--context", "demo"]);
    let store = fx.env.cpd_home().join("contexts/demo");
    copy_dir(&other.join("contexts/demo"), &store);
    let remote = fx.env.root().join("remote.git");
    fx.env.git(fx.env.root(), &["init", "-q", "--bare", remote.to_str().unwrap()]);
    fx.env.git(&store, &["remote", "add", "origin", remote.to_str().unwrap()]);

    let engine = fx.start();
    wait_until("the push", || fx.env.git(&remote, &["branch", "--list", "main"]).contains("main"));
    engine.stop();

    assert_eq!(fx.query::<String>("SELECT title FROM contexts WHERE id = 'demo'"), "Demo");
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM context_records WHERE context = 'demo'"), 1);
    assert_eq!(fx.count("context-created"), 1);
    assert_eq!(fx.count("context-record"), 1);
    assert_eq!(fx.env.git(&remote, &["rev-parse", "main"]), fx.env.git(&store, &["rev-parse", "main"]));
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

// ---------- scripts/install-cpd.sh (Finding 5) ----------

/// Runs `scripts/install-cpd.sh` with a fake `cargo` that "builds" a fake
/// `cpd`, in `env`: the fake `cpd` answers `config` with the private tmux
/// socket, and records `watch --ensure` with the windows of the work session
/// at that moment. Returns the script's stderr and the fake's calls.
fn install(env: &TestEnv) -> (String, Vec<Vec<String>>) {
    let socket = env.tmux_socket();
    let built = env.fake(
        "cpd-built",
        &format!(
            r#"case "$1" in
config) echo '{{"ok":true,"config":{{"tmux":{{"socket":"{socket}","cockpit_session":"cpd","work_session":"cpd-work"}}}}}}' ;;
watch) tmux -L '{socket}' list-windows -t =cpd-work -F '#{{window_name}}' > '{windows}' 2>&1; echo '{{"ok":true,"started":true,"pane":"%9"}}' ;;
esac"#,
            windows = env.root().join("windows-at-ensure").display()
        ),
    );
    let target = env.root().join("target");
    let cargo = env.fake("fake-cargo", &format!("mkdir -p '{t}/release' && cp '{}' '{t}/release/cpd'", built.display(), t = target.display()));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out = env
        .command(root.join("scripts/install-cpd.sh"))
        .env("CARGO", cargo)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{stderr}");
    (stderr, env.calls("cpd-built"))
}

#[test]
fn install_restarts_a_running_engine_with_ensure_and_says_so() {
    let env = TestEnv::new();
    let new = |args: &[&str]| {
        let out = env.tmux(args);
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    new(&["new-session", "-d", "-s", "cpd-work", "-n", "watch", "sleep", "600"]);
    new(&["new-window", "-d", "-t", "=cpd-work:", "-n", "other", "sleep", "600"]);

    let (stderr, calls) = install(&env);

    assert!(stderr.contains("restarting the running engine"), "{stderr}");
    assert_eq!(calls, [vec!["config"], vec!["watch", "--ensure"]]);
    // The window was killed before --ensure ran; the other window is untouched.
    assert_eq!(fs::read_to_string(env.root().join("windows-at-ensure")).unwrap().trim(), "other");
}

#[test]
fn install_leaves_a_server_without_an_engine_alone() {
    let env = TestEnv::new();
    let out = env.tmux(&["new-session", "-d", "-s", "cpd-work", "-n", "other", "sleep", "600"]);
    assert!(out.status.success());

    let (stderr, calls) = install(&env);

    assert!(!stderr.contains("restarting the running engine"), "{stderr}");
    assert_eq!(calls, [vec!["config"]]);
}
