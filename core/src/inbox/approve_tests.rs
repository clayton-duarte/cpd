//! `cpd inbox approve <id>` (commands.md §21.13, swr-github.md §9): the mutation it queues from the stored run, and its refusals.
//! No process, no network, no `gh` call.

use std::sync::{Arc, Mutex};

use clap::Parser;
use serde_json::{Value, json};

use super::cmd::{Command, approve_in};
use super::model::{Condition, Kind, Urgency, reconcile};
use crate::clock::ManualClock;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::{Call, FakeRunner, Handler, Reply};
use crate::host::spawn::FakeSpawn;
use crate::output::Output;
use crate::ports::Ports;

const LABEL: &str = "acme/pay deploy production: Release 1.2";
const GET: &str = "repos/acme/pay/actions/runs/123/pending_deployments";

/// The scripted `gh`: GET answers `get` (`None`: exit 1, HTTP 500), POST answers `[]` (or fails when `post_fails`).
struct Gh {
    get: Option<Value>,
    post_fails: bool,
    seen: Mutex<Vec<Call>>,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        self.seen.lock().unwrap().push(call.clone());
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", GET] => match &self.get {
                Some(v) => Reply::ok(&v.to_string()),
                None => Reply::exit(1, "", "gh: HTTP 500: Server Error\n"),
            },
            ["api", "--method", "POST", GET, "--input", "-"] if self.post_fails => {
                Reply::exit(1, "", "gh: HTTP 422\n")
            }
            ["api", "--method", "POST", GET, "--input", "-"] => Reply::ok("[]"),
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    env: Env,
    runner: Arc<FakeRunner>,
}

fn entry(id: u64, name: &str, can: bool) -> Value {
    json!({"environment": {"id": id, "name": name}, "wait_timer": 0, "current_user_can_approve": can, "reviewers": []})
}

fn condition(kind: Kind, subject: &str) -> Condition {
    Condition {
        kind,
        urgency: Urgency::Yellow,
        subject: subject.into(),
        label: LABEL.into(),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
        head_oid: None,
        reviewed_oid: None,
    }
}

/// A temp home with item 1 of `kind` and `subject`, and a `gh` answering `get`.
fn fixture_of(
    kind: Kind,
    subject: &str,
    get: Option<Value>,
    post_fails: bool,
) -> (Fixture, Arc<Gh>) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let clock = ManualClock::new();
    let gh = Arc::new(Gh {
        get,
        post_fails,
        seen: Mutex::new(Vec::new()),
    });
    let runner = Arc::new(FakeRunner::new(clock.clone()).route("gh", gh.clone()));
    let ports = Ports {
        clock: Arc::new(clock),
        runner: runner.clone(),
        spawn: Arc::new(FakeSpawn::new()),
        ..Ports::real()
    };
    let mut env = Env::open_home_with(&ports, home.clone()).unwrap();
    if let Some(pending) = &gh.get {
        let run = json!({"id": 123, "updated_at": "U0", "display_title": "Release 1.2", "created_at": "2026-10-27T13:05:00Z"});
        env.store
            .write("fixture deploy", |tx| {
                tx.execute(
                    "INSERT INTO swr_deploys (repo, run_id, updated_at, run_json, pending_json, inactive_at) VALUES ('acme/pay', 123, 'U0', ?1, ?2, NULL)",
                    [run.to_string(), pending.to_string()],
                )?;
                Ok::<(), CpdError>(())
            })
            .unwrap();
    }
    reconcile(
        &mut env.store,
        &[condition(kind, subject)],
        "2026-01-01T00:00:00.000Z",
    )
    .unwrap();
    (
        Fixture {
            _dir: dir,
            env,
            runner,
        },
        gh,
    )
}

fn fixture(get: Value) -> Fixture {
    fixture_of(Kind::Deploy, "deploy:acme/pay#123", Some(get), false).0
}

fn fields(out: Result<Output, CpdError>) -> Value {
    match out.unwrap() {
        Output::Success(f) => Value::Object(f),
        Output::Owned => panic!("a success"),
    }
}

fn refused(out: Result<Output, CpdError>, code: ErrorCode) -> CpdError {
    let e = out.expect_err("an error");
    assert_eq!(e.code(), code, "{e}");
    e
}

fn gh_calls(f: &Fixture) -> Vec<Call> {
    f.runner
        .calls()
        .into_iter()
        .filter(|c| c.tool == "gh")
        .collect()
}

fn closed_at(f: &mut Fixture) -> Option<String> {
    f.env
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT closed_at FROM inbox_items WHERE id = 1", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
}

