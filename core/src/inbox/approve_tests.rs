//! `cpd inbox approve <id>` (commands.md §21.13) on a scripted `gh`: the calls it makes, what it writes, and its refusals.
//! No process, no network.

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
            ["api", "--method", "POST", GET, "--input", "-"] if self.post_fails => Reply::exit(1, "", "gh: HTTP 422\n"),
            ["api", "--method", "POST", GET, "--input", "-"] => Reply::ok("[]"),
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    home: CpdHome,
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
    }
}

/// A temp home with item 1 of `kind` and `subject`, and a `gh` answering `get`.
fn fixture_of(kind: Kind, subject: &str, get: Option<Value>, post_fails: bool) -> (Fixture, Arc<Gh>) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let clock = ManualClock::new();
    let gh = Arc::new(Gh { get, post_fails, seen: Mutex::new(Vec::new()) });
    let runner = Arc::new(FakeRunner::new(clock.clone()).route("gh", gh.clone()));
    let ports = Ports { clock: Arc::new(clock), runner: runner.clone(), spawn: Arc::new(FakeSpawn::new()), ..Ports::real() };
    let mut env = Env::open_home_with(&ports, home.clone()).unwrap();
    reconcile(&mut env.store, &[condition(kind, subject)], "2026-01-01T00:00:00.000Z").unwrap();
    (Fixture { _dir: dir, home, env, runner }, gh)
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
    f.runner.calls().into_iter().filter(|c| c.tool == "gh").collect()
}

fn closed_at(f: &mut Fixture) -> Option<String> {
    f.env.store.read(|c| Ok(c.query_row("SELECT closed_at FROM inbox_items WHERE id = 1", [], |r| r.get(0))?)).unwrap()
}

fn closed_events(f: &mut Fixture) -> Vec<Value> {
    f.env
        .store
        .read(|c| {
            let mut s = c.prepare("SELECT data FROM events WHERE kind = 'inbox-closed' ORDER BY id")?;
            let rows = s.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap()
        .into_iter()
        .map(|d| serde_json::from_str(&d).unwrap())
        .collect()
}

fn cache_path(f: &Fixture) -> std::path::PathBuf {
    f.home.run_dir().join("deploy-runs.json")
}

fn run_entry(repo: &str, run_id: u64) -> Value {
    json!({"repo": repo, "workflow": "main.yml", "run_id": run_id, "environment": "production", "title": "Release 1.2",
           "url": format!("https://github.com/{repo}/actions/runs/{run_id}"), "created_at": "2026-10-27T13:05:00Z"})
}

fn cache(runs: &[Value]) -> Value {
    json!({"version": 1, "polled_at": "2026-10-27T14:59:55.000Z", "runs": runs})
}

fn put_cache(f: &Fixture, value: &Value) {
    put_raw(f, &value.to_string());
}

fn put_raw(f: &Fixture, text: &str) {
    std::fs::create_dir_all(f.home.run_dir()).unwrap();
    std::fs::write(cache_path(f), text).unwrap();
}

fn read_cache(f: &Fixture) -> Value {
    serde_json::from_str(&std::fs::read_to_string(cache_path(f)).unwrap()).unwrap()
}

fn stdin_of(call: &Call) -> Value {
    serde_json::from_slice(call.stdin.as_ref().expect("a stdin")).unwrap()
}

#[test]
fn approve_posts_the_environments_the_user_can_approve_and_closes_the_item() {
    let mut f = fixture(json!([entry(7, "production", true), entry(9, "staging", false)]));
    put_cache(&f, &cache(&[run_entry("acme/pay", 123)]));
    let out = fields(approve_in(&mut f.env, 1));

    let calls = gh_calls(&f);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0].args, ["api", GET]);
    assert_eq!(calls[0].cwd.as_deref(), Some(f.home.root()));
    assert_eq!(calls[0].deadline, std::time::Duration::from_secs(15));
    assert_eq!(calls[1].args, ["api", "--method", "POST", GET, "--input", "-"]);
    assert_eq!(calls[1].deadline, std::time::Duration::from_secs(15));
    assert_eq!(stdin_of(&calls[1]), json!({"environment_ids": [7], "state": "approved", "comment": ""}));
    assert_eq!(out, json!({"id": 1, "repo": "acme/pay", "run_id": 123, "environments": ["production"]}));
    assert!(out.get("already").is_none());

    assert!(closed_at(&mut f).is_some());
    assert_eq!(closed_events(&mut f), [json!({"item": 1, "kind": "deploy"})]);
    assert_eq!(read_cache(&f)["runs"], json!([]));
}

#[test]
fn every_approvable_environment_is_posted_sorted_by_name() {
    let mut f = fixture(json!([entry(3, "staging", true), entry(7, "production", true)]));
    let out = fields(approve_in(&mut f.env, 1));
    let calls = gh_calls(&f);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(stdin_of(&calls[1])["environment_ids"], json!([7, 3]));
    assert_eq!(out["environments"], json!(["production", "staging"]));
}

