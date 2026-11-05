//! Characterization (pro-d408, pro-n345): a refused `inbox claim` writes nothing, except a stale claim, which closes its row
//! (scout-actions §2; cmd.rs:570-579).

use std::fs;
use std::sync::Arc;

use clap::Parser;
use serde_json::{Value, json};

use super::cmd::{Command, run_with};
use super::model::{Condition, Kind, Urgency, reconcile};
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::{FakeRunner, Reply};
use crate::host::spawn::FakeSpawn;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// A temp home holding item 1 (`kind`, `subject`) and the ports of `runner`.
/// `Env::open_with` reads `$CPD_HOME`: the one variable these tests set, to their own temp dir.
fn fixture(
    kind: Kind,
    subject: &str,
    runner: FakeRunner,
) -> (tempfile::TempDir, CpdHome, Store, Ports) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let condition = Condition {
        kind,
        urgency: Urgency::Yellow,
        subject: subject.into(),
        label: "acme/pay#5 wants a review".into(),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
    };
    reconcile(&mut store, &[condition], "2026-01-01T00:00:00.000Z").unwrap();
    let ports = Ports {
        clock: Arc::new(ManualClock::new()),
        runner: Arc::new(runner),
        spawn: Arc::new(FakeSpawn::new()),
        ..Ports::real()
    };
    unsafe { std::env::set_var("CPD_HOME", home.root()) };
    (dir, home, store, ports)
}

fn run(ports: &Ports, argv: &[&str]) -> Result<Output, CpdError> {
    let command = Cli::try_parse_from(std::iter::once("cpd").chain(argv.iter().copied()))
        .unwrap()
        .command;
    run_with(ports, command)
}

fn refused(out: Result<Output, CpdError>, code: ErrorCode) -> CpdError {
    let e = out.expect_err("an error");
    assert_eq!(e.code(), code, "{e}");
    e
}

/// Every row of `inbox_items` and the counts of Events and transactions: what a refusal must leave as it was.
fn snapshot(store: &mut Store) -> Value {
    store
        .read(|c| {
            let mut s = c.prepare(
                "SELECT id, kind, subject, closed_at, snoozed_until, dismissed_at, claimed_at FROM inbox_items ORDER BY id",
            )?;
            let rows = s
                .query_map([], |r| {
                    Ok(json!([
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, Option<String>>(6)?
                    ]))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let events: i64 = c.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?;
            let txns: i64 = c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?;
            Ok(json!({"rows": rows, "events": events, "txns": txns}))
        })
        .unwrap()
}

const SUBJECT: &str = "pr:acme/pay#5";

fn graphql(state: &str) -> Value {
    json!({"data": {"viewer": {"login": "clayton-duarte"}, "p0": {"pullRequest": {
        "title": "Add retries", "isDraft": false, "state": state, "reviewDecision": null,
        "author": {"login": "ryan"}, "latestReviews": {"nodes": []}}}}})
}

fn gh(reply: Reply) -> FakeRunner {
    FakeRunner::new(ManualClock::new()).on("gh", |_| true, reply)
}

/// A review-requested item 1 with a team cache holding its primary post.
fn with_cache(runner: FakeRunner) -> (tempfile::TempDir, CpdHome, Store, Ports) {
    let f = fixture(Kind::ReviewRequested, SUBJECT, runner);
    let path = f.1.run_dir().join("team-review.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let post = json!({"repo": "acme/pay", "number": 5, "ts": 1767225600.0, "ts_raw": "1767225600.000100",
        "author": "Ryan Case", "primary": true, "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false});
    let cache = json!({"version": 1, "polled_at": "2026-01-01T00:00:00.000Z", "channel": "C02JU6C2Z1D", "read": 10, "posts": [post]});
    fs::write(path, cache.to_string()).unwrap();
    f
}

fn closed_at(store: &mut Store) -> Option<String> {
    store
        .read(|c| {
            Ok(
                c.query_row("SELECT closed_at FROM inbox_items WHERE id = 1", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
}

#[test]
fn a_refused_claim_of_an_unknown_item_is_item_not_found_and_changes_no_row() {
    let (_d, _h, mut store, ports) = with_cache(gh(Reply::ok("{}")));
    let before = snapshot(&mut store);
    let e = refused(run(&ports, &["claim", "99"]), ErrorCode::ItemNotFound);
    assert_eq!(e.exit().code(), 4);
    assert_eq!(snapshot(&mut store), before);
}

#[test]
fn a_refused_claim_of_an_item_of_another_kind_is_not_claimable_and_changes_no_row() {
    let (_d, _h, mut store, ports) = fixture(Kind::Dialog, "core-docs", gh(Reply::ok("{}")));
    let before = snapshot(&mut store);
    let e = refused(run(&ports, &["claim", "1"]), ErrorCode::NotClaimable);
    assert_eq!(e.details()["kind"], "dialog");
    assert_eq!(snapshot(&mut store), before);
}

#[test]
fn a_refused_claim_with_no_team_cache_is_post_not_found_and_changes_no_row() {
    let (_d, _h, mut store, ports) = fixture(Kind::ReviewRequested, SUBJECT, gh(Reply::ok("{}")));
    let before = snapshot(&mut store);
    refused(run(&ports, &["claim", "1"]), ErrorCode::PostNotFound);
    assert_eq!(snapshot(&mut store), before);
}

#[test]
fn a_refused_claim_whose_lookup_fails_is_gh_failed_and_leaves_the_row_open() {
    let (_d, _h, mut store, ports) = with_cache(gh(Reply::exit(1, "", "gh: HTTP 500\n")));
    let before = snapshot(&mut store);
    refused(run(&ports, &["claim", "1"]), ErrorCode::GhFailed);
    assert_eq!(snapshot(&mut store), before);
    assert_eq!(closed_at(&mut store), None);
}

#[test]
fn a_refused_claim_of_a_merged_pull_request_closes_the_row_and_names_the_reason() {
    let (_d, _h, mut store, ports) = with_cache(gh(Reply::ok(&graphql("MERGED").to_string())));
    let e = refused(run(&ports, &["claim", "1"]), ErrorCode::NotClaimable);
    assert_eq!(e.details()["reason"], "merged");
    assert!(closed_at(&mut store).is_some());
    let again = refused(run(&ports, &["claim", "1"]), ErrorCode::ItemClosed);
    assert_eq!(again.exit().code(), 3);
}

#[test]
fn a_refused_claim_of_a_closed_pull_request_closes_the_row_and_names_the_reason() {
    let (_d, _h, mut store, ports) = with_cache(gh(Reply::ok(&graphql("CLOSED").to_string())));
    let e = refused(run(&ports, &["claim", "1"]), ErrorCode::NotClaimable);
    assert_eq!(e.details()["reason"], "closed");
    assert!(closed_at(&mut store).is_some());
}
