//! `pr post`'s auto-merge request (commands.md §9.8b): `pr post` queues the post and makes no GitHub call; the merge methods it
//! checks are the cache's (`swr_repos`). The auto-merge is queued when the Slack send confirms (swr-slack.md §6.2) and enabled
//! by the outbox drain, which the tests of cli/tests/engine/watch_auto_merge.rs run. `gh` and the Slack helper are the repo's
//! fakes; every `gh` call is recorded.

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
    env.repo_policy(
        "acme",
        "owner",
        "[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\n",
    );
    env.repo_policy("acme", "pay", "");
    sql(
        &pay,
        "UPDATE contexts SET owner = 'acme' WHERE id = 'ctx';
         UPDATE workflows SET name = 'pay' WHERE plan = 'pay' AND key = 'repo';
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
    pay.store()
        .write("fixture", |tx| tx.execute_batch(sql).map_err(Into::into))
        .unwrap();
}

/// The `pr_posts` row's `(auto_merge, auto_merge_at)`; `None` when there is no row.
fn row(pay: &Pay) -> Option<(String, Option<String>)> {
    pay.store()
        .read(|c| {
            let mut statement = c.prepare("SELECT auto_merge, auto_merge_at FROM pr_posts WHERE plan = 'pay' AND workflow = 'repo'")?;
            let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?.into_iter().next())
        })
        .unwrap()
}

fn posts(pay: &Pay) -> i64 {
    pay.store()
        .read(|c| Ok(c.query_row("SELECT count(*) FROM pr_posts", [], |r| r.get(0))?))
        .unwrap()
}

fn post_cmd(pay: &Pay) -> crate::common::Run {
    pay.fx
        .cpd(&["pr", "post", "--workflow", "repo", "--plan", "pay"])
}

/// Every recorded `gh` call, its argv joined.
fn gh(pay: &Pay) -> Vec<String> {
    pay.fx
        .calls("gh")
        .into_iter()
        .map(|call| call.args.join(" "))
        .collect()
}

fn mutations(pay: &Pay) -> Vec<String> {
    gh(pay)
        .into_iter()
        .filter(|c| c.contains("enablePullRequestAutoMerge"))
        .collect()
}

// `pr post` only validates and queues the post (swr-slack.md §5): the send, the `pr_posts` row and the auto-merge run in the
// outbox. What this file keeps is what `pr post` does before the queue: the merge-method check from the cache, the refusals,
// and that no GitHub call, no GitHub mutation and no Slack send happen at enqueue.

#[test]
fn enqueues_the_post_with_no_gh_call_and_no_send() {
    let pay = post();

    let out = post_cmd(&pay).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert!(
        out.get("auto_merge").is_none(),
        "no auto-merge at enqueue: {out}"
    );
    assert_eq!(
        gh(&pay),
        Vec::<String>::new(),
        "the merge methods are the cache's"
    );
    assert!(mutations(&pay).is_empty(), "no mutation at enqueue");
    assert_eq!(row(&pay), None, "no pr_posts row until the send answers");
    assert_eq!(pay.fx.calls("node").len(), 0, "nothing is sent yet");
}

#[test]
fn uses_the_graphql_mutation_and_never_gh_pr_merge() {
    let pay = post();

    post_cmd(&pay).ok();

    let calls: Vec<Vec<String>> = pay
        .fx
        .calls("gh")
        .into_iter()
        .map(|call| call.args)
        .collect();
    assert!(
        calls
            .iter()
            .all(|a| a[..2] == ["api".to_owned(), "graphql".to_owned()]),
        "only `gh api graphql`: {calls:?}"
    );
    assert!(
        calls
            .iter()
            .all(|a| !(a.iter().any(|w| w == "pr") && a.iter().any(|w| w == "merge"))),
        "never `gh pr merge`: {calls:?}"
    );
}

#[test]
fn a_repo_that_does_not_allow_auto_merge_still_posts_and_says_not_allowed() {
    let pay = post();
    pay.gh.auto.lock().unwrap().allowed = false;

    let out = post_cmd(&pay).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert!(mutations(&pay).is_empty(), "nothing is requested");
    assert_eq!(row(&pay), None);
    assert_eq!(pay.fx.calls("node").len(), 0, "nothing is sent yet");
    assert_eq!(events(&pay, "error"), 0, "no error Event");
}

fn stack(pay: &Pay) {
    sql(
        pay,
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'base', '/nowhere', 'main', 'dispatch', 'cpd/pay/base', '/nowhere-base', cpd_txn(), 't', 't');
         UPDATE workflows SET base = 'cpd/pay/base' WHERE plan = 'pay' AND key = 'repo';",
    );
}

