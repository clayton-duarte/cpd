//! The engine, `cpd watch` (contract §3.7, §6.6; commands.md §12): the lock
//! and its refusal, the signals, `--ensure`, and each step E1–E6 (E7, the Workflow start, is in workflow_start.rs) on a
//! prepared database, with a private tmux server and fake `pi` and `cpd`.

use crate::common;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::engine::EngineWindow;
use common::fx::Fx as Ip;
use common::{CPD, PR_POLL_EVERY_TICK, Run, SLEEP_SECONDS, TICK_SECONDS, TestEnv, wait_until};
use cpd_core::clock::Clock;
use cpd_core::error::CpdError;
use cpd_core::git::{self, Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::{CpdHome, Infra};
use cpd_core::host::runner::{Call, FakeRunner, Reply};
use cpd_core::host::spawn::{Launch, Pid as SpawnPid, Spawn};
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

/// An Event row: `(plan, job, attempt, data)`.
type EventRow = (Option<String>, Option<String>, Option<String>, Value);

const PLAN: &str = "pay";

/// O supports efforts 0–5, S 1–5, H 0–3.
const EFFORTS: [(&str, &str, &str); 4] = [
    ("O", "p/opus", "[0,1,2,3,4,5]"),
    ("S", "p/sonnet", "[1,2,3,4,5]"),
    ("H", "p/haiku", "[0,1,2,3]"),
    ("G", "p/gemini", "[]"),
];

/// A time `minutes` ago, as cpd writes times (contract §1.3).
fn ago(minutes: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::minutes(minutes))
        .format(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .unwrap()
}

/// SQL for Attempt `<key>-a<number>` ended `failed` / `crashed` at `ended_at`; `driver_pid` is null when its driver never started.
fn ended_attempt(
    key: &str,
    number: u32,
    tier: &str,
    driver_pid: Option<i64>,
    ended_at: &str,
) -> String {
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
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, description, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'main', 'dispatch', 'cpd/pay', '{lead}', '{PR_TITLE}', 'A description', 'config', cpd_txn(), 't', 't');",
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
    for (key, value) in [
        ("user.name", "cpd test"),
        ("user.email", "test@cpd.invalid"),
        ("commit.gpgsign", "false"),
    ] {
        env.git(&repo, &["config", key, value]);
    }
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: env.root().join("lead.log"),
    };
    let lead = create_lead_worktree(&repo, &PlanId::parse(PLAN).unwrap(), "main", &setup)
        .unwrap()
        .path;
    env.write_models();
    env.quiet_models(OffsetDateTime::now_utc());
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n{config_extra}\n\
             [watch]\ntick_seconds = {TICK_SECONDS}\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env
        .root()
        .join("install/releases/1.0.0/node_modules/@earendil-works");
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
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', '{pr}', '{key}', 'Title of {key}', '{tier}', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        );
        for need in needs {
            sql += &format!(
                "INSERT INTO dependencies (plan, job, needs) VALUES ('pay', '{key}', '{need}');"
            );
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
        self.add_running_since(
            key,
            status,
            started_at,
            (status == "running").then_some(started_at),
        );
    }

    /// `add_running` with the `starting → running` Event at `running_at` (none: the Attempt is still `starting`).
    fn add_running_since(
        &self,
        key: &str,
        status: &str,
        started_at: &str,
        running_at: Option<&str>,
    ) {
        let worktree = self
            .repo
            .parent()
            .unwrap()
            .join(format!("pay.pay--{key}-a1"));
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

    /// Holds Attempt `attempt`'s driver lock, as a live driver does.
    fn live_driver(&self, attempt: &str) -> Flock<File> {
        let home = self.home();
        let path = home.attempt_lock(
            &PlanId::parse(PLAN).unwrap(),
            &AttemptId::parse(attempt).unwrap(),
        );
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .unwrap();
        Flock::lock(file, FlockArg::LockExclusiveNonblock)
            .map_err(|(_, e)| e)
            .unwrap()
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
        let child = command
            .arg("watch")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
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
    fn events(&self, kind: &str) -> Vec<EventRow> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT plan, job, attempt, data FROM events WHERE kind = ?1 ORDER BY id",
            )?;
            let rows = stmt
                .query_map([kind], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows
                .into_iter()
                .map(|(p, j, a, d)| (p, j, a, serde_json::from_str(&d).unwrap()))
                .collect())
        })
    }

    fn count(&self, kind: &str) -> usize {
        self.events(kind).len()
    }

    fn texts(&self, kind: &str) -> Vec<String> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT text FROM events WHERE kind = ?1 ORDER BY id")?;
            stmt.query_map([kind], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
    }

    fn attempt(&self, id: &str) -> (String, Option<String>, Option<String>, Option<String>) {
        self.env.read(|conn| {
            conn.query_row(
                "SELECT status, outcome, cause, stop_request FROM attempts WHERE plan = 'pay' AND id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
        })
    }

    fn job_status(&self, key: &str) -> String {
        self.env.query(&format!(
            "SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"
        ))
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
    assert_eq!(
        fx.events("watch-started"),
        [(None, None, None, json!({"pid": pid}))]
    );
    assert_eq!(
        fx.events("watch-stopped"),
        [(None, None, None, json!({"pid": pid}))]
    );
    let log = fs::read_to_string(fx.env.cpd_home().join("run/watch.log")).unwrap();
    assert!(log.contains(&format!("started (pid {pid})")), "{log}");
    assert!(
        out.stderr.contains(&format!("started (pid {pid})")),
        "{out:?}"
    );

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
    assert_eq!(
        fx.events("watch-stopped"),
        [(None, None, None, json!({"pid": std::process::id()}))]
    );
}

#[test]
fn watch_refuses_unknown_flags_and_needs_its_config() {
    let fx = engine("");
    let run = fx.cpd(&["watch", "--bogus"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (2, json!("usage")));
    fs::remove_file(fx.env.cpd_home().join("config.toml")).unwrap();
    let run = fx.cpd(&["watch"]);
    assert_eq!(
        (run.exit, run.error()["code"].clone()),
        (1, json!("config_missing"))
    );
}

// ---------- --ensure ----------

/// A home for `cpd watch --ensure` in-process: its config on the fake tmux server of `fx.tmux(..)`. The window the call makes runs no
/// program: what its process does is [`Ip::engine_window`], called from [`Ip::on_nap`] while the call waits.
fn ensuring() -> Ip {
    let fx = Ip::with_ports();
    fx.engine_config_on_tmux("");
    // The `pane-died` hook of cpd's server runs `sh -c <the supervising `cpd watch --ensure`>`: tmux's to fire, answered here.
    fx.fake("sh", Reply::ok(""));
    fx
}

/// The `-F` of [`panes_of`].
const PANE_FIELDS: &str = "#{pane_id} #{session_name}:#{window_name} #{pane_dead} #{pane_pid}";

/// Every pane of the fake server, `<pane> <session>:<window> <dead> <pid>` per line.
fn panes_of(fx: &Ip) -> String {
    String::from_utf8_lossy(&fx.tmux(&["list-panes", "-a", "-F", PANE_FIELDS]).stdout).into_owned()
}

/// The windows of the work session, one name per line.
fn work_windows(fx: &Ip) -> String {
    String::from_utf8_lossy(
        &fx.tmux(&["list-windows", "-t", "=cpd-work", "-F", "#{window_name}"])
            .stdout,
    )
    .into_owned()
}

/// What the product ran to start the engine: its window's `new-session`/`new-window`/`respawn-pane` calls, whose argv ends in the command.
fn window_commands(fx: &Ip) -> Vec<Vec<String>> {
    let makes = |call: &Call| {
        call.args
            .iter()
            .any(|a| ["new-session", "new-window", "respawn-pane"].contains(&a.as_str()))
    };
    fx.calls("tmux")
        .into_iter()
        .filter(makes)
        .map(|call| call.args)
        .collect()
}

#[test]
fn ensure_starts_the_engine_in_the_work_session_once() {
    let mut fx = ensuring();
    let engine = fx.engine_window(EngineWindow::Runs);
    let runs = engine.clone();
    fx.on_nap(move |_| runs.at_nap());

    let out = fx.cpd(&["watch", "--ensure"]).ok();
    assert_eq!(out["started"], true, "{out}");
    let pane = out["pane"].as_str().unwrap().to_owned();
    assert!(pane.starts_with('%'), "{out}");
    let panes = panes_of(&fx);
    let line = panes
        .lines()
        .find(|l| l.starts_with(&format!("{pane} ")))
        .unwrap_or_else(|| panic!("{panes}"));
    let fields: Vec<&str> = line.split(' ').collect();
    assert_eq!(&fields[1..3], ["cpd-work:watch", "0"], "{panes}");
    // Its process took the lock and said so.
    let pid: u32 = fields[3].parse().unwrap();
    assert_eq!(
        fx.events("watch-started"),
        [(None, None, None, json!({"pid": pid}))]
    );
    assert_eq!(fx.cpd(&["watch"]).error()["code"], "watch_running");
    // The one window is the engine's own start: `cpd watch`, E5 runs the engine's executable, not `cpd` from `PATH`.
    let commands = window_commands(&fx);
    assert_eq!(commands.len(), 1, "{commands:?}");
    assert_eq!(
        commands[0][commands[0].len() - 2..],
        ["cpd", "watch"],
        "{commands:?}"
    );

    let again = fx.cpd(&["watch", "--ensure"]).ok();
    assert_eq!(
        again,
        json!({"ok": true, "started": false, "already": true})
    );
    assert_eq!(fx.count("watch-started"), 1);

    // A dead engine's window is replaced, not duplicated: the supervising hook starts it again (contract §6.6). That tmux fires the
    // hook is tmux's; what it runs is the string it was given, and that command starts the engine in the same window.
    engine.stop();
    fx.pane_exit(&pane, 0);
    let hook: Vec<String> = fx
        .calls("sh")
        .into_iter()
        .map(|call| call.args.join(" "))
        .collect();
    assert_eq!(hook.len(), 1, "{hook:?}");
    assert!(hook[0].contains("watch --ensure --wait"), "{hook:?}");
    fx.cpd(&["watch", "--ensure", "--wait"]).ok();
    assert_eq!(fx.count("watch-started"), 2);
    assert_eq!(work_windows(&fx), "watch\n");
}

/// hygiene.md 3c: the late supervising `ensure` of a test whose env is being removed must not make a home again. The
/// home it finds is the one `rm -rf` leaves for a moment: `config.toml` still readable, `cpd.db` (which every set-up
/// home has: the store is opened before anything runs `watch --ensure`) already gone, as `run/` and `locks/`. (With no
/// `$CPD_HOME` at all `ensure` fails already: `config_missing`.) The marker the fix keys on is `cpd.db`.
#[test]
fn ensure_in_a_home_that_does_not_exist_fails_and_creates_nothing() {
    let fx = ensuring();
    let home = fx.env.root().join("home-being-removed");
    fs::create_dir_all(&home).unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tmux]\nsocket = \"{}\"\n",
            fx.env.tmux_socket()
        ),
    )
    .unwrap();
    let entries = || -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&home)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };

    // The server, if it started, would be the fake's, on the config's socket.
    let run = fx.cpd_with(
        fx.env.root(),
        &["watch", "--ensure"],
        &[("CPD_HOME", home.to_str().unwrap())],
    );

    assert_ne!(
        run.exit, 0,
        "ensure succeeded in a home with no cpd.db: {run:?}"
    );
    assert_eq!(
        entries(),
        ["config.toml"],
        "ensure made something in the home: {run:?}"
    );
    assert!(fx.tmux_state().is_none(), "ensure started a tmux server");
}

/// engine-W10: `list-panes` fails while a concurrent call's `new-session` is starting the server, and the server is running
/// a moment later. The call lists again instead of failing with the first call's stderr.
#[test]
fn ensure_lists_panes_again_when_the_first_list_panes_fails_while_the_server_runs() {
    let mut fx = ensuring();
    let runs = fx.engine_window(EngineWindow::Runs);
    fx.on_nap(move |_| runs.at_nap());
    fx.tmux(&["new-session", "-d", "-s", "other"]);
    let failed = Arc::new(AtomicBool::new(false));
    let first = failed.clone();
    let _ = (*fx.fakes.runner).clone().once(
        "tmux",
        move |call| {
            let hit = call.args.iter().any(|a| a == "list-panes");
            if hit {
                first.store(true, Ordering::SeqCst);
            }
            hit
        },
        Reply::exit(1, "", "error connecting to x (No such file or directory)\n"),
    );

    let out = fx.cpd(&["watch", "--ensure"]).ok();

    assert_eq!(out["started"], true, "{out}");
    assert!(
        failed.load(Ordering::SeqCst),
        "the fake tmux never failed a list-panes"
    );
}

#[test]
fn ensure_fails_when_the_engine_does_not_start() {
    let mut fx = ensuring();
    // The command of the window exits with 7.
    let exits = fx.engine_window(EngineWindow::Exits(7));
    fx.on_nap(move |_| exits.at_nap());

    let run = fx.cpd(&["watch", "--ensure"]);

    assert_eq!(run.exit, 1, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "watch_start_failed");
    assert_eq!(error["details"]["exit"], 7);
    assert_eq!(error["details"]["next"], "cpd watch --ensure");
    assert!(error["details"]["pane"].as_str().unwrap().starts_with('%'));
}

#[test]
fn ensure_says_the_exit_status_is_unknown_when_tmux_has_none() {
    let mut fx = ensuring();
    // Killed by a signal: the pane is dead and `#{pane_dead_status}` is empty.
    let killed = fx.engine_window(EngineWindow::IsKilled);
    fx.on_nap(move |_| killed.at_nap());

    let run = fx.cpd(&["watch", "--ensure"]);

    let error = run.error();
    assert_eq!(error["code"], "watch_start_failed");
    assert_eq!(error["details"]["exit"], Value::Null);
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("its exit status is unknown"), "{message}");
}

/// A plain `--ensure` waits for `locks/watch-ensure.lock` like a supervising one: held by another call between its probe and its
/// start, it keeps the plain call from probing, so no second window starts. Released, the call starts the engine.
#[test]
fn a_plain_ensure_waits_for_the_ensure_lock_another_call_holds() {
    let mut fx = ensuring();
    let path = fx.home().locks_dir().join("watch-ensure.lock");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    let held = Flock::lock(file, FlockArg::LockExclusiveNonblock)
        .map_err(|(_, e)| e)
        .unwrap();
    let runs = fx.engine_window(EngineWindow::Runs);
    let runner = fx.fakes.runner.clone();
    let mut held = Some(held);
    let mut waited = 0;
    // The call naps while it waits for the lock: nothing started and nothing probed until the other call lets go (its second nap).
    fx.on_nap(move |_| {
        waited += 1;
        if waited == 2 {
            assert_eq!(
                runner
                    .calls()
                    .iter()
                    .filter(|c| c.program.ends_with("tmux"))
                    .count(),
                0,
                "the plain call probed while the lock was held"
            );
            drop(held.take());
        }
        runs.at_nap();
    });

    let out = fx.cpd(&["watch", "--ensure"]).ok();

    assert_eq!(out["started"], true, "{out}");
    assert_eq!(
        fx.fakes.clock.naps()[..2],
        [Duration::from_millis(100); 2],
        "the call waited for the lock on virtual time"
    );
    assert_eq!(window_commands(&fx).len(), 1);
}

/// The install's `cpd watch --ensure` runs from a launchd agent that sets only `PATH`: tmux then prints the tabs of cpd's `-F`
/// formats as `_`, and the call failed `tmux_failed: tmux list-panes printed unexpected output` (mac-pro, hs-d463). What cpd
/// controls is that every tmux call it makes is a UTF-8 client, `-u` first, whatever locale the caller has.
#[test]
fn ensure_starts_the_engine_for_a_caller_without_a_utf8_locale() {
    let mut failures = Vec::new();
    for locale in common::LOCALES {
        let mut fx = ensuring();
        let runs = fx.engine_window(EngineWindow::Runs);
        fx.on_nap(move |_| runs.at_nap());
        let out = fx.cpd_with(fx.env.root(), &["watch", "--ensure"], locale.vars());
        if out.exit != 0 || out.json()["started"] != true {
            failures.push(format!(
                "{locale:?}: watch --ensure exited {}: {}",
                out.exit, out.stdout
            ));
            continue;
        }
        // The second call sees the engine's window through the same listing.
        let again = fx.cpd_with(fx.env.root(), &["watch", "--ensure"], locale.vars());
        if again.json()["already"] != true {
            failures.push(format!(
                "{locale:?}: the second watch --ensure: {}",
                again.stdout
            ));
        }
        let calls = fx.calls("tmux");
        if calls.is_empty()
            || calls
                .iter()
                .any(|c| c.args.first().map(String::as_str) != Some("-u"))
        {
            failures.push(format!(
                "{locale:?}: a tmux call is not a UTF-8 client: {calls:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "cpd watch --ensure fails without a UTF-8 locale:\n{}",
        failures.join("\n")
    );
}

// ---------- supervision: the engine comes back whatever stopped it (contract §6.6, hs-d328) ----------

// ---------- the engine in-process: `watch::run_with(ports, until)` on a `ManualClock` ----------

/// A home for the engine on fake ports ([`Fx::watch`]): its config, the routes it needs, Context `ctx`. No tmux, no git, no
/// process, no wait: the clock is the `ManualClock`, so a time a test sets is exact.
fn engine(config_extra: &str) -> Ip {
    let fx = engine_home(config_extra);
    fx.engine_routes();
    fx
}

/// [`engine`] with no answer yet for the runner's calls ([`Ip::engine_routes`], [`Ip::engine_routes_polling`]).
fn engine_home(config_extra: &str) -> Ip {
    let fx = Ip::with_ports();
    fx.engine_config(config_extra);
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
    fn events(&self, kind: &str) -> Vec<EventRow>;
    fn texts(&self, kind: &str) -> Vec<String>;
    /// The messages the engine sent: `(session, context, data)`.
    fn messages(&self) -> Vec<(String, Option<String>, Value)>;
    fn manager(&self, id: &str);
    /// How many Events of `kind` there are.
    fn count(&self, kind: &str) -> usize;
    /// Attempt `id`: `(status, outcome, cause, stop_request)`.
    fn attempt(&self, id: &str) -> (String, Option<String>, Option<String>, Option<String>);
    fn job_status(&self, key: &str) -> String;
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

    fn events(&self, kind: &str) -> Vec<EventRow> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT plan, job, attempt, data FROM events WHERE kind = ?1 ORDER BY id",
            )?;
            let rows = stmt
                .query_map([kind], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows
                .into_iter()
                .map(|(p, j, a, d)| (p, j, a, serde_json::from_str(&d).unwrap()))
                .collect())
        })
    }

    fn texts(&self, kind: &str) -> Vec<String> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT text FROM events WHERE kind = ?1 ORDER BY id")?;
            stmt.query_map([kind], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
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

    fn count(&self, kind: &str) -> usize {
        self.env.query::<i64>(&format!(
            "SELECT count(*) FROM events WHERE kind = '{kind}'"
        )) as usize
    }

    fn attempt(&self, id: &str) -> (String, Option<String>, Option<String>, Option<String>) {
        self.env.read(|conn| {
            conn.query_row(
                "SELECT status, outcome, cause, stop_request FROM attempts WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
        })
    }

    fn job_status(&self, key: &str) -> String {
        self.env.query(&format!(
            "SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"
        ))
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
        self.raw_marker(
            &json!({"by": "install-cpd.sh", "at": self.stamp_ago(age_ms), "engine_pid": pid})
                .to_string(),
        );
    }

    fn raw_marker(&self, text: &str) {
        fs::create_dir_all(self.marker_path().parent().unwrap()).unwrap();
        fs::write(self.marker_path(), text).unwrap();
    }

    fn start_and_stop(&self) {
        self.watch_first_tick().unwrap();
    }

    fn told_with(&self, how: &str) {
        assert_eq!(
            self.messages(),
            [],
            "a restart sends no session-message (hs-d514): the Managers are not told"
        );
        assert!(
            self.started_text().contains(how),
            "{how:?} not in {}",
            self.started_text()
        );
    }
}

// ---------- the downtime message (hs-d328) ----------

/// `Fx::watch` ends the engine at the nap that follows the first tick: one tick, one nap of a tenth of a second, no wait.
#[test]
fn the_engine_in_process_runs_until_the_first_tick_and_naps_once_on_virtual_time() {
    let fx = engine("");

    fx.watch_first_tick().unwrap();

    assert_eq!(fx.fakes.clock.naps(), [Duration::from_millis(100)]);
    assert_eq!(
        (
            fx.events("watch-started").len(),
            fx.events("watch-stopped").len()
        ),
        (1, 1)
    );
}

#[test]
fn the_engine_in_process_runs_until_the_nap_it_is_told_and_the_hook_sees_each_nap_before_it_passes()
{
    let fx = engine("");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();

    fx.watch(3, move |nap| log.lock().unwrap().push(nap))
        .unwrap();

    assert_eq!(*seen.lock().unwrap(), [1, 2, 3]);
    assert_eq!(fx.fakes.clock.naps(), [Duration::from_millis(100); 3]);
}

#[test]
fn the_engine_in_process_runs_until_its_step_says_the_nap_is_the_last_and_the_step_sees_the_state_after_each_tick()
 {
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

    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM jobs WHERE plan = 'pay' AND key = 'api'"),
        "queued"
    );
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT count(*) FROM jobs WHERE plan = 'pay' AND fingerprint IS NOT NULL"
        ),
        2
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT needs FROM dependencies WHERE plan = 'pay' AND job = 'web'"),
        "api"
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT phase FROM plans WHERE id = 'pay'"),
        "dispatch"
    );
}

#[test]
fn a_restart_sends_no_downtime_notice() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.watch_first_tick().unwrap();
    let stopped_at = fx
        .env
        .query::<String>("SELECT at FROM events WHERE kind = 'watch-stopped' ORDER BY id LIMIT 1");
    fx.watch_first_tick().unwrap();
    // The record is the Event's text, not a message (hs-d514: the Managers are not told of a restart).
    // The field stays UTC; the sentence says the time in the engine's zone, which the harness pins (`TZ`, `TestEnv::isolation`) to
    // the rule of `ManualClock`'s zone.
    let at =
        time::OffsetDateTime::parse(&stopped_at, &time::format_description::well_known::Rfc3339)
            .unwrap();
    let local = cpd_core::clock::prose(at, &cpd_core::clock::ManualClock::new().zone());
    assert!(
        fx.texts("watch-started")[1].contains(&format!("it stopped at {local}")),
        "{local} {:?}",
        fx.texts("watch-started")
    );
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
    assert!(
        text.contains("it was last alive"),
        "{:?}",
        fx.texts("watch-started")
    );
    assert!(
        text.contains("The engine was down for 0.5 s"),
        "down from the lock's last mark, not from its start: {text}"
    );
}

// ---------- @cpd_workers ----------

// Each fixture's dispatch PR has its `open` Job, which E4 adds at the first tick and which waits for the PR's other Jobs: one queued Job.
const NO_WORKERS: &str = "workers 1\u{f0798}0\u{f0798}0/3";

/// The engine in-process on the fake tmux server of `fx.tmux(..)`: Plan `pay` in `dispatch` (its `open` Job is the one queued
/// Job of [`NO_WORKERS`]), no tmux server yet ([`Ip::tmux_server`]).
fn engine_on_tmux() -> Ip {
    engine_on_tmux_with("")
}

/// [`engine_on_tmux`] with `config_extra` in the config.
fn engine_on_tmux_with(config_extra: &str) -> Ip {
    let fx = engine_home(config_extra);
    fx.engine_config_on_tmux(config_extra);
    fx.engine_routes_on_tmux();
    fx.engine_plan();
    fx
}

