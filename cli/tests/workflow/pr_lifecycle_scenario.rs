//! PR lifecycle end-to-end scenario tests covering all 9 transitions
//! (`docs/architecture/pr-lifecycle-scenario.md`).
//!
//! T1: open -> CI fails
//! T2: CI fails -> Lead woken
//! T3: Lead woken -> push -> CI green
//! T4: CI green -> bot comments -> Lead woken
//! T5: Lead woken -> replies -> comments clear
//! T6: conflict -> Lead rebases (pro-d554)
//! T7: ready -> one Inbox item
//! T8: ready -> peer review requested -> approved
//! T9: merged -> closed

use cpd_core::clock::Clock;
use cpd_core::host::runner::Reply;
use cpd_core::inbox::model::Urgency;
use cpd_core::inbox::pr;
use serde_json::{Value, json};

use super::workflow::{Poll, Rows, node, pay, poll_fx};

fn page(nodes: Vec<Value>) -> Value {
    json!({"totalCount": nodes.len(), "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": nodes})
}

fn comment(id: &str, login: &str) -> Value {
    json!({"id": id, "author": {"login": login}, "body": "Please change this", "url": format!("https://github.com/acme/pay/pull/812#{id}"),
           "createdAt": "2026-10-20T09:00:00Z"})
}

fn thread(id: &str, login: &str) -> Value {
    let last = format!("{id}-c");
    json!({"id": id, "isResolved": false, "isOutdated": false, "path": "src/a.rs", "comments": page(vec![comment(&last, login)])})
}

fn answer(updated_at: &str, threads: Vec<Value>) -> String {
    let empty = page(vec![]);
    json!({"data": {"viewer": {"login": "me"}, "rateLimit": {"cost": 1, "remaining": 4900, "resetAt": "2026-10-20T11:00:00Z"},
        "p0": {"pullRequest": {"id": "PR_1", "updatedAt": updated_at, "reviewThreads": page(threads), "comments": empty, "reviews": empty}}}})
    .to_string()
}

fn fixture(posting: bool, posted: bool, checks: &str) -> Poll {
    let fx = poll_fx("deploy");
    let post = if posting {
        "[pr.post]\nenabled = true\nchannel = \"C02JU6C2Z1D\"\n"
    } else {
        "[pr.post]\nenabled = false\n"
    };
    fx.env.owner_policy(
        "acme",
        &format!("[pr]\ngate = \"local\"\n[pr.feedback]\nchecks = \"{checks}\"\n{post}"),
    );
    if posted {
        fx.store()
            .write("fixture", |tx| {
                tx.execute_batch(
                    "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, reaction, reacted_at)
                     VALUES ('pay', 'repo', 'C02JU6C2Z1D', '1793040197.490209', 'https://example.slack.com/p1', 'opened', NULL, NULL)",
                )
                .map_err(Into::into)
            })
            .unwrap();
    }
    fx
}

fn step_full(
    fx: &Poll,
    updated_at: &str,
    rollup: &str,
    review: Option<&str>,
    threads: Vec<Value>,
    mergeable: &str,
    merge_state_: &str,
) -> Value {
    let pull = node("acme/pay", 812, "OPEN", review, Some(rollup))
        .replace(
            r#""updatedAt":"U1""#,
            &format!(r#""updatedAt":"{updated_at}""#),
        )
        .replace(
            r#""mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN""#,
            &format!(r#""mergeable":"{mergeable}","mergeStateStatus":"{merge_state_}""#),
        )
        .replace(
            r#""isArchived":false"#,
            r#""isArchived":false,"defaultBranchRef":{"name":"main"}"#,
        )
        .replace(
            r#""isDraft":false,"#,
            r#""isDraft":false,"baseRefName":"main","headRefOid":"h1","#,
        );
    fx.search_raw(&format!(
        r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{pull}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}}}}"#
    ));
    *fx.gh.comments.lock().unwrap() = Some(answer(updated_at, threads));
    fx.poll()
}

fn step(
    fx: &Poll,
    updated_at: &str,
    rollup: &str,
    review: Option<&str>,
    threads: Vec<Value>,
) -> Value {
    step_full(
        fx,
        updated_at,
        rollup,
        review,
        threads,
        "MERGEABLE",
        "CLEAN",
    )
}

