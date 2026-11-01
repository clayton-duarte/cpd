//! E8, the Inbox (contract §2.2 "Inbox item", §3.7), one `inbox::run` per tick on a prepared store: items open and close
//! within a tick with their Events; a failed Job is stuck only after `inbox.stuck_job_minutes` and then goes to the Context's
//! Manager as one message, not as an item (hs-d318); a tick with no change writes nothing; and the `loop-guard` item
//! (hs-d552, hs-d571): a Lead, fork or Manager Session `blocked` with the label `φ: loop guard` is one red item, whatever its
//! Plan's phase and never also a `dialog`, closed when the Session leaves that state.

use serde_json::{Value, json};

use super::inbox;
use super::testkit::{Fx, ago};

const GUARD: &str = "φ: loop guard";

/// Plan `pay` in `dispatch`, its Lead `lead1` `working`, a tick of E8 and `stuck_job_minutes = 1`.
pub(super) fn fx() -> Fx {
    let mut fx = Fx::with_plan("[inbox]\nstuck_job_minutes = 1\n");
    fx.fixture("UPDATE sessions SET state = 'working' WHERE id = 'lead1';");
    fx
}

/// One tick of E8.
pub(super) fn tick(fx: &mut Fx) {
    inbox::run(&mut fx.engine);
}

/// A `failed` Job `key` of Plan `pay` whose `job-failed` Event is `minutes` old.
pub(super) fn failed_job(fx: &mut Fx, key: &str, minutes: i64) {
    fx.fixture(&format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, failed_cause, created_txn, created_at)
         VALUES ('pay', 'repo', '{key}', 'Title', 'S2', 'Goal.', 'Because.', '[\"a\"]', '[\"ok\"]', '[]', 'failed', 0,
                 hex(zeroblob(32)), 'crashed', cpd_txn(), 't');
         INSERT INTO events (txn, at, kind, context, plan, job, text, data)
         VALUES (cpd_txn(), '{}', 'job-failed', 'ctx', 'pay', '{key}', 'x', '{{}}');",
        ago(minutes)
    ));
}

/// The open items: `(kind, urgency, subject, label)`.
pub(super) fn open_items(fx: &mut Fx) -> Vec<(String, String, String, String)> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT kind, urgency, subject, label FROM inbox_items WHERE closed_at IS NULL ORDER BY id")?;
            Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// The open items: `(kind, urgency, session, label)`.
fn open_sessions(fx: &mut Fx) -> Vec<(String, String, Option<String>, String)> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT kind, urgency, session, label FROM inbox_items WHERE closed_at IS NULL ORDER BY id")?;
            Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

fn item(fx: &mut Fx, kind: &str) -> Option<(String, String, Option<String>, String)> {
    open_sessions(fx).into_iter().find(|i| i.0 == kind)
}

pub(super) fn data(fx: &mut Fx, kind: &str) -> Vec<Value> {
    fx.events(kind).into_iter().map(|e| e.3).collect()
}

/// A live Manager Session `mgr1` of Context `ctx`, idle.
pub(super) fn manager(fx: &mut Fx) {
    fx.fixture(
        "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
           VALUES ('mgr1', 'manager', 'ctx', 'Mgr', '/', 'O3', 'idle', 't', 't');",
    );
}

/// The `session-message` Events, oldest first: `(session, from, wake, text)`.
pub(super) fn messages(fx: &mut Fx) -> Vec<(String, Option<String>, bool, String)> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT session, json_extract(data, '$.from'), json_extract(data, '$.wake'), json_extract(data, '$.text')
                   FROM events WHERE kind = 'session-message' ORDER BY id",
            )?;
            Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// The Lead of a Plan in `dispatch` that blocks is a red `dialog` within a tick, with `inbox-opened`; when it
