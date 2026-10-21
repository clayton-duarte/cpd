//! The engine, `cpd watch` (contract §3.7, §6.6; commands.md §12): the lock
//! and its refusal, the signals, `--ensure`, and each step E1–E6 (E7, the PR start, is in pr_start.rs) on a
//! prepared database, with a private tmux server and fake `pi` and `cpd`.

use crate::common;
#[path = "../common/checkout.rs"]
mod checkout;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{CPD, PR_POLL_EVERY_TICK, Run, SLEEP_SECONDS, TICK_SECONDS, TestEnv, copy_executable, wait_until, wait_until_for};
use cpd_core::clock;
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

    /// The Manager `mgr1` got no downtime message: a restart is in the `watch-started` Event and the log only (hs-d514).
    fn manager_heard_nothing(&self) {
        assert_eq!(self.messages(), [], "a restart sends no session-message");
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
    fx.manager_heard_nothing();
}

#[test]
fn an_engine_that_got_sigkill_is_back_within_seconds() {
    let fx = fx("");
    fx.manager("mgr1");
    let pid = fx.supervised();
    kill(Pid::from_raw(pid as i32), Signal::SIGKILL).unwrap();
    fx.back_after(1);
    fx.manager_heard_nothing();
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
    fx.manager_heard_nothing();
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
fn a_restart_sends_no_downtime_notice() {
    let fx = fx("");
    fx.manager("mgr1");
    let first = fx.start();
    let _ = first.stop();
    let second = fx.start();
    let stopped_at = fx.env.query::<String>("SELECT at FROM events WHERE kind = 'watch-stopped'");
    // The record is the Event's text, not a message (hs-d514: the Managers are not told of a restart).
    assert!(fx.texts("watch-started")[1].contains(&format!("it stopped at {stopped_at}")), "{:?}", fx.texts("watch-started"));
    assert_eq!(fx.messages(), [], "no Manager is sent the downtime");
    let _ = second.stop();
}

#[test]
fn an_engine_that_died_without_a_watch_stopped_sends_no_notice() {
    let fx = fx("");
    fx.manager("mgr1");
    let first = fx.start();
    first.signal(Signal::SIGKILL);
    let mut first = first;
    let _ = first.child.take().unwrap().wait();
    let lock = fx.env.cpd_home().join("locks/watch.lock");
    wait_until("the killed engine's lock to be free", || {
        let file = OpenOptions::new().read(true).write(true).open(&lock).unwrap();
        Flock::lock(file, FlockArg::LockExclusiveNonblock).is_ok()
    });
    let second = fx.start();
    fx.env.wait_whole_ticks(2);
    assert_eq!(fx.messages(), []);
    assert!(fx.texts("watch-started")[1].contains("it was last alive"), "{:?}", fx.texts("watch-started"));
    let _ = second.stop();
}

#[test]
fn an_engine_error_sends_no_message_to_a_manager() {
    let fx = fx("");
    fx.add_manager();
    fx.add_job_on("O3", "repo", "api", &[], "");
    let engine = fx.start();
    wait_until("the error", || fx.count("error") == 1);
    fx.env.wait_whole_ticks(2);
    engine.stop();
    assert_eq!(fx.events("error").len(), 1);
    assert_eq!(fx.messages(), [], "an engine error is an Event only (hs-d514)");
}

/// hs-d514: a stuck Job is blocked work, so its message to the Context's Manager wakes it.
#[test]
fn a_stuck_job_wakes_its_context_manager() {
    let fx = fx("[inbox]\nstuck_job_minutes = 1\n");
    fx.add_manager();
    fx.add_job(
        "api",
        &[],
        "UPDATE jobs SET status = 'running' WHERE key = 'api';
         UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE key = 'api';",
    );
    fx.fixture(&format!(
        "INSERT INTO events (txn, at, kind, context, plan, job, text, data)
         VALUES (cpd_txn(), '{}', 'job-failed', 'ctx', 'pay', 'api', 'x', '{{}}');",
        ago(2)
    ));
    let engine = fx.start();
    wait_until("the message", || !fx.messages().is_empty());
    engine.stop();
    let messages = fx.messages();
    assert_eq!(messages.len(), 1, "{messages:?}");
    let (session, _, data) = &messages[0];
    assert_eq!((session.as_str(), data["from"].clone(), data["wake"].clone()), ("mgr1", Value::Null, json!(true)), "{data}");
    assert!(data["text"].as_str().unwrap().starts_with("Job pay/api failed "), "{data}");
}

// ---------- @cpd_workers ----------

// Each fixture's dispatch PR has its `open` Job, which E4 adds at the first tick and which waits for the PR's other Jobs: one queued Job.
const NO_WORKERS: &str = "#[dim]workers #[nodim,fg=colour8]0#[default,dim]/3 · 0 waiting · 1 queued#[default]";
const ONE_WORKER: &str = "#[dim]workers #[nodim,fg=green]1#[default,dim]/3 · 0 waiting · 1 queued#[default]";

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
    assert_eq!(fx.option("@cpd_workers_short"), "#[nodim,fg=green]1#[default,dim]/3·0·1#[default]");
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
    // Ready but the only slot is taken (E3 starts nothing); the other needs it, so it is queued and not waiting. The PR's `open` Job, which
    // E4 adds at the first tick, waits for them all: queued too.
    fx.add_job("free", &[], "");
    fx.add_job("blocked", &["free"], "");

    let engine = fx.start();
    let long = "#[dim]workers #[nodim,fg=green]1#[default,dim]/1 · #[nodim,fg=yellow]1#[default,dim] waiting · 3 queued#[default]";
    wait_until("@cpd_workers", || fx.workers() == long);
    assert_eq!(fx.option("@cpd_workers_short"), "#[nodim,fg=green]1#[default,dim]/1·#[nodim,fg=yellow]1#[default,dim]·3#[default]");
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

/// hs-d506: a store written before the rule may hold a queued Job on an `O` or effort-5 Tier. E3 never starts it:
/// one `error` Event (`tier_not_allowed`) per Job, once, the Job stays `queued`, no Attempt is made.
#[test]
fn e3_does_not_start_a_job_on_an_o_or_effort_5_tier_and_raises_one_error_per_job() {
    let fx = fx("");
    for (key, tier) in [("api", "O3"), ("web", "S5")] {
        fx.add_job_on(tier, "repo", key, &[], "");
    }

    let engine = fx.start();
    wait_until("two errors", || fx.count("error") == 2);
    fx.env.wait_whole_ticks(2);
    engine.stop();

    let errors = fx.events("error");
    assert_eq!(errors.len(), 2, "{errors:?}");
    let mut jobs_named: Vec<_> = errors.iter().map(|(plan, job, attempt, _)| (plan.clone(), job.clone().unwrap(), attempt.clone())).collect();
    jobs_named.sort();
    assert_eq!(jobs_named, [(Some("pay".into()), "api".into(), None), (Some("pay".into()), "web".into(), None)]);
    for (_, _, _, data) in &errors {
        assert_eq!((&data["source"], &data["code"]), (&json!("watch"), &json!("tier_not_allowed")), "{data}");
    }
    assert!(fx.texts("error").iter().all(|t| t.contains("hs-d506")), "{:?}", fx.texts("error"));
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts"), 0);
    assert_eq!((fx.job_status("api"), fx.job_status("web")), ("queued".into(), "queued".into()));
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

// ---------- E3: a draft ----------

/// A draft added after the lock (`job add --draft`, contract §3.2): E3 never starts it, and the PR's open Job waits while it remains (§3.2
/// Ready: the PR waits for the batch). Cancelling it frees the open Job: `the_open_job_is_ready_only_when_...` below.
#[test]
fn e3_never_starts_a_draft_and_the_open_job_waits_while_one_remains() {
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
    assert_eq!(fx.count("all-merged"), 0, "all-merged is retired");
    assert_eq!(fx.jobs_of_kind("open"), [("open-repo".to_owned(), "queued".to_owned())], "the open Job is added, and waits for the draft");
}

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

/// E4 adds the PR's `open` Job once the PR is `dispatch`, whatever its other Jobs are doing: `open-repo`, Tier `open_tier` (S2),
/// `queued`, `appended`, no Dependency rows (its Ready rule is derived), and one `job-added` Event, from the engine. The ticks
/// that follow add nothing. (A draft of the PR keeps it from being Ready, so E3 starts nothing here.)
#[test]
fn e4_adds_one_open_job_to_a_dispatch_pr_whatever_its_other_jobs_are_doing_and_adds_no_second() {
    let fx = fx("");
    fx.add_merged_api();
    fx.add_draft("later");

    let engine = fx.start();
    wait_until("the open Job", || !fx.jobs_of_kind("open").is_empty());
    fx.env.wait_whole_ticks(3);
    engine.stop();

    assert_eq!(fx.jobs_of_kind("open"), [("open-repo".to_owned(), "queued".to_owned())]);
    let row: (String, String, String, i64, i64, bool) = fx.env.read(|conn| {
        Ok(conn.query_row(
            "SELECT title, tier, pr, appended, urgent, fingerprint IS NOT NULL FROM jobs WHERE plan = 'pay' AND key = 'open-repo'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )?)
    });
    assert_eq!(row, (format!("Open the pull request: {PR_TITLE}"), "S2".into(), "repo".into(), 1, 0, true));
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM dependencies WHERE plan = 'pay' AND job = 'open-repo'"), 0, "its needs are derived");
    let added = fx.events("job-added");
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!((&added[0].0, &added[0].1), (&Some("pay".to_owned()), &Some("open-repo".to_owned())));
    let data = &added[0].3;
    assert_eq!((&data["kind"], &data["by"], &data["pr"], &data["status"], &data["tier"]), (&json!("open"), &json!("engine"), &json!("repo"), &json!("queued"), &json!("S2")));
    assert_eq!(data["needs"], json!([]));
    assert_eq!(fx.count("all-merged"), 0, "the all-merged steer is retired");
    assert_eq!(fx.env.query::<Option<String>>("SELECT all_merged_at FROM prs WHERE plan = 'pay'"), None);
}

/// `open_tier` is a key of its own (§8.2): the Job takes its Tier from it.
#[test]
fn e4_gives_the_open_job_the_tier_open_tier_names() {
    let fx = fx("open_tier = \"H1\"");
    fx.add_merged_api();
    fx.add_draft("later");

    let engine = fx.start();
    wait_until("the open Job", || !fx.jobs_of_kind("open").is_empty());
    engine.stop();

    assert_eq!(fx.env.query::<String>("SELECT tier FROM jobs WHERE plan = 'pay' AND key = 'open-repo'"), "H1");
}

/// A PR that has an open Job `queued` or `failed` gets no second one (a `failed` one waits for the Lead: `job retry`, or `job cancel`).
/// PR `web`, which has none, gets its own in the same ticks: the engine has looked at both.
#[test]
fn e4_adds_no_open_job_to_a_pr_whose_open_job_is_queued_or_failed() {
    for then in ["", "UPDATE jobs SET status = 'running' WHERE key = 'open-repo'; UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE key = 'open-repo';"] {
        let fx = fx("");
        fx.add_merged_api();
        fx.add_draft("later");
        fx.add_cpd_job("open-repo", "open", then);
        fx.fixture(&format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
               VALUES ('pay', 'web', '{}', 'main', 'dispatch', 'cpd/pay/web', '{}', 'Show the receipt', 'config', cpd_txn(), 't', 't');",
            fx.repo.display(),
            fx.lead.display()
        ));
        fx.add_job_in("web", "ui", &[], "UPDATE jobs SET status = 'running' WHERE key = 'ui'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'ui';");
        fx.add_draft_in("web", "later-web");

        let engine = fx.start();
        wait_until("the open Job of PR web", || fx.jobs_of_kind("open").iter().any(|(key, _)| key == "open-web"));
        fx.env.wait_whole_ticks(3);
        engine.stop();

        let open: Vec<String> = fx.jobs_of_kind("open").into_iter().map(|(key, _)| key).collect();
        assert_eq!(open, ["open-repo", "open-web"], "{then:?}");
        assert_eq!(fx.count("job-added"), 1, "{then:?}: only PR web's was added");
    }
}

/// A `merged` open Job on a PR that is still `dispatch` (its merge died before it opened the pull request, §7.4 R) counts for nothing:
/// the next tick adds a fresh one, `open-repo-2`. Only a `merged` one: a `cancelled` one is the Lead's and stands (next tests).
#[test]
fn e4_adds_a_fresh_open_job_after_a_merged_one_on_a_pr_still_dispatch() {
    let fx = fx("");
    fx.add_merged_api();
    fx.add_draft("later");
    fx.add_cpd_job("open-repo", "open", "UPDATE jobs SET status = 'running' WHERE key = 'open-repo'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'open-repo';");

    let engine = fx.start();
    wait_until("a fresh open Job", || fx.jobs_of_kind("open").len() == 2);
    fx.env.wait_whole_ticks(3);
    engine.stop();

    assert_eq!(fx.jobs_of_kind("open"), [("open-repo".to_owned(), "merged".to_owned()), ("open-repo-2".to_owned(), "queued".to_owned())]);
    assert_eq!(fx.count("job-added"), 1);
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

/// The Lead's cancel of a PR's `open` Job stands (§3.7 E4, hs-d301): the PR stays `dispatch` and no `open-repo-2` follows, over several
/// ticks and across an engine restart. Seen: Plan ui-design's `open-proto` was cancelled and `open-proto-2` came four seconds later.
#[test]
fn e4_adds_no_open_job_to_a_dispatch_pr_whose_open_job_the_lead_cancelled() {
    let fx = fx_cancelled_open_job();

    for _ in 0..2 {
        let engine = fx.start();
        wait_until("the open Job of PR web", || fx.jobs_of_kind("open").iter().any(|(key, _)| key == "open-web"));
        fx.env.wait_whole_ticks(3);
        engine.stop();
    }

    assert_eq!(fx.jobs_of_kind("open"), [("open-repo".to_owned(), "cancelled".to_owned()), ("open-web".to_owned(), "queued".to_owned())]);
    assert_eq!(fx.count("job-added"), 1, "only PR web's was added");
    assert_eq!(fx.env.query::<String>("SELECT status FROM prs WHERE plan = 'pay' AND key = 'repo'"), "dispatch");
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

/// Only a PR that is `dispatch`, of a Plan that is not archived, gets one.
#[test]
fn e4_adds_no_open_job_to_a_waiting_pr_or_to_a_pr_of_an_archived_plan() {
    // A PR that is still `waiting`, beside the PR that is `dispatch`.
    let waiting = fx("");
    waiting.add_draft("later");
    waiting.fixture(&format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, title, shape, created_txn, created_at)
           VALUES ('pay', 'web', '{}', 'main', 'waiting', 'cpd/pay/web', 'Show the receipt', 'config', cpd_txn(), 't');
         INSERT INTO pr_needs (plan, pr, needs_plan, needs) VALUES ('pay', 'web', 'pay', 'repo');",
        waiting.repo.display()
    ));
    let engine = waiting.start();
    wait_until("the open Job of the dispatch PR", || !waiting.jobs_of_kind("open").is_empty());
    waiting.env.wait_whole_ticks(3);
    engine.stop();
    assert_eq!(waiting.jobs_of_kind("open").len(), 1);
    assert_eq!(waiting.env.query::<String>("SELECT pr FROM jobs WHERE kind = 'open'"), "repo");

    // The Plan is archived: its dispatch PR gets none.
    let archived = fx_archived();
    let engine = archived.start();
    archived.env.wait_whole_ticks(3);
    engine.stop();
    assert_eq!(archived.jobs_of_kind("open"), Vec::<(String, String)>::new());
}

fn fx_archived() -> Fx {
    let fx = fx("");
    fx.add_merged_api();
    fx.fixture("UPDATE plans SET archived_at = 't' WHERE id = 'pay';");
    fx
}

/// The work session as tui leaves it and a Worker that stays: an Attempt E3 starts shows as `attempt-started`.
/// An open Job is Ready only when every other Job of its PR is `merged` or `cancelled` and one is `merged` (§3.2): a `draft`, a
/// `queued`, a `running` or a `failed` Job keeps it back, and so do Jobs that are all `cancelled`.
#[test]
fn the_open_job_is_ready_only_when_every_other_job_is_merged_or_cancelled_and_one_is_merged() {
    // A draft holds it; cancelling the draft (`job cancel`) frees it: E3 starts `open-repo-a1`.
    let draft = fx("");
    work_session_with_a_worker_that_stays(&draft);
    draft.add_merged_api();
    draft.add_draft("later");
    let engine = draft.start();
    wait_until("the open Job", || !draft.jobs_of_kind("open").is_empty());
    draft.env.wait_whole_ticks(3);
    assert_eq!(draft.count("attempt-started"), 0, "a draft holds the open Job back");
    draft.env.cpd(&["job", "cancel", "later", "--reason", "Not needed.", "--plan", "pay"]).ok();
    wait_until("the open Job's Attempt", || draft.count("attempt-started") == 1);
    engine.stop();
    assert_eq!(draft.events("attempt-started")[0].1, Some("open-repo".to_owned()));
    assert_eq!(draft.job_status("open-repo"), "running");

    // A `failed` Job holds it too, as it holds a Dependent back.
    let failed = fx("");
    work_session_with_a_worker_that_stays(&failed);
    failed.add_merged_api();
    failed.add_job("broken", &[], "UPDATE jobs SET status = 'running' WHERE key = 'broken'; UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE key = 'broken';");
    let engine = failed.start();
    wait_until("the open Job", || !failed.jobs_of_kind("open").is_empty());
    failed.env.wait_whole_ticks(3);
    engine.stop();
    assert_eq!(failed.count("attempt-started"), 0, "a failed Job holds the open Job back");

    // No Job merged: a PR whose Jobs are all cancelled has nothing to open.
    let cancelled = fx("");
    work_session_with_a_worker_that_stays(&cancelled);
    cancelled.add_job("api", &[], "UPDATE jobs SET status = 'cancelled', cancel_reason = 'no', cancel_requested_at = 't' WHERE key = 'api';");
    let engine = cancelled.start();
    wait_until("the open Job", || !cancelled.jobs_of_kind("open").is_empty());
    cancelled.env.wait_whole_ticks(3);
    engine.stop();
    assert_eq!(cancelled.count("attempt-started"), 0, "no Job merged: the open Job is not Ready");
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
    assert_eq!(row, (format!("Bring the pull request up to date: {PR_TITLE}"), "S2".into(), 1, true));
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

    assert_eq!(messages(&fx), Vec::<String>::new(), "an engine error is an Event only (hs-d514)");
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
    assert_eq!(messages(&fx), Vec::<String>::new(), "an engine error is an Event only (hs-d514)");
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
        Titles::with_pi(panes, |_| pi.to_owned())
    }

    /// [`Titles::new`] with pi's script made from the env's root (where it keeps its state).
    fn with_pi(panes: usize, pi: impl FnOnce(&Path) -> String) -> Titles {
        let fx = fx("");
        fx.env.fake_pi(&format!("cat > /dev/null\n{}", pi(fx.env.root())));
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

    /// `fx.start()` with the 10 minutes a Session's titling must fail in a row before E10 reports it (contract §3.7 E10,
    /// hs-d356) set to `seconds`: `CPD_TEST_TITLE_FAILING`, a test override (contract §1.2) the fix adds to its table.
    fn start_reporting_after(&self, seconds: &str) -> Engine {
        let vars: Vec<(&str, &str)> = common::PLAN_POLLS.iter().copied().chain([("CPD_TEST_TITLE_FAILING", seconds)]).collect();
        self.fx.start_with(Path::new(CPD), &vars)
    }

    /// The answers of a fake pi (the first argument of [`Titles::new`]) in the real order of Session 4965bfc8
    /// (hs-d356): its first call answers nothing, every later one the model's preamble, never a `TITLE:` line.
    fn empty_then_preamble(root: &Path) -> String {
        format!(
            "calls='{}/pi-calls'\necho x >> \"$calls\"\n[ \"$(wc -l < \"$calls\")\" -lt 2 ] || echo '{PREAMBLE}'",
            root.display()
        )
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
    let mut t = Titles::new(7, "echo 'TITLE: Auxiliary agent titles'");
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
        &format!("i=0\nuntil [ -e '{r}' ] || [ $i -ge 1200 ]; do sleep 0.1; i=$((i+1)); done\necho 'TITLE: A title'", r = release.display()),
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
    let mut t = Titles::new(3, "echo 'TITLE: A title'");
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

    let engine = t.start_reporting_after("0");
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

/// pi's answer to E10's retry of Session 4965bfc8 at 15:05:37Z: a preamble, which the old code wrote as the title.
const PREAMBLE: &str = "Now I understand. I need to output just the title to stdout…";

/// An empty answer and then the preamble (the two real answers, hs-d356) are no title and, while the Session's titling has
/// failed for less than 10 minutes in a row (here: the limit, set to an hour), no `error` Event either: the Session stays
/// due and E10 retries it at the next tick.
#[test]
fn e10_does_not_report_an_empty_answer_or_a_preamble_within_the_limit_and_writes_no_title() {
    let mut t = Titles::with_pi(1, Titles::empty_then_preamble);
    t.session("due", "lead", "idle", 0, Some(1), None);

    let engine = t.start_reporting_after("3600");
    wait_until("the empty answer and the preamble", || t.fx.env.calls("pi").len() >= 2);
    t.ticks();
    engine.stop();

    assert_eq!(t.fx.count("error"), 0, "a failing titling is logged, not reported, before the limit: {:?}", t.fx.events("error"));
    assert_eq!(t.fx.env.query::<Option<String>>("SELECT title FROM sessions WHERE id = 'due'"), None, "the preamble is no title");
    assert_eq!(t.fx.env.query::<Option<String>>("SELECT titled_at FROM sessions WHERE id = 'due'"), None);
    assert_eq!(t.retitled(), Vec::<String>::new());
    assert!(t.fx.env.calls("pi").len() >= 2, "the Session stays due and is retried");
}

/// A titling that keeps failing past the limit (2 s here) is reported once, as an `error` Event of E10 that names the
/// Session and sets no Session or Plan; not a moment before the limit.
#[test]
fn e10_reports_a_titling_that_keeps_failing_past_the_limit_once() {
    const LIMIT: Duration = Duration::from_secs(2);
    let mut t = Titles::with_pi(1, Titles::empty_then_preamble);
    t.session("due", "lead", "idle", 0, Some(1), None);

    let started = Instant::now();
    let engine = t.start_reporting_after("2");
    wait_until("a retry of the failed titling", || t.fx.env.calls("pi").len() >= 2);
    if started.elapsed() < LIMIT {
        assert_eq!(t.fx.count("error"), 0, "reported before the limit: {:?}", t.fx.events("error"));
    }
    wait_until("the report", || t.fx.count("error") >= 1);
    assert!(started.elapsed() >= LIMIT, "reported {:?} after the engine started, before the limit", started.elapsed());
    t.ticks();
    engine.stop();

    let errors = t.fx.events("error");
    assert_eq!(errors.len(), 1, "reported once: {errors:?}");
    let data = &errors[0].3;
    assert_eq!((data["source"].as_str(), data["code"].as_str()), (Some("watch"), Some("title_failed")), "{data}");
    assert_eq!(data["details"]["session"], "due");
    assert!(data["details"]["log"].as_str().unwrap().ends_with("run/titles/due.log"), "{data}");
    assert_eq!(t.fx.env.query::<Option<String>>("SELECT session FROM events WHERE kind = 'error'"), None);
    assert_eq!(t.fx.env.query::<Option<String>>("SELECT title FROM sessions WHERE id = 'due'"), None);
    assert_eq!(t.retitled(), Vec::<String>::new());
}

/// A success clears the failure: the 10 minutes in a row start again. Here the limit is 2 s; the titling fails for 1 s,
/// succeeds, is due again and fails for the rest of the time. More than 2 s after the first failure and less than 2 s
/// after the last success there is no `error`; a streak of 2 s since the success then is reported, once.
#[test]
fn e10_counts_the_failures_in_a_row_from_the_last_success() {
    const LIMIT: Duration = Duration::from_secs(2);
    let mut t = Titles::with_pi(1, |root| {
        format!("if [ \"$(cat '{}/mode')\" = ok ]; then echo 'TITLE: Dynamic session titles'; fi", root.display())
    });
    let mode = t.fx.env.root().join("mode");
    fs::write(&mode, "fail").unwrap();
    t.session("due", "lead", "idle", 0, Some(1), None);

    let first_failure = Instant::now();
    let engine = t.start_reporting_after("2");
    wait_until("a failed titling", || t.fx.env.calls("pi").len() >= 1);
    wait_until("half the limit of failures", || first_failure.elapsed() >= LIMIT / 2);
    assert_eq!(t.fx.count("error"), 0, "reported before the limit: {:?}", t.fx.events("error"));

    fs::write(&mode, "ok").unwrap();
    wait_until("the success", || t.retitled().len() == 1);
    assert_eq!(t.fx.env.query::<String>("SELECT title FROM sessions WHERE id = 'due'"), "Dynamic session titles");
    fs::write(&mode, "fail").unwrap();
    let success = Instant::now();
    t.fx.fixture(&format!("UPDATE sessions SET titled_at = '{}' WHERE id = 'due';", ago(20)));
    let calls = t.fx.env.calls("pi").len();
    wait_until("failures again", || t.fx.env.calls("pi").len() >= calls + 2);
    wait_until("the limit after the first failure", || first_failure.elapsed() > LIMIT + Duration::from_millis(200));
    if success.elapsed() < LIMIT {
        assert_eq!(t.fx.count("error"), 0, "the success did not clear the failures in a row: {:?}", t.fx.events("error"));
    }

    wait_until("the report of the second streak", || t.fx.count("error") >= 1);
    assert!(success.elapsed() >= LIMIT, "reported {:?} after the success, before the limit", success.elapsed());
    t.ticks();
    engine.stop();
    assert_eq!(t.fx.count("error"), 1, "{:?}", t.fx.events("error"));
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
    assert_eq!(calls, [vec!["--version"], vec!["config"], vec!["watch", "--check"], vec!["watch", "--mark-restart"], vec!["watch", "--ensure"]]);
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

/// The install's `cpd watch --ensure` runs from a launchd agent that sets only `PATH`: tmux then prints the tabs of cpd's `-F`
/// formats as `_`, and the call failed `tmux_failed: tmux list-panes printed unexpected output` (mac-pro, hs-d463).
#[test]
fn ensure_starts_the_engine_for_a_caller_without_a_utf8_locale() {
    let mut failures = Vec::new();
    for locale in crate::common::LOCALES {
        let fx = fx("");
        fx.env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
        let mut cmd = fx.env.command(CPD);
        locale.apply(&mut cmd).args(["watch", "--ensure"]);
        let out = common::output(&mut cmd).unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        if !out.status.success() || !stdout.contains("\"started\":true") {
            failures.push(format!("{locale:?}: watch --ensure exited {:?}: {stdout}{}", out.status.code(), String::from_utf8_lossy(&out.stderr)));
            continue;
        }
        // The second call sees the engine's window through the same listing.
        let mut again = fx.env.command(CPD);
        locale.apply(&mut again).args(["watch", "--ensure"]);
        let out = common::output(&mut again).unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        if !stdout.contains("\"already\":true") {
            failures.push(format!("{locale:?}: the second watch --ensure: {stdout}{}", String::from_utf8_lossy(&out.stderr)));
        }
    }
    assert!(failures.is_empty(), "cpd watch --ensure fails without a UTF-8 locale:\n{}", failures.join("\n"));
}

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

    /// The install's marker, written `age` seconds ago for the engine `pid`.
    fn marker(&self, pid: i64, age: f64) {
        self.raw_marker(&json!({"by": "install-cpd.sh", "at": secs_ago(age), "engine_pid": pid}).to_string());
    }

    fn raw_marker(&self, text: &str) {
        fs::create_dir_all(self.marker_path().parent().unwrap()).unwrap();
        fs::write(self.marker_path(), text).unwrap();
    }

    /// Starts the engine, lets it finish its start and stops it: what the Managers were told is then settled.
    fn start_and_stop(&self) {
        let _ = self.start().stop();
    }

    /// The `watch-started` text of the engine under test.
    fn started_text(&self) -> String {
        self.texts("watch-started").pop().unwrap()
    }

    /// No Manager got a downtime message, whatever `how` the engine stopped.
    fn told_with(&self, _how: &str) {
        self.manager_heard_nothing();
    }
}

#[test]
fn a_planned_restart_back_within_ten_seconds_is_logged_and_tells_no_manager() {
    let fx = fx("");
    fx.manager("mgr1");
    fx.previous_engine(0.5, true);
    fx.marker(PREVIOUS_ENGINE, 1.0);
    fx.start_and_stop();
    assert_eq!(fx.messages(), [], "a quiet restart sends no session-message");
    let text = fx.started_text();
    assert!(text.contains("The engine was down for ") && text.contains(&format!("{PLANNED}.")), "{text}");
    assert!(watch_log(&fx).contains(&format!("{PLANNED}.")), "{}", watch_log(&fx));
    assert!(!fx.marker_path().exists(), "the marker is consumed at the start");
}

#[test]
fn a_stop_with_no_marker_tells_each_manager_as_before() {
    let fx = fx("");
    fx.manager("mgr1");
    fx.previous_engine(0.5, true);
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(!fx.started_text().contains(PLANNED), "{}", fx.started_text());
}

#[test]
fn a_planned_restart_that_stayed_down_past_ten_seconds_tells_each_manager_it_took_longer() {
    let fx = fx("");
    fx.manager("mgr1");
    fx.previous_engine(11.0, true);
    fx.marker(PREVIOUS_ENGINE, 12.0);
    fx.start_and_stop();
    fx.told_with("it was an install's restart that took longer than 10 s");
    assert!(!fx.marker_path().exists(), "the marker is consumed at the start");
}

/// The bound is `downtime <= 10 s` quiet, `> 10 s` tells. The new engine starts a moment after the time the fixture sets, so the
/// quiet side is set a little under 10 s (exactly 10.0 would be a race with the process start) and the other a little over.
#[test]
fn the_bound_is_ten_seconds_quiet_up_to_it_and_told_past_it() {
    let quiet = fx("");
    quiet.manager("mgr1");
    quiet.previous_engine(9.2, true);
    quiet.marker(PREVIOUS_ENGINE, 9.5);
    quiet.start_and_stop();
    assert_eq!(quiet.messages(), [], "under 10 s is quiet");

    let told = fx("");
    told.manager("mgr1");
    told.previous_engine(10.6, true);
    told.marker(PREVIOUS_ENGINE, 11.0);
    told.start_and_stop();
    told.told_with("took longer than 10 s");
}

#[test]
fn a_marker_for_another_engine_does_not_silence_the_stop_and_is_deleted() {
    let fx = fx("");
    fx.manager("mgr1");
    fx.previous_engine(0.5, true);
    fx.marker(PREVIOUS_ENGINE + 1, 1.0);
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(!fx.started_text().contains(PLANNED), "{}", fx.started_text());
    assert!(!fx.marker_path().exists(), "a foreign marker is deleted too");
}

#[test]
fn a_marker_older_than_120_seconds_does_not_silence_the_stop_and_is_deleted() {
    let fx = fx("");
    fx.manager("mgr1");
    fx.previous_engine(0.5, true);
    fx.marker(PREVIOUS_ENGINE, 130.0);
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(!fx.started_text().contains(PLANNED), "{}", fx.started_text());
    assert!(!fx.marker_path().exists(), "a stale marker is deleted too");
}

#[test]
fn an_unparseable_marker_does_not_silence_the_stop_is_logged_once_deleted_and_the_engine_runs() {
    let fx = fx("");
    fx.manager("mgr1");
    fx.previous_engine(0.5, true);
    fx.raw_marker("{not json");
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(!fx.marker_path().exists(), "the unreadable marker is deleted");
    let lines = watch_log(&fx).lines().filter(|l| l.contains("install-restart.json")).count();
    assert_eq!(lines, 1, "{}", watch_log(&fx));
}

#[test]
fn a_marker_with_a_field_of_another_type_is_no_marker() {
    let fx = fx("");
    fx.manager("mgr1");
    fx.previous_engine(0.5, true);
    fx.raw_marker(&json!({"by": "install-cpd.sh", "at": secs_ago(1.0), "engine_pid": "424242"}).to_string());
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(!fx.marker_path().exists());
}

#[test]
fn a_crashed_engine_with_a_valid_marker_is_quiet_by_the_time_it_was_last_alive() {
    let fx = fx("");
    fx.manager("mgr1");
    fx.previous_engine(0.5, false);
    fx.marker(PREVIOUS_ENGINE, 1.0);
    fx.start_and_stop();
    assert_eq!(fx.messages(), []);
    let text = fx.started_text();
    assert!(text.contains("it was last alive") && text.contains(&format!("{PLANNED}.")), "{text}");
}

#[test]
fn a_crashed_engine_with_a_valid_marker_past_ten_seconds_tells() {
    let fx = fx("");
    fx.manager("mgr1");
    fx.previous_engine(11.0, false);
    fx.marker(PREVIOUS_ENGINE, 12.0);
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