/// The engine's own `errors` that name `@cpd_workers`.
fn workers_errors(fx: &Ip) -> Vec<String> {
    fx.texts("error")
        .into_iter()
        .filter(|t| t.contains("@cpd_workers"))
        .collect()
}

/// Ticks to run for "more than one engine tick" (`tick_seconds` 1, a nap of a tenth of a second): 2.5 s of virtual time.
const OVER_ONE_TICK: usize = 25;

#[test]
fn without_a_tmux_server_the_workers_step_does_nothing_and_sets_the_text_once_one_appears() {
    let fx = engine_on_tmux();
    // No error over more than one engine tick without a tmux server; the engine records nothing at a tick that does nothing.
    fx.watch_ticks(OVER_ONE_TICK).unwrap();
    assert_eq!(fx.events("error").len(), 0, "{:?}", fx.texts("error"));
    assert!(!fx.tmux(&["has-session"]).status.success());

    fx.tmux_server();
    fx.watch_ticks(1).unwrap();
    assert_eq!(fx.tmux_option("@cpd_workers"), NO_WORKERS);
    assert_eq!(fx.events("error").len(), 0, "{:?}", fx.texts("error"));
}

#[test]
fn a_tmux_failure_of_the_workers_step_is_one_error_and_the_engine_goes_on() {
    let fx = engine_on_tmux();
    fx.tmux_server();
    // A tmux that fails `set -g @cpd_workers` and is the route otherwise, until the test says it works again.
    let broken = Arc::new(AtomicBool::new(true));
    let failing = broken.clone();
    let _ = (*fx.fakes.runner).clone().on(
        "tmux",
        move |call| {
            failing.load(Ordering::SeqCst)
                && format!(" {}", call.args.join(" ")).contains(" set -g @cpd_workers")
        },
        Reply::exit(1, "", "boom\n"),
    );

    // One engine run over more than one tick: the failed set is reported once, not at every tick (the engine records nothing at a
    // tick that sets nothing new).
    fx.watch_ticks(OVER_ONE_TICK).unwrap();
    assert_eq!(workers_errors(&fx).len(), 1, "{:?}", workers_errors(&fx));
    assert!(
        workers_errors(&fx)[0].contains("tmux set failed: boom"),
        "{:?}",
        workers_errors(&fx)
    );
    assert_eq!(fx.tmux_option("@cpd_workers"), "");

    // It kept running, and sets the text once tmux works again.
    broken.store(false, Ordering::SeqCst);
    fx.watch_ticks(1).unwrap();
    assert_eq!(fx.tmux_option("@cpd_workers"), NO_WORKERS);
}

#[test]
fn the_engine_counts_waiting_and_queued_jobs() {
    let fx = engine_on_tmux_with("concurrency = 1");
    fx.tmux_server();
    let _lock = fx.live_driver("run-a1");
    fx.add_running("run", "running", &ago_in(&fx, 1));
    // Ready but the only slot is taken (E3 starts nothing); the other needs it, so it is queued and not waiting. The PR's `open` Job, which
    // E4 adds at the first tick, waits for them all: queued too.
    fx.add_job("free", &[], "");
    fx.add_job("blocked", &["free"], "");

    // The second tick counts the `open` Job the first adds.
    fx.watch_ticks(OVER_ONE_TICK).unwrap();

    assert_eq!(
        fx.tmux_option("@cpd_workers"),
        "workers 3\u{f0798}1\u{f0798}1/1"
    );
    assert_eq!(
        fx.tmux_option("@cpd_workers_short"),
        "3\u{f0798}1\u{f0798}1/1"
    );
    // The engine sets no widths: tmux measures the texts (#347).
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

#[test]
fn watch_cpd_workers_option_counts_are_unchanged_with_workers_as_a_resource() {
    // Workers are a resource now (contract §8.3), but `@cpd_workers` and the StatusBar keep their meaning and numbers (§6.2): the
    // same fixture as `the_engine_counts_waiting_and_queued_jobs` gives the same texts, and the Attempts they count are the
    // tenants `job show` names for `workers`.
    let fx = engine_on_tmux_with("concurrency = 1");
    fx.tmux_server();
    let _lock = fx.live_driver("run-a1");
    fx.add_running("run", "running", &ago_in(&fx, 1));
    fx.add_job("free", &[], "");
    fx.add_job("blocked", &["free"], "");

    fx.watch_ticks(OVER_ONE_TICK).unwrap();

    assert_eq!(
        fx.tmux_option("@cpd_workers"),
        "workers 3\u{f0798}1\u{f0798}1/1"
    );
    assert_eq!(
        fx.tmux_option("@cpd_workers_short"),
        "3\u{f0798}1\u{f0798}1/1"
    );
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
    // The ready Job waits for `workers`, whose one tenant is the one Attempt the `1/1` counts.
    let shown = fx.cpd(&["job", "show", "free", "--plan", PLAN]).ok();
    let waiting = shown["job"]["waiting_for"]
        .as_array()
        .unwrap_or_else(|| panic!("`free` waits for workers: {shown}"));
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(
        (
            &waiting[0]["resource"],
            &waiting[0]["scope"],
            &waiting[0]["slots"]
        ),
        (&json!("workers"), &json!("host"), &json!(1))
    );
    let holders = waiting[0]["holders"].as_array().unwrap();
    assert_eq!(holders.len(), 1);
    assert_eq!(
        (
            &holders[0]["by"],
            &holders[0]["job"],
            &holders[0]["attempt"]
        ),
        (&json!("attempt"), &json!("run"), &json!("run-a1"))
    );
}

#[test]
fn watch_cpd_workers_option_counts_tenant_jobs_including_a_failed_one() {
    // `N` is the occupancy of the `workers` pool as the starter counts it (contract §3.4 "Tenancy"): the tenant Jobs, so a `failed`
    // Job whose Attempt ended still counts, and the option and the starter agree. Three slots (the default), one running Job and
    // one failed Job: 2/3.
    let fx = engine_on_tmux();
    fx.tmux_server();
    let _lock = fx.live_driver("run-a1");
    fx.add_running("run", "running", &ago_in(&fx, 1));
    fx.add_job(
        "bad",
        &[],
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'bad';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at)
           VALUES ('pay', 'bad-a1', 'bad', 1, 'S2', 'ladder', 'ended', 'failed', 'timed_out', 'cpd/pay--bad-a1', '/w', 't', 't');
         UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE plan = 'pay' AND key = 'bad';",
    );

    fx.watch_ticks(OVER_ONE_TICK).unwrap();

    assert_eq!(
        fx.tmux_option("@cpd_workers"),
        "workers 1\u{f0798}0\u{f0798}2/3"
    );
    assert_eq!(
        fx.tmux_option("@cpd_workers_short"),
        "1\u{f0798}0\u{f0798}2/3"
    );
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

// ---------- E1 ----------

#[test]
fn e1_expires_open_escalations_whose_asker_is_dead() {
    let fx = engine("");
    fx.engine_plan();
    // The askers are processes of the `FakeSpawn`: one has ended, one runs.
    let (dead, alive) = (fx.ended_pid(), fx.live_pid());
    let lock = fx.live_driver("api-a1");
    fx.add_running("api", "running", &fx.stamp_ago(60_000));
    fx.env.sql(&format!(
        "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
           VALUES ('pay', 'api-a1-q1', 'api-a1', 1, 'Why?', 'open', {dead}, '{t}'),
                  ('pay', 'api-a1-q2', 'api-a1', 2, 'How?', 'open', {alive}, '{t}');",
        t = fx.stamp_ago(60_000)
    ));

    fx.watch_first_tick().unwrap();
    drop(lock);

    assert_eq!(
        fx.events("escalation-expired"),
        [(
            Some("pay".into()),
            Some("api".into()),
            Some("api-a1".into()),
            json!({"escalation": "api-a1-q1", "reason": "asker_dead"})
        )]
    );
    let status = |id: &str| {
        fx.env
            .query::<String>(&format!("SELECT status FROM escalations WHERE id = '{id}'"))
    };
    assert_eq!(
        (status("api-a1-q1"), status("api-a1-q2")),
        ("expired".into(), "open".into())
    );
    assert!(
        fx.env
            .query::<Option<String>>("SELECT expired_at FROM escalations WHERE id = 'api-a1-q1'")
            .is_some()
    );
}

#[test]
fn a_running_attempt_in_process_has_its_event_its_driver_pid_and_the_lock_of_a_live_driver() {
    let fx = engine("");
    fx.engine_plan();
    let lock = fx.live_driver("api-a1");
    fx.add_running("api", "running", &fx.stamp_ago(1_000));
    fx.add_running("web", "starting", &fx.stamp_ago(1_000));

    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM jobs WHERE key = 'api'"),
        "running"
    );
    assert_eq!(
        fx.env
            .query::<i64>("SELECT driver_pid FROM attempts WHERE id = 'api-a1'"),
        4242
    );
    assert_eq!(
        fx.events("attempt-status").len(),
        1,
        "a starting Attempt has no starting → running Event"
    );
    let path = fx.home().attempt_lock(
        &PlanId::parse(PLAN).unwrap(),
        &AttemptId::parse("api-a1").unwrap(),
    );
    let second = Flock::lock(
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap(),
        FlockArg::LockExclusiveNonblock,
    );
    assert!(second.is_err(), "the held lock refuses a second one");
    drop(lock);
}

// ---------- E2 ----------

/// The time `minutes` minutes before the engine's clock.
fn ago_in(fx: &Ip, minutes: i64) -> String {
    fx.stamp_ago(minutes * 60_000)
}

/// [`engine`] with Plan `pay` (`fx.engine_plan()`).
fn engine_with_plan(config_extra: &str) -> Ip {
    let fx = engine(config_extra);
    fx.engine_plan();
    fx
}

#[test]
fn e2_ends_attempts_whose_driver_died_crashed_or_cancelled() {
    let fx = engine_with_plan("retry_cap = 0");
    fx.add_running("api", "running", &ago_in(&fx, 1));
    fx.add_running("web", "starting", &ago_in(&fx, 1));
    fx.request_cancel("web");

    fx.watch_first_tick().unwrap();

    assert_eq!(
        fx.attempt("api-a1"),
        (
            "ended".into(),
            Some("failed".into()),
            Some("crashed".into()),
            None
        )
    );
    assert_eq!(fx.job_status("api"), "failed");
    let failed = fx.events("job-failed");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].3["cause"], "crashed");
    assert_eq!(failed[0].2.as_deref(), Some("api-a1"));
    assert_eq!(
        fx.attempt("web-a1"),
        (
            "ended".into(),
            Some("cancelled".into()),
            Some("cancel_requested".into()),
            None
        )
    );
    assert_eq!(fx.job_status("web"), "cancelled");
    assert_eq!(
        fx.events("job-cancelled")[0].3,
        json!({"reason": "not needed"})
    );
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

#[test]
fn e2_removes_the_lock_of_a_dead_driver_it_ends_and_does_not_recreate_it() {
    let fx = engine_with_plan("retry_cap = 0");
    // `api-a1`: a dead driver left its lock file, free. `web-a1`: the file is missing, as an ended
    // Attempt's is (E2's probe must not create it: no driver).
    drop(fx.live_driver("api-a1"));
    fx.add_running("api", "running", &ago_in(&fx, 1));
    fx.add_running("web", "running", &ago_in(&fx, 1));
    let lock = |attempt: &str| {
        fx.home().attempt_lock(
            &PlanId::parse(PLAN).unwrap(),
            &AttemptId::parse(attempt).unwrap(),
        )
    };
    assert!(lock("api-a1").exists() && !lock("web-a1").exists());

    // The lock goes with the end, and the next ticks do not bring it back.
    fx.watch_ticks(OVER_ONE_TICK).unwrap();

    assert_eq!(
        (fx.attempt("api-a1").0, fx.attempt("web-a1").0),
        ("ended".into(), "ended".into())
    );
    assert!(!lock("api-a1").exists() && !lock("web-a1").exists());
    let left = fs::read_dir(fx.home().locks_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    assert!(
        left.iter()
            .all(|n| !n.to_string_lossy().starts_with("attempt-")),
        "{left:?}"
    );
}

#[test]
fn e2_retries_a_dead_driver_at_the_same_tier() {
    let fx = engine_with_plan("concurrency = 1");
    // A live Attempt holds the only slot; `api` is a tenant already, so E3 starts its next Attempt at once, competing for nothing.
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago_in(&fx, 1));
    fx.add_running("api", "running", &ago_in(&fx, 1));

    fx.watch_first_tick().unwrap();

    assert_eq!(fx.attempt("api-a1").0, "ended");
    // The Attempt E2 ended is retried at the same Tier once; the next one started at once (this engine has no installed binary, so its driver
    // could not start: its own `attempt-retry`, which is not E2's).
    let retry: Vec<_> = fx
        .events("attempt-retry")
        .into_iter()
        .filter(|e| e.2.as_deref() == Some("api-a1"))
        .collect();
    assert_eq!(retry.len(), 1);
    assert_eq!(
        (&retry[0].3["cause"], &retry[0].3["next_tier"]),
        (&json!("crashed"), &json!("S2"))
    );
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM attempts WHERE job = 'api' AND number <= 2"),
        2
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT tier FROM attempts WHERE id = 'api-a2'"),
        "S2"
    );
}

/// hs-n465, hs-d632: the catalog refuses the Tier of an Attempt whose driver died (`S2` when
/// `S` supports 0,1,3,5). E2 ends it and requeues the Job at the next valid Tier in the Worker
/// range at or above its effort (`S3`), never wedging with `tier_invalid` and leaving it `starting`.
#[test]
fn e2_ends_an_attempt_whose_tier_the_catalog_refuses_and_requeues_at_the_next_valid_tier() {
    let fx = engine_with_plan("concurrency = 1");
    fx.env
        .sql("UPDATE tier_catalog SET efforts = '[0,1,3,5]' WHERE letter = 'S';");
    // A live Attempt holds the only slot; `api` is a tenant already, so E3 starts its next Attempt after E2 queues it, competing for nothing.
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago_in(&fx, 1));
    fx.add_running("api", "starting", &ago_in(&fx, 1));

    fx.watch_first_tick().unwrap();

    assert_ne!(
        fx.attempt("api-a1").0,
        "starting",
        "the Attempt stayed starting"
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT tier FROM attempts WHERE id = 'api-a2'"),
        "S3",
        "the tenant Job's next Attempt started at once, at S3"
    );
    // The Attempt E2 ended climbs the ladder once (the next one's driver could not start, with no installed binary: its own `attempt-retry`).
    let retry: Vec<_> = fx
        .events("attempt-retry")
        .into_iter()
        .filter(|e| e.2.as_deref() == Some("api-a1"))
        .collect();
    assert_eq!(retry.len(), 1, "{retry:?}");
    assert_eq!(
        (
            retry[0].1.as_deref(),
            retry[0].2.as_deref(),
            &retry[0].3["cause"],
            &retry[0].3["next_tier"]
        ),
        (Some("api"), Some("api-a1"), &json!("crashed"), &json!("S3"))
    );
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'error' AND json_extract(data, '$.code') = 'tier_invalid'"),
        0,
        "{:?}",
        fx.texts("error")
    );
}

#[test]
fn e2_asks_live_drivers_to_stop_on_timeout_or_cancel_with_escalations_pausing_the_clock() {
    let fx = engine_with_plan("");
    let _locks: Vec<_> = ["late-a1", "gone-a1", "asked-a1", "fresh-a1"]
        .iter()
        .map(|a| fx.live_driver(a))
        .collect();
    fx.add_running("late", "running", &ago_in(&fx, 31));
    fx.add_running("gone", "running", &ago_in(&fx, 1));
    fx.request_cancel("gone");
    // 40 minutes, 20 of them waiting for answers (two overlapping Escalations count once).
    fx.add_running("asked", "running", &ago_in(&fx, 40));
    fx.env.sql(&format!(
        "INSERT INTO escalations (plan, id, attempt, number, question, status, answer, asker_pid, asked_at, answered_at)
           VALUES ('pay', 'asked-a1-q1', 'asked-a1', 1, 'Q?', 'answered', 'A.', 1, '{}', '{}'),
                  ('pay', 'asked-a1-q2', 'asked-a1', 2, 'Q?', 'answered', 'A.', 1, '{}', '{}');",
        ago_in(&fx, 30),
        ago_in(&fx, 15),
        ago_in(&fx, 25),
        ago_in(&fx, 10)
    ));
    fx.add_running("fresh", "running", &ago_in(&fx, 1));
    // 40 minutes since it started, 39 of them waiting for its Lead in `starting` (engine-W11): 1 minute active.
    fx.add_running_since("waited", "running", &ago_in(&fx, 40), Some(&ago_in(&fx, 1)));
    let _waited = fx.live_driver("waited-a1");

    // More than one tick, for the Attempts it must leave alone.
    fx.watch_ticks(OVER_ONE_TICK).unwrap();

    assert_eq!(
        fx.attempt("late-a1"),
        ("running".into(), None, None, Some("timed_out".into()))
    );
    assert_eq!(
        fx.attempt("gone-a1"),
        ("running".into(), None, None, Some("cancelled".into()))
    );
    assert_eq!(fx.attempt("asked-a1"), ("running".into(), None, None, None));
    assert_eq!(fx.attempt("fresh-a1"), ("running".into(), None, None, None));
    assert_eq!(
        fx.attempt("waited-a1"),
        ("running".into(), None, None, None)
    );
    // A data write for the driver: no Event of its own.
    assert_eq!(
        fx.count("job-cancelled") + fx.count("job-failed") + fx.count("error"),
        0,
        "{:?}",
        fx.texts("error")
    );
}

/// `attempt-status` check Events of `<key>-a1`: `(at, "start" | "end")`, in order.
fn check_events(fx: &Ip, key: &str, edges: &[(&str, &str)]) {
    for (at, edge) in edges {
        fx.env.sql(&format!(
            "INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
               VALUES (cpd_txn(), '{at}', 'attempt-status', 'ctx', 'pay', '{key}', '{key}-a1', 'check',
                       '{{\"from\":\"running\",\"to\":\"running\",\"check\":\"{edge}\",\"what\":\"tests\"}}');"
        ));
    }
}

#[test]
fn e2_does_not_count_a_check_span_toward_the_attempts_timeout() {
    // hs-d497, contract §7.7: 31 minutes since `running`, 25 of them in a Gate (one closed span, one still open): not over 30 active.
    let fx = engine_with_plan("");
    let _locks: Vec<_> = ["spent-a1", "over-a1"]
        .iter()
        .map(|a| fx.live_driver(a))
        .collect();
    fx.add_running("spent", "running", &ago_in(&fx, 31));
    check_events(
        &fx,
        "spent",
        &[
            (&ago_in(&fx, 30), "start"),
            (&ago_in(&fx, 20), "end"),
            (&ago_in(&fx, 15), "start"),
        ],
    );
    // The same time with a span of one minute: 31 minutes active (the clock is virtual: no time passes while the test sets up), which is over.
    fx.add_running("over", "running", &ago_in(&fx, 32));
    check_events(
        &fx,
        "over",
        &[(&ago_in(&fx, 10), "start"), (&ago_in(&fx, 9), "end")],
    );

    fx.watch_first_tick().unwrap();

    assert_eq!(fx.attempt("over-a1").3.as_deref(), Some("timed_out"));
    assert_eq!(
        fx.attempt("spent-a1"),
        ("running".into(), None, None, None),
        "a span still open is not active time either"
    );
}

#[test]
fn e2_closes_a_check_span_whose_submit_died_and_leaves_one_that_is_held() {
    let fx = engine_with_plan("");
    let _drivers: Vec<_> = ["dead-a1", "held-a1", "done-a1"]
        .iter()
        .map(|a| fx.live_driver(a))
        .collect();
    for key in ["dead", "held", "done"] {
        fx.add_running(key, "running", &ago_in(&fx, 5));
    }
    check_events(&fx, "dead", &[(&ago_in(&fx, 4), "start")]);
    check_events(&fx, "held", &[(&ago_in(&fx, 4), "start")]);
    check_events(
        &fx,
        "done",
        &[(&ago_in(&fx, 4), "start"), (&ago_in(&fx, 3), "end")],
    );
    // `held`'s submit is alive: it holds the check lock.
    let path = fx.home().check_lock(
        &PlanId::parse(PLAN).unwrap(),
        &AttemptId::parse("held-a1").unwrap(),
    );
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    let _submit = Flock::lock(file, FlockArg::LockExclusiveNonblock)
        .map_err(|(_, e)| e)
        .unwrap();

    fx.watch_ticks(OVER_ONE_TICK).unwrap();

    let ends: Vec<_> = fx
        .events("attempt-status")
        .into_iter()
        .filter(|e| e.3["died"] == json!(true))
        .collect();
    assert_eq!(
        ends.len(),
        1,
        "once, and only for the span nobody holds: {ends:?}"
    );
    assert_eq!(
        (ends[0].1.as_deref(), ends[0].2.as_deref(), &ends[0].3),
        (
            Some("dead"),
            Some("dead-a1"),
            &json!({"from": "running", "to": "running", "check": "end", "what": "tests", "died": true})
        )
    );
}

/// The engine of Plan `pay` on a repo and a Lead worktree of the fake git, with the fake tmux server of `fx.tmux(..)`
/// ([`engine_on_tmux`], no server yet): what E2's merge recovery (R2, R3) and a Worker's window need.
fn engine_on_git() -> (Ip, PathBuf, PathBuf) {
    let fx = engine_home("");
    fx.engine_config_on_tmux("");
    fx.engine_routes_on_tmux();
    let repo = fx.git_repo("pay").canonicalize().unwrap();
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("lead.log"),
    };
    let created = fx
        .scoped(|| create_lead_worktree(&repo, &PlanId::parse(PLAN).unwrap(), "main", &setup))
        .unwrap();
    fx.engine_plan_on(&repo, &created.path, &created.branch);
    (fx, repo, created.path)
}

/// Attempt `api-a1` of Plan `pay` in `merging`, its worktree cut from the Lead Branch with one commit, its driver
/// dead. With `moved` the driver had moved the Lead Branch (step 8) and synced the Lead worktree before it died.
/// Returns the worktree and the tip.
fn merging_api(fx: &Ip, repo: &Path, lead: &Path, moved: bool) -> (git::Created, String) {
    let attempt = AttemptId::parse("api-a1").unwrap();
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("attempt.log"),
    };
    let made = fx
        .scoped(|| {
            create_attempt_worktree(repo, lead, &PlanId::parse(PLAN).unwrap(), &attempt, &setup)
        })
        .unwrap();
    fx.commit_file(&made.path, "api.txt", "api\n", "api");
    let tip = fx.git(&made.path, &["rev-parse", "HEAD"]);
    if moved {
        fx.git(lead, &["merge", "-q", "--ff-only", &made.branch]);
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
            t = ago_in(fx, 1)
        ),
    );
    (made, tip)
}

#[test]
fn e2_recovers_a_merge_whose_driver_died_after_the_compare_and_swap() {
    let (fx, repo, lead) = engine_on_git();
    // The driver moved the Lead Branch (step 8) and synced the Lead worktree, then died.
    let (made, tip) = merging_api(&fx, &repo, &lead, true);

    fx.watch_first_tick().unwrap();

    assert_eq!(
        fx.attempt("api-a1"),
        (
            "ended".into(),
            Some("merged".into()),
            Some("merged".into()),
            None
        )
    );
    assert_eq!(fx.job_status("api"), "merged");
    assert_eq!(
        fx.events("job-merged")[0].3,
        json!({"sha": tip, "tier": "S2", "kind": "work", "turn": false})
    );
    assert!(!made.path.exists());
}

