//! `pr post`'s squash auto-merge (commands.md §9.8b): after the Slack send and the `pr_posts` insert, one read query and the
//! GraphQL mutation `enablePullRequestAutoMerge` (never `gh pr merge`). Its outcome never undoes or fails the post.
//! `gh` and the Slack helper are the repo's fakes; every `gh` call is recorded.

use std::fs;

use cpd_core::host::runner::Reply;
use serde_json::json;

use super::pr_view::{Pay, fx};
use crate::common::refused;

const OK: &str = r#"{"ok":true,"channel":"C0POLICY1","ts":"1793040197.490209","link":"https://acme.slack.com/archives/C0POLICY1/p1793040197490209"}"#;
const AUTH: &str = r#"{"ok":false,"error":{"code":"slack_auth","message":"Slack needs a sign-in. Run: pi mcp login slack","details":{}}}"#;

/// The baseline of `pr_post.rs`: Plan `pay`'s one open PR, ready to post, the helper answering [`OK`].
fn post_with_node(node: Reply) -> Pay {
    let pay = fx("open");
    let env = &pay.fx.env;
    let adapter = env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    env.write_config(&format!(
        "version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n[inbox]\nteam_channel = \"C0MACHINE1\"\nslack_user_id = \"U0USER001\"\n[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        env.tmux_socket()
    ));
    pay.fx.install_pi();
    env.repo_policy("acme", "owner", "[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\n");
    env.repo_policy("acme", "pay", "");
    sql(
        &pay,
        "UPDATE contexts SET owner = 'acme' WHERE id = 'ctx';
         UPDATE prs SET name = 'pay' WHERE plan = 'pay' AND key = 'repo';
         UPDATE pull_requests SET mergeable = 'mergeable', review_decision = 'none', checks = 'passed', authored = 1, feedback = '[]'
           WHERE number = 812;",
    );
    pay.fx.fake_node(node);
    pay
}

fn post() -> Pay {
    post_with_node(Reply::ok(&format!("{OK}\n")))
}

fn sql(pay: &Pay, sql: &str) {
    pay.store().write("fixture", |tx| tx.execute_batch(sql).map_err(Into::into)).unwrap();
}

/// The `pr_posts` row's `(auto_merge, auto_merge_at)`; `None` when there is no row.
fn row(pay: &Pay) -> Option<(String, Option<String>)> {
    pay.store()
        .read(|c| {
            let mut statement = c.prepare("SELECT auto_merge, auto_merge_at FROM pr_posts WHERE plan = 'pay' AND pr = 'repo'")?;
            let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?.into_iter().next())
        })
        .unwrap()
}

fn posts(pay: &Pay) -> i64 {
    pay.store().read(|c| Ok(c.query_row("SELECT count(*) FROM pr_posts", [], |r| r.get(0))?)).unwrap()
}

fn post_cmd(pay: &Pay) -> crate::common::Run {
    pay.fx.cpd(&["pr", "post", "--pr", "repo", "--plan", "pay"])
}

/// Every recorded `gh` call, its argv joined.
fn gh(pay: &Pay) -> Vec<String> {
    pay.fx.calls("gh").into_iter().map(|call| call.args.join(" ")).collect()
}

fn mutations(pay: &Pay) -> Vec<String> {
    gh(pay).into_iter().filter(|c| c.contains("enablePullRequestAutoMerge")).collect()
}

#[test]
fn enables_squash_auto_merge_on_an_unstacked_pr_after_posting() {
    let pay = post();

    let out = post_cmd(&pay).ok();

    let calls = gh(&pay);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[0].contains("autoMergeAllowed") && calls[0].contains("autoMergeRequest"), "the read query first: {calls:?}");
    assert!(calls[1].contains("enablePullRequestAutoMerge") && calls[1].contains("SQUASH"), "then the mutation: {calls:?}");
    assert_eq!(out["auto_merge"], json!({"state": "enabled", "reason": null}));
    let (state, at) = row(&pay).expect("the post row");
    assert_eq!(state, "enabled");
    assert!(at.is_some_and(|at| !at.is_empty()), "auto_merge_at is set");
    assert_eq!(pay.fx.calls("node").len(), 1);
}

#[test]
fn uses_the_graphql_mutation_and_never_gh_pr_merge() {
    let pay = post();

    post_cmd(&pay).ok();

    let calls: Vec<Vec<String>> = pay.fx.calls("gh").into_iter().map(|call| call.args).collect();
    assert!(calls.iter().any(|a| a.join(" ").contains("enablePullRequestAutoMerge")), "the mutation is sent: {calls:?}");
    assert!(calls.iter().all(|a| a[..2] == ["api".to_owned(), "graphql".to_owned()]), "only `gh api graphql`: {calls:?}");
    assert!(calls.iter().all(|a| !(a.iter().any(|w| w == "pr") && a.iter().any(|w| w == "merge"))), "never `gh pr merge`: {calls:?}");
}

