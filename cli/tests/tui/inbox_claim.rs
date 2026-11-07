//! `cpd inbox claim <id>`, the fixed `codereview` target and the `hidden` object's `claimed` count (commands.md §21.7,
//! §21.1; contract §8.2): a temporary `$CPD_HOME`, the Inbox fixture of `inbox.rs`, a fake tmux.

use crate::inbox::{CLOSED_Y, Fixture, condition};
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
    format!(
        "version = 1\nmachine = \"m1\"\nlead_tier = \"S4\"\nlead_runner = \"pi\"\n[tmux]\nsocket = \"{socket}\"\n"
    )
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
        head_oid: None,
        reviewed_oid: None,
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
             VALUES ('{MANAGER}', 'manager', '{id}', NULL, NULL, '{id}-manager', '/sessions/{MANAGER}.jsonl', '/repo', 'O4', NULL, '{state}', NULL,
                     '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL);"
        ),
    );
}

/// The post of `acme/pay#<number>` in `swr_slack_posts`, the only one.
fn write_cache(fx: &Fixture, number: u64) {
    seed_post(&fx.fx, CHANNEL, "acme/pay", number, TS_RAW);
}

/// The `slack` source's rows for one primary post: the channel's query has data and one `swr_slack_posts` row.
fn seed_post(env: &crate::common::fx::Fx, channel: &str, repo: &str, number: u64, ts_raw: &str) {
    env.env.sql(&format!(
        "DELETE FROM swr_slack_posts; DELETE FROM swr_queries WHERE key LIKE 'slack:channel:%';
         INSERT INTO swr_queries (key, system, status, data_hash) VALUES ('slack:channel:{channel}', 'slack', 'success', 'h');
         INSERT INTO swr_slack_posts (channel, ts, repo, number, primary_pr, author_id, author, author_is_me, reactions_json, claim_json, eyes_me, thread_tags_me, fetched_at)
           VALUES ('{channel}', '{ts_raw}', '{repo}', {number}, 1, '', 'Ryan Case', 0, '{{}}', NULL, NULL, 0, 't');"
    ));
}

/// The single pass's rows for `repo#number`, from the GraphQL answer GitHub would have given (alias `p0`, the viewer): the
/// viewer, the raw row, and the detail with the reviews and the head's checks.
pub(super) fn seed(fx: &Fixture, repo: &str, number: u64, answer: &str) {
    let reply: Value = serde_json::from_str(answer).unwrap();
    let me = reply["data"]["viewer"]["login"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut sql = format!(
        "INSERT INTO swr_viewer (id, login, fetched_at) VALUES (1, '{me}', 't') ON CONFLICT (id) DO UPDATE SET login = excluded.login;
         DELETE FROM pull_requests WHERE repo = '{repo}' AND number = {number};"
    );
    if !reply["data"]["p0"].is_null() {
        let raw = &reply["data"]["p0"]["pullRequest"];
        let state = match raw["state"].as_str().unwrap() {
            "OPEN" => "open",
            "MERGED" => "merged",
            _ => "closed",
        };
        let decision = match raw["reviewDecision"].as_str() {
            Some("APPROVED") => "approved",
            Some("CHANGES_REQUESTED") => "changes_requested",
            _ => "none",
        };
        let head = raw["headRefOid"].as_str();
        let head_sql = head.map_or("NULL".to_owned(), |h| format!("'{h}'"));
        let authored = raw["author"]["login"].as_str() == Some(me.as_str());
        let detail = json!({
            "latestReviews": raw["latestReviews"].clone(),
            "commits": {"nodes": [{"commit": {"oid": head, "statusCheckRollup": {"state": raw["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"].clone(), "contexts": {"nodes": []}}}}]},
            "reviewThreads": {"nodes": []}, "comments": {"nodes": []}
        });
        sql.push_str(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored, review_requested, head_oid, merged_at)
               VALUES ('{repo}', {number}, 'https://github.com/{repo}/pull/{number}', '{}', {}, '{state}', '{decision}', 'none', 't', 't', {}, 1, {head_sql}, {});
             INSERT OR REPLACE INTO swr_pr_detail (repo, number, head_oid, updated_at, hydrated_at, detail_json, detail_hash)
               VALUES ('{repo}', {number}, {head_sql}, 'U', 't', '{detail}', 'h');",
            raw["title"].as_str().unwrap(),
            i32::from(raw["isDraft"].as_bool().unwrap()),
            i32::from(authored),
            if state == "merged" { "'t'" } else { "NULL" }
        ));
    }
    fx.fx.env.sql(&sql);
}

