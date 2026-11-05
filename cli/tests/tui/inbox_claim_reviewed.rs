//! The reviewed PR of a review Plan (commands.md §21.7 "The reviewed PR", contract §2.2 Plan `reviewed_pr`): `inbox
//! claim` stores it, `plan show` prints it. Mocks only: `Fx::with_ports`, `fake_gh`, the fake cockpit.

use crate::inbox::{Fixture, T0, condition};
use cpd_core::host::runner::Reply;
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use serde_json::{Value, json};
use std::fs;

const URL: &str = "https://github.com/acme/pay/pull/812";
const SUBJECT: &str = "pr:acme/pay#812";
const PLAN: &str = "r-pay-812";
const T5: &str = "2026-01-01T05:00:00.000Z";
const GH: &str = r#"{"data":{"viewer":{"login":"clayton-duarte"},"p0":{"pullRequest":{"title":"Pay the bills","isDraft":false,"state":"OPEN","reviewDecision":null,"author":{"login":"ryan"},"latestReviews":{"nodes":[]}}}}}"#;

fn sql(fx: &mut Fixture, statements: &str) {
    fx.store
        .write("fixture", |tx| {
            tx.execute_batch(statements)?;
            Ok(())
        })
        .unwrap();
}

/// A Fixture with an open review request for `acme/pay#812`, a `codereview` Context with an idle Manager, the post
/// in the team cache and the fake cockpit up. Returns the item's id.
fn claimable() -> (Fixture, i64) {
    let mut fx = Fixture::new();
    fx.fx.fake_gh(Reply::ok(GH));
    let socket = fx.fx.env.tmux_socket();
    fx.fx.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\nlead_tier = \"S4\"\nlead_runner = \"pi\"\n[tmux]\nsocket = \"{socket}\"\n"
    ));
    sql(
        &mut fx,
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('codereview', 'Review', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at, archived_at)
         VALUES ('mgr-rc', 'manager', 'codereview', NULL, NULL, 'codereview-manager', '/sessions/mgr-rc.jsonl', '/repo', 'O4', NULL, 'idle', NULL,
                 '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL);",
    );
    let path = fx.fx.env.cpd_home().join("run/team-review.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let post = json!({"repo": "acme/pay", "number": 812, "ts": 1_700_000_000.0, "ts_raw": "1700000000.000100", "author": "Ryan Case",
                      "primary": true, "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false});
    let cache = json!({"version": 1, "polled_at": T0, "channel": "C02JU6C2Z1D", "read": 10, "posts": [post]});
    fs::write(path, cache.to_string()).unwrap();
    let mut all = vec![
        condition(Kind::JobStuck, Urgency::Red, "stuck", "ctx1"),
        condition(Kind::Dialog, Urgency::White, "dialog", "ctx1"),
        condition(Kind::PlanReview, Urgency::Yellow, "review", "ctx2"),
        condition(Kind::Unread, Urgency::Magenta, "unread", "ctx1"),
        condition(Kind::Question, Urgency::Yellow, "question", "ctx1"),
    ];
    all.push(Condition {
        kind: Kind::ReviewRequested,
        urgency: Urgency::Blue,
        subject: SUBJECT.to_owned(),
        label: "acme/pay#812 review requested".to_owned(),
        url: Some(URL.to_owned()),
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
    });
    reconcile(&mut fx.store, &all, T5).unwrap();
    let id = fx.store_count(&format!(
        "SELECT id FROM inbox_items WHERE subject = '{SUBJECT}'"
    ));
    fx.fx.cockpit();
    (fx, id)
}

/// `plan show` of `plan`, the whole `plan` object.
fn shown(fx: &Fixture, plan: &str) -> Value {
    fx.cpd(&["plan", "show", "--plan", plan]).ok()["plan"].clone()
}

fn pull_request(state: &str) -> String {
    format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
         VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, '{state}', 'none', 'none', 't', NULL, 't');"
    )
}

#[test]
fn claim_stores_the_reviewed_pr_on_the_review_plan() {
    let (mut fx, id) = claimable();
    sql(&mut fx, &pull_request("open"));
    fx.ok(&["claim", &id.to_string()]);
    assert_eq!(
        shown(&fx, PLAN)["reviewed_pr"],
        json!({"repo": "acme/pay", "number": 812, "url": URL, "title": "Pay the bills", "state": "open"})
    );
}

#[test]
fn plan_show_of_an_ordinary_plan_has_a_null_reviewed_pr() {
    let mut fx = Fixture::new();
    sql(
        &mut fx,
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-pay', 'lead', 'ctx1', 'pay', NULL, 'pay', '/repo', 'O2', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay', 'ctx1', 'Pay', 'grounding', 'lead-pay', 't');",
    );
    let plan = shown(&fx, "pay");
    assert_eq!(plan.get("reviewed_pr"), Some(&Value::Null), "{plan}");
}

#[test]
fn reviewed_pr_state_follows_the_pull_requests_row() {
    let (mut fx, id) = claimable();
    fx.ok(&["claim", &id.to_string()]);
    let state = |fx: &Fixture| shown(fx, PLAN)["reviewed_pr"]["state"].clone();
    assert!(
        shown(&fx, PLAN)["reviewed_pr"].is_object(),
        "claimed Plan has a reviewed_pr"
    );
    assert_eq!(state(&fx), Value::Null, "no row, no state");
    sql(&mut fx, &pull_request("open"));
    assert_eq!(state(&fx), json!("open"));
    sql(
        &mut fx,
        "UPDATE pull_requests SET state = 'merged', merged_at = 't' WHERE repo = 'acme/pay' AND number = 812;",
    );
    assert_eq!(state(&fx), json!("merged"));
}
