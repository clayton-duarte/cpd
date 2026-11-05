//! Claim reuse (pro-d416, A5): a claim of a review-requested item reuses and wakes the unarchived `grounding` codereview
//! Plan of the same PR instead of spawning a second one. On the fake `gh`, the in-process fake tmux and the manual clock.

use std::fs;
use std::sync::Arc;

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::cmd::{DismissArgs, claim_in};
use super::model::{Condition, Kind, Urgency, reconcile};
use crate::clock::ManualClock;
use crate::env::Env;
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::host::runner::{FakeRunner, Reply};
use crate::host::spawn::FakeSpawn;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;

const SUBJECT: &str = "pr:acme/pay#5";
const URL: &str = "https://github.com/acme/pay/pull/5";
const CHANNEL: &str = "C02JU6C2Z1D";
const TS_RAW: &str = "1767225600.000100";
const S_HEAD: &str = "5555555555555555555555555555555555555555";
const S_REV: &str = "1111111111111111111111111111111111111111";
const EXISTING: &str = "r-pay-5";

#[derive(Clone, Copy, PartialEq)]
enum Existing {
    None,
    Live,
    Archived,
    Done,
}

struct World {
    _dir: tempfile::TempDir,
    store: Store,
    ports: Ports,
    runner: FakeRunner,
    home: CpdHome,
}

/// The viewer's latest review, CHANGES_REQUESTED at `S_REV`, on a PR whose head is `S_HEAD`: a follow-up.
fn graphql() -> String {
    json!({"data": {"viewer": {"login": "clayton-duarte"}, "p0": {"pullRequest": {
        "title": "Add retries", "isDraft": false, "state": "OPEN", "reviewDecision": "CHANGES_REQUESTED",
        "headRefOid": S_HEAD,
        "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "SUCCESS"}}}]}, "author": {"login": "ryan"},
        "latestReviews": {"nodes": [{"state": "CHANGES_REQUESTED", "author": {"login": "clayton-duarte"},
                                      "commit": {"oid": S_REV}}]}}}}})
    .to_string()
}