#[test]
fn approve_removes_only_that_run_from_the_cache() {
    let mut f = fixture(json!([entry(7, "production", true)]));
    let others = [run_entry("acme/pay", 124), run_entry("acme/web", 123)];
    put_cache(&f, &cache(&[run_entry("acme/pay", 123), others[0].clone(), others[1].clone()]));
    fields(approve_in(&mut f.env, 1));
    assert_eq!(read_cache(&f), cache(&others));
}

#[test]
fn nothing_approvable_posts_nothing_and_still_clears_the_cache_and_the_item() {
    for get in [json!([]), json!([entry(9, "staging", false)])] {
        let mut f = fixture(get);
        put_cache(&f, &cache(&[run_entry("acme/pay", 123)]));
        let out = fields(approve_in(&mut f.env, 1));
        let calls = gh_calls(&f);
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(calls[0].args, ["api", GET]);
        assert_eq!(out, json!({"id": 1, "repo": "acme/pay", "run_id": 123, "environments": [], "already": true}));
        assert!(closed_at(&mut f).is_some());
        assert_eq!(closed_events(&mut f), [json!({"item": 1, "kind": "deploy"})]);
        assert_eq!(read_cache(&f)["runs"], json!([]));
    }
}

#[test]
fn a_missing_or_invalid_cache_is_left_alone_and_the_item_still_closes() {
    let version2 = json!({"version": 2, "polled_at": "2026-10-27T14:59:55.000Z", "runs": [run_entry("acme/pay", 123)]}).to_string();
    for raw in [None, Some("not json".to_owned()), Some(version2)] {
        let mut f = fixture(json!([entry(7, "production", true)]));
        if let Some(raw) = &raw {
            put_raw(&f, raw);
        }
        fields(approve_in(&mut f.env, 1));
        match &raw {
            None => assert!(!cache_path(&f).exists()),
            Some(raw) => assert_eq!(&std::fs::read_to_string(cache_path(&f)).unwrap(), raw),
        }
        assert_eq!(gh_calls(&f).len(), 2);
        assert!(closed_at(&mut f).is_some());
    }
}

#[test]
fn an_unknown_item_is_item_not_found_and_makes_no_gh_call() {
    let mut f = fixture(json!([entry(7, "production", true)]));
    let e = refused(approve_in(&mut f.env, 99), ErrorCode::ItemNotFound);
    assert_eq!(e.exit().code(), 4);
    assert!(gh_calls(&f).is_empty());
    // A positive effect beside the negative one: the known item is approved.
    fields(approve_in(&mut f.env, 1));
    assert_eq!(gh_calls(&f).len(), 2);
}

#[test]
fn a_closed_item_is_item_closed_and_a_second_approve_posts_nothing() {
    let mut f = fixture(json!([entry(7, "production", true)]));
    fields(approve_in(&mut f.env, 1));
    let e = refused(approve_in(&mut f.env, 1), ErrorCode::ItemClosed);
    assert_eq!(e.exit().code(), 3);
    assert_eq!(gh_calls(&f).len(), 2);
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

#[test]
fn a_failed_read_leaves_the_item_open_and_the_cache_intact() {
    let (mut f, _) = fixture_of(Kind::Deploy, "deploy:acme/pay#123", None, false);
    let raw = cache(&[run_entry("acme/pay", 123)]).to_string();
    put_raw(&f, &raw);
    refused(approve_in(&mut f.env, 1), ErrorCode::GhFailed);
    let calls = gh_calls(&f);
    assert_eq!(calls.iter().map(|c| c.args.clone()).collect::<Vec<_>>(), [["api", GET]]);
    assert_eq!(closed_at(&mut f), None);
    assert_eq!(std::fs::read_to_string(cache_path(&f)).unwrap(), raw);
}

#[test]
fn a_failed_post_leaves_the_item_open_and_the_cache_intact() {
    let (mut f, _) = fixture_of(Kind::Deploy, "deploy:acme/pay#123", Some(json!([entry(7, "production", true)])), true);
    let raw = cache(&[run_entry("acme/pay", 123)]).to_string();
    put_raw(&f, &raw);
    refused(approve_in(&mut f.env, 1), ErrorCode::GhFailed);
    assert_eq!(gh_calls(&f).len(), 2);
    assert_eq!(closed_at(&mut f), None);
    assert_eq!(std::fs::read_to_string(cache_path(&f)).unwrap(), raw);
}

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[test]
fn inbox_approve_parses_with_the_item_id() {
    assert!(matches!(Cli::try_parse_from(["cpd", "approve", "7"]).unwrap().command, Command::Approve(_)));
    assert!(Cli::try_parse_from(["cpd", "approve"]).is_err());
    assert!(Cli::try_parse_from(["cpd", "approve", "x"]).is_err());
    // Red against the stub: the id reaches the seam, so item 7 does not exist.
    let mut f = fixture(json!([]));
    refused(approve_in(&mut f.env, 7), ErrorCode::ItemNotFound);
}
