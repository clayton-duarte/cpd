//! The built-in `workers` pool counts **live Attempts, not Jobs** (contract §3.4 "Tenancy", air-n90): a
//! cancelled, failed or requeued Job whose every Attempt has ended holds no Worker slot, exactly as a
//! merged Job does, so a queued Job of an incident Plan starts while the pool is full of doomed Jobs.
//!
//! The incident this pins: `cpd incident start` cancelled six running Attempts and returned their Jobs
//! to `queued`; the pool read those six Jobs (plus a `failed` one) as tenants, `free` was 0, and E3
//! skipped every incident Job for 41 minutes with zero Attempts running. The fix is worker-pool-impl.

use super::*;
use crate::driver::schedule::{Ready, schedule};

/// The config's `concurrency` in the fixture.
const CONCURRENCY: u32 = 2;

/// A store in incident mode with Plan `incident`: three Jobs whose only Attempt has `ended` (the
/// incident's start cancelled them, so the Jobs sit `queued`, `cancelled` or `failed`) and one queued
/// Job, `deploy-scout`, still waiting for a Worker slot for its first Attempt.
fn store_in_incident() -> (tempfile::TempDir, crate::store::Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = crate::store::Store::open(&home, 5000).unwrap();
    let fingerprint = "f".repeat(64);
    let mut sql = String::from(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'incident', 'Lead', '/', 'O3', 'working', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('incident', 'ctx', 'Incident', 'grounding', 'lead1', 't', NULL);
         INSERT INTO incident (id, started_at, started_by, reason, started_txn)
           VALUES (1, 't', 'lead1', 'deadlock', cpd_txn());
         INSERT INTO incident_plans (plan, started_txn) VALUES ('incident', cpd_txn());",
    );
    // A `scout` Job (no Workflow) of `status` whose one Attempt is `ended`, with `outcome`/`cause`.
    let mut dead_job = |key: &str, status: &str, cause: &str| {
        let failed_cause = if status == "failed" {
            "'crashed'"
        } else {
            "NULL"
        };
        let cancel = if status == "cancelled" {
            ", cancel_reason, cancel_requested_at"
        } else {
            ""
        };
        let cancel_values = if status == "cancelled" {
            ", 'incident', 't'"
        } else {
            ""
        };
        sql += &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, failed_cause, appended, fingerprint, created_txn, created_at, kind{cancel})
               VALUES ('incident', NULL, '{key}', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', {failed_cause}, 0, '{fingerprint}', cpd_txn(), 't', 'scout'{cancel_values});
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, outcome, cause, ended_at)
               VALUES ('incident', '{key}-a1', '{key}', 1, 'S2', 'ladder', 'ended', 'cpd/incident--{key}-a1', '/w', 't0', '{status_outcome}', '{cause}', 't1');",
            status_outcome = if status == "failed" { "failed" } else { "cancelled" },
        );
    };
    dead_job("open-ready", "queued", "cancel_requested");
    dead_job("preview-tests", "cancelled", "cancel_requested");
    dead_job("refusals-scout", "failed", "crashed");
    // The queued incident Job: no Attempt yet, so it needs the pool for its first.
    sql += &format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at, kind)
           VALUES ('incident', NULL, 'deploy-scout', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, '{fingerprint}', cpd_txn(), 't', 'scout');"
    );
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

/// A queued Job of an incident Plan starts while the `workers` pool is full of Jobs no live Attempt
/// holds: the pool counts live Attempts, so E3 finds a free slot and walks the incident Job (air-n90).
#[test]
fn an_incident_job_starts_with_a_pool_full_of_cancelled_jobs() {
    let (_dir, mut store) = store_in_incident();
    let plan = PlanId::parse("incident").unwrap();

    // The incident is on and Plan `incident` is in it: E3 does not hold its Jobs (hs-d643).
    assert!(
        !store
            .read(|conn| Ok(crate::incident::is_held(conn, &plan)))
            .unwrap(),
        "the incident Plan's Jobs are walked, not held"
    );

    // Every tenant Job's Attempt has ended: none holds a Worker slot, so the pool is not full.
    let workers = store.read(|conn| workers(conn, CONCURRENCY)).unwrap();
    assert_eq!(
        workers.holders,
        [],
        "a cancelled, failed or requeued Job whose every Attempt has ended holds no Worker slot"
    );
    assert_eq!(
        workers.pool().free,
        CONCURRENCY,
        "no live Attempt: every Worker slot is free"
    );

    // `workers` is pool 0; the incident Job needs only it.
    let ready = [Ready {
        id: 0,
        urgent: false,
        skipped: None,
        seq: 1,
        tenant: false,
        needs: vec![0],
    }];
    let walk = schedule(&[workers.pool()], &ready);
    assert_eq!(
        walk.start,
        [0],
        "the incident Job starts: the pool holds live Attempts, not dead tenant Jobs"
    );
    assert!(walk.skipped.is_empty(), "nothing is skipped");
}
