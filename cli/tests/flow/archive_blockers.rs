//! The complete archive rule (commands.md §3.9, §4.10, §10.8, §10.10; decision pro-d358): `plan archive` and `context
//! archive` are refused unless the covered Plans are done, their Workflows merged or cancelled, no Job is
//! draft/queued/running/failed, and no covered Session has an open, undismissed Inbox item. In this process (`Fx::with_ports`).

use crate::common;

use common::fx::Fx;
use common::refused;
use serde_json::{Value, json};

/// A finished Plan `pay` in Context `ctx`: phase `done`, its Lead `lead-pay`, Workflow `api` merged, one Job
/// `build` that is `merged`, no Escalation, no review, no Inbox item. `phase`, `workflow` and `job` are what a test breaks.
fn world(fx: &Fx, phase: &str, workflow: &str, job: &str) {
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    let done = if phase == "done" { "'t'" } else { "NULL" };
    let locked = if matches!(phase, "grounding" | "plan") {
        "NULL"
    } else {
        "'t'"
    };
    fx.env.sql(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-pay', 'lead', 'ctx', 'pay', 'Lead pay', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Plan pay', '{phase}', 'lead-pay', 't', {locked}, {done});
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'Add api', 0, '{pr_state}', 'none', 'none', 't', {merged_at}, 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                                created_txn, created_at, started_at, merged_at)
           VALUES ('pay', 'api', '/r/api', 'main', '{workflow}', 'cpd/pay/api', '/w', 7, 'https://github.com/acme/api/pull/7', 't',
                   (SELECT id FROM pull_requests WHERE number = 7), cpd_txn(), 't', 't', {merged_at});
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           merged_sha, failed_cause, created_txn, created_at)
           VALUES ('pay', 'api', 'build', 'Job build', 'S2', 'Goal.', 'Context.', '[\"src/d.rs\"]', '[\"cargo test\"]', '[]', '{job}', 0, '{fp}',
                   iif('{job}' = 'merged', 'abc123', NULL), iif('{job}' = 'failed', 'check_failed', NULL), cpd_txn(), 't');",
        merged_at = if workflow == "merged" { "'t'" } else { "NULL" },
        pr_state = if workflow == "merged" { "merged" } else { "open" },
        fp = "a".repeat(64),
    ));
}

fn finished(fx: &Fx) {
    world(fx, "done", "merged", "merged");
}

/// An open Inbox item of the Lead, `snoozed` or not; returns its id.
fn inbox(fx: &Fx, closed: bool, dismissed: bool, snoozed: bool) -> i64 {
    let at = |on: bool| {
        if on {
            "'2999-01-01T00:00:00.000Z'"
        } else {
            "NULL"
        }
    };
    fx.env.sql(&format!(
        "INSERT INTO inbox_items (kind, urgency, subject, context, plan, session, label, opened_at, closed_at, snoozed_until, dismissed_at)
         VALUES ('unread', 'blue', 'Lead pay waits', 'ctx', 'pay', 'lead-pay', 'Lead pay', 't', {}, {}, {});",
        at(closed),
        at(snoozed),
        at(dismissed)
    ));
    fx.env
        .query("SELECT id FROM inbox_items ORDER BY id DESC LIMIT 1")
}

fn archived(fx: &Fx) -> i64 {
    fx.env.query("SELECT count(*) FROM (SELECT archived_at FROM contexts UNION ALL SELECT archived_at FROM plans UNION ALL SELECT archived_at FROM sessions) WHERE archived_at IS NOT NULL")
}

/// `args` is refused with `archive_blocked` (exit 3) naming `expected` among its blockers, and archives nothing.
fn assert_refused(fx: &Fx, args: &[&str], expected: Value) {
    let before = archived(fx);
    let error = refused(&fx.cpd(args), "archive_blocked", 3);
    let blockers = error["details"]["blockers"].as_array().unwrap();
    assert!(blockers.contains(&expected), "{expected} not in {error}");
    assert_eq!(archived(fx), before, "a refusal archives nothing");
}

#[test]
fn plan_archive_is_refused_while_the_plan_is_not_done() {
    let fx = Fx::with_ports();
    world(&fx, "grounding", "merged", "merged");
    assert_refused(
        &fx,
        &["plan", "archive", "--plan", "pay"],
        json!({"kind": "plan", "id": "pay", "plan": "pay", "reason": "Plan pay is in phase grounding (not done)."}),
    );
}

#[test]
fn plan_archive_is_refused_while_a_workflow_is_not_merged_or_cancelled() {
    let fx = Fx::with_ports();
    world(&fx, "done", "open", "merged");
    assert_refused(
        &fx,
        &["plan", "archive", "--plan", "pay"],
        json!({"kind": "workflow", "id": "api", "plan": "pay", "reason": "Workflow api of Plan pay is open."}),
    );
}

#[test]
fn plan_archive_is_refused_while_a_job_is_failed() {
    let fx = Fx::with_ports();
    world(&fx, "done", "merged", "failed");
    assert_refused(
        &fx,
        &["plan", "archive", "--plan", "pay"],
        json!({"kind": "job", "id": "build", "plan": "pay", "reason": "Job build of Plan pay is failed."}),
    );
}

#[test]
fn plan_archive_is_refused_while_the_lead_has_an_open_inbox_item() {
    let fx = Fx::with_ports();
    finished(&fx);
    let id = inbox(&fx, false, false, false);
    assert_refused(
        &fx,
        &["plan", "archive", "--plan", "pay"],
        json!({"kind": "inbox", "id": id.to_string(), "reason": format!("Session Lead pay has open Inbox item {id} (unread).")}),
    );
}

#[test]
fn a_snoozed_open_inbox_item_still_blocks_plan_archive() {
    let fx = Fx::with_ports();
    finished(&fx);
    let id = inbox(&fx, false, false, true);
    assert_refused(
        &fx,
        &["plan", "archive", "--plan", "pay"],
        json!({"kind": "inbox", "id": id.to_string(), "reason": format!("Session Lead pay has open Inbox item {id} (unread).")}),
    );
}

#[test]
fn context_archive_is_refused_while_a_plan_of_it_is_not_done() {
    let fx = Fx::with_ports();
    world(&fx, "grounding", "merged", "merged");
    assert_refused(
        &fx,
        &["context", "archive", "ctx"],
        json!({"kind": "plan", "id": "pay", "plan": "pay", "reason": "Plan pay is in phase grounding (not done)."}),
    );
}

/// Guard (passes today and after): a finished Plan with only dismissed and closed Inbox items archives.
#[test]
fn a_finished_plan_archives() {
    let fx = Fx::with_ports();
    finished(&fx);
    inbox(&fx, false, true, false);
    inbox(&fx, true, false, false);
    fx.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"),
        1
    );
}

/// Guard (passes today and after): `plan delete` keeps its own rules, and an archived Plan in phase `grounding` is
/// not refused for its phase.
#[test]
fn plan_delete_ignores_the_phase_and_inbox_rules() {
    let fx = Fx::with_ports();
    world(&fx, "grounding", "merged", "merged");
    fx.env.sql(
        "UPDATE sessions SET archived_at = '2000-01-01T00:00:00.000Z' WHERE id = 'lead-pay';
         UPDATE plans SET archived_at = '2000-01-01T00:00:00.000Z' WHERE id = 'pay';",
    );
    fx.cpd(&["plan", "delete", "pay", "--yes"]).ok();
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM plans WHERE id = 'pay' AND deleted_at IS NOT NULL"),
        1
    );
}
