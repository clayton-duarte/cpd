//! Red tests for the incident hold on E5's `update` Jobs (contract §2.2 "Job kinds", §3.7 E5;
//! commands.md §9.2, hs-d643): a conflict-driven `update` Job is not queued for a held Plan,
//! while a feedback fix Job still is. Job inc-v2-guard implements the conflict-only skip in
//! `core/src/job/open.rs::queue_update` and makes these pass.

use rusqlite::OptionalExtension;

use crate::config::Tier;
use crate::home::CpdHome;
use crate::ids::PlanId;
use crate::plan::WorkflowRow;
use crate::store::Store;

use super::open::{Cause, queue_update};

/// A store with a live incident over the Plan `pay` (the incident Plan) and the Plan `ops`
/// outside it (held); each has one Workflow (`api`, `web`) in `waiting`.
struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('ops', 'ctx', 'Ops', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('pay', 'api', '/r', 'main', 'waiting', 'cpd/pay', cpd_txn(), 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('ops', 'web', '/r', 'main', 'waiting', 'cpd/ops', cpd_txn(), 't');
                 INSERT INTO incident (id, started_at, started_by, reason, started_txn)
                   VALUES (1, 't', 'mgr', 'why', cpd_txn());
                 INSERT INTO incident_plans (plan, started_txn) VALUES ('pay', cpd_txn());",
            )?;
            Ok(())
        })
        .unwrap();
    Fixture { _dir: dir, store }
}

/// Every `update` Job key of `plan`, sorted.
fn update_jobs(f: &mut Fixture, plan: &str) -> Vec<String> {
    f.store
        .read(|c| {
            let mut stmt = c.prepare("SELECT key FROM jobs WHERE plan = ?1 AND kind = 'update' ORDER BY key")?;
            Ok(stmt.query_map([plan], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// The `reason` of the latest `job-added` Event of `plan`, if any.
fn last_reason(f: &mut Fixture, plan: &str) -> Option<String> {
    f.store
        .read(|c| {
            Ok(c.query_row(
                "SELECT json_extract(data, '$.reason') FROM events WHERE kind = 'job-added' AND plan = ?1 ORDER BY id DESC LIMIT 1",
                [plan],
                |r| r.get(0),
            )
            .optional()?)
        })
        .unwrap()
}

#[test]
fn pr_poll_does_not_queue_a_conflict_update_for_a_held_plan() {
    let mut f = fixture();
    let pay = PlanId::parse("pay").unwrap();
    let ops = PlanId::parse("ops").unwrap();
    let pay_workflow = WorkflowRow::fixture("api", "/r", "main", &[], "open");
    let ops_workflow = WorkflowRow::fixture("web", "/r", "main", &[], "open");

    // `ops` is held: the poll's conflict cause queues nothing.
    let queued = f.store.write("pr poll", |tx| queue_update(tx, &ops, &ops_workflow, Tier::parse("S2").unwrap(), "poll", Cause::Conflict)).unwrap();
    assert_eq!(queued, None, "a held Plan queues no conflict update Job");
    assert!(update_jobs(&mut f, "ops").is_empty(), "no update Job row is written for the held Plan");

    // `pay` is in the incident: the same call queues as it always has.
    let queued = f.store.write("pr poll", |tx| queue_update(tx, &pay, &pay_workflow, Tier::parse("S2").unwrap(), "poll", Cause::Conflict)).unwrap();
    let key = queued.expect("an incident Plan still queues its conflict update Job");
    assert_eq!(update_jobs(&mut f, "pay"), vec![key.as_str().to_owned()], "the incident Plan's update Job is queued");
    assert_eq!(last_reason(&mut f, "pay").as_deref(), Some("conflict"), "the incident Plan's Job carries the conflict cause");
}
