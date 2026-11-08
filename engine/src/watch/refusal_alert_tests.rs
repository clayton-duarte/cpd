//! E25, the refusal threshold (contract §3.7 E25, §2.2 `refusal-alert`; commands.md §14.7;
//! Plan `agent-refusals`, air-d64, air-d85, air-d105): one `refusal_alert` tick counts the
//! `guard` Events (contract §5.2) of each unarchived Session over the sliding window of
//! `watch.refusal_alert_minutes`, and at or above `watch.refusal_alert_threshold` it appends
//! one `error` Event with the code `refusal_threshold` and wakes the Context's Manager; below
//! the threshold it writes nothing.

use serde_json::{Value, json};

use super::inbox;
use super::refusal_alert;
use super::testkit::{Fx, ago};

/// Plan `pay` with its Lead `lead1`, a live Manager `mgr1` of the same Context, and the two
/// config values: `threshold` refusals inside `minutes` alert.
fn fx(threshold: i64, minutes: i64) -> Fx {
    let mut fx = Fx::with_plan(&format!(
        "[watch]\nrefusal_alert_threshold = {threshold}\nrefusal_alert_minutes = {minutes}\n"
    ));
    fx.fixture(
        "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
           VALUES ('mgr1', 'manager', 'ctx', 'Mgr', '/', 'O3', 'idle', 't', 't');",
    );
    fx
}

/// One `guard` Event of Session `lead1` at `at`, refused by `code` for `model`.
fn refusal(fx: &mut Fx, at: &str, code: &str, model: &str) {
    refusal_of(fx, "lead1", at, code, model);
}

/// One `guard` Event of Session `session` at `at`, refused by `code` for `model`.
fn refusal_of(fx: &mut Fx, session: &str, at: &str, code: &str, model: &str) {
    fx.fixture(&format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
           VALUES (cpd_txn(), '{at}', 'guard', 'ctx', 'pay', '{session}', 'φ: {code}',
                   '{{\"code\":\"{code}\",\"role\":\"lead\",\"tier\":\"O3\",\"model\":\"{model}\",\"tool\":\"bash\",\"excerpt\":\"curl x\"}}');"
    ));
}

/// The open `refusal-alert` items: `(subject, label, context, plan, session)`, oldest first.
fn alert_items(
    fx: &mut Fx,
) -> Vec<(
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
)> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT subject, label, context, plan, session FROM inbox_items
                  WHERE kind = 'refusal-alert' AND closed_at IS NULL ORDER BY id",
            )?;
            Ok(stmt
                .query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// The `refusal-alert` items closed so far.
fn closed_alerts(fx: &mut Fx) -> i64 {
    fx.engine
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM inbox_items WHERE kind = 'refusal-alert' AND closed_at IS NOT NULL",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

/// A config.toml with the two refusal keys, written over the engine's home after it started (as `Fx::new` writes it).
fn rewrite_config(fx: &Fx, threshold: i64, minutes: i64) {
    let text = format!(
        "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n\n[watch]\nrefusal_alert_threshold = {threshold}\nrefusal_alert_minutes = {minutes}\n\n[tmux]\nsocket = \"testkit\"\n"
    );
    std::fs::write(fx.engine.home.config(), text).unwrap();
}

/// The `error` Events whose `code` is `refusal_threshold`, oldest first: `(text, data)`.
fn threshold_events(fx: &mut Fx) -> Vec<(String, Value)> {
    fx.events("error")
        .into_iter()
        .map(|e| e.3)
        .filter(|d| d["code"] == "refusal_threshold")
        .map(|d| (d["message"].as_str().unwrap_or_default().to_owned(), d))
        .collect()
}

/// The `session-message` Events addressed to `session`, oldest first: `(from, wake, text)`.
fn messages_to(fx: &mut Fx, session: &str) -> Vec<(Option<String>, bool, String)> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT data, text FROM events WHERE kind = 'session-message' AND session = ?1 ORDER BY id",
            )?;
            let rows = stmt
                .query_map([session], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows
                .into_iter()
                .map(|(data, text)| {
                    let data: Value = serde_json::from_str(&data).unwrap();
                    (data["from"].as_str().map(str::to_owned), data["wake"] == true, text)
                })
                .collect())
        })
        .unwrap()
}

