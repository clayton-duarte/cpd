//! The Inbox's core conditions (contract §2.2 "Inbox item", §3.7 E8) computed from a store the built binary wrote:
//! a Plan's Lead that blocks, a Context's questions and answers, and the pull requests the poll wrote.

mod common;

use std::fs;
use std::time::Duration;

use common::{CPD, SLEEP_SECONDS, TestEnv};
use cpd_core::home::CpdHome;
use cpd_core::inbox::model::{Condition, Kind, Urgency};
use cpd_core::inbox::rules::{EngineError, conditions};
use cpd_core::store::Store;
use time::OffsetDateTime;

/// The env with its config, a fake `cpd` for the Lead's pane, Context `ctx` and Plan `pay` (in `grounding`).
/// Returns the Lead's Session id.
fn setup(env: &TestEnv) -> String {
    let config = format!("version = 1\nmachine = \"hs\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket());
    fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    env.fake(
        "cpd",
        &format!(
            "case \"$1 $2\" in\n\"session run\") exec sleep {SLEEP_SECONDS};;\n\"watch --ensure\") echo '{{\"ok\": true, \"started\": true, \"pane\": \"%9\"}}'; exit 0;;\nesac\nexec '{CPD}' \"$@\""
        ),
    );
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    let plan = env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Pay"]).ok();
    plan["plan"]["lead_session"].as_str().unwrap().to_owned()
}

fn found(env: &TestEnv, errors: &[EngineError]) -> Vec<Condition> {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    conditions(&mut store, &home, OffsetDateTime::now_utc(), Duration::from_secs(600), errors).unwrap()
}

#[test]
fn a_blocked_lead_in_grounding_is_a_magenta_dialog_until_it_works_again() {
    let env = TestEnv::new();
    let lead = setup(&env);
    assert!(found(&env, &[]).is_empty());

    env.cpd(&["session", "state", "idle", "--session", &lead]).ok();
    env.cpd(&["session", "state", "blocked", "--label", "Which repo?", "--session", &lead]).ok();
    let items = found(&env, &[]);
    assert_eq!(items.len(), 1, "{items:?}");
    let c = &items[0];
    assert_eq!((c.kind, c.urgency), (Kind::Dialog, Urgency::Magenta));
    assert_eq!((c.subject.clone(), c.label.as_str()), (format!("session:{lead}"), "pay asks you"));
    let latest: i64 = env.cpd(&["events", "list", "--kind", "session-state", "--session", &lead]).ok()["events"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    assert_eq!(c.event, Some(latest), "its latest session-state Event");

    env.cpd(&["session", "state", "working", "--session", &lead]).ok();
    assert!(found(&env, &[]).is_empty());
}

#[test]
fn a_question_is_an_item_until_its_answer_is_recorded() {
    let env = TestEnv::new();
    setup(&env);
    env.cpd(&["context", "ask", "Why?", "--context", "ctx"]).ok();
    let items = found(&env, &[]);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!((items[0].kind, items[0].urgency), (Kind::Question, Urgency::Magenta));
    assert_eq!((items[0].subject.as_str(), items[0].label.as_str()), ("record:ctx/hs-q1", "hs-q1 open question"));

    env.cpd(&["context", "answer", "hs-q1", "Because.", "--context", "ctx"]).ok();
    assert!(found(&env, &[]).is_empty());
}

#[test]
fn an_engine_error_is_an_item_while_the_engine_reports_it() {
    let env = TestEnv::new();
    setup(&env);
    let error = EngineError { step: "E5".into(), scope: "machine".into(), code: "gh_rate_limited".into(), event: 7 };
    let items = found(&env, std::slice::from_ref(&error));
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!((items[0].kind, items[0].urgency), (Kind::EngineError, Urgency::Red));
    assert_eq!((items[0].subject.as_str(), items[0].label.as_str()), ("error:E5/machine/gh_rate_limited", "engine E5: gh_rate_limited"));
    assert_eq!(items[0].event, Some(7));
    assert!(found(&env, &[]).is_empty());
}

/// Writes a pull request row as the poll would, with the columns the Inbox reads.
fn pull_request(env: &TestEnv, n: u32, set: &str) {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("test", |tx| {
            tx.execute(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                 VALUES ('acme/shop', ?1, 'https://github.com/acme/shop/pull/' || ?1, 'T', 0, 'open', 'none', 'none', 't', 't')",
                [n],
            )?;
            tx.execute(&format!("UPDATE pull_requests SET {set} WHERE number = {n}"), [])?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn the_polled_pull_requests_open_and_clear_their_items() {
    let env = TestEnv::new();
    setup(&env);
    pull_request(&env, 1, "checks = 'passed', mergeable = 'conflicting'");
    pull_request(&env, 2, "review_decision = 'changes_requested'");
    pull_request(&env, 3, "checks = 'passed', mergeable = 'mergeable'");
    pull_request(&env, 4, "authored = 0, review_requested = 1");
    let items = found(&env, &[]);
    let seen: Vec<_> = items.iter().map(|c| (c.kind, c.urgency, c.label.as_str())).collect();
    assert_eq!(
        seen,
        [
            (Kind::PrFailing, Urgency::Red, "shop#1 merge conflict"),
            (Kind::PrChanges, Urgency::Red, "shop#2 changes requested"),
            (Kind::PrReady, Urgency::Yellow, "shop#3 ready to merge"),
            (Kind::ReviewRequested, Urgency::Yellow, "review shop#4"),
        ]
    );
    assert_eq!(items[0].subject, "pr:acme/shop#1");
    assert_eq!(items[0].url.as_deref(), Some("https://github.com/acme/shop/pull/1"));

    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    Store::open(&home, 5000)
        .unwrap()
        .write("test", |tx| {
            tx.execute("UPDATE pull_requests SET state = 'merged', merged_at = 't'", [])?;
            Ok(())
        })
        .unwrap();
    assert!(found(&env, &[]).is_empty());
}

fn failing(env: &TestEnv) -> Vec<(String, String)> {
    found(env, &[]).into_iter().filter(|c| c.kind == Kind::PrFailing).map(|c| (c.subject, c.label)).collect()
}

fn policy_file(env: &TestEnv, rel: &str, toml: &str) {
    let path = env.cpd_home().join("policy").join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, toml).unwrap();
}

const NO_CHECKS: &str = "[pr.feedback]\nchecks = \"none\"\n";

#[test]
fn an_unowned_row_raises_failed_checks_only_where_its_policy_says_any() {
    let env = TestEnv::new();
    setup(&env);
    // `acme` says none; its repo `shop` says any again; `nofile` has no owner file (cpd's default, any).
    env.owner_policy("acme", NO_CHECKS);
    policy_file(&env, "repos/acme/shop.toml", "[pr.feedback]\nchecks = \"any\"\n");
    pull_request(&env, 1, "checks = 'failed'");
    pull_request(&env, 2, "checks = 'failed', repo = 'acme/other', url = 'https://github.com/acme/other/pull/2'");
    pull_request(&env, 3, "checks = 'failed', repo = 'nofile/shop', url = 'https://github.com/nofile/shop/pull/3'");

    assert_eq!(
        failing(&env),
        [("pr:acme/shop#1".to_owned(), "shop#1 checks failed".to_owned()), ("pr:nofile/shop#3".to_owned(), "shop#3 checks failed".to_owned())]
    );
}

#[test]
fn a_conflict_stays_pr_failing_when_checks_are_not_feedback() {
    let env = TestEnv::new();
    setup(&env);
    env.owner_policy("acme", NO_CHECKS);
    pull_request(&env, 1, "checks = 'failed', mergeable = 'conflicting'");
    pull_request(&env, 2, "checks = 'failed', mergeable = 'mergeable'");

    assert_eq!(failing(&env), [("pr:acme/shop#1".to_owned(), "shop#1 merge conflict".to_owned())]);
}

#[test]
fn an_owned_row_takes_its_prs_policy_not_the_default_of_an_unowned_row() {
    let env = TestEnv::new();
    setup(&env);
    env.owner_policy("acme", NO_CHECKS);
    pull_request(&env, 5, "checks = 'failed'");
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let own = |name: &str| {
        Store::open(&home, 5000)
            .unwrap()
            .write("test", |tx| {
                tx.execute_batch(&format!(
                    "UPDATE contexts SET owner = 'acme' WHERE id = 'ctx';
                     INSERT INTO prs (plan, key, repo, base, name, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                                      pr_number, pr_url, pr_opened_at, pull_request)
                       VALUES ('pay', 'shop', '/r', 'main', '{name}', 'open', 'cpd/pay/shop', '/w', cpd_txn(), 't', 't',
                               5, 'https://github.com/acme/shop/pull/5', 't', (SELECT id FROM pull_requests WHERE number = 5));"
                ))?;
                Ok(())
            })
            .unwrap();
    };
    own("shop");
    assert!(failing(&env).is_empty(), "the PR's key is acme/shop: owner none");

    policy_file(&env, "repos/acme/shop.toml", "[pr.feedback]\nchecks = \"any\"\n");
    assert_eq!(failing(&env), [("pr:acme/shop#5".to_owned(), "shop#5 checks failed".to_owned())]);
}
