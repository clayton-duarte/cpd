//! Red tests (bug-guard-tests-a1) for engine step E25, the bug protocol check (contract §3.7 E25,
//! §2.2 "Bug record", "The wake and the breach check"; air-d104, air-d105, air-d106, air-n120; Plan
//! `protocol-enforce`, Workflow `bug-guard`): every tick E25 reads the open `bug_records` rows and
//!   - wakes the owner once per bug, ever: a waking `session-message` (`from` null, `wake` true)
//!     whose text begins `Bug <id> is open`, so a sleeping owner is resumed;
//!   - appends one `bug-breach` Event when a bug has no mitigation past the 15-minute bound
//!     (`unmitigated_past_bound`) or when a new open bug's normalized symptom equals an open bug's
//!     (`recurring_symptom`), and, in the same transaction, one waking `session-message` whose text
//!     begins `Bug <id> breached`;
//!   - the User's alert is the bug's `bug` Inbox item, which E8's `conditions` rule keeps open while
//!     the bug is unmitigated and closes when it is mitigated.
//!
//! The notification layer is mocked: the testkit's tmux runs no program, `tell` writes the
//! `session-message` Event to the temporary store, and the `bug` Inbox item is E8's row in that
//! store. No real session or Inbox is touched. Red until `bug-guard-impl` implements the step.

use serde_json::Value;

use super::bug_guard;
use super::inbox;
use super::testkit::{Fx, ago};

/// The mitigation bound of contract §2.2: 900 s (15 minutes), a code constant.
const BOUND: i64 = 900;

/// One open bug record `id`: the Plan it names (`None`: a cross-Plan bug), its symptom, reported
/// `minutes` ago, with no mitigation.
fn bug(fx: &mut Fx, id: &str, plan: Option<&str>, symptom: &str, minutes: i64) {
    let plan = plan.map_or_else(|| "NULL".to_owned(), |p| format!("'{p}'"));
    fx.fixture(&format!(
        "INSERT INTO bug_records (id, plan, title, symptom, status, reported_at)
           VALUES ('{id}', {plan}, 'Title of {id}', '{symptom}', 'open', '{}');",
        ago(minutes)
    ));
}

/// The `session-message` Events, oldest first: `(session, from, wake, text)`.
fn messages(fx: &mut Fx) -> Vec<(String, Option<String>, bool, String)> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT session, json_extract(data, '$.from'), json_extract(data, '$.wake'), json_extract(data, '$.text')
                   FROM events WHERE kind = 'session-message' ORDER BY id",
            )?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// The messages whose text begins with `prefix`, as `(session, from, wake, text)`.
fn messages_starting(fx: &mut Fx, prefix: &str) -> Vec<(String, Option<String>, bool, String)> {
    messages(fx)
        .into_iter()
        .filter(|m| m.3.starts_with(prefix))
        .collect()
}

/// The `data` of the `bug-breach` Event for bug `id`, when one exists.
fn breach(fx: &mut Fx, id: &str) -> Option<Value> {
    fx.events("bug-breach")
        .into_iter()
        .map(|e| e.3)
        .find(|d| d["bug"] == id)
}

/// The open `bug` Inbox item, as `(subject, session)`.
fn bug_item(fx: &mut Fx) -> Option<(String, Option<String>)> {
    fx.engine
        .store
        .read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT subject, session FROM inbox_items WHERE kind = 'bug' AND closed_at IS NULL ORDER BY id LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok())
        })
        .unwrap()
}

/// Plan `pay`, its Lead `lead1` idle, and `[inbox]` config for the E8 reconcile.
fn fx() -> Fx {
    let mut fx = Fx::with_plan("[inbox]\nstuck_job_minutes = 1\n");
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    fx
}