#[test]
fn alerts_the_manager_when_a_rate_crosses_the_threshold() {
    let mut fx = fx(3, 60);
    let first = ago(30);
    let last = ago(10);
    refusal(&mut fx, &first, "usage", "p/opus");
    refusal(&mut fx, &ago(20), "network", "p/opus");
    refusal(&mut fx, &last, "usage", "p/opus");

    refusal_alert::run(&mut fx.engine);

    let alerts = threshold_events(&mut fx);
    assert_eq!(alerts.len(), 1, "one threshold Event: {alerts:?}");
    let (text, details) = &alerts[0];
    assert_eq!(details["details"]["session"], "lead1");
    assert_eq!(details["details"]["role"], "lead");
    assert_eq!(details["details"]["tier"], "O3");
    assert_eq!(details["details"]["model"], "p/opus");
    assert_eq!(details["details"]["window_minutes"], 60);
    assert_eq!(details["details"]["threshold"], 3);
    assert_eq!(details["details"]["count"], 3);
    assert_eq!(
        details["details"]["per_guard"],
        json!({"usage": 2, "network": 1})
    );
    assert_eq!(details["details"]["first"], first);
    assert_eq!(details["details"]["last"], last);
    assert!(
        text.starts_with("Session lead1 was refused 3 times in the last 60 min: "),
        "{text}"
    );

    let messages = messages_to(&mut fx, "mgr1");
    assert_eq!(messages.len(), 1, "the Manager is woken once: {messages:?}");
    let (from, wake, text) = &messages[0];
    assert_eq!(*from, None, "no sender: the engine wakes it");
    assert!(wake, "the message wakes the Manager");
    assert!(
        text.starts_with("Session lead1 was refused 3 times in the last 60 min: "),
        "{text}"
    );
}

#[test]
fn stays_silent_below_the_threshold() {
    let mut fx = fx(3, 60);
    refusal(&mut fx, &ago(30), "usage", "p/opus");
    refusal(&mut fx, &ago(20), "network", "p/opus");

    refusal_alert::run(&mut fx.engine);

    assert_eq!(
        threshold_events(&mut fx).len(),
        0,
        "two refusals are below the threshold of three"
    );
    assert_eq!(
        messages_to(&mut fx, "mgr1").len(),
        0,
        "no Manager wakes below the threshold"
    );
}

#[test]
fn e8_opens_one_item_per_session_at_the_threshold_and_closes_it_below_it() {
    let mut fx = fx(3, 60);
    fx.fixture(
        "INSERT INTO sessions (id, role, context, plan, forked_from, name, cwd, tier, state, state_at, created_at)
           VALUES ('fork1', 'lead', 'ctx', 'pay', 'lead1', 'Fork', '/', 'O3', 'stopped', 't', 't');",
    );

    // Below the threshold: two refusals each, no item.
    refusal(&mut fx, &ago(3), "usage", "p/opus");
    refusal(&mut fx, &ago(2), "usage", "p/opus");
    refusal_of(&mut fx, "fork1", &ago(3), "usage", "p/sonnet");
    refusal_of(&mut fx, "fork1", &ago(2), "usage", "p/sonnet");
    inbox::run(&mut fx.engine);
    assert_eq!(
        alert_items(&mut fx),
        vec![],
        "below the threshold opens nothing"
    );

    // The third refusal of each Session reaches the threshold: one red item per Session, labelled and linked.
    refusal(&mut fx, &ago(1), "network", "p/opus");
    refusal_of(&mut fx, "fork1", &ago(1), "network", "p/sonnet");
    inbox::run(&mut fx.engine);
    assert_eq!(
        alert_items(&mut fx),
        vec![
            (
                "refusals:lead1".to_owned(),
                "pay: 3 refusals in 60 min".to_owned(),
                Some("ctx".to_owned()),
                Some("pay".to_owned()),
                Some("lead1".to_owned()),
            ),
            (
                "refusals:fork1".to_owned(),
                "pay·fork: 3 refusals in 60 min".to_owned(),
                Some("ctx".to_owned()),
                Some("pay".to_owned()),
                Some("fork1".to_owned()),
            ),
        ]
    );

    // Idempotent: a second tick opens no second item and closes none.
    inbox::run(&mut fx.engine);
    assert_eq!(alert_items(&mut fx).len(), 2);
    assert_eq!(closed_alerts(&mut fx), 0);

    // The window slides past the older refusals: the count falls below the threshold and both items close.
    rewrite_config(&fx, 3, 1);
    inbox::run(&mut fx.engine);
    assert_eq!(
        alert_items(&mut fx),
        vec![],
        "the count fell below the threshold"
    );
    assert_eq!(closed_alerts(&mut fx), 2);
}

#[test]
fn e8_closes_the_refusal_alert_item_when_the_session_is_archived() {
    let mut fx = fx(3, 60);
    refusal(&mut fx, &ago(3), "usage", "p/opus");
    refusal(&mut fx, &ago(2), "usage", "p/opus");
    refusal(&mut fx, &ago(1), "usage", "p/opus");

    inbox::run(&mut fx.engine);
    assert_eq!(alert_items(&mut fx).len(), 1);

    fx.fixture("UPDATE sessions SET archived_at = 't' WHERE id = 'lead1';");
    inbox::run(&mut fx.engine);
    assert_eq!(
        alert_items(&mut fx),
        vec![],
        "an archived Session holds no item"
    );
    assert_eq!(closed_alerts(&mut fx), 1);
}
