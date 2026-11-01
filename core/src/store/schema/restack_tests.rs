//! The restack of a `waiting` Workflow after the lock (contract §3.8, §4.4 `prs_locked`, §4.5 `pr-restack`): its `base`
//! may move to the Lead Branch of a Workflow of its Plan and repo, and the need on that Workflow may go.

use crate::error::{CpdError, ErrorCode};
use crate::store::Store;

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn assert_locked(store: &mut Store, sql: &str) {
    let err = write(store, sql).expect_err(sql);
    assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{sql}: {err:?}");
    assert_eq!(err.exit().code(), 3);
}

fn plan_sql(id: &str) -> String {
    format!(
        "INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-{id}', 'lead', 'ctx', '{id}', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('{id}', 'ctx', 'Plan', 'plan', 'lead-{id}', 't', NULL, NULL);"
    )
}

fn workflow(plan: &str, key: &str, repo: &str) -> String {
    format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('{plan}', '{key}', '{repo}', 'main', 'waiting', '{plan}/{key}', cpd_txn(), 't');"
    )
}

fn need(workflow: &str, needs: &str) -> String {
    format!("INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pay', '{workflow}', '{needs}');")
}

const LOCK: &str = "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';";

/// Plan `pay` (in `/r`) with `keys` waiting, the `needs` pairs `(pr, needs)`, then locked.
fn locked(keys: &[&str], needs: &[(&str, &str)]) -> (tempfile::TempDir, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    let mut sql = plan_sql("pay");
    for key in keys {
        sql += &workflow("pay", key, "/r");
    }
    for (p, n) in needs {
        sql += &need(p, n);
    }
    write(&mut store, &sql).unwrap();
    write(&mut store, LOCK).unwrap();
    (dir, store)
}

#[test]
fn prs_locked_lets_a_waiting_pr_stack_on_a_pr_of_its_plan_and_repo_after_the_lock() {
    let (_dir, mut store) = locked(&["a", "b"], &[]);
    write(&mut store, "UPDATE workflows SET base = 'pay/a' WHERE key = 'b'").expect("a restack of a waiting Workflow");
}

#[test]
fn prs_locked_still_refuses_every_other_base_change_of_a_pr_after_the_lock() {
    let (_dir, mut store) = locked(&["a", "b"], &[]);
    write(
        &mut store,
        &format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
               VALUES ('pay', 'o', '/other', 'main', 'waiting', 'pay/o', cpd_txn(), 't');
             {} {}
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
               VALUES ('pay', 'd', '/r', 'main', 'dispatch', 'pay/d', '/w', 't', cpd_txn(), 't');",
            plan_sql("oth"),
            workflow("oth", "x", "/r"),
        ),
    )
    .unwrap();
    for sql in [
        "UPDATE workflows SET base = 'develop' WHERE plan = 'pay' AND key = 'b'",
        "UPDATE workflows SET base = 'pay/o' WHERE plan = 'pay' AND key = 'b'",
        "UPDATE workflows SET base = 'oth/x' WHERE plan = 'pay' AND key = 'b'",
        "UPDATE workflows SET base = 'pay/a' WHERE plan = 'pay' AND key = 'd'",
        "UPDATE workflows SET repo = '/other' WHERE plan = 'pay' AND key = 'b'",
        "UPDATE workflows SET lead_branch = 'pay/a' WHERE plan = 'pay' AND key = 'b'",
    ] {
        assert_locked(&mut store, sql);
    }
}

#[test]
fn pr_needs_no_delete_lets_a_waiting_prs_need_go_when_it_is_the_prs_base_pr_after_the_lock() {
    let (_dir, mut store) = locked(&["a", "b"], &[("b", "a")]);
    write(
        &mut store,
        "UPDATE workflows SET base = 'pay/a' WHERE plan = 'pay' AND key = 'b';
         DELETE FROM workflow_needs WHERE workflow = 'b' AND needs = 'a';",
    )
    .expect("the need on the base Workflow goes with the restack");
    let deleted = store
        .read(|conn| {
            Ok(conn.query_row("SELECT count(*) FROM history WHERE tbl = 'workflow_needs' AND op = 'delete' AND pk = '[\"pay\",\"b\",\"a\"]'", [], |r| r.get::<_, i64>(0))?)
        })
        .unwrap();
    assert_eq!(deleted, 1, "history records the delete");
}

#[test]
fn pr_needs_no_delete_still_refuses_after_the_lock_a_need_that_is_not_the_prs_base_pr() {
    let (_dir, mut store) = locked(&["a", "b", "c"], &[("b", "a"), ("b", "c")]);
    write(&mut store, "UPDATE workflows SET base = 'pay/a' WHERE key = 'b'").ok();
    assert_locked(&mut store, "DELETE FROM workflow_needs WHERE workflow = 'b' AND needs = 'c'");

    let (_dir, mut store) = locked(&["a", "b"], &[("b", "a")]);
    assert_locked(&mut store, "DELETE FROM workflow_needs WHERE workflow = 'b' AND needs = 'a'");

    let (_dir, mut store) = locked(&["a", "d"], &[("d", "a")]);
    write(
        &mut store,
        "UPDATE workflows SET status = 'dispatch', lead_worktree = '/w', started_at = 't', base = 'pay/a' WHERE key = 'd'",
    )
    .ok();
    // Whether or not the base moved, a `dispatch` Workflow keeps its needs.
    assert_locked(&mut store, "DELETE FROM workflow_needs WHERE workflow = 'd' AND needs = 'a'");
}