/// Item 1 (review-requested, head `S_HEAD`, reviewed `S_REV`), the team cache, the Context `codereview` with an idle
/// Manager, a fake tmux cockpit, and `existing` as the PR's Plan with a stopped `runner` Lead.
fn world(existing: Existing, runner_kind: &str) -> World {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    fs::write(
        home.config(),
        "version = 1\nmachine = \"m1\"\nlead_tier = \"S4\"\nlead_runner = \"pi\"\n[tmux]\nsocket = \"t\"\n",
    )
    .unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let condition = Condition {
        kind: Kind::ReviewRequested,
        urgency: Urgency::Blue,
        subject: SUBJECT.into(),
        label: "acme/pay#5 review requested".into(),
        url: Some(URL.into()),
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
        head_oid: Some(S_HEAD.into()),
        reviewed_oid: Some(S_REV.into()),
    };
    reconcile(&mut store, &[condition], "2026-01-01T00:00:00.000Z").unwrap();
    let path = home.run_dir().join("team-review.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let post = json!({"repo": "acme/pay", "number": 5, "ts": 1767225600.0, "ts_raw": TS_RAW,
        "author": "Ryan Case", "primary": true, "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false});
    let cache = json!({"version": 1, "polled_at": "2026-01-01T00:00:00.000Z", "channel": CHANNEL, "read": 10, "posts": [post]});
    fs::write(path, cache.to_string()).unwrap();
    let (phase, archived, done) = match existing {
        Existing::Done => ("done", "NULL", "'t'"),
        Existing::Archived => ("grounding", "'t'", "NULL"),
        _ => ("grounding", "NULL", "NULL"),
    };
    let locked = if existing == Existing::Done {
        "'t'"
    } else {
        "NULL"
    };
    let plan = if existing == Existing::None {
        String::new()
    } else {
        format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner, asleep_at)
               VALUES ('oldlead', 'lead', 'codereview', '{EXISTING}', 'Old', '/', 'S4', 'stopped', 't', 't', '{runner_kind}', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, archived_at, done_at, pr_subject, changes_requested_at)
               VALUES ('{EXISTING}', 'codereview', 'Review pay#5 Add retries', '{phase}', 'oldlead', 't', {locked}, {archived}, {done}, '{SUBJECT}', 't');"
        )
    };
    store
        .write("fixture", |tx| {
            // The Plan's Lead references the Plan and the Plan its Lead: the fixture writes both with checks deferred.
            tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
            Ok(tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('codereview', 'codereview', 'clayton-duarte', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, state, state_at, created_at)
                   VALUES ('mgr', 'manager', 'codereview', NULL, NULL, 'codereview-manager', '/s/mgr.jsonl', '/repo', 'O4', 'idle', 't', 't');
                 {plan}"
            ))?)
        })
        .unwrap();
    let runner = FakeRunner::with_tmux().on("gh", |_| true, Reply::ok(&graphql()));
    let ports = Ports {
        clock: Arc::new(ManualClock::new()),
        runner: Arc::new(runner.clone()),
        spawn: Arc::new(FakeSpawn::new()),
        ..Ports::real()
    };
    // One test per process (nextest): the fake tmux's server key, the home and no sender are this test's.
    unsafe {
        std::env::set_var("TMUX_TMPDIR", dir.path());
        std::env::set_var("HOME", dir.path());
        std::env::set_var("CPD_HOME", home.root());
        std::env::remove_var("CPD_SESSION");
        std::env::remove_var("PI_SESSION_ID");
    }
    Tmux::new("t")
        .with_runner(Arc::new(runner.clone()))
        .run(&["new-session", "-d", "-s", "cpd", "sleep", "600"].map(str::to_owned))
        .unwrap();
    World {
        _dir: dir,
        store,
        ports,
        runner,
        home,
    }
}

fn claim(w: &mut World) -> Value {
    let env = Env::open_home_with(&w.ports, w.home.clone()).unwrap();
    match claim_in(&w.ports, env, DismissArgs { id: 1 }).expect("the claim succeeds") {
        Output::Success(fields) => Value::Object(fields),
        _ => panic!("a success"),
    }
}

fn count(w: &mut World, sql: &str) -> i64 {
    w.store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

fn text(w: &mut World, sql: &str) -> Option<String> {
    w.store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0)).optional()?.flatten()))
        .unwrap()
}

fn new_windows(w: &World) -> usize {
    w.runner
        .calls()
        .iter()
        .filter(|c| c.args.iter().any(|a| a == "new-window"))
        .count()
}

#[test]
fn claim_reuses_existing_codereview_plan_instead_of_spawning_second() {
    let mut w = world(Existing::Live, "pi");
    claim(&mut w);
    assert_eq!(
        count(&mut w, "SELECT count(*) FROM plans WHERE id = 'r-pay-5-2'"),
        0
    );
    assert_eq!(count(&mut w, "SELECT count(*) FROM plans"), 1);
}

#[test]
fn claim_of_reused_plan_marks_item_claimed_with_head_oid() {
    let mut w = world(Existing::Live, "pi");
    claim(&mut w);
    assert_eq!(
        count(
            &mut w,
            &format!(
                "SELECT count(*) FROM inbox_items WHERE id = 1 AND claimed_at IS NOT NULL AND head_oid = '{S_HEAD}'"
            )
        ),
        1
    );
    // Reuse spawns nothing: the claim of the item is the only change of plans.
    assert_eq!(count(&mut w, "SELECT count(*) FROM plans"), 1);
}

#[test]
fn claim_of_reused_plan_queues_eyes_reaction() {
    let mut w = world(Existing::Live, "pi");
    claim(&mut w);
    assert_eq!(
        count(
            &mut w,
            &format!(
                "SELECT count(*) FROM slack_reactions WHERE channel = '{CHANNEL}' AND ts = '{TS_RAW}' AND emoji = 'eyes'"
            )
        ),
        1
    );
    assert_eq!(count(&mut w, "SELECT count(*) FROM plans"), 1);
}