fn events(pay: &Pay, kind: &str) -> i64 {
    pay.store()
        .read(|c| {
            Ok(
                c.query_row("SELECT count(*) FROM events WHERE kind = ?1", [kind], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
}

/// The merge methods `acme/pay` allows, as the pass stored them in `swr_repos`, which `pr post` reads.
fn methods(pay: &Pay, squash: bool, merge: bool, rebase: bool) {
    sql(
        pay,
        &format!(
            "UPDATE swr_repos SET squash_merge_allowed = {}, merge_commit_allowed = {}, rebase_merge_allowed = {}
              WHERE repo = 'acme/pay';",
            i32::from(squash),
            i32::from(merge),
            i32::from(rebase)
        ),
    );
}

#[test]
fn a_repo_that_allows_only_rebase_enables_rebase_auto_merge() {
    let pay = post();
    methods(&pay, false, false, true);

    let out = post_cmd(&pay).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert!(mutations(&pay).is_empty(), "{:?}", gh(&pay));
}

#[test]
fn a_repo_that_allows_merge_commit_and_rebase_enables_merge_auto_merge() {
    let pay = post();
    methods(&pay, false, true, true);

    let out = post_cmd(&pay).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert!(mutations(&pay).is_empty(), "{:?}", gh(&pay));
}

#[test]
fn a_stacked_pr_is_queued_with_no_gh_call() {
    let pay = post();
    stack(&pay);

    let out = post_cmd(&pay).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(row(&pay), None);
    assert!(gh(&pay).is_empty(), "{:?}", gh(&pay));
    assert!(mutations(&pay).is_empty());
}

#[test]
fn a_stacked_pr_on_a_repo_without_auto_merge_says_not_allowed_and_stores_none() {
    let pay = post();
    stack(&pay);
    sql(
        &pay,
        "UPDATE swr_repos SET auto_merge_allowed = 0 WHERE repo = 'acme/pay';",
    );

    let out = post_cmd(&pay).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(row(&pay), None);
    assert!(gh(&pay).is_empty(), "{:?}", gh(&pay));
    assert!(mutations(&pay).is_empty());
}

#[test]
fn a_repo_that_allows_no_merge_method_refuses_before_the_send() {
    let pay = post();
    methods(&pay, false, false, false);

    let run = post_cmd(&pay);

    let error = refused(&run, "no_merge_method", 3);
    assert_eq!(
        error["details"],
        json!({"repo": "acme/pay", "number": 812, "squash": false, "merge": false, "rebase": false})
    );
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|m| m.contains("acme/pay")),
        "{error}"
    );
    assert_eq!(pay.fx.calls("node").len(), 0, "nothing is sent");
    assert_eq!(posts(&pay), 0);
    assert_eq!(row(&pay), None);
}

#[test]
fn a_github_refusal_of_the_mutation_is_stored_refused_and_reported() {
    let pay = post();
    pay.gh.auto.lock().unwrap().refuse = Some("Pull request is in clean status".to_owned());

    let out = post_cmd(&pay).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(row(&pay), None, "the refusal is stored when E18 runs");
    assert!(mutations(&pay).is_empty(), "never at enqueue");
}

#[test]
fn a_gh_failure_of_the_mutation_leaves_the_request_pending_and_the_post_stands() {
    let pay = post();
    pay.gh.auto.lock().unwrap().fail_mutation = true;

    let out = post_cmd(&pay).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert!(mutations(&pay).is_empty(), "never at enqueue");
    assert_eq!(pay.fx.calls("node").len(), 0, "the send is the outbox's");
    assert_eq!(posts(&pay), 0);
}

#[test]
fn a_post_makes_no_gh_call_and_writes_no_row() {
    let pay = post_with_node(Reply::exit(3, &format!("{AUTH}\n"), ""));

    let out = post_cmd(&pay).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert!(gh(&pay).is_empty(), "{:?}", gh(&pay));
    assert!(mutations(&pay).is_empty());
    assert_eq!(posts(&pay), 0);
    assert_eq!(
        row(&pay),
        None,
        "no pr_posts row, auto_merge column included"
    );
}

#[test]
fn auto_merge_adds_no_readiness_check_of_its_own() {
    let pay = post();
    let out = post_cmd(&pay).ok();
    assert_eq!(out["ok"], true);
    assert_eq!(out["queued"], true);
    drop(pay);

    let unready = post();
    sql(
        &unready,
        "UPDATE pull_requests SET checks = 'failed' WHERE number = 812;",
    );
    let error = refused(&post_cmd(&unready), "pr_not_ready", 3);
    assert_eq!(error["details"]["reasons"], json!(["checks_failed"]));
    assert!(gh(&unready).is_empty(), "{:?}", gh(&unready));
    assert_eq!(posts(&unready), 0);
}

#[test]
fn an_already_enabled_request_counts_as_enabled_without_a_second_mutation() {
    let pay = post();
    pay.gh.auto.lock().unwrap().already = true;

    let out = post_cmd(&pay).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert!(mutations(&pay).is_empty(), "{:?}", gh(&pay));
    assert_eq!(row(&pay), None);
}