/// works again the item is closed, with `inbox-closed`.
#[test]
fn a_blocked_lead_in_dispatch_is_a_red_dialog_until_it_works_again() {
    let mut fx = fx();
    fx.fixture(
        "UPDATE sessions SET state = 'blocked', state_label = 'Which repo?' WHERE id = 'lead1';",
    );

    tick(&mut fx);
    assert_eq!(
        open_items(&mut fx),
        [(
            "dialog".into(),
            "red".into(),
            "session:lead1".into(),
            "pay asks you".into()
        )]
    );
    let id: i64 = fx.query("SELECT id FROM inbox_items WHERE kind = 'dialog'");
    assert_eq!(
        data(&mut fx, "inbox-opened"),
        [json!({"item": id, "kind": "dialog", "urgency": "red", "label": "pay asks you"})]
    );
    assert_eq!(
        fx.query::<String>("SELECT plan || '/' || session FROM events WHERE kind = 'inbox-opened'"),
        "pay/lead1"
    );
    assert!(data(&mut fx, "inbox-closed").is_empty());

    fx.fixture("UPDATE sessions SET state = 'working', state_label = NULL WHERE id = 'lead1';");
    tick(&mut fx);
    assert_eq!(open_items(&mut fx), []);
    assert_eq!(
        data(&mut fx, "inbox-closed"),
        [json!({"item": id, "kind": "dialog"})]
    );
    assert!(
        fx.query::<Option<String>>(&format!(
            "SELECT closed_at FROM inbox_items WHERE id = {id}"
        ))
        .is_some()
    );
    assert_eq!(data(&mut fx, "error"), Vec::<Value>::new());
}

/// hs-d694: the plan review's visible label is `φ: plan review`, so a Lead `blocked` with exactly that label
/// opens no `dialog` (the matcher must recognise the φ label).
#[test]
fn a_phi_plan_review_blocked_session_is_not_a_dialog() {
    let mut fx = fx();
    fx.fixture(
        "UPDATE sessions SET state = 'blocked', state_label = 'φ: plan review' WHERE id = 'lead1';",
    );

    tick(&mut fx);
    assert!(
        item(&mut fx, "dialog").is_none(),
        "{:?}",
        open_sessions(&mut fx)
    );
    assert!(
        data(&mut fx, "inbox-opened")
            .iter()
            .all(|e| e["kind"] != "dialog"),
        "{:?}",
        data(&mut fx, "inbox-opened")
    );
}

/// The text of the one message for a Workflow's stuck Jobs (contract §3.7 "A stuck Job"): `<key>` and `<T>` stay for the
/// Manager to choose.
pub(super) fn stuck_text(key: &str, minutes: i64, cause: &str) -> String {
    format!(
        "Jobs of Workflow pay/repo failed and their Lead has not retried or cancelled them: \
         pay/{key} ({minutes} min ago, {cause}). \
         Retry each (cpd job retry <key> --plan pay --tier <T>), nudge the Lead, or ask the User."
    )
}

/// hs-d647: the stuck Jobs of one Workflow are ONE message to the Manager, oldest `job-failed` Event first, each Job with
/// its own minutes and `failed_cause`; it is sent once, and no item opens (hs-d318).
#[test]
fn stuck_jobs_of_one_pr_are_one_message_to_the_manager() {
    let mut fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    manager(&mut fx);
    failed_job(&mut fx, "old", 2);
    failed_job(&mut fx, "fresh", 2);

    tick(&mut fx);
    // Three ticks later: still the one message for the Workflow, and no item opened.
    for _ in 0..3 {
        tick(&mut fx);
    }
    assert_eq!(
        messages(&mut fx),
        [(
            "mgr1".to_owned(),
            None,
            true,
            "Jobs of Workflow pay/repo failed and their Lead has not retried or cancelled them: \
             pay/old (2 min ago, crashed); pay/fresh (2 min ago, crashed). \
             Retry each (cpd job retry <key> --plan pay --tier <T>), nudge the Lead, or ask the User."
                .to_owned()
        )]
    );
    assert_eq!(open_items(&mut fx), []);
    assert_eq!(data(&mut fx, "inbox-opened"), Vec::<Value>::new());
}