#[test]
fn claim_of_reused_plan_clears_changes_requested_at() {
    let mut w = world(Existing::Live, "pi");
    claim(&mut w);
    assert_eq!(
        text(
            &mut w,
            &format!("SELECT changes_requested_at FROM plans WHERE id = '{EXISTING}'")
        ),
        None
    );
}

#[test]
fn claim_of_reused_plan_returns_existing_plan_and_lead_in_json() {
    let mut w = world(Existing::Live, "pi");
    let out = claim(&mut w);
    assert_eq!(out["plan"], EXISTING, "{out}");
    assert_eq!(out["lead"], "oldlead", "{out}");
    assert!(out["item"]["claimed_at"].is_string(), "{out}");
    assert!(out["manager"].is_string(), "{out}");
}

#[test]
fn claim_of_reused_plan_sends_rereview_message_with_pr_slack_and_both_oids() {
    let mut w = world(Existing::Live, "pi");
    claim(&mut w);
    let message = text(
        &mut w,
        "SELECT text || ' ' || data FROM events WHERE kind = 'session-message' AND session = 'oldlead'",
    )
    .expect("a session-message Event to the existing Lead");
    for needle in [
        URL,
        CHANNEL,
        TS_RAW,
        S_REV,
        S_HEAD,
        "re-review",
        &format!("{S_REV}..{S_HEAD}"),
    ] {
        assert!(message.contains(needle), "{needle} missing in {message}");
    }
}

#[test]
fn claim_of_reused_plan_wakes_stopped_lead() {
    // A pi Lead gets a parked window, as `cpd session send` does.
    let mut pi = world(Existing::Live, "pi");
    claim(&mut pi);
    assert_eq!(new_windows(&pi), 1, "the stopped pi Lead's window");
    assert_eq!(
        count(
            &mut pi,
            "SELECT count(*) FROM sessions WHERE id = 'oldlead' AND tmux_pane IS NOT NULL"
        ),
        1
    );
    // A durable Lead is woken without a pane: `asleep_at` is cleared.
    let mut durable = world(Existing::Live, "durable");
    claim(&mut durable);
    assert_eq!(new_windows(&durable), 0, "no pane for a durable Lead");
    assert_eq!(
        count(
            &mut durable,
            "SELECT count(*) FROM sessions WHERE id = 'oldlead' AND asleep_at IS NULL"
        ),
        1
    );
}

#[test]
fn claim_without_existing_plan_spawns_plan_recording_pr_subject_and_head_oid() {
    let mut w = world(Existing::None, "pi");
    let out = claim(&mut w);
    assert_eq!(out["plan"], EXISTING, "{out}");
    assert_eq!(
        text(
            &mut w,
            &format!("SELECT pr_subject FROM plans WHERE id = '{EXISTING}'")
        )
        .as_deref(),
        Some(SUBJECT)
    );
    assert_eq!(
        text(&mut w, "SELECT head_oid FROM inbox_items WHERE id = 1").as_deref(),
        Some(S_HEAD)
    );
}

#[test]
fn claim_with_archived_plan_spawns_suffixed_plan() {
    let mut w = world(Existing::Archived, "pi");
    let out = claim(&mut w);
    assert_eq!(out["plan"], "r-pay-5-2", "{out}");
    assert_eq!(count(&mut w, "SELECT count(*) FROM plans"), 2);
}

#[test]
fn claim_with_done_plan_spawns_new_plan() {
    let mut w = world(Existing::Done, "pi");
    let out = claim(&mut w);
    assert_eq!(out["plan"], "r-pay-5-2", "{out}");
    assert_eq!(count(&mut w, "SELECT count(*) FROM plans"), 2);
}