#[test]
fn a_repo_that_does_not_allow_auto_merge_still_posts_and_says_not_allowed() {
    let pay = post();
    pay.gh.auto.lock().unwrap().allowed = false;

    let out = post_cmd(&pay).ok();

    assert_eq!(out["auto_merge"]["state"], "not_allowed");
    assert!(out["auto_merge"]["reason"].as_str().is_some_and(|r| !r.is_empty()), "{out}");
    assert!(mutations(&pay).is_empty(), "nothing is requested");
    assert_eq!(row(&pay), Some(("none".to_owned(), None)));
    assert_eq!(pay.fx.calls("node").len(), 1);
}

#[test]
fn a_stacked_pr_is_deferred_with_no_gh_call() {
    let pay = post();
    sql(
        &pay,
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'base', '/nowhere', 'main', 'dispatch', 'cpd/pay/base', '/nowhere-base', cpd_txn(), 't', 't');
         UPDATE prs SET base = 'cpd/pay/base' WHERE plan = 'pay' AND key = 'repo';",
    );

    let out = post_cmd(&pay).ok();

    assert_eq!(out["auto_merge"]["state"], "deferred");
    assert_eq!(row(&pay), Some(("pending".to_owned(), None)));
    assert!(gh(&pay).is_empty(), "no gh call: {:?}", gh(&pay));
}

#[test]
fn a_github_refusal_of_the_mutation_is_stored_refused_and_reported() {
    let pay = post();
    pay.gh.auto.lock().unwrap().refuse = Some("Pull request is in clean status".to_owned());

    let out = post_cmd(&pay).ok();

    assert_eq!(out["auto_merge"]["state"], "refused");
    assert!(out["auto_merge"]["reason"].as_str().unwrap().contains("Pull request is in clean status"), "{out}");
    let (state, at) = row(&pay).expect("the post row");
    assert_eq!(state, "refused");
    assert!(at.is_some(), "auto_merge_at is set");
    assert_eq!(mutations(&pay).len(), 1, "never retried");
}

#[test]
fn a_gh_failure_leaves_the_request_pending_and_the_post_stands() {
    let pay = post();
    pay.gh_fails("gh: HTTP 502");

    let out = post_cmd(&pay).ok();

    assert_eq!(out["auto_merge"]["state"], "failed");
    assert!(out["auto_merge"]["reason"].as_str().is_some_and(|r| !r.is_empty()), "{out}");
    assert_eq!(row(&pay), Some(("pending".to_owned(), None)));
    assert_eq!(pay.fx.calls("node").len(), 1, "the Slack message was sent");
    assert_eq!(posts(&pay), 1);
}

#[test]
fn an_already_enabled_request_counts_as_enabled_without_a_second_mutation() {
    let pay = post();
    pay.gh.auto.lock().unwrap().already = true;

    let out = post_cmd(&pay).ok();

    assert_eq!(out["auto_merge"]["state"], "enabled");
    assert!(mutations(&pay).is_empty(), "{:?}", gh(&pay));
    assert_eq!(row(&pay).map(|r| r.0), Some("enabled".to_owned()));
}

#[test]
fn a_failed_slack_send_makes_no_gh_call_and_writes_no_row() {
    let pay = post_with_node(Reply::exit(3, &format!("{AUTH}\n"), ""));

    refused(&post_cmd(&pay), "slack_auth", 3);

    assert!(gh(&pay).is_empty(), "{:?}", gh(&pay));
    assert_eq!(posts(&pay), 0);
    assert_eq!(row(&pay), None, "no pr_posts row, auto_merge column included");
}

#[test]
fn auto_merge_adds_no_readiness_check_of_its_own() {
    let pay = post();
    let out = post_cmd(&pay).ok();
    assert_eq!(out["ok"], true);
    assert_eq!(out["auto_merge"]["state"], "enabled");
    drop(pay);

    let unready = post();
    sql(&unready, "UPDATE pull_requests SET checks = 'failed' WHERE number = 812;");
    let error = refused(&post_cmd(&unready), "pr_not_ready", 3);
    assert_eq!(error["details"]["reasons"], json!(["checks_failed"]));
    assert!(gh(&unready).is_empty(), "{:?}", gh(&unready));
    assert_eq!(posts(&unready), 0);
}
