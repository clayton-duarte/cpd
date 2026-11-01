//! The Lead Branch name `<plan>/<key>` (contract §2.1, §4.4, migration `lead-branch-name`): `prs.lead_branch` and
//! `attempts.branch` accept it beside the two legacy forms, and `prs_locked` no longer lets a legacy name move.

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

fn message(result: Result<(), CpdError>, what: &str) -> String {
    result.expect_err(what).details()["sqlite_message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// A fully migrated store with Plan `pay` (in `phase`) and its Lead.
fn fresh(phase: &str) -> (tempfile::TempDir, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    let (locked, done) = match phase {
        "plan" => ("NULL", "NULL"),
        "dispatch" => ("'t'", "NULL"),
        _ => ("'t'", "'t'"),
    };
    write(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('pay', 'ctx', 'Plan', '{phase}', 'lead', 't', {locked}, {done});"
        ),
    )
    .unwrap();
    (dir, store)
}

fn pr(plan: &str, key: &str, lead_branch: &str) -> String {
    format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('{plan}', '{key}', '/r', 'main', 'waiting', '{lead_branch}', cpd_txn(), 't');"
    )
}

#[test]
fn prs_accepts_a_plan_slash_key_lead_branch() {
    let (_dir, mut store) = fresh("plan");
    write(&mut store, &pr("pay", "api", "pay/api")).expect("`<plan>/<key>` is a Lead Branch");
}

#[test]
fn prs_still_accepts_the_two_legacy_lead_branch_forms() {
    let (_dir, mut store) = fresh("plan");
    write(&mut store, &pr("pay", "api", "cpd/pay/api")).expect("`cpd/<plan>/<key>`");
    write(&mut store, &pr("pay", "old", "cpd/pay")).expect("`cpd/<plan>`");
}

#[test]
fn prs_refuses_a_lead_branch_that_is_not_derived_from_plan_and_key() {
    let (_dir, mut store) = fresh("plan");
    for (key, branch) in [
        ("api", "other/api"),
        ("api", "pay/other"),
        ("api", "cpd/other/api"),
    ] {
        let failed = message(write(&mut store, &pr("pay", key, branch)), branch);
        assert!(failed.contains("CHECK"), "{branch}: {failed}");
    }
    // Refused for the CHECK, so the same PR with its own name goes in.
    write(&mut store, &pr("pay", "api", "pay/api")).expect("the derived name");
}

/// PR `api` with a queued Job `j` and, over it, one running Attempt on `branch`.
fn attempt_on(branch: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
         VALUES ('pay', 'api', 'j', 'T', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, 'work', NULL, cpd_txn(), 't');
         UPDATE jobs SET status = 'queued', fingerprint = '{FP}' WHERE key = 'j';
         UPDATE jobs SET status = 'running' WHERE key = 'j';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
         VALUES ('pay', 'j-a1', 'j', 1, 'S2', 'ladder', 'running', '{branch}', '/w', 't');"
    )
}

#[test]
fn attempts_accepts_a_linear_attempt_on_a_plan_slash_key_branch() {
    let (_dir, mut store) = fresh("plan");
    write(&mut store, &pr("pay", "api", "pay/api")).expect("the PR");
    write(&mut store, &attempt_on("pay/api"))
        .expect("a linear Attempt's branch is its PR's Lead Branch");

    let (_dir, mut store) = fresh("plan");
    write(&mut store, &pr("pay", "api", "pay/api")).expect("the PR");
    write(&mut store, &attempt_on("cpd/pay--j-a1"))
        .expect("a parallel Attempt branch keeps its name");

    let (_dir, mut store) = fresh("plan");
    write(&mut store, &pr("pay", "api", "pay/api")).expect("the PR");
    let failed = message(
        write(&mut store, &attempt_on("paid/api")),
        "another plan's branch",
    );
    assert!(failed.contains("CHECK"), "{failed}");
}

#[test]
fn prs_locked_refuses_moving_a_legacy_lead_branch() {
    for phase in ["dispatch", "done"] {
        let (_dir, mut store) = fresh(phase);
        write(
            &mut store,
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/pay', 1, 'u', 'T', 0, 'merged', 'none', 'none', 't', 't', 't');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                              pr_number, pr_url, pr_opened_at, pull_request, merged_at)
               VALUES ('pay', 'api', '/r', 'main', 'merged', 'cpd/pay', '/w', cpd_txn(), 't', 't', 1, 'u', 't',
                       (SELECT id FROM pull_requests WHERE number = 1), 't');",
        )
        .unwrap();
        let moved = write(
            &mut store,
            "UPDATE prs SET lead_branch = 'cpd/pay/api' WHERE key = 'api'",
        );
        assert!(message(moved, phase).contains("pr_locked"), "{phase}");
    }
}
