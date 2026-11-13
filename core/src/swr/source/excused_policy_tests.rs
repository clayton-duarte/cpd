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
