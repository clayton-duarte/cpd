//! The drafts a merged PR cancels (commands.md §9.2, §9.10, contract §7.6, hs-d735), in this process (`Fx::with_ports`) with a
//! scripted `gh`: the poll (or `pr merge`) that records a PR `merged` cancels its `draft` Jobs in the same transaction.

use crate::common;

use common::fx::Fx;
use cpd_core::host::runner::Reply;
use serde_json::Value;

const MERGED_AT: &str = "2026-10-21T15:01:30Z";

/// The GraphQL search answer listing acme/api#7 merged.
fn search_merged() -> Reply {
    let node = format!(
        r#"{{"repository":{{"nameWithOwner":"acme/api","defaultBranchRef":{{"name":"main"}}}},"number":7,"url":"https://github.com/acme/api/pull/7","title":"Add the thing","isDraft":false,"state":"MERGED","createdAt":"t","updatedAt":"U1","reviewThreads":{{"nodes":[]}},"mergedAt":"{MERGED_AT}","baseRefName":"main","reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","commits":{{"nodes":[{{"commit":{{"statusCheckRollup":null}}}}]}}}}"#
    );
    Reply::ok(&format!(
        r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-22T11:00:00Z"}}}}}}"#
    ))
}

/// Plan `pay-1` in phase `deploy` whose PR `api` is `open` on acme/api#7, with a full `work` draft `late` (Tier and Spec) and,
/// when `with_draft` is false, none. The Plan holds a second PR `web` (open too) so that the Plan is not done by the merge.
fn world(fx: &Fx, with_draft: bool) {
    let env = &fx.env;
    std::fs::write(
        env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"t\"\n",
    )
    .unwrap();
    env.sql(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, merge_state, authored, base_ref, default_branch)
         VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'Add the thing', 0, 'open', 'none', 'none', 't', 't', 'clean', 1, 'main', 'main');
         INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay-1', 'ctx', 'Pay', 'deploy', 'lead1', '1999-12-31T00:00:00.000Z', '2000-01-01T00:00:00.000Z');
         INSERT INTO workflows (plan, key, repo, base, name, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                          pr_number, pr_url, pr_opened_at, pull_request)
           VALUES ('pay-1', 'api', '/r', 'main', 'api', 'open', 'cpd/pay-1/api', '/w', cpd_txn(), 't', 't',
                   7, 'https://github.com/acme/api/pull/7', 't', (SELECT id FROM pull_requests WHERE number = 7));",
    );
    if with_draft {
        env.sql(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay-1', 'api', 'late', 'Late work', 'S2', 'Goal.', 'Context.', '[\"src/late.rs\"]', '[\"cargo test\"]', '[]', 'draft', 1, cpd_txn(), 't');",
        );
    }
}

fn events(fx: &Fx, kind: &str) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", kind, "--plan", "pay-1"])
        .ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

fn status(fx: &Fx, key: &str) -> String {
    fx.env
        .query(&format!("SELECT status FROM jobs WHERE key = '{key}'"))
}

/// The PR is merged, its draft is cancelled with the reason `pr merged` and one `job-cancelled` Event, and `pr-merged` is written.
fn assert_draft_cancelled_with_the_merge(fx: &Fx) {
    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM workflows WHERE key = 'api'"),
        "merged"
    );
    assert_eq!(events(fx, "pr-merged").len(), 1);
    assert_eq!(status(fx, "late"), "cancelled");
    assert_eq!(
        fx.env
            .query::<String>("SELECT cancel_reason FROM jobs WHERE key = 'late'"),
        "pr merged"
    );
    let cancelled = events(fx, "job-cancelled");
    assert_eq!(cancelled.len(), 1, "{cancelled:?}");
    assert_eq!(cancelled[0]["job"], "late");
    assert_eq!(cancelled[0]["data"]["reason"], "pr merged");
}

#[test]
fn poll_merge_cancels_drafts() {
    let fx = Fx::with_ports();
    world(&fx, true);
    fx.fake_gh(search_merged());
    fx.cpd(&["pr", "poll"]).ok();
    assert_draft_cancelled_with_the_merge(&fx);
}

#[test]
fn pr_merge_cancels_drafts() {
    let fx = Fx::with_ports();
    world(&fx, true);
    let view = |state: &str, merge_state: &str, merged_at: Option<&str>| {
        serde_json::json!({"state": state, "mergeStateStatus": merge_state, "headRefOid": "0123456789abcdef0123456789abcdef01234567", "title": "Add the thing", "mergedAt": merged_at})
            .to_string()
    };
    let open = view("OPEN", "CLEAN", None);
    let merged = view("MERGED", "UNKNOWN", Some(MERGED_AT));
    // `pr view` reads open until `pr merge` ran, merged after; the poll after it fails, so the command itself records the merge.
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    struct Gh(
        String,
        String,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    );
    impl cpd_core::host::runner::Handler for Gh {
        fn handle(&self, call: &cpd_core::host::runner::Call) -> Reply {
            use std::sync::atomic::Ordering::SeqCst;
            let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
            match args[..2] {
                ["pr", "view"] => Reply::ok(&format!(
                    "{}\n",
                    if self.2.load(SeqCst) {
                        &self.1
                    } else {
                        &self.0
                    }
                )),
                ["repo", "view"]
                    if args
                        .contains(&"squashMergeAllowed,mergeCommitAllowed,rebaseMergeAllowed") =>
                {
                    Reply::ok(
                        "{\"squashMergeAllowed\":true,\"mergeCommitAllowed\":false,\"rebaseMergeAllowed\":false}\n",
                    )
                }
                ["pr", "merge"] => {
                    self.2.store(true, SeqCst);
                    Reply::ok("")
                }
                _ => Reply::exit(1, "", ""),
            }
        }
    }
    let _ = (*fx.fakes.runner)
        .clone()
        .route("gh", std::sync::Arc::new(Gh(open, merged, ran)));
    fx.cpd(&["pr", "merge", "acme/api#7"]).ok();
    assert_draft_cancelled_with_the_merge(&fx);
}

/// Guard: a PR with no draft changes nothing more than today.
#[test]
fn poll_merge_without_drafts_cancels_nothing() {
    let fx = Fx::with_ports();
    world(&fx, false);
    fx.fake_gh(search_merged());
    fx.cpd(&["pr", "poll"]).ok();
    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM workflows WHERE key = 'api'"),
        "merged"
    );
    assert_eq!(events(&fx, "pr-merged").len(), 1);
    assert!(events(&fx, "job-cancelled").is_empty());
}