/// hs-d318: a failed Job, `stuck_job_minutes` (here 1) after its `job-failed` Event and with its Lead not working, is one
/// message to its Context's Manager, from the User's side waking, once, and opens no `job-stuck` item. A fresh
/// failure is not stuck yet.
#[test]
fn a_stuck_job_is_one_message_to_the_manager_and_opens_no_item() {
    let mut fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';"); // a working Lead holds the message back (audit Q1)
    manager(&mut fx);
    failed_job(&mut fx, "old", 2);
    failed_job(&mut fx, "new", 0);

    tick(&mut fx);
    assert_eq!(messages(&mut fx).len(), 1);
    // Three ticks later: still the one message, the fresh failure is not stuck, and no item opened.
    for _ in 0..3 {
        tick(&mut fx);
    }
    assert_eq!(
        messages(&mut fx),
        [(
            "mgr1".to_owned(),
            None,
            true,
            stuck_text("old", 2, "crashed")
        )]
    );
    assert_eq!(open_items(&mut fx), []);
    assert_eq!(data(&mut fx, "inbox-opened"), Vec::<Value>::new());
}

/// pro-d88: a failed Job with a `queued` fix Job of its Workflow created after it is being worked on, so the Manager hears
/// nothing while the fix is underway; when the fix is cancelled the next tick sends the one message, once.
#[test]
fn a_failed_job_with_a_fix_underway_sends_the_manager_no_nudge_until_the_fix_ends() {
    let mut fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    manager(&mut fx);
    failed_job(&mut fx, "old", 2);
    fx.fixture(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, created_at)
         VALUES ('pay', 'repo', 'fix', 'Title', 'S2', 'Goal.', 'Because.', '[\"a\"]', '[\"ok\"]', '[]', 'queued', 1,
                 hex(zeroblob(32)), cpd_txn(), 't');",
    );
    for _ in 0..3 {
        tick(&mut fx);
    }
    assert_eq!(messages(&mut fx), []);
    assert_eq!(open_items(&mut fx), []);

    fx.fixture("UPDATE jobs SET status = 'cancelled', cancel_reason = 'r', cancel_requested_at = 't' WHERE key = 'fix';");
    for _ in 0..3 {
        tick(&mut fx);
    }
    assert_eq!(
        messages(&mut fx),
        [(
            "mgr1".to_owned(),
            None,
            true,
            stuck_text("old", 2, "crashed")
        )]
    );
    assert_eq!(open_items(&mut fx), []);
}

/// The record that a Job was told of is the message itself, in the store: a restarted engine sends nothing again.
#[test]
fn a_restarted_engine_does_not_tell_the_manager_of_the_same_failure_again() {
    let mut fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    manager(&mut fx);
    failed_job(&mut fx, "old", 2);
    tick(&mut fx);
    assert_eq!(messages(&mut fx).len(), 1);

    let mut second = fx.restarted();
    for _ in 0..3 {
        inbox::run(&mut second);
    }
    // The restarted engine also tells the Manager how long it was down (contract §3.7, hs-d328): not a stuck-Job message.
    let messages: Vec<_> = messages(&mut fx)
        .into_iter()
        .filter(|(_, _, _, text)| !text.contains("The engine was down for "))
        .collect();
    assert_eq!(
        messages,
        [(
            "mgr1".to_owned(),
            None,
            true,
            stuck_text("old", 2, "crashed")
        )]
    );
}

/// After `job retry` and a new failure (a newer `job-failed` Event) the Job is told again, once more.
#[test]
fn a_job_that_fails_again_after_a_retry_is_sent_again() {
    let mut fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    manager(&mut fx);
    failed_job(&mut fx, "old", 2);
    tick(&mut fx);
    assert_eq!(messages(&mut fx).len(), 1);

    // The retry and the new failure in one transaction: the engine sees only the Job failed again.
    fx.fixture(&format!(
        "UPDATE jobs SET status = 'queued', failed_cause = NULL WHERE plan = 'pay' AND key = 'old';
         UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'old';
         UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE plan = 'pay' AND key = 'old';
         INSERT INTO events (txn, at, kind, context, plan, job, text, data)
         VALUES (cpd_txn(), '{}', 'job-failed', 'ctx', 'pay', 'old', 'x', '{{}}');",
        ago(3)
    ));
    tick(&mut fx);
    assert_eq!(messages(&mut fx).len(), 2);
    for _ in 0..3 {
        tick(&mut fx);
    }
    assert_eq!(
        messages(&mut fx)
            .into_iter()
            .map(|m| m.3)
            .collect::<Vec<_>>(),
        [
            stuck_text("old", 2, "crashed"),
            stuck_text("old", 3, "timed_out")
        ]
    );
    assert_eq!(open_items(&mut fx), []);
}

