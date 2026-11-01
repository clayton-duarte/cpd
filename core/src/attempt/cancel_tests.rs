//! Red tests for a cancelled Attempt failing its dependents (contract §3.2 "Cancel", §3.4,
//! Job fail-loud-tests, hs-d647): the transaction that ends a running Attempt `cancelled`
//! fails every `queued` Job of the Plan that needs the cancelled Job, directly or transitively
//! (`failed_cause` `its need <key> was cancelled`, one `job-failed` Event). These tests are red
//! until that transaction is implemented; they model the engine's own path (`End::Cancelled`).

use serde_json::{Value, json};

use crate::config::{Catalog, TierModel};
use crate::home::CpdHome;
use crate::ids::{AttemptId, PlanId};
use crate::store::Store;

use super::ledger::{self, End, Ladder, Stage};

/// A temporary store with a Plan `pay`, its PR `api`, Job `a` `running` with the Attempt `a-a1`
/// the engine stopped (`stop_request = 'cancelled'`, its `cancel_requested_at` set by
/// `job cancel`), and the queued Job `b` that needs `a`.
fn fixture() -> (tempfile::TempDir, Store) {
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
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, cancel_reason, cancel_requested_at, created_txn, created_at)
                   VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fingerprint}', 'Gave up.', 't', cpd_txn(), 't');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'b', 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, '{fingerprint}', cpd_txn(), 't');
                 INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'b', 'a');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, stop_request, started_at)
                   VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pay--a-a1', '/w', 4242, 'cancelled', 't');"
            ))?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

/// A catalog the ladder can read without pi (a `cancelled` end uses no Tier).
fn catalog() -> Catalog {
    let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![1, 2, 3] };
    Catalog {
        pi_version: "1.0.0".to_owned(),
        o: model("o/m"),
        s: model("s/m"),
        h: model("h/m"),
        g: TierModel { model: "g/m".to_owned(), efforts: vec![] },
    }
}

fn job_row(store: &mut Store, plan: &str, key: &str, column: &str) -> Value {
    store
        .read(|conn| {
            Ok(conn.query_row(
                &format!("SELECT {column} FROM jobs WHERE plan = ?1 AND key = ?2"),
                [plan, key],
                |r| {
                    Ok(match r.get_ref(0)? {
                        rusqlite::types::ValueRef::Null => Value::Null,
                        rusqlite::types::ValueRef::Integer(i) => json!(i),
                        rusqlite::types::ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                        other => panic!("{other:?}"),
                    })
                },
            )?)
        })
        .unwrap()
}

#[test]
fn a_cancelled_attempt_fails_the_dependents() {
    let (_dir, mut store) = fixture();
    let attempt = store
        .read(|conn| ledger::load(conn, &PlanId::parse("pay").unwrap(), &AttemptId::parse("a-a1").unwrap()))
        .unwrap();
    assert_eq!(attempt.stop_request.as_deref(), Some("cancelled"), "the engine asked the Attempt to stop");

    let catalog = catalog();
    let ladder = Ladder { catalog: &catalog, retry_cap: 3 };
    store.write("test", |tx| ledger::end(tx, &attempt, &End::Cancelled, None, &ladder, Stage::Driver)).unwrap();

    // The transaction that makes `a` cancelled also fails its queued dependent `b` (hs-d647).
    assert_eq!(job_row(&mut store, "pay", "a", "status"), "cancelled");
    assert_eq!(job_row(&mut store, "pay", "b", "status"), "failed");
    assert_eq!(job_row(&mut store, "pay", "b", "failed_cause"), "its need a was cancelled");

    // Exactly one `job-failed` Event, with Job `b`.
    let failed: Vec<(String, Value)> = store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT job, data FROM events WHERE kind = 'job-failed' ORDER BY id")?;
            Ok(stmt
                .query_map([], |r| {
                    let job: String = r.get(0)?;
                    let data: String = r.get(1)?;
                    Ok((job, serde_json::from_str::<Value>(&data).expect("data is JSON")))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(failed.len(), 1, "one job-failed Event");
    assert_eq!(failed[0].0, "b");
}
