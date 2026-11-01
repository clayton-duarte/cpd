//! `jobs_locked` (contract §4.4; ADR-0014): the Spec of a `queued` Job with no Attempt may change, nothing else of a Locked Job may.

use super::*;
use crate::error::CpdError;
use crate::store::Store;

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn message(store: &mut Store, sql: &str) -> String {
    write(store, sql).expect_err(sql).details()["sqlite_message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// One queued Job of `kind` and spec columns `columns`/`values`.
fn queued(key: &str, kind: &str, columns: &str, values: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind{columns}, fingerprint, created_txn, created_at)
         VALUES ('pl', 'a', '{key}', 'T', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, '{kind}'{values}, NULL, cpd_txn(), 't');
         UPDATE jobs SET status = 'queued', fingerprint = '{FP}' WHERE key = '{key}';"
    )
}

fn attempt(key: &str, ended: bool) -> String {
    let mut sql = format!(
        "UPDATE jobs SET status = 'running' WHERE key = '{key}';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
         VALUES ('pl', '{key}-a1', '{key}', 1, 'S2', 'ladder', 'running', 'cpd/pl--{key}-a1', '/w', 't');"
    );
    if ended {
        sql += &format!(
            "UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'timed_out', ended_at = 't' WHERE id = '{key}-a1';"
        );
    }
    sql
}

/// A fully migrated store with Plan `pl` (locked), its PR `a` and the Jobs the tests name.
fn store() -> (tempfile::TempDir, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    write(
        &mut store,
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pl', 'ctx', 'Plan', 'dispatch', 'lead', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pl', 'a', '/r', 'main', 'waiting', 'cpd/pl/a', cpd_txn(), 't');",
    )
    .unwrap();
    (dir, store)
}

fn with(sql: &[String]) -> (tempfile::TempDir, Store) {
    let (dir, mut store) = store();
    write(&mut store, &sql.concat()).unwrap();
    (dir, store)
}

fn test_job(key: &str) -> String {
    queued(
        key,
        "test",
        ", tests, test_files",
        ", '[\"t::one\"]', '[\"tests/t.rs\"]'",
    )
}

const SPEC: [&str; 5] = [
    "goal = 'new goal'",
    "context = 'new context'",
    "touches = '[\"z\"]'",
    "acceptance = '[\"w\"]'",
    "out_of_scope = '[\"v\"]'",
];

#[test]
fn jobs_locked_lets_the_spec_of_a_queued_job_with_no_attempt_change() {
    let (_dir, mut store) = with(&[queued("q", "work", "", ""), test_job("t")]);
    for set in SPEC {
        write(
            &mut store,
            &format!("UPDATE jobs SET {set} WHERE key = 'q'"),
        )
        .unwrap_or_else(|e| panic!("{set}: {e:?}"));
    }
    write(
        &mut store,
        &format!("UPDATE jobs SET {} WHERE key = 't'", SPEC.join(", ")),
    )
    .unwrap();
    write(
        &mut store,
        "UPDATE jobs SET tests = '[\"t::two\"]' WHERE key = 't'",
    )
    .unwrap();
    write(
        &mut store,
        "UPDATE jobs SET test_files = '[\"tests/u.rs\"]' WHERE key = 't'",
    )
    .unwrap();
    write(
        &mut store,
        "UPDATE jobs SET tests = '[\"t::three\"]', test_files = '[\"tests/v.rs\"]' WHERE key = 't'",
    )
    .unwrap();
}

#[test]
fn jobs_locked_still_refuses_the_other_locked_columns_of_that_job() {
    let (_dir, mut store) = with(&[
        queued("q", "work", "", ""),
        queued("i", "impl", "", ""),
        test_job("t"),
    ]);
    store.write("fixture", |tx| {
        tx.execute_batch("INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at) VALUES ('pl', 'b', '/r', 'main', 'waiting', 'cpd/pl/b', cpd_txn(), 't');")?;
        Ok::<(), CpdError>(())
    })
    .unwrap();
    // The Spec change that is allowed is made first: the refusal below is of the other column alone.
    for set in [
        "title = 'New'",
        "tier = 'S3'",
        "pr = 'b'",
        "review = 1",
        "kind = 'docs'",
    ] {
        let sql = format!("UPDATE jobs SET goal = 'other goal', {set} WHERE key = 'q'");
        assert_eq!(message(&mut store, &sql), "job_locked", "{sql}");
    }
    write(
        &mut store,
        "UPDATE jobs SET goal = 'other goal' WHERE key = 'q'",
    )
    .unwrap();
    for sql in [
        "UPDATE jobs SET kind = 'impl' WHERE key = 't'",
        "UPDATE jobs SET kind = 'work' WHERE key = 'i'",
    ] {
        assert_eq!(message(&mut store, sql), "job_locked", "{sql}");
    }
}

#[test]
fn jobs_locked_refuses_the_spec_once_the_job_has_an_attempt_or_another_status() {
    let (_dir, mut store) = with(&[
        queued("retried", "work", "", ""),
        queued("running", "work", "", ""),
        queued("failed", "work", "", ""),
        queued("merged", "work", "", ""),
        queued("gone", "work", "", ""),
        attempt("retried", true),
        // Retried: a Job that failed and was queued again keeps its Attempt row.
        "UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'retried';
         UPDATE jobs SET status = 'queued', failed_cause = NULL WHERE key = 'retried';"
            .to_owned(),
        attempt("running", false),
        attempt("failed", true),
        "UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'failed';".to_owned(),
        attempt("merged", false),
        "UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'merged';
         UPDATE jobs SET status = 'cancelled', cancel_reason = 'no', cancel_requested_at = 't' WHERE key = 'gone';"
            .to_owned(),
    ]);
    for key in ["retried", "running", "failed", "merged", "gone"] {
        for set in SPEC {
            let sql = format!("UPDATE jobs SET {set} WHERE key = '{key}'");
            assert_eq!(message(&mut store, &sql), "job_locked", "{sql}");
        }
    }
}

#[test]
fn jobs_locked_refuses_the_spec_of_open_and_update_jobs() {
    let (_dir, mut store) = with(&[queued("o", "open", "", ""), queued("u", "update", "", "")]);
    for key in ["o", "u"] {
        for set in SPEC {
            let sql = format!("UPDATE jobs SET {set} WHERE key = '{key}'");
            assert_eq!(message(&mut store, &sql), "job_locked", "{sql}");
        }
    }
}
