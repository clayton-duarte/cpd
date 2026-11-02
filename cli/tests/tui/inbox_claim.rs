//! `cpd inbox claim <id>`, the fixed `codereview` target and the `hidden` object's `claimed` count (commands.md §21.7,
//! §21.1; contract §8.2): a temporary `$CPD_HOME`, the Inbox fixture of `inbox.rs`, a fake tmux.

use crate::inbox::{CLOSED_Y, Fixture, T0, condition};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use serde_json::{Value, json};
use std::fs;

const URL: &str = "https://github.com/acme/pay/pull/812";
const SUBJECT: &str = "pr:acme/pay#812";
const CHANNEL: &str = "C02JU6C2Z1D";
const TS_RAW: &str = "1700000000.000100";
const MANAGER: &str = "mgr-rc";
const T5: &str = "2026-01-01T05:00:00.000Z";

fn config(socket: &str) -> String {
    format!("version = 1\nmachine = \"m1\"\n[tmux]\nsocket = \"{socket}\"\n")
}

/// The five open items `Fixture::new` leaves, as conditions: a reconcile that omits one closes it.
fn base() -> Vec<Condition> {
    vec![
        condition(Kind::JobStuck, Urgency::Red, "stuck", "ctx1"),
        condition(Kind::Dialog, Urgency::White, "dialog", "ctx1"),
        condition(Kind::PlanReview, Urgency::Yellow, "review", "ctx2"),
        condition(Kind::Unread, Urgency::Magenta, "unread", "ctx1"),
        condition(Kind::Question, Urgency::Yellow, "question", "ctx1"),
    ]
}

fn review_request() -> Condition {
    Condition {
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
    }
}

fn sql(fx: &mut Fixture, statements: &str) {
    fx.store
        .write("fixture", |tx| {
            tx.execute_batch(statements)?;
            Ok(())
        })
        .unwrap();
}

/// The Context `codereview` with a Manager Session `MANAGER` in `state`.
fn codereview_context(fx: &mut Fixture, state: &str) {
    let id = "codereview";
    sql(
        fx,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('{id}', 'Review', 't', 'm1');
             INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at, archived_at)
             VALUES ('{MANAGER}', 'manager', '{id}', NULL, NULL, '{id}-manager', '/sessions/{MANAGER}.jsonl', '/repo', 'O5', NULL, '{state}', NULL,
                     '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL);"
        ),
    );
}

fn write_cache(fx: &Fixture, number: u64) {
    let path = fx.fx.env.cpd_home().join("run/team-review.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let post = json!({"repo": "acme/pay", "number": number, "ts": 1_700_000_000.0, "ts_raw": TS_RAW, "author": "Ryan Case", "primary": true,
                      "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false});
    let cache =
        json!({"version": 1, "polled_at": T0, "channel": CHANNEL, "read": 10, "posts": [post]});
    fs::write(path, cache.to_string()).unwrap();
}

/// The Fixture with an open `review-requested` item for `acme/pay#812`, the Context `codereview` with an idle live
/// Manager and the post in the cache. Returns the item's id.
fn claimable() -> (Fixture, i64) {
    let mut fx = Fixture::new();
    fx.fx.env.write_config(&config(fx.fx.env.tmux_socket()));
    codereview_context(&mut fx, "idle");
    write_cache(&fx, 812);
    let mut all = base();
    all.push(review_request());
    reconcile(&mut fx.store, &all, T5).unwrap();
    let id = fx.store_count(&format!(
        "SELECT id FROM inbox_items WHERE subject = '{SUBJECT}'"
    ));
    (fx, id)
}

/// A refusal wrote nothing: no Event, no message, no `claimed_at`.
fn assert_nothing_written(fx: &Fixture) {
    assert!(fx.events("inbox-claimed").is_empty());
    assert!(fx.events("session-message").is_empty());
    assert_eq!(
        fx.store_count("SELECT count(*) FROM inbox_items WHERE claimed_at IS NOT NULL"),
        0
    );
}

fn refusal(fx: &Fixture, id: &str) -> (i32, String) {
    let (exit, error) = fx.fail(&["claim", id]);
    (exit, error["code"].as_str().unwrap_or("").to_owned())
}

#[test]
fn claim_marks_the_item_claimed_hides_it_and_writes_inbox_claimed() {
    let (fx, id) = claimable();
    let out = fx.ok(&["claim", &id.to_string()]);
    assert_eq!(out["ok"], true);
    assert_eq!(out["item"]["id"], id);
    assert!(out["item"]["claimed_at"].is_string(), "{out}");
    assert_eq!(out["manager"], MANAGER);
    let events = fx.events("inbox-claimed");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["data"], json!({"item": id}));

    let list = fx.ok(&["list"]);
    assert!(
        !list["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == id),
        "{list}"
    );
    let all = fx.ok(&["list", "--all"]);
    let item = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id)
        .unwrap_or_else(|| panic!("{all}"));
    assert_eq!(item["hidden"], "claimed");
    assert_eq!(item["claimed_at"], out["item"]["claimed_at"]);
    assert_eq!(
        item["closed_at"],
        Value::Null,
        "the item stays open until E8 closes it"
    );
}

