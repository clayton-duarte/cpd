//! The parked-draft predicate (contract §2.2 "A parked draft", hs-d822) over in-memory stores: `parked_drafts` and the door.
//!
//! A `draft` Job is legal in exactly two forms (hs-d822, superseding hs-d731): incomplete (a stub, no Tier or no Spec),
//! or covered by a current Plan review. Every other draft is parked, in particular a complete draft whose needs chain
//! to a `queued`, `running` or `failed` work Job (added queued with `--needs` instead) or to a `merged` one.

use super::jobs;
use super::parked::{door, parked_drafts};
use crate::error::ErrorCode;
use crate::home::CpdHome;
use crate::ids::PlanId;
use crate::plan::plan_fingerprint;
use crate::store::Store;

fn plan() -> PlanId {
    PlanId::parse("pl").unwrap()
}

/// A store with Plan `pl` in `phase` and its Workflow `r` (`dispatch`), then `extra` SQL.
fn store(phase: &str, extra: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let locked = if phase == "plan" { "NULL" } else { "'t'" };
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES ('pl', 'ctx', 'P', '{phase}', 'lead', 't', {locked});
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pl', 'r', '/r', 'main', 'dispatch', 'cpd/pl', '/w', cpd_txn(), 't', 't');
                 {extra}"
            ))?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

/// SQL for the Job `key` of Workflow `pr`: a stub draft, a full draft (`full`) or any other status, with its needs.
fn job(key: &str, workflow: &str, kind: &str, status: &str, full: bool, needs: &[&str]) -> String {
    let (tier, spec) = if full || status != "draft" {
        ("'S2'", "'g', 'c', '[\"a\"]', '[\"b\"]', '[]'")
    } else {
        ("NULL", "NULL, NULL, NULL, NULL, NULL")
    };
    let fingerprint = if status == "draft" {
        "NULL".to_owned()
    } else {
        format!("'{}'", "f".repeat(64))
    };
    let column = |wanted: &str, value: &str| {
        if status == wanted {
            value.to_owned()
        } else {
            "NULL".to_owned()
        }
    };
    let (failed, merged, cancelled) = (
        column("failed", "'timed_out'"),
        column("merged", "'abc'"),
        column("cancelled", "'x'"),
    );
    let mut sql = format!(
        "INSERT INTO jobs (plan, workflow, key, title, kind, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, failed_cause, merged_sha, cancel_reason, cancel_requested_at, created_txn, created_at)
         VALUES ('pl', '{workflow}', '{key}', 'T', '{kind}', {tier}, {spec}, '{status}', 1, {fingerprint}, {failed}, {merged}, {cancelled}, {cancelled}, cpd_txn(), 't');"
    );
    for need in needs {
        sql.push_str(&format!(
            "INSERT INTO dependencies (plan, job, needs) VALUES ('pl', '{key}', '{need}');"
        ));
    }
    sql
}

fn parked(store: &mut Store) -> Vec<String> {
    store.read(|conn| parked_drafts(conn, &plan())).unwrap()
}

fn stub(key: &str, needs: &[&str]) -> String {
    job(key, "r", "work", "draft", false, needs)
}

// --- The new rule: a stub is legal; a complete draft waiting on a work Job is parked. ---

#[test]
fn an_incomplete_draft_with_no_review_is_parked() {
    let (_d, mut s) = store("plan", &stub("k", &[]));
    assert_eq!(parked(&mut s), ["k"], "plan: a stub is not exempt");
    let (_d, mut s) = store("dispatch", &stub("k", &[]));
    assert_eq!(parked(&mut s), ["k"], "dispatch: a stub is not exempt");
}

#[test]
fn a_complete_draft_waiting_on_a_queued_work_job_is_parked() {
    let (_d, mut s) = store(
        "dispatch",
        &format!(
            "{}{}",
            job("a", "r", "work", "queued", true, &[]),
            job("k", "r", "work", "draft", true, &["a"])
        ),
    );
    assert_eq!(parked(&mut s), ["k"]);
}

#[test]
fn a_complete_draft_waiting_on_a_running_work_job_is_parked() {
    let (_d, mut s) = store(
        "dispatch",
        &format!(
            "{}{}",
            job("a", "r", "work", "running", true, &[]),
            job("k", "r", "work", "draft", true, &["a"])
        ),
    );
    assert_eq!(parked(&mut s), ["k"]);
}

