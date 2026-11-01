//! The poll's base read for 'ready to merge' (pro-d118; contract §7.6): `pull_requests.base_ref` and `default_branch` come
//! from `baseRefName` and `repository { defaultBranchRef { name } }` of the one search, with no extra `gh` call. All in-process
//! over a mocked `gh`: no real GitHub.

use serde_json::json;

use super::workflow::{Poll, Rows, node, pay, poll_fx};

/// `node`, as GitHub reports its base branch and the repo's default branch (`None`: `defaultBranchRef` is null).
fn with_base(node: String, base: &str, default: Option<&str>) -> String {
    let default = default.map_or("null".to_owned(), |d| format!(r#"{{"name":"{d}"}}"#));
    let node = node.replacen(
        r#""mergedAt":"#,
        &format!(r#""baseRefName":"{base}","mergedAt":"#),
        1,
    );
    let node = node.replacen(
        r#""},"number":"#,
        &format!(r#"","defaultBranchRef":{default}}},"number":"#),
        1,
    );
    assert!(node.contains("defaultBranchRef"), "{node}");
    node
}

/// `(base_ref, default_branch)` of pull request `number`.
fn stored(fx: &Poll, number: i64) -> (Option<String>, Option<String>) {
    fx.store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT base_ref, default_branch FROM pull_requests WHERE number = ?1",
                [number],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

fn some(base: &str, default: &str) -> (Option<String>, Option<String>) {
    (Some(base.to_owned()), Some(default.to_owned()))
}

#[test]
fn stacked_ready_poll_stores_base_ref_and_default_branch() {
    let fx = poll_fx("deploy");
    fx.search(&[
        with_base(pay("OPEN", None, None), "main", Some("main")),
        with_base(
            node("acme/web", 900, "OPEN", None, None),
            "cpd/pay",
            Some("main"),
        ),
    ]);
    fx.poll();
    assert_eq!(stored(&fx, 812), some("main", "main"));
    assert_eq!(stored(&fx, 900), some("cpd/pay", "main"));
    // No call beyond today's: the one search carries both fields, and no per-PR lookup is made. (The comments read of
    // the owned PR, whose `updated_at` is NULL, is baseline: §7.6 step 3a.)
    let calls = fx.gh_calls();
    let carrying: Vec<_> = calls
        .iter()
        .filter(|c| c[3].contains("defaultBranchRef { name }"))
        .collect();
    assert_eq!(carrying.len(), 1, "{calls:?}");
    assert!(
        carrying[0][3].contains("search(type: ISSUE"),
        "{}",
        carrying[0][3]
    );
    assert!(
        calls
            .iter()
            .all(|c| !c[3].contains("pullRequest(number: 812) { repository")),
        "no lookup: {calls:?}"
    );
}

#[test]
fn stacked_ready_poll_rewrites_the_row_when_the_base_is_retargeted() {
    let fx = poll_fx("deploy");
    fx.search(&[with_base(
        pay("OPEN", None, None),
        "feature-branch",
        Some("main"),
    )]);
    fx.poll();
    assert_eq!(stored(&fx, 812), some("feature-branch", "main"));
    let told = fx.events("workflow-base-changed").len();

    fx.search(&[with_base(pay("OPEN", None, None), "main", Some("main"))]);
    let out = fx.poll();
    assert_eq!(
        stored(&fx, 812),
        some("main", "main"),
        "the retargeted row is rewritten"
    );
    assert_eq!(
        out["base_changed"],
        json!([{"plan": "pay", "pr": "repo", "from": "feature-branch", "to": "main"}])
    );
    assert_eq!(
        fx.events("workflow-base-changed").len(),
        told + 1,
        "the base follow still happens"
    );
}

#[test]
fn stacked_ready_poll_stores_null_default_branch_when_github_has_none() {
    let fx = poll_fx("deploy");
    fx.search(&[with_base(pay("OPEN", None, None), "main", None)]);
    fx.poll();
    assert_eq!(stored(&fx, 812), (Some("main".to_owned()), None));
}