/// No Manager: nothing is sent and nothing is written, and a Manager that comes later is told at the next tick.
#[test]
fn with_no_manager_nothing_is_sent_and_a_later_manager_is_told() {
    let mut fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    failed_job(&mut fx, "old", 2);
    for _ in 0..3 {
        tick(&mut fx);
    }
    assert_eq!(messages(&mut fx), []);
    assert_eq!(open_items(&mut fx), []);
    let txns = fx.query::<i64>("SELECT count(*) FROM txns");
    for _ in 0..2 {
        tick(&mut fx);
    }
    assert_eq!(
        fx.query::<i64>("SELECT count(*) FROM txns"),
        txns,
        "a stuck Job nobody can be told of writes nothing"
    );

    manager(&mut fx);
    tick(&mut fx);
    assert_eq!(
        messages(&mut fx),
        [(
            "mgr1".to_owned(),
            None,
            true,
            stuck_text("old", 2, "crashed")
        )]
    );
}

/// A working Lead is handling the failure: nothing is sent until it stops (audit Q1).
#[test]
fn a_working_lead_holds_the_message_back() {
    let mut fx = fx();
    manager(&mut fx);
    failed_job(&mut fx, "old", 2);
    for _ in 0..3 {
        tick(&mut fx);
    }
    assert_eq!(messages(&mut fx), []);

    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    tick(&mut fx);
    assert_eq!(messages(&mut fx).len(), 1);
}

/// An item an older engine opened for a stuck Job closes at the next reconcile, with its Event, and none opens again.
#[test]
fn an_open_job_stuck_item_closes_and_none_opens() {
    let mut fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    manager(&mut fx);
    failed_job(&mut fx, "old", 2);
    fx.fixture(
        "INSERT INTO inbox_items (kind, urgency, subject, label, context, plan, job, opened_at)
         VALUES ('job-stuck', 'red', 'job:pay/old', 'pay/old failed', 'ctx', 'pay', 'old', 't');",
    );
    assert_eq!(open_items(&mut fx).len(), 1);

    tick(&mut fx);
    assert_eq!(open_items(&mut fx), []);
    assert_eq!(data(&mut fx, "inbox-closed").len(), 1);
    for _ in 0..2 {
        tick(&mut fx);
    }
    assert_eq!(open_items(&mut fx), []);
    assert_eq!(data(&mut fx, "inbox-opened"), Vec::<Value>::new());
}

/// Pull request missionlane/cc-api-orc-apollo#2385 as the poll stores it, with the given review decision and merge state.
fn polled_2385(fx: &mut Fx, review_decision: &str, merge_state: &str) {
    fx.fixture(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, merge_state, created_at, changed_at,
                                        base_ref, default_branch)
         VALUES ('missionlane/cc-api-orc-apollo', 2385, 'https://github.com/missionlane/cc-api-orc-apollo/pull/2385', 'T2385', 0,
                 'open', '{review_decision}', 'passed', '{merge_state}', 't', 't', 'main', 'main');"
    ));
}

const PR_READY_OPEN: &str = "SELECT count(*) FROM inbox_items WHERE kind = 'pr-ready' AND label LIKE '% ready to merge' || char(10) || '%' AND closed_at IS NULL";

/// hs-d481: a pull request GitHub reports REVIEW_REQUIRED and BLOCKED (stored `none`, `blocked`) is never `pr-ready`.
/// `a_clean_approved_pull_request_is_ready` is the positive control.
#[test]
fn a_review_required_blocked_pull_request_is_never_ready() {
    let mut fx = fx();
    polled_2385(&mut fx, "none", "blocked");
    assert_eq!(
        fx.query::<i64>(
            "SELECT count(*) FROM pull_requests WHERE number = 2385 AND merge_state = 'blocked'"
        ),
        1
    );
    for _ in 0..2 {
        tick(&mut fx);
    }
    assert_eq!(
        fx.query::<i64>(PR_READY_OPEN),
        0,
        "{:?}",
        open_items(&mut fx)
    );
}

/// The positive control: the same pull request, approved and clean, opens one `pr-ready` item.
#[test]
fn a_clean_approved_pull_request_is_ready() {
    let mut fx = fx();
    polled_2385(&mut fx, "approved", "clean");
    tick(&mut fx);
    assert_eq!(fx.query::<i64>(PR_READY_OPEN), 1);
}

