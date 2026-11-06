//! GitHub writes through the outbox, the optimistic overlay, the cache-only readers and the SQLite deploys
//! (docs/architecture/swr-github.md §6, §8, §9, §10). In process on the janitor's `Rig`: `gh` is a recording route of the
//! `FakeRunner`, time is the rig's `ManualClock`; no process, no network, no sleep. The CLI commands are called as library
//! functions on `Ports::scope`.

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::github::GithubSource;
use super::{Cx, ExecOutcome, Source};
use crate::clock::{Clock, timestamp};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::host::runner::{Call, Handler, Reply};
use crate::inbox::model::Kind;
use crate::janitor_tests::Rig;
use crate::output::Output;
use crate::pr::comments::{Item, Kind as ItemKind};
use crate::store::Store;
use crate::swr::github::effective_pr;
use crate::swr::outbox::{self, Coalesce, Op, Retry};
use crate::swr::{cache, overlay};

const KEY: &str = "github:pr:acme/pay#801";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

/// `gh` answers every call with an empty object: a test that must not call it asserts the record, not the answer.
struct Gh;

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        match call.args.first().map(String::as_str) {
            Some("pr") if call.args.get(1).map(String::as_str) == Some("merge") => Reply::ok(""),
            _ => Reply::ok("{}"),
        }
    }
}

/// `gh pr merge` is refused by GitHub.
struct Refusing;

impl Handler for Refusing {
    fn handle(&self, call: &Call) -> Reply {
        if call.args.iter().any(|a| a == "merge") {
            Reply::exit(1, "", "GraphQL: Pull request is not mergeable\n")
        } else {
            Reply::ok("{}")
        }
    }
}

/// A `pr` command, as clap parses it: the arguments of most commands are private to their module.
#[derive(Parser)]
struct PrCli {
    #[command(subcommand)]
    command: crate::pr::Command,
}

#[derive(Parser)]
struct WorkflowCli {
    #[command(subcommand)]
    command: crate::workflow::WorkflowCommand,
}

#[derive(Parser)]
struct ReviewCli {
    #[command(subcommand)]
    command: crate::pr::review::Command,
}

/// The rig: the open authored Workflow `api` (pull request `acme/pay#801`, clean, checks passed, head `HEAD`), the repo's
/// metadata cached (squash only), `gh` the recording route, `$CPD_HOME` the rig's.
fn rig() -> Rig {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    let fetched = timestamp(rig.fakes.clock.utc());
    rig.sql(&format!(
        "UPDATE pull_requests SET mergeable = 'mergeable', review_decision = 'none', checks = 'passed', authored = 1,
                feedback = '[]', merge_state = 'clean', base_ref = 'main', default_branch = 'main', head_oid = '{HEAD}'
          WHERE number = 801;
         INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed,
                                merge_commit_allowed, rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/pay', 'main', 0, 1, 1, 0, 0, 1, '{fetched}');"
    ));
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(Gh));
    let policy = rig.home.root().join("policy/acme");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(policy.join("owner.toml"), "").unwrap();
    std::fs::write(policy.join("pay.toml"), "").unwrap();
    // `Env::open_with` reads `$CPD_HOME`: the one variable these tests set, to their own temp dir.
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    rig
}

fn now(rig: &Rig) -> OffsetDateTime {
    rig.fakes.clock.utc()
}

fn store(rig: &Rig) -> Store {
    Store::open(&rig.home, 5000).unwrap()
}

fn gh_calls(rig: &Rig) -> Vec<String> {
    rig.fakes
        .runner
        .calls()
        .iter()
        .filter(|c| c.tool == "gh")
        .map(|c| c.args.join(" "))
        .collect()
}

fn field(out: &Output, name: &str) -> Value {
    match out {
        Output::Success(map) => map.get(name).cloned().unwrap_or(Value::Null),
        Output::Owned => Value::Null,
    }
}

fn pr_command(rig: &Rig, argv: &[&str]) -> Result<Output, CpdError> {
    let mut full = vec!["cpd"];
    full.extend_from_slice(argv);
    let command = PrCli::try_parse_from(full).unwrap().command;
    crate::ports::Ports::scope(&rig.fakes.ports, || {
        crate::pr::run(&rig.fakes.ports, command)
    })
}