/// `(target_key, op_kind, status, vars, patch)` of every mutation.
fn mutations(f: &mut Fixture) -> Vec<(String, String, String, Value, Value)> {
    f.env
        .store
        .read(|c| {
            let mut s = c.prepare("SELECT target_key, op_kind, status, vars_json, patch_json FROM swr_mutations ORDER BY id")?;
            let rows = s
                .query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap()
        .into_iter()
        .map(|(t, k, st, v, p)| (t, k, st, serde_json::from_str(&v).unwrap(), serde_json::from_str(&p).unwrap()))
        .collect()
}

#[test]
fn approve_queues_the_environments_the_user_can_approve_and_calls_no_gh() {
    let mut f = fixture(json!([
        entry(7, "production", true),
        entry(9, "staging", false)
    ]));
    let out = fields(approve_in(&mut f.env, 1));

    assert!(gh_calls(&f).is_empty(), "the command never calls GitHub");
    assert_eq!(out["queued"], true);
    assert!(out["mutation_id"].is_i64());
    assert_eq!(out["environments"], json!(["production"]));
    assert_eq!(
        mutations(&mut f),
        [(
            "github:deploy:acme/pay#123".to_owned(),
            "approve".to_owned(),
            "pending".to_owned(),
            json!({"run_id": 123, "environments": ["production"]}),
            json!({"deploy": "approving"})
        )]
    );
}

#[test]
fn every_approvable_environment_is_named_sorted() {
    let mut f = fixture(json!([
        entry(3, "staging", true),
        entry(7, "production", true)
    ]));
    let out = fields(approve_in(&mut f.env, 1));
    assert_eq!(out["environments"], json!(["production", "staging"]));
}

#[test]
fn nothing_approvable_queues_nothing() {
    for get in [json!([]), json!([entry(9, "staging", false)])] {
        let mut f = fixture(get);
        let out = fields(approve_in(&mut f.env, 1));
        assert!(gh_calls(&f).is_empty());
        assert_eq!(
            out,
            json!({"id": 1, "repo": "acme/pay", "run_id": 123, "environments": [], "already": true})
        );
        assert!(mutations(&mut f).is_empty());
    }
}

#[test]
fn an_unknown_item_is_item_not_found_and_queues_nothing() {
    let mut f = fixture(json!([entry(7, "production", true)]));
    let e = refused(approve_in(&mut f.env, 99), ErrorCode::ItemNotFound);
    assert_eq!(e.exit().code(), 4);
    assert!(mutations(&mut f).is_empty());
    // A positive effect beside the negative one: the known item is queued.
    fields(approve_in(&mut f.env, 1));
    assert_eq!(mutations(&mut f).len(), 1);
}

#[test]
fn a_second_approve_of_the_same_run_queues_no_second_mutation() {
    let mut f = fixture(json!([entry(7, "production", true)]));
    let first = fields(approve_in(&mut f.env, 1));
    let second = fields(approve_in(&mut f.env, 1));
    assert_eq!(first["mutation_id"], second["mutation_id"]);
    assert_eq!(mutations(&mut f).len(), 1);
    assert!(gh_calls(&f).is_empty());
}

#[test]
fn an_item_of_another_kind_is_not_approvable_and_stays_open() {
    let (mut f, _) = fixture_of(Kind::ReviewRequested, "acme/pay#5", Some(json!([])), false);
    let e = refused(approve_in(&mut f.env, 1), ErrorCode::NotApprovable);
    assert_eq!(e.exit().code(), 3);
    assert_eq!(e.details()["kind"], "review-requested");
    assert_eq!(e.details()["item"], 1);
    assert!(gh_calls(&f).is_empty());
    assert_eq!(closed_at(&mut f), None);
}

#[test]
fn a_malformed_deploy_subject_is_a_store_error_and_makes_no_gh_call() {
    let (mut f, _) = fixture_of(Kind::Deploy, "deploy:nonsense", Some(json!([])), false);
    refused(approve_in(&mut f.env, 1), ErrorCode::StoreFailed);
    assert!(gh_calls(&f).is_empty());
    assert_eq!(closed_at(&mut f), None);
}

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[test]
fn inbox_approve_parses_with_the_item_id() {
    assert!(matches!(
        Cli::try_parse_from(["cpd", "approve", "7"])
            .unwrap()
            .command,
        Command::Approve(_)
    ));
    assert!(Cli::try_parse_from(["cpd", "approve"]).is_err());
    assert!(Cli::try_parse_from(["cpd", "approve", "x"]).is_err());
    // Red against the stub: the id reaches the seam, so item 7 does not exist.
    let mut f = fixture(json!([]));
    refused(approve_in(&mut f.env, 7), ErrorCode::ItemNotFound);
}
