//! E20's message (contract §3.7 E20, hs-d731): the keys are sorted and the text begins with the record [`prefix`] names.

use super::parked::text;
use cpd_core::ids::PlanId;
use cpd_core::plan::parked_wake::prefix;

#[test]
fn the_message_lists_the_keys_sorted_after_the_prefix() {
    let plan = PlanId::parse("pay").unwrap();
    let message = text(&plan, &["zeta".to_owned(), "alpha".to_owned()]);
    assert!(
        message.starts_with(&format!(
            "{}alpha, zeta (no pending Plan review, no unfinished need).",
            prefix(&plan)
        )),
        "{message}"
    );
    assert!(message.ends_with("Nothing runs it otherwise."), "{message}");
}

use super::parked;
use super::testkit::Fx;

fn draft(fx: &mut Fx) {
    fx.fixture("INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at) VALUES ('pay', 'repo', 'draft', 'Draft', 'S2', 'Goal', 'Context', '[\"file\"]', '[\"works\"]', '[]', 'draft', 0, cpd_txn(), 't');");
}

#[test]
fn archived_draft_and_related_records_survive_repeated_ticks() {
    let mut fx = Fx::with_plan("");
    draft(&mut fx);
    fx.fixture(&format!("INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('pay', 'r1', 1, '{}', 'pending', 't'); UPDATE plans SET archived_at = 't' WHERE id = 'pay'; UPDATE sessions SET state = 'idle' WHERE id = 'lead1';", "a".repeat(64)));
    let history: i64 = fx.query("SELECT count(*) FROM history");
    for _ in 0..3 {
        parked::run(&mut fx.engine);
    }
    assert_eq!(
        fx.query::<String>("SELECT status FROM jobs WHERE key = 'draft'"),
        "draft"
    );
    assert_eq!(
        fx.query::<String>(
            "SELECT goal || context || touches || acceptance FROM jobs WHERE key = 'draft'"
        ),
        "GoalContext[\"file\"][\"works\"]"
    );
    assert_eq!(
        fx.query::<String>("SELECT status FROM workflows WHERE plan = 'pay' AND key = 'repo'"),
        "dispatch"
    );
    assert_eq!(
        fx.query::<String>("SELECT status FROM plan_reviews WHERE plan = 'pay' AND id = 'r1'"),
        "pending"
    );
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM history"), history);
    assert_eq!(
        fx.query::<i64>(
            "SELECT count(*) FROM events WHERE kind IN ('job-cancelled', 'session-message')"
        ),
        0
    );
}

#[test]
fn active_parked_draft_warns_once_and_remains_a_draft() {
    let mut fx = Fx::with_plan("");
    draft(&mut fx);
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    for _ in 0..3 {
        parked::run(&mut fx.engine);
    }
    assert_eq!(
        fx.query::<String>("SELECT status FROM jobs WHERE key = 'draft'"),
        "draft"
    );
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM events WHERE kind = 'session-message' AND session = 'lead1' AND json_extract(data, '$.from') IS NULL AND json_extract(data, '$.wake') = 1"), 1);
}