#[test]
fn claim_tells_the_codereview_manager_with_the_pull_request_url_and_the_slack_post() {
    let (fx, id) = claimable();
    fx.ok(&["claim", &id.to_string()]);
    let messages = fx.events("session-message");
    assert_eq!(messages.len(), 1, "{messages:?}");
    let message = &messages[0];
    assert_eq!(message["session"], MANAGER);
    assert_eq!(message["context"], "codereview");
    assert_eq!(
        message["data"]["from"],
        Value::Null,
        "sent with no sender Session"
    );
    let text = message["data"]["text"].as_str().unwrap();
    for part in [URL, CHANNEL, TS_RAW] {
        assert!(text.contains(part), "{part} in {text}");
    }
}

#[test]
fn claim_is_idempotent_and_the_second_claim_writes_and_sends_nothing() {
    let (fx, id) = claimable();
    let id = id.to_string();
    let first = fx.ok(&["claim", &id]);
    assert_eq!(first.get("already"), None);
    let cursor = fx.ok(&["list"])["events_cursor"].clone();
    let count = fx.store_count("SELECT count(*) FROM txns");

    let again = fx.ok(&["claim", &id]);
    assert_eq!(again["already"], true);
    assert_eq!(again["item"]["claimed_at"], first["item"]["claimed_at"]);
    assert_eq!(fx.events("inbox-claimed").len(), 1);
    assert_eq!(fx.events("session-message").len(), 1);
    assert_eq!(fx.ok(&["list"])["events_cursor"], cursor);
    assert_eq!(fx.store_count("SELECT count(*) FROM txns"), count);
}

#[test]
fn claim_refuses_an_item_that_is_missing_closed_dismissed_or_not_a_review_request_and_writes_nothing()
 {
    let (mut fx, id) = claimable();
    assert_eq!(refusal(&fx, "999"), (4, "item_not_found".to_owned()));
    // another kind
    assert_eq!(
        refusal(&fx, &fx.id("unread").to_string()),
        (3, "not_claimable".to_owned())
    );
    // a dismissed review-requested item
    fx.ok(&["dismiss", &id.to_string()]);
    assert_eq!(
        refusal(&fx, &id.to_string()),
        (3, "not_claimable".to_owned())
    );
    // a closed one: the pull request's condition ended
    reconcile(&mut fx.store, &base(), CLOSED_Y).unwrap();
    assert_eq!(refusal(&fx, &id.to_string()), (3, "item_closed".to_owned()));
    assert_nothing_written(&fx);
}

#[test]
fn claim_refuses_when_the_slack_post_is_not_in_the_cache_and_writes_nothing() {
    let (fx, id) = claimable();
    let cache = fx.fx.env.cpd_home().join("run/team-review.json");
    write_cache(&fx, 7);
    assert_eq!(
        refusal(&fx, &id.to_string()),
        (3, "post_not_found".to_owned()),
        "a cache without a post of this pull request"
    );
    fs::remove_file(cache).unwrap();
    assert_eq!(
        refusal(&fx, &id.to_string()),
        (3, "post_not_found".to_owned()),
        "no cache"
    );
    assert_nothing_written(&fx);
}

