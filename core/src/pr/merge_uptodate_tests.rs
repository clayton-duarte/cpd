//! `pr merge` does not gate on up-to-dateness (b79): a clean head behind the default branch merges like any other. In process on
//! the janitor's `Rig`: `gh` is a scripted route on the `FakeRunner`, the store is the rig's temp database. No process,
//! no network, no real git.

use std::sync::Arc;

use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::ports::Ports;
use crate::store::Store;

use super::{MergeArgs, PullRef};

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

/// The `gh` of the merge gate: the compare read answers `status`; the merge itself is a no-op.
struct Gh {
    status: &'static str,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", _, ..] if args[1].starts_with("repos/acme/pay/compare/main...") => {
                Reply::ok(&format!(r#"{{"status":"{}"}}"#, self.status))
            }
            ["pr", "merge", ..] => Reply::ok(""),
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

/// A rig with the open Workflow `api` of Plan `pay` (pull request `acme/pay#801`), clean on `main`, with the head
/// `HEAD` and the repo's merge method cached. `gh` answers the containment read with `status`.
fn rig(status: &'static str) -> Rig {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    rig.sql(&format!(
        "UPDATE pull_requests SET mergeable = 'mergeable', merge_state = 'clean', base_ref = 'main',
                default_branch = 'main', head_oid = '{HEAD}', checks = 'passed' WHERE number = 801;
         INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed,
                                merge_commit_allowed, rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/pay', 'main', 0, 1, 1, 0, 0, 1, 't');"
    ));
    let _ = (*rig.fakes.runner)
        .clone()
        .route("gh", Arc::new(Gh { status }));
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    rig
}

fn merge_args() -> MergeArgs {
    MergeArgs {
        pull: "acme/pay#801".parse::<PullRef>().unwrap(),
    }
}

fn merge_ops(store: &mut Store) -> i64 {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM swr_mutations WHERE op_kind = 'merge'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

#[test]
fn a_behind_head_is_not_refused() {
    let rig = rig("behind");
    let out = Ports::scope(&rig.fakes.ports, || {
        super::merge(&rig.fakes.ports, merge_args())
    })
    .expect("a clean head behind main is not refused");
    assert!(matches!(out, crate::output::Output::Success(_)));
    let mut store = Store::open(&rig.home, 5000).unwrap();
    assert_eq!(merge_ops(&mut store), 1, "the merge op is enqueued");
    let updates: i64 = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM jobs WHERE plan = 'pay' AND kind = 'update'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(updates, 0, "no update Job is queued");
}

#[test]
fn a_contained_head_is_not_refused() {
    let rig = rig("ahead");
    let out = Ports::scope(&rig.fakes.ports, || {
        super::merge(&rig.fakes.ports, merge_args())
    })
    .expect("a contained head is not refused");
    assert!(matches!(out, crate::output::Output::Success(_)));
    let mut store = Store::open(&rig.home, 5000).unwrap();
    assert_eq!(merge_ops(&mut store), 1, "the normal merge path enqueues");
}
