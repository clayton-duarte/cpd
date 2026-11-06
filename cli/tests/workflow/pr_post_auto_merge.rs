//! `pr post`'s auto-merge (commands.md §9.8b, swr-github.md §9): after the Slack send and the `pr_posts` insert the command checks
//! the cached repo settings and queues an `auto_merge` mutation; the drain makes the GraphQL read and the mutation
//! `enablePullRequestAutoMerge` (never `gh pr merge`) and settles `pr_posts.auto_merge`. Its outcome never undoes or fails the
//! post. `gh` and the Slack helper are the repo's fakes; every `gh` call is recorded.

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

/// The `auto_merge` mutations on the outbox: `(target_key, status)`.
fn queued(pay: &Pay) -> Vec<(String, String)> {
    pay.store()
        .read(|c| {
            let mut s = c.prepare("SELECT target_key, status FROM swr_mutations WHERE op_kind = 'auto_merge' ORDER BY id")?;
            let rows = s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap()
}

/// Runs the queued mutations through the source, as the engine's drain does: the outcomes.
fn drain(pay: &Pay) -> Vec<cpd_core::swr::source::ExecOutcome> {
    use cpd_core::config::Config;
    use cpd_core::home::CpdHome;
    use cpd_core::swr::outbox;
    use cpd_core::swr::source::github::GithubSource;
    use cpd_core::swr::source::{Cx, Source};
    let home = CpdHome::resolve(Some(pay.fx.env.cpd_home().into()), None).unwrap();
    let mut store = pay.store();
    let config = Config::load(&home).unwrap();
    let now = cpd_core::clock::Clock::utc(&*pay.fx.fakes.ports.clock);
    let mut out = Vec::new();
    while let Some(m) = outbox::claim_next(&mut store, now, "github").unwrap() {
        let mut cx = Cx {
            store: &mut store,
            ports: &pay.fx.fakes.ports,
            home: &home,
            config: &config,
        };
        out.push(GithubSource::default().execute(&mut cx, &m));
    }
    out
}

/// The cached settings of `acme/pay`: auto-merge allowed, and the methods the repo allows; the fake GitHub says the same.
fn repo(pay: &Pay, auto: bool, squash: bool, merge: bool, rebase: bool) {
    sql(
        pay,
        &format!(
            "UPDATE swr_repos SET auto_merge_allowed = {}, squash_merge_allowed = {}, merge_commit_allowed = {}, rebase_merge_allowed = {}
              WHERE repo = 'acme/pay';",
            i32::from(auto),
            i32::from(squash),
            i32::from(merge),
            i32::from(rebase)
        ),
    );
    let mut github = pay.gh.auto.lock().unwrap();
    (github.allowed, github.squash, github.merge, github.rebase) = (auto, squash, merge, rebase);
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

fn stack(pay: &Pay) {
    sql(
        pay,
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'base', '/nowhere', 'main', 'dispatch', 'cpd/pay/base', '/nowhere-base', cpd_txn(), 't', 't');
         UPDATE workflows SET base = 'cpd/pay/base' WHERE plan = 'pay' AND key = 'repo';",
    );
}

#[test]
fn queues_the_auto_merge_of_an_unstacked_pr_after_posting_and_the_drain_enables_it() {
    let pay = post();

    let out = post_cmd(&pay).ok();

    assert_eq!(out["auto_merge"]["state"], "queued", "{out}");
    assert!(
        gh(&pay).is_empty(),
        "the command calls no GitHub: {:?}",
        gh(&pay)
    );
    assert_eq!(
        queued(&pay),
        [("github:pr:acme/pay#812".to_owned(), "pending".to_owned())]
    );
    assert_eq!(row(&pay), Some(("pending".to_owned(), None)));
    assert_eq!(pay.fx.calls("node").len(), 1);

    assert_eq!(drain(&pay), [cpd_core::swr::source::ExecOutcome::Done]);
    let calls = gh(&pay);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(
        calls[0].contains("autoMergeAllowed") && calls[0].contains("autoMergeRequest"),
        "the read query first: {calls:?}"
    );
    assert!(
        calls[1].contains("enablePullRequestAutoMerge") && calls[1].contains("SQUASH"),
        "then the mutation: {calls:?}"
    );
    let (state, at) = row(&pay).expect("the post row");
    assert_eq!(state, "enabled");
    assert!(at.is_some_and(|at| !at.is_empty()), "auto_merge_at is set");
}

#[test]
fn the_drain_uses_the_graphql_mutation_and_never_gh_pr_merge() {
    let pay = post();
    post_cmd(&pay).ok();
    drain(&pay);

    let calls: Vec<Vec<String>> = pay
        .fx
        .calls("gh")
        .into_iter()
        .map(|call| call.args)
        .collect();
    assert!(
        calls
            .iter()
            .any(|a| a.join(" ").contains("enablePullRequestAutoMerge")),
        "{calls:?}"
    );
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
    repo(&pay, false, true, false, false);

    let out = post_cmd(&pay).ok();

    assert_eq!(out["auto_merge"]["state"], "not_allowed");
    let reason = out["auto_merge"]["reason"].as_str().unwrap_or_default();
    assert!(
        reason.contains("does not allow auto-merge") && reason.contains("cpd pr merge"),
        "{out}"
    );
    assert!(queued(&pay).is_empty(), "nothing is queued");
    assert_eq!(row(&pay), Some(("none".to_owned(), None)));
    assert_eq!(pay.fx.calls("node").len(), 1);
    assert_eq!(events(&pay, "error"), 0, "no error Event");
}

#[test]
fn a_repo_that_allows_only_rebase_or_merge_commit_enables_that_method() {
    for ((squash, merge, rebase), want) in [
        ((false, false, true), "REBASE"),
        ((false, true, true), "MERGE"),
    ] {
        let pay = post();
        repo(&pay, true, squash, merge, rebase);
        post_cmd(&pay).ok();
        drain(&pay);
        let sent = mutations(&pay);
        assert_eq!(sent.len(), 1, "{:?}", gh(&pay));
        assert!(
            sent[0].contains(want) && !sent[0].contains("SQUASH"),
            "{sent:?}"
        );
        assert_eq!(row(&pay).map(|r| r.0), Some("enabled".to_owned()));
    }
}

#[test]
fn a_stacked_pr_is_deferred_with_no_mutation() {
    let pay = post();
    stack(&pay);

    let out = post_cmd(&pay).ok();

    assert_eq!(out["auto_merge"]["state"], "deferred");
    assert_eq!(row(&pay), Some(("pending".to_owned(), None)));
    assert!(gh(&pay).is_empty());
    assert!(queued(&pay).is_empty());
}

#[test]
fn a_stacked_pr_on_a_repo_without_auto_merge_says_not_allowed_and_stores_none() {
    let pay = post();
    stack(&pay);
    repo(&pay, false, true, false, false);

    let out = post_cmd(&pay).ok();

    assert_eq!(out["auto_merge"]["state"], "not_allowed");
    assert_eq!(row(&pay), Some(("none".to_owned(), None)));
    assert!(gh(&pay).is_empty());
    assert!(queued(&pay).is_empty());
}

#[test]
fn a_repo_that_allows_no_merge_method_refuses_before_the_send() {
    let pay = post();
    repo(&pay, true, false, false, false);

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
fn repo_settings_the_poll_has_not_read_refuse_before_the_send_and_ask_for_them() {
    let pay = post();
    sql(&pay, "DELETE FROM swr_repos;");

    refused(&post_cmd(&pay), "item_not_found", 4);

    assert_eq!(pay.fx.calls("node").len(), 0, "nothing is sent");
    assert_eq!(posts(&pay), 0);
    assert_eq!(row(&pay), None);
    assert_eq!(
        pay.store()
            .read(|c| Ok(c.query_row(
                "SELECT invalidated FROM swr_queries WHERE key = 'github:repo:acme/pay'",
                [],
                |r| r.get::<_, i64>(0)
            )?))
            .unwrap(),
        1,
        "the next pass reads the settings"
    );
}

#[test]
fn a_github_refusal_in_the_drain_is_stored_refused_and_never_retried() {
    let pay = post();
    pay.gh.auto.lock().unwrap().refuse = Some("Pull request is in clean status".to_owned());
    post_cmd(&pay).ok();

    let outcomes = drain(&pay);

    assert!(
        matches!(&outcomes[..], [cpd_core::swr::source::ExecOutcome::Failed { error }] if error.contains("Pull request is in clean status")),
        "{outcomes:?}"
    );
    let (state, at) = row(&pay).expect("the post row");
    assert_eq!(state, "refused");
    assert!(at.is_some(), "auto_merge_at is set");
    assert_eq!(mutations(&pay).len(), 1, "never retried");
}

#[test]
fn a_gh_failure_of_the_mutation_is_retried_and_the_post_stands() {
    let pay = post();
    pay.gh.auto.lock().unwrap().fail_mutation = true;
    post_cmd(&pay).ok();

    let outcomes = drain(&pay);

    assert!(
        matches!(
            &outcomes[..],
            [cpd_core::swr::source::ExecOutcome::Retry { .. }]
        ),
        "{outcomes:?}"
    );
    assert_eq!(
        row(&pay).map(|r| r.0),
        Some("pending".to_owned()),
        "a failed call settles nothing"
    );
    assert_eq!(pay.fx.calls("node").len(), 1, "the Slack message was sent");
    assert_eq!(posts(&pay), 1);
}

#[test]
fn an_already_enabled_request_counts_as_enabled_without_a_second_mutation() {
    let pay = post();
    pay.gh.auto.lock().unwrap().already = true;
    post_cmd(&pay).ok();

    drain(&pay);

    assert!(mutations(&pay).is_empty(), "{:?}", gh(&pay));
    assert_eq!(row(&pay).map(|r| r.0), Some("enabled".to_owned()));
}

#[test]
fn a_failed_slack_send_queues_nothing_and_writes_no_row() {
    let pay = post_with_node(Reply::exit(3, &format!("{AUTH}\n"), ""));

    refused(&post_cmd(&pay), "slack_auth", 3);

    assert!(gh(&pay).is_empty(), "{:?}", gh(&pay));
    assert!(queued(&pay).is_empty());
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
    assert_eq!(out["auto_merge"]["state"], "queued");
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
