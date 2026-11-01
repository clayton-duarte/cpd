//! The closed-pull-request wake step: one message from null with wake true to an idle Lead per (Workflow, pull request number).

use serde_json::Value;

use super::closed;
use super::testkit::Fx;

fn text(key: &str, n: i64) -> String {
    format!(
        "Workflow {key} of Plan pay is open in cpd but its pull request #{n} was closed without a merge. https://github.com/acme/shop/pull/{n} \
         A person or GitHub closed it (for example GitHub does when a stacked pull request's base branch is deleted). \
         cpd has not cancelled the Workflow and will not. Decide: ask the User, reopen the pull request on GitHub, or give the Workflow up with \
         `cpd workflow cancel --workflow {key}`."
    )
}

/// Pull request `n` of acme/shop in `state`, with the id `n`.
fn pull_request(fx: &mut Fx, n: i64, state: &str) {
    let merged = if state == "merged" { "'t'" } else { "NULL" };
    fx.fixture(&format!(
        "INSERT INTO pull_requests (id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ({n}, 'acme/shop', {n}, 'https://github.com/acme/shop/pull/{n}', 'T{n}', 0, '{state}', 'none', 'none', 't', {merged}, 't');"
    ));
}

/// Workflow `key` of Plan `pay`, `open` on pull request `n`.
fn open_pr(fx: &mut Fx, key: &str, n: i64) {
    fx.fixture(&format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                          created_txn, created_at, started_at)
           VALUES ('pay', '{key}', '/r', 'main', 'open', 'cpd/pay/{key}', '/w/{key}', {n}, 'https://github.com/acme/shop/pull/{n}', 't', {n},
                   cpd_txn(), 't', 't');"
    ));
}

/// A fixture with an idle Lead and Workflow `ship` open on the closed pull request #7.
fn fixture() -> Fx {
    let mut fx = Fx::with_plan("");
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    pull_request(&mut fx, 7, "closed");
    open_pr(&mut fx, "ship", 7);
    fx
}

/// The `session-message` Events: `(session, from, wake, text)`.
fn messages(fx: &mut Fx) -> Vec<(Option<String>, Value, Value, String)> {
    fx.events("session-message")
        .into_iter()
        .map(|(_, _, _, d)| (None, d["from"].clone(), d["wake"].clone(), d["text"].as_str().unwrap_or_default().to_owned()))
        .collect()
}

fn texts(fx: &mut Fx) -> Vec<String> {
    messages(fx).into_iter().map(|m| m.3).collect()
}

#[test]
fn an_idle_lead_is_told_once_when_its_open_pr_has_a_closed_pull_request() {
    let mut fx = fixture();
    closed::run(&mut fx.engine);
    let sent = messages(&mut fx);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!((sent[0].1.clone(), sent[0].2.clone()), (Value::Null, Value::Bool(true)));
    assert_eq!(fx.query::<String>("SELECT session FROM events WHERE kind = 'session-message'"), "lead1");
}

#[test]
fn the_message_is_the_wake_text() {
    let mut fx = fixture();
    closed::run(&mut fx.engine);
    assert_eq!(texts(&mut fx), [text("ship", 7)]);
}

#[test]
fn a_second_tick_sends_nothing_more() {
    let mut fx = fixture();
    closed::run(&mut fx.engine);
    closed::run(&mut fx.engine);
    assert_eq!(texts(&mut fx), [text("ship", 7)]);
}

#[test]
fn a_new_pull_request_number_of_the_same_pr_wakes_the_lead_again() {
    let mut fx = fixture();
    closed::run(&mut fx.engine);
    pull_request(&mut fx, 8, "closed");
    fx.fixture(
        "UPDATE prs SET pr_number = 8, pr_url = 'https://github.com/acme/shop/pull/8', pull_request = 8 WHERE plan = 'pay' AND key = 'ship';",
    );
    closed::run(&mut fx.engine);
    closed::run(&mut fx.engine);
    assert_eq!(texts(&mut fx), [text("ship", 7), text("ship", 8)]);
}

#[test]
fn a_busy_lead_is_told_when_it_goes_idle() {
    let mut fx = fixture();
    fx.fixture("UPDATE sessions SET state = 'working' WHERE id = 'lead1';");
    closed::run(&mut fx.engine);
    assert_eq!(texts(&mut fx), Vec::<String>::new(), "a busy Lead is not told");
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    closed::run(&mut fx.engine);
    closed::run(&mut fx.engine);
    assert_eq!(texts(&mut fx), [text("ship", 7)]);
}

#[test]
fn a_cancelled_pr_or_an_open_pull_request_sends_nothing() {
    let mut fx = Fx::with_plan("");
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    pull_request(&mut fx, 9, "open");
    open_pr(&mut fx, "live", 9);
    fx.fixture(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'gone', '/r', 'main', 'cancelled', 'cpd/pay/gone', cpd_txn(), 't');",
    );
    closed::run(&mut fx.engine);
    assert_eq!(texts(&mut fx), Vec::<String>::new());
    // A Workflow that was open on a closed pull request and is then cancelled sends nothing either.
    pull_request(&mut fx, 7, "closed");
    open_pr(&mut fx, "ship", 7);
    fx.fixture("UPDATE prs SET status = 'cancelled', lead_worktree = NULL, started_at = NULL, pr_number = NULL, pr_url = NULL, pr_opened_at = NULL, pull_request = NULL WHERE key = 'ship';");
    closed::run(&mut fx.engine);
    assert_eq!(texts(&mut fx), Vec::<String>::new());
    // Only a Workflow that is open on a closed pull request is told of.
    pull_request(&mut fx, 10, "closed");
    open_pr(&mut fx, "real", 10);
    closed::run(&mut fx.engine);
    assert_eq!(texts(&mut fx), [text("real", 10)]);
}

#[test]
fn two_closed_prs_of_one_plan_send_one_message_each() {
    let mut fx = fixture();
    pull_request(&mut fx, 8, "closed");
    open_pr(&mut fx, "api", 8);
    closed::run(&mut fx.engine);
    closed::run(&mut fx.engine);
    assert_eq!(texts(&mut fx), [text("api", 8), text("ship", 7)]);
}

#[test]
fn a_held_plan_sends_nothing() {
    let mut fx = fixture();
    fx.fixture("INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, 't', 'mgr', 'why', cpd_txn());");
    closed::run(&mut fx.engine);
    assert_eq!(texts(&mut fx), Vec::<String>::new());
    // Red until the step exists: once the incident ends, the Lead is told.
    fx.fixture("DELETE FROM incident;");
    closed::run(&mut fx.engine);
    assert_eq!(texts(&mut fx), [text("ship", 7)]);
}