/// A window `pay--api-a1` in the work session, with another to keep the session alive.
fn worker_window(fx: &Ip) {
    for args in [
        &[
            "new-session",
            "-d",
            "-s",
            "cpd-work",
            "-n",
            "keep",
            "sleep",
            "600",
        ][..],
        &[
            "new-window",
            "-d",
            "-t",
            "=cpd-work:",
            "-n",
            "pay--api-a1",
            "sleep",
            "600",
        ][..],
    ] {
        let out = fx.tmux(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert!(worker_window_is_up(fx));
}

fn worker_window_is_up(fx: &Ip) -> bool {
    let out = fx.tmux(&["list-windows", "-a", "-F", "#{session_name}:#{window_name}"]);
    String::from_utf8_lossy(&out.stdout).contains("cpd-work:pay--api-a1")
}

/// The fake tmux's `kill-window` fails: the first call only, or every call; every other call is the route's. The flag is set
/// at the first failure.
fn failing_kill_window(fx: &Ip, every_call: bool) -> Arc<AtomicBool> {
    let failed = Arc::new(AtomicBool::new(false));
    let seen = failed.clone();
    let is_kill = move |call: &Call| {
        let hit = call.args.iter().any(|a| a == "kill-window");
        if hit {
            seen.store(true, Ordering::SeqCst);
        }
        hit
    };
    let reply = Reply::exit(1, "", "fake: kill-window failed\n");
    let _ = if every_call {
        (*fx.fakes.runner).clone().on("tmux", is_kill, reply)
    } else {
        (*fx.fakes.runner).clone().once("tmux", is_kill, reply)
    };
    failed
}

#[test]
fn e2_kills_the_window_of_a_dead_driver_it_ends_though_the_first_kill_window_fails() {
    let fx = engine_on_tmux_with("retry_cap = 0");
    fx.add_running("api", "running", &ago_in(&fx, 1));
    worker_window(&fx);
    let failed = failing_kill_window(&fx, false);

    fx.watch_ticks(OVER_ONE_TICK).unwrap();

    assert!(
        failed.load(Ordering::SeqCst),
        "the fake tmux never failed a kill-window"
    );
    assert!(
        fx.attempt("api-a1").0 == "ended" && !worker_window_is_up(&fx),
        "the attempt ended and its window gone"
    );
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

#[test]
fn r3_kills_the_window_of_a_merge_that_did_not_happen_though_the_first_kill_window_fails() {
    let (fx, repo, lead) = engine_on_git();
    let (made, _) = merging_api(&fx, &repo, &lead, false);
    worker_window(&fx);
    let failed = failing_kill_window(&fx, false);

    fx.watch_ticks(OVER_ONE_TICK).unwrap();

    assert!(
        failed.load(Ordering::SeqCst),
        "the fake tmux never failed a kill-window"
    );
    assert!(
        fx.attempt("api-a1").0 == "ended" && !worker_window_is_up(&fx),
        "the attempt ended and its window gone"
    );
    assert_eq!(fx.attempt("api-a1").2.as_deref(), Some("crashed"));
    // R3 keeps the worktree for debugging.
    assert!(made.path.exists());
}

#[test]
fn r2_removes_the_worktree_though_every_kill_window_fails() {
    let (fx, repo, lead) = engine_on_git();
    let (made, _) = merging_api(&fx, &repo, &lead, true);
    worker_window(&fx);
    let failed = failing_kill_window(&fx, true);

    fx.watch_ticks(OVER_ONE_TICK).unwrap();

    assert!(
        failed.load(Ordering::SeqCst),
        "the fake tmux never failed a kill-window"
    );
    assert!(
        fx.attempt("api-a1").0 == "ended" && !made.path.exists(),
        "the merge recorded and its worktree removed"
    );
    assert_eq!(fx.attempt("api-a1").1.as_deref(), Some("merged"));
}

/// A Session whose pane is dead or gone becomes `stopped` with no pane, and
/// its place `none` (Escalation e2e-a1-q1); a `stopped` Session keeps a live pane.
#[test]
fn e2_stops_sessions_whose_pane_is_dead_or_gone() {
    let fx = engine_on_tmux();
    let new = |args: &[&str]| {
        let out = fx.tmux(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let live = new(&[
        "new-session",
        "-d",
        "-s",
        "x",
        "-P",
        "-F",
        "#{pane_id}",
        "sleep",
        "600",
    ]);
    new(&["set", "-g", "remain-on-exit", "on"]);
    let waiting = new(&[
        "new-window",
        "-d",
        "-t",
        "=x:",
        "-P",
        "-F",
        "#{pane_id}",
        "sleep",
        "600",
    ]);
    let dead = new(&[
        "new-window",
        "-d",
        "-t",
        "=x:",
        "-P",
        "-F",
        "#{pane_id}",
        "sh",
        "-c",
        "exit 3",
    ]);
    fx.pane_exit(&dead, 3);
    assert_eq!(new(&["list-panes", "-t", &dead, "-F", "#{pane_dead}"]), "1");
    fx.env.sql(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
           VALUES ('s-live', 'lead', 'ctx', 'pay', 'Live', '/', 'O3', '{live}', 'idle', NULL, 't', 't'),
                  ('s-dead', 'lead', 'ctx', 'pay', 'Dead', '/', 'O3', '{dead}', 'blocked', 'cpd: dialog', 't', 't'),
                  ('s-gone', 'lead', 'ctx', NULL, 'Gone', '/', 'O3', '%999', 'working', NULL, 't', 't'),
                  ('s-waiting', 'lead', 'ctx', 'pay', 'Waiting', '/', 'O3', '{waiting}', 'stopped', NULL, 't', 't'),
                  ('s-shut', 'lead', 'ctx', 'pay', 'Shut', '/', 'O3', '%998', 'stopped', NULL, 't', 't');"
    ));

    fx.watch_ticks(OVER_ONE_TICK).unwrap();

    let row = |id: &str| {
        fx.env.read(|c| {
            c.query_row(
                "SELECT state, tmux_pane, state_at FROM sessions WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
        })
    };
    let rows: Vec<(String, Option<String>, String)> =
        ["s-live", "s-dead", "s-gone", "s-waiting", "s-shut"]
            .iter()
            .map(|id| row(id))
            .collect();
    let (states, panes): (Vec<&str>, Vec<Option<&str>>) =
        rows.iter().map(|r| (r.0.as_str(), r.1.as_deref())).unzip();
    assert_eq!(states, ["idle", "stopped", "stopped", "stopped", "stopped"]);
    assert_eq!(
        panes,
        [
            Some(live.as_str()),
            None,
            None,
            Some(waiting.as_str()),
            None
        ]
    );
    // Already stopped: its state did not change, so neither did state_at.
    assert_eq!(rows[4].2, "t");
    assert_eq!(
        fx.env
            .query::<Option<String>>("SELECT state_label FROM sessions WHERE id = 's-dead'"),
        None
    );
    let events = fx.events("session-state");
    assert_eq!(events.len(), 2);
    assert!(
        events.contains(&(
            Some("pay".into()),
            None,
            None,
            json!({"from": "blocked", "to": "stopped", "label": null})
        )),
        "{events:?}"
    );
    assert!(
        events.contains(&(
            None,
            None,
            None,
            json!({"from": "working", "to": "stopped", "label": null})
        )),
        "{events:?}"
    );
    let moved = fx.events("session-moved");
    let none = json!({"pane": null, "window": null, "place": "none"});
    assert_eq!(
        moved,
        [
            (Some("pay".into()), None, None, none.clone()),
            (None, None, None, none.clone()),
            (Some("pay".into()), None, None, none)
        ]
    );
    let places: Vec<String> = fx.cpd(&["session", "list"]).ok()["sessions"]
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
    let fx = engine_on_tmux();
    let new = |args: &[&str]| {
        let out = fx.tmux(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let pane = new(&[
        "new-session",
        "-d",
        "-s",
        "x",
        "-P",
        "-F",
        "#{pane_id}",
        "sleep",
        "600",
    ]);
    new(&["set", "-p", "-t", &pane, "@cpd_view", "hologram"]);
    fx.env.sql(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, tmux_pane, state, state_at, created_at)
           VALUES ('s-gone', 'lead', 'ctx', 'old', 'Gone', '/', 'O3', '%999', 'working', 't', 't');
         -- A done Plan's Lead is not restarted (A4): the restart is tested in session.rs, this test is about the unknown view.
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('old', 'ctx', 'Old', 'done', 's-gone', 't', 't', 't');",
    );

    // The product prints the unknown view to its stderr, once per value and process.
    let (ran, stderr) = fx.with_stderr(|| fx.watch_ticks(OVER_ONE_TICK));
    ran.unwrap();

    assert_eq!(fx.count("session-moved"), 1);
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
    assert_eq!(
        fx.env
            .query::<String>("SELECT state FROM sessions WHERE id = 's-gone'"),
        "stopped"
    );
    assert_eq!(
        stderr.matches("hologram").count(),
        1,
        "logged once: {stderr}"
    );
}

// ---------- E3 ----------

/// The engine in-process for E3: Plan `pay`, one slot (`concurrency = 1`, `retry_cap = 0`) and `config_extra`. The drivers E3 starts
/// are launches of the `FakeSpawn` (nothing runs; the driver's own tests are in flow): its `self_exe` is a file that exists, which
/// `driver::start` checks before it spawns.
fn engine_scheduling(config_extra: &str) -> Ip {
    let fx = engine_with_plan(&format!("concurrency = 1\nretry_cap = 0\n{config_extra}"));
    let exe = fx.env.root().join("cpd-installed");
    fs::write(&exe, "").unwrap();
    fx.fakes.spawn.set_self_exe(&exe);
    // E5 runs `<exe> pr poll` at every tick, from the recorded path: it finds no pull request.
    let poll = json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}});
    fx.fake("cpd-installed", Reply::ok(&poll.to_string()));
    fx
}

/// The `Launch` of Attempt `attempt`'s driver the `FakeSpawn` recorded, with the pid the Attempt stores (a process of the
/// `FakeSpawn`, alive until the test says it exited): `<exe> attempt run <attempt> --plan pay`, detached, with the lock as fd 3
/// and the log of the Attempt.
fn driver_launch(fx: &Ip, attempt: &str) -> (Launch, SpawnPid) {
    let launch = fx
        .fakes
        .spawn
        .launches()
        .into_iter()
        .find(|l| l.args.get(2).map(String::as_str) == Some(attempt))
        .unwrap_or_else(|| {
            panic!(
                "no driver was launched for {attempt}: {:?}",
                fx.fakes.spawn.launches()
            )
        });
    let exe = fx.fakes.spawn.self_exe().unwrap();
    assert_eq!(
        (launch.program.as_str(), launch.args.as_slice()),
        (
            exe.to_str().unwrap(),
            ["attempt", "run", attempt, "--plan", PLAN]
                .map(str::to_owned)
                .as_slice()
        )
    );
    assert_eq!(
        launch.env_set,
        [(
            "CPD_HOME".to_owned(),
            fx.env.cpd_home().display().to_string()
        )]
    );
    assert_eq!(
        launch.stdout_to,
        Some(fx.home().driver_log(
            &PlanId::parse(PLAN).unwrap(),
            &AttemptId::parse(attempt).unwrap()
        ))
    );
    assert!(
        launch.setsid && launch.hand_over.is_some() && !launch.foreground,
        "{launch:?}"
    );
    let pid = SpawnPid(
        u32::try_from(driver_pid(fx, attempt).expect("the Attempt has a driver pid")).unwrap(),
    );
    assert!(
        fx.fakes.spawn.alive(pid),
        "the driver of {attempt} is a running process of the FakeSpawn"
    );
    (launch, pid)
}

/// The pid stored as Attempt `id`'s driver.
fn driver_pid(fx: &Ip, id: &str) -> Option<i64> {
    fx.env.query::<Option<i64>>(&format!(
        "SELECT driver_pid FROM attempts WHERE id = '{id}'"
    ))
}

#[test]
fn e3_one_tick_schedules_a_ready_job_within_the_concurrency() {
    let fx = engine_scheduling("");
    fx.add_job("api", &[], "");
    fx.add_job("web", &[], "");
    fx.add_job("ui", &["api"], "");

    fx.watch_first_tick().unwrap();

    let started = fx.events("attempt-started");
    assert_eq!(
        started,
        [(
            Some("pay".into()),
            Some("api".into()),
            Some("api-a1".into()),
            json!({"number": 1, "tier": "S2", "origin": "ladder"})
        )]
    );
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts"), 1);
    assert_eq!(
        (fx.job_status("web"), fx.job_status("ui")),
        ("queued".into(), "queued".into())
    );
    assert!(driver_pid(&fx, "api-a1").is_some());
    // One driver was launched, and its pid is the one stored; it runs until the test says it ended.
    assert_eq!(fx.fakes.spawn.launches_but_janitor().len(), 1);
    let (_, pid) = driver_launch(&fx, "api-a1");
    // Its driver exits at once with no lock held and no Event: the next tick ends the Attempt crashed.
    fx.fakes.spawn.exit(pid, 1);
    fx.watch_first_tick().unwrap();
    assert_eq!(fx.attempt("api-a1").0, "ended");
}

/// hs-n465, hs-d632: a stored Job whose Tier the catalog refuses (`S2` when `S` supports
/// 0,1,3,5) is started at the next valid Tier in the Worker range at or above its effort
/// (`S3`), never wedging with `tier_invalid`.
#[test]
fn e3_starts_a_job_whose_stored_tier_the_catalog_refuses_at_the_next_valid_tier() {
    let fx = engine_scheduling("");
    fx.env
        .sql("UPDATE tier_catalog SET efforts = '[0,1,3,5]' WHERE letter = 'S';");
    fx.add_job("api", &[], "");

    fx.watch_first_tick().unwrap();

    let started = fx.events("attempt-started");
    assert_eq!(
        started,
        [(
            Some("pay".into()),
            Some("api".into()),
            Some("api-a1".into()),
            json!({"number": 1, "tier": "S3", "origin": "ladder"})
        )]
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT tier FROM attempts WHERE id = 'api-a1'"),
        "S3"
    );
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts"), 1);
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'error' AND json_extract(data, '$.code') = 'tier_invalid'"),
        0,
        "{:?}",
        fx.texts("error")
    );
    // Its driver was launched; it exits at once and the next tick ends the Attempt crashed.
    let (_, pid) = driver_launch(&fx, "api-a1");
    fx.fakes.spawn.exit(pid, 1);
    fx.watch_first_tick().unwrap();
    assert_eq!(fx.attempt("api-a1").0, "ended");
}

/// A Job stored on a Tier of the worker range (H3..O1 on the default roles GX / H3 / O2 / O4 and this catalog) starts at that Tier.
#[test]
fn e3_starts_a_job_stored_on_a_tier_of_the_worker_range_at_that_tier() {
    for tier in ["H3", "S1", "S3"] {
        let fx = engine_scheduling("");
        fx.add_job_on(tier, "repo", "api", &[], "");

        fx.watch_first_tick().unwrap();

        let started = fx.events("attempt-started");
        assert_eq!(
            started,
            [(
                Some("pay".into()),
                Some("api".into()),
                Some("api-a1".into()),
                json!({"number": 1, "tier": tier, "origin": "ladder"})
            )],
            "{tier}"
        );
        assert_eq!(
            fx.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'error' AND json_extract(data, '$.code') = 'tier_not_allowed'"),
            0,
            "{tier}: {:?}",
            fx.texts("error")
        );
        // Its driver was launched; it exits at once and the next tick ends the Attempt crashed.
        let (_, pid) = driver_launch(&fx, "api-a1");
        fx.fakes.spawn.exit(pid, 1);
        fx.watch_first_tick().unwrap();
    }
}

/// The roles GX / H3 / `lead` / O3 on the catalog H [0-4], S [0,1,3,4], O [0,2,3,4], G []: the worker range is H3..S3
/// for `lead` = S4 and H3..S1 for S2. S has no effort 2, so `S2` is a Tier the catalog refuses.
fn ranged_roles(lead: &str) -> String {
    format!(
        "context_distill_tier = \"GX\"\nworker_tier = \"H3\"\nlead_tier = \"{lead}\"\nmanager_tier = \"O3\"\n"
    )
}

/// [`ranged_roles`]' catalog, over the engine Plan's.
fn ranged_catalog(fx: &Ip) {
    fx.env.sql(
        "UPDATE tier_catalog SET efforts = '[0,1,2,3,4]' WHERE letter = 'H';
         UPDATE tier_catalog SET efforts = '[0,1,3,4]' WHERE letter = 'S';
         UPDATE tier_catalog SET efforts = '[0,2,3,4]' WHERE letter = 'O';",
    );
}

/// The Job keys E3 raised a `tier_not_allowed` `error` Event for, with how many each got.
fn tier_not_allowed_errors(fx: &Ip) -> Vec<(String, i64)> {
    fx.env.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT job, count(*) FROM events WHERE kind = 'error' AND json_extract(data, '$.code') = 'tier_not_allowed' GROUP BY job ORDER BY job",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()
    })
}

/// A stored Job Tier outside the worker range is never started: one `error` Event per Job (`tier_not_allowed`), the Job stays
/// queued with no Attempt, and E3 goes on to the Jobs in range.
#[test]
fn e3_never_starts_a_job_stored_outside_the_worker_range() {
    {
        let fx = engine_scheduling(&ranged_roles("S4"));
        ranged_catalog(&fx);
        // Outside H3..S3: below the floor, above the ceiling, another letter.
        fx.add_job_on("H2", "repo", "low", &[], "");
        fx.add_job_on("S4", "repo", "high", &[], "");
        fx.add_job_on("O2", "repo", "other", &[], "");
        fx.add_job_on("S3", "repo", "ok", &[], "");

        fx.watch_first_tick().unwrap();

        assert_eq!(
            tier_not_allowed_errors(&fx),
            [("high".into(), 1), ("low".into(), 1), ("other".into(), 1)],
            "{:?}",
            fx.texts("error")
        );
        for key in ["low", "high", "other"] {
            assert_eq!(fx.job_status(key), "queued", "{key}");
        }
        assert_eq!(
            fx.env
                .query::<String>("SELECT group_concat(job) FROM attempts"),
            "ok",
            "only the Job in range started"
        );
        assert_eq!(
            fx.events("attempt-started")[0].3["tier"],
            "S3",
            "the ceiling is in range"
        );
        let (_, pid) = driver_launch(&fx, "ok-a1");
        fx.fakes.spawn.exit(pid, 1);
        fx.watch_first_tick().unwrap();
    }

    // A second config moves the ceiling: with lead_tier = S2 the range is H3..S1 and a Job on S3 is refused.
    let fx = engine_scheduling(&ranged_roles("S2"));
    ranged_catalog(&fx);
    fx.add_job_on("S3", "repo", "high", &[], "");
    fx.add_job_on("S1", "repo", "ok", &[], "");
    fx.watch_first_tick().unwrap();
    assert_eq!(tier_not_allowed_errors(&fx), [("high".into(), 1)]);
    assert_eq!(fx.job_status("high"), "queued");
    assert_eq!(
        fx.env
            .query::<String>("SELECT group_concat(job) FROM attempts"),
        "ok"
    );
    let (_, pid) = driver_launch(&fx, "ok-a1");
    fx.fakes.spawn.exit(pid, 1);
    fx.watch_first_tick().unwrap();
}

/// E2 steps an Attempt whose Tier the catalog refuses (S2) to the next valid rung of the worker range (S3); at the ceiling
/// there is none: with lead_tier = S2 the range ends at S1, the Job fails and nothing is retried above it.
#[test]
fn e2_ladder_climbs_inside_the_worker_range_and_fails_at_its_ceiling() {
    {
        let fx = engine_with_plan(&format!(
            "concurrency = 1\nretry_cap = 3\n{}",
            ranged_roles("S4")
        ));
        ranged_catalog(&fx);
        let _busy = fx.live_driver("busy-a1");
        fx.add_running("busy", "running", &ago_in(&fx, 1));
        fx.add_running("api", "starting", &ago_in(&fx, 1));
        fx.watch_first_tick().unwrap();
        assert_ne!(fx.attempt("api-a1").0, "starting");
        assert_eq!(
            fx.env
                .query::<String>("SELECT tier FROM attempts WHERE id = 'api-a2'"),
            "S3",
            "the next valid rung inside H3..S3"
        );
    }

    let fx = engine_with_plan(&format!(
        "concurrency = 1\nretry_cap = 3\n{}",
        ranged_roles("S2")
    ));
    ranged_catalog(&fx);
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago_in(&fx, 1));
    fx.add_running("api", "starting", &ago_in(&fx, 1));
    fx.watch_first_tick().unwrap();
    assert_eq!(fx.attempt("api-a1").0, "ended");
    assert_eq!(
        fx.job_status("api"),
        "failed",
        "no rung of H3..S1 at or above S2"
    );
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM attempts WHERE job = 'api' AND number > 1"),
        0,
        "no Attempt above the ceiling"
    );
    assert_eq!(fx.events("job-failed").len(), 1);
}

/// SQL that marks Job `key` urgent, for `add_job`'s `then`.
fn urgent(key: &str) -> String {
    format!("UPDATE jobs SET urgent = 1 WHERE plan = 'pay' AND key = '{key}';")
}

/// E3 starts urgent Ready Jobs first (§3.7): with every slot free, the urgent Job goes before an older non-urgent one.
#[test]
fn e3_starts_an_urgent_job_before_an_older_one() {
    let fx = engine_scheduling("");
    fx.add_job("old", &[], "");
    fx.add_job("hot", &[], &urgent("hot"));

    fx.watch_first_tick().unwrap();
    // The Worker stays alive: its driver holds the Attempt's lock. Two more ticks: the one slot is taken, so the older Job still waits.
    let _driver = fx.live_driver("hot-a1");
    fx.watch_ticks(2 * OVER_ONE_TICK).unwrap();

    let started: Vec<Option<String>> = fx
        .events("attempt-started")
        .into_iter()
        .map(|e| e.1)
        .collect();
    assert_eq!(started, [Some("hot".to_owned())]);
    assert_eq!(
        (fx.job_status("hot"), fx.job_status("old")),
        ("running".into(), "queued".into())
    );
    assert_eq!(fx.fakes.spawn.launches_but_janitor().len(), 1);
}

/// An urgent Job may start one Attempt beyond `concurrency`, and no more; the extra slot is never a non-urgent Job's.
#[test]
fn e3_starts_one_urgent_job_beyond_the_concurrency_and_not_a_second() {
    let fx = engine_scheduling("");
    // One Attempt is running: the only slot is taken.
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &ago_in(&fx, 1));
    fx.add_job("old", &[], "");
    fx.add_job("hot1", &[], &urgent("hot1"));
    fx.add_job("hot2", &[], &urgent("hot2"));

    fx.watch_first_tick().unwrap();
    // Its driver stays alive. Two more ticks: the second urgent Job and the older one still wait.
    let _driver = fx.live_driver("hot1-a1");
    fx.watch_ticks(2 * OVER_ONE_TICK).unwrap();

    // The first urgent Job by `seq` started (running Attempts: 2 = concurrency + 1); the second urgent one and the older Job wait.
    let started: Vec<Option<String>> = fx
        .events("attempt-started")
        .into_iter()
        .map(|e| e.1)
        .collect();
    assert_eq!(started, [Some("hot1".to_owned())]);
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM attempts WHERE status <> 'ended'"),
        2
    );
    assert_eq!(
        (
            fx.job_status("hot1"),
            fx.job_status("hot2"),
            fx.job_status("old")
        ),
        ("running".into(), "queued".into(), "queued".into())
    );
    assert_eq!(fx.fakes.spawn.launches_but_janitor().len(), 1);
}

