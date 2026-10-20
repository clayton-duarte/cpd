//! The engine, `cpd watch` (contract §3.7, §6.6; commands.md §12): the lock
//! and its refusal, the signals, `--ensure`, and each step E1–E6 (E7, the PR start, is in pr_start.rs) on a
//! prepared database, with a private tmux server and fake `pi` and `cpd`.

use crate::common;
#[path = "common/checkout.rs"]
mod checkout;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{CPD, PR_POLL_EVERY_TICK, Run, SLEEP_SECONDS, TICK_SECONDS, TestEnv, copy_executable, wait_until, wait_until_for};
use cpd_core::error::CpdError;
use cpd_core::git::{self, Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::{CpdHome, Infra};
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
        let mut sql = format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', '{pr}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
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
        let engine = Engine { child: Some(child) };
        let pid = engine.pid();
        wait_until("watch-started", || {
            self.env.query::<i64>(&format!(
                "SELECT count(*) FROM events WHERE kind = 'watch-started' AND json_extract(data, '$.pid') = {pid}"
            )) == 1
        });
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

    // A dead engine's window is replaced, not duplicated: the supervising hook starts it again at once (contract §6.6).
    kill(Pid::from_raw(pid as i32), Signal::SIGTERM).unwrap();
    wait_until("watch-stopped", || fx.count("watch-stopped") == 1);
    wait_until("the engine again", || fx.count("watch-started") == 2);
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

// ---------- supervision: the engine comes back whatever stopped it (contract §6.6, hs-d328) ----------

/// How long the engine has to be back after it stopped, whatever stopped it: "within seconds" (contract §6.6).
const BACK: Duration = Duration::from_secs(10);

impl Fx {
    /// The engine, started as a Plan starts it (`cpd watch --ensure`); its pid.
    fn supervised(&self) -> i64 {
        self.env.cpd(&["watch", "--ensure"]).ok();
        self.engine_pid()
    }

    /// The pid of the newest `watch-started`.
    fn engine_pid(&self) -> i64 {
        self.env.query("SELECT json_extract(data, '$.pid') FROM events WHERE kind = 'watch-started' ORDER BY id DESC LIMIT 1")
    }

    /// Waits until a new engine runs after the `starts` it had counted: a newer `watch-started`
    /// whose process is alive, in one `watch` window of the work session. Returns the time it took.
    fn back_after(&self, starts: usize) -> Duration {
        let began = Instant::now();
        wait_until_for("the engine to be back", BACK, || {
            self.count("watch-started") > starts && kill(Pid::from_raw(self.engine_pid() as i32), None).is_ok()
        });
        let windows = self.env.tmux(&["list-windows", "-t", "=cpd-work", "-F", "#{window_name}"]);
        assert_eq!(String::from_utf8_lossy(&windows.stdout), "watch\n", "one engine window, no dead one");
        began.elapsed()
    }

    /// A Manager Session of Context `ctx`.
    fn manager(&self, id: &str) {
        self.fixture(&format!(
            "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
               VALUES ('{id}', 'manager', 'ctx', 'Manager {id}', '/', 'O3', 'stopped', 't', 't');"
        ));
    }

    /// The one message the Manager `mgr1` got, a downtime that says `how`.
    fn manager_heard(&self, how: &str) {
        let messages = self.messages();
        assert_eq!(messages.len(), 1, "{messages:?}");
        let (session, _, data) = &messages[0];
        let text = data["text"].as_str().unwrap();
        assert_eq!((session.as_str(), data["wake"].clone()), ("mgr1", json!(false)), "{messages:?}");
        assert!(text.contains("The engine was down for ") && text.contains(how), "{text}");
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

/// A `cpd` whose views only sleep, so a cockpit can be built without terminals.
fn views_sleep(fx: &Fx) {
    fx.env.fake("cpd", &format!("case \"$1 $2\" in \"tui view\") exec sleep {SLEEP_SECONDS};; esac\nexec '{CPD}' \"$@\""));
}

#[test]
fn a_crashed_engine_is_back_within_seconds() {
    let fx = fx("");
    let pid = fx.supervised();
    kill(Pid::from_raw(pid as i32), Signal::SIGABRT).unwrap();
    fx.back_after(1);
    assert_eq!(fx.count("watch-stopped"), 0, "a crash says nothing");
    assert_ne!(fx.engine_pid(), pid);
}

#[test]
fn an_engine_that_got_sigterm_is_back_within_seconds() {
    let fx = fx("");
    fx.manager("mgr1");
    let pid = fx.supervised();
    kill(Pid::from_raw(pid as i32), Signal::SIGTERM).unwrap();
    fx.back_after(1);
    assert_eq!(fx.count("watch-stopped"), 1);
    fx.manager_heard("it stopped at");
}

#[test]
fn an_engine_that_got_sigkill_is_back_within_seconds() {
    let fx = fx("");
    fx.manager("mgr1");
    let pid = fx.supervised();
    kill(Pid::from_raw(pid as i32), Signal::SIGKILL).unwrap();
    fx.back_after(1);
    fx.manager_heard("it was last alive");
}

/// The work session holds nothing else: tmux would exit with its last window, and the server stays so that its hook can run (`exit-empty off`).
#[test]
fn an_engine_whose_window_was_killed_is_back_within_seconds() {
    let fx = fx("");
    fx.supervised();
    assert!(fx.env.tmux(&["kill-window", "-t", "=cpd-work:watch"]).status.success());
    fx.back_after(1);
}

/// An install that kills the `watch` window and then does nothing (it failed, or it was cut short).
#[test]
fn an_engine_killed_by_an_install_that_then_does_nothing_is_back_within_seconds() {
    let fx = fx("");
    fx.env.tmux(&["new-session", "-d", "-s", "cockpit", "sleep", "600"]);
    fx.supervised();
    fx.env.fake("fake-install", &format!("tmux -L '{}' kill-window -t =cpd-work:watch", fx.env.tmux_socket()));
    assert!(fx.env.command(fx.env.bin().join("fake-install")).status().unwrap().success());
    fx.back_after(1);
}

/// A plain `--ensure` waits for `locks/watch-ensure.lock` like a supervising one: held by another call between its probe and its
/// start, it keeps the plain call from probing, so no second window starts. Released, the call starts the engine.
#[test]
fn a_plain_ensure_waits_for_the_ensure_lock_another_call_holds() {
    let fx = fx("");
    let path = fx.env.cpd_home().join("locks/watch-ensure.lock");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
    let held = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    let mut child = common::spawn(fx.env.command(CPD).args(["watch", "--ensure"]).stdout(Stdio::piped()).stderr(Stdio::piped())).unwrap();
    // time-claim: nothing signals "the call did not start", so it gets 2 s, far over what a start that is not held takes to begin.
    std::thread::sleep(Duration::from_secs(2));
    assert!(child.try_wait().unwrap().is_none(), "the plain call returned while the lock was held");
    assert_eq!(fx.count("watch-started"), 0);
    drop(held);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(serde_json::from_slice::<Value>(&out.stdout).unwrap()["started"], true);
    fx.back_after(0);
}

/// The install's own `watch --ensure` right after its kill: the hook and the script both start the engine, and one runs.
#[test]
fn an_install_that_restarts_the_engine_itself_leaves_one_engine() {
    let fx = fx("");
    fx.supervised();
    assert!(fx.env.tmux(&["kill-window", "-t", "=cpd-work:watch"]).status.success());
    fx.env.cpd(&["watch", "--ensure"]).ok();
    fx.back_after(1);
    wait_until("the dust to settle: one engine process holds the lock", || fx.env.cpd(&["watch"]).error()["code"] == "watch_running");
    assert_eq!(fx.count("watch-started"), 2);
}

/// A tmux restart kills the server, hooks and all (nothing in it can start the engine); the restart's own path builds the cockpit
/// again (`cpd tui`, contract §6.1), which starts the engine.
#[test]
fn an_engine_killed_with_its_tmux_server_is_back_once_the_cockpit_is_rebuilt() {
    let fx = fx("");
    views_sleep(&fx);
    fx.manager("mgr1");
    fx.env.cpd(&["tui"]).ok();
    wait_until("the engine", || fx.count("watch-started") == 1);
    fx.env.tmux(&["kill-server"]);
    wait_until("the engine to stop with its server", || fx.count("watch-stopped") == 1);

    fx.env.cpd(&["tui"]).ok();
    fx.back_after(1);
    fx.manager_heard("it stopped at");
}

#[test]
fn an_engine_that_cannot_start_is_tried_once_every_few_seconds_not_in_a_loop() {
    let fx = fx("");
    // The engine itself exits 7 at once; `watch --ensure` is the real one, as the hooks call it.
    fx.env.fake("cpd", &format!("case \"$2\" in --ensure) exec '{CPD}' \"$@\";; esac\nexit 7"));
    assert_eq!(fx.env.cpd(&["watch", "--ensure"]).error()["code"], "watch_start_failed");
    // time-claim: the back-off is a rate, so the bound is a duration; no signal says "no more starts will come".
    std::thread::sleep(Duration::from_secs(5));
    // Each failed start waits 2 s while it holds the one-at-a-time lock: a few starts in 5 s, not a loop of hundreds.
    let calls = fx.env.calls("cpd").len();
    assert!((3..=14).contains(&calls), "{calls} calls in 5 s");
    let log = fs::read_to_string(fx.env.cpd_home().join("run/watch.log")).unwrap();
    assert!(log.contains("supervision: cannot start the engine"), "{log}");
}

// ---------- the downtime message (hs-d328) ----------

#[test]
fn the_first_engine_tells_nobody_and_the_next_one_tells_each_manager_how_long_it_was_down() {
    let fx = fx("");
    fx.manager("mgr1");
    fx.fixture(
        "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at, archived_at)
           VALUES ('mgr0', 'manager', 'ctx', 'Old', '/', 'O3', 'stopped', 't', 't', 't');
         UPDATE contexts SET archived_at = NULL;",
    );
    let first = fx.start();
    assert_eq!(fx.messages(), [], "no engine before it");
    let started = fx.texts("watch-started");
    assert!(!started[0].contains("down for"), "{started:?}");
    let _ = first.stop();
    assert_eq!(fx.messages(), []);

    let second = fx.start();
    let messages = fx.messages();
    assert_eq!(messages.len(), 1, "one message, to the one Manager that is not archived: {messages:?}");
    let (session, context, data) = &messages[0];
    assert_eq!((session.as_str(), context.as_deref()), ("mgr1", Some("ctx")));
    assert_eq!(data["wake"], false);
    assert_eq!(data["from"], Value::Null);
    let text = data["text"].as_str().unwrap();
    let stopped_at = fx.env.query::<String>("SELECT at FROM events WHERE kind = 'watch-stopped'");
    assert!(text.contains(&format!("it stopped at {stopped_at}")), "{text}");
    // The downtime is the time between the stop and the second start.
    let started_at = fx.env.query::<String>("SELECT at FROM events WHERE kind = 'watch-started' ORDER BY id DESC LIMIT 1");
    let secs: f64 = text.split("down for ").nth(1).unwrap().split(" s").next().unwrap().parse().unwrap();
    let between = (parse_time(&started_at) - parse_time(&stopped_at)).as_seconds_f64();
    assert!((secs - between).abs() < 0.5, "{secs} s said, {between} s between {stopped_at} and {started_at}: {text}");
    // Without a Manager the same sentence is in the Event, which is enough.
    assert!(fx.texts("watch-started")[1].contains(&format!("down for {secs:.1} s")), "{:?}", fx.texts("watch-started"));
    let _ = second.stop();
}

#[test]
fn an_engine_that_died_without_a_watch_stopped_is_told_from_the_last_time_it_was_alive() {
    let fx = fx("");
    fx.manager("mgr1");
    let first = fx.start();
    // Its ticks mark the lock file: the time it was last seen alive is its newest tick, not its start.
    let started_at = fx.env.query::<String>("SELECT at FROM events WHERE kind = 'watch-started'");
    let lock = fx.env.cpd_home().join("locks/watch.lock");
    wait_until("a tick after its start marked the lock", || {
        let marked = OffsetDateTime::from(fs::metadata(&lock).unwrap().modified().unwrap());
        marked - parse_time(&started_at) > time::Duration::milliseconds(500)
    });
    first.signal(Signal::SIGKILL);
    let mut first = first;
    let _ = first.child.take().unwrap().wait();
    assert_eq!(fx.count("watch-stopped"), 0);

    let second = fx.start();
    let messages = fx.messages();
    assert_eq!(messages.len(), 1, "{messages:?}");
    let text = messages[0].2["text"].as_str().unwrap();
    assert!(text.contains("it was last alive (it left no watch-stopped: a crash or a kill) at "), "{text}");
    assert!(!text.contains(&started_at), "alive since its ticks, not since its start: {text}\nstarted {started_at}");
    let _ = second.stop();
}

// ---------- @cpd_workers ----------

const NO_WORKERS: &str = "#[dim]workers #[nodim,fg=colour8]0#[default,dim]/3 · 0 waiting · 0 queued#[default]";
const ONE_WORKER: &str = "#[dim]workers #[nodim,fg=green]1#[default,dim]/3 · 0 waiting · 0 queued#[default]";

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
fn the_engine_sets_cpd_workers_at_start_and_only_when_it_changes() {
    let fx = fx("");
    fx.tmux_server();
    let lock = fx.live_driver("api-a1");
    fx.add_running("api", "running", &ago(1));

    let engine = fx.start();
    wait_until("@cpd_workers", || fx.workers() == ONE_WORKER);
    assert_eq!(fx.option("@cpd_workers_short"), "#[nodim,fg=green]1#[default,dim]/3·0·0#[default]");
    assert_eq!(fx.option("@cpd_workers_width"), "46", "inbox 0, one space, the long form, a pill's two caps each");
    assert_eq!(fx.option("@cpd_workers_short_width"), "19", "the same with the short form");

    // Unchanged, it is not set again: a value put there by hand survives the next ticks.
    assert!(fx.env.tmux(&["set", "-g", "@cpd_workers", "by hand"]).status.success());
    // time-claim: the value is not set again over more than one engine tick; no per-poll signal from the engine, which
    // sets nothing at a tick where the text did not change.
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(fx.workers(), "by hand");

    // A change sets it.
    fx.fixture("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'timed_out', ended_at = 't' WHERE id = 'api-a1';");
    wait_until("@cpd_workers to change", || fx.workers() == NO_WORKERS);
    engine.stop();
    drop(lock);
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

#[test]
fn the_engine_counts_waiting_and_queued_jobs() {
    let fx = fx("concurrency = 1");
    fx.tmux_server();
    let lock = fx.live_driver("run-a1");
    fx.add_running("run", "running", &ago(1));
    // Ready but the only slot is taken (E3 starts nothing); the other needs it, so it is queued and not waiting.
    fx.add_job("free", &[], "");
    fx.add_job("blocked", &["free"], "");

    let engine = fx.start();
    let long = "#[dim]workers #[nodim,fg=green]1#[default,dim]/1 · #[nodim,fg=yellow]1#[default,dim] waiting · 2 queued#[default]";
    wait_until("@cpd_workers", || fx.workers() == long);
    assert_eq!(fx.option("@cpd_workers_short"), "#[nodim,fg=green]1#[default,dim]/1·#[nodim,fg=yellow]1#[default,dim]·2#[default]");
    assert_eq!(fx.option("@cpd_workers_width"), "46", "inbox 0, one space, the long form, a pill's two caps each");
    assert_eq!(fx.option("@cpd_workers_short_width"), "19", "the same with the short form");
    engine.stop();
    drop(lock);
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

/// The Inbox counter: dim `inbox 0` with no item; two open, visible items, the most urgent red, make it red `inbox 2`,
/// and the width the status line compares counts it.
#[test]
fn the_engine_sets_cpd_inbox_from_the_open_visible_inbox_items() {
    let fx = fx("");
    fx.tmux_server();
    let engine = fx.start();
    wait_until("@cpd_inbox", || fx.option("@cpd_inbox") == "#[dim]inbox 0#[default]");

    // The Plan's Lead, in a live pane, asks the User in a dialog in Dispatch: E8 opens a red `dialog` item. (No pull
    // request row: the engine's own `pr poll` prunes the rows its fake `gh` does not list.)
    let pane = fx.env.tmux(&["display-message", "-p", "-t", "keepalive", "#{pane_id}"]);
    let pane = String::from_utf8(pane.stdout).unwrap().trim().to_owned();
    fx.fixture(&format!("UPDATE sessions SET state = 'blocked', state_label = 'dialog', state_at = 't2', tmux_pane = '{pane}' WHERE id = 'lead1';"));
    wait_until("@cpd_inbox red 1", || fx.option("@cpd_inbox") == "#[fg=red]inbox 1#[default]");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM inbox_items WHERE closed_at IS NULL AND kind = 'dialog'"), 1);
    assert_eq!(fx.option("@cpd_workers_width"), "46");
    engine.stop();
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
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

    let engine = fx.start();
    wait_until("three without a pane", || fx.count("session-moved") == 3);
    fx.env.wait_whole_ticks(1);
    engine.stop();

    let row = |id: &str| {
        fx.env.read(|c| Ok(c.query_row("SELECT state, tmux_pane, state_at FROM sessions WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
    };
    let rows: Vec<(String, Option<String>, String)> = ["s-live", "s-dead", "s-gone", "s-waiting", "s-shut"].iter().map(|id| row(id)).collect();
    let (states, panes): (Vec<&str>, Vec<Option<&str>>) = rows.iter().map(|r| (r.0.as_str(), r.1.as_deref())).unzip();
    assert_eq!(states, ["idle", "stopped", "stopped", "stopped", "stopped"]);
    assert_eq!(panes, [Some(live.as_str()), None, None, Some(waiting.as_str()), None]);
    // Already stopped: its state did not change, so neither did state_at.
    assert_eq!(rows[4].2, "t");
    assert_eq!(fx.env.query::<Option<String>>("SELECT state_label FROM sessions WHERE id = 's-dead'"), None);
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
    fx.env.wait_whole_ticks(2);
    let out = engine.stop();

    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
    assert_eq!(fx.env.query::<String>("SELECT state FROM sessions WHERE id = 's-gone'"), "stopped");
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
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts"), 1);
    assert_eq!((fx.job_status("web"), fx.job_status("ui")), ("queued".into(), "queued".into()));
    assert!(fx.env.query::<Option<i64>>("SELECT driver_pid FROM attempts WHERE id = 'api-a1'").is_some());
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

/// E3 starts urgent Ready Jobs first (§3.7): with every slot free, the urgent Job goes before an older non-urgent one.
#[test]
fn e3_starts_an_urgent_job_before_an_older_one() {
    let fx = fx("concurrency = 1\nretry_cap = 0");
    work_session_with_a_worker_that_stays(&fx);
    fx.add_job("old", &[], "");
    fx.add_job("hot", &[], &urgent("hot"));

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
    wait_until_for("the driver to end api-a1", Duration::from_secs(60), || fx.attempt("api-a1").0 == "ended");
    assert!(fx.env.query::<Option<i64>>("SELECT driver_pid FROM attempts WHERE id = 'api-a1'").is_some(), "the driver was not started");
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
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts"), 0);
    assert_eq!(fx.job_status("api"), "queued");
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

/// A Job whose latest 9 Attempts never had a driver, the last just ended: E3 waits 300 s, so no tick starts it.
#[test]
fn e3_does_not_start_a_job_in_its_backoff() {
    let fx = fx("");
    let now = ago(0);
    let attempts: String = (1..=9).map(|n| ended_attempt("api", n, "S2", None, &now)).collect();
    fx.add_job("api", &[], &attempts);

    let engine = fx.start();
    fx.env.wait_whole_ticks(3);
    engine.stop();

    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts"), 9);
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

    // A step: the second tick's E4 meets the lock, taken while E5 of the first tick is still running.
    fx.fixture("UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");
    let busy = watch_log(&fx).matches("store_busy: ").count();
    let held = hold_write_lock(&fx);
    fs::write(&go, "").unwrap();
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

    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    fx.env.wait_whole_ticks(2);
    engine.stop();

    assert_eq!(fx.events("all-merged"), [(Some("pay".into()), None, None, json!({"pr": "repo", "merged": ["api"], "cancelled": ["web"]}))]);
    // The text the Lead is steered with (hs-d334): open now, waiting for nothing.
    assert!(fx.texts("all-merged")[0].starts_with("No Job of PR repo of Plan pay is draft, queued or running. Open the pull request now: it waits for no slot and no other work."));
    assert!(fx.texts("all-merged")[0].ends_with("Next: cpd pr open --pr repo --plan pay"));
    assert!(fx.env.query::<Option<String>>("SELECT all_merged_at FROM prs WHERE plan = 'pay'").is_some());
}

#[test]
fn e4_sends_all_merged_again_after_a_job_is_appended_and_merged() {
    let fx = fx("");
    fx.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");

    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    let spec = fx.env.root().join("late-spec.json");
    fs::write(&spec, json!({"goal": "Do it.", "context": "Because.", "touches": ["late.txt"], "acceptance": ["ok"], "out_of_scope": []}).to_string()).unwrap();
    let added = fx.env.command(CPD).args(["job", "add", "late", "--title", "Late", "--tier", "S2", "--spec"]).arg(&spec).args(["--plan", "pay"]).output().unwrap();
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stdout));
    assert!(fx.env.query::<Option<String>>("SELECT all_merged_at FROM prs WHERE plan = 'pay'").is_none());
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

    let engine = fx.start();
    fx.env.wait_whole_ticks(3);
    engine.stop();
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts"), 0, "a draft never gets an Attempt");
    assert_eq!(fx.job_status("later"), "draft");
    assert_eq!(fx.count("all-merged"), 0, "the PR waits for its draft");
    assert!(fx.env.query::<Option<String>>("SELECT all_merged_at FROM prs WHERE plan = 'pay'").is_none());

    fx.env.cpd(&["job", "cancel", "later", "--reason", "Not needed.", "--plan", "pay"]).ok();
    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    engine.stop();
    assert_eq!(fx.events("all-merged")[0].3, json!({"pr": "repo", "merged": ["api"], "cancelled": ["later"]}));
}

// ---------- E4: a PR still unopened is escalated to the Manager (hs-d334) ----------

impl Fx {
    /// The Context's live Manager, Session `mgr1`.
    fn add_manager(&self) {
        self.fixture(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('mgr1', 'manager', 'ctx', NULL, 'Manager', '/', 'O5', 'idle', 't', 't');",
        );
    }

    /// Job `api` merged: the engine sends `all-merged`.
    fn add_merged_api(&self) {
        self.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");
    }

    /// Makes the PR's `all-merged` `minutes` old, as if that long had passed: the engine's only clock is the store's `all_merged_at`.
    fn age_all_merged(&self, minutes: i64) {
        self.fixture(&format!("UPDATE prs SET all_merged_at = '{}' WHERE plan = 'pay' AND key = 'repo';", ago(minutes)));
    }

    /// Every escalation to a Manager: `(context, plan, session, data)`.
    fn escalations(&self) -> Vec<(Option<String>, Option<String>, Option<String>, Value)> {
        self.store()
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT context, plan, session, data FROM events WHERE kind = 'session-message' ORDER BY id")?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows.into_iter().map(|(c, p, s, d)| (c, p, s, serde_json::from_str(&d).unwrap())).collect())
            })
            .unwrap()
    }
}

#[test]
fn e4_tells_the_manager_once_when_a_pr_is_still_unopened_ten_minutes_after_all_merged() {
    let fx = fx("");
    fx.add_manager();
    fx.add_merged_api();

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    // Within the window: nothing, however many ticks.
    fx.age_all_merged(9);
    fx.env.wait_whole_ticks(2);
    assert_eq!(fx.count("session-message"), 0);

    // Past it: the Manager gets one message naming the Plan, the PR and how long; the next ticks add nothing.
    fx.age_all_merged(11);
    wait_until("the escalation", || fx.count("session-message") == 1);
    fx.env.wait_whole_ticks(3);
    engine.stop();

    let sent = fx.escalations();
    assert_eq!(sent.len(), 1, "{sent:?}");
    let (context, plan, session, data) = &sent[0];
    assert_eq!((context.as_deref(), plan.as_deref(), session.as_deref()), (Some("ctx"), None, Some("mgr1")));
    let text = "Plan pay (Pay): PR repo has had all its Jobs merged for 11 minutes and its pull request is not opened. The Lead was told to run cpd pr open --pr repo and has not. Look at the Lead: cpd session show lead1, and tell it or the User.";
    assert_eq!(
        data,
        &json!({"from": "lead1", "text": text, "wake": true, "escalation": "pr_unopened", "plan": "pay", "pr": "repo", "minutes": 11})
    );
    assert_eq!(fx.texts("session-message"), [text]);
    assert_eq!(fx.count("error"), 0);
}

/// SQL that takes the PR out of `dispatch`, as `pr open`, its adoption of a merged pull request or `pr cancel` do.
fn opened(status: &str) -> String {
    const URL: &str = "https://github.com/acme/pay/pull/7";
    let pull_request = format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
           VALUES ('acme/pay', 7, '{URL}', 'Pay', 0, 'open', 'none', 'none', 't', 't');"
    );
    let number = format!("pr_number = 7, pr_url = '{URL}', pr_opened_at = 't', pull_request = (SELECT id FROM pull_requests WHERE number = 7)");
    let (before, set) = match status {
        "open" => (pull_request, number),
        // `dispatch` goes to `merged` through `open` (the adoption of a merged pull request does both).
        "merged" => (format!("{pull_request} UPDATE prs SET status = 'open', {number} WHERE plan = 'pay' AND key = 'repo';"), "merged_at = 't'".to_owned()),
        _ => (String::new(), "lead_worktree = NULL, started_at = NULL".to_owned()),
    };
    format!("{before} UPDATE prs SET status = '{status}', {set} WHERE plan = 'pay' AND key = 'repo';")
}

#[test]
fn e4_tells_no_one_of_a_pr_opened_after_all_merged_however_late() {
    for status in ["open", "merged", "cancelled"] {
        let fx = fx("");
        fx.add_manager();
        fx.add_merged_api();

        poll_every(&fx, 1);
        let engine = fx.start();
        wait_until("all-merged", || fx.count("all-merged") == 1);
        // Opened, and long past the window: the engine finds the PR opened, not late.
        fx.fixture(&format!("{} UPDATE prs SET all_merged_at = '{}' WHERE plan = 'pay' AND key = 'repo';", opened(status), ago(60)));
        fx.env.wait_whole_ticks(2);
        engine.stop();
        // (E5 may report its own error for the pull request the fake `gh` does not know, and the Manager hears of that
        // error, hs-d314: no message of E4's.)
        let escalations: Vec<_> = fx.escalations().into_iter().filter(|e| e.3["escalation"] == "pr_unopened").collect();
        assert!(escalations.is_empty(), "{status}: {escalations:?}");
        assert!(fx.events("error").iter().all(|e| e.3["code"] != "pr_unopened"), "{status}");
    }
}

#[test]
fn e4_counts_the_window_from_pr_open_escalate_minutes() {
    let fx = fx("pr_open_escalate_minutes = 5");
    fx.add_manager();
    fx.add_merged_api();

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    // 4 minutes is short of 5, 6 is past.
    fx.age_all_merged(4);
    fx.env.wait_whole_ticks(2);
    assert_eq!(fx.count("session-message"), 0);
    fx.age_all_merged(6);
    wait_until("the escalation", || fx.count("session-message") == 1);
    engine.stop();
    assert_eq!(fx.escalations()[0].3["minutes"], 6);
}

#[test]
fn e4_starts_a_new_window_when_a_fix_job_is_merged_after_the_escalation() {
    let fx = fx("");
    fx.add_manager();
    fx.add_merged_api();

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    fx.age_all_merged(11);
    wait_until("the escalation", || fx.count("session-message") == 1);

    // A fix Job is added (it clears `all_merged_at`) and merges: a second `all-merged`, a fresh window.
    let spec = fx.env.root().join("fix-spec.json");
    fs::write(&spec, json!({"goal": "Fix it.", "context": "Because.", "touches": ["fix.txt"], "acceptance": ["ok"], "out_of_scope": []}).to_string()).unwrap();
    let added = fx.env.command(CPD).args(["job", "add", "fix", "--title", "Fix", "--tier", "S2", "--spec"]).arg(&spec).args(["--plan", "pay"]).output().unwrap();
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stdout));
    fx.fixture("UPDATE jobs SET status = 'running' WHERE key = 'fix'; UPDATE jobs SET status = 'merged', merged_sha = 'def' WHERE key = 'fix';");
    wait_until("second all-merged", || fx.count("all-merged") == 2);
    fx.env.wait_whole_ticks(3);
    assert_eq!(fx.count("session-message"), 1, "the new window has not run out");

    fx.age_all_merged(12);
    wait_until("the second escalation", || fx.count("session-message") == 2);
    fx.env.wait_whole_ticks(3);
    engine.stop();
    assert_eq!(fx.count("session-message"), 2);
    assert_eq!(fx.escalations()[1].3["minutes"], 12);
}

#[test]
fn e4_a_restarted_engine_repeats_no_escalation() {
    let fx = fx("");
    fx.add_manager();
    fx.add_merged_api();

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    fx.age_all_merged(11);
    wait_until("the escalation", || fx.count("session-message") == 1);
    engine.stop();

    let engine = fx.start();
    fx.env.wait_whole_ticks(3);
    engine.stop();
    // The restarted engine tells the Manager how long it was down (contract §3.7, hs-d328): that message
    // is not an escalation, so only the others are counted.
    let escalations: Vec<String> = fx.texts("session-message").into_iter().filter(|t| !t.contains("The engine was down for ")).collect();
    assert_eq!(escalations.len(), 1, "{escalations:?}");
}

#[test]
fn e4_without_a_manager_gives_the_lead_one_error_instead() {
    let fx = fx("");
    fx.add_merged_api();

    poll_every(&fx, 1);
    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    fx.age_all_merged(11);
    wait_until("the error", || fx.count("error") == 1);
    fx.env.wait_whole_ticks(3);
    engine.stop();

    assert_eq!(fx.count("session-message"), 0);
    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, _, _, data) = &errors[0];
    assert_eq!(plan.as_deref(), Some("pay"));
    assert_eq!((&data["source"], &data["code"], &data["details"]), (&json!("watch"), &json!("pr_unopened"), &json!({"pr": "repo", "minutes": 11})));
}

// ---------- E4: a stacked PR's 10 minutes start when its base PR is open or merged (hs-d301) ----------

/// What the fake `gh` answers while PR 7 (`base_open`) is open: the search lists nothing, the lookup says "still open".
fn gh_pr7_open(fx: &Fx) {
    fx.env.fake_gh(&format!(
        "case \"$4\" in *'search(type'*) echo '{EMPTY_SEARCH}' ;; *) echo '{LOOKUP_OPEN}' ;; esac"
    ));
}

impl Fx {
    /// PR `top` of Plan `pay` in `dispatch` on `base` (a stacked PR when `base` is `cpd/pay`, the Lead Branch of PR
    /// `repo`), its one Job `web` merged: the engine sends `all-merged`.
    fn add_top(&self, base: &str) {
        self.fixture(&format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', 'top', '{}', '{base}', 'dispatch', 'cpd/pay/top', '/x/top', cpd_txn(), 't', 't');",
            self.repo.display()
        ));
        self.add_job_in("top", "web", &[], "UPDATE jobs SET status = 'running' WHERE key = 'web'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'web';");
    }

    /// Makes `top`'s `all-merged` `minutes` old.
    fn age_all_merged_top(&self, minutes: i64) {
        self.fixture(&format!("UPDATE prs SET all_merged_at = '{}' WHERE plan = 'pay' AND key = 'top';", ago(minutes)));
    }

    /// The base PR `repo` `open` since `minutes` ago (its pull request is PR 7, so use [`gh_pr7_open`]).
    fn base_open(&self, minutes: i64) {
        self.fixture(&format!("{} UPDATE prs SET pr_opened_at = '{}' WHERE plan = 'pay' AND key = 'repo';", opened("open"), ago(minutes)));
    }

    /// The base PR `repo` `merged`, `pr_opened_at` `opened_ago` minutes ago, `merged_at` `merged_ago` minutes ago.
    fn base_merged(&self, opened_ago: i64, merged_ago: i64) {
        self.fixture(&format!(
            "{} UPDATE prs SET pr_opened_at = '{}', merged_at = '{}' WHERE plan = 'pay' AND key = 'repo';",
            opened("merged"),
            ago(opened_ago),
            ago(merged_ago)
        ));
    }

    /// A base PR `base`, which `top` is stacked on (`cpd/pay/base`), not open: `waiting` (never started), `dispatch` or `cancelled`.
    fn base_not_ready(&self, status: &str) {
        let (state, worktree, started, failed) = match status {
            "waiting" => ("waiting", "NULL", "NULL", "'t'"),
            _ => ("dispatch", "'/x/base'", "'t'", "NULL"),
        };
        self.fixture(&format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, start_failed_at)
               VALUES ('pay', 'base', '{}', 'main', '{state}', 'cpd/pay/base', {worktree}, cpd_txn(), 't', {started}, {failed});",
            self.repo.display()
        ));
        if status == "cancelled" {
            self.fixture("UPDATE prs SET status = 'cancelled', lead_worktree = NULL, started_at = NULL WHERE plan = 'pay' AND key = 'base';");
        }
    }

    /// The escalations of E4 (`pr_unopened`) and the `pr_unopened` errors.
    fn unopened(&self) -> (Vec<Value>, usize) {
        let messages = self.escalations().into_iter().map(|e| e.3).filter(|d| d["escalation"] == "pr_unopened").collect();
        (messages, self.events("error").iter().filter(|e| e.3["code"] == "pr_unopened").count())
    }
}

