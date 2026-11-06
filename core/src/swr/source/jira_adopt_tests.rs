//! The ticketless rows of the `jira` source (swr-jira.md §7): a Workflow with no ticket queues rows that wait with no key,
//! and setting the ticket queues them again under it.

use super::jira;
use crate::clock::{Clock, ManualClock};
use crate::swr::outbox;
use crate::swr::outbox_tests::store;

const FIXTURE: &str = "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
       VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
     INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
       VALUES ('pay', 'api', '/nowhere', 'main', 'dispatch', 'cpd/pay/api', '/nowhere/api', cpd_txn(), 't', 't');";

#[test]
fn a_ticketless_row_waits_and_is_queued_again_under_the_ticket_when_it_is_set() {
    let (_dir, mut store) = store();
    let now = ManualClock::new().utc();
    store
        .write("fixture", |tx| {
            tx.execute_batch(FIXTURE)?;
            Ok::<(), crate::error::CpdError>(())
        })
        .unwrap();
    store
        .write("sites", |tx| {
            jira::enqueue_for_workflow(tx, "pay", "api", "in_progress")?;
            jira::enqueue_for_workflow(tx, "pay", "api", "in_progress")?;
            jira::enqueue_for_workflow(tx, "pay", "api", "in_review")
        })
        .unwrap();
    assert!(
        outbox::claim_next(&mut store, now, "jira")
            .unwrap()
            .is_none(),
        "a row with no ticket is never claimed"
    );

    store
        .write("ticket", |tx| {
            tx.execute("UPDATE workflows SET jira = 'ABC-1' WHERE key = 'api'", [])?;
            jira::adopt_ticket(tx, "pay", "api", "ABC-1")
        })
        .unwrap();
    let claimed = outbox::claim_next(&mut store, now, "jira")
        .unwrap()
        .unwrap();
    assert_eq!(claimed.target_key, "jira:issue:ABC-1");
    assert!(claimed.vars_json.contains("in_review"), "{claimed:?}");
    assert!(
        outbox::claim_next(&mut store, now, "jira")
            .unwrap()
            .is_none(),
        "the lower rank was superseded by the forward rule: one row runs"
    );
}

const PR: &str = "INSERT INTO pull_requests (id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, mergeable, merge_state, authored, review_requested)
       VALUES (7, 'o/r', 7, 'https://example/7', 'Ship', 0, 'open', 'none', 'none', 't', NULL, 't', 'unknown', 'unknown', 0, 0);";
const OPEN: &str = "UPDATE workflows SET status = 'open', pr_number = 7, pr_url = 'https://example/7', pr_opened_at = 't', pull_request = 7 WHERE key = 'api';";
const MERGED: &str = "UPDATE workflows SET status = 'merged', merged_at = 't' WHERE key = 'api';";
/// Marker: the fixture's Workflow is `waiting` (it cannot go back from `dispatch`, so the row is built so).
const WAITING: &str = "waiting";

/// A Job of Workflow `api` (`key`), with an Attempt of `attempt` status when given.
fn job(key: &str, status: &str, attempt: Option<&str>) -> String {
    let fingerprint = "f".repeat(64);
    let mut sql = format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
           VALUES ('pay', 'api', '{key}', '{key}', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, '{fingerprint}', cpd_txn(), 't');"
    );
    if let Some(attempt) = attempt {
        sql.push_str(&format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
               VALUES ('pay', '{key}-a1', '{key}', 1, 'S2', 'ladder', '{attempt}', 'cpd/pay--{key}-a1', '/w', 't');"
        ));
    }
    sql
}

/// The Workflow `api` after `setup`, then the ticket set and `adopt_ticket` run `times` times; the targets of its pending
/// rows under the ticket, in rank order.
fn adopted(setup: &str, times: usize) -> Vec<String> {
    adopted_after(setup, "", times)
}

fn adopted_after(setup: &str, queued: &str, times: usize) -> Vec<String> {
    let (_dir, mut store) = store();
    store
        .write("fixture", |tx| {
            if setup == WAITING {
                tx.execute_batch(&format!(
                    "{}{PR}",
                    FIXTURE
                        .replace(
                            "'dispatch', 'cpd/pay/api', '/nowhere/api'",
                            "'waiting', 'cpd/pay/api', NULL"
                        )
                        .replace(", cpd_txn(), 't', 't');", ", cpd_txn(), 't', NULL);")
                ))?;
            } else {
                tx.execute_batch(&format!("{FIXTURE}{PR}{setup}"))?;
            }
            Ok::<(), crate::error::CpdError>(())
        })
        .unwrap();
    if !queued.is_empty() {
        store
            .write("queued", |tx| {
                jira::enqueue_for_workflow(tx, "pay", "api", queued)
            })
            .unwrap();
    }
    for _ in 0..times {
        store
            .write("ticket", |tx| {
                tx.execute("UPDATE workflows SET jira = 'ABC-1' WHERE key = 'api'", [])?;
                jira::adopt_ticket(tx, "pay", "api", "ABC-1")
            })
            .unwrap();
    }
    store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT json_extract(vars_json, '$.target') FROM swr_mutations
                 WHERE system = 'jira' AND status = 'pending' AND target_key = 'jira:issue:ABC-1'
                 ORDER BY json_extract(vars_json, '$.rank'), id",
            )?;
            Ok(stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap()
}

#[test]
fn adopt_ticket_enqueues_in_progress_for_a_dispatch_workflow_with_a_started_job() {
    assert_eq!(
        adopted(&job("a", "running", Some("running")), 1),
        ["in_progress"]
    );
}

#[test]
fn adopt_ticket_enqueues_in_review_for_an_open_workflow() {
    assert_eq!(adopted(OPEN, 1), ["in_review"]);
}

#[test]
fn adopt_ticket_enqueues_done_for_a_merged_workflow() {
    assert_eq!(adopted(&format!("{OPEN}{MERGED}"), 1), ["done"]);
}

#[test]
fn adopt_ticket_enqueues_nothing_for_a_waiting_workflow() {
    assert!(adopted(WAITING, 1).is_empty());
}

#[test]
fn adopt_ticket_enqueues_nothing_for_a_dispatch_workflow_with_no_started_job() {
    assert!(adopted(&job("a", "queued", None), 1).is_empty());
}

#[test]
fn adopt_ticket_does_not_enqueue_a_target_that_is_already_covered() {
    assert!(adopted(OPEN, 2).len() <= 1, "twice: no duplicate");
    assert_eq!(
        adopted_after(OPEN, "done", 1),
        ["done"],
        "a pending higher rank covers in_review"
    );
}
