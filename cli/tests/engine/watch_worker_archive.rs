//! Engine step E12, the Worker archive (contract §3.7, §2.2 "Archived"; commands.md §10.10, hs-d516): every
//! stopped Worker Session whose Attempt is `ended` is archived, once, with one `session-archived` Event; one whose
//! Attempt is live is not. `cpd watch` on a prepared database, with a private tmux server and fake `pi`, `gh`, `cpd`.

use crate::common;

use std::fs::{self, File, OpenOptions};
use std::process::{Child, Stdio};

use common::{CPD, TICK_SECONDS, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::store::{Store, WriteTxn};
use nix::fcntl::{Flock, FlockArg};
use serde_json::{Value, json};

const EMPTY_SEARCH: &str = r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#;

/// A session file with one assistant text line, for `attempt read`.
const TRANSCRIPT: &str = concat!(
    r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"The schema compiles."}]}}"#,
    "\n"
);

/// A running `cpd watch`; SIGKILLed when dropped.
struct Engine(Child);

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Plan `pay` with Lead `lead1`, PR `repo`, Jobs `done` (Attempt `done-a1`, `ended`, Worker `w-done`) and `live`
/// (Attempt `live-a1`, `running`, Worker `w-live`, its driver lock held); both Workers `stopped`.
struct Fx {
    env: TestEnv,
    _lock: Flock<File>,
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn fx() -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
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
    env.fake("cpd", &format!("exec '{CPD}' \"$@\""));

    let file = env.root().join("done.jsonl");
    fs::write(&file, TRANSCRIPT).unwrap();
    let job = |key: &str| {
        format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, failed_cause, appended, fingerprint, created_txn, created_at)
               VALUES ('pay', 'repo', '{key}', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', {cause}, 0, '{fp}', cpd_txn(), 't');",
            status = if key == "done" { "failed" } else { "running" },
            cause = if key == "done" { "'crashed'" } else { "NULL" },
            fp = "a".repeat(64)
        )
    };
    let sql = format!(
        "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES
           ('1.0.0', 'O', 'p/opus', '[0,1,2,3,4,5]', 't'), ('1.0.0', 'S', 'p/sonnet', '[1,2,3,4,5]', 't'), ('1.0.0', 'H', 'p/haiku', '[0,1,2,3]', 't');
         INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'main', 'dispatch', 'cpd/pay', '{repo}', 'Title', 'config', cpd_txn(), 't', 't');
         {done_job} {live_job}
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, state, state_at, created_at) VALUES
           ('w-done', 'worker', 'ctx', 'pay', 'done-a1', 'pay done a1', '{file}', '/', 'S2', 'stopped', 't', 't'),
           ('w-live', 'worker', 'ctx', 'pay', 'live-a1', 'pay live a1', NULL, '/', 'S2', 'stopped', 't', 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, worker_session)
           VALUES ('pay', 'done-a1', 'done', 1, 'S2', 'ladder', 'ended', 'failed', 'crashed', 'cpd/pay--done-a1', '/w', 't', 't', 'w-done');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, driver_pid, worker_session)
           VALUES ('pay', 'live-a1', 'live', 1, 'S2', 'ladder', 'running', 'cpd/pay--live-a1', '/w2', 't', 4242, 'w-live');",
        repo = repo.display(),
        file = file.display(),
        done_job = job("done"),
        live_job = job("live")
    );
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    Store::open(&home, 5000).unwrap().write("fixture", |tx| exec(tx, &sql)).unwrap();

    // The live Attempt's driver holds its lock, so E2 leaves it alone.
    let path = home.attempt_lock(&PlanId::parse("pay").unwrap(), &AttemptId::parse("live-a1").unwrap());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
    let lock = Flock::lock(lock, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    Fx { env, _lock: lock }
}

impl Fx {
    fn start(&self) -> Engine {
        let mut command = self.env.command(std::path::Path::new(CPD));
        command.envs(common::PLAN_POLLS.iter().copied());
        Engine(command.arg("watch").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap())
    }

    fn archived_at(&self, id: &str) -> Option<String> {
        self.env.query(&format!("SELECT archived_at FROM sessions WHERE id = '{id}'"))
    }

    fn archive_events(&self) -> Vec<(String, Value)> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT session, data FROM events WHERE kind = 'session-archived' ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.into_iter().map(|(s, d)| (s, serde_json::from_str(&d).unwrap())).collect())
        })
    }

    /// Two ticks of the engine after now: the tick count is the number of `watch` heartbeats the test cannot see,
    /// so it waits a few ticks of wall time.
    fn some_ticks(&self) {
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
}

#[test]
fn ended_attempt_archives_its_worker_session() {
    let fx = fx();
    let engine = fx.start();

    wait_until("the Worker of the ended Attempt is archived", || fx.archived_at("w-done").is_some());
    fx.some_ticks();
    drop(engine);

    assert_eq!(fx.archive_events(), [("w-done".to_owned(), json!({"archived": true}))], "one Event, though many ticks ran");
}

#[test]
fn live_attempt_keeps_its_worker_unarchived() {
    let fx = fx();
    let engine = fx.start();

    // The ended Attempt's Worker is archived, so E12 ran; the live Attempt's is not.
    wait_until("E12 ran", || fx.archived_at("w-done").is_some());
    fx.some_ticks();
    drop(engine);

    assert_eq!(fx.archived_at("w-live"), None);
    assert_eq!(fx.archived_at("lead1"), None);
}

#[test]
fn archived_worker_is_still_readable_by_attempt_read() {
    let fx = fx();
    let engine = fx.start();
    wait_until("the Worker is archived", || fx.archived_at("w-done").is_some());
    drop(engine);

    let read = fx.env.cpd(&["attempt", "read", "done-a1", "--plan", "pay"]).ok();
    assert_eq!(read["session"], "w-done");
    assert_eq!(read["lines"], json!(["assistant: The schema compiles."]));
    let show = fx.env.cpd(&["attempt", "show", "done-a1", "--plan", "pay"]).ok();
    assert_eq!(show["attempt"]["id"], "done-a1");
}