/// `top` (on `base`) is all merged, `all_merged_ago` minutes ago, with a Manager and the base PR as `setup` leaves
/// it; the engine runs for a few whole ticks. Returns what E4 told the Manager.
fn stacked_run(base: &str, setup: impl Fn(&Fx), all_merged_ago: i64) -> Vec<Value> {
    let fx = fx("");
    fx.add_manager();
    fx.add_top(base);
    gh_pr7_open(&fx);
    setup(&fx);

    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    fx.age_all_merged_top(all_merged_ago);
    fx.env.wait_whole_ticks(3);
    engine.stop();
    let (messages, errors) = fx.unopened();
    assert_eq!(errors, 0);
    messages
}

/// `stacked_run` must say nothing for a base PR ready `before` minutes ago, and one message of `after` minutes for one ready `after` minutes ago.
fn the_window_starts_at_the_base_pr(setup: impl Fn(&Fx, i64)) {
    let text = "Plan pay (Pay): PR top has had all its Jobs merged for 11 minutes and its pull request is not opened. The Lead was told to run cpd pr open --pr top and has not. Look at the Lead: cpd session show lead1, and tell it or the User.";
    // Everything merged an hour ago: only the base PR's moment can say it is too early.
    let early = stacked_run("cpd/pay", |fx| setup(fx, 9), 60);
    assert!(early.is_empty(), "the base PR opened 9 minutes ago: {early:?}");
    let late = stacked_run("cpd/pay", |fx| setup(fx, 11), 60);
    assert_eq!(late.len(), 1, "{late:?}");
    // `minutes` is the age of the clock start (the base PR's moment), not of `all_merged_at` (60).
    assert_eq!(late[0], json!({"from": "lead1", "text": text, "wake": true, "escalation": "pr_unopened", "plan": "pay", "pr": "top", "minutes": 11}));
}