#[test]
fn claim_wakes_a_stopped_manager_as_session_send_does() {
    let mut fx = Fixture::new();
    fx.fx.env.write_config(&config(fx.fx.env.tmux_socket()));
    codereview_context(&mut fx, "stopped");
    write_cache(&fx, 812);
    let mut all = base();
    all.push(review_request());
    reconcile(&mut fx.store, &all, T5).unwrap();
    let id = fx.store_count(&format!(
        "SELECT id FROM inbox_items WHERE subject = '{SUBJECT}'"
    ));
    fx.fx.cockpit();

    let out = fx.ok(&["claim", &id.to_string()]);
    assert_eq!(out["manager"], MANAGER);
    let started: Vec<Vec<String>> = fx
        .fx
        .calls("tmux")
        .into_iter()
        .map(|c| c.args)
        .filter(|a| a.iter().any(|x| x == "new-window"))
        .collect();
    assert_eq!(started.len(), 1, "one new-window: {started:?}");
    assert_eq!(
        started[0][started[0].len() - 3..],
        ["session", "run", MANAGER],
        "{:?}",
        started[0]
    );
    let shown = fx.cpd(&["session", "show", MANAGER]).ok();
    assert!(shown["session"]["tmux_pane"].is_string(), "{shown}");
    assert_eq!(fx.events("session-message").len(), 1);
}

#[test]
fn list_counts_and_names_a_claimed_item_with_all() {
    let (fx, id) = claimable();
    assert_eq!(
        fx.ok(&["list"])["hidden"],
        json!({"snoozed": 0, "dismissed": 0, "claimed": 0})
    );
    fx.ok(&["claim", &id.to_string()]);
    let list = fx.ok(&["list"]);
    assert_eq!(
        list["hidden"],
        json!({"snoozed": 0, "dismissed": 0, "claimed": 1})
    );
    let all = fx.ok(&["list", "--all"]);
    assert_eq!(
        all["hidden"],
        json!({"snoozed": 0, "dismissed": 0, "claimed": 1})
    );
}

/// `(channel, ts, emoji)` of every `slack_reactions` row, oldest first, plus the first row's `reacted_at`. Empty while
/// the table does not exist; any other error panics.
fn queued_rows(fx: &Fixture) -> Vec<(String, String, String, Option<String>)> {
    let home = cpd_core::home::CpdHome::resolve(Some(fx.fx.env.cpd_home().into()), None).unwrap();
    let result = cpd_core::store::Store::open(&home, 5000)
        .unwrap()
        .read(|c| {
            let rows = c
                .prepare("SELECT channel, ts, emoji, reacted_at FROM slack_reactions ORDER BY id")
                .and_then(|mut s| {
                    s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                        .collect::<Result<Vec<_>, _>>()
                });
            Ok(rows)
        })
        .unwrap();
    match result {
        Ok(rows) => rows,
        Err(e) if e.to_string().contains("no such table") => Vec::new(),
        Err(e) => panic!("{e}"),
    }
}

fn queued(fx: &Fixture) -> Vec<(String, String, String)> {
    queued_rows(fx)
        .into_iter()
        .map(|(c, t, e, _)| (c, t, e))
        .collect()
}

fn eyes() -> (String, String, String) {
    (CHANNEL.to_owned(), TS_RAW.to_owned(), "eyes".to_owned())
}

#[test]
fn claim_queues_eyes_on_the_post_with_the_caches_channel_and_ts_raw() {
    let (fx, id) = claimable();
    fx.ok(&["claim", &id.to_string()]);
    assert_eq!(queued(&fx), vec![eyes()]);
    assert_eq!(
        queued_rows(&fx)[0].3,
        None,
        "reacted_at is null until the helper succeeds"
    );
    let messages = fx.events("session-message");
    let text = messages[0]["data"]["text"].as_str().unwrap();
    assert!(text.contains(CHANNEL) && text.contains(TS_RAW), "{text}");
}

