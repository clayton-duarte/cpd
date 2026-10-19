//! The engine, `cpd watch` (contract §3.7, §6.6; commands.md §12): the lock
//! and its refusal, the signals, `--ensure`, and each step E1–E6 (E7, the PR start, is in pr_start.rs) on a
//! prepared database, with a private tmux server and fake `pi` and `cpd`.

mod common;
#[path = "common/checkout.rs"]
mod checkout;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{CPD, Run, SLEEP_SECONDS, TestEnv, copy_executable, wait_until};
use cpd_core::error::CpdError;
use cpd_core::git::{self, Setup, create_attempt_worktree, create_lead_worktree};
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
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'main', 'dispatch', 'cpd/pay', '{lead}', cpd_txn(), 't', 't');",
        repo = fx.repo.display(),
        lead = fx.lead.display()
    ));
    fx
}

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
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');"
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

    /// `start` with the engine run from the binary at `exe`.
    fn start_from(&self, exe: &Path) -> Engine {
        self.start_with(exe, &[])
    }

    /// `start_from` with the variables `vars` added to the engine's environment.
    fn start_with(&self, exe: &Path, vars: &[(&str, &str)]) -> Engine {
        let mut command = self.env.command(exe);
        command.envs(vars.iter().copied());
        let child = command.arg("watch").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
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

/// `tmux kill-window` (the install's restart of the engine) sends SIGHUP: it is a stop, not a crash.
#[test]
fn watch_stops_on_sighup_with_one_watch_stopped() {
    let fx = fx("");
    let engine = fx.start();
    let pid = engine.pid();

    let out = engine.stop_with(Signal::SIGHUP);
    assert_eq!(out.json(), json!({"ok": true, "stopped": true}), "{out:?}");
    assert_eq!(out.exit, 0);
    assert_eq!(fx.events("watch-stopped"), [(None, None, None, json!({"pid": pid}))]);
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
    // The one call is the engine's own start: E5 runs the engine's executable, not `cpd` from `PATH`.
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

/// engine-W10: both calls probe the lock before either engine has taken it (the fake `cpd` is slow to start), so both create
/// a window and one engine loses the lock (`watch_running`, its pane dies). Both calls succeed, one engine runs, no dead window.
#[test]
fn two_concurrent_ensures_both_succeed_and_leave_one_engine() {
    let fx = fx("");
    fx.env.fake("cpd", &format!("sleep 1; exec '{CPD}' \"$@\""));

    let spawn = || {
        common::spawn(fx.env.command(CPD).args(["watch", "--ensure"]).stdout(Stdio::piped()).stderr(Stdio::piped())).unwrap()
    };
    let (first, second) = (spawn(), spawn());
    let runs: Vec<Run> = [first, second]
        .into_iter()
        .map(|child| {
            let out = child.wait_with_output().unwrap();
            Run {
                exit: out.status.code().expect("cpd was killed by a signal"),
                stdout: String::from_utf8(out.stdout).unwrap(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            }
        })
        .collect();
    for run in &runs {
        run.ok();
    }
    assert_eq!(runs.iter().filter(|r| r.json()["started"] == true).count(), 1, "{runs:?}");
    assert_eq!(fx.count("watch-started"), 1);
    let panes = fx.env.tmux(&["list-panes", "-a", "-F", "#{session_name}:#{window_name} #{pane_dead}"]);
    assert_eq!(String::from_utf8_lossy(&panes.stdout), "cpd-work:watch 0\n");
}

/// engine-W10: `list-panes` fails while a concurrent call's `new-session` is starting the server, and the server is running
/// a moment later. The call lists again instead of failing with the first call's stderr.
#[test]
fn ensure_lists_panes_again_when_the_first_list_panes_fails_while_the_server_runs() {
    let fx = fx("");
    fx.env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
    fx.env.tmux(&["new-session", "-d", "-s", "other"]);
    let real = fx.env.run_ok(fx.env.root(), "sh", &["-c", "command -v tmux"]);
    let flag = fx.env.root().join("list-panes-failed");
    fx.env.fake(
        "tmux",
        &format!(
            "case \" $* \" in *\" list-panes \"*) [ -e '{flag}' ] || {{ : > '{flag}'; echo 'error connecting to x (No such file or directory)' >&2; exit 1; }};; esac\nexec '{real}' \"$@\"",
            flag = flag.display(),
            real = real.trim()
        ),
    );
    let out = fx.env.cpd(&["watch", "--ensure"]).ok();
    assert_eq!(out["started"], true, "{out}");
    assert!(flag.exists());
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
fn e2_removes_the_lock_of_a_dead_driver_it_ends_and_does_not_recreate_it() {
    let fx = fx("retry_cap = 0");
    // `api-a1`: a dead driver left its lock file, free. `web-a1`: the file is missing, as an ended
    // Attempt's is (E2's probe must not create it: no driver).
    drop(fx.live_driver("api-a1"));
    fx.add_running("api", "running", &ago(1));
    fx.add_running("web", "running", &ago(1));
    let lock = |attempt: &str| fx.home().attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(attempt).unwrap());
    assert!(lock("api-a1").exists() && !lock("web-a1").exists());

    poll_every(&fx, 1);
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
    // 40 minutes since it started, 39 of them waiting for its Lead in `starting` (engine-W11): 1 minute active.
    fx.add_running_since("waited", "running", &ago(40), Some(&ago(1)));
    let _waited = fx.live_driver("waited-a1");

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("stop requests", || fx.attempt("late-a1").3.is_some() && fx.attempt("gone-a1").3.is_some());
    // One more tick, for the Attempts it must leave alone.
    fx.env.wait_whole_ticks(1);
    engine.stop();

    assert_eq!(fx.attempt("late-a1"), ("running".into(), None, None, Some("timed_out".into())));
    assert_eq!(fx.attempt("gone-a1"), ("running".into(), None, None, Some("cancelled".into())));
    assert_eq!(fx.attempt("asked-a1"), ("running".into(), None, None, None));
    assert_eq!(fx.attempt("fresh-a1"), ("running".into(), None, None, None));
    assert_eq!(fx.attempt("waited-a1"), ("running".into(), None, None, None));
    // A data write for the driver: no Event of its own.
    assert_eq!(fx.count("job-cancelled") + fx.count("job-failed") + fx.count("error"), 0, "{:?}", fx.texts("error"));
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
    assert_eq!(fx.events("job-merged")[0].3, json!({"sha": tip, "tier": "S2"}));
    assert!(!made.path.exists());
}

/// A window `pay--api-a1` in the work session, with another to keep the session alive.
fn worker_window(fx: &Fx) {
    for args in [
        &["new-session", "-d", "-s", "cpd-work", "-n", "keep", "sleep", "600"][..],
        &["new-window", "-d", "-t", "=cpd-work:", "-n", "pay--api-a1", "sleep", "600"][..],
    ] {
        let out = fx.env.tmux(args);
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    assert!(worker_window_is_up(fx));
}

fn worker_window_is_up(fx: &Fx) -> bool {
    let out = fx.env.tmux(&["list-windows", "-a", "-F", "#{session_name}:#{window_name}"]);
    String::from_utf8_lossy(&out.stdout).contains("cpd-work:pay--api-a1")
}

/// A `tmux` on `PATH` whose `kill-window` fails: the first call only, or every call; every other call is the real tmux.
/// Returns the file it creates at its first failure.
fn failing_kill_window(fx: &Fx, every_call: bool) -> PathBuf {
    let real = String::from_utf8(fx.env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    let real = real.trim();
    assert!(real.starts_with('/'), "{real}");
    let marker = fx.env.root().join("kill-failed");
    let only_first = format!(" && [ ! -e '{}' ]", marker.display());
    let condition = if every_call { "" } else { only_first.as_str() };
    fx.env.fake(
        "tmux",
        &format!(
            "for a in \"$@\"; do if [ \"$a\" = kill-window ]{condition}; then : > '{m}'; echo 'fake: kill-window failed' >&2; exit 1; fi; done\nexec '{real}' \"$@\"",
            m = marker.display()
        ),
    );
    marker
}

#[test]
fn e2_kills_the_window_of_a_dead_driver_it_ends_though_the_first_kill_window_fails() {
    let fx = fx("retry_cap = 0");
    fx.add_running("api", "running", &ago(1));
    worker_window(&fx);
    let marker = failing_kill_window(&fx, false);

    let engine = fx.start();
    wait_until("the attempt ended and its window gone", || fx.attempt("api-a1").0 == "ended" && !worker_window_is_up(&fx));
    engine.stop();

    assert!(marker.exists(), "the fake tmux never failed a kill-window");
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

#[test]
fn r3_kills_the_window_of_a_merge_that_did_not_happen_though_the_first_kill_window_fails() {
    let fx = fx("");
    let (made, _) = merging_api(&fx, false);
    worker_window(&fx);
    let marker = failing_kill_window(&fx, false);

    let engine = fx.start();
    wait_until("the attempt ended and its window gone", || fx.attempt("api-a1").0 == "ended" && !worker_window_is_up(&fx));
    engine.stop();

    assert!(marker.exists(), "the fake tmux never failed a kill-window");
    assert_eq!(fx.attempt("api-a1").2.as_deref(), Some("crashed"));
    // R3 keeps the worktree for debugging.
    assert!(made.path.exists());
}

#[test]
fn r2_removes_the_worktree_though_every_kill_window_fails() {
    let fx = fx("");
    let (made, _) = merging_api(&fx, true);
    worker_window(&fx);
    let marker = failing_kill_window(&fx, true);

    let engine = fx.start();
    wait_until("the merge recorded and its worktree removed", || fx.attempt("api-a1").0 == "ended" && !made.path.exists());
    engine.stop();

    assert!(marker.exists(), "the fake tmux never failed a kill-window");
    assert_eq!(fx.attempt("api-a1").1.as_deref(), Some("merged"));
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

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("three without a pane", || fx.count("session-moved") == 3);
    fx.env.wait_whole_ticks(1);
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

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("the missing pane's Session stopped", || fx.count("session-moved") == 1);
    fx.env.wait_whole_ticks(2);
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

/// E3 starts urgent Ready Jobs first (§3.7): with every slot free, the urgent Job goes before an older non-urgent one.
#[test]
fn e3_starts_an_urgent_job_before_an_older_one() {
    let fx = fx("concurrency = 1\nretry_cap = 0");
    work_session_with_a_worker_that_stays(&fx);
    fx.add_job("old", &[], "");
    fx.add_job("hot", &[], &urgent("hot"));

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("attempt-started", || fx.count("attempt-started") >= 1);
    // Two more ticks: the one slot is taken, so the older Job still waits.
    fx.env.wait_whole_ticks(2);
    engine.stop();

    let started: Vec<Option<String>> = fx.events("attempt-started").into_iter().map(|e| e.1).collect();
    assert_eq!(started, [Some("hot".to_owned())]);
    assert_eq!((fx.job_status("hot"), fx.job_status("old")), ("running".into(), "queued".into()));
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

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("attempt-started", || fx.count("attempt-started") >= 1);
    // Two more ticks: the second urgent Job and the older one still wait.
    fx.env.wait_whole_ticks(2);
    engine.stop();

    // The first urgent Job by `seq` started (running Attempts: 2 = concurrency + 1); the second urgent one and the older Job wait.
    let started: Vec<Option<String>> = fx.events("attempt-started").into_iter().map(|e| e.1).collect();
    assert_eq!(started, [Some("hot1".to_owned())]);
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM attempts WHERE status <> 'ended'"), 2);
    assert_eq!((fx.job_status("hot1"), fx.job_status("hot2"), fx.job_status("old")), ("running".into(), "queued".into(), "queued".into()));
}

/// A pending restart of the tmux server (contract §6.1, hs-d294) holds E3: no Attempt starts while it
/// is pending, a running one is left alone, and the Job starts at the next tick once it is cleared.
#[test]
fn a_pending_tmux_restart_stops_e3_from_starting_attempts_and_running_ones_finish() {
    let fx = fx("concurrency = 2\nretry_cap = 0");
    work_session_with_a_worker_that_stays(&fx);
    // One Attempt runs (its driver holds the lock); a slot is free and a Job is ready.
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago(1));
    fx.add_job("next", &[], "");
    let pending = fx.env.cpd_home().join("run/tmux-restart.pending");
    fs::create_dir_all(pending.parent().unwrap()).unwrap();
    fs::write(&pending, r#"{"at":"2026-10-19T12:00:00.000Z","server":"3.4","installed":"3.8"}"#).unwrap();

    let engine = fx.start();
    // `doctor` shows the planned hold, which is not a failure.
    let doctor = fx.env.cpd(&["doctor"]).json();
    let checks = if doctor["ok"] == true { &doctor["checks"] } else { &doctor["error"]["details"]["checks"] };
    let check = checks.as_array().unwrap().iter().find(|c| c["name"] == "tmux_restart").unwrap();
    assert_eq!((check["ok"].as_bool(), check["message"].as_str()), (Some(true), Some("tmux restart pending")));
    wait_until("E3's hold to be logged", || fs::read_to_string(fx.home().watch_log()).unwrap_or_default().contains("E3: holding"));
    // time-claim: a pending restart holds E3, so no Attempt starts over several engine ticks; 2.5 s covers at least two ticks; no per-poll signal from the engine for a tick that held.
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(fx.count("attempt-started"), 0, "a pending restart holds E3");
    assert_eq!((fx.job_status("next"), fx.attempt("busy-a1").0), ("queued".into(), "running".into()));
    assert!(fx.events("error").is_empty(), "the hold is not an error: {:?}", fx.events("error"));

    // Cleared (a restart that ran, or none needed): the Job starts at once.
    fs::remove_file(&pending).unwrap();
    wait_until("attempt-started", || fx.count("attempt-started") == 1);
    engine.stop();
    assert_eq!(fx.events("attempt-started")[0].1.as_deref(), Some("next"));
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
    let deadline = Instant::now() + Duration::from_secs(60);
    while fx.attempt("api-a1").0 != "ended" {
        assert!(Instant::now() < deadline, "the driver never ended api-a1");
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(fx.query::<Option<i64>>("SELECT driver_pid FROM attempts WHERE id = 'api-a1'").is_some(), "the driver was not started");
    let texts = fx.texts("attempt-ended");
    assert!(texts.iter().all(|t| !t.contains("cannot run") && !t.contains("deleted")), "{texts:?}");
    assert!(!fx.texts("error").iter().any(|t| t.contains("deleted")));
}

/// E5 runs `cpd pr poll` from the path recorded at start, not from `PATH` (where `fx_bare` put a `cpd` that records its calls).
#[test]
fn e5_polls_with_the_engines_recorded_executable_not_the_one_on_path() {
    let fx = fx_bare("");
    let installed = fx.env.root().join("cpd-installed");
    copy_executable(CPD, &installed);
    let engine = fx.start_from(&installed);

    wait_until("a poll (the search reaches gh)", || !fx.env.calls("gh").is_empty());
    engine.stop();

    assert_eq!(fx.env.calls("cpd"), Vec::<Vec<String>>::new(), "E5 ran the cpd on PATH");
    assert_eq!(fx.count("error"), 0);
}

#[test]
fn e3_reports_a_tampered_job_once_and_does_not_start_it() {
    let fx = fx("");
    fx.add_job("api", &[], &format!("UPDATE jobs SET fingerprint = '{}' WHERE key = 'api';", "0".repeat(64)));

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("error", || fx.count("error") == 1);
    fx.env.wait_whole_ticks(2);
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

/// The 00:02Z incident, reproduced: the engine cannot run the driver (its file is gone), so every Attempt
/// ends `failed` / `crashed` with `driver_pid` null. No Tier is climbed, no try is used up (`retry_cap = 0`
/// would fail the Job at the first), and E3 waits `2^k` seconds after the k-th failure.
#[test]
fn e3_spawn_failures_requeue_at_the_same_tier_never_fail_the_job_and_back_off() {
    let fx = fx("concurrency = 1\nretry_cap = 0");
    let installed = fx.env.root().join("cpd-installed");
    copy_executable(CPD, &installed);
    let engine = fx.start_from(&installed);
    fs::remove_file(&installed).unwrap();
    fx.add_job("api", &[], "");

    wait_until("three spawn failures", || fx.query::<i64>("SELECT count(*) FROM attempts WHERE status = 'ended'") >= 3);
    engine.stop();

    assert_ne!(fx.job_status("api"), "failed");
    assert_eq!(fx.count("job-failed"), 0);
    let attempts: Vec<(String, String, String, Option<i64>, String, String)> = fx
        .store()
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, tier, origin, driver_pid, started_at, ended_at FROM attempts WHERE job = 'api' AND status = 'ended' ORDER BY number",
            )?;
            Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    assert!(attempts.len() >= 3, "{attempts:?}");
    for (id, tier, origin, pid, _, _) in &attempts {
        assert_eq!((tier.as_str(), origin.as_str(), *pid), ("S2", "ladder", None), "{id}");
    }
    for (i, (id, ..)) in attempts.iter().enumerate() {
        assert_eq!(fx.attempt(id), ("ended".into(), Some("failed".into()), Some("crashed".into()), None));
        let retry = &fx.events("attempt-retry")[i];
        assert_eq!((&retry.3["cause"], &retry.3["next_tier"]), (&json!("crashed"), &json!("S2")));
    }
    // After the k-th failure the next start waits 2^k seconds.
    for (k, pair) in attempts.windows(2).enumerate() {
        let waited = parse_time(&pair[1].4) - parse_time(&pair[0].5);
        let k = (k + 1) as u32;
        assert!(waited >= time::Duration::seconds(2i64.pow(k)), "after failure {k}: waited {waited}");
    }
}

/// A Job whose latest 9 Attempts never had a driver, the last just ended: E3 waits 300 s, so no tick starts it.
#[test]
fn e3_does_not_start_a_job_in_its_backoff() {
    let fx = fx("");
    let now = ago(0);
    let attempts: String = (1..=9).map(|n| ended_attempt("api", n, "S2", None, &now)).collect();
    fx.add_job("api", &[], &attempts);

    poll_every(&fx, 1);
    let engine = fx.start();
    fx.env.wait_whole_ticks(3);
    engine.stop();

    assert_eq!(fx.query::<i64>("SELECT count(*) FROM attempts"), 9);
    assert_eq!(fx.job_status("api"), "queued");
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
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
    let child = fx.env.command(CPD).arg("watch").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    Engine { child: Some(child) }
}

/// A store that stays busy for longer than `store_busy_ms` does not end the engine, at its start or in a
/// step, and no Event is lost: it waits, logs, and goes on when the lock is released.
#[test]
fn a_store_busy_for_longer_than_store_busy_ms_never_ends_the_engine_or_loses_an_event() {
    let fx = fx(SHORT_BUSY);
    // E5's poll is slow, and says when it runs: the engine is then in its first tick, past E4.
    let marker = fx.env.root().join("gh-ran");
    fx.env.fake_gh(&format!("touch '{}'; sleep 1.5; echo '{EMPTY_SEARCH}'", marker.display()));
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

    // A step: the second tick's E4 meets the lock, taken while E5 of the first tick is still running.
    fx.fixture("UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");
    let busy = watch_log(&fx).matches("store_busy: ").count();
    let held = hold_write_lock(&fx);
    wait_until("the engine to meet the lock", || watch_log(&fx).matches("store_busy: ").count() > busy);
    assert_eq!(fx.count("all-merged"), 0);
    held.release();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    assert_eq!(engine.child.as_mut().unwrap().try_wait().unwrap(), None, "the engine exited");
    engine.stop();

    let log = watch_log(&fx);
    assert!(log.contains("store_busy: "), "{log}");
    assert!(log.contains("the store is clear again after "), "{log}");
    assert_eq!(fx.count("error"), 0, "{log}");
    assert_eq!(fx.count("all-merged"), 1);
}

/// `watch-stopped` is appended when the engine stops while the store is busy, however long that lasts.
#[test]
fn watch_stopped_waits_for_a_busy_store() {
    let fx = fx(SHORT_BUSY);
    let mut engine = fx.start();
    let pid = engine.pid();

    let held = hold_write_lock(&fx);
    engine.signal(Signal::SIGTERM);
    // `watch-stopped` met the lock for longer than `store_busy_ms`: its `store_busy` is logged after the stop.
    wait_until("the stopping engine to meet the lock", || {
        watch_log(&fx).split("stopping on a signal").nth(1).is_some_and(|after| after.contains("store_busy: "))
    });
    assert_eq!(engine.child.as_mut().unwrap().try_wait().unwrap(), None, "the engine exited without watch-stopped");
    assert_eq!(fx.count("watch-stopped"), 0);
    held.release();
    let out = engine.child.take().unwrap().wait_with_output().unwrap();

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(fx.events("watch-stopped"), [(None, None, None, json!({"pid": pid}))]);
    assert!(!watch_log(&fx).contains("cannot append watch-stopped"), "{}", watch_log(&fx));
}

// ---------- E4 ----------

#[test]
fn e4_appends_all_merged_once_when_no_job_is_left_to_run() {
    let fx = fx("");
    fx.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");
    fx.add_job("web", &[], "UPDATE jobs SET status = 'cancelled', cancel_reason = 'no', cancel_requested_at = 't' WHERE key = 'web';");

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    fx.env.wait_whole_ticks(2);
    engine.stop();

    assert_eq!(fx.events("all-merged"), [(Some("pay".into()), None, None, json!({"pr": "repo", "merged": ["api"], "cancelled": ["web"]}))]);
    assert!(fx.texts("all-merged")[0].ends_with("Next: cpd pr open --pr repo --plan pay"));
    assert!(fx.query::<Option<String>>("SELECT all_merged_at FROM prs WHERE plan = 'pay'").is_some());
}

#[test]
fn e4_sends_all_merged_again_after_a_job_is_appended_and_merged() {
    let fx = fx("");
    fx.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    let spec = fx.env.root().join("late-spec.json");
    fs::write(&spec, json!({"goal": "Do it.", "context": "Because.", "touches": ["late.txt"], "acceptance": ["ok"], "out_of_scope": []}).to_string()).unwrap();
    let added = fx.env.command(CPD).args(["job", "add", "late", "--title", "Late", "--tier", "S2", "--spec"]).arg(&spec).args(["--plan", "pay"]).output().unwrap();
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stdout));
    assert!(fx.query::<Option<String>>("SELECT all_merged_at FROM prs WHERE plan = 'pay'").is_none());
    // The Job merges (set directly; no Worker runs here).
    fx.fixture("UPDATE jobs SET status = 'running' WHERE key = 'late'; UPDATE jobs SET status = 'merged', merged_sha = 'def' WHERE key = 'late';");
    wait_until("second all-merged", || fx.count("all-merged") == 2);
    fx.env.wait_whole_ticks(2);
    engine.stop();

    assert_eq!(fx.count("all-merged"), 2);
    assert_eq!(fx.events("all-merged")[1].3["merged"], json!(["api", "late"]));
}

#[test]
fn e4_waits_while_a_job_is_queued() {
    let fx = fx("concurrency = 1");
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago(1));
    fx.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");

    poll_every(&fx, 1);
    let engine = fx.start();
    fx.env.wait_whole_ticks(2);
    engine.stop();

    assert_eq!(fx.count("all-merged"), 0);
}

/// A draft added after the lock (`job add --draft`, contract §3.2): E3 never
/// starts it, and E4 does not send `all-merged` while it remains, so the PR
/// waits for the batch. Once it is cancelled, E4 completes the PR.
#[test]
fn e3_never_starts_a_draft_and_e4_waits_while_one_remains() {
    let fx = fx("");
    // The work session as tui leaves it, so E3 could start an Attempt if it chose to.
    for args in [&["new-session", "-d", "-s", "cpd-work", "-n", "x", "sleep", "600"][..], &["set", "-g", "remain-on-exit", "on"]] {
        assert!(fx.env.tmux(args).status.success(), "tmux {args:?}");
    }
    fx.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");
    let spec = fx.env.root().join("spec.json");
    fs::write(&spec, r#"{"goal": "Later.", "context": "c", "touches": ["b.txt"], "acceptance": ["ok"], "out_of_scope": []}"#).unwrap();
    let added = fx.env.cpd(&["job", "add", "later", "--title", "Later", "--tier", "S2", "--spec", spec.to_str().unwrap(), "--draft", "--plan", "pay"]).ok();
    assert_eq!((&added["job"]["status"], &added["job"]["appended"]), (&json!("draft"), &json!(true)));

    poll_every(&fx, 1);
    let engine = fx.start();
    fx.env.wait_whole_ticks(3);
    engine.stop();
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM attempts"), 0, "a draft never gets an Attempt");
    assert_eq!(fx.job_status("later"), "draft");
    assert_eq!(fx.count("all-merged"), 0, "the PR waits for its draft");
    assert!(fx.query::<Option<String>>("SELECT all_merged_at FROM prs WHERE plan = 'pay'").is_none());

    fx.env.cpd(&["job", "cancel", "later", "--reason", "Not needed.", "--plan", "pay"]).ok();
    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    engine.stop();
    assert_eq!(fx.events("all-merged")[0].3, json!({"pr": "repo", "merged": ["api"], "cancelled": ["later"]}));
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

/// The lookup of PR 7 (`in_pr`) that a poll makes when the search does not list it: still open, so nothing changes.
const LOOKUP_OPEN: &str = r#"{"data":{"p0":{"pullRequest":{"repository":{"nameWithOwner":"acme/pay"},"number":7,"url":"https://github.com/acme/pay/pull/7","title":"Pay","isDraft":false,"state":"OPEN","createdAt":"t","mergedAt":null,"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","commits":{"nodes":[]}}}}}"#;

/// Sets `pr_poll_seconds` (`tick_seconds` is one second).
fn poll_every(fx: &Fx, seconds: u32) {
    let config = fx.env.cpd_home().join("config.toml");
    fs::write(&config, fs::read_to_string(&config).unwrap().replace("tick_seconds = 1\n", &format!("tick_seconds = 1\npr_poll_seconds = {seconds}\n"))).unwrap();
}

/// Milliseconds since the epoch, as the fake `gh` writes them.
fn epoch_ms(at: &str) -> i128 {
    OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).unwrap().unix_timestamp_nanos() / 1_000_000
}

/// `seconds` from now, as `rate_limit.reset_at` is written.
fn in_seconds(seconds: i64) -> String {
    (OffsetDateTime::now_utc() + time::Duration::seconds(seconds))
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .unwrap()
}

#[test]
fn e5_polls_every_pr_poll_seconds_after_the_previous_poll_ended_whatever_the_plans() {
    // No Plan at all: E5 polls anyway.
    let fx = fx_bare("");
    poll_every(&fx, 2);
    let log = fx.env.root().join("poll.log");
    // A poll that lasts a second, so that "after the end" differs from "after the start", and that notes both.
    fx.env.fake_gh(&format!(
        r#"echo "start $(date +%s%N)" >> '{log}'; sleep 1; echo "end $(date +%s%N)" >> '{log}'; echo '{}'; exit 0"#,
        search_reply(4987, "2099-01-01T00:00:00Z"),
        log = log.display()
    ));

    let engine = fx.start();
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
            (0, "start", "end") => assert!(pair[1].1 - pair[0].1 >= 1000, "{notes:?}"),
            (1, "end", "start") => assert!(pair[1].1 - pair[0].1 >= 2000, "the next poll starts 2 s after the end of the last: {notes:?}"),
            _ => panic!("polls overlap or are out of order: {notes:?}"),
        }
    }
    assert_eq!(fx.count("error"), 0);
}

/// engine-W9: a poll of 1 s with a tick of 5 s and `pr_poll_seconds = 2` runs every 3 s (end + 2 s), not at every other tick.
#[test]
fn e5_polls_at_its_due_time_not_at_the_next_tick() {
    let fx = fx_bare("");
    poll_every(&fx, 2);
    let config = fx.env.cpd_home().join("config.toml");
    fs::write(&config, fs::read_to_string(&config).unwrap().replace("tick_seconds = 1\n", "tick_seconds = 5\n")).unwrap();
    let log = fx.env.root().join("poll.log");
    fx.env.fake_gh(&format!(
        r#"echo "start $(date +%s%N)" >> '{log}'; sleep 1; echo '{}'; exit 0"#,
        search_reply(4987, "2099-01-01T00:00:00Z"),
        log = log.display()
    ));

    let engine = fx.start();
    wait_until("three polls", || fs::read_to_string(&log).map_or(0, |t| t.matches("start").count()) >= 3);
    engine.stop();

    let starts: Vec<i128> = fs::read_to_string(&log).unwrap().lines().map(|l| l.split_once(' ').unwrap().1.parse::<i128>().unwrap() / 1_000_000).collect();
    for pair in starts.windows(2) {
        let gap = pair[1] - pair[0];
        assert!((2900..4000).contains(&gap), "a poll every 3 s (1 s poll + 2 s), not every 5 s tick: {starts:?}");
    }
    assert_eq!(fx.count("error"), 0);
}

#[test]
fn e5_polls_with_a_plan_in_pr_too_and_does_not_poll_more_often_than_every_pr_poll_seconds() {
    let fx = fx("");
    in_pr(&fx);
    poll_every(&fx, 3);
    fx.env.fake_gh(&format!(
        "case \"$4\" in *'search(type'*) echo '{}' ;; *) echo '{LOOKUP_OPEN}' ;; esac",
        search_reply(4987, "2099-01-01T00:00:00Z")
    ));

    let engine = fx.start();
    wait_until("the first poll", || fx.env.poll_searches() >= 1);
    let first = Instant::now();
    wait_until("the second poll", || fx.env.poll_searches() >= 2);
    let gap = first.elapsed();
    engine.stop();

    assert!(gap >= Duration::from_millis(1500), "one poll per pr_poll_seconds, not one per tick: {gap:?}");
    assert_eq!(fx.count("error"), 0);
}

#[test]
fn e5_reports_a_failing_poll_once() {
    let fx = fx("");
    in_pr(&fx);
    fx.env.fake_gh("echo 'gh: boom' >&2; exit 1");
    poll_every(&fx, 1);

    let engine = fx.start();
    wait_until("three polls", || fx.env.calls("gh").len() >= 3);
    engine.stop();

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!((plan, job, attempt), (&None, &None, &None));
    assert_eq!((&data["source"], &data["code"], &data["details"]["exit"]), (&json!("watch"), &json!("gh_failed"), &json!(1)));
}

/// Whether process `pid` is running (a zombie is not).
fn running(pid: i32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.rsplit(") ").next().is_some_and(|r| !r.starts_with('Z')))
}

/// engine-W4: a `cpd pr poll` child that hangs (its `gh` sleeps, and so does a child of
/// that `gh`) is killed at its deadline with its whole process group, and E5 reports
/// `tool_timeout`. The deadline is shortened to 2 s by `CPD_TEST_DEADLINE_CPD`, which a
/// debug build reads.
#[test]
fn e5_kills_a_hung_poll_child_and_its_group_at_the_deadline_with_tool_timeout() {
    let fx = fx_bare("");
    let pids = fx.env.root().join("gh.pids");
    fx.env.fake_gh(&format!("sleep {SLEEP_SECONDS} & echo \"$$ $!\" >> '{}'; wait", pids.display()));

    let engine = fx.start_with(Path::new(CPD), &[("CPD_TEST_DEADLINE_CPD", "2")]);
    wait_until("E5's tool_timeout", || fx.count("error") >= 1);
    engine.stop();

    let errors = fx.events("error");
    let data = &errors[0].3;
    assert_eq!((&data["source"], &data["code"]), (&json!("watch"), &json!("tool_timeout")), "{errors:?}");
    let details = &data["details"];
    assert_eq!((&details["tool"], &details["deadline_seconds"]), (&json!("cpd"), &json!(2)), "{details}");
    assert_eq!(details["argv"], json!([CPD, "pr", "poll"]));
    for pid in fs::read_to_string(&pids).unwrap().split_whitespace() {
        let pid: i32 = pid.parse().unwrap();
        wait_until(&format!("gh's process {pid} to end"), || !running(pid));
    }
}

#[test]
fn e5_pauses_below_100_requests_until_reset_and_reports_one_gh_rate_limited() {
    let fx = fx("");
    poll_every(&fx, 1);
    let reset_at = in_seconds(4);
    let log = fx.env.root().join("poll.log");
    let first = fx.env.root().join("first-poll-done");
    // The first poll leaves 50 requests until `reset_at`; the next one, which E5 must not run before that, 4987.
    fx.env.fake_gh(&format!(
        r#"echo "start $(date +%s%N)" >> '{log}'
if [ ! -e '{first}' ]; then touch '{first}'; echo '{low}'; else echo '{ok}'; fi; exit 0"#,
        log = log.display(),
        first = first.display(),
        low = search_reply(50, &reset_at),
        ok = search_reply(4987, "2099-01-01T00:00:00Z"),
    ));

    let engine = fx.start();
    wait_until("the second poll", || fx.env.calls("gh").len() >= 2);
    engine.stop();

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

#[test]
fn a_prerequisite_seen_merged_by_e5_still_lets_e7_start_its_dependent_in_the_same_tick() {
    let fx = fx("");
    // One tick only: whatever happens after the first poll happens in that tick.
    let config = fx.env.cpd_home().join("config.toml");
    fs::write(&config, fs::read_to_string(&config).unwrap().replace("tick_seconds = 1\n", "tick_seconds = 600\n")).unwrap();
    // The clean-up of the merged `repo` keeps a Lead worktree with commits not on origin (and raises an `error`): give
    // the repo an `origin` that has them, so the only `error` this test could see is E5's or E7's own.
    let origin = fx.env.root().join("repos/origin.git");
    fx.env.git(fx.env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    fx.env.git(&fx.repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    fx.env.git(&fx.repo, &["push", "-q", "origin", "main"]);
    // `web` is in its own repo: git cannot hold `cpd/pay` beside `cpd/pay/web`.
    let web = fx.env.git_repo("web").canonicalize().unwrap();
    fx.fixture(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
           VALUES ('acme/pay', 7, 'https://github.com/acme/pay/pull/7', 'Pay', 0, 'open', 'none', 'none', 't', 't');
         UPDATE prs SET status = 'open', pr_number = 7, pr_url = 'https://github.com/acme/pay/pull/7', pr_opened_at = 't',
                pull_request = (SELECT id FROM pull_requests WHERE number = 7) WHERE plan = 'pay' AND key = 'repo';
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'web', '{repo}', 'main', 'waiting', 'cpd/pay/web', cpd_txn(), 't');
         INSERT INTO pr_needs (plan, pr, needs) VALUES ('pay', 'web', 'repo');",
        repo = web.display()
    ));
    // The real `pr poll`, against a `gh` that finds PR 7 merged: the search lists nothing, the lookup answers MERGED.
    let merged = r#"{"repository":{"nameWithOwner":"acme/pay"},"number":7,"url":"https://github.com/acme/pay/pull/7","title":"Pay","isDraft":false,"state":"MERGED","createdAt":"t","mergedAt":"2026-10-14T10:00:00Z","reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","commits":{"nodes":[]}}"#;
    fx.env.fake_gh(&format!("case \"$4\" in *'search(type'*) echo '{EMPTY_SEARCH}' ;; *) echo '{{\"data\":{{\"p0\":{{\"pullRequest\":{merged}}}}}}}' ;; esac"));

    let engine = fx.start();
    wait_until("E7 to start web", || fx.count("pr-started") == 1);
    engine.stop();

    assert_eq!(fx.query::<String>("SELECT status FROM prs WHERE key = 'repo'"), "merged");
    assert_eq!(fx.query::<String>("SELECT status FROM prs WHERE key = 'web'"), "dispatch");
    assert!(fx.query::<i64>("SELECT id FROM events WHERE kind = 'pr-merged'") < fx.query::<i64>("SELECT id FROM events WHERE kind = 'pr-started'"));
    assert!(!fx.lead.exists(), "the poll that saw the merge cleaned up the Lead worktree");
    assert_eq!(fx.count("error"), 0);
}

// ---------- E6 (removed) ----------

#[test]
fn the_engine_neither_imports_nor_syncs_a_context_store() {
    let fx = fx("");
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

    poll_every(&fx, 1);
    let engine = fx.start();
    fx.env.wait_whole_ticks(2);
    let run = engine.stop();

    assert_eq!(fx.query::<i64>("SELECT count(*) FROM contexts WHERE id = 'demo'"), 0);
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM context_records"), 0);
    assert_eq!(fx.count("context-created") + fx.count("context-record"), 0);
    assert_eq!(fx.count("error"), 0);
    assert!(store.join("context.json").is_file(), "the folder is left alone");
    assert!(!run.stderr.contains("E6"), "{}", run.stderr);
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
watch) tmux -L '{socket}' list-windows -t =cpd-work -F '#{{window_name}}' > '{windows}' 2>&1; echo '{{"ok":true,"started":true,"pane":"%9"}}' ;;
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
    (out, env.calls("cpd-built"))
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
    assert_eq!(calls, [vec!["--version"], vec!["config"], vec!["watch", "--ensure"]]);
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
    assert_eq!(calls, [vec!["--version"], vec!["config"]]);
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
    let adapter = env.home().join(".local/share/cpd/pi");
    let expected: Vec<String> = tree(&checkout.join("pi")).into_iter().filter(|p| p != "test" && !p.starts_with("test/")).collect();
    assert!(expected.contains(&"extension.ts".to_string()) && expected.contains(&"lib/cpd.ts".to_string()), "{expected:?}");
    assert!(tree(&checkout.join("pi")).contains(&"test/cpd.test.ts".to_string()));
    assert_eq!(tree(&adapter), expected);
    // No temporary directory is left beside it.
    assert_eq!(names(adapter.parent().unwrap()), ["pi"]);
}

#[test]
fn install_replaces_an_installed_adapter_as_a_whole_and_it_is_never_missing() {
    let env = TestEnv::new();
    let adapter = env.home().join(".local/share/cpd/pi");
    fs::create_dir_all(adapter.join("old")).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fs::write(adapter.join("stale.ts"), "").unwrap();
    // A big old adapter, so that removing it in place takes a while.
    for i in 0..5000 {
        fs::write(adapter.join("old").join(format!("f{i}.ts")), "").unwrap();
    }

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher = {
        let (stop, manifest) = (stop.clone(), adapter.join("package.json"));
        std::thread::spawn(move || {
            let mut missing = 0u32;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                if !manifest.exists() {
                    missing += 1;
                }
            }
            missing
        })
    };
    let (out, _) = install_with(&env, OPENS);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let missing = watcher.join().unwrap();

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(missing, 0, "the adapter's package.json was missing {missing} times during the install");
    assert!(!adapter.join("stale.ts").exists() && !adapter.join("old").exists(), "the old adapter was not replaced as a whole");
    assert!(adapter.join("package.json").exists());
    assert_eq!(names(adapter.parent().unwrap()), ["pi"]);
}