#[test]
fn e4_a_stacked_pr_is_escalated_ten_minutes_after_its_base_pr_opened_not_after_all_merged() {
    the_window_starts_at_the_base_pr(|fx, minutes| fx.base_open(minutes));
}

#[test]
fn e4_a_stacked_pr_counts_from_the_merge_of_a_merged_base_pr_and_from_its_pr_opened_at_when_it_has_one() {
    // `pr_opened_at`, not `merged_at` (the merge was long after): a merged PR always carries `pr_opened_at` (schema CHECK).
    the_window_starts_at_the_base_pr(|fx, minutes| fx.base_merged(minutes, 1));
}

#[test]
fn e4_a_stacked_pr_whose_all_merged_is_later_than_its_base_prs_opening_counts_from_all_merged() {
    // The base PR opened an hour ago: `all_merged_at` is the later moment.
    let early = stacked_run("cpd/pay", |fx| fx.base_open(60), 9);
    assert!(early.is_empty(), "{early:?}");
    let late = stacked_run("cpd/pay", |fx| fx.base_open(60), 11);
    assert_eq!(late.len(), 1, "{late:?}");
    assert_eq!(late[0]["minutes"], 11);
}

#[test]
fn e4_a_stacked_pr_whose_base_pr_is_not_open_raises_no_alarm_however_late() {
    for status in ["waiting", "dispatch", "cancelled"] {
        let sent = stacked_run("cpd/pay/base", |fx| fx.base_not_ready(status), 60);
        assert!(sent.is_empty(), "base PR {status}: {sent:?}");
    }
}