/// A bug past the 15-minute bound with `mitigation` null breaches: one `bug-breach` Event
/// (`unmitigated_past_bound`) and one waking `session-message` to the owner.
#[test]
fn alerts_when_a_bug_has_no_mitigation_past_the_bound() {
    let mut fx = fx();
    bug(&mut fx, "b1", Some("pay"), "Payment fails at checkout", 16);

    bug_guard::run(&mut fx.engine);

    let data = breach(&mut fx, "b1").expect("a bug past the bound breaches");
    assert_eq!(data["reason"], "unmitigated_past_bound");
    assert_eq!(data["symptom"], "Payment fails at checkout");
    assert_eq!(data["bound"].as_i64(), Some(BOUND));
    assert_eq!(
        fx.events("bug-breach")[0].0.as_deref(),
        Some("pay"),
        "the Event carries the bug's Plan"
    );

    let sent = messages_starting(&mut fx, "Bug b1 breached");
    assert_eq!(sent.len(), 1, "{:?}", messages(&mut fx));
    assert_eq!(sent[0].0, "lead1");
    assert_eq!(sent[0].1, None, "the engine is the sender");
    assert!(sent[0].2, "the alert wakes the owner");

    // The User's alert: the bug's Inbox item, opened by E8's rule.
    inbox::run(&mut fx.engine);
    assert_eq!(
        bug_item(&mut fx).as_ref().map(|i| i.0.as_str()),
        Some("bug:b1")
    );
}

/// A new open bug whose normalized symptom equals an open bug's breaches as a recurrence: one
/// `bug-breach` Event (`recurring_symptom`) for the new record, none for the first.
#[test]
fn alerts_on_a_recurring_normalized_symptom() {
    let mut fx = fx();
    bug(&mut fx, "b1", Some("pay"), "Payment fails at checkout", 10);
    bug(&mut fx, "b2", Some("pay"), "payment fails at CHECKOUT!!", 1);

    bug_guard::run(&mut fx.engine);

    let data = breach(&mut fx, "b2").expect("the recurrence breaches");
    assert_eq!(data["reason"], "recurring_symptom");
    assert_eq!(data["bound"].as_i64(), Some(BOUND));
    assert!(
        breach(&mut fx, "b1").is_none(),
        "the first bug is not a recurrence"
    );
}

/// A bug reported inside the 15-minute bound with `mitigation` null breaches nothing: no
/// `bug-breach` Event and no breach alert (the open wake is not a breach).
#[test]
fn stays_silent_below_the_bound() {
    let mut fx = fx();
    bug(&mut fx, "b1", Some("pay"), "Payment fails at checkout", 5);

    bug_guard::run(&mut fx.engine);

    // The new record still wakes its owner (the open wake), but breaches nothing.
    assert_eq!(
        messages_starting(&mut fx, "Bug b1 is open").len(),
        1,
        "{:?}",
        messages(&mut fx)
    );
    assert_eq!(fx.count("bug-breach"), 0, "{:?}", fx.events("bug-breach"));
    assert!(
        messages_starting(&mut fx, "Bug b1 breached").is_empty(),
        "{:?}",
        messages(&mut fx)
    );
}

/// A new bug record wakes its sleeping owner: one waking `session-message` whose text begins
/// `Bug <id> is open`, and the visible part of the wake, the bug's Inbox item, opens with E8 and
/// closes when the bug is mitigated.
#[test]
fn wakes_a_sleeping_owner_on_a_new_bug_record() {
    let mut fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle', asleep_at = 't' WHERE id = 'lead1';");
    bug(&mut fx, "b1", Some("pay"), "Payment fails at checkout", 1);

    bug_guard::run(&mut fx.engine);

    let sent = messages_starting(&mut fx, "Bug b1 is open");
    assert_eq!(sent.len(), 1, "{:?}", messages(&mut fx));
    assert_eq!(sent[0].0, "lead1");
    assert_eq!(sent[0].1, None, "the engine is the sender");
    assert!(sent[0].2, "a new bug record wakes its owner");

    // The wake's visible part: E8 opens the bug's Inbox item, linked to the owner.
    inbox::run(&mut fx.engine);
    let item = bug_item(&mut fx).expect("the bug's Inbox item is open");
    assert_eq!(item.0, "bug:b1");
    assert_eq!(item.1.as_deref(), Some("lead1"));

    // It closes when the bug is mitigated.
    fx.fixture(
        "UPDATE bug_records SET mitigation = 'Restarted the worker', mitigated_at = 't' WHERE id = 'b1';",
    );
    inbox::run(&mut fx.engine);
    assert!(
        bug_item(&mut fx).is_none(),
        "the item closes when the bug is mitigated"
    );
}
