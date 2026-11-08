//! The drain's up-to-date merge check: immediately before `gh pr merge`, the engine re-reads whether the merge
//! operation's head still contains the default branch's head, and a head that no longer does is not merged. In
//! process on the janitor's `Rig`: `gh` is a scripted route on the `FakeRunner`, the store is the rig's temp
//! database. No process, no network, no real git.

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
        "UPDATE pull_requests SET base_ref = 'main', default_branch = 'main', head_oid = '{HEAD}',
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
fn the_drain_does_not_merge_a_head_that_no_longer_contains_main() {
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
        !matches!(outcome, ExecOutcome::Done),
        "the drain does not execute the merge"
    );
    let calls = rig.fakes.runner.calls();
    assert!(
        !calls
            .iter()
            .any(|c| { c.program == "gh" && c.args.iter().any(|a| a == "merge") }),
        "gh pr merge is not called: {calls:?}"
    );
}
