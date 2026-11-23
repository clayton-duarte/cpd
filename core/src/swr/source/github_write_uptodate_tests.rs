//! The drain submits a clean behind-main PR with its expected head; GitHub enforces branch protection.

use std::sync::Arc;

use serde_json::json;

use crate::config::Config;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::ports::Ports;
use crate::store::Store;
use crate::swr::outbox;
use crate::swr::source::github::GithubSource;
use crate::swr::source::{Cx, ExecOutcome, Source};

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

/// The `gh` of the drain: the compare read says the head no longer contains `main`; the merge itself is a no-op.
struct Gh;

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", _, ..] if args[1].starts_with("repos/acme/pay/compare/main...") => {
                Reply::ok(r#"{"status":"behind"}"#)
            }
            ["pr", "merge", ..] => Reply::ok(""),
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

/// A rig with the open Workflow `api` of Plan `pay` (pull request `acme/pay#801`), clean on `main` with the head
/// `HEAD`; `gh` answers the containment read as behind.
fn rig() -> Rig {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    rig.sql(&format!(
        "UPDATE pull_requests SET base_ref = 'main', default_branch = 'main', head_oid = '{HEAD}', review_threads_complete = 1, open_threads = '[]',
                mergeable = 'mergeable', merge_state = 'clean', checks = 'passed' WHERE number = 801;
         INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed,
                                merge_commit_allowed, rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/pay', 'main', 0, 1, 1, 0, 0, 1, 't');"
    ));
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(Gh));
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    rig
}

#[test]
fn the_drain_merges_a_clean_head_even_when_it_does_not_contain_main() {
    let rig = rig();
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let now = rig.fakes.ports.clock.utc();
    let id = super::enqueue(
        &mut store,
        now,
        "github:pr:acme/pay#801",
        "merge",
        json!({"expected_head_oid": HEAD, "method": "squash"}),
        Some(json!({"state": "merging"})),
    )
    .unwrap();
    let config = Config::load(&rig.home).unwrap();
    let m = outbox::get(&mut store, id).unwrap().unwrap();
    let outcome = Ports::scope(&rig.fakes.ports, || {
        let mut cx = Cx {
            store: &mut store,
            ports: &rig.fakes.ports,
            home: &rig.home,
            config: &config,
        };
        GithubSource::default().execute(&mut cx, &m)
    });
    assert!(
        matches!(outcome, ExecOutcome::Done),
        "the drain executes the merge"
    );
    let calls = rig.fakes.runner.calls();
    assert!(
        !calls.iter().any(|c| c
            .args
            .iter()
            .any(|a| a.starts_with("repos/acme/pay/compare/"))),
        "no redundant containment request: {calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|c| { c.program == "gh" && c.args.iter().any(|a| a == "merge") }),
        "gh pr merge is called: {calls:?}"
    );
}

#[test]
fn a_historical_no_change_update_is_not_requeued_by_the_merge_drain() {
    let rig = rig();
    // The old behind-triggered update completed without changing the lead branch.
    rig.sql(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, created_txn, created_at, merged_sha)
         VALUES ('pay', 'api', 'old-update', 'Update', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'update', 'merged', 1, '{}', cpd_txn(), 't', '{HEAD}');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha, submitted_sha, submitted_at, merged_sha, actual_files, started_at, ended_at)
         VALUES ('pay', 'old-update-a1', 'old-update', 1, 'S2', 'ladder', 'ended', 'merged', 'merged', 'cpd/pay--old-update-a1', '/w', '{HEAD}', '{HEAD}', 't', '{HEAD}', '[]', 't', 't');",
        "f".repeat(64)
    ));
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let now = rig.fakes.ports.clock.utc();
    let id = super::enqueue(
        &mut store,
        now,
        "github:pr:acme/pay#801",
        "merge",
        json!({"expected_head_oid": HEAD, "method": "squash"}),
        Some(json!({"state": "merging"})),
    )
    .unwrap();
    let m = outbox::claim_next(&mut store, now, "github")
        .unwrap()
        .unwrap();
    assert_eq!(m.id, id);
    let config = Config::load(&rig.home).unwrap();
    let outcome = Ports::scope(&rig.fakes.ports, || {
        let mut cx = Cx {
            store: &mut store,
            ports: &rig.fakes.ports,
            home: &rig.home,
            config: &config,
        };
        GithubSource::default().execute(&mut cx, &m)
    });
    assert!(matches!(outcome, ExecOutcome::Done));
    let jobs = store.read(|c| {
        let mut stmt = c.prepare("SELECT key, status FROM jobs WHERE plan = 'pay' AND workflow = 'api' AND kind = 'update' ORDER BY seq")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }).unwrap();
    assert_eq!(
        jobs,
        vec![("old-update".into(), "merged".into())],
        "the completed update remains the last update"
    );
    let calls = rig.fakes.runner.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.program == "gh" && c.args.iter().any(|a| a == "merge")),
        "merge was submitted: {calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c
            .args
            .iter()
            .any(|a| a.starts_with("repos/acme/pay/compare/"))),
        "no retired behind re-check: {calls:?}"
    );
}
