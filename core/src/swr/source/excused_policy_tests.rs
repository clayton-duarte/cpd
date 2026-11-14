//! Bug b54: the SWR github pass resolves `pr.excused_labels` per PR from that PR's repo/owner policy (as `pr/poll.rs` does),
//! not from a static empty list. The policy files are a temp `$CPD_HOME` fixture; `gh` is the `github_tests` fake world.

use super::github_tests::{Rig, pr};

const LABEL: &str = "No Merges on Friday";

fn write_policy(rig: &Rig, rel: &str, text: &str) {
    let path = rig.home.root().join("policy").join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Stored `checks` of one labelled PR in `repo` whose only failing check is `danger`, after one pass.
fn stored_checks(rig: &mut Rig, repo: &'static str) -> Option<String> {
    let mut p = pr(1);
    p.repo = repo;
    p.labels = vec![LABEL];
    p.checks = vec![("danger", "FAILURE"), ("build", "SUCCESS")];
    rig.state().authored = vec![p];
    rig.pass(0).unwrap();
    rig.text(&format!(
        "SELECT checks FROM pull_requests WHERE repo = '{repo}' AND number = 1"
    ))
}

#[test]
fn excused_label_policy_marks_failing_check_excused() {
    let mut rig = Rig::new();
    write_policy(
        &rig,
        "acme/owner.toml",
        &format!("[pr]\nexcused_labels = [\"{LABEL}\"]\n"),
    );
    assert_eq!(
        stored_checks(&mut rig, "acme/pay").as_deref(),
        Some("passed"),
        "the repo's policy excuses the label: the failing check is excused, not failed"
    );
}

#[test]
fn repo_without_excused_labels_stays_failed() {
    let mut rig = Rig::new();
    write_policy(
        &rig,
        "acme/owner.toml",
        &format!("[pr]\nexcused_labels = [\"{LABEL}\"]\n"),
    );
    write_policy(&rig, "beta/owner.toml", "");
    assert_eq!(
        stored_checks(&mut rig, "beta/web").as_deref(),
        Some("failed"),
        "a repo whose policy excuses nothing keeps the failure"
    );
}

// Bug b61 (#1176, a regression of b54). Assumption A1: under a policy hold, a failed rollup whose individual checks are
// unknown (empty or absent contexts) is NOT stored as `failed`; only a non-Danger failing check seen in the contexts may fail it.

const HOLD_LABELS: [&str; 2] = ["Dont Merge", LABEL];

fn hold_policy(rig: &Rig) {
    write_policy(
        rig,
        "acme/owner.toml",
        "[pr]\nexcused_labels = [\"Dont Merge\", \"No Merges on Friday\"]\n",
    );
}

fn stored(rig: &mut Rig) -> Option<String> {
    rig.text("SELECT checks FROM pull_requests WHERE repo = 'acme/pay' AND number = 1")
}

#[test]
fn excused_label_with_empty_contexts_and_failed_rollup_is_not_failed() {
    let mut rig = Rig::new();
    hold_policy(&rig);
    // GitHub's FAILURE rollup, its check contexts absent (the light read).
    rig.state().light_only = true;
    let mut p = pr(1);
    p.labels = HOLD_LABELS.to_vec();
    p.checks = vec![("danger", "FAILURE")];
    rig.state().authored = vec![p];
    rig.pass(0).unwrap();
    assert_ne!(
        stored(&mut rig).as_deref(),
        Some("failed"),
        "a held PR whose failing checks are unknown is not stored as failed"
    );
}

#[test]
fn excused_label_with_both_danger_entries_is_not_failed() {
    let mut rig = Rig::new();
    hold_policy(&rig);
    // The light search read carries the FAILURE rollup without contexts; the row is written before any hydration.
    rig.state().light_only = true;
    let mut p = pr(1);
    p.labels = HOLD_LABELS.to_vec();
    p.checks = vec![("danger", "FAILURE")];
    p.contexts = vec![("Danger", "FAILURE")];
    rig.state().world.insert(("acme/pay".into(), 1), p.clone());
    rig.state().authored = vec![p];
    rig.pass(0).unwrap();
    assert_ne!(
        stored(&mut rig).as_deref(),
        Some("failed"),
        "the failing CheckRun `danger` and StatusContext `Danger` are both excused"
    );
}

#[test]
fn excused_hold_raises_no_checks_failed_feedback() {
    use crate::inbox::rules::conditions_with;
    use time::macros::datetime;

    let mut rig = Rig::new();
    hold_policy(&rig);
    // The light search read carries the FAILURE rollup without contexts; the row is written before any hydration.
    rig.state().light_only = true;
    let mut p = pr(1);
    p.labels = HOLD_LABELS.to_vec();
    p.checks = vec![("danger", "FAILURE")];
    p.contexts = vec![("Danger", "FAILURE")];
    rig.state().world.insert(("acme/pay".into(), 1), p.clone());
    rig.state().authored = vec![p];
    rig.pass(0).unwrap();
    // An open Workflow of an idle Lead owns the row: the Inbox and `pr-feedback` look at it as they would live.
    let seed = "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'deploy', 'l-pay', 't', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pull_request, pr_number, pr_url,
                                pr_opened_at, created_txn, created_at, started_at)
           VALUES ('pay', 'p1', '/r', 'main', 'open', 'cpd/pay/p1', '/lw',
                   (SELECT id FROM pull_requests WHERE number = 1), 1, 'u', 't', 1, 't', 't');";
    rig.store
        .write("test seed", |tx| {
            tx.execute_batch(seed)?;
            Ok::<(), crate::error::CpdError>(())
        })
        .unwrap();
    let conditions = conditions_with(
        &mut rig.store,
        &rig.home,
        datetime!(2026-01-02 00:00 UTC),
        20,
        1,
        None,
    )
    .unwrap();
    let broken: Vec<_> = conditions
        .iter()
        .filter(|c| c.subject == "pr:acme/pay#1")
        .filter(|c| {
            let label = c.label.to_lowercase();
            label.contains("failing") || label.contains("failed") || label.contains("fixing")
        })
        .collect();
    assert!(broken.is_empty(), "the hold wakes no Lead: {broken:?}");
    assert_eq!(
        rig.int("SELECT count(*) FROM events WHERE kind = 'pr-feedback'"),
        0
    );
    assert_ne!(stored(&mut rig).as_deref(), Some("failed"));
}