#[test]
fn a_complete_draft_waiting_on_a_failed_work_job_is_parked() {
    let (_d, mut s) = store(
        "dispatch",
        &format!(
            "{}{}",
            job("a", "r", "work", "failed", true, &[]),
            job("k", "r", "work", "draft", true, &["a"])
        ),
    );
    assert_eq!(parked(&mut s), ["k"]);
}

#[test]
fn a_complete_draft_waiting_on_a_merged_work_job_is_parked() {
    // The queued sibling pins the rule: a work Job is never a legal wait, whatever its status. Today's code keeps the
    // queued one legal, so this fails for the right reason.
    let (_d, mut s) = store(
        "dispatch",
        &format!(
            "{}{}{}{}",
            job("m", "r", "work", "merged", true, &[]),
            job("q", "r", "work", "queued", true, &[]),
            job("km", "r", "work", "draft", true, &["m"]),
            job("kq", "r", "work", "draft", true, &["q"])
        ),
    );
    assert_eq!(parked(&mut s), ["km", "kq"]);
}

#[test]
fn a_complete_draft_waiting_through_a_draft_on_a_work_job_is_parked() {
    let (_d, mut s) = store(
        "dispatch",
        &format!(
            "{}{}{}",
            job("w", "r", "work", "queued", true, &[]),
            stub("d", &["w"]),
            job("k", "r", "work", "draft", true, &["d"])
        ),
    );
    assert_eq!(parked(&mut s), ["k"]);
}

const WEB_NEEDS_R: &str =
    "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
     VALUES ('pl', 'web', '/web', 'main', 'waiting', 'cpd/pl/web', cpd_txn(), 't');
     INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pl', 'web', 'r');";

#[test]
fn a_complete_draft_of_a_workflow_that_waits_for_another_workflow_is_parked() {
    // hs-d731's Workflow-waits clause is gone: a Workflow that waits for another Workflow no longer keeps a complete draft legal.
    let extra = format!(
        "{WEB_NEEDS_R}{}",
        job("k", "web", "work", "draft", true, &[])
    );
    let (_d, mut s) = store("dispatch", &extra);
    assert_eq!(
        parked(&mut s),
        ["k"],
        "dispatch: the waiting Workflow no longer exempts"
    );
    let (_d, mut s) = store("plan", &extra);
    assert_eq!(parked(&mut s), ["k"]);
}

// --- The door. ---

#[test]
fn the_door_refuses_a_complete_draft_that_needs_a_queued_work_job() {
    let (_d, mut s) = store("dispatch", &job("a", "r", "work", "queued", true, &[]));
    let all = s.read(|conn| jobs::load(conn, &plan())).unwrap();
    let mut row = all[0].clone();
    // A complete draft (Tier and Spec kept from the queued Job), needing the queued work Job `a`.
    (row.key, row.status, row.needs) = ("k".into(), "draft".into(), vec!["a".into()]);
    let e = s
        .read(|conn| door(conn, &plan(), "dispatch", &all, &row))
        .unwrap_err();
    assert_eq!(e.code(), ErrorCode::DraftParked);
}

#[test]
fn the_door_refuses_a_stub_that_needs_a_queued_work_job() {
    let (_d, mut s) = store("dispatch", &job("a", "r", "work", "queued", true, &[]));
    let all = s.read(|conn| jobs::load(conn, &plan())).unwrap();
    let mut row = all[0].clone();
    (row.key, row.status, row.tier, row.spec, row.fingerprint) =
        ("k".into(), "draft".into(), None, None, None);
    row.needs = vec!["a".into()];
    let e = s
        .read(|conn| door(conn, &plan(), "dispatch", &all, &row))
        .unwrap_err();
    assert_eq!(e.code(), ErrorCode::DraftParked);
}

