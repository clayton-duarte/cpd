//! A PR's Jira ticket (commands.md §9.4 `pr add --jira`, §9.5 `pr edit <key> --jira`, §9.8 `plan show` and `pr view`): stored in
//! `prs.jira`, never filled from the Plan's own key, settable in any phase until the PR ends.

use serde_json::json;

use super::workflow::{Rows, code, poll_fx};
use super::workflow_crud::crud;
use super::pr_view;

/// The stored `jira` of PR `key` of Plan `plan`.
fn stored(mut store: cpd_core::store::Store, plan: &str, key: &str) -> Option<String> {
    store.read(|c| Ok(c.query_row("SELECT jira FROM prs WHERE plan = ?1 AND key = ?2", [plan, key], |r| r.get(0))?)).unwrap()
}

/// How many `prs` rows there are.
fn pr_rows(mut store: cpd_core::store::Store) -> i64 {
    store.read(|c| Ok(c.query_row("SELECT count(*) FROM prs", [], |r| r.get(0))?)).unwrap()
}

#[test]
fn pr_add_with_jira_stores_the_key_and_a_pr_without_it_has_none() {
    let fx = crud("plan");
    let repo = fx.pay.to_str().unwrap().to_owned();

    let out = fx.pr(&["add", "k", "--repo", &repo, "--title", "T", "--jira", "ABC-12"]).ok();
    assert_eq!(out["pr"]["jira"], "ABC-12");
    let out = fx.pr(&["add", "j", "--repo", &repo, "--title", "U"]).ok();
    assert_eq!(out["pr"]["jira"], json!(null));

    let shown = fx.cpd(&["plan", "show", "--plan", "pay-1"]).ok();
    let jira = |key: &str| shown["prs"].as_array().unwrap().iter().find(|p| p["key"] == key).unwrap()["jira"].clone();
    assert_eq!((jira("k"), jira("j")), (json!("ABC-12"), json!(null)));
    assert_eq!(fx.env.query::<String>("SELECT jira FROM prs WHERE key = 'k'"), "ABC-12");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM prs WHERE key = 'j' AND jira IS NULL"), 1);
}

#[test]
fn pr_add_refuses_a_malformed_jira_key() {
    let fx = crud("plan");
    let repo = fx.pay.to_str().unwrap().to_owned();

    fx.pr(&["add", "good", "--repo", &repo, "--title", "T", "--jira", "ABC-1"]).ok();
    for bad in ["abc-1", "A-1", "ABC", "ABC-", "ABC-x"] {
        let run = fx.pr(&["add", "k", "--repo", &repo, "--title", "T", "--jira", bad]);
        assert_eq!(code(&run), ("usage".into(), 2), "{bad}");
        assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM prs"), 1, "{bad}: no PR row");
    }
}

#[test]
fn pr_edit_sets_the_jira_key_after_the_lock_and_again_is_a_no_op() {
    let fx = poll_fx("dispatch");
    let before = fx.events("workflow-edited").len();

    let out = fx.cpd(&["workflow", "edit", "repo", "--jira", "ABC-1", "--plan", "pay"]).ok();

    assert_eq!(out["fields"], json!(["jira"]));
    assert_eq!(stored(fx.store(), "pay", "repo").as_deref(), Some("ABC-1"));
    let events = fx.events("workflow-edited");
    assert_eq!(events.len(), before + 1);
    assert_eq!(events.last().unwrap().1["fields"], json!(["jira"]));

    let txns = fx.txns();
    let out = fx.cpd(&["workflow", "edit", "repo", "--jira", "ABC-1", "--plan", "pay"]).ok();
    assert_eq!(out["already"], true);
    assert_eq!(fx.txns(), txns, "no transaction");
    assert_eq!(fx.events("workflow-edited").len(), before + 1, "no Event");
}

#[test]
fn pr_edit_replaces_a_jira_key_and_sets_it_on_an_open_pr() {
    {
        let fx = poll_fx("dispatch");
        fx.cpd(&["workflow", "edit", "repo", "--jira", "ABC-1", "--plan", "pay"]).ok();

        let out = fx.cpd(&["workflow", "edit", "repo", "--jira", "XYZ-22", "--plan", "pay"]).ok();

        assert_eq!(out["fields"], json!(["jira"]));
        assert_eq!(stored(fx.store(), "pay", "repo").as_deref(), Some("XYZ-22"));
    }
    let fx = pr_view::fx("open");

    let out = fx.fx.cpd(&["workflow", "edit", "repo", "--jira", "ABC-1", "--plan", "pay"]).ok();

    assert_eq!(out["fields"], json!(["jira"]));
    assert_eq!(stored(fx.store(), "pay", "repo").as_deref(), Some("ABC-1"));
}

#[test]
fn pr_edit_jira_is_refused_on_a_merged_pr_and_a_malformed_key() {
    let fx = poll_fx("done");
    let txns = fx.txns();

    let run = fx.cpd(&["workflow", "edit", "repo", "--jira", "ABC-1", "--plan", "pay"]);
    assert_eq!(code(&run), ("workflow_ended".into(), 3));
    let run = fx.cpd(&["workflow", "edit", "repo", "--jira", "abc-1", "--plan", "pay"]);
    assert_eq!(code(&run), ("usage".into(), 2));

    assert_eq!(stored(fx.store(), "pay", "repo"), None);
    assert_eq!(fx.txns(), txns);
}

#[test]
fn pr_edit_with_no_flag_names_jira_among_the_flags() {
    let fx = poll_fx("dispatch");

    let run = fx.cpd(&["workflow", "edit", "repo", "--plan", "pay"]);

    assert_eq!(code(&run), ("nothing_to_edit".into(), 2));
    assert!(run.error()["message"].as_str().unwrap().contains("--jira"), "{}", run.error()["message"]);
}

#[test]
fn plan_show_and_pr_view_print_the_prs_jira_key() {
    let fx = pr_view::fx("open");
    let show = || fx.fx.cpd(&["plan", "show", "--plan", "pay"]).ok()["prs"][0]["jira"].clone();
    let view = || fx.fx.cpd(&["pr", "view", "--workflow", "repo", "--plan", "pay"]).ok()["pr"]["jira"].clone();
    assert_eq!((show(), view()), (json!(null), json!(null)));

    fx.fx.cpd(&["workflow", "edit", "repo", "--jira", "ABC-1", "--plan", "pay"]).ok();

    assert_eq!((show(), view()), (json!("ABC-1"), json!("ABC-1")));
}

#[test]
fn a_plans_jira_key_is_never_its_prs_ticket() {
    let fx = poll_fx("plan");
    fx.store().write("fixture", |tx| Ok(tx.execute_batch("UPDATE plans SET jira = 'PLAN-1' WHERE id = 'pay'")?)).unwrap();

    let shown = fx.cpd(&["plan", "show", "--plan", "pay"]).ok();

    assert_eq!(shown["plan"]["jira"], "PLAN-1");
    assert!(pr_rows(fx.store()) > 0);
    for pr in shown["prs"].as_array().unwrap() {
        assert!(pr.get("jira").is_some_and(|j| j.is_null()), "{pr}");
    }
    assert_eq!(stored(fx.store(), "pay", "repo"), None);
}
