//! `sessions.working_pr` and its three triggers (contract §3.6 `The Lead's Workflow`, §4.4; decisions pro-d217,
//! pro-d219, pro-d259): non-null only for a `working` Lead whose Plan has a Workflow with that key; leaving `working`
//! clears it; a new Session names nothing.

use crate::error::{CpdError, ErrorCode};
use crate::store::Store;

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn assert_invalid(store: &mut Store, sql: &str) {
    let err = write(store, sql).expect_err(sql);
    assert_eq!(err.code(), ErrorCode::StoreFailed, "{sql}: {err:?}");
    assert_eq!(
        err.details()["sqlite_message"].as_str().unwrap_or_default(),
        "working_pr_invalid",
        "{sql}"
    );
}

fn working_pr(store: &mut Store, id: &str) -> Option<String> {
    store
        .write("read", |tx| {
            Ok::<_, CpdError>(tx.query_row(
                "SELECT working_pr FROM sessions WHERE id = ?1",
                [id],
                |r| r.get::<_, Option<String>>(0),
            )?)
        })
        .unwrap()
}

fn fixture() -> (tempfile::TempDir, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    let sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at) VALUES
           ('lead-pay', 'lead', 'ctx', 'pay', NULL, 'Lead', '/', 'O3', 'working', 't', 't'),
           ('lead-other', 'lead', 'ctx', 'other', NULL, 'Lead', '/', 'O3', 'working', 't', 't'),
           ('mgr', 'manager', 'ctx', NULL, NULL, 'M', '/', 'O4', 'working', 't', 't'),
           ('w', 'worker', 'ctx', 'pay', 'api-a1', 'W', '/', 'S2', 'working', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
           ('pay', 'ctx', 'Pay', 'plan', 'lead-pay', 't'),
           ('other', 'ctx', 'Other', 'plan', 'lead-other', 't');
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, merge_state, authored,
                                    base_ref, default_branch, created_at, changed_at, merged_at)
           VALUES ('acme/repo', 1, 'https://github.com/acme/repo/pull/1', 'T', 0, 'merged', 'none', 'passed', 'clean', 1,
                   'main', 'main', 't', 't', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
           VALUES ('pay', 'a', '/r', 'main', 'dispatch', 'pay/a', '/wt-a', 't', cpd_txn(), 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pull_request, created_txn, created_at, started_at, merged_at)
           VALUES ('pay', 'm', '/r', 'main', 'merged', 'pay/m', '/wt-m', 1, 'https://github.com/acme/repo/pull/1', 't',
                   (SELECT id FROM pull_requests WHERE number = 1), cpd_txn(), 't', 't', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'c', '/r', 'main', 'cancelled', 'pay/c', cpd_txn(), 't'),
                  ('other', 'x', '/r', 'main', 'waiting', 'other/x', cpd_txn(), 't');
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
           VALUES ('pay', 'a', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{FP}', cpd_txn(), 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
           VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 'w', 't');"
    );
    write(&mut store, &sql).unwrap();
    (dir, store)
}

fn name(workflow: &str) -> String {
    format!("UPDATE sessions SET working_pr = '{workflow}' WHERE id = 'lead-pay'")
}

#[test]
fn working_pr_store_a_working_lead_takes_a_pr_of_its_plan() {
    let (_dir, mut store) = fixture();
    write(&mut store, &name("a")).unwrap();
    assert_eq!(working_pr(&mut store, "lead-pay").as_deref(), Some("a"));
}

#[test]
fn working_pr_store_a_merged_or_cancelled_pr_of_its_plan_may_be_named() {
    let (_dir, mut store) = fixture();
    write(&mut store, &name("m")).unwrap();
    assert_eq!(working_pr(&mut store, "lead-pay").as_deref(), Some("m"));
    write(&mut store, &name("c")).unwrap();
    assert_eq!(working_pr(&mut store, "lead-pay").as_deref(), Some("c"));
}

#[test]
fn working_pr_store_refuses_a_pr_of_another_plan_or_no_pr() {
    let (_dir, mut store) = fixture();
    assert_invalid(&mut store, &name("x"));
    assert_invalid(&mut store, &name("nope"));
    assert_eq!(working_pr(&mut store, "lead-pay"), None);
}

#[test]
fn working_pr_store_refuses_a_lead_that_is_not_working() {
    let (_dir, mut store) = fixture();
    for state in ["idle", "stopped"] {
        write(
            &mut store,
            &format!("UPDATE sessions SET state = '{state}' WHERE id = 'lead-pay'"),
        )
        .unwrap();
        assert_invalid(&mut store, &name("a"));
        assert_eq!(working_pr(&mut store, "lead-pay"), None);
    }
}

#[test]
fn working_pr_store_refuses_a_worker_and_a_manager() {
    let (_dir, mut store) = fixture();
    assert_invalid(
        &mut store,
        "UPDATE sessions SET working_pr = 'a' WHERE id = 'w'",
    );
    assert_invalid(
        &mut store,
        "UPDATE sessions SET working_pr = 'a' WHERE id = 'mgr'",
    );
}

#[test]
fn working_pr_store_leaving_working_clears_it() {
    for (state, label) in [
        ("idle", "NULL"),
        ("waiting", "'l'"),
        ("blocked", "'l'"),
        ("stopped", "NULL"),
    ] {
        let (_dir, mut store) = fixture();
        write(&mut store, &name("a")).unwrap();
        assert_eq!(
            working_pr(&mut store, "lead-pay").as_deref(),
            Some("a"),
            "{state}"
        );
        write(
            &mut store,
            &format!(
                "UPDATE sessions SET state = '{state}', state_label = {label} WHERE id = 'lead-pay'"
            ),
        )
        .unwrap();
        assert_eq!(working_pr(&mut store, "lead-pay"), None, "{state}");
        write(
            &mut store,
            "UPDATE sessions SET state = 'working', state_label = NULL WHERE id = 'lead-pay'",
        )
        .unwrap();
        assert_eq!(
            working_pr(&mut store, "lead-pay"),
            None,
            "re-entering working after {state}"
        );
    }
}

#[test]
fn working_pr_store_a_new_session_cannot_have_one() {
    let (_dir, mut store) = fixture();
    assert_invalid(
        &mut store,
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, working_pr)
           VALUES ('lead-new', 'lead', 'ctx', 'pay', NULL, 'Lead', '/', 'O3', 'working', 't', 't', 'a')",
    );
}

#[test]
fn working_pr_store_staying_in_working_keeps_it() {
    let (_dir, mut store) = fixture();
    write(&mut store, &name("a")).unwrap();
    write(
        &mut store,
        "UPDATE sessions SET state = 'working', state_at = 't2' WHERE id = 'lead-pay'",
    )
    .unwrap();
    assert_eq!(working_pr(&mut store, "lead-pay").as_deref(), Some("a"));
}

#[test]
fn working_pr_store_null_is_always_allowed() {
    let (_dir, mut store) = fixture();
    write(
        &mut store,
        "UPDATE sessions SET state = 'idle' WHERE id = 'lead-pay'",
    )
    .unwrap();
    write(
        &mut store,
        "UPDATE sessions SET working_pr = NULL WHERE id = 'lead-pay'",
    )
    .unwrap();
    assert_eq!(working_pr(&mut store, "lead-pay"), None);
}