/// The 00:02Z incident (hs-d151): the file the engine runs from is replaced, then
/// E3 starts an Attempt. The driver starts from the path recorded at start
/// (`cannot run <path> (deleted)` before). The recorded path is `Spawn::self_exe`: the launch's program is that path, whatever
/// the file at it became.
#[test]
fn e3_starts_the_driver_after_the_engines_binary_is_replaced() {
    let fx = engine_scheduling("");
    let installed = fx.env.root().join("cpd-installed");
    assert_eq!(fx.fakes.spawn.self_exe().unwrap(), installed);

    let new = fx.env.root().join("cpd-new");
    fs::write(&new, "").unwrap();
    fs::rename(&new, &installed).unwrap();
    fx.add_job("api", &[], "");

    fx.watch_first_tick().unwrap();

    assert_eq!(fx.count("attempt-started"), 1);
    let (launch, pid) = driver_launch(&fx, "api-a1");
    assert_eq!(
        launch.program,
        installed.display().to_string(),
        "the driver starts from the recorded path"
    );
    fx.fakes.spawn.exit(pid, 1);
    fx.watch_first_tick().unwrap();
    assert_eq!(fx.attempt("api-a1").0, "ended");
    let texts = fx.texts("attempt-ended");
    assert!(
        texts
            .iter()
            .all(|t| !t.contains("cannot run") && !t.contains("deleted")),
        "{texts:?}"
    );
    assert!(!fx.texts("error").iter().any(|t| t.contains("deleted")));
}

/// The 00:02Z incident, reproduced: the engine cannot run the driver (its file is gone), so every Attempt
/// ends `failed` / `crashed` with `driver_pid` null. No Tier is climbed, no try is used up (`retry_cap = 0`
/// would fail the Job at the first), and E3 waits `2^k` seconds after the k-th failure: the engine's virtual clock is set to the
/// instant before and at the end of each wait (the Attempt's `ended_at` is the store's own stamp, read back).
#[test]
fn e3_spawn_failures_requeue_at_the_same_tier_never_fail_the_job_and_back_off() {
    let fx = engine_scheduling("");
    fs::remove_file(fx.env.root().join("cpd-installed")).unwrap();
    fx.add_job("api", &[], "");
    let ended_at = |n: u32| {
        parse_time(&fx.env.query::<String>(&format!(
            "SELECT ended_at FROM attempts WHERE id = 'api-a{n}'"
        )))
    };
    // The virtual clock reads the instant `at` (`Clock::utc` is the epoch of the `ManualClock` plus what elapsed), if it is not past it.
    let set_clock_to = |at: OffsetDateTime| {
        if let Ok(forward) = Duration::try_from(at - fx.fakes.clock.utc()) {
            fx.advance(forward);
            // The jump is months of virtual time, past the week E14 waits: the list was polled just now.
            fx.env.quiet_models(fx.fakes.clock.utc());
        }
    };

    fx.watch_first_tick().unwrap();
    for k in 1..=3u32 {
        assert_eq!(
            fx.env.query::<i64>("SELECT count(*) FROM attempts"),
            i64::from(k)
        );
        // After the k-th failure the next start waits 2^k seconds: one millisecond short of it nothing starts,
        let wait = time::Duration::seconds(2i64.pow(k));
        set_clock_to(ended_at(k) + wait - time::Duration::milliseconds(1));
        fx.watch_first_tick().unwrap();
        assert_eq!(
            fx.env.query::<i64>("SELECT count(*) FROM attempts"),
            i64::from(k),
            "started within the back-off after failure {k}"
        );
        // and a tick later (the engine's nap passed a tenth of a second) the next Attempt starts and fails the same way.
        set_clock_to(ended_at(k) + wait);
        fx.watch_first_tick().unwrap();
    }

    assert_ne!(fx.job_status("api"), "failed");
    assert_eq!(fx.count("job-failed"), 0);
    let attempts: Vec<(String, String, String, Option<i64>)> = fx.env.read(|conn| {
        let mut stmt = conn.prepare("SELECT id, tier, origin, driver_pid FROM attempts WHERE job = 'api' AND status = 'ended' ORDER BY number")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()
    });
    assert_eq!(attempts.len(), 4, "{attempts:?}");
    for (id, tier, origin, pid) in &attempts {
        assert_eq!(
            (tier.as_str(), origin.as_str(), *pid),
            ("S2", "ladder", None),
            "{id}"
        );
    }
    for (i, (id, ..)) in attempts.iter().enumerate() {
        assert_eq!(
            fx.attempt(id),
            (
                "ended".into(),
                Some("failed".into()),
                Some("crashed".into()),
                None
            )
        );
        let retry = &fx.events("attempt-retry")[i];
        assert_eq!(
            (&retry.3["cause"], &retry.3["next_tier"]),
            (&json!("crashed"), &json!("S2"))
        );
    }
    assert_eq!(
        fx.fakes.spawn.launches_but_janitor(),
        [],
        "no driver was started"
    );
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
    // `web`: spawn failure, then a started driver that died: one try used, the same Tier.
    fx.add_job(
        "web",
        &[],
        &(ended_attempt("web", 1, "S2", None, &ago(2)) + &running("web", 2, "S2")),
    );
    // `api`: a started driver that crashed, then another that died: both tries used, the Job fails.
    fx.add_job(
        "api",
        &[],
        &(ended_attempt("api", 1, "S2", Some(4242), &ago(2)) + &running("api", 2, "S3")),
    );

    let engine = fx.start();
    wait_until("both ended", || {
        fx.attempt("web-a2").0 == "ended" && fx.attempt("api-a2").0 == "ended"
    });
    engine.stop();

    assert_eq!(fx.job_status("web"), "queued");
    let retry = fx.events("attempt-retry");
    assert_eq!(retry.len(), 1, "{retry:?}");
    assert_eq!(
        (retry[0].1.as_deref(), &retry[0].3["next_tier"]),
        (Some("web"), &json!("S2"))
    );
    assert_eq!(fx.job_status("api"), "failed");
    let failed = fx.events("job-failed");
    assert_eq!(
        (failed.len(), failed[0].1.as_deref(), &failed[0].3["cause"]),
        (1, Some("api"), &json!("crashed"))
    );
}

/// hs-n278: a driver that an install or a store migration ended (it left its `.infra` marker) is not its Worker's
/// failure: E2 ends it as one whose driver never started, so the Job is queued at the same Tier and `retry_cap`
/// does not count it. A driver that died with no marker still counts.
#[test]
fn e2_ends_a_driver_ended_by_the_store_at_the_same_tier_uncounted_and_other_crashes_still_count() {
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
    let before = ended_attempt("web", 1, "S2", None, &ago(5))
        + &ended_attempt("web", 2, "S2", None, &ago(4));
    fx.add_job("web", &[], &(before + &running("web", 3, "S2")));
    // `api`: the same Attempts, but its last driver died with no marker: it counts.
    fx.add_job(
        "api",
        &[],
        &(ended_attempt("api", 1, "S2", Some(4242), &ago(2)) + &running("api", 2, "S3")),
    );
    let plan = PlanId::parse(PLAN).unwrap();
    fx.home()
        .leave_infra(
            &plan,
            &AttemptId::parse("web-a3").unwrap(),
            Infra::StoreTooNew,
            "The store is newer than this cpd.",
        )
        .unwrap();

    let engine = fx.start();
    wait_until("both ended", || {
        fx.attempt("web-a3").0 == "ended" && fx.attempt("api-a2").0 == "ended"
    });
    engine.stop();

    // `web-a3`: same Tier S2, not counted, and the marker went with the Attempt.
    assert_eq!(fx.job_status("web"), "queued");
    assert_eq!(
        fx.attempt("web-a3"),
        (
            "ended".into(),
            Some("failed".into()),
            Some("crashed".into()),
            None
        )
    );
    assert_eq!(
        fx.env
            .query::<Option<i64>>("SELECT driver_pid FROM attempts WHERE id = 'web-a3'"),
        None
    );
    assert!(
        !fx.home()
            .attempt_infra(&plan, &AttemptId::parse("web-a3").unwrap())
            .exists()
    );
    let retry: Vec<_> = fx
        .events("attempt-retry")
        .into_iter()
        .filter(|e| e.1.as_deref() == Some("web"))
        .collect();
    assert_eq!(
        (retry.len(), &retry[0].3["next_tier"]),
        (1, &json!("S2")),
        "{retry:?}"
    );
    assert!(
        retry[0].3["feedback"]
            .as_str()
            .unwrap()
            .contains("store_too_new"),
        "{retry:?}"
    );
    // `api-a2`: a crash that counts: both tries used, the Job fails.
    assert_eq!(fx.job_status("api"), "failed");
    assert_eq!(fx.events("job-failed").len(), 1);
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

// ---------- a busy store (contract §3.7, §4.2) ----------

/// `store_busy_ms` of 1 ms (its least): SQLite's own wait for the lock is then no time, and the engine's wait for it to clear is
/// the `ManualClock`'s.
const SHORT_BUSY: &str = "[timeouts]\nstore_busy_ms = 1\n";

/// A connection that is not cpd's holding the write lock until it is dropped.
fn hold_write_lock(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::ZERO).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    conn
}

/// A store that stays busy for longer than `store_busy_ms` does not end the engine, at its start or in a
/// step, and no Event is lost: it waits, logs, and goes on when the lock is released.
#[test]
fn a_store_busy_for_longer_than_store_busy_ms_never_ends_the_engine_or_loses_an_event() {
    let fx = engine(SHORT_BUSY);
    fx.engine_plan();
    fx.add_job(
        "api",
        &[],
        "UPDATE jobs SET status = 'running' WHERE key = 'api';",
    );
    let (home, db, log) = (fx.home(), fx.home().db(), fx.home().watch_log());
    let count = {
        let db = db.clone();
        move |sql: &str| {
            Connection::open(&db)
                .unwrap()
                .query_row(sql, [], |r| r.get::<_, i64>(0))
                .unwrap()
        }
    };
    let busy_lines = move || {
        fs::read_to_string(&log)
            .unwrap_or_default()
            .matches("store_busy: ")
            .count()
    };

    // The start: `watch-started` waits for the lock, held from before the engine starts.
    let mut held = Some(hold_write_lock(&db));
    #[derive(Clone, Copy, PartialEq)]
    enum Phase {
        AtStart,
        Ticking,
        InTick { busy: usize },
        Released,
    }
    let mut phase = Phase::AtStart;
    let out = fx
        .watch_steps(move |_| {
            match phase {
                // Its first `store_busy` is logged once the lock outlasted `store_busy_ms`: the engine is in the wait, not ended.
                Phase::AtStart if busy_lines() > 0 => {
                    assert_eq!(count("SELECT count(*) FROM events WHERE kind = 'watch-started'"), 0);
                    held.take().expect("held");
                    phase = Phase::Ticking;
                }
                // A step: the next tick's E4 meets the lock. The first tick added `open-repo`; an open Job that merged on a PR
                // still `dispatch` counts for nothing, so E4 has a Job to add (a write) at the next tick.
                Phase::Ticking if count("SELECT count(*) FROM jobs WHERE kind = 'open'") == 1 => {
                    assert_eq!(count("SELECT count(*) FROM events WHERE kind = 'watch-started'"), 1);
                    common::sql(&mut Store::open(&home, 5000).unwrap(), "UPDATE jobs SET status = 'running' WHERE key = 'open-repo'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'open-repo';");
                    held = Some(hold_write_lock(&db));
                    phase = Phase::InTick { busy: busy_lines() };
                }
                Phase::InTick { busy } if busy_lines() > busy => {
                    assert_eq!(count("SELECT count(*) FROM jobs WHERE kind = 'open'"), 1);
                    held.take().expect("held");
                    phase = Phase::Released;
                }
                _ => {}
            }
            phase == Phase::Released && count("SELECT count(*) FROM jobs WHERE kind = 'open'") == 2
        })
        .unwrap();

    assert_eq!(out, Output::success(json!({"stopped": true})));
    let log = engine_log(&fx);
    assert!(log.contains("store_busy: "), "{log}");
    assert!(log.contains("the store is clear again after "), "{log}");
    assert_eq!(fx.events("error").len(), 0, "{log}");
    assert_eq!(
        fx.events("job-added").len(),
        2,
        "each open Job's Event was appended once"
    );
    assert_eq!(fx.events("watch-started").len(), 1);
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
                let count = Connection::open(&db)
                    .unwrap()
                    .query_row(
                        "SELECT count(*) FROM events WHERE kind = 'watch-stopped'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                let text = fs::read_to_string(&log).unwrap_or_default();
                *looked.lock().unwrap() = Some((count, text));
                taken
                    .lock()
                    .unwrap()
                    .take()
                    .expect("held")
                    .execute_batch("ROLLBACK")
                    .unwrap();
            }
            _ => {}
        })
        .unwrap();

    let (count, text) = seen
        .lock()
        .unwrap()
        .take()
        .expect("the stop waited for the store");
    assert_eq!(count, 0, "the engine exited without watch-stopped");
    assert!(
        text.split("stopping on a signal")
            .nth(1)
            .is_some_and(|after| after.contains("store_busy: ")),
        "{text}"
    );
    assert_eq!(out, Output::success(json!({"stopped": true})));
    assert_eq!(
        fx.events("watch-stopped"),
        [(None, None, None, json!({"pid": std::process::id()}))]
    );
    assert!(
        !engine_log(&fx).contains("cannot append watch-stopped"),
        "{}",
        engine_log(&fx)
    );
}

// ---------- E3: a draft ----------

// ---------- E4: the open Job (contract §3.7 E4, §3.2 Ready, §2.2 "Job kinds"; hs-d340) ----------

/// PR `repo` (an `open` Job the Lead cancelled) beside PR `web`, which has none: `web`'s own `open-web` is what shows the engine has
/// looked at both.
fn engine_cancelled_open_job() -> Ip {
    let fx = engine("");
    fx.engine_plan();
    fx.add_merged_api();
    fx.add_cpd_job_in("repo", "open-repo", "open", "UPDATE jobs SET status = 'cancelled', cancel_reason = 'never to be opened', cancel_requested_at = 't' WHERE key = 'open-repo';");
    fx.add_pr("web", "Show the receipt");
    fx.add_draft_in("web", "later-web");
    fx
}

/// The ways on after that cancel (§3.8, §7.6): `pr open --workflow repo` is the repair and is not refused `open_job_live` (no `open` Job is
/// live); E4 still adds nothing, before or after it.
#[test]
fn the_way_back_from_a_cancelled_open_job_is_pr_open_which_is_not_refused_open_job_live() {
    let fx = engine_cancelled_open_job();
    fx.watch_first_tick().unwrap();
    assert!(
        fx.jobs_of_kind("open")
            .iter()
            .any(|(key, _)| key == "open-web"),
        "the open Job of PR web: {:?}",
        fx.jobs_of_kind("open")
    );
    fx.watch_ticks(2).unwrap();
    let keys = |fx: &Ip| -> Vec<String> {
        fx.jobs_of_kind("open")
            .into_iter()
            .map(|(key, _)| key)
            .collect()
    };
    assert_eq!(
        keys(&fx),
        ["open-repo", "open-web"],
        "E4 added no fresh open Job for PR repo"
    );

    let run = fx.cpd(&["pr", "open", "--workflow", "repo", "--plan", "pay"]);
    fx.watch_ticks(2).unwrap();

    let json = run.json();
    assert_ne!(json["error"]["code"], "open_job_live", "{run:?}");
    // `pr open` queues the PR's opener itself (#383, `pr::open`): the one fresh Job is its own, and E4 adds none after it.
    assert_eq!(
        (&json["job"], &json["status"]),
        (&json!("open-repo-2"), &json!("queued")),
        "{run:?}"
    );
    assert_eq!(
        keys(&fx),
        ["open-repo", "open-repo-2", "open-web"],
        "no open Job for PR repo but the one `pr open` queued"
    );
}

/// The other way on is `workflow cancel`: it cancels the Workflow's own queued `open` Job with the Workflow, and E4 re-adds nothing (the Workflow is no longer
/// `dispatch`), over several ticks. PR `repo` is the one the engine is seen to look at.
#[test]
fn pr_cancel_cancels_the_prs_own_open_job_with_it_and_e4_adds_none() {
    let fx = engine("");
    fx.engine_plan();
    fx.add_merged_api();
    fx.add_pr("web", "Show the receipt");
    fx.add_cpd_job_in("web", "open-web", "open", "");
    fx.cpd(&["workflow", "cancel", "web", "--plan", "pay"]).ok();

    fx.watch_first_tick().unwrap();
    assert!(
        fx.jobs_of_kind("open")
            .iter()
            .any(|(key, _)| key == "open-repo"),
        "the open Job of PR repo: {:?}",
        fx.jobs_of_kind("open")
    );
    fx.watch_ticks(3).unwrap();

    let web: Vec<_> = fx
        .jobs_of_kind("open")
        .into_iter()
        .filter(|(key, _)| key.starts_with("open-web"))
        .collect();
    assert_eq!(web, [("open-web".to_owned(), "cancelled".to_owned())]);
    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM workflows WHERE plan = 'pay' AND key = 'web'"),
        "cancelled"
    );
}

/// `watch_ticks` is `n` ticks of each run, whatever ran before it on the clock.
#[test]
fn the_engine_in_process_runs_n_more_ticks_after_an_earlier_run() {
    let fx = engine("");
    fx.watch_ticks(3).unwrap();
    fx.watch_ticks(2).unwrap();
    assert_eq!(fx.fakes.clock.naps().len(), 5);
    assert_eq!(fx.events("watch-started").len(), 2);
}

// ---------- E5: the poll, in-process (contract §3.7 E5, §7.6 step 6; hs-d340, hs-d354, hs-d497) ----------
//
// `cpd pr poll` is what the engine's poll thread runs; here it is `run_with` in this process ([`Ip::engine_routes_polling`]) and its `gh`
// calls are a closure route of the `FakeRunner`. Time is the `ManualClock`: a poll that lasts a while is a reply that `takes` it.

/// What GitHub says to the poll's search of pull request 812: a text the test changes between two runs of the engine.
struct Says(Arc<Mutex<String>>);

impl Says {
    fn set(&self, mergeable: &str, merge_state: &str) {
        *self.0.lock().unwrap() = common::pay_search(mergeable, merge_state);
    }
}

/// An engine home whose E5 runs the poll for real (in this process), one poll about every tick ([`PR_POLL_EVERY_TICK`]).
fn polling(config_extra: &str) -> Ip {
    let fx = engine_home(config_extra);
    fx.engine_routes_polling();
    fx.export(PR_POLL_EVERY_TICK.0, PR_POLL_EVERY_TICK.1);
    fx
}

/// Plan `pay` in `deploy` with PR `repo` `open` (pull request `acme/pay` #812, stored `mergeable`, `clean`), Job `api` merged, and a queued
/// Job `blocked` that waits for a draft, so no other Job of the PR could run: E3 starts nothing. `gh` answers the search with what
/// [`Says`] holds.
fn open_pr() -> (Ip, Says) {
    // The update Job is queued at the config's `worker_tier` (default H3 since #383): this one is S1.
    open_pr_with("worker_tier = \"S1\"")
}

/// [`open_pr`] on the config `config_extra`.
fn open_pr_with(config_extra: &str) -> (Ip, Says) {
    let fx = polling(config_extra);
    fx.engine_plan();
    fx.env.sql(
        "UPDATE plans SET title = 'Pay the bills', phase = 'deploy' WHERE id = 'pay';
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, mergeable, merge_state, created_at, merged_at, changed_at, updated_at, feedback)
           VALUES ('acme/pay', 812, 'https://github.com/acme/pay/pull/812', 'Pay the bills', 0, 'open', 'none', 'none', 'mergeable', 'clean', 'opened', NULL, 'opened', 'U1', '[]');
         UPDATE workflows SET status = 'open', pr_number = 812, pr_url = 'https://github.com/acme/pay/pull/812', pr_opened_at = 'opened',
            pull_request = (SELECT id FROM pull_requests WHERE number = 812) WHERE plan = 'pay' AND key = 'repo';",
    );
    fx.add_merged_api();
    fx.add_draft_in("repo", "later");
    fx.add_job("blocked", &["later"], "");
    let says = Says(Arc::new(Mutex::new(common::pay_search(
        "MERGEABLE",
        "CLEAN",
    ))));
    let text = says.0.clone();
    fx.route_fn("gh", move |_| Reply::ok(&text.lock().unwrap()));
    (fx, says)
}

/// The `update` Job runs at `worker_tier`, floored to a valid rung when it is not one: H4 is no rung (H offers 0-3 here), so the
/// lowest valid rung at or above it is S1 (S offers 1-5; its effort 5 is no rung).
#[test]
fn an_open_job_runs_at_worker_tier_floored_to_a_valid_rung() {
    let (fx, says) = open_pr_with("worker_tier = \"H4\"\nlead_tier = \"S4\"");
    says.set("CONFLICTING", "DIRTY");
    polls(&fx, 4);
    assert_eq!(
        fx.jobs_of_kind("update"),
        [("up-repo".to_owned(), "queued".to_owned())]
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT tier FROM jobs WHERE plan = 'pay' AND key = 'up-repo'"),
        "S1",
        "worker_tier H4 floored to its rung"
    );
}

/// Whether `call` is the poll's search (the one `gh api graphql` of a poll that no pull request is stored for).
fn is_search(call: &Call) -> bool {
    call.args.iter().any(|arg| arg.contains("search(type"))
}

/// How many polls have searched so far (`gh` calls that are the search), in all the runs of the engine.
fn searches(runner: &FakeRunner) -> usize {
    runner
        .calls()
        .iter()
        .filter(|call| call.program.ends_with("gh") && is_search(call))
        .count()
}

/// Runs the engine until `n` more polls have called `gh`, and stops it: the poll in flight is then taken into the store.
fn polls(fx: &Ip, n: usize) {
    let runner = fx.fakes.runner.clone();
    let to = searches(&runner) + n;
    fx.watch_steps(move |_| searches(&runner) >= to).unwrap();
}

/// The poll queues the PR's `update` Job in the transaction that stores `conflicting`, once per transition, and a base that merely moved
/// (`BEHIND`, still `MERGEABLE`) queues nothing (hs-d497: no `behind` detection). The conflict is no `pr-feedback`.
#[test]
fn e5_queues_one_update_job_when_the_pull_request_conflicts_and_none_for_a_base_that_merely_moved()
{
    let (fx, says) = open_pr();

    says.set("MERGEABLE", "BEHIND");
    polls(&fx, 3);
    assert_eq!(
        fx.env
            .query::<String>("SELECT merge_state FROM pull_requests WHERE number = 812"),
        "behind",
        "the poll stored that the base moved"
    );
    assert_eq!(
        fx.jobs_of_kind("update"),
        Vec::<(String, String)>::new(),
        "a base that merely moved queues nothing"
    );

    says.set("CONFLICTING", "DIRTY");
    polls(&fx, 4);

    assert_eq!(
        fx.jobs_of_kind("update"),
        [("up-repo".to_owned(), "queued".to_owned())],
        "once per conflict, however many polls see it"
    );
    let row: (String, String, i64, bool) = fx.env.read(|conn| {
        conn.query_row(
            "SELECT title, tier, appended, fingerprint IS NOT NULL FROM jobs WHERE plan = 'pay' AND key = 'up-repo'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
    });
    assert_eq!(
        row,
        (
            format!("Bring the pull request up to date: {PR_TITLE}"),
            "S1".into(),
            1,
            true
        )
    );
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT count(*) FROM dependencies WHERE plan = 'pay' AND job = 'up-repo'"
        ),
        0
    );
    let added = fx.events("job-added");
    assert_eq!(added.len(), 1, "{added:?}");
    let data = &added[0].3;
    assert_eq!(
        (
            &data["kind"],
            &data["by"],
            &data["reason"],
            &data["workflow"]
        ),
        (
            &json!("update"),
            &json!("poll"),
            &json!("conflict"),
            &json!("repo")
        )
    );
    assert_eq!(
        fx.events("pr-feedback").len(),
        0,
        "a conflict is no pr-feedback reason any more"
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM workflows WHERE plan = 'pay'"),
        "open",
        "the PR's status does not change"
    );
}

