//! The Inbox's core conditions (contract §2.2 "Inbox item", §3.7 E8) computed from a store `cpd` wrote in this process (`run_with` on `Fx::with_ports`, the Lead in the fake tmux):
//! a Plan's Lead that blocks, and the pull requests the poll wrote.

use crate::common;

use std::fs;

use common::TestEnv;
use common::fx::Fx;
use cpd_core::home::CpdHome;
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::inbox::rules::conditions;
use cpd_core::store::Store;

/// The `Fx` on fake ports, its config, Context `ctx` and Plan `pay` (in `grounding`): the Lead goes into the fake tmux,
/// where no pane program runs. Returns the Lead's Session id.
fn setup(fx: &Fx) -> String {
    let config = format!("version = 1\nmachine = \"hs\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n", fx.env.tmux_socket());
    fx.env.write_config(&config);
    fx.fake_cpd_views();
    fx.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    let plan = fx.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Pay"]).ok();
    plan["plan"]["lead_session"].as_str().unwrap().to_owned()
}

fn found(env: &TestEnv) -> Vec<Condition> {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    conditions(&mut store, &home).unwrap()
}

#[test]
fn a_blocked_lead_in_grounding_is_a_magenta_dialog_until_it_works_again() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    let lead = setup(&fx);
    assert!(found(env).is_empty());

    fx.cpd(&["session", "state", "idle", "--session", &lead]).ok();
    fx.cpd(&["session", "state", "blocked", "--label", "Which repo?", "--session", &lead]).ok();
    let items = found(env);
    assert_eq!(items.len(), 1, "{items:?}");
    let c = &items[0];
    assert_eq!((c.kind, c.urgency), (Kind::Dialog, Urgency::Magenta));
    assert_eq!((c.subject.clone(), c.label.as_str()), (format!("session:{lead}"), "pay asks you"));
    let latest: i64 = fx.cpd(&["events", "list", "--kind", "session-state", "--session", &lead]).ok()["events"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    assert_eq!(c.event, Some(latest), "its latest session-state Event");

    fx.cpd(&["session", "state", "working", "--session", &lead]).ok();
    assert!(found(env).is_empty());
}

#[test]
fn a_lead_waiting_for_a_worker_opens_no_dialog_and_its_reply_turn_is_unread_hs_d303() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    let lead = setup(&fx);
    fx.cpd(&["session", "state", "idle", "--session", &lead]).ok();
    fx.cpd(&["session", "state", "working", "--session", &lead]).ok();
    fx.cpd(&["session", "state", "waiting", "--label", "cpd: Worker question", "--session", &lead]).ok();
    assert!(found(env).is_empty(), "a Lead that waits for an agent does not need the User");

    // The wait ends in a turn that answered the User: `waiting` to `idle` with `--reply` is a reply to read.
    fx.cpd(&["session", "state", "idle", "--reply", "--session", &lead]).ok();
    let items = found(env);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!((items[0].kind, items[0].urgency), (Kind::Unread, Urgency::White));

    // `blocked` is still the human's: a dialog, even under the label a Lead's wait used to carry.
    fx.cpd(&["session", "state", "working", "--session", &lead]).ok();
    fx.cpd(&["session", "state", "blocked", "--label", "cpd: Worker question", "--session", &lead]).ok();
    let items = found(env);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].kind, Kind::Dialog);
}

#[test]
fn a_question_opens_no_item_hs_d306() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    setup(&fx);
    fx.cpd(&["context", "ask", "Why?", "--context", "ctx"]).ok();
    assert!(found(env).is_empty());

    fx.cpd(&["context", "answer", "hs-q1", "Because.", "--context", "ctx"]).ok();
    assert!(found(env).is_empty());
}

/// hs-d314: the engine's errors go to the Manager, not the Inbox, so E8 computes no `engine-error`, and an item an
/// older engine opened for one closes at the next reconcile.
#[test]
fn no_engine_error_item_opens_and_one_an_older_engine_opened_closes() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    setup(&fx);
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let event = store.read(cpd_core::events::highest_id).unwrap();
    let old = Condition {
        kind: Kind::EngineError,
        urgency: Urgency::Red,
        subject: "error:E5/machine/gh_rate_limited".into(),
        label: "engine E5: gh_rate_limited".into(),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: Some(event),
        opened_at: None,
    };
    let at = "2026-01-01T00:00:00.000Z";
    assert_eq!(reconcile(&mut store, &[old], at).unwrap().opened.len(), 1, "the row an older engine wrote");

    let found = found(env);
    assert!(found.is_empty(), "{found:?}");
    let changes = reconcile(&mut store, &found, at).unwrap();
    assert_eq!((changes.opened.len(), changes.closed.len()), (0, 1), "the open item closes");
    assert!(reconcile(&mut store, &[], at).unwrap().is_empty(), "idempotent");
}

