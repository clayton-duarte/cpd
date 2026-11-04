//! The migration `20261104T1600-workflow-base-retarget` (pro-d379): after the lock a dispatch Workflow's `base` may change to an
//! unrelated value; its `lead_branch` still may not.

use super::*;
use crate::error::CpdError;
use crate::store::Store;

const MIGRATION: &str = "20261104T1600-workflow-base-retarget";

const SEED: &str = "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('cpd', 'Cpd', 't', 'm');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('l1', 'lead', 'cpd', 'pay', 'L1', '/', 'S3', 'idle', 't', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
       VALUES ('pay', 'cpd', 'Pay', 'deploy', 'l1', 't', 't');
     INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
       VALUES ('pay', 'wf', '/r/api', 'gone', 'dispatch', 'cpd/pay/wf', '/w/wf', cpd_txn(), 't', 't');";

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

#[test]
fn workflow_base_retarget_is_a_built_in_migration() {
    assert!(NAMED.iter().any(|n| n.name == MIGRATION));
}

#[test]
fn a_dispatch_workflow_base_changes_after_the_lock_but_its_lead_branch_does_not() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    write(&mut store, SEED).unwrap_or_else(|e| panic!("fixture: {e:?}"));
    write(
        &mut store,
        "UPDATE workflows SET base = 'main' WHERE key = 'wf'",
    )
    .expect("an unrelated base is permitted");
    let refused = write(
        &mut store,
        "UPDATE workflows SET lead_branch = 'cpd/pay/other' WHERE key = 'wf'",
    );
    assert!(
        format!("{refused:?}").contains("workflow_locked"),
        "{refused:?}"
    );
}