/// The update Job that ended is a cause that ended: a conflict that comes after it is a new transition, and a new Job, `up-repo-2`.
#[test]
fn e5_queues_a_new_update_job_for_a_conflict_that_comes_after_the_first_job_ended() {
    let (fx, says) = open_pr();
    says.set("CONFLICTING", "DIRTY");
    polls(&fx, 2);
    assert_eq!(fx.jobs_of_kind("update").len(), 1);

    // The Job ran and merged; GitHub recomputes (`unknown`), then says the pull request is clean, then conflicts again.
    fx.env.sql("UPDATE jobs SET status = 'running' WHERE key = 'up-repo'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'up-repo';");
    says.set("UNKNOWN", "UNKNOWN");
    polls(&fx, 3);
    says.set("MERGEABLE", "CLEAN");
    polls(&fx, 3);
    assert_eq!(fx.jobs_of_kind("update").len(), 1);
    says.set("CONFLICTING", "DIRTY");
    polls(&fx, 3);

    assert_eq!(
        fx.jobs_of_kind("update"),
        [
            ("up-repo".to_owned(), "merged".to_owned()),
            ("up-repo-2".to_owned(), "queued".to_owned())
        ]
    );
}

/// The test kit's `cpd` route: a call of `cpd` through the runner is `run_with` of the `Fx`'s ports, and its answer is what the
/// command printed and its exit code; `gh`, which the command calls in turn, is a closure route.
#[test]
fn the_cpd_route_runs_the_command_in_process_and_answers_its_stdout_and_exit_code() {
    use cpd_core::host::runner::Runner;
    let fx = polling("");
    fx.route_fn("gh", |_| {
        Reply::ok(&search_reply(4987, "2099-01-01T00:00:00Z"))
    });

    let mut poll = std::process::Command::new(common::engine::ENGINE_EXE);
    poll.args(["pr", "poll"]);
    let out = fx
        .fakes
        .runner
        .output(&mut poll, "cpd", Duration::from_secs(30))
        .unwrap();
    let mut refused = std::process::Command::new(common::engine::ENGINE_EXE);
    refused.args(["no-such-command"]);
    let usage = fx
        .fakes
        .runner
        .output(&mut refused, "cpd", Duration::from_secs(30))
        .unwrap();

    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        (
            out.status.code(),
            &printed["ok"],
            &printed["rate_limit"]["remaining"]
        ),
        (Some(0), &json!(true), &json!(4987)),
        "{printed}"
    );
    assert_eq!(
        fx.calls("gh").len(),
        1,
        "the poll called gh once, through the runner"
    );
    assert_ne!(
        usage.status.code(),
        Some(0),
        "a command that fails answers its exit code"
    );
}

/// Plan `pay` in phase `deploy`, PR `repo` open as pull request 7.
fn in_pr(fx: &Ip) {
    fx.engine_plan();
    fx.env.sql(
        "UPDATE plans SET phase = 'deploy' WHERE id = 'pay';
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
           VALUES ('acme/pay', 7, 'https://github.com/acme/pay/pull/7', 'Pay', 0, 'open', 'none', 'none', 't', 't');
         UPDATE workflows SET status = 'open', pr_number = 7, pr_url = 'https://github.com/acme/pay/pull/7', pr_opened_at = 't',
            pull_request = (SELECT id FROM pull_requests WHERE number = 7) WHERE plan = 'pay';",
    );
}

/// The lookup of PR 7 (`in_pr`) that a poll makes when the search does not list it: still open, so nothing changes.
const LOOKUP_OPEN: &str = r#"{"data":{"p0":{"pullRequest":{"repository":{"nameWithOwner":"acme/pay"},"number":7,"url":"https://github.com/acme/pay/pull/7","title":"Pay","isDraft":false,"state":"OPEN","createdAt":"t","updatedAt":"U1","mergedAt":null,"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,"commits":{"nodes":[]}}}}}"#;

/// What the fake `gh` answers to the search when it found no pull request, with `remaining` requests left until `reset_at`.
fn search_reply(remaining: u32, reset_at: &str) -> String {
    format!(
        r#"{{"data":{{"search":{{"issueCount":0,"nodes":[]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":{remaining},"resetAt":"{reset_at}"}}}}}}"#
    )
}

/// `gh` answers every search with an empty result that takes `takes`; the time of each call, on the `ManualClock`, is noted.
fn gh_noting_starts(fx: &Ip, takes: Duration) -> Arc<Mutex<Vec<Duration>>> {
    let starts = Arc::new(Mutex::new(Vec::new()));
    let (clock, noted) = (fx.fakes.clock.clone(), starts.clone());
    fx.route_fn("gh", move |_| {
        noted.lock().unwrap().push(clock.elapsed());
        Reply::ok(&search_reply(4987, "2099-01-01T00:00:00Z")).takes(takes)
    });
    starts
}

#[test]
fn e5_polls_every_pr_poll_seconds_after_the_previous_poll_ended_whatever_the_plans() {
    // No Plan at all: E5 polls anyway. A poll that lasts 0.2 s, so that "after the end" differs from "after the start".
    let fx = polling("");
    // The poll period, `pr_poll_seconds`, is 0.3 s here (its test override): longer than a poll and than a tick.
    fx.export("CPD_TEST_PR_POLL", "0.3");
    let starts = gh_noting_starts(&fx, Duration::from_millis(200));

    let seen = starts.clone();
    fx.watch_steps(move |_| seen.lock().unwrap().len() >= 3)
        .unwrap();

    // A poll never starts before the previous one ended (0.2 s after its start), and `pr_poll_seconds` (0.3 s) after its end.
    let starts = starts.lock().unwrap();
    for pair in starts.windows(2) {
        assert!(
            pair[1] - pair[0] >= Duration::from_millis(500),
            "the next poll starts 0.3 s after the end of the last, which ends 0.2 s after its start: {starts:?}"
        );
    }
    assert_eq!(fx.events("error").len(), 0);
}

/// engine-W9: a poll of 0.1 s with a tick of 5 s and a poll period (`pr_poll_seconds`, its test override) of 0.2 s runs
/// every 0.3 s (end + 0.2 s), not at the next tick.
///
/// What the test looks at is when each tick begins (the mark the engine leaves on its lock): the engine's clock runs only while the
/// engine naps, so the test holds the engine at its first nap of every tick until the poll that tick started has ended
/// ([`Ip::watch_gated`]); a tick then begins at a time that the polls alone decide.
#[test]
fn e5_polls_at_its_due_time_not_at_the_next_tick() {
    use std::sync::mpsc::channel;
    use std::time::SystemTime;
    /// The longest a gate waits for the poll thread: a failing test's answer, never a pause of a passing one.
    const WAIT: Duration = Duration::from_secs(10);

    let fx = engine_home("");
    let config = fx.env.cpd_home().join("config.toml");
    let text = fs::read_to_string(&config).unwrap();
    let tick = "tick_seconds = 1\n".to_string();
    assert!(text.contains(&tick), "{text}");
    fs::write(&config, text.replace(&tick, "tick_seconds = 5\n")).unwrap();
    fx.engine_base_routes();
    fx.export("CPD_TEST_PR_POLL", "0.2");
    // `cpd pr poll`: it says it began, waits for the test to let it go, and lasts 0.1 s.
    let (entered_tx, entered_rx) = channel::<()>();
    let (go_tx, go_rx) = channel::<()>();
    let (ended_tx, ended_rx) = channel::<()>();
    let (entered_tx, go_rx, ended_tx) = (
        Mutex::new(entered_tx),
        Mutex::new(go_rx),
        Mutex::new(ended_tx),
    );
    let poll = json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2099-01-01T00:00:00.000Z"}});
    fx.route_fn("cpd", move |_| {
        entered_tx.lock().unwrap().send(()).unwrap();
        go_rx
            .lock()
            .unwrap()
            .recv_timeout(WAIT)
            .expect("the engine held at its nap lets the poll go");
        Reply::ok(&poll.to_string()).takes(Duration::from_millis(100))
    });

    let lock = fx.home().watch_lock();
    let ticks = Arc::new(Mutex::new(Vec::<SystemTime>::new()));
    let seen = ticks.clone();
    fx.watch_gated(
        move |_| {
            let began = fs::metadata(&lock).unwrap().modified().unwrap();
            let mut ticks = seen.lock().unwrap();
            if ticks.last() == Some(&began) {
                return false;
            }
            ticks.push(began);
            // A tick began, and at the start of it E5 (due) started a poll: hold here until it ended.
            entered_rx.recv_timeout(WAIT).unwrap_or_else(|_| {
                panic!(
                    "a tick began and no poll started in it (E5 was not due): ticks at {ticks:?}"
                )
            });
            go_tx.send(()).unwrap();
            ended_rx.recv_timeout(WAIT).expect("the poll ended");
            ticks.len() >= 3
        },
        move || {
            let _ = ended_tx.lock().unwrap().send(());
        },
    )
    .unwrap();

    let ticks = ticks.lock().unwrap();
    for pair in ticks.windows(2) {
        let gap = pair[1].duration_since(pair[0]).unwrap();
        assert_eq!(
            gap.as_millis(),
            300,
            "a poll every 0.3 s (0.1 s poll + 0.2 s), not every 5 s tick: {ticks:?}"
        );
    }
    assert_eq!(fx.calls("cpd").len(), 3);
    assert_eq!(fx.events("error").len(), 0);
}

/// The poll thread hands its result over late: the hook of the second poll (it runs when the poll thread has exited, after its
/// send) parks that thread until the third tick has begun. With the hook before the send, the engine naps through its 5 s tick
/// without the result and the third tick begins at ~5.3 s; with the hook after the send, at 0.6 s.
#[test]
fn e5_a_poll_handed_over_late_does_not_slip_to_the_next_tick() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::channel;
    use std::time::SystemTime;
    const WAIT: Duration = Duration::from_secs(10);

    let fx = engine_home("");
    let config = fx.env.cpd_home().join("config.toml");
    let text = fs::read_to_string(&config).unwrap();
    let tick = "tick_seconds = 1\n".to_string();
    assert!(text.contains(&tick), "{text}");
    fs::write(&config, text.replace(&tick, "tick_seconds = 5\n")).unwrap();
    fx.engine_base_routes();
    fx.export("CPD_TEST_PR_POLL", "0.2");
    let (entered_tx, entered_rx) = channel::<()>();
    let (go_tx, go_rx) = channel::<()>();
    let (ended_tx, ended_rx) = channel::<()>();
    let (release_tx, release_rx) = channel::<()>();
    let (entered_tx, go_rx, ended_tx, release_rx) = (
        Mutex::new(entered_tx),
        Mutex::new(go_rx),
        Mutex::new(ended_tx),
        Mutex::new(release_rx),
    );
    let poll = json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2099-01-01T00:00:00.000Z"}});
    fx.route_fn("cpd", move |_| {
        let _ = entered_tx.lock().unwrap().send(());
        go_rx
            .lock()
            .unwrap()
            .recv_timeout(WAIT)
            .expect("the engine held at its nap lets the poll go");
        Reply::ok(&poll.to_string()).takes(Duration::from_millis(100))
    });

    let lock = fx.home().watch_lock();
    let ticks = Arc::new(Mutex::new(Vec::<SystemTime>::new()));
    let seen = ticks.clone();
    let hooks = AtomicUsize::new(0);
    fx.watch_gated(
        move |_| {
            let began = fs::metadata(&lock).unwrap().modified().unwrap();
            let mut ticks = seen.lock().unwrap();
            if ticks.last() == Some(&began) {
                return false;
            }
            ticks.push(began);
            if ticks.len() >= 3 {
                // The third tick began: let the parked poll thread go, and any third poll (it waits for a `go`).
                let _ = release_tx.send(());
                let _ = go_tx.send(());
                return true;
            }
            entered_rx.recv_timeout(WAIT).unwrap_or_else(|_| {
                panic!(
                    "a tick began and no poll started in it (E5 was not due): ticks at {ticks:?}"
                )
            });
            go_tx.send(()).unwrap();
            ended_rx.recv_timeout(WAIT).expect("the poll ended");
            false
        },
        move || {
            let _ = ended_tx.lock().unwrap().send(());
            // The second poll's thread stays parked until the third tick has begun (its sender drops if the test panics).
            if hooks.fetch_add(1, Ordering::SeqCst) == 1 {
                let _ = release_rx.lock().unwrap().recv();
            }
        },
    )
    .unwrap();

    let ticks = ticks.lock().unwrap();
    assert_eq!(ticks.len(), 3, "{ticks:?}");
    for pair in ticks.windows(2) {
        let gap = pair[1].duration_since(pair[0]).unwrap();
        assert_eq!(
            gap.as_millis(),
            300,
            "a poll handed over late slipped to the next 5 s tick instead of 0.3 s: ticks at {ticks:?}"
        );
    }
}

/// The one test of `pr_poll_seconds` itself, the config key (no test override): whole seconds.
#[test]
fn e5_polls_with_a_plan_in_pr_too_and_does_not_poll_more_often_than_every_pr_poll_seconds() {
    let fx = engine_home("");
    fx.engine_routes_polling();
    in_pr(&fx);
    let config = fx.env.cpd_home().join("config.toml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        text.replace("pr_poll_seconds = 3600", "pr_poll_seconds = 1"),
    )
    .unwrap();
    let starts = Arc::new(Mutex::new(Vec::<Duration>::new()));
    let (clock, noted) = (fx.fakes.clock.clone(), starts.clone());
    fx.route_fn("gh", move |call| {
        if is_search(call) {
            noted.lock().unwrap().push(clock.elapsed());
            Reply::ok(&search_reply(4987, "2099-01-01T00:00:00Z"))
        } else if let Some(query) = call.args.iter().find(|arg| arg.contains("reviewThreads")) {
            Reply::ok(&common::empty_comments(query))
        } else {
            Reply::ok(LOOKUP_OPEN)
        }
    });

    let seen = starts.clone();
    fx.watch_steps(move |_| seen.lock().unwrap().len() >= 2)
        .unwrap();

    // Ten ticks of 0.1 s to a poll: a poll per tick would come within a few of them.
    let starts = starts.lock().unwrap();
    let gap = starts[1] - starts[0];
    assert!(
        gap >= Duration::from_secs(1),
        "one poll per pr_poll_seconds (1 s), not one per tick: {gap:?}"
    );
    assert_eq!(fx.events("error").len(), 0);
}

#[test]
fn e5_reports_a_failing_poll_once() {
    let fx = polling("");
    in_pr(&fx);
    fx.route_fn("gh", |_| Reply::exit(1, "", "gh: boom\n"));

    polls(&fx, 3);

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!((plan, job, attempt), (&None, &None, &None));
    assert_eq!(
        (&data["source"], &data["code"], &data["details"]["exit"]),
        (&json!("watch"), &json!("gh_failed"), &json!(1))
    );
}

// ---------- E5: a gh HTTP 5xx or timeout is transient (hs-d354) ----------

/// What GitHub's 502 looks like through `gh api graphql`: nginx's HTML page on stdout and `gh`'s message on stderr, exit 1.
fn gh_502() -> Reply {
    Reply::exit(
        1,
        "<!DOCTYPE html><html><head><title>502 Bad Gateway</title></head><body><center><h1>502 Bad Gateway</h1></center><hr><center>nginx</center></body></html>\n",
        &format!("{GH_502_LINE}\n"),
    )
}

/// The stderr of [`gh_502`].
const GH_502_LINE: &str = "gh: HTTP 502: Bad Gateway (https://api.github.com/graphql)";

fn failing_file(fx: &Ip) -> PathBuf {
    fx.env.cpd_home().join("run/pr-poll-failing.json")
}

/// `run/pr-poll-failing.json` as the engine keeps it: the `call` that failed and `since` when it first did, `minutes` ago.
fn fail_since(fx: &Ip, call: &str, minutes: i64) {
    fs::create_dir_all(failing_file(fx).parent().unwrap()).unwrap();
    fs::write(
        failing_file(fx),
        json!({"call": call, "since": ago(minutes)}).to_string(),
    )
    .unwrap();
}

fn failing(fx: &Ip) -> Option<Value> {
    fs::read_to_string(failing_file(fx))
        .ok()
        .map(|t| serde_json::from_str(&t).unwrap())
}

/// Sets `watch.pr_poll_transient_minutes`.
fn transient_minutes(fx: &Ip, minutes: u32) {
    let config = fx.env.cpd_home().join("config.toml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        text.replace(
            "[watch]\n",
            &format!("[watch]\npr_poll_transient_minutes = {minutes}\n"),
        ),
    )
    .unwrap();
}

/// What the Manager was sent that is not E4's: `data.text` of every `session-message`.
fn messages(fx: &Ip) -> Vec<String> {
    fx.events("session-message")
        .into_iter()
        .map(|e| e.3["text"].as_str().unwrap().to_owned())
        .collect()
}

/// `gh` that answers `failure` until the returned flag is set, then finds no pull request.
fn gh_failing_until_healed(fx: &Ip, failure: Reply) -> Arc<AtomicBool> {
    let healed = Arc::new(AtomicBool::new(false));
    let flag = healed.clone();
    fx.route_fn("gh", move |_| {
        if flag.load(Ordering::SeqCst) {
            Reply::ok(&search_reply(4987, "2099-01-01T00:00:00Z"))
        } else {
            failure.clone()
        }
    });
    healed
}

#[test]
fn e5_a_gh_502_raises_no_error_and_no_message_logs_a_line_and_the_next_poll_heals() {
    let fx = polling("");
    fx.manager("mgr1");
    let healed = gh_failing_until_healed(&fx, gh_502());

    polls(&fx, 3);
    let record = failing(&fx).expect("the engine's record of the failure");
    assert_eq!(record["call"], "search");
    let since = parse_time(record["since"].as_str().unwrap());
    assert!(
        (OffsetDateTime::now_utc() - since).whole_seconds() < 60,
        "the first failure was just now: {record}"
    );
    assert_eq!(
        (fx.events("error").len(), messages(&fx)),
        (0, Vec::<String>::new())
    );

    // The next poll heals it: the record is gone, and nothing was ever raised.
    healed.store(true, Ordering::SeqCst);
    polls(&fx, 3);
    assert_eq!(
        failing(&fx),
        None,
        "the record is deleted by a poll that succeeded"
    );
    assert_eq!(
        (fx.events("error").len(), messages(&fx)),
        (0, Vec::<String>::new())
    );
    let log = engine_log(&fx);
    assert!(
        log.contains("pr poll: gh search failed transiently (HTTP 502), failing since "),
        "{log}"
    );
    assert!(log.contains("): retrying at the next poll"), "{log}");
}

#[test]
fn e5_a_gh_timeout_is_transient_too() {
    let fx = polling("");
    fx.manager("mgr1");
    // `gh` hangs for longer than the poll's 15 s deadline (contract §1.2, `CPD_TEST_DEADLINE_GH` shortens it, not needed on a virtual clock).
    fx.route_fn("gh", |_| {
        Reply::ok("").takes(Duration::from_secs(SLEEP_SECONDS))
    });

    polls(&fx, 1);

    assert_eq!(
        (fx.events("error").len(), messages(&fx)),
        (0, Vec::<String>::new())
    );
    let log = engine_log(&fx);
    assert!(
        log.contains("pr poll: gh search failed transiently (timeout), failing since "),
        "{log}"
    );
    assert_eq!(failing(&fx).unwrap()["call"], "search");
}

/// An engine home with a Manager, a `gh` that always answers `failure` and `run/pr-poll-failing.json` for `minutes` minutes of failing.
fn polling_with_manager_failing(minutes: i64, failure: Reply) -> Ip {
    let fx = polling("");
    fx.manager("mgr1");
    fx.route_fn("gh", move |_| failure.clone());
    fail_since(&fx, "search", minutes);
    fx
}

#[test]
fn e5_raises_one_error_after_ten_minutes_of_the_same_call_failing_and_none_before() {
    // Nine minutes of failing (the record the engine kept, as if it had polled that long): still nothing.
    {
        let fx = polling_with_manager_failing(9, gh_502());
        polls(&fx, 3);
        assert_eq!(
            (fx.events("error").len(), messages(&fx)),
            (0, Vec::<String>::new())
        );
        assert_eq!(failing(&fx).unwrap()["call"], "search");
    }

    // Eleven: one error Event for the machine, and one message to the Manager, not one per tick.
    let fx = polling_with_manager_failing(11, gh_502());
    let since = failing(&fx).unwrap()["since"].clone();
    polls(&fx, 5);

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!((plan, job, attempt), (&None, &None, &None));
    assert_eq!(
        (&data["source"], &data["code"]),
        (&json!("watch"), &json!("gh_failed"))
    );
    let details = &data["details"];
    assert_eq!(
        (
            &details["call"],
            &details["since"],
            &details["minutes"],
            &details["reason"],
            &details["exit"]
        ),
        (
            &json!("search"),
            &since,
            &json!(11),
            &json!("HTTP 502"),
            &json!(1)
        )
    );
    assert_eq!(
        details["argv"].as_array().unwrap()[..3],
        ["gh", "api", "graphql"]
    );
    assert!(
        details["stderr"].as_str().unwrap().contains(GH_502_LINE),
        "{details}"
    );

    assert_eq!(
        messages(&fx),
        Vec::<String>::new(),
        "an engine error is an Event only (hs-d514)"
    );
    // The failure stays: the record keeps its first time, the polls (and their log lines) go on.
    assert_eq!(failing(&fx).unwrap()["since"], since);
    assert!(
        engine_log(&fx)
            .matches("failed transiently (HTTP 502)")
            .count()
            >= 2,
        "{}",
        engine_log(&fx)
    );
}

#[test]
fn e5_counts_the_failing_window_from_pr_poll_transient_minutes() {
    for (minutes, raised) in [(4, 0), (6, 1)] {
        let fx = polling_with_manager_failing(minutes, gh_502());
        transient_minutes(&fx, 5);
        polls(&fx, 3);
        assert_eq!(
            fx.events("error").len(),
            raised,
            "failing for {minutes} minutes, limit 5"
        );
    }
}

/// `gh`'s own failure to parse a response that GitHub cut off mid-body (hs-d354, 2026-10-20 16:43:13Z): exit 1, this on stderr, an empty stdout.
fn gh_cut() -> Reply {
    Reply::exit(1, "", "unexpected end of JSON input\n")
}

#[test]
fn e5_a_cut_gh_response_raises_no_error_and_no_message_logs_a_line_and_the_next_poll_heals() {
    let fx = polling("");
    fx.manager("mgr1");
    let healed = gh_failing_until_healed(&fx, gh_cut());

    polls(&fx, 3);
    assert_eq!(
        failing(&fx).expect("the engine's record of the failure")["call"],
        "search"
    );
    assert_eq!(
        (fx.events("error").len(), messages(&fx)),
        (0, Vec::<String>::new())
    );

    // The next poll heals it: the record is gone, and nothing was ever raised.
    healed.store(true, Ordering::SeqCst);
    polls(&fx, 3);
    assert_eq!(
        failing(&fx),
        None,
        "the record is deleted by a poll that succeeded"
    );
    assert_eq!(
        (fx.events("error").len(), messages(&fx)),
        (0, Vec::<String>::new())
    );
    let log = engine_log(&fx);
    assert!(
        log.contains(
            "pr poll: gh search failed transiently (unexpected end of JSON input), failing since "
        ),
        "{log}"
    );
    assert!(log.contains("): retrying at the next poll"), "{log}");
}

