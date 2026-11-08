//! The cold repo-settings cache (Context records air-n70, air-n71): `pr merge` reads a missing `swr_repos` row once
//! on demand through `gh` instead of refusing, refuses with the rate limit when the `github` endpoint is paused
//! (naming the reset and reading nothing), never drops a row on a refusal, and `pr poll` fills the row for the repos
//! it polls. In process on the janitor's `Rig`: `gh` is a scripted route on the `FakeRunner`, the store is the rig's
//! temp database. No process, no network, no real gh or git.

use std::sync::Arc;

use clap::Parser;
use rusqlite::OptionalExtension;
use serde_json::Value;

use crate::args::PlanArg;
use crate::clock::Clock;
use crate::error::ErrorCode;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;

use super::PollArgs;
use super::merge::MergeArgs;
use super::poll::poll;

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

/// The REST settings `gh api repos/acme/pay` answers (`allow_*` and `default_branch`, `archived`).
const SETTINGS_REST: &str = r#"{"default_branch":"main","archived":false,"allow_merge_commit":false,"allow_squash_merge":true,"allow_rebase_merge":false,"delete_branch_on_merge":true}"#;

/// The same settings as `gh repo view --json ...` answers them.
const SETTINGS_VIEW: &str = r#"{"defaultBranchRef":{"name":"main"},"isArchived":false,"autoMergeAllowed":false,"squashMergeAllowed":true,"mergeCommitAllowed":false,"rebaseMergeAllowed":false,"deleteBranchOnMerge":true}"#;

/// The `gh` of the settings read: the repo's settings, in the REST shape (the fix's read) or the `repo view` shape;
/// any other call is unexpected and exits 9.
struct SettingsGh;

impl Handler for SettingsGh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", path] if path.starts_with("repos/") && !path.contains("compare") => {
                Reply::ok(SETTINGS_REST)
            }
            ["repo", "view", _, "--json", ..] => Reply::ok(SETTINGS_VIEW),
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

/// The `gh` that must not be called: a settings read is a read, and a paused refusal reads nothing.
struct NoGh;

impl Handler for NoGh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" ")))
    }
}

/// The in-process `gh` of one `pr poll`: the search lists `acme/pay#801` open (so the poll owns a repo to fill) and its
/// repository node carries the settings, the comments read is empty. Any other call exits 9: the poll makes no separate
/// settings read.
struct PollGh;

impl Handler for PollGh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        let query = args.get(3).copied().unwrap_or_default();
        if query.contains("search(type: ISSUE") {
            let node = r#"{"repository":{"nameWithOwner":"acme/pay","defaultBranchRef":{"name":"main"},"isArchived":false,"autoMergeAllowed":false,"squashMergeAllowed":true,"mergeCommitAllowed":false,"rebaseMergeAllowed":false,"deleteBranchOnMerge":true},"number":801,"url":"https://github.com/acme/pay/pull/801","title":"Workflow 801","isDraft":false,"state":"OPEN","createdAt":"opened","updatedAt":"U1","mergedAt":null,"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,"reviewThreads":{"nodes":[]},"commits":{"nodes":[{"commit":{"statusCheckRollup":null}}]}}"#;
            return Reply::ok(&format!(
                r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-01-01T01:00:00Z"}}}}}}"#
            ));
        }
        if query.contains("reviewThreads") && query.contains(": repository(") {
            let empty = serde_json::json!({"totalCount": 0, "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": []});
            let mut data = serde_json::json!({"viewer": {"login": "me"}});
            for i in 0..query.matches(": repository(").count() {
                data[format!("p{i}")] = serde_json::json!({"pullRequest": {"id": format!("PR_{i}"), "updatedAt": "U1", "reviewThreads": empty, "comments": empty, "reviews": empty}});
            }
            return Reply::ok(&serde_json::json!({"data": data}).to_string());
        }
        Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" ")))
    }
}

/// `MergeArgs` as clap parses them: its pull request is private to `merge` (as `poll_now_tests.rs`).
#[derive(Parser)]
struct Merge {
    #[command(flatten)]
    args: MergeArgs,
}

fn merge_args() -> MergeArgs {
    Merge::try_parse_from(["merge", "acme/pay#801"])
        .unwrap()
        .args
}

/// A rig with the open Workflow `api` (pull request `acme/pay#801`) clean, with the head `HEAD` and no `swr_repos`
/// row: the settings cache is cold. The repo's base is not its default branch (no `base_ref`), so the up-to-date
/// containment gate does not read `gh` and this file's `gh` is the settings read alone.
fn cold_rig() -> Rig {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    rig.sql(&format!(
        "UPDATE pull_requests SET mergeable = 'mergeable', merge_state = 'clean', checks = 'passed',
                head_oid = '{HEAD}' WHERE number = 801;"
    ));
    // `CpdHome::from_env` and `Env::open_with` read `$CPD_HOME`: the one variable these tests set, to the rig's temp
    // dir (as `merge_uptodate_tests.rs`, `poll_tests.rs`).
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    rig
}

