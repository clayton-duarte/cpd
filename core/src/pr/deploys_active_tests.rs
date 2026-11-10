//! pro-d468: the deploy read finds the approvable run (decoupled from the PR poll, with its own viewer, over the active runs
//! of a declared workflow). `gh` is a scripted route of the `FakeRunner`; no process, no network, no sleep.

use std::sync::Arc;

use clap::Parser;
use serde_json::{Value, json};

use crate::clock::Clock;
use crate::config::Config;
use crate::error::CpdError;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::store::Store;
use crate::swr::source::Cx;
use crate::swr::source::github::GithubSource;

/// A run of `deploy.yml` with `status` as the run list reports it, the User's (`Ada`).
fn run(id: u64, status: &str) -> Value {
    json!({"id": id, "status": status, "display_title": format!("Release {id}"), "updated_at": "U1",
           "html_url": format!("https://github.com/acme/pay/actions/runs/{id}"), "created_at": "2026-10-27T15:00:00.000Z",
           "triggering_actor": {"login": "Ada"}})
}

/// The deploy REST of `acme/pay`: `runs` are listed under each of `statuses` (a read without a `status` filter lists them all),
/// every run has one approvable environment. `gh api user` is `ada`. The GraphQL calls fail when `graphql_fails`, else answer
/// an empty listing.
struct Gh {
    runs: Vec<Value>,
    statuses: Vec<&'static str>,
    graphql_fails: bool,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "user", "--jq", ".login"] => Reply::ok("ada\n"),
            ["api", "graphql", ..] if self.graphql_fails => {
                Reply::exit(1, "", "gh: HTTP 502: Bad Gateway\n")
            }
            ["api", "graphql", ..] => Reply::ok(
                &json!({"data": {"viewer": {"login": "ada"},
                    "rateLimit": {"remaining": 4000, "resetAt": "2030-01-01T00:00:00Z", "cost": 1},
                    "authored": {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": []},
                    "reviews": {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": []}}})
                .to_string(),
            ),
            ["api", path] if path.contains("/actions/workflows/deploy.yml/runs") => {
                let status = path
                    .split_once('?')
                    .and_then(|(_, q)| q.split('&').find_map(|kv| kv.strip_prefix("status=")));
                let listed: Vec<&Value> = match status {
                    Some(s) if !self.statuses.contains(&s) => Vec::new(),
                    _ => self.runs.iter().collect(),
                };
                Reply::ok(&json!({"total_count": listed.len(), "workflow_runs": listed}).to_string())
            }
            ["api", path] if path.ends_with("/pending_deployments") => Reply::ok(
                &json!([{"environment": {"id": 1, "name": "prod"}, "wait_timer": 0, "current_user_can_approve": true, "reviewers": []}])
                    .to_string(),
            ),
            _ => Reply::exit(1, "", &format!("gh: unexpected call: {}\n", args.join(" "))),
        }
    }
}

/// A rig whose repo policy declares `deploy.yml` and whose `gh` is `gh`; `$CPD_HOME` is the rig's (the one variable set, to its
/// own temp dir, as the other `pr` tests do).
fn rig(gh: Gh) -> Rig {
    let rig = Rig::new();
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(gh));
    let policy = rig.home.root().join("policy/acme");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(policy.join("owner.toml"), "").unwrap();
    std::fs::write(
        policy.join("pay.toml"),
        "[deploys]\nworkflows = [\"deploy.yml\"]\n",
    )
    .unwrap();
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    rig
}

fn store(rig: &Rig) -> Store {
    Store::open(&rig.home, 5000).unwrap()
}

/// The ids of the active `swr_deploys` rows.
fn active(rig: &Rig) -> Vec<i64> {
    store(rig)
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT run_id FROM swr_deploys WHERE inactive_at IS NULL ORDER BY run_id",
            )?;
            Ok(stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap()
}

#[derive(Parser)]
struct PrCli {
    #[command(subcommand)]
    command: crate::pr::Command,
}

#[test]
fn pr_graphql_error_still_polls_deploys() {
    let rig = rig(Gh {
        runs: vec![run(42, "waiting")],
        statuses: vec!["waiting", "in_progress", "queued"],
        graphql_fails: true,
    });
    rig.sql("INSERT INTO swr_viewer (id, login, fetched_at) VALUES (1, 'ada', 't');");
    let command = PrCli::try_parse_from(["cpd", "poll"]).unwrap().command;

    // The PR read fails; whether the command reports it is not this test's contract.
    let _ = crate::ports::Ports::scope(&rig.fakes.ports, || {
        crate::pr::run(&rig.fakes.ports, command)
    });

    assert_eq!(
        active(&rig),
        [42],
        "a failed PR GraphQL read must not skip the deploy read"
    );
}

#[test]
fn empty_swr_viewer_fetches_and_persists_login() {
    let rig = rig(Gh {
        runs: vec![run(42, "waiting")],
        statuses: vec!["waiting", "in_progress", "queued"],
        graphql_fails: false,
    });
    let mut s = store(&rig);

    let read = crate::pr::deploys::read_into_store(
        &*rig.fakes.runner,
        &mut s,
        &rig.home,
        rig.fakes.clock.utc(),
    );

    read.expect("the deploy read");
    let login: Option<String> = s
        .read(|c| {
            Ok(rusqlite::OptionalExtension::optional(c.query_row(
                "SELECT login FROM swr_viewer WHERE id = 1",
                [],
                |r| r.get(0),
            ))?)
        })
        .unwrap();
    assert_eq!(
        login.as_deref(),
        Some("ada"),
        "the viewer is fetched with `gh api user` and kept"
    );
    assert_eq!(active(&rig), [42]);
}

#[test]
fn in_progress_run_with_pending_deployment_is_listed() {
    // Parallel jobs keep the run `in_progress` while its production deployment already waits for approval.
    let rig = rig(Gh {
        runs: vec![run(42650847363, "in_progress")],
        statuses: vec!["in_progress", "queued"],
        graphql_fails: false,
    });
    rig.sql("INSERT INTO swr_viewer (id, login, fetched_at) VALUES (1, 'ada', 't');");
    let mut s = store(&rig);

    crate::pr::deploys::read_into_store(
        &*rig.fakes.runner,
        &mut s,
        &rig.home,
        rig.fakes.clock.utc(),
    )
    .expect("the deploy read");

    assert_eq!(active(&rig), [42650847363]);
}

#[test]
fn github_source_commit_reads_deploys_into_store() {
    let rig = rig(Gh {
        runs: vec![run(42, "waiting")],
        statuses: vec!["waiting", "in_progress", "queued"],
        graphql_fails: false,
    });
    let mut s = store(&rig);
    let config = Config::load(&rig.home).unwrap();
    let mut cx = Cx {
        store: &mut s,
        ports: &rig.fakes.ports,
        home: &rig.home,
        config: &config,
    };

    let passed: Result<(), CpdError> = GithubSource::default().pass(&mut cx, rig.fakes.clock.utc());

    passed.expect("the GitHub pass");
    assert_eq!(
        active(&rig),
        [42],
        "the engine's GitHub pass fills swr_deploys without a manual `cpd pr poll`"
    );
}