#[test]
fn e5_a_cut_gh_response_is_an_error_only_after_the_window_and_then_once() {
    // Nine minutes of failing: still nothing.
    {
        let fx = polling_with_manager_failing(9, gh_cut());
        polls(&fx, 3);
        assert_eq!(
            (fx.events("error").len(), messages(&fx)),
            (0, Vec::<String>::new())
        );
        assert_eq!(failing(&fx).unwrap()["call"], "search");
    }

    // Eleven: one error Event and one message to the Manager, not one per tick.
    let fx = polling_with_manager_failing(11, gh_cut());
    let since = failing(&fx).unwrap()["since"].clone();
    polls(&fx, 5);

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let data = &errors[0].3;
    assert_eq!(
        (&data["source"], &data["code"]),
        (&json!("watch"), &json!("gh_failed"))
    );
    let details = &data["details"];
    assert_eq!(
        (
            &details["call"],
            &details["since"],
            &details["minutes"],
            &details["reason"],
            &details["exit"]
        ),
        (
            &json!("search"),
            &since,
            &json!(11),
            &json!("unexpected end of JSON input"),
            &json!(1)
        )
    );
    assert!(
        details["stderr"]
            .as_str()
            .unwrap()
            .contains("unexpected end of JSON input"),
        "{details}"
    );
    assert_eq!(
        messages(&fx),
        Vec::<String>::new(),
        "an engine error is an Event only (hs-d514)"
    );
    assert_eq!(failing(&fx).unwrap()["since"], since);
}

#[test]
fn e5_a_failure_that_is_not_transient_is_an_error_at_once_and_clears_the_record() {
    for (name, failure, line) in [
        (
            "a 401",
            Reply::exit(1, "", "gh: HTTP 401: Bad credentials\n"),
            "gh: HTTP 401: Bad credentials",
        ),
        (
            "an HTML page with no 5xx status",
            Reply::exit(
                1,
                "<html><body>nginx</body></html>\n",
                "gh: exit status 1\n",
            ),
            "gh: exit status 1",
        ),
    ] {
        // A transient failure was recorded a minute ago; this one is not that.
        let fx = polling_with_manager_failing(1, failure);
        polls(&fx, 4);

        let errors = fx.events("error");
        assert_eq!(errors.len(), 1, "{name}: {errors:?}");
        let data = &errors[0].3;
        assert_eq!(
            (
                &data["source"],
                &data["code"],
                &data["details"]["transient"],
                &data["details"]["call"]
            ),
            (
                &json!("watch"),
                &json!("gh_failed"),
                &json!(false),
                &json!("search")
            ),
            "{name}"
        );
        assert!(
            data["details"]["stderr"].as_str().unwrap().contains(line),
            "{name}: {data}"
        );
        assert_eq!(
            failing(&fx),
            None,
            "{name}: a failure that is not transient deletes the record"
        );
        assert!(!engine_log(&fx).contains("failed transiently"), "{name}");
    }
}

#[test]
fn e5_pauses_below_100_requests_until_reset_and_reports_one_gh_rate_limited() {
    let fx = polling("");
    // The first poll leaves 50 requests until `reset_at`, a minute from then; the next one, which E5 must not run before that (it
    // polls every tick otherwise), 4987. Both the stamp and the engine's wait are the `ManualClock`'s, so the real handover
    // between the first poll answering and the guard reading the reset does not shorten the wait (a flake under load).
    let starts = Arc::new(Mutex::new(Vec::<Duration>::new()));
    let reset = Arc::new(Mutex::new(None::<String>));
    let (clock, noted, stamped) = (fx.fakes.clock.clone(), starts.clone(), reset.clone());
    fx.route_fn("gh", move |_| {
        let mut noted = noted.lock().unwrap();
        noted.push(clock.elapsed());
        if noted.len() == 1 {
            let reset_at = (clock.utc() + time::Duration::seconds(60))
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap();
            *stamped.lock().unwrap() = Some(reset_at.clone());
            Reply::ok(&search_reply(50, &reset_at))
        } else {
            Reply::ok(&search_reply(4987, "2099-01-01T00:00:00Z"))
        }
    });

    let seen = starts.clone();
    fx.watch_steps(move |_| seen.lock().unwrap().len() >= 2)
        .unwrap();

    // The second poll started when the reset came (a minute, less the handover to the engine): the ticks in between ran no poll.
    let starts = starts.lock().unwrap();
    assert_eq!(starts.len(), 2, "{starts:?}");
    let reset_at = reset
        .lock()
        .unwrap()
        .clone()
        .expect("the first poll answered a reset");
    assert!(
        starts[1] - starts[0] >= Duration::from_secs(55),
        "polled {:?} after the first, before the reset ({reset_at})",
        starts[1] - starts[0]
    );
    // One Event, for the machine, with what the User needs; resuming raised nothing more.
    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!((plan, job, attempt), (&None, &None, &None));
    assert_eq!(
        (&data["source"], &data["code"]),
        (&json!("watch"), &json!("gh_rate_limited"))
    );
    assert_eq!(
        data["details"],
        json!({"remaining": 50, "reset_at": reset_at})
    );
}

// ---------- E3's back-off, E5's executable and E6, in-process ----------

/// A Job whose latest 9 Attempts never had a driver, the last just ended: E3 waits 300 s, so no tick starts it.
#[test]
fn e3_does_not_start_a_job_in_its_backoff() {
    let fx = engine("");
    fx.engine_plan();
    let now = fx.stamp_ago(0);
    let attempts: String = (1..=9)
        .map(|n| ended_attempt("api", n, "S2", None, &now))
        .collect();
    fx.add_job("api", &[], &attempts);

    // Three whole ticks (a tick is ten naps of a tenth of a second).
    fx.watch(30, |_| {}).unwrap();

    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts"), 9);
    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM jobs WHERE plan = 'pay' AND key = 'api'"),
        "queued"
    );
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
    assert_eq!(
        polls
            .iter()
            .map(|call| (call.program.as_str(), call.args.as_slice()))
            .collect::<Vec<_>>(),
        [(
            installed.to_str().unwrap(),
            ["pr".to_owned(), "poll".to_owned()].as_slice()
        )]
    );
    assert_eq!(
        fx.calls("cpd"),
        Vec::<Call>::new(),
        "E5 ran the cpd on PATH"
    );
    assert_eq!(fx.events("error").len(), 0);
}

/// A prerequisite PR that E5 sees merged lets E7 start its dependent PR in that same tick: the poll's result and E7 are one tick.
#[test]
fn a_prerequisite_seen_merged_by_e5_still_lets_e7_start_its_dependent_in_the_same_tick() {
    let fx = polling("");
    // The Lead worktree has commits: give the repo an `origin` that has them, so the only `error` this test could see is E5's or E7's own.
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    let origin = fx.env.root().join("repos/origin.git");
    fx.git(
        fx.env.root(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    fx.local_origin(&repo, &origin);
    fx.git(&repo, &["push", "-q", "origin", "main"]);
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("lead.log"),
    };
    let lead = fx
        .scoped(|| create_lead_worktree(&repo, &PlanId::parse(PLAN).unwrap(), "main", &setup))
        .unwrap()
        .path;
    fx.engine_plan_at(&repo, &lead);
    // `web` is in its own repo: git cannot hold `cpd/pay` beside `cpd/pay/web`.
    let web = fx.git_repo("web").canonicalize().unwrap();
    fx.env.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
           VALUES ('acme/pay', 7, 'https://github.com/acme/pay/pull/7', 'Pay', 0, 'open', 'none', 'none', 't', 't');
         UPDATE workflows SET status = 'open', pr_number = 7, pr_url = 'https://github.com/acme/pay/pull/7', pr_opened_at = 't',
                pull_request = (SELECT id FROM pull_requests WHERE number = 7) WHERE plan = 'pay' AND key = 'repo';
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'web', '{repo}', 'main', 'waiting', 'cpd/pay/web', cpd_txn(), 't');
         INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pay', 'web', 'repo');",
        repo = web.display()
    ));
    // The real `pr poll`, against a `gh` that finds PR 7 merged: the search lists nothing, the lookup answers MERGED.
    let merged = r#"{"repository":{"nameWithOwner":"acme/pay"},"number":7,"url":"https://github.com/acme/pay/pull/7","title":"Pay","isDraft":false,"state":"MERGED","createdAt":"t","updatedAt":"U1","mergedAt":"2026-10-14T10:00:00Z","reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,"commits":{"nodes":[]}}"#;
    fx.route_fn("gh", move |call| {
        if is_search(call) {
            Reply::ok(&search_reply(4987, "2099-01-01T00:00:00Z"))
        } else {
            Reply::ok(&format!(
                r#"{{"data":{{"p0":{{"pullRequest":{merged}}}}}}}"#
            ))
        }
    });

    // The engine is held at its first nap until the poll the first tick started has ended ([`Ip::watch_gated`]): the second tick takes
    // its result and, in that tick, E7 starts `web`.
    let (ended_tx, ended_rx) = std::sync::mpsc::channel::<()>();
    let ended_tx = Mutex::new(ended_tx);
    fx.watch_gated(
        move |nap| {
            if nap == 1 {
                // The longest the gate waits for the poll thread: a failing test's answer, never a pause of a passing one.
                ended_rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("the poll ended");
            }
            nap >= OVER_ONE_TICK
        },
        move || {
            let _ = ended_tx.lock().unwrap().send(());
        },
    )
    .unwrap();

    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM workflows WHERE key = 'repo'"),
        "merged"
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM workflows WHERE key = 'web'"),
        "dispatch"
    );
    assert_eq!(fx.count("workflow-started"), 1);
    assert!(
        fx.env
            .query::<i64>("SELECT id FROM events WHERE kind = 'pr-merged'")
            < fx.env
                .query::<i64>("SELECT id FROM events WHERE kind = 'workflow-started'")
    );
    assert!(
        lead.exists(),
        "the poll that saw the merge leaves the Lead worktree to the janitor"
    );
    assert_eq!(fx.count("error"), 0, "{:?}", fx.texts("error"));
}

// ---------- E10 ----------
//
// E10 starts `<exe> session retitle <id> --width <w>` through `Spawn` and reads the run's end with `try_wait` at the next tick: the
// `FakeSpawn` records each launch, and the test runs it (`Children::run_pending`, in each nap's step), which retitles through `run_with`
// on a `pi` route of the `FakeRunner` and marks the pid exited with the command's exit code. A run the test does not run is a run in
// flight. Time is the `ManualClock`: a titling "fails in a row for 10 minutes" by `advance`.

/// A handle on the store of an in-process home that a step of the engine's run can own (it is `Send + 'static`): what a step reads and
/// writes between two ticks.
#[derive(Clone)]
struct Db {
    home: CpdHome,
}

impl Db {
    fn count(&self, sql: &str) -> i64 {
        let db = self.home.root().join("cpd.db");
        let conn =
            Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn write(&self, text: &str) {
        common::sql(&mut Store::open(&self.home, 5000).unwrap(), text);
    }

    fn errors(&self) -> i64 {
        self.count("SELECT count(*) FROM events WHERE kind = 'error'")
    }
}

/// How many times `pi` was called so far.
fn pi_calls(runner: &FakeRunner) -> usize {
    runner
        .calls()
        .iter()
        .filter(|call| call.program.ends_with("pi"))
        .count()
}

/// What E10's tests share: the `Fx` with a fake tmux server with a live pane per Session (E2 stops an idle Session without one) and the
/// chat slot to read the width of, a Job `api` with its live driver for the Workers' Attempt, and the `cpd` children the engine started
/// that the test runs.
struct Titles {
    fx: Ip,
    panes: Vec<String>,
    children: Arc<Mutex<common::fx::Children>>,
    _driver: Flock<File>,
}

/// A Session file with one short conversation, as pi writes it.
fn conversation(fx: &Ip, id: &str) -> PathBuf {
    let path = fx.env.root().join(format!("{id}.jsonl"));
    let lines = [
        json!({"type": "session", "id": id}),
        json!({"type": "message", "message": {"role": "user", "content": "How do we title Sessions?"}}),
        json!({"type": "message", "message": {"role": "assistant", "content": [{"type": "text", "text": "With an auxiliary agent."}]}}),
    ];
    fs::write(
        &path,
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    path
}

/// The Sessions E10 started a run for (`run/titles/<session>.log`, appended by every run), by id.
fn titled_logs(fx: &Ip) -> Vec<String> {
    let mut logs: Vec<String> = fs::read_dir(fx.env.cpd_home().join("run/titles"))
        .map(|d| {
            d.map(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .trim_end_matches(".log")
                    .to_owned()
            })
            .collect()
        })
        .unwrap_or_default();
    logs.sort();
    logs
}

impl Titles {
    /// `fx`, a pane per id in `panes`, `pi` answering `reply`.
    fn new(panes: usize, reply: Reply) -> Titles {
        Titles::with_pi(panes, |fx| fx.fake_pi(reply))
    }

