//! Escalations (contract §3.3, §3.5; commands.md §8), run as the built binary
//! against a temporary `$CPD_HOME`. Plans, Jobs, Attempts and Sessions are
//! written straight into the database, as fixtures: they belong to other
//! areas. `ask` blocks, so it runs as a child process the test answers.

mod common;

use std::process::{Child, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use common::{CPD, Run, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

// ---------- fixtures ----------

fn store(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn fixture(env: &TestEnv, sql: &str) {
    store(env).write("fixture", |tx| exec(tx, sql)).unwrap();
}

fn query<T: rusqlite::types::FromSql>(env: &TestEnv, sql: &str) -> T {
    store(env).read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

/// Plan `pl` (Lead session `lead-pl`), Job `a` running, and Attempt `a-a1`
/// `running` with Worker Session `w1` `working`.
fn setup(env: &TestEnv) {
    std::fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    fixture(
        env,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Context', 't', 'm1');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
             VALUES ('pl', 'ctx', 'Plan', 'grounding', 'lead-pl', 't');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
             VALUES ('pl', 'repo', '/repo', 'main', 'waiting', 'cpd/pl', cpd_txn(), 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
             VALUES ('lead-pl', 'lead', 'ctx', 'pl', 'pl', '/repo', 'O3', 'idle', 't', 't');
             INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
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

fn refused(run: &Run, code: &str, exit: i32) -> Value {
    let error = run.error();
    assert_eq!(error["code"], code, "{run:?}");
    assert_eq!(run.exit, exit, "{run:?}");
    error
}

fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

fn session_state(env: &TestEnv) -> (String, Option<String>) {
    store(env)
        .read(|c| Ok(c.query_row("SELECT state, state_label FROM sessions WHERE id = 'w1'", [], |r| Ok((r.get(0)?, r.get(1)?)))?))
        .unwrap()
}

/// Kills and reaps its child on drop, so a panicking test leaves no process.
struct KillOnDrop(Option<Child>);

impl KillOnDrop {
    fn new(child: Child) -> Self {
        KillOnDrop(Some(child))
    }

    fn into_inner(mut self) -> Child {
        self.0.take().unwrap()
    }
}

impl std::ops::Deref for KillOnDrop {
    type Target = Child;
    fn deref(&self) -> &Child {
        self.0.as_ref().unwrap()
    }
}

impl std::ops::DerefMut for KillOnDrop {
    fn deref_mut(&mut self) -> &mut Child {
        self.0.as_mut().unwrap()
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn a_guarded_child_is_gone_after_a_panic() {
    let pid = std::cell::Cell::new(0);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let child = KillOnDrop::new(std::process::Command::new("sleep").arg("60").spawn().unwrap());
        pid.set(child.id());
        panic!("boom");
    }));
    assert!(result.is_err());
    let alive = std::process::Command::new("kill").args(["-0", &pid.get().to_string()]).output().unwrap().status.success();
    assert!(!alive, "child {} survived the panic", pid.get());
}

/// `cpd ask a-a1 <question> --plan p` running in the background.
fn spawn_ask(env: &TestEnv, question: &str) -> KillOnDrop {
    KillOnDrop::new(env.command(CPD)
        .args(["ask", "a-a1", question, "--plan", "pl"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap())
}

/// Waits until Escalation `id` exists.
fn wait_for(env: &TestEnv, id: &str) {
    let start = Instant::now();
    while query::<i64>(env, &format!("SELECT count(*) FROM escalations WHERE id = '{id}'")) == 0 {
        assert!(start.elapsed() < Duration::from_secs(20), "ask never opened {id}");
        sleep(Duration::from_millis(50));
    }
}

fn finish(child: KillOnDrop) -> Run {
    let out = child.into_inner().wait_with_output().unwrap();
    Run {
        exit: out.status.code().unwrap(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

// ---------- ask ----------

#[test]
fn ask_blocks_until_answered_and_prints_the_answer() {
    let env = TestEnv::new();
    setup(&env);
    let child = spawn_ask(&env, "Which field names?");
    let pid = child.id();
    wait_for(&env, "a-a1-q1");

    // While it waits: the row is open, the Worker Session blocked, both Events appended.
    let (status, asker_pid, question): (String, u32, String) = store(&env)
        .read(|c| Ok(c.query_row("SELECT status, asker_pid, question FROM escalations WHERE id = 'a-a1-q1'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
        .unwrap();
    assert_eq!((status.as_str(), asker_pid, question.as_str()), ("open", pid, "Which field names?"));
    assert_eq!(session_state(&env), ("blocked".to_owned(), Some("cpd: question to the Lead".to_owned())));
    let asked = events(&env, "escalation");
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["plan"], "pl");
    assert_eq!(asked[0]["job"], "a");
    assert_eq!(asked[0]["attempt"], "a-a1");
    assert_eq!(asked[0]["data"], json!({"escalation": "a-a1-q1", "question": "Which field names?"}));
    assert!(asked[0]["text"].as_str().unwrap().contains("cpd answer a-a1-q1"));
    let states = events(&env, "session-state");
    assert_eq!(states.last().unwrap()["data"], json!({"from": "working", "to": "blocked", "label": "cpd: question to the Lead"}));
    assert_eq!(states.last().unwrap()["session"], "w1");

    env.cpd(&["answer", "a-a1-q1", "Use the v2 field names.", "--plan", "pl"]).ok();
    let run = finish(child);
    assert_eq!(run.ok(), json!({"ok": true, "escalation": "a-a1-q1", "answer": "Use the v2 field names."}));
    assert_eq!(session_state(&env), ("working".to_owned(), None));
    let states = events(&env, "session-state");
    assert_eq!(states.last().unwrap()["data"], json!({"from": "blocked", "to": "working", "label": null}));
}

#[test]
fn every_ask_is_a_new_escalation() {
    let env = TestEnv::new();
    setup(&env);
    for k in 1..=2 {
        let child = spawn_ask(&env, "Again?");
        let id = format!("a-a1-q{k}");
        wait_for(&env, &id);
        env.cpd(&["answer", &id, "Yes.", "--plan", "pl"]).ok();
        assert_eq!(finish(child).ok()["escalation"], id.as_str());
    }
}

#[test]
fn ask_times_out_and_expires_the_escalation() {
    let env = TestEnv::new();
    setup(&env);
    let run = env.cpd(&["ask", "a-a1", "Anyone?", "--timeout-minutes", "0", "--plan", "pl"]);
    let error = refused(&run, "ask_timeout", 5);
    assert_eq!(error["details"]["escalation"], "a-a1-q1");

    let (status, expired_at, answer): (String, Option<String>, Option<String>) = store(&env)
        .read(|c| Ok(c.query_row("SELECT status, expired_at, answer FROM escalations WHERE id = 'a-a1-q1'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
        .unwrap();
    assert_eq!(status, "expired");
    assert!(expired_at.is_some());
    assert_eq!(answer, None);
    let expired = events(&env, "escalation-expired");
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0]["data"], json!({"escalation": "a-a1-q1", "reason": "timeout"}));
    assert_eq!(expired[0]["attempt"], "a-a1");
    assert_eq!(session_state(&env), ("working".to_owned(), None));

    // An expired Escalation cannot be answered any more.
    refused(&env.cpd(&["answer", "a-a1-q1", "Late.", "--plan", "pl"]), "escalation_expired", 3);
}

#[test]
fn ask_refuses_an_attempt_that_is_not_running() {
    let env = TestEnv::new();
    setup(&env);
    fixture(&env, "UPDATE attempts SET status = 'merging', submitted_sha = 's', submitted_at = 't' WHERE id = 'a-a1';");
    let error = refused(&env.cpd(&["ask", "a-a1", "Q?", "--plan", "pl"]), "attempt_not_running", 3);
    assert_eq!(error["details"]["status"], "merging");
    fixture(&env, "UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE id = 'a-a1';");
    refused(&env.cpd(&["ask", "a-a1", "Q?", "--plan", "pl"]), "attempt_not_running", 3);
    // Nothing was written.
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM escalations"), 0);
    assert!(events(&env, "escalation").is_empty());
}

#[test]
fn ask_refusals_before_the_attempt() {
    let env = TestEnv::new();
    setup(&env);
    refused(&env.cpd(&["ask", "a-a2", "Q?", "--plan", "pl"]), "attempt_not_found", 4);
    refused(&env.cpd(&["ask", "a-a1", "Q?", "--plan", "nope"]), "plan_not_found", 4);
    refused(&env.cpd(&["ask", "a-a1", "Q?"]), "plan_required", 2);
    refused(&env.cpd(&["ask", "not-an-attempt", "Q?", "--plan", "pl"]), "invalid_attempt_id", 2);
    refused(&env.cpd(&["ask", "a-a1", "", "--plan", "pl"]), "usage", 2);
    refused(&env.cpd(&["ask", "a-a1", "Q?", "--timeout-minutes", "soon", "--plan", "pl"]), "usage", 2);
    // A stopped Worker Session cannot become blocked (§3.6): nothing is written.
    fixture(&env, "UPDATE sessions SET state = 'stopped' WHERE id = 'w1';");
    refused(&env.cpd(&["ask", "a-a1", "Q?", "--plan", "pl"]), "illegal_transition", 3);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM escalations"), 0);
}

#[test]
fn ask_takes_the_plan_from_the_environment() {
    let env = TestEnv::new();
    setup(&env);
    let run = {
        let out = env.command(CPD).env("CPD_PLAN", "pl").args(["ask", "a-a1", "Q?", "--timeout-minutes", "0"]).output().unwrap();
        Run { exit: out.status.code().unwrap(), stdout: String::from_utf8(out.stdout).unwrap(), stderr: String::new() }
    };
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
    let env = TestEnv::new();
    setup(&env);
    open_escalation(&env);
    let out = env.cpd(&["answer", "a-a1-q1", "Use the v2 field names.", "--plan", "pl"]).ok();
    assert_eq!(out["escalation"], "a-a1-q1");
    assert_eq!(out["attempt"], "a-a1");
    let answered_at = out["answered_at"].as_str().unwrap();
    assert!(answered_at.ends_with('Z') && answered_at.len() == 24, "{answered_at}");

    let (status, answer, at): (String, String, String) = store(&env)
        .read(|c| Ok(c.query_row("SELECT status, answer, answered_at FROM escalations WHERE id = 'a-a1-q1'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
        .unwrap();
    assert_eq!((status.as_str(), answer.as_str(), at.as_str()), ("answered", "Use the v2 field names.", answered_at));
    let answered = events(&env, "escalation-answered");
    assert_eq!(answered.len(), 1);
    assert_eq!(answered[0]["data"], json!({"escalation": "a-a1-q1"}));
    assert_eq!((answered[0]["plan"].as_str(), answered[0]["job"].as_str(), answered[0]["attempt"].as_str()), (Some("pl"), Some("a"), Some("a-a1")));
}

#[test]
fn answer_is_refused_unless_the_escalation_is_open() {
    let env = TestEnv::new();
    setup(&env);
    open_escalation(&env);
    env.cpd(&["answer", "a-a1-q1", "First.", "--plan", "pl"]).ok();
    let events_before = events(&env, "escalation-answered").len();
    refused(&env.cpd(&["answer", "a-a1-q1", "Second.", "--plan", "pl"]), "escalation_answered", 3);
    assert_eq!(query::<String>(&env, "SELECT answer FROM escalations WHERE id = 'a-a1-q1'"), "First.");
    assert_eq!(events(&env, "escalation-answered").len(), events_before);

    fixture(
        &env,
        "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at, expired_at)
         VALUES ('pl', 'a-a1-q2', 'a-a1', 2, 'Gone?', 'expired', 1, 't', 't');",
    );
    refused(&env.cpd(&["answer", "a-a1-q2", "Late.", "--plan", "pl"]), "escalation_expired", 3);
}

#[test]
fn answer_refusals_before_the_escalation() {
    let env = TestEnv::new();
    setup(&env);
    open_escalation(&env);
    refused(&env.cpd(&["answer", "a-a1-q9", "A.", "--plan", "pl"]), "escalation_not_found", 4);
    refused(&env.cpd(&["answer", "a-a1-q1", "A.", "--plan", "other"]), "escalation_not_found", 4);
    refused(&env.cpd(&["answer", "a-a1-q1", "A."]), "plan_required", 2);
    refused(&env.cpd(&["answer", "q1", "A.", "--plan", "pl"]), "invalid_escalation_id", 2);
    refused(&env.cpd(&["answer", "a-a1-q1", "", "--plan", "pl"]), "usage", 2);
    assert_eq!(query::<String>(&env, "SELECT status FROM escalations WHERE id = 'a-a1-q1'"), "open");
}