/// Writes a pull request row as the poll would, with the columns the Inbox reads.
fn pull_request(env: &TestEnv, n: u32, set: &str) {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("test", |tx| {
            tx.execute(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, merge_state)
                 VALUES ('acme/shop', ?1, 'https://github.com/acme/shop/pull/' || ?1, 'T', 0, 'open', 'none', 'none', 't', 't', 'clean')",
                [n],
            )?;
            tx.execute(&format!("UPDATE pull_requests SET {set} WHERE number = {n}"), [])?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn the_polled_pull_requests_open_and_clear_their_items() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    setup(&fx);
    pull_request(env, 1, "checks = 'passed', mergeable = 'conflicting', merge_state = 'dirty'");
    pull_request(env, 2, "review_decision = 'changes_requested'");
    pull_request(env, 3, "checks = 'passed', mergeable = 'mergeable'");
    pull_request(env, 4, "authored = 0, review_requested = 1");
    // #2385 of missionlane/cc-api-orc-apollo (hs-d481): everything looks ready, but GitHub's merge state is BLOCKED.
    pull_request(env, 5, "checks = 'passed', mergeable = 'mergeable', merge_state = 'blocked', review_decision = 'none'");
    let items = found(env);
    let seen: Vec<_> = items.iter().map(|c| (c.kind, c.urgency, c.label.as_str())).collect();
    assert_eq!(
        seen,
        [
            (Kind::PrReady, Urgency::Yellow, "shop#3 ready to merge"),
        ],
        "a conflict (1) and changes requested (2) open no item (hs-d470); a blocked merge state (5, #2385) is not ready (hs-d481); pull request 4 requests the User's review on GitHub, which alone opens nothing (hs-d320)"
    );
    assert_eq!(items[0].subject, "pr:acme/shop#3");
    assert_eq!(items[0].url.as_deref(), Some("https://github.com/acme/shop/pull/3"));

    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    Store::open(&home, 5000)
        .unwrap()
        .write("test", |tx| {
            tx.execute("UPDATE pull_requests SET state = 'merged', merged_at = 't'", [])?;
            Ok(())
        })
        .unwrap();
    assert!(found(env).is_empty());
}

fn failing(env: &TestEnv) -> Vec<(String, String)> {
    found(env).into_iter().filter(|c| c.kind == Kind::PrFailing).map(|c| (c.subject, c.label)).collect()
}

fn policy_file(env: &TestEnv, rel: &str, toml: &str) {
    let path = env.cpd_home().join("policy").join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, toml).unwrap();
}

const NO_CHECKS: &str = "[pr.feedback]\nchecks = \"none\"\n";

#[test]
fn an_unowned_row_with_failed_checks_opens_no_item_whatever_its_policy() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    setup(&fx);
    // `acme` says none; its repo `shop` says any again; `nofile` has no owner file (cpd's default, any).
    env.owner_policy("acme", NO_CHECKS);
    policy_file(env, "acme/shop.toml", "[pr.feedback]\nchecks = \"any\"\n");
    // Failed required checks on a mergeable PR: GitHub reports `blocked`.
    pull_request(env, 1, "checks = 'failed', merge_state = 'blocked'");
    pull_request(env, 2, "checks = 'failed', merge_state = 'blocked', repo = 'acme/other', url = 'https://github.com/acme/other/pull/2'");
    pull_request(env, 3, "checks = 'failed', merge_state = 'blocked', repo = 'nofile/shop', url = 'https://github.com/nofile/shop/pull/3'");

    assert!(found(env).is_empty(), "failed checks open no item under any policy (hs-d470)");
}

#[test]
fn a_conflict_opens_no_pr_failing_when_checks_are_not_feedback() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    setup(&fx);
    env.owner_policy("acme", NO_CHECKS);
    pull_request(env, 1, "checks = 'failed', mergeable = 'conflicting'");
    pull_request(env, 2, "checks = 'failed', mergeable = 'mergeable'");

    assert!(failing(env).is_empty(), "a conflict opens no pr-failing (hs-d470)");
}

