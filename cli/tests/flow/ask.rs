//! Escalations (contract §3.3, §3.5; commands.md §8), against a temporary `$CPD_HOME`, all in this process (`Fx` on
//! the fake ports). Plans, Jobs, Attempts and Sessions are written straight into the database, as fixtures: they belong
//! to other areas. `ask` blocks and polls every 500 ms on the clock of the ports: the `ManualClock` passes that time at
//! once, and what another process does meanwhile (`answer`) the test does in a hook of [`Fx::on_nap`], at the first nap.

use crate::common;

use std::path::Path;
use std::time::Duration;

use common::fx::Fx;
use common::{Run, TestEnv, refused};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

// ---------- fixtures ----------

fn store(env: &TestEnv) -> Store {
    store_at(&env.cpd_home())
}

fn store_at(home: &Path) -> Store {
    Store::open(&CpdHome::resolve(Some(home.into()), None).unwrap(), 5000).unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn fixture(env: &TestEnv, sql: &str) {
    store(env).write("fixture", |tx| exec(tx, sql)).unwrap();
}

/// Plan `pl` (Lead session `lead-pl`), Job `a` running, and Attempt `a-a1`
/// `running` with Worker Session `w1` `working`.
fn setup(env: &TestEnv) {
    std::fs::write(
        env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"m1\"\n",
    )
    .unwrap();
    fixture(
        env,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Context', 't', 'm1');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
             VALUES ('pl', 'ctx', 'Plan', 'grounding', 'lead-pl', 't');
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
             VALUES ('pl', 'repo', '/repo', 'main', 'waiting', 'cpd/pl', cpd_txn(), 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
             VALUES ('lead-pl', 'lead', 'ctx', 'pl', 'pl', '/repo', 'O3', 'idle', 't', 't');
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
             VALUES ('pl', 'repo', 'a', 'A', 'S2', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', 'draft', 0, NULL, cpd_txn(), 't');
             UPDATE jobs SET status = 'queued', fingerprint = '{fp}' WHERE key = 'a';
             UPDATE jobs SET status = 'running' WHERE key = 'a';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
             VALUES ('pl', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pl--a-a1', '/w', 'w1', 't');
             INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
             VALUES ('w1', 'worker', 'ctx', 'pl', 'a-a1', 'a-a1', '/w', 'S2', 'working', 't', 't');",
            fp = "0".repeat(64)
        ),
    );
}

/// The Events of `kind`, through `cpd`: `Fx::cpd` or the `cpd` of a [`common::fx::Nap`].
fn events_by(cpd: impl Fn(&[&str]) -> Run, kind: &str) -> Vec<Value> {
    cpd(&["events", "list", "--kind", kind]).ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

fn fx_events(fx: &Fx, kind: &str) -> Vec<Value> {
    events_by(|args| fx.cpd(args), kind)
}

fn session_state_at(home: &Path) -> (String, Option<String>) {
    store_at(home)
        .read(|c| {
            Ok(c.query_row(
                "SELECT state, state_label FROM sessions WHERE id = 'w1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

fn session_state(env: &TestEnv) -> (String, Option<String>) {
    session_state_at(&env.cpd_home())
}

/// `cpd ask a-a1 <question> --plan pl`, in this process.
fn ask(fx: &Fx, question: &str) -> Run {
    fx.cpd(&["ask", "a-a1", question, "--plan", "pl"])
}

// ---------- ask ----------

#[test]
fn ask_blocks_until_answered_and_prints_the_answer() {
    let mut fx = Fx::with_ports();
    setup(&fx.env);
    let home = fx.env.cpd_home().to_path_buf();
    // At its first nap `ask` waits for an answer: what a test sees "while it waits" is read here, then `answer` runs.
    fx.on_nap(move |nap| {
        // While it waits: the row is open, the Worker Session waiting (for the Lead, not the User), both Events appended.
        let (status, asker_pid, question): (String, u32, String) = store_at(&home)
            .read(|c| {
                Ok(c.query_row(
                    "SELECT status, asker_pid, question FROM escalations WHERE id = 'a-a1-q1'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap();
        assert_eq!(
            (status.as_str(), asker_pid, question.as_str()),
            ("open", std::process::id(), "Which field names?")
        );
        assert_eq!(
            session_state_at(&home),
            (
                "waiting".to_owned(),
                Some("φ: question to the Lead".to_owned())
            )
        );
        let asked = events_by(|args| nap.cpd(args), "escalation");
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0]["plan"], "pl");
        assert_eq!(asked[0]["job"], "a");
        assert_eq!(asked[0]["attempt"], "a-a1");
        assert_eq!(
            asked[0]["data"],
            json!({"escalation": "a-a1-q1", "question": "Which field names?"})
        );
        assert!(
            asked[0]["text"]
                .as_str()
                .unwrap()
                .contains("cpd answer a-a1-q1")
        );
        let states = events_by(|args| nap.cpd(args), "session-state");
        assert_eq!(
            states.last().unwrap()["data"],
            json!({"from": "working", "to": "waiting", "label": "φ: question to the Lead"})
        );
        assert_eq!(states.last().unwrap()["session"], "w1");

        nap.cpd(&[
            "answer",
            "a-a1-q1",
            "Use the v2 field names.",
            "--plan",
            "pl",
        ])
        .ok();
    });

    let run = ask(&fx, "Which field names?");
    assert_eq!(
        run.ok(),
        json!({"ok": true, "escalation": "a-a1-q1", "answer": "Use the v2 field names."})
    );
    assert_eq!(session_state(&fx.env), ("working".to_owned(), None));
    let states = fx_events(&fx, "session-state");
    assert_eq!(
        states.last().unwrap()["data"],
        json!({"from": "waiting", "to": "working", "label": null})
    );
}

#[test]
fn every_ask_is_a_new_escalation() {
    let mut fx = Fx::with_ports();
    setup(&fx.env);
    for k in 1..=2 {
        let id = format!("a-a1-q{k}");
        let answered = id.clone();
        // `answer` is refused (`escalation_not_found`) unless `ask` opened the row first.
        fx.on_nap(move |nap| {
            nap.cpd(&["answer", &answered, "Yes.", "--plan", "pl"]).ok();
        });
        assert_eq!(ask(&fx, "Again?").ok()["escalation"], id.as_str());
    }
}

/// Restored from `adb53fc8^`: the deadline is read on the `ManualClock`, 500 ms at a time, with no real wait.
#[test]
fn ask_times_out_and_expires_the_escalation() {
    let mut fx = Fx::with_ports();
    setup(&fx.env);
    let naps = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = naps.clone();
    fx.on_nap(move |_| {
        counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    let run = fx.cpd(&[
        "ask",
        "a-a1",
        "Anyone?",
        "--timeout-minutes",
        "1",
        "--plan",
        "pl",
    ]);
    let error = refused(&run, "ask_timeout", 5);
    assert_eq!(error["details"]["escalation"], "a-a1-q1");
    // One minute of virtual time, in the 500 ms polls of §8.1; the hook ran at every one.
    assert!(
        fx.fakes.clock.elapsed() >= Duration::from_secs(60),
        "{:?}",
        fx.fakes.clock.elapsed()
    );
    assert!(
        fx.fakes
            .clock
            .naps()
            .iter()
            .all(|nap| *nap == Duration::from_millis(500)),
        "{:?}",
        fx.fakes.clock.naps()
    );
    assert_eq!(
        naps.load(std::sync::atomic::Ordering::Relaxed),
        fx.fakes.clock.naps().len()
    );

    let (status, expired_at, answer): (String, Option<String>, Option<String>) = store(&fx.env)
        .read(|c| {
            Ok(c.query_row(
                "SELECT status, expired_at, answer FROM escalations WHERE id = 'a-a1-q1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(status, "expired");
    assert!(expired_at.is_some());
    assert_eq!(answer, None);
    let expired = fx_events(&fx, "escalation-expired");
    assert_eq!(expired.len(), 1);
    assert_eq!(
        expired[0]["data"],
        json!({"escalation": "a-a1-q1", "reason": "timeout"})
    );
    assert_eq!(expired[0]["attempt"], "a-a1");
    assert_eq!(session_state(&fx.env), ("working".to_owned(), None));

    // An expired Escalation cannot be answered any more.
    refused(
        &fx.cpd(&["answer", "a-a1-q1", "Late.", "--plan", "pl"]),
        "escalation_expired",
        3,
    );
}

#[test]
fn ask_refuses_an_attempt_that_is_not_running() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fixture(
        &fx.env,
        "UPDATE attempts SET status = 'merging', submitted_sha = 's', submitted_at = 't' WHERE id = 'a-a1';",
    );
    let error = refused(
        &fx.cpd(&["ask", "a-a1", "Q?", "--plan", "pl"]),
        "attempt_not_running",
        3,
    );
    assert_eq!(error["details"]["status"], "merging");
    fixture(
        &fx.env,
        "UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE id = 'a-a1';",
    );
    refused(
        &fx.cpd(&["ask", "a-a1", "Q?", "--plan", "pl"]),
        "attempt_not_running",
        3,
    );
    // Nothing was written.
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM escalations"), 0);
    assert!(fx_events(&fx, "escalation").is_empty());
}

#[test]
fn ask_refusals_before_the_attempt() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    refused(
        &fx.cpd(&["ask", "a-a2", "Q?", "--plan", "pl"]),
        "attempt_not_found",
        4,
    );
    refused(
        &fx.cpd(&["ask", "a-a1", "Q?", "--plan", "nope"]),
        "plan_not_found",
        4,
    );
    refused(&fx.cpd(&["ask", "a-a1", "Q?"]), "plan_required", 2);
    refused(
        &fx.cpd(&["ask", "not-an-attempt", "Q?", "--plan", "pl"]),
        "invalid_attempt_id",
        2,
    );
    refused(&fx.cpd(&["ask", "a-a1", "", "--plan", "pl"]), "usage", 2);
    refused(
        &fx.cpd(&[
            "ask",
            "a-a1",
            "Q?",
            "--timeout-minutes",
            "soon",
            "--plan",
            "pl",
        ]),
        "usage",
        2,
    );
    // A stopped Worker Session cannot become waiting (§3.6): nothing is written.
    fixture(
        &fx.env,
        "UPDATE sessions SET state = 'stopped' WHERE id = 'w1';",
    );
    refused(
        &fx.cpd(&["ask", "a-a1", "Q?", "--plan", "pl"]),
        "illegal_transition",
        3,
    );
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM escalations"), 0);
}

#[test]
fn ask_takes_the_plan_from_the_environment() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    let run = fx.cpd_with(
        fx.env.root(),
        &["ask", "a-a1", "Q?", "--timeout-minutes", "0"],
        &[("CPD_PLAN", "pl")],
    );
    refused(&run, "ask_timeout", 5);
}

// ---------- answer ----------

/// An open Escalation `a-a1-q1`, as `ask` leaves it.
fn open_escalation(env: &TestEnv) {
    fixture(
        env,
        "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
         VALUES ('pl', 'a-a1-q1', 'a-a1', 1, 'Which?', 'open', 1, 't');",
    );
}

#[test]
fn answer_records_the_answer() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    open_escalation(&fx.env);
    let out = fx
        .cpd(&[
            "answer",
            "a-a1-q1",
            "Use the v2 field names.",
            "--plan",
            "pl",
        ])
        .ok();
    assert_eq!(out["escalation"], "a-a1-q1");
    assert_eq!(out["attempt"], "a-a1");
    let answered_at = out["answered_at"].as_str().unwrap();
    assert!(
        answered_at.ends_with('Z') && answered_at.len() == 24,
        "{answered_at}"
    );

    let (status, answer, at): (String, String, String) = store(&fx.env)
        .read(|c| {
            Ok(c.query_row(
                "SELECT status, answer, answered_at FROM escalations WHERE id = 'a-a1-q1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        (status.as_str(), answer.as_str(), at.as_str()),
        ("answered", "Use the v2 field names.", answered_at)
    );
    let answered = fx_events(&fx, "escalation-answered");
    assert_eq!(answered.len(), 1);
    assert_eq!(answered[0]["data"], json!({"escalation": "a-a1-q1"}));
    assert_eq!(
        (
            answered[0]["plan"].as_str(),
            answered[0]["job"].as_str(),
            answered[0]["attempt"].as_str()
        ),
        (Some("pl"), Some("a"), Some("a-a1"))
    );
}

#[test]
fn answer_is_refused_unless_the_escalation_is_open() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    open_escalation(&fx.env);
    fx.cpd(&["answer", "a-a1-q1", "First.", "--plan", "pl"])
        .ok();
    let events_before = fx_events(&fx, "escalation-answered").len();
    refused(
        &fx.cpd(&["answer", "a-a1-q1", "Second.", "--plan", "pl"]),
        "escalation_answered",
        3,
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT answer FROM escalations WHERE id = 'a-a1-q1'"),
        "First."
    );
    assert_eq!(fx_events(&fx, "escalation-answered").len(), events_before);

    fixture(
        &fx.env,
        "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at, expired_at)
         VALUES ('pl', 'a-a1-q2', 'a-a1', 2, 'Gone?', 'expired', 1, 't', 't');",
    );
    refused(
        &fx.cpd(&["answer", "a-a1-q2", "Late.", "--plan", "pl"]),
        "escalation_expired",
        3,
    );
}

#[test]
fn answer_refusals_before_the_escalation() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    open_escalation(&fx.env);
    refused(
        &fx.cpd(&["answer", "a-a1-q9", "A.", "--plan", "pl"]),
        "escalation_not_found",
        4,
    );
    refused(
        &fx.cpd(&["answer", "a-a1-q1", "A.", "--plan", "other"]),
        "escalation_not_found",
        4,
    );
    refused(&fx.cpd(&["answer", "a-a1-q1", "A."]), "plan_required", 2);
    refused(
        &fx.cpd(&["answer", "q1", "A.", "--plan", "pl"]),
        "invalid_escalation_id",
        2,
    );
    refused(
        &fx.cpd(&["answer", "a-a1-q1", "", "--plan", "pl"]),
        "usage",
        2,
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM escalations WHERE id = 'a-a1-q1'"),
        "open"
    );
}