/// A tick that changes nothing writes nothing: no `txns` row, no Event (a stuck Job already sent included).
#[test]
fn a_tick_with_no_change_writes_no_transaction() {
    let mut fx = fx();
    manager(&mut fx);
    fx.fixture(
        "UPDATE sessions SET state = 'blocked', state_label = 'Which repo?' WHERE id = 'lead1';",
    );
    failed_job(&mut fx, "old", 2);
    // The dialog holds the Lead `blocked`, which is not `working`: the stuck Job is sent as well.
    tick(&mut fx);
    assert!(!open_items(&mut fx).is_empty());
    assert!(!messages(&mut fx).is_empty());
    let reconciles = "SELECT count(*) FROM txns WHERE command = 'inbox reconcile'";
    assert_eq!(fx.query::<i64>(reconciles), 1);
    let (txns, events) = (
        fx.query::<i64>("SELECT count(*) FROM txns"),
        fx.query::<i64>("SELECT count(*) FROM events"),
    );

    tick(&mut fx);
    assert_eq!(
        fx.query::<i64>("SELECT count(*) FROM txns"),
        txns,
        "no transaction of any step"
    );
    for _ in 0..3 {
        tick(&mut fx);
    }
    assert_eq!(fx.query::<i64>(reconciles), 1);
    assert_eq!(
        fx.query::<i64>("SELECT count(*) FROM txns"),
        txns,
        "no transaction of any step"
    );
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM events"), events);
}

// ---------- the loop-guard item ----------

/// Plan `pay` in `phase` with its Lead `lead1` (`working`) and a Manager `mgr1` (`idle`).
fn guard_fx(phase: &str, extra_config: &str) -> Fx {
    let mut fx = Fx::with_plan_in(phase, extra_config);
    fx.fixture("UPDATE sessions SET state = 'working' WHERE id = 'lead1';");
    manager(&mut fx);
    fx
}

fn guard(fx: &mut Fx, session: &str) {
    fx.fixture(&format!(
        "UPDATE sessions SET state = 'blocked', state_label = '{GUARD}' WHERE id = '{session}';"
    ));
}

/// A `blocked` Lead with the label is one open red `loop-guard` item with the label, linked to its Session and no `dialog`.
#[test]
fn e8_opens_a_red_loop_guard_item_for_a_blocked_lead_with_the_label() {
    let mut fx = guard_fx("dispatch", "");
    guard(&mut fx, "lead1");
    tick(&mut fx);
    assert_eq!(
        open_sessions(&mut fx),
        [(
            "loop-guard".into(),
            "red".into(),
            Some("lead1".into()),
            "pay: loop guard, 21 turns in 10 min".into()
        )]
    );
    let id: i64 = fx.query("SELECT id FROM inbox_items WHERE kind = 'loop-guard'");
    assert_eq!(
        data(&mut fx, "inbox-opened"),
        [
            json!({"item": id, "kind": "loop-guard", "urgency": "red", "label": "pay: loop guard, 21 turns in 10 min"})
        ]
    );
    assert_eq!(
        fx.query::<String>(
            "SELECT plan || '/' || session FROM inbox_items WHERE kind = 'loop-guard'"
        ),
        "pay/lead1"
    );
    assert!(item(&mut fx, "dialog").is_none());
    assert_eq!(data(&mut fx, "error"), Vec::<Value>::new());
}

/// The item is red in `grounding` and in `dispatch` (a dialog is magenta in `grounding`), and is never a dialog.
#[test]
fn e8_loop_guard_item_is_red_in_every_phase_and_is_no_dialog() {
    for phase in ["grounding", "dispatch"] {
        let mut fx = guard_fx(phase, "");
        guard(&mut fx, "lead1");
        tick(&mut fx);
        assert!(item(&mut fx, "loop-guard").is_some(), "{phase}");
        assert_eq!(item(&mut fx, "loop-guard").unwrap().1, "red", "{phase}");
        assert_eq!(
            open_sessions(&mut fx).len(),
            1,
            "{phase}: {:?}",
            open_sessions(&mut fx)
        );
        assert!(item(&mut fx, "dialog").is_none(), "{phase}");
    }
}