#[test]
fn a_refused_claim_queues_nothing() {
    let (mut fx, id) = claimable();
    assert_eq!(refusal(&fx, "999"), (4, "item_not_found".to_owned()));
    assert_eq!(
        refusal(&fx, &fx.id("unread").to_string()),
        (3, "not_claimable".to_owned())
    );
    write_cache(&fx, 7);
    assert_eq!(
        refusal(&fx, &id.to_string()),
        (3, "post_not_found".to_owned())
    );
    reconcile(&mut fx.store, &base(), CLOSED_Y).unwrap();
    assert_eq!(refusal(&fx, &id.to_string()), (3, "item_closed".to_owned()));
    assert!(queued(&fx).is_empty());
}

#[test]
fn claiming_an_already_claimed_item_queues_nothing_more() {
    let (fx, id) = claimable();
    let id = id.to_string();
    fx.ok(&["claim", &id]);
    assert_eq!(fx.ok(&["claim", &id])["already"], true);
    assert_eq!(queued(&fx), vec![eyes()]);
}

/// A Fixture whose store has the Inbox item and the cache but no `codereview` Context; the fake cockpit is up.
fn without_codereview() -> (Fixture, i64) {
    let mut fx = Fixture::new();
    fx.fx.env.write_config(&config(fx.fx.env.tmux_socket()));
    write_cache(&fx, 812);
    let mut all = base();
    all.push(review_request());
    reconcile(&mut fx.store, &all, T5).unwrap();
    let id = fx.store_count(&format!(
        "SELECT id FROM inbox_items WHERE subject = '{SUBJECT}'"
    ));
    fx.fx.cockpit();
    (fx, id)
}

fn new_windows(fx: &Fixture) -> usize {
    fx.fx
        .calls("tmux")
        .into_iter()
        .filter(|c| c.args.iter().any(|x| x == "new-window"))
        .count()
}

#[test]
fn claim_creates_the_codereview_context_and_manager_when_none_exists() {
    let (fx, id) = without_codereview();
    assert_eq!(
        fx.store_count("SELECT count(*) FROM contexts WHERE id = 'codereview'"),
        0
    );
    let out = fx.ok(&["claim", &id.to_string()]);
    assert_eq!(
        fx.store_count(
            "SELECT count(*) FROM contexts WHERE id = 'codereview' AND owner = 'clayton-duarte'"
        ),
        1
    );
    assert_eq!(
        fx.store_count(
            "SELECT count(*) FROM sessions WHERE role = 'manager' AND context = 'codereview'"
        ),
        1
    );
    let manager = out["manager"].as_str().unwrap();
    let messages = fx.events("session-message");
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0]["session"], manager);
    assert_eq!(messages[0]["context"], "codereview");
    assert_eq!(new_windows(&fx), 1, "the stopped Manager is started");
    let shown = fx.cpd(&["session", "show", manager]).ok();
    assert!(shown["session"]["tmux_pane"].is_string(), "{shown}");
}

#[test]
fn claim_gives_an_archived_codereview_manager_a_new_one() {
    let (mut fx, id) = without_codereview();
    sql(
        &mut fx,
        &format!(
            "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('codereview', 'codereview', 'clayton-duarte', 't', 'm1');
             INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at, archived_at)
             VALUES ('{MANAGER}', 'manager', 'codereview', NULL, NULL, 'codereview-manager', '/sessions/{MANAGER}.jsonl', '/repo', 'O5', NULL, 'stopped', NULL,
                     '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z');"
        ),
    );
    let out = fx.ok(&["claim", &id.to_string()]);
    let manager = out["manager"].as_str().unwrap();
    assert_ne!(manager, MANAGER);
    assert_eq!(
        fx.store_count(
            "SELECT count(*) FROM sessions WHERE role = 'manager' AND context = 'codereview' AND archived_at IS NULL"
        ),
        1
    );
    let messages = fx.events("session-message");
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0]["session"], manager);
    assert_eq!(
        fx.store_count(&format!(
            "SELECT count(*) FROM sessions WHERE id = '{MANAGER}' AND archived_at = '2026-01-03T00:00:00.000Z' AND state = 'stopped'"
        )),
        1,
        "the archived row is unchanged"
    );
}