#[test]
fn e4_a_stacked_pr_with_no_manager_gives_the_lead_no_error_while_its_base_pr_is_not_open() {
    let fx = fx("");
    fx.add_top("cpd/pay");
    gh_pr7_open(&fx);

    let engine = fx.start();
    wait_until("all-merged", || fx.count("all-merged") == 1);
    fx.age_all_merged_top(60);
    fx.env.wait_whole_ticks(3);
    engine.stop();
    assert_eq!(fx.unopened(), (vec![], 0));
}

/// SQL for a `pr-base-changed` Event of PR `top` `minutes` ago, `from_pr` the base PR before.
fn base_changed(minutes: i64, from_pr: Option<&str>) -> String {
    let data = json!({"pr": "top", "from": "cpd/pay", "to": "main", "from_pr": from_pr, "to_pr": null, "by": "engine"});
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, text, data)
           VALUES (cpd_txn(), '{}', 'pr-base-changed', 'ctx', 'pay', 'PR top of Plan pay now targets main (was cpd/pay).', '{data}');",
        ago(minutes)
    )
}

#[test]
fn e4_a_pr_unstacked_after_its_base_pr_merged_counts_from_its_latest_pr_base_changed_with_from_pr() {
    for (minutes, told) in [(9, false), (11, true)] {
        // The PR now targets `main`; its base PR merged long ago and the repoint was `minutes` ago.
        let sent = stacked_run("main", |fx| fx.fixture(&format!("{}{}", base_changed(60, Some("pay/repo")), base_changed(minutes, Some("pay/repo")))), 60);
        assert_eq!(sent.len(), usize::from(told), "the base changed {minutes} minutes ago: {sent:?}");
        if told {
            assert_eq!(sent[0]["minutes"], 11);
        }
    }
}