/// The Fixture with an open `review-requested` item for `acme/pay#812`, the Context `codereview` with an idle live
/// Manager and the post in the cache. Returns the item's id.
fn claimable() -> (Fixture, i64) {
    claimable_with(NEEDS_REVIEW)
}

/// [`claimable`] with the pass's rows as the GraphQL answer `gh` says (the cache the claim's live check reads).
fn claimable_with(gh: &str) -> (Fixture, i64) {
    let mut fx = Fixture::new();
    seed(&fx, "acme/pay", 812, gh);
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

/// The GraphQL answer of a pull request nobody reviewed.
const NEEDS_REVIEW: &str = r#"{"data":{"viewer":{"login":"clayton-duarte"},"p0":{"pullRequest":{"title":"Pay the bills","isDraft":false,"state":"OPEN","reviewDecision":null,"author":{"login":"ryan"},"latestReviews":{"nodes":[]}}}}}"#;

/// The same shape for the one pull request (alias `p0`); the viewer stays the User.
fn gh_answer(
    state: &str,
    draft: bool,
    decision: Option<&str>,
    author: &str,
    reviews: &[(&str, &str)],
) -> String {
    let nodes: Vec<Value> = reviews
        .iter()
        .map(|(state, login)| json!({"state": state, "author": {"login": login}}))
        .collect();
    json!({"data": {"viewer": {"login": "clayton-duarte"}, "p0": {"pullRequest": {
        "title": "Pay the bills", "isDraft": draft, "state": state, "reviewDecision": decision,
        "author": {"login": author}, "latestReviews": {"nodes": nodes}}}}})
    .to_string()
}

/// A refusal wrote nothing: no Event, no message, no `claimed_at`.
fn assert_nothing_written(fx: &Fixture) {
    assert!(fx.events("inbox-claimed").is_empty());
    assert!(fx.events("session-message").is_empty());
    assert_eq!(
        fx.store_count("SELECT count(*) FROM inbox_items WHERE claimed_at IS NOT NULL"),
        0
    );
    assert_eq!(fx.store_count("SELECT count(*) FROM plans"), 0);
    assert_eq!(
        fx.store_count("SELECT count(*) FROM sessions WHERE role = 'lead'"),
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
    assert!(fx.events("session-message").is_empty());
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
    // a closed one: the pull request's condition ended; claiming it is the no-op of §21's intro (air-d26)
    reconcile(&mut fx.store, &base(), CLOSED_Y).unwrap();
    let txns = fx.store_count("SELECT count(*) FROM txns");
    let out = fx.ok(&["claim", &id.to_string()]);
    assert_eq!(out["already"], "closed");
    assert_eq!(out["item"]["closed_at"], CLOSED_Y);
    // No Event and no transaction was written.
    assert_eq!(fx.store_count("SELECT count(*) FROM txns"), txns);
    assert_nothing_written(&fx);
}

#[test]
fn claim_refuses_when_the_slack_post_is_not_in_the_cache_and_writes_nothing() {
    let (fx, id) = claimable();
    write_cache(&fx, 7);
    assert_eq!(
        refusal(&fx, &id.to_string()),
        (3, "post_not_found".to_owned()),
        "a cache without a post of this pull request"
    );
    fx.fx.env.sql(
        "DELETE FROM swr_slack_posts; DELETE FROM swr_queries WHERE key LIKE 'slack:channel:%';",
    );
    assert_eq!(
        refusal(&fx, &id.to_string()),
        (3, "post_not_found".to_owned()),
        "no post at all"
    );
    assert_nothing_written(&fx);
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

/// `(channel, ts, emoji)` of every Slack reaction mutation on the outbox, oldest first, plus its `reacted_at` (the completion
/// time once it is done). Empty while the outbox has no such mutation.
fn queued_rows(fx: &Fixture) -> Vec<(String, String, String, Option<String>)> {
    let home = cpd_core::home::CpdHome::resolve(Some(fx.fx.env.cpd_home().into()), None).unwrap();
    cpd_core::store::Store::open(&home, 5000)
        .unwrap()
        .read(|c| {
            let mut s = c.prepare(
                "SELECT json_extract(vars_json, '$.channel'), json_extract(vars_json, '$.ts'), json_extract(vars_json, '$.emoji'),
                        IIF(status IN ('revalidating', 'done'), completed_at, NULL)
                   FROM swr_mutations WHERE system = 'slack' AND op_kind = 'reaction' ORDER BY id",
            )?;
            Ok(s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap()
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
    // A closed item's claim is the no-op of §21's intro (air-d26): it exits 0 with `already: "closed"`.
    let txns = fx.store_count("SELECT count(*) FROM txns");
    let out = fx.ok(&["claim", &id.to_string()]);
    assert_eq!(out["already"], "closed");
    assert_eq!(out["item"]["closed_at"], CLOSED_Y);
    // No Event and no transaction was written, so nothing is queued either.
    assert_eq!(fx.store_count("SELECT count(*) FROM txns"), txns);
    assert!(fx.events("inbox-claimed").is_empty());
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
    without_codereview_with(NEEDS_REVIEW)
}

/// [`without_codereview`] with the pass's rows as the GraphQL answer `gh` says.
fn without_codereview_with(gh: &str) -> (Fixture, i64) {
    let mut fx = Fixture::new();
    seed(&fx, "acme/pay", 812, gh);
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
    assert!(fx.events("session-message").is_empty());
    assert_eq!(new_windows(&fx), 1, "only the Lead's window is started");
    let shown = fx.cpd(&["session", "show", manager]).ok();
    assert_eq!(shown["session"]["tmux_pane"], Value::Null, "{shown}");
}

#[test]
fn claim_gives_an_archived_codereview_manager_a_new_one() {
    let (mut fx, id) = without_codereview();
    sql(
        &mut fx,
        &format!(
            "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('codereview', 'codereview', 'clayton-duarte', 't', 'm1');
             INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at, archived_at)
             VALUES ('{MANAGER}', 'manager', 'codereview', NULL, NULL, 'codereview-manager', '/sessions/{MANAGER}.jsonl', '/repo', 'O4', NULL, 'stopped', NULL,
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
    assert!(fx.events("session-message").is_empty());
    assert_eq!(
        fx.store_count(&format!(
            "SELECT count(*) FROM sessions WHERE id = '{MANAGER}' AND archived_at = '2026-01-03T00:00:00.000Z' AND state = 'stopped'"
        )),
        1,
        "the archived row is unchanged"
    );
}

/// The claim of `id` is refused `not_claimable` for `reason`, and the item is closed, never claimed.
fn assert_stale(fx: &Fixture, id: i64, reason: &str) {
    let (exit, error) = fx.fail(&["claim", &id.to_string()]);
    assert_eq!(exit, 3, "{reason}: {error}");
    assert_eq!(error["code"], "not_claimable", "{reason}: {error}");
    assert_eq!(error["details"]["reason"], reason, "{error}");
    assert_eq!(error["details"]["item"], id, "{error}");
    assert_eq!(
        fx.store_count(&format!(
            "SELECT count(*) FROM inbox_items WHERE id = {id} AND closed_at IS NOT NULL AND claimed_at IS NULL"
        )),
        1,
        "{reason}: closed, not claimed"
    );
    let closed: Vec<Value> = fx
        .events("inbox-closed")
        .into_iter()
        .filter(|e| e["data"]["item"] == id)
        .collect();
    assert_eq!(closed.len(), 1, "{reason}: {closed:?}");
    assert!(fx.events("inbox-claimed").is_empty(), "{reason}");
    assert!(fx.events("session-message").is_empty(), "{reason}");
    assert!(queued(fx).is_empty(), "{reason}");
}

#[test]
fn claim_refuses_a_merged_pull_request_and_closes_the_item() {
    let (fx, id) = claimable_with(&gh_answer("MERGED", false, None, "ryan", &[]));
    assert_stale(&fx, id, "merged");
}

#[test]
fn claim_refuses_a_closed_pull_request_and_closes_the_item() {
    let (fx, id) = claimable_with(&gh_answer("CLOSED", false, None, "ryan", &[]));
    assert_stale(&fx, id, "closed");
}

#[test]
fn claim_refuses_an_approved_pull_request_and_closes_the_item() {
    let (fx, id) = claimable_with(&gh_answer("OPEN", false, Some("APPROVED"), "ryan", &[]));
    assert_stale(&fx, id, "approved");
}

#[test]
fn claim_refuses_every_other_status_that_is_not_needs_review_with_its_reason() {
    let not_found = r#"{"data":{"viewer":{"login":"clayton-duarte"},"p0":null}}"#.to_owned();
    let cases = [
        (gh_answer("OPEN", true, None, "ryan", &[]), "draft"),
        (
            gh_answer("OPEN", false, Some("CHANGES_REQUESTED"), "ryan", &[]),
            "changes-requested",
        ),
        (
            gh_answer("OPEN", false, None, "ryan", &[("COMMENTED", "bea")]),
            "in-review",
        ),
        (gh_answer("OPEN", false, None, "clayton-duarte", &[]), "own"),
        (not_found, "not-found"),
    ];
    for (answer, reason) in cases {
        let (fx, id) = claimable_with(&answer);
        assert_stale(&fx, id, reason);
    }
}

#[test]
fn a_stale_claim_writes_only_the_item_close() {
    let merged = gh_answer("MERGED", false, None, "ryan", &[]);
    let (fx, id) = claimable_with(&merged);
    let counts = |fx: &Fixture| {
        [
            "SELECT count(*) FROM contexts",
            "SELECT count(*) FROM sessions",
            "SELECT count(*) FROM events WHERE kind <> 'inbox-closed'",
        ]
        .map(|q| fx.store_count(q))
    };
    let before = counts(&fx);
    let reactions = queued_rows(&fx).len();
    let (exit, error) = fx.fail(&["claim", &id.to_string()]);
    assert_eq!((exit, error["code"].as_str()), (3, Some("not_claimable")));
    assert_eq!(counts(&fx), before);
    assert_eq!(queued_rows(&fx).len(), reactions);
    assert!(fx.events("session-message").is_empty());
    // The item is now closed: a second claim is the no-op of §21's intro (air-d26), writing nothing.
    let closed_at = fx.ok(&["show", &id.to_string()])["item"]["closed_at"].clone();
    assert!(closed_at.is_string(), "{closed_at}");
    let txns = fx.store_count("SELECT count(*) FROM txns");
    let out = fx.ok(&["claim", &id.to_string()]);
    assert_eq!(out["already"], "closed");
    assert_eq!(out["item"]["closed_at"], closed_at);
    assert_eq!(counts(&fx), before);
    assert_eq!(queued_rows(&fx).len(), reactions);
    assert_eq!(fx.store_count("SELECT count(*) FROM txns"), txns);
}

#[test]
fn a_stale_claim_creates_no_context_and_starts_no_window() {
    let merged = gh_answer("MERGED", false, None, "ryan", &[]);
    // no `codereview` Context: the stale claim creates none and starts nothing
    let (fx, id) = without_codereview_with(&merged);
    assert_eq!(
        refusal(&fx, &id.to_string()),
        (3, "not_claimable".to_owned())
    );
    assert_eq!(
        fx.store_count("SELECT count(*) FROM contexts WHERE id = 'codereview'"),
        0
    );
    assert_eq!(new_windows(&fx), 0);
}

#[test]
fn claim_reads_the_cache_and_asks_github_nothing() {
    let (fx, id) = claimable();
    let id = id.to_string();
    let out = fx.ok(&["claim", &id]);
    assert_eq!(out["ok"], true);
    assert!(fx.fx.calls("gh").is_empty(), "{:?}", fx.fx.calls("gh"));

    assert_eq!(fx.ok(&["claim", &id])["already"], true);
    assert!(fx.fx.calls("gh").is_empty(), "no gh call");
}

// ---- the review Plan and its Lead (commands.md §21.7, FR10) ----

const PLAN: &str = "r-pay-812";
const TITLE: &str = "Review pay#812 Pay the bills";
const POINTER: &str = "Context on demand: cpd context search <query> · cpd context show --since <seq> · cpd context show --record <id>";

/// [`claimable`] for the pull request `<repo>#<number>` (owner `missionlane` or `acme`), the fake cockpit up.
fn claimable_pr(owner: &str, repo: &str, number: u64) -> (Fixture, i64) {
    let subject = format!("pr:{owner}/{repo}#{number}");
    let mut fx = Fixture::new();
    seed(&fx, &format!("{owner}/{repo}"), number, NEEDS_REVIEW);
    fx.fx.env.write_config(&config(fx.fx.env.tmux_socket()));
    codereview_context(&mut fx, "idle");
    seed_post(&fx.fx, CHANNEL, &format!("{owner}/{repo}"), number, TS_RAW);
    let mut all = base();
    let mut item = review_request();
    item.subject = subject.clone();
    item.label = format!("{owner}/{repo}#{number} review requested");
    item.url = Some(format!("https://github.com/{owner}/{repo}/pull/{number}"));
    all.push(item);
    reconcile(&mut fx.store, &all, T5).unwrap();
    let id = fx.store_count(&format!(
        "SELECT id FROM inbox_items WHERE subject = '{subject}'"
    ));
    fx.fx.cockpit();
    (fx, id)
}

fn text(fx: &Fixture, query: &str) -> String {
    fx.fx.env.query::<String>(query)
}

/// The ids of every Plan, sorted.
fn plan_ids(fx: &Fixture) -> Vec<String> {
    fx.fx.env.read(|c| {
        let mut s = c.prepare("SELECT id FROM plans ORDER BY id")?;
        s.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()
    })
}

/// A Plan row `id` with its Lead Session, as other fixtures insert them; `extra` is the Plan's `, archived_at`/`, deleted_at` value list.
fn old_plan(fx: &mut Fixture, id: &str, column: &str, value: &str) {
    let (cols, vals) = if column.is_empty() {
        (String::new(), String::new())
    } else {
        (format!(", {column}"), format!(", '{value}'"))
    };
    sql(
        fx,
        &format!(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead-{id}', 'lead', 'codereview', '{id}', NULL, '{id}', '/repo', 'O2', 'stopped', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at{cols})
               VALUES ('{id}', 'codereview', 'Old', 'grounding', 'lead-{id}', 't'{vals});"
        ),
    );
}

fn new_window_names(fx: &Fixture) -> Vec<String> {
    fx.fx
        .calls("tmux")
        .into_iter()
        .map(|c| c.args)
        .filter(|a| a.iter().any(|x| x == "new-window" || x == "new-session"))
        .flatten()
        .filter(|a| a.starts_with("park-"))
        .collect()
}

#[test]
fn claim_creates_the_review_plan_and_lead_and_wakes_nobody() {
    let (fx, id) = claimable();
    fx.fx.cockpit();
    let manager_before = text(
        &fx,
        &format!(
            "SELECT json_object('s', state, 'p', tmux_pane, 'a', archived_at) FROM sessions WHERE id = '{MANAGER}'"
        ),
    );
    let rows_before = fx.store_count("SELECT count(*) FROM sessions WHERE role = 'manager'");
    fx.ok(&["claim", &id.to_string()]);
    assert_eq!(plan_ids(&fx), vec![PLAN.to_owned()]);
    assert_eq!(
        fx.store_count(&format!(
            "SELECT count(*) FROM plans p JOIN sessions s ON s.id = p.lead_session AND s.plan = p.id
             WHERE p.id = '{PLAN}' AND p.context = 'codereview' AND p.phase = 'grounding'
               AND s.role = 'lead' AND s.context = 'codereview' AND s.by_user = 0 AND s.tier = 'S4' AND s.runner = 'pi'"
        )),
        1
    );
    for kind in ["inbox-claimed", "plan-created", "session-registered"] {
        assert!(!fx.events(kind).is_empty(), "{kind}");
    }
    assert!(fx.events("session-message").is_empty());
    assert_eq!(
        text(
            &fx,
            &format!(
                "SELECT json_object('s', state, 'p', tmux_pane, 'a', archived_at) FROM sessions WHERE id = '{MANAGER}'"
            ),
        ),
        manager_before,
        "a live idle Manager stays idle"
    );
    assert_eq!(
        fx.store_count("SELECT count(*) FROM sessions WHERE role = 'manager'"),
        rows_before
    );
}

#[test]
fn claim_review_plan_id_and_title_come_from_the_repo_the_number_and_the_pr_title() {
    let (fx, id) = claimable();
    fx.ok(&["claim", &id.to_string()]);
    assert_eq!(plan_ids(&fx), vec![PLAN.to_owned()]);
    assert_eq!(
        text(&fx, &format!("SELECT title FROM plans WHERE id = '{PLAN}'")),
        TITLE
    );
}

#[test]
fn claim_review_plan_id_uses_the_known_short_slugs() {
    for (repo, slug) in [
        ("account-management-mobile-app", "mobile"),
        ("cc-api-orc-apollo", "orc"),
        ("card-dashboard", "cd"),
        ("snowflake-analytics", "sfa"),
        ("compass-ui", "cui"),
    ] {
        let (fx, id) = claimable_pr("missionlane", repo, 77);
        fx.ok(&["claim", &id.to_string()]);
        assert_eq!(plan_ids(&fx), vec![format!("r-{slug}-77")], "{repo}");
    }
}

#[test]
fn claim_review_plan_id_takes_the_next_free_suffix() {
    {
        let (mut fx, id) = claimable();
        old_plan(&mut fx, PLAN, "", "");
        fx.ok(&["claim", &id.to_string()]);
        assert_eq!(
            plan_ids(&fx),
            vec![PLAN.to_owned(), "r-pay-812-2".to_owned()]
        );
    }

    let (mut fx, id) = claimable();
    old_plan(&mut fx, PLAN, "", "");
    old_plan(
        &mut fx,
        "r-pay-812-2",
        "archived_at",
        "2026-01-02T00:00:00.000Z",
    );
    old_plan(
        &mut fx,
        "r-pay-812-3",
        "deleted_at",
        "2026-01-02T00:00:00.000Z",
    );
    fx.ok(&["claim", &id.to_string()]);
    assert_eq!(
        text(
            &fx,
            "SELECT max(id) FROM plans WHERE id LIKE 'r-pay-812-%' AND id NOT IN ('r-pay-812-2', 'r-pay-812-3')"
        ),
        "r-pay-812-4"
    );
}

#[test]
fn claim_review_plan_id_fits_24_characters() {
    let grammar = |id: &str| {
        id.len() <= 24
            && id.starts_with(|c: char| c.is_ascii_lowercase())
            && id.len() >= 2
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    {
        let (fx, id) = claimable_pr("acme", "a-very-long-repository-name-indeed", 123_456);
        fx.ok(&["claim", &id.to_string()]);
        assert_eq!(plan_ids(&fx), vec!["r-a-very-long-rep-123456".to_owned()]);
        assert!(grammar(&plan_ids(&fx)[0]));
    }

    let (mut fx, id) = claimable_pr("acme", "a-very-long-repository-name-indeed", 123_456);
    old_plan(&mut fx, "r-a-very-long-rep-123456", "", "");
    fx.ok(&["claim", &id.to_string()]);
    let ids = plan_ids(&fx);
    assert!(
        ids.contains(&"r-a-very-long-r-123456-2".to_owned()),
        "{ids:?}"
    );
    assert!(ids.iter().all(|i| grammar(i)), "{ids:?}");
}

fn brief(home: &str) -> String {
    format!(
        "Review PR {URL} (`acme/pay#812`).
Slack post: `{CHANNEL}/{TS_RAW}` (`--slack {CHANNEL}/{TS_RAW}`).

Read `{home}/cpd-codereview/cpd-code-review/SKILL.md` (v10, `pro-d6`) first and follow it strictly:
1. Turn 1: run `~/cpd-codereview/cpd-code-review/prefetch.sh \"{URL}\" \"/tmp/$CPD_PLAN\"`.
2. Never run `cpd context show --context codereview`; never message any Manager or Lead (`cpd session send` / `cpd board post`).
3. Use `{home}/projects/pay` (`git show`/`git grep`) and `H2` `scout` Jobs (`cpd job add --kind scout --tier H2`) if wide/cross-repo/multi-branch exploration is needed.
4. After the User answers the `Verdict` `ask_user_question`, run 1 bash call (`cpd review submit ... --slack {CHANNEL}/{TS_RAW} && cpd context note ...`) and stop: after that call your turn is complete, so emit no post-submit report or message.
5. Begin the `Verdict` `ask_user_question` with the full PR URL `{URL}`: the question's first words are `{URL}`."
    )
}

#[test]
fn claim_writes_the_review_brief_record_and_the_lead_first_message() {
    let (fx, id) = claimable();
    fx.ok(&["claim", &id.to_string()]);
    let home = fx.fx.env.home().display().to_string();
    let brief = brief(&home);
    assert_eq!(
        text(
            &fx,
            &format!(
                "SELECT text FROM context_records WHERE context = 'codereview' AND kind = 'brief' AND plan_id = '{PLAN}'"
            )
        ),
        brief
    );
    assert_eq!(
        text(
            &fx,
            &format!(
                "SELECT text FROM context_records WHERE context = 'codereview' AND kind = 'plan' AND plan_id = '{PLAN}'"
            )
        ),
        TITLE
    );
    let file = fs::read_to_string(
        fx.fx
            .env
            .cpd_home()
            .join(format!("run/{PLAN}/lead-message.md")),
    )
    .unwrap();
    let header = format!(
        "You are the Lead of Plan {PLAN} \"{TITLE}\" in Context codereview. Phase grounding: follow the cpd-lead skill."
    );
    assert_eq!(
        file.trim_end_matches('\n'),
        format!("{header}\n\n{brief}\n\n{POINTER}")
    );
}

#[test]
fn claim_starts_the_review_lead_window_and_still_queues_eyes() {
    let (fx, id) = claimable();
    fx.fx.cockpit();
    fx.ok(&["claim", &id.to_string()]);
    assert_eq!(new_window_names(&fx), vec![format!("park-{PLAN}")]);
    assert_eq!(queued(&fx), vec![eyes()]);
}

#[test]
fn claim_output_names_item_plan_lead_and_manager() {
    let (fx, id) = claimable();
    let out = fx.ok(&["claim", &id.to_string()]);
    assert!(out["item"]["claimed_at"].is_string(), "{out}");
    assert_eq!(out["plan"], PLAN);
    assert_eq!(out["manager"], MANAGER);
    let lead = text(
        &fx,
        &format!("SELECT lead_session FROM plans WHERE id = '{PLAN}'"),
    );
    assert_eq!(out["lead"], lead.as_str(), "{out}");
}

#[test]
fn claim_twice_creates_one_review_plan() {
    let (fx, id) = claimable();
    fx.fx.cockpit();
    let id = id.to_string();
    fx.ok(&["claim", &id]);
    assert_eq!(fx.ok(&["claim", &id])["already"], true);
    assert_eq!(plan_ids(&fx), vec![PLAN.to_owned()]);
    assert_eq!(
        fx.store_count("SELECT count(*) FROM sessions WHERE role = 'lead'"),
        1
    );
    assert_eq!(new_window_names(&fx), vec![format!("park-{PLAN}")]);
}

#[test]
fn claim_leaves_a_stopped_codereview_manager_stopped_and_unmessaged() {
    let mut fx = Fixture::new();
    seed(&fx, "acme/pay", 812, NEEDS_REVIEW);
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
    assert_eq!(out["plan"], PLAN, "{out}");
    let manager_windows = fx
        .fx
        .calls("tmux")
        .into_iter()
        .filter(|c| c.args.iter().any(|x| x == MANAGER))
        .count();
    assert_eq!(manager_windows, 0, "no pane started for the Manager");
    let shown = fx.cpd(&["session", "show", MANAGER]).ok();
    assert_eq!(shown["session"]["state"], "stopped", "{shown}");
    assert_eq!(shown["session"]["tmux_pane"], Value::Null, "{shown}");
    assert!(fx.events("session-message").is_empty());
}
