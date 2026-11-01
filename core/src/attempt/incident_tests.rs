//! Red tests for the interruption of a running Attempt in incident mode (contract §3.4/§3.7,
//! Job inc-docs, hs-d634): an Attempt that `cpd incident start` asked to stop ends `cancelled` /
//! `cancel_requested` as an Interrupted Attempt (its Job returns to `queued` at the same Tier and an
//! `attempt-retry` Event whose feedback names incident mode is appended), and an Interrupted Attempt
//! is not counted by the retry ladder. The inc-engine Job implements `End::Interrupted` and
//! `automatic_attempts`; these tests are red until then.

use serde_json::{Value, json};

use crate::config::{Catalog, TierModel};
use crate::home::CpdHome;
use crate::ids::{AttemptId, PlanId};
use crate::store::Store;

use super::ledger::{self, End, Ladder, Stage};

/// A temporary store with a Plan `pay`, its PR `api` and the Job `a`, whose Attempts the tests add.
pub(super) struct Fixture {
    _dir: tempfile::TempDir,
    pub(super) store: Store,
}

pub(super) fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let fingerprint = "f".repeat(64);
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, locked_at, created_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
                   VALUES ('pay', 'api', '/r', 'main', 'dispatch', 'cpd/pay', '/lead', 't', cpd_txn(), 't');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fingerprint}', cpd_txn(), 't');"
            ))?;
            Ok(())
        })
        .unwrap();
    Fixture { _dir: dir, store }
}

/// The Job's running Attempt `a-a1`, stopped by `cpd incident start` (`stop_request = 'cancelled'`).
fn running_attempt(f: &mut Fixture) {
    f.store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, stop_request, started_at)
                 VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pay--a-a1', '/w', 4242, 'cancelled', 't')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

/// A catalog the ladder can climb without pi (the Interrupted end uses the same Tier, not the ladder).
pub(super) fn catalog() -> Catalog {
    let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![1, 2, 3] };
    Catalog {
        pi_version: "1.0.0".to_owned(),
        o: model("o/m"),
        s: model("s/m"),
        h: model("h/m"),
        g: TierModel { model: "g/m".to_owned(), efforts: vec![] },
    }
}

#[test]
fn incident_mode_interrupts_a_running_attempt_and_requeues_the_job() {
    let mut f = fixture();
    running_attempt(&mut f);

    let attempt = f
        .store
        .read(|conn| ledger::load(conn, &PlanId::parse("pay").unwrap(), &AttemptId::parse("a-a1").unwrap()))
        .unwrap();
    assert_eq!(attempt.stop_request.as_deref(), Some("cancelled"), "incident start asked it to stop");

    let catalog = catalog();
    let ladder = Ladder { catalog: &catalog, retry_cap: 3 };
    f.store
        .write("test", |tx| ledger::end(tx, &attempt, &End::Interrupted, None, &ladder, Stage::Driver))
        .unwrap();

    // The Attempt ended `cancelled` / `cancel_requested`, and is not a try (the next one keeps the Tier).
    let (status, outcome, cause, driver_pid) = f
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT status, outcome, cause, driver_pid FROM attempts WHERE plan = 'pay' AND id = 'a-a1'",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<i64>>(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!((status.as_str(), outcome.as_str(), cause.as_str()), ("ended", "cancelled", "cancel_requested"));
    assert_eq!(driver_pid, None, "an Interrupted Attempt does not use up a try: the next Attempt keeps the Tier");

    // The Job returns to `queued` at the same Tier.
    let (job_status, job_tier) = f
        .store
        .read(|conn| {
            Ok(conn.query_row("SELECT status, tier FROM jobs WHERE plan = 'pay' AND key = 'a'", [], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?)
        })
        .unwrap();
    assert_eq!((job_status.as_str(), job_tier.as_str()), ("queued", "S2"));

    // One `attempt-retry` Event: cancelled / cancel_requested, the same Tier, feedback naming incident mode.
    let events = f
        .store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT data FROM events WHERE kind = 'attempt-retry' ORDER BY id")?;
            Ok(stmt
                .query_map([], |r| {
                    let data: String = r.get(0)?;
                    Ok(serde_json::from_str::<Value>(&data).expect("data is JSON"))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(events.len(), 1, "stress: one attempt-retry Event");
    let data = &events[0];
    assert_eq!(data["outcome"], json!("cancelled"));
    assert_eq!(data["cause"], json!("cancel_requested"));
    assert_eq!(data["next_tier"], json!("S2"));
    assert!(
        data["feedback"].as_str().unwrap_or_default().to_lowercase().contains("incident"),
        "the feedback names incident mode: {}",
        data["feedback"]
    );
}

#[test]
fn incident_interrupted_attempts_do_not_count_for_the_ladder() {
    let mut f = fixture();
    // a1 failed: it counts. a2 ended `cancelled` / `cancel_requested`: the Interrupted Attempt, ignored.
    f.store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, driver_pid, started_at, ended_at)
                   VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'ended', 'failed', 'crashed', 'cpd/pay--a-a1', '/w', 11, 't', 't');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, driver_pid, started_at, ended_at)
                   VALUES ('pay', 'a-a2', 'a', 2, 'S2', 'ladder', 'ended', 'cancelled', 'cancel_requested', 'cpd/pay--a-a2', '/w', 12, 't', 't');",
            )?;
            Ok(())
        })
        .unwrap();

    let counted = f.store.write("test", |tx| Ok(ledger::automatic_attempts(tx, "pay", "a")?)).unwrap();
    assert_eq!(counted, 1, "only the failed Attempt counts; the Interrupted one is ignored");
}