    /// [`Titles::new`] with `pi` scripted by `script` on the `Fx`.
    fn with_pi(panes: usize, script: impl FnOnce(&Ip)) -> Titles {
        let fx = engine_home("");
        fx.engine_config_on_tmux("");
        fx.engine_routes_on_tmux();
        fx.engine_plan();
        script(&fx);
        let mut ids = Vec::new();
        for n in 0..panes {
            let args: &[&str] = if n == 0 {
                &["new-session", "-d", "-s", "x"]
            } else {
                &["new-window", "-d", "-t", "=x:"]
            };
            let out = fx.tmux(&[args, &["-P", "-F", "#{pane_id}", "sleep", "600"]].concat());
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            ids.push(String::from_utf8(out.stdout).unwrap().trim().to_owned());
        }
        // E10 retitles only with a chat slot to read the width of (hs-d543): the cockpit's `main` window, its placeholder.
        let out = fx.tmux(&[
            "new-session",
            "-d",
            "-s",
            "cpd",
            "-n",
            "main",
            "-x",
            "100",
            "-y",
            "30",
            "-P",
            "-F",
            "#{pane_id}",
            "sleep",
            "600",
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let slot = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        assert!(
            fx.tmux(&["set", "-p", "-t", &slot, "@cpd_view", "chat"])
                .status
                .success()
        );
        let driver = fx.live_driver("api-a1");
        fx.add_running("api", "running", &ago_in(&fx, 1));
        let children = Arc::new(Mutex::new(fx.children()));
        Titles {
            fx,
            panes: ids,
            children,
            _driver: driver,
        }
    }

    /// Session `id` of `role` in `state`, on pane number `pane`, with its finished turn `turn` minutes ago (`None`:
    /// only its start, `stopped` to `idle`) and its title written `titled` minutes ago (`None`: no title).
    fn session(
        &self,
        id: &str,
        role: &str,
        state: &str,
        pane: usize,
        turn: Option<i64>,
        titled: Option<i64>,
    ) {
        let (title, titled_at) = titled.map_or(("NULL".to_owned(), "NULL".to_owned()), |m| {
            (
                "'Old title'".to_owned(),
                format!("'{}'", ago_in(&self.fx, m)),
            )
        });
        let (from, at) = turn.map_or(("stopped", ago_in(&self.fx, 60)), |m| {
            ("working", ago_in(&self.fx, m))
        });
        self.fx.env.sql(&format!(
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

    /// The engine runs `naps` naps of virtual time (a tick is ten naps), and the retitle runs it started are run before each nap passes.
    fn run(&self, naps: usize) {
        let children = self.children.clone();
        self.fx
            .watch(naps, move |_| {
                children.lock().unwrap().run_pending();
            })
            .unwrap();
    }

    /// The engine runs until `step(nap)` says the nap is the last, after the retitle runs it started were run; `step` is a closure of
    /// the test's own (it owns what it reads: a [`Db`], the `FakeRunner`, the clock).
    fn run_steps(&self, mut step: impl FnMut(usize) -> bool + Send + 'static) {
        let children = self.children.clone();
        self.fx
            .watch_steps(move |nap| {
                children.lock().unwrap().run_pending();
                step(nap)
            })
            .unwrap();
    }

    fn db(&self) -> Db {
        Db {
            home: self.fx.home(),
        }
    }

    /// The Sessions with a title written by a retitle run (a `session-titled` Event), by id.
    fn retitled(&self) -> Vec<String> {
        self.fx.env.read(|conn| {
            let mut stmt = conn
                .prepare("SELECT session FROM events WHERE kind = 'session-titled' ORDER BY id")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
    }

    fn started(&self) -> Vec<String> {
        titled_logs(&self.fx)
    }

    /// `pi`'s answers in the real order of Session 4965bfc8 (hs-d356): its first call answers nothing, every later one the model's
    /// preamble, never a `TITLE:` line.
    fn empty_then_preamble(fx: &Ip) {
        let calls = Arc::new(Mutex::new(0u32));
        fx.route_fn("pi", move |_| {
            let mut calls = calls.lock().unwrap();
            *calls += 1;
            Reply::ok(if *calls < 2 { "" } else { PREAMBLE })
        });
    }
}

/// An idle Lead or Manager whose turn ended after its title (and the title is 10 minutes old) is retitled; one titled
/// less than 10 minutes ago, one that is working, one that only started, one without a finished turn and a Worker are
/// not.
#[test]
fn e10_retitles_an_idle_session_whose_turn_ended_after_its_title_and_no_other() {
    let t = Titles::new(7, Reply::ok("TITLE: Auxiliary agent titles\n"));
    t.session("due-manager", "manager", "idle", 0, Some(5), Some(20));
    t.session("due-untitled", "lead", "idle", 1, Some(1), None);
    t.session("throttled", "lead", "idle", 2, Some(1), Some(5));
    t.session("working", "lead", "working", 3, Some(1), Some(20));
    t.session("just-started", "lead", "idle", 4, None, None);
    t.session("title-is-newer", "lead", "idle", 5, Some(40), Some(20));
    t.session("worker", "worker", "idle", 6, Some(1), None);

    // Five ticks: the two runs start in the first, are reaped in the second, and the next three start nothing.
    t.run(50);

    let mut retitled = t.retitled();
    retitled.sort();
    assert_eq!(retitled, ["due-manager", "due-untitled"]);
    assert_eq!(t.started(), ["due-manager", "due-untitled"]);
    assert_eq!(
        t.fx.calls("pi").len(),
        2,
        "no Session is retitled twice: the new title is newer than its turn"
    );
    let titles: Vec<String> = ["due-manager", "due-untitled", "throttled"]
        .iter()
        .map(|id| {
            t.fx.env
                .query(&format!("SELECT title FROM sessions WHERE id = '{id}'"))
        })
        .collect();
    assert_eq!(
        titles,
        [
            "Auxiliary agent titles",
            "Auxiliary agent titles",
            "Old title"
        ]
    );
    assert_eq!(t.fx.count("error"), 0);
}

/// At most 2 retitles at once: a third waits for one to end, and then runs.
#[test]
fn e10_runs_at_most_two_retitles_at_once() {
    let t = Titles::new(3, Reply::ok("TITLE: A title\n"));
    for (n, id) in ["s1", "s2", "s3"].iter().enumerate() {
        t.session(id, "lead", "idle", n, Some(1), None);
    }
    let (spawn, home) = (t.fx.fakes.spawn.clone(), t.fx.env.cpd_home().to_path_buf());
    let waiting = Arc::new(Mutex::new(None));
    let seen = waiting.clone();

    // Nobody runs the launches for three ticks: two are in flight, the third Session waits. Then they run.
    let children = t.children.clone();
    t.fx.watch_steps(move |nap| {
        if nap == 30 {
            let started: Vec<String> = spawn
                .launches_but_janitor()
                .iter()
                .map(|l| l.args[2].clone())
                .collect();
            let logs = fs::read_dir(home.join("run/titles")).unwrap().count();
            *seen.lock().unwrap() = Some((started, logs));
        }
        if nap >= 30 {
            children.lock().unwrap().run_pending();
        }
        nap >= 80
    })
    .unwrap();

    assert_eq!(
        *waiting.lock().unwrap(),
        Some((vec!["s1".to_owned(), "s2".to_owned()], 2)),
        "the third waits"
    );
    assert_eq!(t.retitled().len(), 3);
    assert_eq!(t.fx.calls("pi").len(), 3);
    assert_eq!(t.started(), ["s1", "s2", "s3"]);
    assert_eq!(t.fx.count("error"), 0);
}

/// A run E10 did not start counts: a held `title-<session>.lock` is a run in flight, so its Session is skipped and
/// it takes one of the two places.
#[test]
fn e10_skips_a_session_whose_title_lock_is_held_and_counts_held_locks() {
    let t = Titles::new(3, Reply::ok("TITLE: A title\n"));
    for (n, id) in ["s1", "s2", "s3"].iter().enumerate() {
        t.session(id, "lead", "idle", n, Some(1), None);
    }
    // A lock file of ours in the temp home, held by a second open of it (our own flock: not external software).
    let hold = |id: &str| {
        let path = t.fx.home().locks_dir().join(format!("title-{id}.lock"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        Flock::lock(
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)
                .unwrap(),
            FlockArg::LockExclusiveNonblock,
        )
        .map_err(|(_, e)| e)
        .unwrap()
    };
    let (one, two) = (hold("s1"), hold("s2"));

    t.run(30);
    assert_eq!(t.fx.calls("pi").len(), 0, "two are held: no place for s3");
    assert_eq!(t.fx.fakes.spawn.launches_but_janitor(), []);
    drop(one);
    // s1 starts in the first tick of this run (one place), and s3 once it has ended.
    t.run(50);
    drop(two);

    let mut retitled = t.retitled();
    retitled.sort();
    assert_eq!(retitled, ["s1", "s3"], "s2 is held until the end");
    assert_eq!(t.fx.calls("pi").len(), 2);
}

/// A failed retitle writes nothing, is reported once however many ticks retry it, and the report names no
/// Session or Plan (no Lead gets a turn). A title less than 10 minutes old is not retried at all. A titling is reported once
/// it has failed for 10 minutes in a row (`FAILING`, of the `ManualClock`).
#[test]
fn e10_reports_a_failed_retitle_once_and_retries_only_what_is_due() {
    let t = Titles::new(2, Reply::exit(1, "", "pi: boom\n"));
    t.session("due", "lead", "idle", 0, Some(1), Some(20));
    t.session("throttled", "lead", "idle", 1, Some(1), Some(5));
    let (db, runner, clock) = (t.db(), t.fx.fakes.runner.clone(), t.fx.fakes.clock.clone());
    let before_the_limit = Arc::new(Mutex::new(None));
    let seen = before_the_limit.clone();
    let mut advanced = None;

    t.run_steps(move |nap| {
        match advanced {
            // A retry: the failure is logged, not reported, until it has lasted the limit.
            None if pi_calls(&runner) >= 2 => {
                *seen.lock().unwrap() = Some(db.errors());
                clock.advance(Duration::from_secs(10 * 60));
                // The throttled Session's title is as young as it was.
                db.write(&format!(
                    "UPDATE sessions SET titled_at = '{}' WHERE id = 'throttled';",
                    cpd_core::clock::timestamp(clock.utc() - time::Duration::minutes(5))
                ));
                advanced = Some(nap);
                false
            }
            Some(at) => db.errors() >= 1 && nap >= at + 30,
            None => false,
        }
    });

    assert_eq!(
        *before_the_limit.lock().unwrap(),
        Some(0),
        "not reported before it has failed for the limit"
    );
    assert_eq!(t.started(), ["due"]);
    let errors = t.fx.events("error");
    assert_eq!(errors.len(), 1, "reported once: {errors:?}");
    let data = &errors[0].3;
    assert_eq!(
        (data["source"].as_str(), data["code"].as_str()),
        (Some("watch"), Some("title_failed"))
    );
    assert_eq!(data["details"]["session"], "due");
    assert!(
        data["details"]["tail"]
            .as_str()
            .unwrap()
            .contains("pi: boom"),
        "{data}"
    );
    assert!(
        data["details"]["log"]
            .as_str()
            .unwrap()
            .ends_with("run/titles/due.log"),
        "{data}"
    );
    assert_eq!(
        t.fx.env
            .query::<Option<String>>("SELECT session FROM events WHERE kind = 'error'"),
        None
    );
    assert_eq!(
        t.fx.env
            .query::<Option<String>>("SELECT context FROM events WHERE kind = 'error'"),
        None
    );
    assert_eq!(t.retitled(), Vec::<String>::new());
    assert_eq!(
        t.fx.env
            .query::<String>("SELECT title FROM sessions WHERE id = 'due'"),
        "Old title"
    );
}

/// pi's answer to E10's retry of Session 4965bfc8 at 15:05:37Z: a preamble, which the old code wrote as the title.
const PREAMBLE: &str = "Now I understand. I need to output just the title to stdout…";

/// An empty answer and then the preamble (the two real answers, hs-d356) are no title and, while the Session's titling has
/// failed for less than 10 minutes in a row, no `error` Event either: the Session stays due and E10 retries it at the next tick.
#[test]
fn e10_does_not_report_an_empty_answer_or_a_preamble_within_the_limit_and_writes_no_title() {
    let t = Titles::with_pi(1, Titles::empty_then_preamble);
    t.session("due", "lead", "idle", 0, Some(1), None);

    t.run(50);

    assert_eq!(
        t.fx.count("error"),
        0,
        "a failing titling is logged, not reported, before the limit: {:?}",
        t.fx.events("error")
    );
    assert_eq!(
        t.fx.env
            .query::<Option<String>>("SELECT title FROM sessions WHERE id = 'due'"),
        None,
        "the preamble is no title"
    );
    assert_eq!(
        t.fx.env
            .query::<Option<String>>("SELECT titled_at FROM sessions WHERE id = 'due'"),
        None
    );
    assert_eq!(t.retitled(), Vec::<String>::new());
    assert!(
        t.fx.calls("pi").len() >= 2,
        "the Session stays due and is retried"
    );
}

/// A titling that keeps failing past the limit (10 minutes of the clock) is reported once, as an `error` Event of E10 that names the
/// Session and sets no Session or Plan; not a moment before the limit.
#[test]
fn e10_reports_a_titling_that_keeps_failing_past_the_limit_once() {
    const LIMIT: Duration = Duration::from_secs(10 * 60);
    let t = Titles::with_pi(1, Titles::empty_then_preamble);
    t.session("due", "lead", "idle", 0, Some(1), None);
    let (db, runner, clock) = (t.db(), t.fx.fakes.runner.clone(), t.fx.fakes.clock.clone());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let mut phase = 0;
    let mut reported = None;

    t.run_steps(move |nap| {
        match phase {
            // Failing in a row: a retry of the failed titling, then 20 s short of the limit (the first failure was a few ticks ago).
            0 if pi_calls(&runner) >= 2 => {
                clock.advance(LIMIT - Duration::from_secs(20));
                phase = 1;
            }
            // Two more failed runs later: still short of the limit, no report.
            1 if pi_calls(&runner) >= 4 => {
                log.lock().unwrap().push(db.errors());
                clock.advance(Duration::from_secs(40));
                phase = 2;
            }
            2 if reported.is_none() && db.errors() >= 1 => reported = Some(nap),
            _ => {}
        }
        reported.is_some_and(|at| nap >= at + 30)
    });

    assert_eq!(*seen.lock().unwrap(), [0], "reported before the limit");
    let errors = t.fx.events("error");
    assert_eq!(errors.len(), 1, "reported once: {errors:?}");
    let data = &errors[0].3;
    assert_eq!(
        (data["source"].as_str(), data["code"].as_str()),
        (Some("watch"), Some("title_failed")),
        "{data}"
    );
    assert_eq!(data["details"]["session"], "due");
    assert!(
        data["details"]["log"]
            .as_str()
            .unwrap()
            .ends_with("run/titles/due.log"),
        "{data}"
    );
    assert_eq!(
        t.fx.env
            .query::<Option<String>>("SELECT session FROM events WHERE kind = 'error'"),
        None
    );
    assert_eq!(
        t.fx.env
            .query::<Option<String>>("SELECT title FROM sessions WHERE id = 'due'"),
        None
    );
    assert_eq!(t.retitled(), Vec::<String>::new());
}

/// A success clears the failure: the 10 minutes in a row start again. The titling fails, succeeds, is due again and fails for the
/// rest of the time. More than 10 minutes after the first failure and less than 10 after the last success there is no `error`; a
/// streak of 10 minutes since the success then is reported, once.
#[test]
fn e10_counts_the_failures_in_a_row_from_the_last_success() {
    const STREAK: Duration = Duration::from_secs(9 * 60);
    let works = Arc::new(AtomicBool::new(false));
    let answer = works.clone();
    let t = Titles::with_pi(1, move |fx| {
        fx.route_fn("pi", move |_| {
            if answer.load(Ordering::SeqCst) {
                Reply::ok("TITLE: Dynamic session titles\n")
            } else {
                Reply::ok("")
            }
        });
    });
    t.session("due", "lead", "idle", 0, Some(1), None);
    let (db, runner, clock, log) = (
        t.db(),
        t.fx.fakes.runner.clone(),
        t.fx.fakes.clock.clone(),
        t.fx.home().watch_log(),
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let notes = seen.clone();
    let mut phase = 0;
    let mut calls = 0;
    let mut reported = None;

    t.run_steps(move |nap| {
        let ended = fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("the retitle of Session due ended");
        match phase {
            // Failing: 9 minutes of it, which is short of the limit: no report.
            0 if pi_calls(&runner) >= 2 => {
                clock.advance(STREAK);
                phase = 1;
            }
            1 if pi_calls(&runner) >= 4 => {
                notes
                    .lock()
                    .unwrap()
                    .push(("before the success", db.errors()));
                works.store(true, Ordering::SeqCst);
                phase = 2;
            }
            // The success: the run that titled has ended (E10 saw it, which clears the streak).
            2 if ended => {
                works.store(false, Ordering::SeqCst);
                // Due again: its title is old, its turn newer.
                db.write(&format!(
                    "UPDATE sessions SET titled_at = '{}' WHERE id = 'due';",
                    cpd_core::clock::timestamp(clock.utc() - time::Duration::minutes(20))
                ));
                calls = pi_calls(&runner);
                phase = 3;
            }
            // Failing again: another 9 minutes. More than 10 since the first failure, less than 10 since the success: no report.
            3 if pi_calls(&runner) >= calls + 2 => {
                clock.advance(STREAK);
                phase = 4;
            }
            4 if pi_calls(&runner) >= calls + 4 => {
                notes
                    .lock()
                    .unwrap()
                    .push(("after the success", db.errors()));
                clock.advance(Duration::from_secs(2 * 60));
                phase = 5;
            }
            5 if reported.is_none() && db.errors() >= 1 => reported = Some(nap),
            _ => {}
        }
        reported.is_some_and(|at| nap >= at + 30)
    });

    assert_eq!(
        *seen.lock().unwrap(),
        [("before the success", 0), ("after the success", 0)],
        "the success did not clear the failures in a row"
    );
    assert_eq!(t.fx.count("error"), 1, "{:?}", t.fx.events("error"));
    assert_eq!(t.retitled().len(), 1);
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

    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM contexts WHERE id = 'demo'"),
        0
    );
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM context_records"),
        0
    );
    assert_eq!(
        fx.events("context-created").len() + fx.events("context-record").len(),
        0
    );
    assert_eq!(fx.events("error").len(), 0);
    assert!(
        store.join("context.json").is_file(),
        "the folder is left alone"
    );
    assert!(!engine_log(&fx).contains("E6"), "{}", engine_log(&fx));
}

// ---------- cpd install (Finding 5; `scripts/install-cpd.sh` is a 2-line bootstrap since #401) ----------
//
// The install is `cpd_core::install::run` over the `Ports` of an `Fx`: the checkout is a clone through the git route, the
// built and the installed `cpd` are routes of the runner by path, `tmux` is the tmux route, `HOME` and `CPD_HOME` are the
// env's own, time is the `ManualClock`. No script, no `cargo`, no process, no sleep. The same contract is also pinned in
// `core/src/install/{migration,adapter,followup}_tests.rs` (the tests of `install_script.rs`'s other groups).

/// The `store` object of a build's `--version`: the store opens.
const OPENS: &str = r#"{"opens":true,"refusal":null,"unknown":[],"pending":[]}"#;

/// What one run of the install left: its exit and its stderr.
struct Installed {
    exit: i32,
    stderr: String,
}

/// A checkout (a clone of an origin whose `origin/main` holds the migrations of the tree and an adapter with a `test/`
/// an install must not copy), a built `cpd` whose `--version` reports `store`, and an installed `cpd` that answers the
/// follow-ups with the env's private tmux socket.
struct InstallRig {
    fx: Ip,
    install: cpd_core::install::Install,
}

impl InstallRig {
    /// The rig whose built `cpd` reports `store` (a JSON literal: the verdict object or `null`).
    fn new(store: &str) -> InstallRig {
        let fx = Ip::with_ports();
        let root = fx.env.root().to_path_buf();
        let origin =
            cpd_core::fakes::git::Repo::init(&fx.universe, &root.join("repos/install-origin"));
        for script in ["install-cpd.sh", "cargo.sh", "tag-version.sh"] {
            origin.write(&format!("scripts/{script}"), &format!("{script}\n"));
        }
        for file in [
            "package.json",
            "extension.ts",
            "lib/cpd.ts",
            "lib/adapter.ts",
            "test/cpd.test.ts",
        ] {
            origin.write(&format!("pi/{file}"), &format!("{file}\n"));
        }
        origin.write(
            "core/src/store/migrations/0001-initial.sql",
            "0001-initial.sql\n",
        );
        origin.commit("install files");
        let clone = root.join("repos/install-clone");
        fx.git(
            &root,
            &[
                "clone",
                "-q",
                origin.dir().to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );

        let target_dir = root.join("target");
        fs::create_dir_all(target_dir.join("release")).unwrap();
        let (built, installed, tag) = (
            target_dir.join("release/cpd"),
            fx.env.home().join(".local/bin/cpd"),
            clone.join("scripts/tag-version.sh"),
        );
        fs::write(&built, "built cpd\n").unwrap();
        let socket = fx.env.tmux_socket();
        let version = format!(r#"{{"ok":true,"version":"0.0.0","store":{store}}}"#);
        let config = format!(
            r#"{{"ok":true,"config":{{"tmux":{{"socket":"{socket}","cockpit_session":"cpd","work_session":"cpd-work"}}}}}}"#
        );
        let at = |path: PathBuf, args: &'static [&'static str]| {
            move |c: &Call| Path::new(&c.program) == path && (args.is_empty() || c.args == args)
        };
        let _ = (*fx.fakes.runner)
            .clone()
            .on(
                "cpd",
                at(built.clone(), &["--version"]),
                Reply::ok(&version),
            )
            .on(
                "cpd",
                at(built, &["config", "--migrate"]),
                Reply::ok(r#"{"ok":true,"migrated":[]}"#),
            )
            .on(
                "cpd",
                at(installed.clone(), &["config"]),
                Reply::ok(&config),
            )
            .on(
                "cpd",
                at(installed, &["watch", "--check"]),
                Reply::ok(r#"{"ok":true}"#),
            )
            .on("tag-version.sh", at(tag, &[]), Reply::ok(""));
        let install = cpd_core::install::Install {
            root: clone,
            target_dir,
            home: fx.env.home().to_path_buf(),
            cpd_home: CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap(),
            pid: 4242,
            lock_wait: cpd_core::install::DEFAULT_LOCK_WAIT,
        };
        InstallRig { fx, install }
    }

    /// Runs the install on the `Fx`'s ports.
    fn run(&self) -> Installed {
        let mut stderr = Vec::new();
        let exit = cpd_core::install::run(&self.fx.fakes.ports, &self.install, &mut stderr);
        self.fx.git_route.assert_supported();
        Installed {
            exit,
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }
    }

    /// Runs `cpd install --root <checkout>` through the command tree, in process on the `Fx`'s ports: the CLI's own
    /// wiring of the install, unlike [`InstallRig::run`], which calls `cpd_core::install::run` directly.
    fn cli(&self) -> Run {
        let root = self
            .install
            .root
            .to_str()
            .expect("the checkout path is UTF-8");
        let target = self
            .install
            .target_dir
            .to_str()
            .expect("the target dir path is UTF-8");
        self.fx.cpd_with(
            self.install.root.as_path(),
            &["install", "--root", root],
            &[("CARGO_TARGET_DIR", target)],
        )
    }

    /// [`InstallRig::run`] of an install that must succeed: its stderr.
    fn run_ok(&self) -> String {
        let run = self.run();
        assert_eq!(run.exit, 0, "{}", run.stderr);
        run.stderr
    }

    /// The arguments of every call of the built and the installed `cpd`, in order.
    fn calls(&self) -> Vec<Vec<String>> {
        let (built, installed) = (
            self.install.target_dir.join("release/cpd"),
            self.install.home.join(".local/bin/cpd"),
        );
        let ours = |c: &Call| Path::new(&c.program) == built || Path::new(&c.program) == installed;
        self.fx
            .fakes
            .runner
            .calls()
            .into_iter()
            .filter(ours)
            .map(|c| c.args)
            .collect()
    }

    /// `<HOME>/.local/share/cpd`, where the adapter goes.
    fn share(&self) -> PathBuf {
        self.install.home.join(".local/share/cpd")
    }
}

#[test]
fn install_leaves_a_server_without_an_engine_alone() {
    let rig = InstallRig::new(OPENS);
    let out = rig.fx.tmux(&[
        "new-session",
        "-d",
        "-s",
        "cpd-work",
        "-n",
        "other",
        "sleep",
        "600",
    ]);
    assert!(out.status.success());

    let stderr = rig.run_ok();

    assert!(
        !stderr.contains("restarting the running engine"),
        "{stderr}"
    );
    assert_eq!(
        rig.calls(),
        [
            vec!["--version"],
            vec!["config", "--migrate"],
            vec!["config"],
            vec!["watch", "--check"]
        ]
    );
}

#[test]
fn install_refuses_a_build_that_cannot_open_the_store_before_replacing_anything() {
    let refusing = r#"{"opens":false,"refusal":{"code":"store_too_new","message":"The database was migrated by a newer cpd."},"unknown":[],"pending":[]}"#;
    let rig = InstallRig::new(refusing);
    let out = rig.fx.tmux(&[
        "new-session",
        "-d",
        "-s",
        "cpd-work",
        "-n",
        "watch",
        "sleep",
        "600",
    ]);
    assert!(out.status.success());

    let out = rig.run();

    let stderr = &out.stderr;
    assert_eq!(out.exit, 1, "{stderr}");
    assert!(
        stderr.contains("cpd install refused: this build cannot open "),
        "{stderr}"
    );
    assert!(
        stderr.contains("cpd.db: The database was migrated by a newer cpd."),
        "{stderr}"
    );
    assert!(
        stderr.contains("merge main into this branch, then install again"),
        "{stderr}"
    );
    assert!(!rig.fx.env.home().join(".local/bin/cpd").exists());
    assert!(!rig.fx.env.home().join(".local/share/cpd/pi").exists());
    // Only --version ran: no `tui --respawn`, no `watch --ensure`.
    assert_eq!(rig.calls(), [vec!["--version"]]);
}

#[test]
fn install_accepts_a_build_that_opens_the_store_or_a_home_without_a_database() {
    for store in [OPENS, "null"] {
        // One `Fx` at a time (the process-wide lock): the first is dropped before the second is made.
        let rig = InstallRig::new(store);
        let out = rig.run();
        assert_eq!(out.exit, 0, "{store}: {}", out.stderr);
        assert!(rig.fx.env.home().join(".local/bin/cpd").exists());
    }
}

#[test]
fn install_cli_installs_the_binary_and_the_adapter_into_a_temporary_home() {
    use std::os::unix::fs::PermissionsExt;

    let rig = InstallRig::new(OPENS);

    let run = rig.cli();

    let json = run.ok();
    assert!(
        !json["binary"]
            .as_str()
            .expect("binary is a string")
            .is_empty(),
        "{json}"
    );
    assert!(
        !json["adapter"]
            .as_str()
            .expect("adapter is a string")
            .is_empty(),
        "{json}"
    );
    assert!(
        !json["version"]
            .as_str()
            .expect("version is a string")
            .is_empty(),
        "{json}"
    );

    // The binary is the built one, at <HOME>/.local/bin/cpd, mode 755.
    let built = rig.install.target_dir.join("release/cpd");
    let bin = rig.install.home.join(".local/bin/cpd");
    assert!(bin.is_file(), "{} is not a file", bin.display());
    assert_eq!(
        fs::read(&bin).unwrap(),
        fs::read(&built).unwrap(),
        "the installed binary is not the built one"
    );
    assert_eq!(
        fs::metadata(&bin).unwrap().permissions().mode() & 0o777,
        0o755,
        "the binary is not mode 755"
    );

    // The adapter is <HOME>/.local/share/cpd/pi -> pi.<id>, the checkout's pi/ minus test/.
    let adapter = rig.share().join("pi");
    assert!(
        fs::symlink_metadata(&adapter)
            .unwrap()
            .file_type()
            .is_symlink(),
        "pi is not a symlink"
    );
    let target = fs::read_link(&adapter)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        target.starts_with("pi.") && !target.contains('/'),
        "the target `{target}` is not a relative pi.<id>"
    );
    let expected: Vec<String> = tree(&rig.install.root.join("pi"))
        .into_iter()
        .filter(|p| p != "test" && !p.starts_with("test/"))
        .collect();
    assert_eq!(tree(&adapter), expected);

    // The follow-ups ran on the installed binary.
    let calls = rig.calls();
    assert!(
        calls.contains(&vec!["config".to_string()]),
        "no config follow-up: {calls:?}"
    );
    assert!(
        calls.contains(&vec!["watch".to_string(), "--check".to_string()]),
        "no watch --check follow-up: {calls:?}"
    );
}

// ---------- the adapter swap of `cpd install` (pi-ci-W9) ----------

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
    let mut out: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

#[test]
fn install_puts_the_adapter_at_share_cpd_pi_without_its_tests() {
    let rig = InstallRig::new(OPENS);
    rig.run_ok();

    // The checkout is the fake one of the rig: compare with its `pi/`.
    let checkout = &rig.install.root;
    let share = rig.share();
    let adapter = share.join("pi");
    let expected: Vec<String> = tree(&checkout.join("pi"))
        .into_iter()
        .filter(|p| p != "test" && !p.starts_with("test/"))
        .collect();
    assert!(
        expected.contains(&"extension.ts".to_string())
            && expected.contains(&"lib/cpd.ts".to_string()),
        "{expected:?}"
    );
    assert!(tree(&checkout.join("pi")).contains(&"test/cpd.test.ts".to_string()));
    // `pi` is a symlink to a `pi.<id>` beside it, which holds the checkout's `pi/` minus `test/`.
    assert!(
        fs::symlink_metadata(&adapter)
            .unwrap()
            .file_type()
            .is_symlink(),
        "pi is not a symlink"
    );
    let target = fs::read_link(&adapter)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        target.starts_with("pi.") && !target.contains('/'),
        "the target `{target}` is not a relative pi.<id>"
    );
    assert_eq!(tree(&share.join(&target)), expected);
    assert_eq!(tree(&adapter), expected);
    // Nothing else is left beside it: no tmp directory, no scratch link, no legacy.
    assert_eq!(names(&share), ["pi".to_string(), target]);
}

// The old two tests below recorded each rename(2) of the script's python through an audit hook. In process there is no
// such hook and no fs port: the renames are asserted by the tree they leave (the Lead's decision on research section 5,
// `tests-mock-mig-engine.md` row of `install_*`): the moves the old assertions forbade would each leave a trace in it.

#[test]
fn install_replaces_an_installed_adapter_as_a_whole_by_one_rename() {
    let rig = InstallRig::new(OPENS);
    let share = rig.share();
    let adapter = share.join("pi");
    // An installed adapter as an earlier install left it: `pi` -> `pi.<id>`; the install swaps the link over it.
    let first = "pi.20200101T000000-1";
    fs::create_dir_all(share.join(first)).unwrap();
    fs::write(share.join(first).join("package.json"), "{}").unwrap();
    fs::write(share.join(first).join("stale.ts"), "").unwrap();
    std::os::unix::fs::symlink(first, &adapter).unwrap();

    rig.run_ok();

    // `pi` is never missing: the only rename that touches it is one rename(2) of the new link over it
    // (docs/implementing.md "The adapter swap"), which the kernel does atomically. What that leaves: the link is the
    // new version (named by the virtual UTC second and the pid), no `pi.legacy` (the migration's move aside), no scratch
    // link or temporary directory, and the old version is whole where it was (nothing renamed or moved it).
    let new = fs::read_link(&adapter)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_ne!(new, first, "pi points to the new version");
    assert!(new.starts_with("pi.") && new.ends_with("-4242"), "{new}");
    assert!(
        !adapter.join("stale.ts").exists(),
        "the old adapter was not replaced as a whole"
    );
    assert!(adapter.join("package.json").exists());
    assert!(
        share.join(first).join("stale.ts").exists(),
        "the old version is untouched, kept as the displaced one"
    );
    let mut expected = ["pi".to_string(), first.to_string(), new];
    expected.sort();
    assert_eq!(
        names(&share),
        expected,
        "no scratch link or directory is left"
    );
}

#[test]
fn install_over_a_real_adapter_directory_moves_it_aside_and_links_in_one_process() {
    let rig = InstallRig::new(OPENS);
    let share = rig.share();
    let adapter = share.join("pi");
    fs::create_dir_all(adapter.join("old")).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fs::write(adapter.join("stale.ts"), "").unwrap();
    fs::write(adapter.join("old/f.ts"), "").unwrap();

    rig.run_ok();

    // The migration is two renames of one process (the install itself, in process), back to back (docs/implementing.md
    // "The adapter swap"): the directory aside as a whole (`pi.legacy`), then the link over it. What that leaves:
    assert!(
        fs::symlink_metadata(&adapter)
            .unwrap()
            .file_type()
            .is_symlink(),
        "pi is a symlink afterwards"
    );
    assert!(
        !adapter.join("stale.ts").exists() && !adapter.join("old").exists(),
        "the old adapter was not replaced as a whole"
    );
    assert!(
        share.join("pi.legacy/stale.ts").exists() && share.join("pi.legacy/old/f.ts").exists(),
        "pi.legacy holds the old files"
    );
    assert!(adapter.join("package.json").exists());
    let new = fs::read_link(&adapter)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut expected = ["pi".to_string(), "pi.legacy".to_string(), new];
    expected.sort();
    assert_eq!(
        names(&share),
        expected,
        "no scratch link or directory is left"
    );
}

// ---------- a caller with no UTF-8 locale (hs-d463; contract §6.1 "Every tmux call is a UTF-8 client") ----------

// ---------- a planned restart is logged only (hs-d471, contract §3.7 "A planned restart") ----------

/// The pid of the engine the tests below say ran before the one under test: no process.
const PREVIOUS_ENGINE: i64 = 424_242;

/// The sentence a planned restart adds before the final full stop.
const PLANNED: &str = "(an install's planned restart)";

#[test]
fn a_planned_restart_back_within_ten_seconds_is_logged_and_tells_no_manager() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(500, true);
    fx.marker(PREVIOUS_ENGINE, 1_000);
    fx.start_and_stop();
    assert_eq!(
        fx.messages(),
        [],
        "a quiet restart sends no session-message"
    );
    let text = fx.started_text();
    assert!(
        text.contains("The engine was down for ") && text.contains(&format!("{PLANNED}.")),
        "{text}"
    );
    assert!(
        engine_log(&fx).contains(&format!("{PLANNED}.")),
        "{}",
        engine_log(&fx)
    );
    assert!(
        !fx.marker_path().exists(),
        "the marker is consumed at the start"
    );
}

#[test]
fn a_stop_with_no_marker_tells_each_manager_as_before() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(500, true);
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(
        !fx.started_text().contains(PLANNED),
        "{}",
        fx.started_text()
    );
}

#[test]
fn a_planned_restart_that_stayed_down_past_ten_seconds_tells_each_manager_it_took_longer() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(11_000, true);
    fx.marker(PREVIOUS_ENGINE, 12_000);
    fx.start_and_stop();
    fx.told_with("it was an install's restart that took longer than 10 s");
    assert!(
        !fx.marker_path().exists(),
        "the marker is consumed at the start"
    );
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
        assert!(
            text.contains("The engine was down for 10.0 s")
                && text.contains(&format!("{PLANNED}.")),
            "{text}"
        );
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
    assert!(
        !fx.started_text().contains(PLANNED),
        "{}",
        fx.started_text()
    );
    assert!(
        !fx.marker_path().exists(),
        "a foreign marker is deleted too"
    );
}

#[test]
fn a_marker_older_than_120_seconds_does_not_silence_the_stop_and_is_deleted() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(500, true);
    fx.marker(PREVIOUS_ENGINE, 130_000);
    fx.start_and_stop();
    fx.told_with("it stopped at");
    assert!(
        !fx.started_text().contains(PLANNED),
        "{}",
        fx.started_text()
    );
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
    assert!(
        !fx.marker_path().exists(),
        "the unreadable marker is deleted"
    );
    let lines = engine_log(&fx)
        .lines()
        .filter(|l| l.contains("install-restart.json"))
        .count();
    assert_eq!(lines, 1, "{}", engine_log(&fx));
}

#[test]
fn a_marker_with_a_field_of_another_type_is_no_marker() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.previous_engine(500, true);
    fx.raw_marker(
        &json!({"by": "install-cpd.sh", "at": fx.stamp_ago(1_000), "engine_pid": "424242"})
            .to_string(),
    );
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
    assert!(
        text.contains("it was last alive") && text.contains(&format!("{PLANNED}.")),
        "{text}"
    );
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
    let fx = engine("");
    fx.previous_engine(500, true);
    fx.env.sql(&format!(
        "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '{}', 'watch-started', 'The engine started.', '{{\"pid\":{}}}');",
        fx.stamp_ago(100),
        PREVIOUS_ENGINE + 7
    ));
    // The command reads the time of its ports: the `ManualClock`.
    let before = fx.utc_ago(1000);
    let out = fx.cpd(&["watch", "--mark-restart"]).ok();
    assert_eq!(
        out,
        json!({"ok": true, "marked": true, "engine_pid": PREVIOUS_ENGINE + 7})
    );
    let marker: Value =
        serde_json::from_str(&fs::read_to_string(fx.marker_path()).unwrap()).unwrap();
    assert_eq!(
        (marker["by"].clone(), marker["engine_pid"].clone()),
        (json!("install-cpd.sh"), json!(PREVIOUS_ENGINE + 7)),
        "{marker}"
    );
    let at = parse_time(marker["at"].as_str().unwrap());
    assert!(at >= before && at <= fx.utc_ago(-1000), "{marker}");

    fx.cpd(&["watch", "--mark-restart"]).ok();
    let files: Vec<_> = fs::read_dir(fx.marker_path().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("install-restart"))
        .collect();
    assert_eq!(
        files,
        ["install-restart.json"],
        "one marker, no temporary file left"
    );
}

#[test]
fn mark_restart_with_no_watch_started_writes_nothing_and_fails_watch_not_started() {
    let fx = engine("");
    let run = fx.cpd(&["watch", "--mark-restart"]);
    assert_eq!(run.exit, 3, "{run:?}");
    assert_eq!(run.error()["code"], "watch_not_started");
    assert!(!fx.marker_path().exists());
}

#[test]
fn a_marker_written_by_mark_restart_silences_the_restart_it_marks() {
    let fx = engine("");
    fx.manager("mgr1");
    fx.start_and_stop();
    fx.cpd(&["watch", "--mark-restart"]).ok();
    fx.start_and_stop();
    assert_eq!(
        fx.messages(),
        [],
        "the engine that stopped was marked, and the next one is back within a moment"
    );
    assert!(fx.started_text().contains(PLANNED), "{}", fx.started_text());
}

// ---------- `watch --check` is read-only (hs-d463; contract §3.7 step 1, commands.md §12) ----------

/// A store one migration behind this binary, and what the check must leave alone. The store is the template's, whose
/// newest migration's row is removed (the trigger that makes `schema_migrations` append-only is dropped and recreated
/// as it was): the verdict (§4.5) is then `opens` with that one migration `pending`. The migration's effects are
/// already in the schema, so a check that migrated would fail on it or double-record it: either way the checks below fail.
struct BehindStore {
    fx: Ip,
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
            (
                name.to_string(),
                fs::read(&path).ok(),
                fs::metadata(&path).and_then(|m| m.modified()).ok(),
            )
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
        conn.prepare("SELECT name FROM schema_migrations ORDER BY seq")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()
    })
}

fn user_version(env: &TestEnv) -> i64 {
    env.query("PRAGMA user_version")
}

fn behind_store() -> BehindStore {
    let fx = Ip::with_ports();
    let env = &fx.env;
    env.write_tmux_config("");
    let conn = rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap();
    conn.pragma_update(None, "journal_mode", "wal").unwrap();
    let (pending, trigger): (String, String) = (
        conn.query_row(
            "SELECT name FROM schema_migrations ORDER BY seq DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap(),
        conn.query_row(
            "SELECT sql FROM sqlite_schema WHERE name = 'schema_migrations_no_delete'",
            [],
            |r| r.get(0),
        )
        .unwrap(),
    );
    conn.execute_batch("DROP TRIGGER schema_migrations_no_delete")
        .unwrap();
    conn.execute("DELETE FROM schema_migrations WHERE name = ?1", [&pending])
        .unwrap();
    conn.execute_batch(&trigger).unwrap();
    // `locks/store.lock` exists, as it does on a machine that has run cpd.
    fs::create_dir_all(env.cpd_home().join("locks")).unwrap();
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(env.cpd_home().join("locks/store.lock"))
        .unwrap();
    assert!(
        env.cpd_home().join("cpd.db-wal").exists(),
        "the held connection keeps the WAL"
    );
    BehindStore {
        fx,
        pending,
        _held: conn,
    }
}

impl BehindStore {
    /// The stand-in for the old engine (the binary with the earlier list of migrations; none is built here, and a store
    /// whose last row was removed cannot be opened by this binary's `Store::open`, which would migrate it, the thing
    /// that must not happen): what makes the old engine fail `store_too_new` (§4.5) is an applied migration it does
    /// not know. So the old engine still works against the store when no migration the old list lacks is applied, the
    /// store is not marked newer (`user_version` is the one every named store has), and it reads as it did.
    fn assert_the_old_engine_still_works(&self, before_applied: &[String], before_version: i64) {
        let env = &self.fx.env;
        let now = applied(env);
        assert_eq!(now, before_applied, "no migration was applied");
        assert!(
            !now.contains(&self.pending),
            "the new binary's migration {} was applied by the check",
            self.pending
        );
        assert_eq!(
            user_version(env),
            before_version,
            "user_version is unchanged"
        );
        // The old engine's own read and its own write, from a connection that is not this binary's `Store`.
        let conn = rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        let check: String = conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(check, "ok");
        conn.execute_batch("BEGIN IMMEDIATE; COMMIT")
            .expect("the old engine can still take the write lock");
    }
}

/// The tmux the check fails at: a UTF-8-less client's listing, tabs printed as `_` (the mac-pro failure of hs-d463). A server runs
/// (the route's), and its `list-panes` prints what the incident's tmux printed.
fn tmux_printing_underscores(fx: &Ip) {
    fx.tmux_server();
    let _ = (*fx.fakes.runner).clone().on(
        "tmux",
        |call| call.args.iter().any(|a| a == "list-panes"),
        Reply::ok("%1_cpd_main_0_1234_x\n"),
    );
}

/// (1) A check that passes against a store one migration behind writes nothing: the database, its WAL and `locks/store.lock`
/// are byte-for-byte and mtime-for-mtime what they were, no file is made, the schema version is unchanged.
#[test]
fn watch_check_against_a_store_one_migration_behind_passes_and_changes_no_file_and_no_version() {
    let s = behind_store();
    let before = (
        files(&s.fx.env),
        applied(&s.fx.env),
        user_version(&s.fx.env),
    );
    let run = s.fx.cpd(&["watch", "--check"]);
    let out = run.ok();
    assert_eq!(
        out["checked"],
        json!(["config", "store", "tmux", "listing", "home"]),
        "{out}"
    );
    assert_eq!(
        (
            files(&s.fx.env),
            applied(&s.fx.env),
            user_version(&s.fx.env)
        ),
        before,
        "the check passed and wrote nothing"
    );
    s.assert_the_old_engine_still_works(&before.1, before.2);
}

/// (2) The Manager's test: the check fails after the store step (the tmux listing, a tmux that prints `_` for tabs) against the
/// store one migration behind: the store is not migrated, and the old engine still works against it.
#[test]
fn a_watch_check_that_fails_after_the_store_step_leaves_the_store_unmigrated_for_the_old_engine() {
    let s = behind_store();
    tmux_printing_underscores(&s.fx);
    let before = (
        files(&s.fx.env),
        applied(&s.fx.env),
        user_version(&s.fx.env),
    );
    let run = s.fx.cpd(&["watch", "--check"]);
    let error = run.error();
    assert_eq!(
        error["code"], "tmux_failed",
        "the check fails at the listing, after the store step: {error}"
    );
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("unexpected output"),
        "{error}"
    );
    assert!(
        s.fx.calls("tmux")
            .iter()
            .any(|c| c.args.iter().any(|a| a == "list-panes")),
        "the check got as far as the tmux listing: {:?}",
        s.fx.calls("tmux")
    );
    assert_eq!(
        (
            files(&s.fx.env),
            applied(&s.fx.env),
            user_version(&s.fx.env)
        ),
        before,
        "the failed check left the store as it was"
    );
    s.assert_the_old_engine_still_works(&before.1, before.2);
}

/// (3) The check writes no row and no Event, whether it passes or fails.
#[test]
fn watch_check_writes_no_row_and_no_event_passing_or_failing() {
    let s = behind_store();
    let before = row_counts(&s.fx.env);
    assert!(before.iter().any(|(t, _)| t == "events") && before.iter().any(|(t, _)| t == "txns"));
    s.fx.cpd(&["watch", "--check"]).ok();
    assert_eq!(row_counts(&s.fx.env), before, "a passing check wrote a row");
    tmux_printing_underscores(&s.fx);
    assert_eq!(
        s.fx.cpd(&["watch", "--check"]).error()["code"],
        "tmux_failed"
    );
    assert_eq!(row_counts(&s.fx.env), before, "a failing check wrote a row");
    assert_eq!(
        s.fx.env.query::<i64>("SELECT count(*) FROM events"),
        before.iter().find(|(t, _)| t == "events").unwrap().1
    );
}

// ---- E9: the distill runs (contract §3.7 E9; FX4; hs-d549, hs-d551, hs-d592 R2, R5) ----
//
// E9 starts `<exe> context distill` through `Spawn`: the `FakeSpawn` records the launch, and the test runs it through `run_with`
// ([`Children::run_pending`], as the engine naps) with a `pi` route that answers the distiller. The quiet period and the ticks are
// virtual time.

/// An engine home for E9: the routes of [`engine`], and the distiller (`pi`) curating every record of its batch, noting each
/// batch's size in the returned list. `fits` is the biggest batch it can handle: a bigger one gets no JSON line ("the model cannot
/// do this"), which fails the run.
fn distilling(config_extra: &str, fits: u32) -> (Ip, Arc<Mutex<Vec<u32>>>) {
    let fx = engine(config_extra);
    let sizes = Arc::new(Mutex::new(Vec::new()));
    let seen = sizes.clone();
    fx.route_fn("pi", move |call| {
        let batch = fs::read_to_string(
            call.cwd
                .as_ref()
                .expect("pi runs in the digest directory")
                .join("batch.md"),
        )
        .unwrap();
        let n = u32::try_from(batch.lines().filter(|l| l.starts_with('[')).count()).unwrap();
        seen.lock().unwrap().push(n);
        Reply::ok(&common::engine::pi_stream(&if n <= fits {
            common::engine::curates_all(&batch)
        } else {
            "I cannot do this.".to_owned()
        }))
    });
    (fx, sizes)
}

/// The batch sizes pi saw, in order, a run of equal ones (the ladder's steps of one run) counted once.
fn batch_sizes(sizes: &Mutex<Vec<u32>>) -> Vec<u32> {
    let mut sizes = sizes.lock().unwrap().clone();
    sizes.dedup();
    sizes
}

/// `count` old notes `m1-n<first>`.. in force of Context `ctx`, the newest hours old at the clock's `now`: the quiet period is over.
fn old_notes_at(home: &Path, now: OffsetDateTime, first: i64, count: i64) {
    use cpd_core::context::{By, Draft, Record};
    use cpd_core::ids::{ContextId, RecordId};

    let context = ContextId::parse("ctx").unwrap();
    let mut store = Store::open(&CpdHome::resolve(Some(home.into()), None).unwrap(), 5000).unwrap();
    for n in first..first + count {
        let record = Record {
            id: RecordId::parse(&format!("m1-n{n}")).unwrap(),
            at: cpd_core::clock::timestamp(now - time::Duration::minutes(600 - n)),
            by: By {
                session: None,
                plan: None,
            },
            text: format!("Note number {n}."),
            draft: Draft::Note,
        };
        store
            .write("fixture", |tx| Ok(record.insert(tx, &context).unwrap()))
            .unwrap();
    }
}

fn old_notes(fx: &Ip, first: i64, count: i64) {
    old_notes_at(&fx.env.cpd_home(), fx.fakes.clock.utc(), first, count);
}

/// What a step of a running engine reads: a read-only connection to the store, and the `pi` calls so far. `Send + Sync`, so a step owns it.
struct Probe {
    conn: Mutex<Connection>,
    runner: Arc<FakeRunner>,
}

impl Probe {
    fn new(fx: &Ip) -> Arc<Probe> {
        let db = fx.env.cpd_home().join("cpd.db");
        let conn =
            Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        Arc::new(Probe {
            conn: Mutex::new(conn),
            runner: fx.fakes.runner.clone(),
        })
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn
            .lock()
            .unwrap()
            .query_row(sql, [], |r| r.get(0))
            .unwrap()
    }