fn workflow_command(rig: &Rig, argv: &[&str]) -> Result<Output, CpdError> {
    let mut full = vec!["cpd"];
    full.extend_from_slice(argv);
    let command = WorkflowCli::try_parse_from(full).unwrap().command;
    crate::ports::Ports::scope(&rig.fakes.ports, || crate::workflow::run_workflow(command))
}

/// `(op_kind, target_key, status, vars, patch)` of every mutation, oldest first.
type Row = (String, String, String, Value, Option<Value>);

fn mutations(rig: &Rig) -> Vec<Row> {
    store(rig)
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT op_kind, target_key, status, vars_json, patch_json FROM swr_mutations ORDER BY id",
            )?;
            Ok(stmt
                .query_map([], |r| {
                    let vars: String = r.get(3)?;
                    let patch: Option<String> = r.get(4)?;
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        serde_json::from_str(&vars).unwrap(),
                        patch.map(|p| serde_json::from_str(&p).unwrap()),
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap()
}

fn enqueue(rig: &Rig, kind: &str, target: &str, patch: Option<Value>, vars: Value) -> i64 {
    outbox::enqueue(
        &mut store(rig),
        now(rig),
        Op {
            system: "github".into(),
            target_key: target.into(),
            op_kind: kind.into(),
            vars,
            patch,
            context: None,
        },
        Coalesce::LatestWins,
    )
    .unwrap()
}

fn enqueue_merge(rig: &Rig) -> i64 {
    enqueue(
        rig,
        "merge",
        KEY,
        Some(json!({"state": "merging"})),
        json!({"expected_head_oid": HEAD, "method": "squash"}),
    )
}

/// The `pr-ready` conditions E8 would open now.
fn pr_ready(rig: &Rig) -> usize {
    let (home, at) = (rig.home.clone(), now(rig));
    store(rig)
        .read(|c| crate::inbox::pr::conditions(c, &home, at))
        .unwrap()
        .iter()
        .filter(|c| c.kind == Kind::PrReady && c.subject.contains("acme/pay#801"))
        .count()
}

fn status_of(rig: &Rig, id: i64) -> String {
    outbox::get(&mut store(rig), id).unwrap().unwrap().status
}

fn query<T: rusqlite::types::FromSql>(rig: &Rig, sql: &str) -> T {
    store(rig)
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn gh_merge_enqueues_a_mutation_and_prints_queued() {
    let rig = rig();

    let out = pr_command(&rig, &["merge", "acme/pay#801"]).expect("the merge is queued");

    assert_eq!(field(&out, "queued"), json!(true), "{out:?}");
    assert!(field(&out, "mutation_id").is_i64(), "{out:?}");
    let rows = mutations(&rig);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let (kind, target, status, vars, patch) = &rows[0];
    assert_eq!(
        (kind.as_str(), target.as_str(), status.as_str()),
        ("merge", KEY, "pending")
    );
    assert_eq!(vars["expected_head_oid"], HEAD);
    assert_eq!(
        vars["method"], "squash",
        "the first method swr_repos allows"
    );
    assert_eq!(patch.as_ref().unwrap()["state"], "merging");
    assert_eq!(
        gh_calls(&rig),
        Vec::<String>::new(),
        "a command never calls GitHub for a write"
    );
}

#[test]
fn gh_merge_overlay_hides_the_pr_ready_inbox_item_at_once() {
    let rig = rig();
    assert_eq!(
        pr_ready(&rig),
        1,
        "the clean authored pull request is a pr-ready item"
    );

    enqueue_merge(&rig);

    assert_eq!(
        pr_ready(&rig),
        0,
        "the queued merge closes the item before GitHub has it"
    );
}

#[test]
fn gh_merge_overlay_stays_pinned_until_a_post_completion_poll() {
    let rig = rig();
    let id = enqueue_merge(&rig);
    let mut store = store(&rig);
    let claimed = outbox::claim_next(&mut store, now(&rig), "github")
        .unwrap()
        .unwrap();
    assert_eq!(claimed.id, id);
    rig.fakes.clock.advance(Duration::from_secs(10));
    let completed = now(&rig);
    outbox::complete(&mut store, id, completed).unwrap();
    let state = |store: &mut Store| effective_pr(store, "acme/pay", 801).unwrap().unwrap().state;
    assert_eq!(
        state(&mut store),
        "merging",
        "revalidating contributes its patch"
    );

    // A poll that started before the write completed read GitHub before the merge: its data does not prove the merge.
    cache::commit(
        &mut store,
        KEY,
        "before",
        now(&rig),
        completed - Duration::from_secs(1),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(status_of(&rig, id), "revalidating");
    assert_eq!(state(&mut store), "merging", "the fence keeps the overlay");

    // The next poll started after `completed_at`: the raw row is the truth now.
    rig.fakes.clock.advance(Duration::from_secs(10));
    cache::commit(
        &mut store,
        KEY,
        "after",
        now(&rig),
        completed + Duration::from_secs(1),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(status_of(&rig, id), "done");
    assert_eq!(
        state(&mut store),
        "open",
        "the overlay is gone, the raw row shows"
    );
}

#[test]
fn gh_merge_failure_drops_the_overlay_and_restores_the_item() {
    let rig = rig();
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(Refusing));
    let id = enqueue_merge(&rig);
    let mut store = store(&rig);
    let m = outbox::claim_next(&mut store, now(&rig), "github")
        .unwrap()
        .unwrap();
    assert_eq!(pr_ready(&rig), 0, "hidden while the write is in flight");

    let outcome = {
        let config = Config::load(&rig.home).unwrap();
        let mut cx = Cx {
            store: &mut store,
            ports: &rig.fakes.ports,
            home: &rig.home,
            config: &config,
        };
        GithubSource::default().execute(&mut cx, &m)
    };

    let ExecOutcome::Failed { error } = outcome else {
        panic!("GitHub refused the merge: {outcome:?}")
    };
    assert!(
        gh_calls(&rig)
            .iter()
            .any(|c| c.contains("pr merge") && c.contains(&format!("--match-head-commit {HEAD}"))),
        "the drain merged with the expected head: {:?}",
        gh_calls(&rig)
    );
    outbox::fail(&mut store, id, now(&rig), &error, Retry::Never).unwrap();
    assert_eq!(status_of(&rig, id), "failed");
    assert_eq!(
        pr_ready(&rig),
        1,
        "a true revert: the item returns to its raw state"
    );
}

#[test]
fn gh_unstack_waits_for_merged_in_the_raw_cache() {
    let rig = rig();
    rig.sql(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           SELECT plan, 'web', repo, 'cpd/pay/api', 'dispatch', 'cpd/pay/web', '/tmp/web-lead', 'Web', 'config', cpd_txn(), 't', 't'
             FROM workflows WHERE key = 'api';",
    );

    let out = pr_command(&rig, &["merge", "acme/pay#801"]).expect("the merge is queued");
    assert_eq!(field(&out, "queued"), json!(true), "{out:?}");

    assert_eq!(
        query::<String>(&rig, "SELECT state FROM pull_requests WHERE number = 801"),
        "open",
        "the raw row is not merged until a poll started after the merge says so"
    );
    assert_eq!(
        query::<String>(&rig, "SELECT status FROM workflows WHERE key = 'api'"),
        "open"
    );
    assert_eq!(
        query::<String>(&rig, "SELECT base FROM workflows WHERE key = 'web'"),
        "cpd/pay/api",
        "the dependent branch is not unstacked on the overlay `merging`"
    );
    assert_eq!(
        query::<i64>(&rig, "SELECT count(*) FROM events WHERE kind = 'pr-merged'"),
        0
    );
}

#[test]
fn gh_approve_posts_and_overlays_review_decision() {
    let rig = rig();
    let input = rig.home.root().join("review.json");
    std::fs::write(&input, r#"{"body":"LGTM"}"#).unwrap();
    let command = ReviewCli::try_parse_from([
        "cpd",
        "submit",
        "--pr",
        "acme/pay#801",
        "--event",
        "APPROVE",
        "--head",
        HEAD,
        "--input",
        input.to_str().unwrap(),
    ])
    .unwrap()
    .command;

    let out = crate::ports::Ports::scope(&rig.fakes.ports, || {
        crate::pr::review::run(&rig.fakes.ports, command)
    })
    .expect("the review is queued");

    assert_eq!(field(&out, "queued"), json!(true), "{out:?}");
    let rows = mutations(&rig);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!((rows[0].0.as_str(), rows[0].1.as_str()), ("approve", KEY));
    assert_eq!(rows[0].3["event"], "APPROVE");
    assert_eq!(rows[0].4.as_ref().unwrap()["review_decision"], "approved");
    assert_eq!(gh_calls(&rig), Vec::<String>::new());
    let effective = effective_pr(&mut store(&rig), "acme/pay", 801)
        .unwrap()
        .unwrap();
    assert_eq!(effective.review_decision, "approved");
    assert_eq!(
        effective.row.review_decision, "none",
        "the raw row is unchanged"
    );
}

#[test]
fn gh_reply_describe_retitle_rerun_go_through_the_outbox() {
    let rig = rig();
    let item = Item {
        id: "T1".into(),
        kind: ItemKind::Thread,
        last: "c1".into(),
        author: "ryan".into(),
        bot: false,
        path: None,
        outdated: false,
        url: "https://github.com/acme/pay/pull/801#discussion_r1".into(),
        excerpt: "why?".into(),
    };
    let detail = json!({"commits": {"nodes": [{"commit": {"oid": HEAD, "statusCheckRollup": {"state": "FAILURE",
        "contexts": {"nodes": [{"__typename": "CheckRun", "name": "build", "conclusion": "FAILURE", "status": "COMPLETED",
          "detailsUrl": null, "checkSuite": {"workflowRun": {"databaseId": 9, "workflow": {"name": "ci"}}}}]}}}}]},
        "latestReviews": {"nodes": []}, "reviewThreads": {"nodes": []}, "comments": {"nodes": []}});
    rig.sql(&format!(
        "UPDATE pull_requests SET feedback = '{}', checks = 'failed' WHERE number = 801;
         INSERT INTO swr_pr_detail (repo, number, head_oid, updated_at, hydrated_at, detail_json, detail_hash)
           VALUES ('acme/pay', 801, '{HEAD}', 'U1', 't', '{detail}', 'h');",
        serde_json::to_string(&vec![item]).unwrap()
    ));
    let body = rig.home.root().join("body.md");
    std::fs::write(&body, "A new description\n").unwrap();

    let reply = pr_command(
        &rig,
        &[
            "reply",
            "--plan",
            "pay",
            "--workflow",
            "api",
            "--to",
            "T1",
            "--body",
            "Done",
        ],
    );
    let rerun = pr_command(&rig, &["rerun", "--plan", "pay", "--workflow", "api"]);
    let describe = workflow_command(
        &rig,
        &[
            "describe",
            "api",
            "--plan",
            "pay",
            "--file",
            body.to_str().unwrap(),
        ],
    );
    let retitle = workflow_command(
        &rig,
        &["edit", "api", "--plan", "pay", "--title", "Charge faster"],
    );

    for (name, out) in [
        ("reply", &reply),
        ("rerun", &rerun),
        ("describe", &describe),
        ("retitle", &retitle),
    ] {
        let out = out.as_ref().unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(field(out, "queued"), json!(true), "{name}: {out:?}");
        assert!(field(out, "mutation_id").is_i64(), "{name}: {out:?}");
    }
    let kinds: Vec<(String, String)> = mutations(&rig).into_iter().map(|r| (r.0, r.1)).collect();
    assert_eq!(
        kinds,
        ["reply", "rerun", "describe", "retitle"].map(|k| (k.to_owned(), KEY.to_owned()))
    );
    let rows = mutations(&rig);
    assert_eq!(rows[0].3, json!({"to": "T1", "body": "Done"}));
    assert_eq!(rows[1].3, json!({"run_ids": [9]}));
    assert_eq!(rows[1].4.as_ref().unwrap()["checks"], "pending");
    assert_eq!(rows[2].3["body"], "A new description\n");
    assert_eq!(rows[3].3["title"], "Charge faster");
    assert_eq!(rows[3].4.as_ref().unwrap()["title"], "Charge faster");
    assert_eq!(
        gh_calls(&rig),
        Vec::<String>::new(),
        "no command writes to GitHub itself"
    );
}

#[test]
fn gh_newer_describe_supersedes_an_unstarted_one() {
    let rig = rig();
    let source = GithubSource::default();
    assert_eq!(source.coalesce("describe"), Coalesce::LatestWins);
    assert!(
        matches!(source.coalesce("merge"), Coalesce::Dedupe(_)),
        "a merge dedupes on its vars"
    );
    let file = rig.home.root().join("body.md");
    for text in ["first\n", "second\n"] {
        std::fs::write(&file, text).unwrap();
        workflow_command(
            &rig,
            &[
                "describe",
                "api",
                "--plan",
                "pay",
                "--file",
                file.to_str().unwrap(),
            ],
        )
        .expect("the description is queued");
    }

    let rows = mutations(&rig);
    let describes: Vec<(&str, &Value)> = rows
        .iter()
        .filter(|r| r.0 == "describe")
        .map(|r| (r.2.as_str(), &r.3["body"]))
        .collect();
    assert_eq!(
        describes,
        [
            ("superseded", &json!("first\n")),
            ("pending", &json!("second\n"))
        ]
    );
    let active = overlay::active(&mut store(&rig), "github", KEY).unwrap();
    assert_eq!(active.len(), 1, "one description is active: {active:?}");
}

#[test]
fn gh_pr_view_reads_the_cache_and_makes_no_gh_call() {
    let rig = rig();
    let detail = json!({"commits": {"nodes": [{"commit": {"oid": HEAD, "statusCheckRollup": {"state": "SUCCESS",
        "contexts": {"nodes": [{"__typename": "CheckRun", "name": "build", "conclusion": "SUCCESS", "status": "COMPLETED",
          "detailsUrl": null, "checkSuite": {"workflowRun": {"databaseId": 9, "workflow": {"name": "ci"}}}}]}}}}]},
        "latestReviews": {"nodes": []}, "reviewThreads": {"nodes": []}, "comments": {"nodes": []}});
    rig.sql(&format!(
        "INSERT INTO swr_pr_detail (repo, number, head_oid, updated_at, hydrated_at, detail_json, detail_hash)
           VALUES ('acme/pay', 801, '{HEAD}', 'U1', 't', '{detail}', 'h');"
    ));

    let out = pr_command(&rig, &["view", "--plan", "pay", "--workflow", "api"])
        .expect("pr view reads the cache");

    assert_eq!(
        gh_calls(&rig),
        Vec::<String>::new(),
        "a reader never calls GitHub"
    );
    let view = field(&out, "pr");
    assert_eq!(view["state"], "open", "{out:?}");
    assert_eq!(view["checks"], "passed");
    assert_eq!(field(&out, "checks")[0]["name"], "build");
}

#[test]
fn gh_inbox_claim_reads_the_cache_and_makes_no_gh_call() {
    let rig = rig();
    let at = timestamp(now(&rig));
    rig.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                    authored, review_requested, head_oid)
           VALUES ('acme/pay', 812, 'https://github.com/acme/pay/pull/812', 'Add retries', 0, 'open', 'none', 'passed', '{at}', '{at}', 0, 1, 'abc');
         INSERT INTO swr_queries (key, system, status, data_hash) VALUES ('slack:channel:C02JU6C2Z1D', 'slack', 'success', 'h');
         INSERT INTO swr_slack_posts (channel, ts, repo, number, primary_pr, author_is_me, reactions_json, eyes_me, thread_tags_me, fetched_at)
           VALUES ('C02JU6C2Z1D', '1767225600.000100', 'acme/pay', 812, 1, 0, '{{}}', 0, 0, '{at}');
         INSERT INTO inbox_items (kind, urgency, subject, label, opened_at, head_oid)
           VALUES ('review-requested', 'blue', 'pr:acme/pay#812', 'review pay#812', '{at}', 'abc');"
    ));
    let id: i64 = query(
        &rig,
        "SELECT id FROM inbox_items WHERE subject = 'pr:acme/pay#812'",
    );
    let env = crate::env::Env::open_with(&rig.fakes.ports).unwrap();

    let result =
        crate::inbox::cmd::claim_in(&rig.fakes.ports, env, crate::inbox::cmd::DismissArgs { id });

    assert_eq!(
        gh_calls(&rig),
        Vec::<String>::new(),
        "the live check reads the effective row, not GitHub"
    );
    if let Err(e) = result {
        assert_ne!(
            e.code(),
            ErrorCode::PostNotFound,
            "the post and the pull request are in SQLite, not in run/team-review.json: {e:?}"
        );
    }
}

#[test]
fn gh_merge_waits_for_repo_metadata_instead_of_calling_gh() {
    let rig = rig();
    rig.sql("DELETE FROM swr_repos;");
    let id = enqueue_merge(&rig);
    let mut store = store(&rig);
    let m = outbox::claim_next(&mut store, now(&rig), "github")
        .unwrap()
        .unwrap();
    assert_eq!(m.id, id);

    let outcome = {
        let config = Config::load(&rig.home).unwrap();
        let mut cx = Cx {
            store: &mut store,
            ports: &rig.fakes.ports,
            home: &rig.home,
            config: &config,
        };
        GithubSource::default().execute(&mut cx, &m)
    };

    assert!(
        matches!(outcome, ExecOutcome::Retry { .. }),
        "no metadata yet: wait for the pass, {outcome:?}"
    );
    assert_eq!(
        gh_calls(&rig),
        Vec::<String>::new(),
        "no gh call for the lookup"
    );
    let repo = cache::get(&mut store, "github:repo:acme/pay")
        .unwrap()
        .expect("the repo key is invalidated");
    assert!(repo.invalidated);
}

/// A policy declaring the deploy workflow `deploy.yml` of `acme/pay`, the viewer `ada`.
fn deploy_rig() -> Rig {
    let rig = rig();
    let path = rig.home.root().join("policy/acme/pay.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "[deploys]\nworkflows = [\"deploy.yml\"]\n").unwrap();
    let at = timestamp(now(&rig));
    rig.sql(&format!(
        "INSERT INTO swr_viewer (id, login, fetched_at) VALUES (1, 'ada', '{at}');"
    ));
    rig
}

fn waiting(id: u64, updated: &str) -> Value {
    json!({"id": id, "status": "waiting", "display_title": format!("Release {id}"), "updated_at": updated,
           "html_url": format!("https://github.com/acme/pay/actions/runs/{id}"), "created_at": "2026-10-27T15:00:00.000Z",
           "triggering_actor": {"login": "Ada"}})
}

/// The deploy REST calls: the waiting list of `deploy.yml` and the `pending_deployments` of each run.
struct Deploys(Vec<Value>);

impl Handler for Deploys {
    fn handle(&self, call: &Call) -> Reply {
        let path = call.args.get(1).cloned().unwrap_or_default();
        if path.ends_with("deploy.yml/runs?status=waiting&per_page=100") {
            Reply::ok(&json!({"total_count": self.0.len(), "workflow_runs": self.0}).to_string())
        } else if path.ends_with("/pending_deployments") {
            Reply::ok(
                &json!([{"environment": {"id": 1, "name": "prod"}, "wait_timer": 0, "current_user_can_approve": true, "reviewers": []}])
                    .to_string(),
            )
        } else {
            Reply::exit(9, "", &format!("unexpected gh call: {path}\n"))
        }
    }
}

fn stored_deploy(rig: &Rig, id: u64, updated: &str, pull: Option<i64>) {
    let prs: Vec<Value> = pull.map(|n| json!({"number": n})).into_iter().collect();
    let mut run = waiting(id, updated);
    run["pull_requests"] = json!(prs);
    rig.sql(&format!(
        "INSERT INTO swr_deploys (repo, run_id, updated_at, run_json, pending_json, inactive_at)
           VALUES ('acme/pay', {id}, '{updated}', '{run}',
                   '[{{\"environment\":{{\"id\":1,\"name\":\"prod\"}},\"current_user_can_approve\":true}}]', NULL);"
    ));
}

fn read_deploys(rig: &Rig) -> Result<(), CpdError> {
    crate::pr::deploys::read_into_store(&*rig.fakes.runner, &mut store(rig), &rig.home, now(rig))
}

#[test]
fn gh_deploys_read_pending_deployments_only_for_changed_runs() {
    let rig = deploy_rig();
    stored_deploy(&rig, 1, "U1", None);
    let _ = (*rig.fakes.runner).clone().route(
        "gh",
        Arc::new(Deploys(vec![waiting(1, "U1"), waiting(2, "U2")])),
    );

    read_deploys(&rig).expect("the deploy read");

    let calls = gh_calls(&rig);
    let pending: Vec<&String> = calls
        .iter()
        .filter(|c| c.ends_with("pending_deployments"))
        .collect();
    assert_eq!(pending.len(), 1, "{calls:?}");
    assert!(
        pending[0].contains("/runs/2/"),
        "only the changed run: {calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c.contains("api user")),
        "the viewer is swr_viewer's: {calls:?}"
    );
    assert_eq!(
        query::<i64>(
            &rig,
            "SELECT count(*) FROM swr_deploys WHERE inactive_at IS NULL"
        ),
        2
    );
}

/// The `deploy` conditions E8 would open now.
fn deploy_items(rig: &Rig) -> Vec<String> {
    let (home, at) = (rig.home.clone(), now(rig));
    crate::inbox::rules::conditions_with(&mut store(rig), &home, at, 20, 1, None)
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == Kind::Deploy)
        .map(|c| c.subject)
        .collect()
}

#[test]
fn gh_deploy_approve_overlay_hides_until_revalidated() {
    let rig = deploy_rig();
    stored_deploy(&rig, 77, "U1", None);
    let target = "github:deploy:acme/pay#77";
    assert_eq!(
        deploy_items(&rig),
        ["deploy:acme/pay#77"],
        "the waiting run is a deploy item, from swr_deploys"
    );

    let id = enqueue(
        &rig,
        "approve",
        target,
        Some(json!({"deploy": "approving"})),
        json!({"run_id": 77, "environments": ["prod"]}),
    );
    assert!(deploy_items(&rig).is_empty(), "the item closes at once");
    let mut store = store(&rig);
    outbox::claim_next(&mut store, now(&rig), "github")
        .unwrap()
        .unwrap();
    rig.fakes.clock.advance(Duration::from_secs(5));
    let completed = now(&rig);
    outbox::complete(&mut store, id, completed).unwrap();
    assert!(
        deploy_items(&rig).is_empty(),
        "revalidating keeps it hidden"
    );

    // A poll that started after the approval has read GitHub: the overlay ends.
    cache::settle(
        &mut store,
        target,
        now(&rig),
        completed + Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(status_of(&rig, id), "done");
    assert_eq!(
        deploy_items(&rig),
        ["deploy:acme/pay#77"],
        "the raw row (still waiting) shows again"
    );
}

#[test]
fn gh_deploy_rows_go_inactive_when_the_pr_closes() {
    let rig = deploy_rig();
    stored_deploy(&rig, 5, "U1", Some(801));
    stored_deploy(&rig, 6, "U1", None);
    rig.sql("UPDATE pull_requests SET state = 'closed' WHERE number = 801;");
    let _ = (*rig.fakes.runner).clone().route(
        "gh",
        Arc::new(Deploys(vec![waiting(5, "U1"), waiting(6, "U1")])),
    );

    read_deploys(&rig).expect("the deploy read");

    let at = timestamp(now(&rig));
    assert_eq!(
        query::<Option<String>>(&rig, "SELECT inactive_at FROM swr_deploys WHERE run_id = 5"),
        Some(at),
        "the run of a closed pull request is inactive from now"
    );
    assert_eq!(
        query::<Option<String>>(&rig, "SELECT inactive_at FROM swr_deploys WHERE run_id = 6"),
        None
    );
}