#[test]
fn e4_a_pr_that_was_never_stacked_counts_from_all_merged_alone_whatever_its_base_changed() {
    // On `develop`, no PR's Lead Branch; a repoint with no base PR (`from_pr` null) is no candidate.
    let sent = stacked_run("develop", |fx| fx.fixture(&base_changed(1, None)), 11);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["minutes"], 11);
    let sent = stacked_run("develop", |_| {}, 9);
    assert!(sent.is_empty(), "{sent:?}");
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

/// Sets `tick_seconds` (else [`TICK_SECONDS`]).
fn tick_every(fx: &Fx, seconds: &str) {
    let config = fx.env.cpd_home().join("config.toml");
    let text = fs::read_to_string(&config).unwrap();
    let tick = format!("tick_seconds = {TICK_SECONDS}\n");
    assert!(text.contains(&tick), "{text}");
    fs::write(&config, text.replace(&tick, &format!("tick_seconds = {seconds}\n"))).unwrap();
}

/// Sets `pr_poll_seconds`, which E5 waits after a poll ended when no test override replaces it (`start_with` without
/// [`PR_POLL_EVERY_TICK`]).
fn poll_every(fx: &Fx, seconds: u32) {
    let config = fx.env.cpd_home().join("config.toml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(&config, text.replace("[watch]\n", &format!("[watch]\npr_poll_seconds = {seconds}\n"))).unwrap();
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
        r#"echo "start $(date +%s%N)" >> '{log}'; sleep 0.2; echo "end $(date +%s%N)" >> '{log}'; echo '{}'; exit 0"#,
        search_reply(4987, "2099-01-01T00:00:00Z"),
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

/// engine-W9: a poll of 0.1 s with a tick of 5 s and a poll period (`pr_poll_seconds`, its test override) of 0.2 s runs
/// every 0.3 s (end + 0.2 s), not at the next tick.
#[test]
fn e5_polls_at_its_due_time_not_at_the_next_tick() {
    let fx = fx_bare("");
    tick_every(&fx, "5");
    let log = fx.env.root().join("poll.log");
    fx.env.fake_gh(&format!(
        r#"echo "start $(date +%s%N)" >> '{log}'; sleep 0.1; echo '{}'; exit 0"#,
        search_reply(4987, "2099-01-01T00:00:00Z"),
        log = log.display()
    ));

    let engine = fx.start_with(Path::new(CPD), &[("CPD_TEST_PR_POLL", "0.2")]);
    wait_until("three polls", || fs::read_to_string(&log).map_or(0, |t| t.matches("start").count()) >= 3);
    engine.stop();

    let starts: Vec<i128> = fs::read_to_string(&log).unwrap().lines().map(|l| l.split_once(' ').unwrap().1.parse::<i128>().unwrap() / 1_000_000).collect();
    for pair in starts.windows(2) {
        let gap = pair[1] - pair[0];
        assert!((300..4000).contains(&gap), "a poll every 0.3 s (0.1 s poll + 0.2 s), not every 5 s tick: {starts:?}");
    }
    assert_eq!(fx.count("error"), 0);
}

/// The one test of `pr_poll_seconds` itself, the config key (no test override): whole seconds, so it waits one.
#[test]
fn e5_polls_with_a_plan_in_pr_too_and_does_not_poll_more_often_than_every_pr_poll_seconds() {
    let fx = fx("");
    in_pr(&fx);
    poll_every(&fx, 1);
    fx.env.fake_gh(&format!(
        "case \"$4\" in *'search(type'*) echo '{}' ;; *) echo '{LOOKUP_OPEN}' ;; esac",
        search_reply(4987, "2099-01-01T00:00:00Z")
    ));

    let engine = fx.start_with(Path::new(CPD), &[]);
    wait_until("the first poll", || fx.env.poll_searches() >= 1);
    let first = Instant::now();
    wait_until("the second poll", || fx.env.poll_searches() >= 2);
    let gap = first.elapsed();
    engine.stop();

    // Ten ticks of 0.1 s to a poll: a poll per tick would come within a few of them.
    assert!(gap >= Duration::from_millis(500), "one poll per pr_poll_seconds (1 s), not one per tick: {gap:?}");
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

/// The last line of [`GH_502`]'s stderr.
const GH_502_LINE: &str = "gh: HTTP 502: Bad Gateway (https://api.github.com/graphql)";

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

#[test]
fn e5_raises_one_error_after_ten_minutes_of_the_same_call_failing_and_none_before() {
    // Nine minutes of failing (the record the engine kept, as if it had polled that long): still nothing.
    let fx = fx("");
    fx.add_manager();
    fx.env.fake_gh(GH_502);
    fail_since(&fx, "search", 9);
    let engine = fx.start();
    fx.env.wait_whole_ticks(3);
    engine.stop();
    assert_eq!((fx.count("error"), messages(&fx)), (0, Vec::<String>::new()));
    assert_eq!(failing(&fx).unwrap()["call"], "search");

    // Eleven: one error Event for the machine, and one message to the Manager, not one per tick.
    let fx = fx_with_manager_failing(11);
    let since = failing(&fx).unwrap()["since"].clone();
    let engine = fx.start();
    wait_until("the error", || fx.count("error") >= 1);
    fx.env.wait_whole_ticks(4);
    engine.stop();

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!((plan, job, attempt), (&None, &None, &None));
    assert_eq!((&data["source"], &data["code"]), (&json!("watch"), &json!("gh_failed")));
    let details = &data["details"];
    assert_eq!((&details["call"], &details["since"], &details["minutes"], &details["reason"], &details["exit"]), (&json!("search"), &since, &json!(11), &json!("HTTP 502"), &json!(1)));
    assert_eq!(details["argv"].as_array().unwrap()[..3], ["gh", "api", "graphql"]);
    assert!(details["stderr"].as_str().unwrap().contains(GH_502_LINE), "{details}");

    let sent = messages(&fx);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with("Engine error at "), "{}", sent[0]);
    assert!(sent[0].contains(&format!("pr poll: gh search has failed for 11 minutes (HTTP 502): {GH_502_LINE}")), "{}", sent[0]);
    // The failure stays: the record keeps its first time, the polls (and their log lines) go on.
    assert_eq!(failing(&fx).unwrap()["since"], since);
    assert!(watch_log(&fx).matches("failed transiently (HTTP 502)").count() >= 2, "{}", watch_log(&fx));
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

#[test]
fn e5_a_cut_gh_response_is_an_error_only_after_the_window_and_then_once() {
    // Nine minutes of failing: still nothing.
    let fx = fx("");
    fx.add_manager();
    fx.env.fake_gh(GH_CUT);
    fail_since(&fx, "search", 9);
    let engine = fx.start();
    fx.env.wait_whole_ticks(3);
    engine.stop();
    assert_eq!((fx.count("error"), messages(&fx)), (0, Vec::<String>::new()));
    assert_eq!(failing(&fx).unwrap()["call"], "search");

    // Eleven: one error Event and one message to the Manager, not one per tick.
    let fx = fx_with_manager_failing(11);
    fx.env.fake_gh(GH_CUT);
    let since = failing(&fx).unwrap()["since"].clone();
    let engine = fx.start();
    wait_until("the error", || fx.count("error") >= 1);
    fx.env.wait_whole_ticks(4);
    engine.stop();

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let data = &errors[0].3;
    assert_eq!((&data["source"], &data["code"]), (&json!("watch"), &json!("gh_failed")));
    let details = &data["details"];
    assert_eq!(
        (&details["call"], &details["since"], &details["minutes"], &details["reason"], &details["exit"]),
        (&json!("search"), &since, &json!(11), &json!("unexpected end of JSON input"), &json!(1))
    );
    assert!(details["stderr"].as_str().unwrap().contains("unexpected end of JSON input"), "{details}");
    let sent = messages(&fx);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(
        sent[0].contains("pr poll: gh search has failed for 11 minutes (unexpected end of JSON input): unexpected end of JSON input"),
        "{}",
        sent[0]
    );
    assert_eq!(failing(&fx).unwrap()["since"], since);
}

#[test]
fn e5_a_failure_that_is_not_transient_is_an_error_at_once_and_clears_the_record() {
    for (name, script, line) in [
        ("a 401", "echo 'gh: HTTP 401: Bad credentials' >&2; exit 1", "gh: HTTP 401: Bad credentials"),
        ("an HTML page with no 5xx status", "echo '<html><body>nginx</body></html>'; echo 'gh: exit status 1' >&2; exit 1", "gh: exit status 1"),
    ] {
        let fx = fx("");
        fx.add_manager();
        fx.env.fake_gh(script);
        // A transient failure was recorded a minute ago; this one is not that.
        fail_since(&fx, "search", 1);
        let engine = fx.start();
        wait_until("the error", || fx.count("error") >= 1);
        fx.env.wait_whole_ticks(3);
        engine.stop();

        let errors = fx.events("error");
        assert_eq!(errors.len(), 1, "{name}: {errors:?}");
        let data = &errors[0].3;
        assert_eq!((&data["source"], &data["code"], &data["details"]["transient"], &data["details"]["call"]), (&json!("watch"), &json!("gh_failed"), &json!(false), &json!("search")), "{name}");
        assert!(data["details"]["stderr"].as_str().unwrap().contains(line), "{name}: {data}");
        assert_eq!(failing(&fx), None, "{name}: a failure that is not transient deletes the record");
        assert!(!watch_log(&fx).contains("failed transiently"), "{name}");
    }
}

/// Whether process `pid` is running (a zombie is not).
fn running(pid: i32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.rsplit(") ").next().is_some_and(|r| !r.starts_with('Z')))
}

/// engine-W4: a `cpd pr poll` child that hangs (its `gh` sleeps, and so does a child of
/// that `gh`) is killed at its deadline with its whole process group, and E5 reports
/// `tool_timeout`. The deadline is shortened to 0.5 s by `CPD_TEST_DEADLINE_CPD`, which a
/// debug build reads (contract §1.2 "Test overrides").
#[test]
fn e5_kills_a_hung_poll_child_and_its_group_at_the_deadline_with_tool_timeout() {
    let fx = fx_bare("");
    let pids = fx.env.root().join("gh.pids");
    fx.env.fake_gh(&format!("sleep {SLEEP_SECONDS} & echo \"$$ $!\" >> '{}'; wait", pids.display()));

    let engine = fx.start_with(Path::new(CPD), &[("CPD_TEST_DEADLINE_CPD", "0.5")]);
    wait_until("E5's tool_timeout", || fx.count("error") >= 1);
    engine.stop();

    let errors = fx.events("error");
    let data = &errors[0].3;
    assert_eq!((&data["source"], &data["code"]), (&json!("watch"), &json!("tool_timeout")), "{errors:?}");
    let details = &data["details"];
    assert_eq!((&details["tool"], &details["deadline_seconds"]), (&json!("cpd"), &json!(0.5)), "{details}");
    assert_eq!(details["argv"], json!([CPD, "pr", "poll"]));
    for pid in fs::read_to_string(&pids).unwrap().split_whitespace() {
        let pid: i32 = pid.parse().unwrap();
        wait_until(&format!("gh's process {pid} to end"), || !running(pid));
    }
}

#[test]
fn e5_pauses_below_100_requests_until_reset_and_reports_one_gh_rate_limited() {
    let fx = fx("");
    let log = fx.env.root().join("poll.log");
    let first = fx.env.root().join("first-poll-done");
    // The first poll leaves 50 requests until `reset_at`, 0.5 s after it (written to `reset-at`); the next one, which
    // E5 must not run before that (it polls every tick otherwise), 4987.
    let reset_file = fx.env.root().join("reset-at");
    fx.env.fake_gh(&format!(
        r#"echo "start $(date +%s%N)" >> '{log}'
if [ ! -e '{first}' ]; then
  touch '{first}'; ms=$(( $(date +%s%N) / 1000000 + 500 ))
  reset=$(date -u -d "@$((ms / 1000)).$(printf %03d $((ms % 1000)))" +%Y-%m-%dT%H:%M:%S.%3NZ); printf %s "$reset" > '{reset_file}'
  echo '{low}' | sed "s/RESET_AT/$reset/"
else echo '{ok}'; fi; exit 0"#,
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

#[test]
fn a_prerequisite_seen_merged_by_e5_still_lets_e7_start_its_dependent_in_the_same_tick() {
    let fx = fx("");
    // One tick only: whatever happens after the first poll happens in that tick.
    tick_every(&fx, "600");
    // The clean-up of the merged `repo` keeps a Lead worktree with commits not on origin (and raises an `error`): give
    // the repo an `origin` that has them, so the only `error` this test could see is E5's or E7's own.
    let origin = fx.env.root().join("repos/origin.git");
    fx.env.git(fx.env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    fx.env.local_origin(&fx.repo, &origin);
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
         INSERT INTO pr_needs (plan, pr, needs_plan, needs) VALUES ('pay', 'web', 'pay', 'repo');",
        repo = web.display()
    ));
    // The real `pr poll`, against a `gh` that finds PR 7 merged: the search lists nothing, the lookup answers MERGED.
    let merged = r#"{"repository":{"nameWithOwner":"acme/pay"},"number":7,"url":"https://github.com/acme/pay/pull/7","title":"Pay","isDraft":false,"state":"MERGED","createdAt":"t","mergedAt":"2026-10-14T10:00:00Z","reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","commits":{"nodes":[]}}"#;
    fx.env.fake_gh(&format!("case \"$4\" in *'search(type'*) echo '{EMPTY_SEARCH}' ;; *) echo '{{\"data\":{{\"p0\":{{\"pullRequest\":{merged}}}}}}}' ;; esac"));

    let engine = fx.start();
    wait_until("E7 to start web", || fx.count("pr-started") == 1);
    engine.stop();

    assert_eq!(fx.env.query::<String>("SELECT status FROM prs WHERE key = 'repo'"), "merged");
    assert_eq!(fx.env.query::<String>("SELECT status FROM prs WHERE key = 'web'"), "dispatch");
    assert!(fx.env.query::<i64>("SELECT id FROM events WHERE kind = 'pr-merged'") < fx.env.query::<i64>("SELECT id FROM events WHERE kind = 'pr-started'"));
    assert!(!fx.lead.exists(), "the poll that saw the merge cleaned up the Lead worktree");
    assert_eq!(fx.count("error"), 0);
}

// ---------- E10 ----------

/// What E10's tests share: the Fx, a tmux server with a live pane per Session (E2 stops an idle Session
/// without one), a Job `api` with its live driver and open Escalations that E1 expires at a tick: waiting for
/// two of them to be expired is waiting for a whole tick (E1 runs first in a tick, E10 last), so a test can
/// say "E10 has run, and started nothing" without sleeping on wall time.
struct Titles {
    fx: Fx,
    panes: Vec<String>,
    asked: u32,
    _driver: Flock<File>,
}

/// A Session file with one short conversation, as pi writes it.
fn conversation(fx: &Fx, id: &str) -> PathBuf {
    let path = fx.env.root().join(format!("{id}.jsonl"));
    let lines = [
        json!({"type": "session", "id": id}),
        json!({"type": "message", "message": {"role": "user", "content": "How do we title Sessions?"}}),
        json!({"type": "message", "message": {"role": "assistant", "content": [{"type": "text", "text": "With an auxiliary agent."}]}}),
    ];
    fs::write(&path, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n") + "\n").unwrap();
    path
}

impl Titles {
    /// `fx`, a pane per id in `panes`, pi answering `pi` (a shell body, its stdin read first).
    fn new(panes: usize, pi: &str) -> Titles {
        let fx = fx("");
        fx.env.fake_pi(&format!("cat > /dev/null\n{pi}"));
        let mut ids = Vec::new();
        for n in 0..panes {
            let args: &[&str] = if n == 0 { &["new-session", "-d", "-s", "x"] } else { &["new-window", "-d", "-t", "=x:"] };
            let out = fx.env.tmux(&[args, &["-P", "-F", "#{pane_id}", "sleep", "600"]].concat());
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            ids.push(String::from_utf8(out.stdout).unwrap().trim().to_owned());
        }
        let driver = fx.live_driver("api-a1");
        fx.add_running("api", "running", &ago(1));
        Titles { fx, panes: ids, asked: 0, _driver: driver }
    }

    /// Session `id` of `role` in `state`, on pane number `pane`, with its finished turn `turn` minutes ago (`None`:
    /// only its start, `stopped` to `idle`) and its title written `titled` minutes ago (`None`: no title).
    fn session(&self, id: &str, role: &str, state: &str, pane: usize, turn: Option<i64>, titled: Option<i64>) {
        let (title, titled_at) = titled.map_or(("NULL".to_owned(), "NULL".to_owned()), |m| ("'Old title'".to_owned(), format!("'{}'", ago(m))));
        let (from, at) = turn.map_or(("stopped", ago(60)), |m| ("working", ago(m)));
        self.fx.fixture(&format!(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_at, created_at, title, titled_at)
               VALUES ('{id}', '{role}', 'ctx', {plan}, {attempt}, '{id}', '{file}', '/', 'O3', '{pane}', '{state}', 't', 't', {title}, {titled_at});
             INSERT INTO events (txn, at, kind, context, session, text, data)
               VALUES (cpd_txn(), '{at}', 'session-state', 'ctx', '{id}', 'x', '{{\"from\":\"{from}\",\"label\":null,\"to\":\"idle\"}}');",
            file = conversation(&self.fx, id).display(),
            pane = self.panes[pane],
            plan = if role == "worker" { "'pay'" } else { "NULL" },
            attempt = if role == "worker" { "'api-a1'" } else { "NULL" }
        ));
    }

    /// Waits until the engine has run one whole tick since this call began: an Escalation E1 expires at its next
    /// tick, which ends with E10 (the one before it is expired in a tick that already ended, or in this one).
    fn tick(&mut self) {
        let dead = {
            let mut gone = std::process::Command::new("true").spawn().unwrap();
            let pid = gone.id();
            gone.wait().unwrap();
            pid
        };
        self.asked += 1;
        let n = self.asked;
        self.fx.fixture(&format!(
            "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
               VALUES ('pay', 'api-a1-q{n}', 'api-a1', {n}, 'Why?', 'open', {dead}, '{}');",
            ago(1)
        ));
        wait_until("the escalation expired", || {
            self.fx.env.query::<String>(&format!("SELECT status FROM escalations WHERE id = 'api-a1-q{n}'")) == "expired"
        });
    }

    /// Two whole ticks of E10 after now, so one of them began after this call.
    fn ticks(&mut self) {
        self.tick();
        self.tick();
    }

    /// The Sessions with a title written by a retitle run (`titled_at` moved), by id.
    fn retitled(&self) -> Vec<String> {
        self.fx
            .store()
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT session FROM events WHERE kind = 'session-titled' ORDER BY id")?;
                Ok(stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    fn started(&self) -> Vec<String> {
        let mut logs: Vec<String> = fs::read_dir(self.fx.env.cpd_home().join("run/titles"))
            .map(|d| d.map(|e| e.unwrap().file_name().to_string_lossy().trim_end_matches(".log").to_owned()).collect())
            .unwrap_or_default();
        logs.sort();
        logs
    }
}

/// An idle Lead or Manager whose turn ended after its title (and the title is 10 minutes old) is retitled; one titled
/// less than 10 minutes ago, one that is working, one that only started, one without a finished turn and a Worker are
/// not.
#[test]
fn e10_retitles_an_idle_session_whose_turn_ended_after_its_title_and_no_other() {
    let mut t = Titles::new(7, "echo 'Auxiliary agent titles'");
    t.session("due-manager", "manager", "idle", 0, Some(5), Some(20));
    t.session("due-untitled", "lead", "idle", 1, Some(1), None);
    t.session("throttled", "lead", "idle", 2, Some(1), Some(5));
    t.session("working", "lead", "working", 3, Some(1), Some(20));
    t.session("just-started", "lead", "idle", 4, None, None);
    t.session("title-is-newer", "lead", "idle", 5, Some(40), Some(20));
    t.session("worker", "worker", "idle", 6, Some(1), None);

    let engine = t.fx.start();
    wait_until("both retitled", || t.retitled().len() == 2);
    t.ticks();
    engine.stop();

    let mut retitled = t.retitled();
    retitled.sort();
    assert_eq!(retitled, ["due-manager", "due-untitled"]);
    assert_eq!(t.started(), ["due-manager", "due-untitled"]);
    assert_eq!(t.fx.env.calls("pi").len(), 2, "no Session is retitled twice: the new title is newer than its turn");
    let titles: Vec<String> = ["due-manager", "due-untitled", "throttled"]
        .iter()
        .map(|id| t.fx.env.query(&format!("SELECT title FROM sessions WHERE id = '{id}'")))
        .collect();
    assert_eq!(titles, ["Auxiliary agent titles", "Auxiliary agent titles", "Old title"]);
    assert_eq!(t.fx.count("error"), 0);
}

/// At most 2 retitles at once: a third waits for one to end, and then runs.
#[test]
fn e10_runs_at_most_two_retitles_at_once() {
    let release = std::env::temp_dir().join(format!("e10-release-{}", std::process::id()));
    let mut t = Titles::new(
        3,
        &format!("i=0\nuntil [ -e '{r}' ] || [ $i -ge 1200 ]; do sleep 0.1; i=$((i+1)); done\necho 'A title'", r = release.display()),
    );
    for (n, id) in ["s1", "s2", "s3"].iter().enumerate() {
        t.session(id, "lead", "idle", n, Some(1), None);
    }

    let engine = t.fx.start();
    wait_until("two pi runs", || t.fx.env.calls("pi").len() == 2);
    t.ticks();
    assert_eq!((t.fx.env.calls("pi").len(), t.started()), (2, vec!["s1".to_owned(), "s2".to_owned()]), "the third waits");

    fs::write(&release, "").unwrap();
    wait_until("all three titled", || t.retitled().len() == 3);
    engine.stop();
    fs::remove_file(&release).unwrap();
    assert_eq!(t.fx.env.calls("pi").len(), 3);
    assert_eq!(t.started(), ["s1", "s2", "s3"]);
    assert_eq!(t.fx.count("error"), 0);
}

/// A run E10 did not start counts: a held `title-<session>.lock` is a run in flight, so its Session is skipped and
/// it takes one of the two places.
#[test]
fn e10_skips_a_session_whose_title_lock_is_held_and_counts_held_locks() {
    let mut t = Titles::new(3, "echo 'A title'");
    for (n, id) in ["s1", "s2", "s3"].iter().enumerate() {
        t.session(id, "lead", "idle", n, Some(1), None);
    }
    let hold = |id: &str| {
        let path = t.fx.home().locks_dir().join(format!("title-{id}.lock"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).unwrap();
        file.lock().unwrap();
        file
    };
    let (one, two) = (hold("s1"), hold("s2"));

    let engine = t.fx.start();
    t.ticks();
    assert_eq!(t.fx.env.calls("pi").len(), 0, "two are held: no place for s3");
    drop(one);
    wait_until("s1 and s3 titled", || t.retitled().len() == 2);
    t.ticks();
    engine.stop();
    drop(two);

    let mut retitled = t.retitled();
    retitled.sort();
    assert_eq!(retitled, ["s1", "s3"], "s2 is held until the end");
    assert_eq!(t.fx.env.calls("pi").len(), 2);
}

/// A failed retitle writes nothing, is reported once however many ticks retry it, and the report names no
/// Session or Plan (no Lead gets a turn). A title less than 10 minutes old is not retried at all.
#[test]
fn e10_reports_a_failed_retitle_once_and_retries_only_what_is_due() {
    let mut t = Titles::new(2, "echo 'pi: boom' >&2\nexit 1");
    t.session("due", "lead", "idle", 0, Some(1), Some(20));
    t.session("throttled", "lead", "idle", 1, Some(1), Some(5));

    let engine = t.fx.start();
    wait_until("the failure is reported", || t.fx.count("error") == 1);
    wait_until("a retry", || t.fx.env.calls("pi").len() >= 2);
    t.ticks();
    engine.stop();

    assert_eq!(t.started(), ["due"]);
    let errors = t.fx.events("error");
    assert_eq!(errors.len(), 1, "reported once: {errors:?}");
    let data = &errors[0].3;
    assert_eq!((data["source"].as_str(), data["code"].as_str()), (Some("watch"), Some("title_failed")));
    assert_eq!(data["details"]["session"], "due");
    assert!(data["details"]["tail"].as_str().unwrap().contains("pi: boom"), "{data}");
    assert!(data["details"]["log"].as_str().unwrap().ends_with("run/titles/due.log"), "{data}");
    assert_eq!(t.fx.env.query::<Option<String>>("SELECT session FROM events WHERE kind = 'error'"), None);
    assert_eq!(t.fx.env.query::<Option<String>>("SELECT context FROM events WHERE kind = 'error'"), None);
    assert_eq!(t.retitled(), Vec::<String>::new());
    assert_eq!(t.fx.env.query::<String>("SELECT title FROM sessions WHERE id = 'due'"), "Old title");
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

    let engine = fx.start();
    fx.env.wait_whole_ticks(2);
    let run = engine.stop();

    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM contexts WHERE id = 'demo'"), 0);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM context_records"), 0);
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
watch) tmux -L '{socket}' list-windows -t =cpd-work -F '#{{window_name}}' > '{windows}' 2>&1
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

/// Counts, until `stop` is set, the polls that find `manifest` missing, and all polls: `(missing, polls)`.
fn watch_missing(manifest: PathBuf) -> (std::sync::Arc<std::sync::atomic::AtomicBool>, std::thread::JoinHandle<(u32, u64)>) {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handle = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let (mut missing, mut polls) = (0u32, 0u64);
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                polls += 1;
                if !manifest.exists() {
                    missing += 1;
                }
            }
            (missing, polls)
        })
    };
    (stop, handle)
}

#[test]
fn install_replaces_an_installed_adapter_as_a_whole_and_it_is_never_missing() {
    let env = TestEnv::new();
    let share = env.home().join(".local/share/cpd");
    let adapter = share.join("pi");
    // An installed adapter as an earlier install left it: `pi` -> `pi.<id>`. (One `Checkout` per `TestEnv`, so the
    // first version is laid out by hand; the install under watch swaps the link over it.)
    let first = "pi.20200101T000000-1";
    fs::create_dir_all(share.join(first)).unwrap();
    fs::write(share.join(first).join("package.json"), "{}").unwrap();
    fs::write(share.join(first).join("stale.ts"), "").unwrap();
    std::os::unix::fs::symlink(first, &adapter).unwrap();
    let (stop, watcher) = watch_missing(adapter.join("package.json"));
    let (out, _) = install_with(&env, OPENS);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let (missing, _polls) = watcher.join().unwrap();

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(missing, 0, "the adapter's package.json was missing {missing} times during the swap of a symlink");
    assert_ne!(fs::read_link(&adapter).unwrap().to_string_lossy(), first, "pi points to the new version");
    assert!(!adapter.join("stale.ts").exists(), "the old adapter was not replaced as a whole");
    assert!(adapter.join("package.json").exists());
}

#[test]
fn install_over_a_real_adapter_directory_misses_it_for_a_tiny_fraction_of_polls_at_most() {
    let env = TestEnv::new();
    let share = env.home().join(".local/share/cpd");
    let adapter = share.join("pi");
    fs::create_dir_all(adapter.join("old")).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fs::write(adapter.join("stale.ts"), "").unwrap();
    // A big old adapter: the migration renames it, so its size does not widen the window.
    for i in 0..5000 {
        fs::write(adapter.join("old").join(format!("f{i}.ts")), "").unwrap();
    }

    let (stop, watcher) = watch_missing(adapter.join("package.json"));
    let (out, _) = install_with(&env, OPENS);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let (missing, polls) = watcher.join().unwrap();

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // The migration is two renames of one process, back to back (docs/implementing.md "The adapter swap"):
    // the window holds no process start-up, so a poller finds `pi` missing in a tiny fraction of its polls (at most 1%;
    // with the renames in two processes it is the start-up of python3, a tenth of the install), once per machine.
    assert!(u64::from(missing) * 100 <= polls, "the adapter's package.json was missing in {missing} of {polls} polls during the migration");
    assert!(fs::symlink_metadata(&adapter).unwrap().file_type().is_symlink(), "pi is a symlink afterwards");
    assert!(!adapter.join("stale.ts").exists() && !adapter.join("old").exists(), "the old adapter was not replaced as a whole");
    assert!(share.join("pi.legacy/stale.ts").exists(), "pi.legacy holds the old files");
    assert!(adapter.join("package.json").exists());
}
