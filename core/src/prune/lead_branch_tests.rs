//! `cpd prune` and the Lead Branch `<plan>/<key>` (pro-d112, pro-d113 A6), on the fixture of `tests.rs`.

use std::collections::BTreeSet;

use super::tests::{Fixture, lines};

const PRS_OF_PLAN_PAY: &str =
    r#"[{"number":1,"headRefName":"pay/api","baseRefName":"main","headRefOid":"HEAD"}]"#;

#[test]
fn prune_matches_a_parallel_attempt_to_a_merged_plan_slash_key_pr() {
    let mut fx = Fixture::new();
    let wt = fx.worktree("attempt", "cpd/pay--api-impl-a2", "main", Some("a.txt"));
    let head = fx.head(&wt);
    fx.merged_prs(&PRS_OF_PLAN_PAY.replace("HEAD", &head));

    let out = fx.run(false);
    assert_eq!((out.exit, out.stderr.as_str()), (0, ""), "{}", out.stdout);
    let expected: BTreeSet<String> = [
        format!("remove {} (cpd/pay--api-impl-a2)", wt.display()),
        format!("{}: 1 remove, 0 keep", fx.repo.dir().display()),
    ]
    .into_iter()
    .collect();
    assert_eq!(lines(&out.stdout), expected, "{}", out.stdout);
}

#[test]
fn prune_keeps_a_key_with_a_double_dash_whole() {
    // `--drop pay/a--b` drops the worktree on the Lead Branch `pay/a--b`.
    let mut fx = Fixture::new();
    let wt = fx.worktree("lead", "pay/a--b", "main", Some("a.txt"));
    let out = fx.prune(&["--drop", "pay/a--b"]);
    assert_eq!((out.exit, out.stderr.as_str()), (0, ""), "{}", out.stdout);
    assert!(
        out.stdout
            .contains(&format!("remove {} (pay/a--b): dropped", wt.display())),
        "{}",
        out.stdout
    );

    // A merged Workflow with head `pay/a--b` matches it.
    let mut fx = Fixture::new();
    let wt = fx.worktree("lead", "pay/a--b", "main", Some("a.txt"));
    let head = fx.head(&wt);
    fx.merged_prs(&format!(
        r#"[{{"number":1,"headRefName":"pay/a--b","baseRefName":"main","headRefOid":"{head}"}}]"#
    ));
    let out = fx.run(false);
    assert_eq!((out.exit, out.stderr.as_str()), (0, ""), "{}", out.stdout);
    assert!(
        out.stdout
            .contains(&format!("remove {} (pay/a--b)", wt.display())),
        "{}",
        out.stdout
    );
}

#[test]
fn guard_drop_removes_a_clean_unmerged_plan_slash_key_worktree() {
    let mut fx = Fixture::new();
    let wt = fx.worktree("lead", "pay/api", "main", Some("a.txt"));
    let out = fx.prune(&["--drop", "pay/api"]);
    assert_eq!((out.exit, out.stderr.as_str()), (0, ""), "{}", out.stdout);
    assert!(
        out.stdout
            .contains(&format!("remove {} (pay/api): dropped", wt.display())),
        "{}",
        out.stdout
    );
}

#[test]
fn guard_a_legacy_attempt_still_matches_head_cpd_plan_slash_key() {
    let mut fx = Fixture::new();
    let wt = fx.worktree("attempt", "cpd/pay--api-impl-a2", "main", Some("a.txt"));
    let head = fx.head(&wt);
    fx.merged_prs(&format!(
        r#"[{{"number":1,"headRefName":"cpd/pay/api","baseRefName":"main","headRefOid":"{head}"}}]"#
    ));
    let out = fx.run(false);
    assert!(
        out.stdout
            .contains(&format!("remove {} (cpd/pay--api-impl-a2)", wt.display())),
        "{}",
        out.stdout
    );
}