    fn curations(&self) -> i64 {
        self.count("SELECT count(*) FROM record_curations")
    }

    fn pi_calls(&self) -> usize {
        self.runner
            .calls()
            .iter()
            .filter(|call| call.program.ends_with("pi"))
            .count()
    }
}

/// Runs the engine on virtual time until `step` says so; it is given the virtual time passed since the run began, once per nap,
/// after the `cpd` children E9 started have run.
fn drive(fx: &Ip, mut step: impl FnMut(Duration) -> bool + Send + 'static) {
    let mut children = fx.children();
    let clock = (*fx.fakes.clock).clone();
    let from = clock.elapsed();
    fx.watch_steps(move |_| {
        children.run_pending();
        step(clock.elapsed() - from)
    })
    .unwrap();
}

/// [`drive`] for `d` of virtual time: whole ticks of the engine.
fn drive_for(fx: &Ip, d: Duration) {
    drive(fx, move |elapsed| elapsed >= d);
}

/// [`drive`] until `cond` holds, then for `then` more of virtual time (whole ticks in which something wrong could still start).
fn drive_until(fx: &Ip, mut cond: impl FnMut() -> bool + Send + 'static, then: Duration) {
    let mut at = None;
    drive(fx, move |elapsed| {
        if at.is_none() && cond() {
            at = Some(elapsed);
        }
        at.is_some_and(|at| elapsed >= at + then)
    });
}

/// No curation anywhere: no pi call, no `record_curations` row, no `context-curated` or `error` Event.
fn assert_nothing_curated(probe: &Probe) {
    assert_eq!(probe.pi_calls(), 0, "pi was called");
    assert_eq!(probe.curations(), 0);
    assert_eq!(
        probe.count("SELECT count(*) FROM events WHERE kind = 'context-curated'"),
        0
    );
    assert_eq!(
        probe.count("SELECT count(*) FROM events WHERE kind = 'error'"),
        0
    );
}

const SECOND: Duration = Duration::from_secs(1);

#[test]
fn e9_retries_a_failed_run_with_the_next_smaller_batch() {
    let (fx, sizes) = distilling("", 15);
    old_notes(&fx, 1, 60);
    let probe = Probe::new(&fx);
    drive_until(&fx, move || probe.curations() >= 15, Duration::ZERO);
    assert_eq!(
        batch_sizes(&sizes)[..3],
        [60, 30, 15],
        "a failed state is retried with 30, then 15 (the model gets a smaller batch)"
    );
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM record_curations"),
        15
    );
}

/// E9 (contract §3.7, §4.6; hs-d551): a backlog of uncurated records is curated one batch of at most 60 per run, the next
/// tick starting the next run.
#[test]
fn e9_curates_the_backlog_one_batch_per_run() {
    let (fx, sizes) = distilling("", 60);
    // 125 records in force, the newest hours old: the quiet period is over.
    old_notes(&fx, 1, 125);
    let probe = Probe::new(&fx);
    // The whole backlog curated, then three more ticks in which a fourth run could start.
    drive_until(&fx, move || probe.curations() == 125, 3 * SECOND);
    assert_eq!(*sizes.lock().unwrap(), [60, 60, 5], "one batch per run");
    assert_eq!(fx.calls("pi").len(), 3);
    assert_eq!(fx.events("context-curated").len(), 3);
}

#[test]
fn e9_stops_after_a_failed_run_of_batch_1() {
    let (fx, sizes) = distilling("", 0);
    old_notes(&fx, 1, 70);
    let probe = Probe::new(&fx);
    let seen = sizes.clone();
    let (mut began, mut calls) = (None, None);
    drive(&fx, move |elapsed| {
        if began.is_none() && seen.lock().unwrap().contains(&1) {
            began = Some(elapsed);
        }
        let Some(began) = began else { return false };
        if calls.is_none() && elapsed >= began + 5 * SECOND {
            calls = Some(probe.pi_calls());
        }
        if let Some(calls) = calls {
            assert_eq!(
                probe.pi_calls(),
                calls,
                "the engine stopped trying: the state did not change"
            );
        }
        elapsed >= began + 15 * SECOND
    });
    assert_eq!(
        batch_sizes(&sizes),
        [60, 30, 15, 5, 1],
        "one run per batch size, and none after batch 1 failed"
    );
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM record_curations"),
        0
    );
}

#[test]
fn e9_starts_again_at_60_when_the_state_changes() {
    let (fx, sizes) = distilling("", 0);
    old_notes(&fx, 1, 70);
    let home = fx.env.cpd_home();
    let clock = (*fx.fakes.clock).clone();
    let seen = sizes.clone();
    let (mut began, mut added) = (None, false);
    drive(&fx, move |elapsed| {
        if began.is_none() && seen.lock().unwrap().contains(&1) {
            began = Some(elapsed);
        }
        let Some(began) = began else { return false };
        if !added && elapsed >= began + 5 * SECOND {
            // A new record changes the state: the next run is at 60 again.
            old_notes_at(&home, clock.utc(), 71, 1);
            added = true;
        }
        batch_sizes(&seen).len() > 5
    });
    assert_eq!(batch_sizes(&sizes)[..6], [60, 30, 15, 5, 1, 60]);
}

// ---- E9 and the curation floor (hs-d592 R2, R5) ----

#[test]
fn e9_skips_an_archived_context() {
    let (fx, _) = distilling("context_curation_min_records = 5", 60);
    old_notes(&fx, 1, 30);
    fx.env
        .sql("UPDATE contexts SET archived_at = '2026-10-20T10:00:00.000Z' WHERE id = 'ctx';");
    drive_for(&fx, 4 * SECOND);
    assert_nothing_curated(&Probe::new(&fx));
}

#[test]
fn e9_skips_a_context_under_the_floor_until_it_reaches_it() {
    let (fx, _) = distilling("context_curation_min_records = 5", 60);
    old_notes(&fx, 1, 4);
    let probe = Probe::new(&fx);
    let home = fx.env.cpd_home();
    let clock = (*fx.fakes.clock).clone();
    let (mut added, mut done) = (false, None);
    drive(&fx, move |elapsed| {
        if !added {
            if elapsed >= 4 * SECOND {
                assert_nothing_curated(&probe);
                old_notes_at(&home, clock.utc(), 5, 1);
                added = true;
            }
            return false;
        }
        if done.is_none() && probe.curations() == 5 {
            done = Some(elapsed);
        }
        done.is_some_and(|at| elapsed >= at + 3 * SECOND)
    });
    assert!(!fx.calls("pi").is_empty());
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM record_curations"),
        5
    );
    assert_eq!(fx.events("context-curated").len(), 1);
}

#[test]
fn e9_reads_the_floor_from_config_toml_at_every_tick() {
    let (fx, _) = distilling("context_curation_min_records = 100", 60);
    old_notes(&fx, 1, 30);
    let probe = Probe::new(&fx);
    let path = fx.env.cpd_home().join("config.toml");
    let mut rewritten = false;
    drive(&fx, move |elapsed| {
        if !rewritten {
            if elapsed >= 4 * SECOND {
                assert_nothing_curated(&probe);
                // rewrite the floor, every other line kept, with the engine running
                let text = fs::read_to_string(&path).unwrap();
                assert!(
                    text.contains("context_curation_min_records = 100"),
                    "{text}"
                );
                fs::write(
                    &path,
                    text.replace(
                        "context_curation_min_records = 100",
                        "context_curation_min_records = 25",
                    ),
                )
                .unwrap();
                rewritten = true;
            }
            return false;
        }
        probe.curations() == 30
    });
    assert_eq!(fx.events("context-curated").len(), 1);
}

/// The test kit's [`Children`](common::fx::Children): a launch the `FakeSpawn` recorded runs once, through `run_with`, its output
/// goes to the launch's log and its pid ends with the command's exit code.
#[test]
fn a_recorded_launch_runs_once_in_process_and_its_pid_ends_with_the_exit_code() {
    use cpd_core::host::spawn::{Launch, Spawn};
    use std::os::unix::process::ExitStatusExt;

    let fx = engine("");
    let log = fx.env.root().join("child.log");
    let launch = |args: &[&str]| Launch {
        program: common::engine::ENGINE_EXE.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        stdout_to: Some(log.clone()),
        ..Launch::default()
    };
    let ok = fx.fakes.spawn.spawn(&launch(&["events", "list"])).unwrap();
    let refused = fx
        .fakes
        .spawn
        .spawn(&launch(&["context", "distill", "--context", "nope"]))
        .unwrap();
    assert!(
        fx.fakes.spawn.alive(ok) && fx.fakes.spawn.alive(refused),
        "a recorded launch runs until the test runs it"
    );

    let mut children = fx.children();
    let done = children.run_pending();
    assert_eq!(
        done.iter().map(|(_, code)| *code).collect::<Vec<_>>(),
        [0, 4]
    );
    assert_eq!(fx.fakes.spawn.try_wait(ok).unwrap().unwrap().into_raw(), 0);
    assert_eq!(
        fx.fakes.spawn.try_wait(refused).unwrap().unwrap().code(),
        Some(4)
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap().lines().count(),
        2,
        "each command's JSON line is in the log"
    );
    assert!(children.run_pending().is_empty(), "a launch runs once");
}
