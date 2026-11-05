//! A Plan ends `done` only when no Job of it is left non-final (accepted assumption A1: a Job is non-final when its status is
//! not `merged` or `cancelled`). The three writers that end the last Workflow — the poll's merge, `pr::record_pull` and
//! `workflow cancel` — hold the Plan in `deploy` while one is; with none, the Plan still ends `done`.
//! In process on the janitor's `Rig`: git is the `Universe`, `gh` a scripted handler, the store a temp database.

use std::sync::Arc;

use crate::host::runner::{Call, Handler, Reply};
use crate::ids::{PlanId, WorkflowKey};
use crate::janitor_tests::Rig;
use crate::plan::fingerprint::load_workflows;
use crate::ports::Ports;
use crate::pr::PollArgs;
use crate::store::Store;

const NODE: &str = r#"{"repository":{"nameWithOwner":"acme/pay","isArchived":false},"number":801,"url":"https://github.com/acme/pay/pull/801","title":"T",
    "isDraft":false,"state":"MERGED","createdAt":"2026-10-20T10:00:00Z","updatedAt":"U1","mergedAt":"2026-10-20T11:00:00Z","reviewDecision":null,
    "mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,
    "reviewThreads":{"nodes":[]},"commits":{"nodes":[{"commit":{"statusCheckRollup":null}}]}}"#;

/// The in-process `gh`: pull request `acme/pay#801` is `MERGED` on GitHub; any other call exits 9.
struct MergedGh;

impl Handler for MergedGh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["pr", "list", ..] => Reply::ok(
                r#"[{"number":801,"url":"https://github.com/acme/pay/pull/801","state":"MERGED","createdAt":"2026-10-20T10:00:00Z"}]"#,
            ),
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => {
                Reply::ok(
                    r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#,
                )
            }
            ["api", "graphql", "-f", query, ..] if query.contains(": repository(") => {
                Reply::ok(&format!(r#"{{"data":{{"p0":{{"pullRequest":{NODE}}}}}}}"#))
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

fn rig_with_gh(rig: &Rig) {
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(MergedGh));
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
}

/// Inserts a Job `key` of status `status` in Workflow `workflow` of Plan `pay`.
fn job(rig: &Rig, workflow: &str, key: &str, status: &str) {
    rig.sql(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                           status, failed_cause, merged_sha, appended, fingerprint, created_txn, created_at)
           VALUES ('pay', '{workflow}', '{key}', 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', {cause}, {sha}, 0, '{fp}', cpd_txn(), 't');",
        cause = if status == "failed" { "'crashed'" } else { "NULL" },
        sha = if status == "merged" { "'abc'" } else { "NULL" },
        fp = "e".repeat(64)
    ));
}

/// `(phase, done_at is null, number of plan-done Events)` of Plan `pay`.
fn state(rig: &Rig) -> (String, bool, i64) {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT phase, done_at IS NULL, (SELECT count(*) FROM events WHERE kind = 'plan-done') FROM plans WHERE id = 'pay'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap()
}

#[derive(clap::Parser)]
struct PollCli {
    #[command(flatten)]
    args: PollArgs,
}

/// The poll finds `acme/pay#801`, the one open Workflow `api`, merged on GitHub.
fn poll_once(rig: &Rig) {
    rig_with_gh(rig);
    Ports::scope(&rig.fakes.ports, || {
        crate::pr::run(
            &rig.fakes.ports,
            crate::pr::Command::Poll(
                <PollCli as clap::Parser>::parse_from(["poll", "--plan", "pay"]).args,
            ),
        )
    })
    .expect("the poll runs");
}

#[test]
fn a_poll_merge_of_the_last_workflow_holds_the_plan_in_deploy_while_a_job_is_failed() {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    job(&rig, "api", "api-fix", "failed");

    poll_once(&rig);

    assert_eq!(
        state(&rig),
        ("deploy".to_owned(), true, 0),
        "a failed Job holds the Plan in deploy: no done_at, no plan-done"
    );
}

#[test]
fn a_plan_with_every_job_final_still_ends_done_with_one_plan_done_event() {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    job(&rig, "api", "api-fix", "merged");

    poll_once(&rig);

    assert_eq!(state(&rig), ("done".to_owned(), false, 1));
}

#[test]
fn recording_the_last_merged_pull_request_holds_the_plan_in_deploy_while_a_job_is_not_final() {
    let mut rig = Rig::new();
    rig.workflow("api", "dispatch");
    rig.sql(
        "UPDATE plans SET phase = 'dispatch' WHERE id = 'pay';
         UPDATE workflows SET started_at = '2026-10-20T09:00:00Z' WHERE key = 'api';",
    );
    job(&rig, "api", "api-fix", "failed");
    rig_with_gh(&rig);
    let (plan, key) = (
        PlanId::parse("pay").unwrap(),
        WorkflowKey::parse("api").unwrap(),
    );
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let row = store
        .read(|c| Ok(load_workflows(c, &plan)?.remove(0)))
        .unwrap();
    let lead = row.lead_worktree.clone().expect("a Lead worktree");

    let pulled = Ports::scope(&rig.fakes.ports, || {
        crate::pr::pull_request(&lead, &row, "T", None, &[])
    })
    .expect("the merged pull request is read");
    store
        .write("test", |tx| {
            crate::pr::record_pull(tx, &plan, &key, &pulled)
        })
        .unwrap();

    assert_eq!(state(&rig), ("deploy".to_owned(), true, 0));
}

#[test]
fn cancelling_the_last_open_workflow_holds_the_plan_in_deploy_while_a_job_is_not_final() {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    rig.workflow("web", "merged");
    job(&rig, "web", "web-fix", "queued");
    // `workflow cancel` gives up only a pull request the poll stored `closed`.
    rig.sql("UPDATE pull_requests SET state = 'closed' WHERE number = 801");
    rig_with_gh(&rig);

    Ports::scope(&rig.fakes.ports, || {
        crate::workflow::cancel::cancel({
            #[derive(clap::Parser)]
            struct Cli {
                #[command(flatten)]
                args: crate::workflow::CancelArgs,
            }
            <Cli as clap::Parser>::parse_from(["cancel", "api", "--plan", "pay"]).args
        })
    })
    .expect("the cancel runs");

    assert_eq!(state(&rig), ("deploy".to_owned(), true, 0));
}
