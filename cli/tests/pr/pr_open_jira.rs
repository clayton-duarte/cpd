//! `pr open` and the Jira ticket of a PR (commands.md §9.1, Plan pr-ticket): when the owner's policy sets `pr.jira_required`, a PR with
//! no `jira` and no `head_ref` is refused `jira_required`; a PR with a ticket, an adopted PR and a policy that leaves the key false open.
//! In-process over the fake ports and the fake `gh` of `pr.rs` ([`Poll`]).

use serde_json::json;

use super::pr::{Poll, Rows, code, open_jobs, poll_fx};

/// A PR `repo` in `dispatch`, described, whose owner's policy holds `policy` in its `[pr]` table.
fn fx_with(policy: &str) -> Poll {
    let fx = poll_fx("dispatch");
    fx.env
        .owner_policy("acme", &format!("[pr]\ngate = \"local\"\n{policy}"));
    fx.store()
        .write("fixture", |tx| {
            Ok(tx.execute_batch("UPDATE prs SET description = 'A description'")?)
        })
        .unwrap();
    fx
}

fn open(fx: &Poll) -> crate::common::Run {
    fx.cpd(&["pr", "open", "--pr", "repo", "--plan", "pay"])
}

#[test]
fn pr_open_refuses_jira_required_without_a_ticket() {
    let fx = fx_with("jira_required = true\n");
    let (txns, remote) = (fx.txns(), fx.remote_branch("cpd/pay"));

    let run = open(&fx);

    assert_eq!(code(&run), ("jira_required".into(), 3));
    let error = run.error();
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("cpd pr edit repo --jira"),
        "{error}"
    );
    assert_eq!(error["details"], json!({"plan": "pay", "pr": "repo"}));
    assert_eq!(
        (fx.plan_row().0, fx.txns(), open_jobs(&fx)),
        ("dispatch".to_owned(), txns, Vec::<(String, String)>::new())
    );
    assert_eq!(fx.remote_branch("cpd/pay"), remote, "nothing pushed");
    assert!(
        fx.gh_calls().is_empty(),
        "no pull request created, no gh call"
    );
}

#[test]
fn pr_open_opens_the_pull_request_of_a_pr_with_a_jira_key_when_it_is_required() {
    let fx = fx_with("jira_required = true\n");
    fx.store()
        .write("fixture", |tx| {
            Ok(tx.execute_batch("UPDATE prs SET jira = 'ABC-1'")?)
        })
        .unwrap();

    open(&fx).ok();

    assert_eq!(open_jobs(&fx).len(), 1, "the open Job is queued");
}

/// Guard: a policy that leaves the key false opens a PR with no ticket.
#[test]
fn pr_open_opens_a_pr_with_no_ticket_when_the_policy_does_not_require_one() {
    let fx = fx_with("jira_required = false\n");
    open(&fx).ok();
    assert_eq!(open_jobs(&fx).len(), 1, "the open Job is queued");
}

/// Guard: a policy without the key is the same as false.
#[test]
fn pr_open_opens_a_pr_with_no_ticket_when_the_policy_has_no_key() {
    let fx = fx_with("");
    open(&fx).ok();
    assert_eq!(open_jobs(&fx).len(), 1, "the open Job is queued");
}

/// Guard: an adopted PR (`head_ref` set) needs no ticket. `head_ref` is fixed by a trigger: lifted for the update, put back as the migration has it.
#[test]
fn pr_open_opens_an_adopted_pr_with_no_ticket_when_required() {
    let fx = fx_with("jira_required = true\n");
    fx.store()
        .write("fixture", |tx| {
            Ok(tx.execute_batch(
                "DROP TRIGGER prs_head_ref_fixed; UPDATE prs SET head_ref = 'feature/x';
                 CREATE TRIGGER prs_head_ref_fixed BEFORE UPDATE OF head_ref ON prs\nBEGIN SELECT RAISE(ABORT, 'pr_locked'); END;",
            )?)
        })
        .unwrap();
    open(&fx).ok();
    assert_eq!(open_jobs(&fx).len(), 1, "the open Job is queued");
}
