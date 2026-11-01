//! Engine step E12, the Worker archive (contract §3.7, §2.2 "Archived"; commands.md §10.10, hs-d516): an archived Worker is still
//! readable by `attempt read` / `attempt show`. `cpd watch` in-process (`Fx::watch`, on the `ManualClock`) on a prepared database,
//! with the fake routes of the `FakeRunner` for `tmux` and `cpd` (the PR poll). The archive itself (once per Worker, none for a live Attempt) is tested next to the step: `watch::archive_tests`.

use crate::common;

use std::fs::{self, File, OpenOptions};

use common::fx::Fx;
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use nix::fcntl::{Flock, FlockArg};
use serde_json::json;

/// A session file with one assistant text line, for `attempt read`.
const TRANSCRIPT: &str = concat!(
    r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"The schema compiles."}]}}"#,
    "\n"
);

/// Plan `pay` with Lead `lead1`, PR `repo`, Jobs `done` (Attempt `done-a1`, `ended`, Worker `w-done`) and `live`
/// (Attempt `live-a1`, `running`, Worker `w-live`, its driver lock held); both Workers `stopped`.
struct Archive {
    fx: Fx,
    _lock: Flock<File>,
}

fn fx() -> Archive {
    let fx = Fx::with_ports();
    let env = &fx.env;
    let repo = fx.git_repo("pay");
    fx.engine_config("");
    fx.engine_routes();
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env
        .root()
        .join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }

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
    env.sql(&sql);

    // The live Attempt's driver holds its lock, so E2 leaves it alone.
    let path = home.attempt_lock(
        &PlanId::parse("pay").unwrap(),
        &AttemptId::parse("live-a1").unwrap(),
    );
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    let lock = Flock::lock(lock, FlockArg::LockExclusiveNonblock)
        .map_err(|(_, e)| e)
        .unwrap();
    Archive { fx, _lock: lock }
}

impl Archive {
    fn archived_at(&self, id: &str) -> Option<String> {
        self.fx.env.query(&format!(
            "SELECT archived_at FROM sessions WHERE id = '{id}'"
        ))
    }
}

#[test]
fn archived_worker_is_still_readable_by_attempt_read() {
    let fx = fx();
    fx.fx.watch_first_tick().unwrap();
    assert!(fx.archived_at("w-done").is_some(), "the Worker is archived");

    let read = fx
        .fx
        .cpd(&["attempt", "read", "done-a1", "--plan", "pay"])
        .ok();
    assert_eq!(read["session"], "w-done");
    assert_eq!(read["lines"], json!(["assistant: The schema compiles."]));
    let show = fx
        .fx
        .cpd(&["attempt", "show", "done-a1", "--plan", "pay"])
        .ok();
    assert_eq!(show["attempt"]["id"], "done-a1");
}