fn reasons(fx: &Poll) -> Vec<Vec<String>> {
    fx.events("pr-feedback")
        .iter()
        .map(|(_, data)| {
            data["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_str().unwrap().to_owned())
                .collect()
        })
        .collect()
}

#[test]
fn t1_open_ci_fails() {
    let fx = fixture(true, false, "any");
    step(&fx, "U1", "FAILURE", None, vec![]);
    let (checks, status): (String, String) = fx
        .store()
        .read(|c| {
            let checks: String = c.query_row(
                "SELECT checks FROM pull_requests WHERE number = 812",
                [],
                |r| r.get(0),
            )?;
            let status: String = c.query_row(
                "SELECT status FROM workflows WHERE plan = 'pay' AND key = 'repo'",
                [],
                |r| r.get(0),
            )?;
            Ok((checks, status))
        })
        .unwrap();
    assert_eq!(checks, "failed");
    assert_eq!(status, "open");
}

#[test]
fn t2_lead_woken_on_failing_check() {
    let fx = fixture(true, false, "any");
    step(&fx, "U1", "FAILURE", None, vec![]);
    assert_eq!(reasons(&fx), vec![vec!["checks_failed"]]);
    // The same failure seen again wakes nobody: told once per transition
    step(&fx, "U1", "FAILURE", None, vec![]);
    assert_eq!(reasons(&fx), vec![vec!["checks_failed"]]);
}

#[test]
fn t3_push_ci_green() {
    let fx = fixture(false, false, "any");
    // Start with failing checks
    step(&fx, "U1", "FAILURE", None, vec![]);
    assert_eq!(reasons(&fx), vec![vec!["checks_failed"]]);

    // Push resets checks to pending (contract §7.6 "The reset")
    fx.store()
        .write("push_reset", |tx| {
            tx.execute(
                "UPDATE pull_requests SET checks = 'pending', mergeable = 'unknown', merge_state = 'unknown' WHERE number = 812",
                [],
            )?;
            Ok::<(), cpd_core::error::CpdError>(())
        })
        .unwrap();

    let checks_pending: String = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT checks FROM pull_requests WHERE number = 812",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(checks_pending, "pending");

    // CI goes green (SUCCESS rollup)
    step(&fx, "U2", "SUCCESS", None, vec![]);

    let checks_green: String = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT checks FROM pull_requests WHERE number = 812",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(checks_green, "passed");

    // Green raises no pr-feedback (the Lead is not woken on green)
    assert_eq!(
        reasons(&fx),
        vec![vec!["checks_failed"]],
        "green raises no pr-feedback"
    );
}

#[test]
fn t4_bot_review_comments_wake_lead() {
    let fx = fixture(true, false, "any");
    // CI is green, but bot posted a comment
    step(
        &fx,
        "U1",
        "SUCCESS",
        None,
        vec![thread("T1", "review-bot[bot]")],
    );

    // Stored feedback holds the thread
    let feedback: Option<String> = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT feedback FROM pull_requests WHERE number = 812",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    let items: Vec<Value> = serde_json::from_str(&feedback.expect("feedback stored")).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], "T1");
    assert_eq!(items[0]["bot"], true);

    // Lead is woken with reason "comments"
    assert_eq!(reasons(&fx), vec![vec!["comments"]]);

    // Readiness check shows BotComments (P8): pr post is refused pr_not_ready with bot_comments
    let run = fx.cpd(&["pr", "post", "--workflow", "repo", "--plan", "pay"]);
    let err = crate::common::refused(&run, "pr_not_ready", 3);
    assert_eq!(err["details"]["reasons"], json!(["bot_comments"]));
}

#[test]
fn t5_replies_clear_comments() {
    let fx = fixture(true, false, "any");
    step(
        &fx,
        "U1",
        "SUCCESS",
        None,
        vec![thread("T1", "review-bot[bot]")],
    );

    let runner = (*fx.fakes.runner).clone();
    let has = |text: &'static str| {
        move |c: &cpd_core::host::runner::Call| c.args.join("\n").contains(text)
    };
    let _ = runner
        .on(
            "gh",
            has("addPullRequestReviewThreadReply"),
            Reply::ok(r#"{"data": {"addPullRequestReviewThreadReply": {"comment": {"id": "c1", "url": "https://github.com/acme/pay/pull/812#discussion_r1"}}}}"#),
        )
        .on(
            "gh",
            has("addComment"),
            Reply::ok(r#"{"data": {"addComment": {"commentEdge": {"node": {"id": "c2", "url": "https://github.com/acme/pay/pull/812#issuecomment-2"}}}}}"#),
        )
        .on(
            "gh",
            move |c: &cpd_core::host::runner::Call| {
                let joined = c.args.join("\n");
                joined.contains("pullRequest(number:") && joined.contains("{ id }")
            },
            Reply::ok(r#"{"data": {"repository": {"pullRequest": {"id": "PR_node"}}}}"#),
        );

    let reply_out = fx
        .cpd(&[
            "pr",
            "reply",
            "--workflow",
            "repo",
            "--plan",
            "pay",
            "--to",
            "T1",
            "--body",
            "Fixed in abc.",
        ])
        .ok();
    assert_eq!(reply_out["queued"], true);

    // Next successful poll without the thread keeps it out
    step(&fx, "U2", "SUCCESS", None, vec![]);

    // Ensure compare returns ahead so readiness condition holds
    let _ = (*fx.fakes.runner).clone().on(
        "gh",
        |c| c.args.iter().any(|a| a.contains("/compare/")),
        Reply::ok(r#"{"status":"ahead"}"#),
    );

    // Readiness clears when no bot item remains: now ready for user review
    let now = fx.fakes.clock.utc();
    let conditions = fx
        .store()
        .read(|c| pr::conditions(c, fx.home(), now))
        .unwrap();
    let ready_items: Vec<_> = conditions
        .iter()
        .filter(|c| c.subject == "pr:acme/pay#812")
        .collect();
    assert_eq!(ready_items.len(), 1);
    assert!(
        ready_items[0].label.contains("ready for user review"),
        "label: {}",
        ready_items[0].label
    );
}

#[test]
fn t6_conflict_with_main_queues_update() {
    let fx = fixture(false, false, "any");

    // Q3: Behind base without conflict queues nothing (contract §7.6 rule 3, pro-d554)
    step_full(&fx, "U1", "SUCCESS", None, vec![], "MERGEABLE", "BEHIND");
    let update_jobs: Vec<(String, String)> = fx
        .store()
        .read(|c| {
            let mut stmt =
                c.prepare("SELECT key, status FROM jobs WHERE plan = 'pay' AND kind = 'update'")?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert!(
        update_jobs.is_empty(),
        "behind base without conflict queues nothing"
    );

    // Q1, Q2: Conflicting queues an update Job
    let out = step_full(&fx, "U2", "SUCCESS", None, vec![], "CONFLICTING", "DIRTY");
    let update_jobs: Vec<(String, String)> = fx
        .store()
        .read(|c| {
            let mut stmt =
                c.prepare("SELECT key, status FROM jobs WHERE plan = 'pay' AND kind = 'update'")?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(update_jobs, [("up-repo".to_owned(), "queued".to_owned())]);
    assert_eq!(out["update_jobs"].as_array().unwrap().len(), 1);

    // Conflict is not a pr-feedback reason (pro-d518, G4)
    assert!(fx.events("pr-feedback").is_empty());

    // The Lead is told by a session message (poll.rs:1350)
    let lead_messages = fx.events("session-message");
    assert!(
        lead_messages
            .iter()
            .any(|(summary, _)| summary.contains("conflicting") && summary.contains("up-repo")),
        "lead was told via message: {lead_messages:?}"
    );

    // Q4: No Inbox item for conflict or behind-base
    let now = fx.fakes.clock.utc();
    let conditions = fx
        .store()
        .read(|c| pr::conditions(c, fx.home(), now))
        .unwrap();
    assert!(
        conditions.iter().all(|c| !c.label.contains("conflict")),
        "no inbox item for conflict"
    );
}

#[test]
fn t7_ready_raises_one_inbox_item() {
    let fx = fixture(true, false, "any");

    // Make compare return ahead
    let _ = (*fx.fakes.runner).clone().on(
        "gh",
        |c| c.args.iter().any(|a| a.contains("/compare/")),
        Reply::ok(r#"{"status":"ahead"}"#),
    );

    // Poll with green checks and empty feedback (first stores base/head, second compares)
    step(&fx, "U1", "SUCCESS", None, vec![]);
    step(&fx, "U1", "SUCCESS", None, vec![]);

    let now = fx.fakes.clock.utc();
    let conditions = fx
        .store()
        .read(|c| pr::conditions(c, fx.home(), now))
        .unwrap();

    let ready_items: Vec<_> = conditions
        .iter()
        .filter(|c| c.subject == "pr:acme/pay#812")
        .collect();
    assert_eq!(ready_items.len(), 1, "exactly one inbox item");
    assert_eq!(ready_items[0].urgency, Urgency::Yellow);
    assert!(
        ready_items[0].label.contains("ready for user review"),
        "label: {}",
        ready_items[0].label
    );

    // Q5: A transient compare failure does not overwrite base_contained with NULL
    let base_contained_before: Option<i64> = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT base_contained FROM pull_requests WHERE number = 812",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(base_contained_before, Some(1));

    // Force compare to fail
    let _ = (*fx.fakes.runner).clone().on(
        "gh",
        |c| c.args.iter().any(|a| a.contains("/compare/")),
        Reply::exit(1, "", "HTTP 502: bad gateway\n"),
    );

    // Poll again
    step(&fx, "U2", "SUCCESS", None, vec![]);

    let base_contained_after: Option<i64> = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT base_contained FROM pull_requests WHERE number = 812",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(
        base_contained_after,
        Some(1),
        "failed compare keeps stored value (Q5, b34)"
    );
}

#[test]
fn t8_peer_review_requested_and_approved() {
    let fx = fixture(true, true, "any"); // posted = true

    // Initial poll: posted, clean, green, no decision. Containment is not read (b79), so it is ready to merge at once.
    step(&fx, "U1", "SUCCESS", None, vec![]);

    let now = fx.fakes.clock.utc();
    let conditions = fx
        .store()
        .read(|c| pr::conditions(c, fx.home(), now))
        .unwrap();
    let pr_item = conditions
        .iter()
        .find(|c| c.subject == "pr:acme/pay#812")
        .expect("pr item exists");
    assert!(
        pr_item.label.contains("ready to merge"),
        "label: {}",
        pr_item.label
    );

    // Peer approves
    step(&fx, "U2", "SUCCESS", Some("APPROVED"), vec![]);

    // Peer approval raises one pr-feedback "approved" Event to the Lead
    assert_eq!(reasons(&fx), vec![vec!["approved"]]);

    // Item becomes "ready to merge" (yellow)
    let conditions = fx
        .store()
        .read(|c| pr::conditions(c, fx.home(), now))
        .unwrap();
    let pr_item = conditions
        .iter()
        .find(|c| c.subject == "pr:acme/pay#812")
        .expect("pr item exists");
    assert_eq!(pr_item.urgency, Urgency::Yellow);
    assert!(
        pr_item.label.contains("ready to merge"),
        "label: {}",
        pr_item.label
    );
}

#[test]
fn t9_merged_closes_workflow() {
    let fx = fixture(true, true, "any");

    // Open a pr-ready item in inbox_items
    fx.store()
        .write("inbox_item", |tx| {
            tx.execute(
                "INSERT INTO inbox_items (kind, urgency, subject, context, plan, label, opened_at)
                 VALUES ('pr-ready', 'yellow', 'pr:acme/pay#812', 'ctx', 'pay', 'pay#812 ready', 't')",
                [],
            )?;
            Ok::<(), cpd_core::error::CpdError>(())
        })
        .unwrap();

    // PR merges: search returns empty (no longer in open search), direct lookup returns MERGED
    fx.search(&[]);
    fx.lookup(&[Some(pay("MERGED", Some("APPROVED"), Some("SUCCESS")))]);

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(out["done"], json!(["pay"]));

    // Verify DB state
    let (pr_state, wf_status, plan_phase, open_inbox): (String, String, String, i64) = fx
        .store()
        .read(|c| {
            let pr_state: String = c.query_row(
                "SELECT state FROM pull_requests WHERE number = 812",
                [],
                |r| r.get(0),
            )?;
            let wf_status: String = c.query_row(
                "SELECT status FROM workflows WHERE plan = 'pay' AND key = 'repo'",
                [],
                |r| r.get(0),
            )?;
            let plan_phase: String = c.query_row(
                "SELECT phase FROM plans WHERE id = 'pay'",
                [],
                |r| r.get(0),
            )?;
            let open_inbox: i64 = c.query_row(
                "SELECT count(*) FROM inbox_items WHERE kind = 'pr-ready' AND subject = 'pr:acme/pay#812' AND closed_at IS NULL",
                [],
                |r| r.get(0),
            )?;
            Ok((pr_state, wf_status, plan_phase, open_inbox))
        })
        .unwrap();

    assert_eq!(pr_state, "merged");
    assert_eq!(wf_status, "merged");
    assert_eq!(plan_phase, "done");
    assert_eq!(open_inbox, 0, "pr-ready item closed on merge");
}