#[test]
fn the_door_allows_a_draft_needing_a_merged_job() {
    let (_d, mut s) = store("dispatch", &job("a", "r", "work", "merged", true, &[]));
    let all = s.read(|conn| jobs::load(conn, &plan())).unwrap();
    let mut row = all[0].clone();
    (row.key, row.status, row.fingerprint) = ("k".into(), "draft".into(), None);
    row.needs = vec!["a".into()];
    s.read(|conn| door(conn, &plan(), "dispatch", &all, &row))
        .expect("a merged need is finished");
}

#[test]
fn the_door_allows_a_draft_needing_a_draft_that_waits_on_nothing() {
    let (_d, mut s) = store("dispatch", &job("a", "r", "work", "draft", true, &[]));
    let all = s.read(|conn| jobs::load(conn, &plan())).unwrap();
    let mut row = all[0].clone();
    row.key = "b".into();
    row.needs = vec!["a".into()];
    s.read(|conn| door(conn, &plan(), "dispatch", &all, &row))
        .expect("B needs draft A, A waits on nothing");
}

// --- Kept under the new rule. ---

#[test]
fn a_full_draft_is_parked_unless_a_current_pending_review_covers_it() {
    let (_d, mut s) = store("dispatch", &job("k", "r", "work", "draft", true, &[]));
    assert_eq!(parked(&mut s), ["k"]);
    let fingerprint = s.read(|conn| plan_fingerprint(conn, &plan())).unwrap();
    let review = |status: &str, fingerprint: &str| {
        let decided = if status == "pending" { "NULL" } else { "'t'" };
        format!(
            "INSERT INTO plan_reviews (plan, id, number, fingerprint, title, status, requested_at, decided_at) VALUES ('pl', 'r1', 1, '{fingerprint}', 'T', '{status}', 't', {decided});"
        )
    };
    for (status, fp, expected) in [
        ("pending", fingerprint.as_str(), 0),
        ("pending", "0".repeat(64).as_str(), 1),
        ("approved", fingerprint.as_str(), 1),
    ] {
        let (_d, mut s) = store(
            "dispatch",
            &format!(
                "{}{}",
                job("k", "r", "work", "draft", true, &[]),
                review(status, fp)
            ),
        );
        assert_eq!(parked(&mut s).len(), expected, "{status} {fp}");
    }
}

#[test]
fn the_open_jobs_draft_is_exempt() {
    let (_d, mut s) = store("dispatch", &job("open-r", "r", "open", "draft", false, &[]));
    assert!(parked(&mut s).is_empty());
}

#[test]
fn an_archived_plan_has_none() {
    let (_d, mut s) = store(
        "dispatch",
        &format!("{}UPDATE plans SET archived_at = 't';", stub("k", &[])),
    );
    assert!(parked(&mut s).is_empty());
}

fn review(status: &str, fingerprint: &str) -> String {
    let decided = if status == "pending" { "NULL" } else { "'t'" };
    format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, title, status, requested_at, decided_at) VALUES ('pl', 'r1', 1, '{fingerprint}', 'T', '{status}', 't', {decided});"
    )
}

fn current_fingerprint() -> String {
    let (_d, mut s) = store("plan", &job("k", "r", "work", "draft", true, &[]));
    s.read(|conn| plan_fingerprint(conn, &plan())).unwrap()
}

#[test]
fn an_approved_current_review_covers_a_draft_in_phase_plan() {
    let fingerprint = current_fingerprint();
    let draft = job("k", "r", "work", "draft", true, &[]);
    let (_d, mut s) = store(
        "plan",
        &format!("{draft}{}", review("approved", &fingerprint)),
    );
    assert!(
        parked(&mut s).is_empty(),
        "approved and current in phase plan covers the draft"
    );
    let (_d, mut s) = store(
        "dispatch",
        &format!("{draft}{}", review("approved", &fingerprint)),
    );
    assert_eq!(parked(&mut s), ["k"], "only phase plan gains the case");
}

#[test]
fn an_approved_stale_or_rejected_review_does_not_cover_a_draft_in_phase_plan() {
    let current = current_fingerprint();
    let draft = job("k", "r", "work", "draft", true, &[]);
    for (status, fp) in [("approved", "0".repeat(64)), ("rejected", current)] {
        let (_d, mut s) = store("plan", &format!("{draft}{}", review(status, &fp)));
        assert_eq!(parked(&mut s), ["k"], "{status} {fp}");
    }
}
