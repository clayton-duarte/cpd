//! E12, the Worker archive (contract §3.7, §2.2 "Archived"; commands.md §10.10, hs-d516): every stopped Worker Session whose
//! Attempt is `ended` is archived, once, with one `session-archived` Event; one whose Attempt is live is not. One `archive::run`
//! per tick on a prepared store. The live Attempt's driver holds its lock (a kernel `flock` on our own file in the temp home).

use std::fs::{self, File, OpenOptions};

use cpd_core::ids::{AttemptId, PlanId};
use nix::fcntl::{Flock, FlockArg};
use serde_json::{Value, json};

use super::archive;
use super::testkit::Fx;

/// Plan `pay` with Lead `lead1`, Workflow `repo`, Jobs `done` (Attempt `done-a1`, `ended`, Worker `w-done`) and `live` (Attempt
/// `live-a1`, `running`, Worker `w-live`, its driver lock held); both Workers `stopped`.
fn fx() -> (Fx, Flock<File>) {
    let mut fx = Fx::with_plan("");
    fx.add_job("done", &[], "UPDATE jobs SET status = 'running' WHERE key = 'done'; UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE key = 'done';");
    fx.add_job("live", &[], "UPDATE jobs SET status = 'running' WHERE key = 'live';");
    fx.fixture(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, state, state_at, created_at) VALUES
           ('w-done', 'worker', 'ctx', 'pay', 'done-a1', 'pay done a1', NULL, '/', 'S2', 'stopped', 't', 't'),
           ('w-live', 'worker', 'ctx', 'pay', 'live-a1', 'pay live a1', NULL, '/', 'S2', 'stopped', 't', 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, worker_session)
           VALUES ('pay', 'done-a1', 'done', 1, 'S2', 'ladder', 'ended', 'failed', 'crashed', 'cpd/pay--done-a1', '/w', 't', 't', 'w-done');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, driver_pid, worker_session)
           VALUES ('pay', 'live-a1', 'live', 1, 'S2', 'ladder', 'running', 'cpd/pay--live-a1', '/w2', 't', 4242, 'w-live');",
    );
    let path = fx.engine.home.attempt_lock(&PlanId::parse("pay").unwrap(), &AttemptId::parse("live-a1").unwrap());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
    let lock = Flock::lock(lock, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    (fx, lock)
}

fn tick(fx: &mut Fx) {
    archive::run(&mut fx.engine);
}

fn archived_at(fx: &mut Fx, id: &str) -> Option<String> {
    fx.query(&format!("SELECT archived_at FROM sessions WHERE id = '{id}'"))
}

fn archive_events(fx: &mut Fx) -> Vec<(String, Value)> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT session, data FROM events WHERE kind = 'session-archived' ORDER BY id")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.into_iter().map(|(s, d)| (s, serde_json::from_str(&d).unwrap())).collect())
        })
        .unwrap()
}

#[test]
fn ended_attempt_archives_its_worker_session() {
    let (mut fx, _lock) = fx();
    for _ in 0..4 {
        tick(&mut fx);
    }
    assert!(archived_at(&mut fx, "w-done").is_some());
    assert_eq!(archive_events(&mut fx), [("w-done".to_owned(), json!({"archived": true}))], "one Event, though many ticks ran");
}

#[test]
fn live_attempt_keeps_its_worker_unarchived() {
    let (mut fx, _lock) = fx();
    for _ in 0..4 {
        tick(&mut fx);
    }
    // The ended Attempt's Worker is archived, so E12 ran; the live Attempt's is not.
    assert!(archived_at(&mut fx, "w-done").is_some());
    assert_eq!(archived_at(&mut fx, "w-live"), None);
    assert_eq!(archived_at(&mut fx, "lead1"), None);
}