#[test]
fn an_owned_row_takes_its_prs_policy_not_the_default_of_an_unowned_row() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    setup(&fx);
    env.owner_policy("acme", NO_CHECKS);
    // A Plan owns the row, so `pr-failing` never reads it (hs-d302); its policy decides `pr-ready`, which counts checks only
    // where the policy says `any`.
    pull_request(env, 5, "checks = 'failed', mergeable = 'mergeable'");
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let ready = |env: &TestEnv| -> Vec<String> { found(env).into_iter().filter(|c| c.kind == Kind::PrReady).map(|c| c.subject).collect() };
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
    assert!(failing(env).is_empty(), "an owned row opens no pr-failing (hs-d302)");
    assert_eq!(ready(env), ["pr:acme/shop#5"], "ready by merge state and review, whatever its checks (hs-d481)");

    policy_file(env, "acme/shop.toml", "[pr.feedback]\nchecks = \"any\"\n");
    assert_eq!(ready(env), ["pr:acme/shop#5"], "its repo says any: failed checks no longer keep it from ready (hs-d481)");
    assert!(failing(env).is_empty(), "and still no pr-failing for an owned row (hs-d302)");
}

/// Makes Plan `pay`'s PR `shop` point to the pull request `n` of `acme/shop`.
fn own_pull_request(env: &TestEnv, n: u32) {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    Store::open(&home, 5000)
        .unwrap()
        .write("test", |tx| {
            tx.execute(
                "INSERT INTO prs (plan, key, repo, base, name, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                                  pr_number, pr_url, pr_opened_at, pull_request)
                   VALUES ('pay', 'shop', '/r', 'main', 'shop', 'open', 'cpd/pay/shop', '/w', cpd_txn(), 't', 't',
                           ?1, 'https://github.com/acme/shop/pull/' || ?1, 't', (SELECT id FROM pull_requests WHERE number = ?1))",
                [n],
            )?;
            Ok(())
        })
        .unwrap();
}

fn kinds(items: &[Condition]) -> Vec<(Kind, String)> {
    items.iter().map(|c| (c.kind, c.subject.clone())).collect()
}

#[test]
fn a_row_whose_owner_policy_does_not_resolve_opens_no_engine_error_and_pr_ready_follows_the_merge_state_only() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    setup(&fx);
    pull_request(env, 5, "checks = 'passed', mergeable = 'mergeable'");
    own_pull_request(env, 5);
    pull_request(env, 6, "review_decision = 'changes_requested'");
    pull_request(env, 7, "checks = 'failed', merge_state = 'blocked', repo = 'other/x', url = 'https://github.com/other/x/pull/7'");
    fs::remove_file(env.cpd_home().join("policy/acme/owner.toml")).unwrap();

    let items = found(env);

    // `pr-ready` reads no policy (hs-d481): the unknown owner opens no engine-error; no pr-failing or pr-changes (hs-d470).
    assert_eq!(kinds(&items), [(Kind::PrReady, "pr:acme/shop#5".to_owned())]);
}

#[test]
fn a_pr_of_a_context_with_no_owner_opens_no_engine_error_and_pr_ready_follows_the_merge_state_only() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    setup(&fx);
    pull_request(env, 5, "checks = 'passed', mergeable = 'mergeable'");
    own_pull_request(env, 5);
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    Store::open(&home, 5000)
        .unwrap()
        .write("test", |tx| {
            tx.execute("UPDATE contexts SET owner = NULL WHERE id = 'ctx'", [])?;
            Ok(())
        })
        .unwrap();

    let items = found(env);

    // No policy is read (hs-d481): an unset owner opens no engine-error, and no pr-failing (hs-d470).
    assert_eq!(kinds(&items), [(Kind::PrReady, "pr:acme/shop#5".to_owned())]);
}

#[test]
fn an_unstable_pull_request_opens_no_pr_ready_item_hs_d525() {
    let fx = Fx::with_ports();
    let env = &fx.env;
    setup(&fx);
    pull_request(env, 1, "merge_state = 'unstable'");
    pull_request(env, 2, "merge_state = 'has_hooks', review_decision = 'approved'");
    pull_request(env, 3, "merge_state = 'clean', review_decision = 'approved'");
    let ready: Vec<String> = found(env).into_iter().filter(|c| c.kind == Kind::PrReady).map(|c| c.subject).collect();
    assert_eq!(ready, ["pr:acme/shop#3"], "only a CLEAN pull request is ready to merge (hs-d525)");
}
