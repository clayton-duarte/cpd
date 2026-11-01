//! `plan archive` and `context archive` refused while a Plan has a `draft` Job (commands.md §4.10, contract §7.6, hs-d734), and
//! the pins that `pr cancel` and `plan cancel` still cancel drafts (hs-d573), in this process (`Fx::with_ports`).

use crate::common;

use common::fx::Fx;
use common::refused;
use serde_json::{Value, json};

/// Plan `pay` (phase `plan`, with its Lead) in Context `ctx`, one PR `api` and one full draft Job `d`.
fn world(fx: &Fx) {
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.cpd(&[
        "plan",
        "new",
        "pay",
        "--context",
        "ctx",
        "--title",
        "Plan pay",
    ])
    .ok();
    let repo = fx.keyed_repo("api");
    fx.cpd(&["workflow", "add", "api", "--repo", repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pay"]).ok();
    for key in ["api-config", "open-api"] {
        fx.cpd(&[
            "job",
            "cancel",
            key,
            "--reason",
            "a stub of the template",
            "--plan",
            "pay",
        ])
        .ok();
    }
    fx.env.sql(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
         VALUES ('pay', 'api', 'd', 'Job d', 'S2', 'Goal.', 'Context.', '[\"src/d.rs\"]', '[\"cargo test\"]', '[]', 'draft', 0, cpd_txn(), 't');",
    );
}

fn archived(fx: &Fx) -> i64 {
    fx.env.query("SELECT count(*) FROM (SELECT archived_at FROM contexts UNION ALL SELECT archived_at FROM plans) WHERE archived_at IS NOT NULL")
}

fn event_count(fx: &Fx) -> i64 {
    fx.env.query("SELECT count(*) FROM events")
}

fn assert_draft_blocker(error: &Value) {
    let blockers = error["details"]["blockers"].as_array().unwrap();
    assert_eq!(blockers.len(), 1, "{error}");
    assert_eq!(
        blockers[0],
        json!({"kind": "job", "id": "pay/d", "reason": "draft"}),
        "{error}"
    );
}

#[test]
fn archive_with_draft_is_refused() {
    let fx = Fx::with_ports();
    world(&fx);
    let before = event_count(&fx);
    let error = refused(
        &fx.cpd(&["plan", "archive", "--plan", "pay"]),
        "archive_blocked",
        3,
    );
    assert_draft_blocker(&error);
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("plan cancel") && message.contains("job cancel"),
        "{message}"
    );
    assert_eq!(
        (archived(&fx), event_count(&fx)),
        (0, before),
        "a refusal writes nothing"
    );

    // Guard: once the draft is cancelled the archive passes.
    fx.cpd(&["job", "cancel", "d", "--reason", "No.", "--plan", "pay"])
        .ok();
    fx.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"),
        1
    );
}

#[test]
fn context_archive_with_draft_is_refused() {
    let fx = Fx::with_ports();
    world(&fx);
    let before = event_count(&fx);
    let error = refused(
        &fx.cpd(&["context", "archive", "ctx"]),
        "archive_blocked",
        3,
    );
    assert!(
        error["details"]["blockers"]
            .as_array()
            .unwrap()
            .contains(&json!({"kind": "job", "id": "pay/d", "reason": "draft"})),
        "{error}"
    );
    assert_eq!(
        (archived(&fx), event_count(&fx)),
        (0, before),
        "a refusal writes nothing"
    );

    fx.cpd(&["job", "cancel", "d", "--reason", "No.", "--plan", "pay"])
        .ok();
    fx.cpd(&["context", "archive", "ctx"]).ok();
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM contexts WHERE archived_at IS NOT NULL"),
        1
    );
}

/// Guard (hs-d573, passes today): `plan cancel` cancels the Plan's drafts.
#[test]
fn plan_cancel_still_cancels_drafts() {
    let fx = Fx::with_ports();
    world(&fx);
    fx.cpd(&["plan", "cancel", "--plan", "pay"]).ok();
    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM jobs WHERE key = 'd'"),
        "cancelled"
    );
}
