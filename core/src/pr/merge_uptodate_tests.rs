//! The up-to-date merge gate: `pr merge` refuses a head that does not contain the current default branch's head,
//! queues the Workflow's update Job, and leaves a head that does contain it on the normal merge path. In process on
//! the janitor's `Rig`: `gh` is a scripted route on the `FakeRunner`, the store is the rig's temp database. No process,
//! no network, no real git.

use std::sync::Arc;

use rusqlite::OptionalExtension;

use crate::error::ErrorCode;
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
fn a_behind_head_is_refused_with_head_behind_main() {
    let rig = rig("behind");
    let err = Ports::scope(&rig.fakes.ports, || {
        super::merge(&rig.fakes.ports, merge_args())
    })
    .expect_err("a head behind main is refused");
    assert_eq!(
        err.code(),
        ErrorCode::HeadBehindMain,
        "the refusal is head_behind_main: {}",
        err.message()
    );
    let mut store = Store::open(&rig.home, 5000).unwrap();
    assert_eq!(merge_ops(&mut store), 0, "no merge op is enqueued");
}

#[test]
fn a_behind_head_queues_the_update_job() {
    let rig = rig("behind");
    let err = Ports::scope(&rig.fakes.ports, || {
        super::merge(&rig.fakes.ports, merge_args())
    })
    .expect_err("a head behind main is refused");
    assert_eq!(err.code(), ErrorCode::HeadBehindMain);

    let mut store = Store::open(&rig.home, 5000).unwrap();
    let jobs: Vec<String> = store
        .read(|c| {
            let mut stmt =
                c.prepare("SELECT key FROM jobs WHERE plan = 'pay' AND kind = 'update'")?;
            Ok(stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(jobs.len(), 1, "the Workflow's update Job is queued");
    let reason: Option<String> = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT json_extract(data, '$.reason') FROM events WHERE kind = 'job-added' AND plan = 'pay' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
        })
        .unwrap();
    assert_eq!(
        reason.as_deref(),
        Some("behind"),
        "the Job's cause is behind"
    );
}

#[test]
fn a_contained_head_is_not_refused() {
    let rig = rig("ahead");
    let cwd = rig.home.root();
    let contained = Ports::scope(&rig.fakes.ports, || {
        crate::pr::gh::head_contains_base(&*rig.fakes.runner, cwd, "acme/pay", "main", HEAD)
    })
    .expect("the containment read succeeds");
    assert!(contained, "the head contains main");

    let out = Ports::scope(&rig.fakes.ports, || {
        super::merge(&rig.fakes.ports, merge_args())
    })
    .expect("a contained head is not refused");
    assert!(
        matches!(out, crate::output::Output::Success(_)),
        "the merge path is unchanged"
    );

    let mut store = Store::open(&rig.home, 5000).unwrap();
    assert_eq!(merge_ops(&mut store), 1, "the normal merge path enqueues");
    let calls = rig.fakes.runner.calls();
    assert!(
        calls.iter().any(|c| {
            c.program == "gh"
                && c.args
                    .iter()
                    .any(|a| a.starts_with("repos/acme/pay/compare/main..."))
        }),
        "the merge gate read the containment comparison"
    );
}