/// Leaving `blocked` (to `working`, `idle` or `stopped`) closes the item with `inbox-closed`.
#[test]
fn e8_closes_the_loop_guard_item_when_the_session_leaves_blocked() {
    let mut fx = guard_fx("dispatch", "");
    let mut closed = 0;
    for state in ["working", "idle", "stopped"] {
        guard(&mut fx, "lead1");
        tick(&mut fx);
        assert!(item(&mut fx, "loop-guard").is_some(), "{state}");
        let id: i64 =
            fx.query("SELECT id FROM inbox_items WHERE kind = 'loop-guard' AND closed_at IS NULL");
        fx.fixture(&format!(
            "UPDATE sessions SET state = '{state}', state_label = NULL WHERE id = 'lead1';"
        ));
        tick(&mut fx);
        assert_eq!(open_sessions(&mut fx), [], "{state}");
        closed += 1;
        let events = data(&mut fx, "inbox-closed");
        assert_eq!(events.len(), closed, "{state}: {events:?}");
        assert_eq!(
            events.last().unwrap(),
            &json!({"item": id, "kind": "loop-guard"}),
            "{state}"
        );
        assert!(
            fx.query::<Option<String>>(&format!(
                "SELECT closed_at FROM inbox_items WHERE id = {id}"
            ))
            .is_some()
        );
    }
}

/// `loop_guard_turns = 3` makes the label say `4 turns`; a Manager's label starts with its Context id.
#[test]
fn e8_loop_guard_label_follows_loop_guard_turns_and_names_a_manager_by_its_context() {
    let mut fx = guard_fx("dispatch", "loop_guard_turns = 3\n");
    guard(&mut fx, "lead1");
    tick(&mut fx);
    assert_eq!(
        item(&mut fx, "loop-guard").unwrap().3,
        "pay: loop guard, 4 turns in 10 min"
    );

    guard(&mut fx, "mgr1");
    tick(&mut fx);
    assert_eq!(
        open_sessions(&mut fx)
            .iter()
            .filter(|i| i.0 == "loop-guard")
            .count(),
        2
    );
    let manager = open_sessions(&mut fx)
        .into_iter()
        .find(|i| i.2.as_deref() == Some("mgr1"))
        .unwrap();
    assert_eq!(
        (manager.0.as_str(), manager.1.as_str(), manager.3.as_str()),
        ("loop-guard", "red", "ctx: loop guard, 4 turns in 10 min")
    );
    assert!(
        item(&mut fx, "dialog").is_none(),
        "{:?}",
        open_sessions(&mut fx)
    );
}

// ---------- the message wakes the Manager ----------

/// hs-d514: a stuck Job is blocked work, so its message to the Context's Manager wakes it.
#[test]
fn a_stuck_job_wakes_its_context_manager() {
    let mut fx = Fx::with_plan("[inbox]\nstuck_job_minutes = 1\n");
    fx.fixture(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('mgr1', 'manager', 'ctx', NULL, 'Manager', '/', 'O5', 'idle', 't', 't');",
    );
    fx.add_job(
        "api",
        &[],
        "UPDATE jobs SET status = 'running' WHERE key = 'api';
         UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE key = 'api';",
    );
    fx.fixture(&format!(
        "INSERT INTO events (txn, at, kind, context, plan, job, text, data)
         VALUES (cpd_txn(), '{}', 'job-failed', 'ctx', 'pay', 'api', 'x', '{{}}');",
        ago(2)
    ));
    tick(&mut fx);
    let messages = data(&mut fx, "session-message");
    assert_eq!(messages.len(), 1, "{messages:?}");
    let data = &messages[0];
    assert_eq!(
        (
            fx.query::<String>("SELECT session FROM events WHERE kind = 'session-message'")
                .as_str(),
            data["from"].clone(),
            data["wake"].clone()
        ),
        ("mgr1", Value::Null, json!(true)),
        "{data}"
    );
    // hs-d647: the stuck Jobs of a Workflow are one message (contract §3.7 "A stuck Job").
    let text = data["text"].as_str().unwrap();
    assert!(
        text.starts_with("Jobs of Workflow pay/repo failed "),
        "{data}"
    );
    assert!(text.contains("pay/api (2 min ago, crashed)"), "{data}");
}