/// Pauses the `github` endpoint until an hour past the rig's clock (`endpoint::is_paused`).
fn pause_github(rig: &Rig) {
    let until = crate::clock::timestamp(rig.fakes.clock.utc() + time::Duration::hours(1));
    rig.sql(&format!(
        "INSERT INTO swr_endpoints (endpoint, fetch_status, paused_until) VALUES ('github', 'paused', '{until}');"
    ));
}

/// Runs `pr merge` on the rig's ports.
fn merge(rig: &Rig) -> Result<Output, crate::error::CpdError> {
    Ports::scope(&rig.fakes.ports, || {
        super::merge::merge(&rig.fakes.ports, merge_args())
    })
}

/// How many `gh` calls the runner has seen.
fn gh_calls(rig: &Rig) -> usize {
    rig.fakes
        .runner
        .calls()
        .into_iter()
        .filter(|c| c.program == "gh")
        .count()
}

/// `(default_branch, squash, merge, rebase)` of the `swr_repos` row of `repo`, or `None` when it is absent.
fn stored_settings(rig: &Rig, repo: &str) -> Option<(String, bool, bool, bool)> {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT default_branch, squash_merge_allowed, merge_commit_allowed, rebase_merge_allowed
                   FROM swr_repos WHERE repo = ?1",
                [repo],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                    ))
                },
            )
            .optional()?)
        })
        .unwrap()
}

/// The `rate_limited` flag of an error's details.
fn rate_limited(e: &crate::error::CpdError) -> Option<bool> {
    e.details().get("rate_limited").and_then(Value::as_bool)
}

#[test]
fn a_cold_settings_cache_is_read_once_on_demand_and_the_merge_proceeds() {
    let rig = cold_rig();
    assert!(
        stored_settings(&rig, "acme/pay").is_none(),
        "the cache starts cold"
    );
    let _ = (*rig.fakes.runner)
        .clone()
        .route("gh", Arc::new(SettingsGh));

    let out = merge(&rig).expect("a cold cache is read on demand, so the merge proceeds");
    assert!(
        matches!(out, Output::Success(_)),
        "the merge is queued: {out:?}"
    );

    assert_eq!(
        stored_settings(&rig, "acme/pay"),
        Some(("main".to_owned(), true, false, false)),
        "the on-demand read wrote the swr_repos row"
    );
    assert_eq!(
        gh_calls(&rig),
        1,
        "exactly one gh read fills the row (the settings read; no up-to-date compare)"
    );
}

#[test]
fn a_paused_settings_read_refuses_with_the_rate_limit() {
    let rig = cold_rig();
    pause_github(&rig);
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(NoGh));

    let err = merge(&rig).expect_err("a paused settings read refuses");
    assert_eq!(
        err.code(),
        ErrorCode::GhRateLimited,
        "the refusal is the rate limit: {}",
        err.message()
    );
    assert_eq!(
        rate_limited(&err),
        Some(true),
        "the details carry rate_limited: {:?}",
        err.details()
    );
    assert!(
        err.message().contains("2026-01-01T01:00"),
        "the reset is named: {}",
        err.message()
    );
    assert_eq!(gh_calls(&rig), 0, "a paused refusal reads nothing from gh");
}

#[test]
fn a_paused_refusal_does_not_drop_the_cached_settings() {
    let rig = cold_rig();
    pause_github(&rig);
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(NoGh));

    // (1) The cache is cold and the endpoint is paused: the refusal is the rate limit, and reads nothing.
    let err = merge(&rig).expect_err("a paused settings read refuses");
    assert_eq!(err.code(), ErrorCode::GhRateLimited, "{}", err.message());
    assert_eq!(rate_limited(&err), Some(true), "{:?}", err.details());

    // (2) The row is then filled, as the poll or the SWR source would.
    rig.sql(
        "INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed,
                                merge_commit_allowed, rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/pay', 'main', 0, 0, 1, 0, 0, 1, 't');",
    );

    // (3) The endpoint is still paused, but the cache is warm: the merge proceeds from the row and reads nothing.
    let out = merge(&rig)
        .expect("a refusal left the row, so the second merge does not refuse for a cold cache");
    assert!(
        matches!(out, Output::Success(_)),
        "the merge is queued: {out:?}"
    );
    assert_eq!(
        stored_settings(&rig, "acme/pay"),
        Some(("main".to_owned(), true, false, false)),
        "the row survives"
    );
    assert_eq!(gh_calls(&rig), 0, "neither merge read gh");
}

#[test]
fn the_poll_fills_the_repo_settings_cache() {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    // The stored row's `updated_at` is NULL against the search's `U1`: the poll reads the comments and writes, so the
    // poll cycle that fills the settings cache is a normal one.
    rig.sql("UPDATE pull_requests SET updated_at = NULL WHERE number = 801;");
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(PollGh));
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };

    Ports::scope(&rig.fakes.ports, || {
        poll(
            &rig.fakes.ports,
            PollArgs {
                plan: PlanArg { plan: None },
            },
        )
    })
    .expect("the poll runs");

    assert_eq!(
        stored_settings(&rig, "acme/pay"),
        Some(("main".to_owned(), true, false, false)),
        "the poll filled the swr_repos row of the repo it polled"
    );
}
