//! The Jira title under `workflow.jira_required` (commands.md §9.4, §9.5, contract §7.6 "The Jira title", pro-d399):
//! `workflow add` and `workflow edit` compose `<KEY>/` into the title, or refuse `jira_title_invalid`.

use super::workflow::code;
use super::workflow_crud::{Crud, crud};

/// A Plan with owner policy `workflow.jira_required = <required>`.
fn fx(required: bool) -> Crud {
    let fx = crud("plan");
    fx.env
        .owner_policy("acme", &format!("[workflow]\njira_required = {required}\n"));
    fx
}

fn add(fx: &Crud, key: &str, title: &str, extra: &[&str]) -> crate::common::Run {
    let repo = fx.pay.to_str().unwrap().to_owned();
    let mut args = vec![
        "add", key, "--repo", &repo, "--title", title, "--shape", "config",
    ];
    args.extend(extra);
    fx.pr(&args)
}

fn title_of(fx: &Crud, key: &str) -> String {
    fx.env
        .query::<String>(&format!("SELECT title FROM workflows WHERE key = '{key}'"))
}

#[test]
fn workflow_add_composes_the_jira_key_into_the_title_when_required() {
    let fx = fx(true);

    let out = add(&fx, "k", "feat: Show skeletons", &["--jira", "ABC-12"]).ok();

    assert_eq!(out["workflow"]["title"], "feat: ABC-12/Show skeletons");
    assert_eq!(title_of(&fx, "k"), "feat: ABC-12/Show skeletons");
}

#[test]
fn workflow_add_refuses_a_title_without_a_type_when_jira_is_required() {
    let fx = fx(true);

    let run = add(&fx, "k", "Show skeletons", &["--jira", "ABC-12"]);

    assert_eq!(code(&run), ("jira_title_invalid".into(), 3));
    assert!(
        run.error()["message"]
            .as_str()
            .unwrap()
            .contains("cpd workflow edit k --title"),
        "{}",
        run.error()["message"]
    );
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM workflows"), 0);
}

#[test]
fn workflow_add_leaves_the_title_alone_when_jira_is_not_required() {
    let fx = fx(false);

    let out = add(&fx, "k", "Show skeletons", &["--jira", "ABC-12"]).ok();

    assert_eq!(out["workflow"]["title"], "Show skeletons");
    assert_eq!(title_of(&fx, "k"), "Show skeletons");
}

#[test]
fn workflow_edit_title_composes_the_stored_jira_key() {
    let fx = fx(true);
    add(&fx, "k", "feat: First", &["--jira", "ABC-12"]).ok();

    fx.pr(&["edit", "k", "--title", "fix: Second"]).ok();

    assert_eq!(title_of(&fx, "k"), "fix: ABC-12/Second");
}

#[test]
fn workflow_edit_jira_composes_it_into_the_stored_title() {
    let fx = fx(true);
    add(&fx, "k", "feat: Show skeletons", &[]).ok();

    fx.pr(&["edit", "k", "--jira", "ABC-12"]).ok();

    assert_eq!(title_of(&fx, "k"), "feat: ABC-12/Show skeletons");
}

#[test]
fn workflow_edit_jira_is_refused_when_the_title_holds_another_key() {
    let fx = fx(true);
    add(&fx, "k", "feat: XYZ-9/Show skeletons", &[]).ok();

    let run = fx.pr(&["edit", "k", "--jira", "ABC-12"]);

    assert_eq!(code(&run), ("jira_title_invalid".into(), 3));
    assert_eq!(title_of(&fx, "k"), "feat: XYZ-9/Show skeletons");
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM workflows WHERE jira IS NOT NULL"),
        0
    );
}
