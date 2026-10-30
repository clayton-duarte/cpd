//! The commands that name the Lead's Workflow (contract §3.6 "The Lead's Workflow", commands.md §1.3): run AS the Plan's
//! working Lead (`CPD_SESSION`), a command of the list sets `sessions.working_pr` to the key of its PR; anyone else, a Lead
//! that is not `working`, a command that writes nothing, a read: nothing. In-process (`Fx`) against a Plan of two PRs, `a` and
//! `b`, written as fixtures; the Lead's `state` and the Workers' rows are fixtures too.

use serde_json::{Value, json};

use super::job::{add_kind, add_needing, count, engine, fixture, lock, plan, setup, shaped_pr, spec_file, write_spec};
use crate::common::fx::Fx;
use crate::common::{Run, refused};

const LEAD: &str = "lead-pl";

/// Plan `pl` in phase `plan` with PRs `a` and `b` (and `repo`), the Lead `working` and naming `before`.
fn world(before: Option<&str>) -> Fx {
    world_shaped(before, "config")
}

/// [`world`] where PR `b` has the shape `shape`.
fn world_shaped(before: Option<&str>, shape: &str) -> Fx {
    let fx = Fx::with_ports();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    shaped_pr(&fx.env, "pl", "a", "config");
    shaped_pr(&fx.env, "pl", "b", shape);
    fixture(&fx.env, &format!("UPDATE sessions SET state = 'working' WHERE id = '{LEAD}';"));
    if let Some(pr) = before {
        fixture(&fx.env, &format!("UPDATE sessions SET working_pr = '{pr}' WHERE id = '{LEAD}';"));
    }
    fx
}

fn named(fx: &Fx) -> Option<String> {
    fx.env.query(&format!("SELECT working_pr FROM sessions WHERE id = '{LEAD}'"))
}

/// `cpd <args>` run as the session `session` (`None`: the User at a terminal).
fn as_session(fx: &Fx, session: Option<&str>, args: &[&str]) -> Run {
    match session {
        Some(id) => fx.cpd_with(fx.env.root(), args, &[("CPD_SESSION", id)]),
        None => fx.cpd_with(fx.env.root(), args, &[]),
    }
}

fn as_lead(fx: &Fx, args: &[&str]) -> Run {
    as_session(fx, Some(LEAD), args)
}

/// A work Job `key` of PR `pr`, added by the User (it names nothing).
fn seed(fx: &Fx, key: &str, pr: &str) {
    let spec = spec_file(&fx.env);
    add_kind(fx, "pl", key, "work", &spec, pr).ok();
}

// ---------- one per command ----------

#[test]
fn working_pr_cmd_job_add_names_the_leads_workflow() {
    let fx = world(None);
    let spec = spec_file(&fx.env);
    as_lead(&fx, &["job", "add", "k", "--title", "Title of k", "--tier", "S2", "--spec", &spec, "--pr", "b", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
}

#[test]
fn working_pr_cmd_job_edit_names_the_leads_workflow() {
    let fx = world(Some("a"));
    seed(&fx, "k", "b");
    as_lead(&fx, &["job", "edit", "k", "--title", "Other", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
}

#[test]
fn working_pr_cmd_job_retry_names_the_leads_workflow() {
    let fx = world(None);
    seed(&fx, "k", "b");
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "k", "failed");
    as_lead(&fx, &["job", "retry", "k", "--tier", "S3", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
}

#[test]
fn working_pr_cmd_job_cancel_names_the_leads_workflow() {
    let fx = world(Some("a"));
    seed(&fx, "k", "b");
    as_lead(&fx, &["job", "cancel", "k", "--reason", "No.", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
}

#[test]
fn working_pr_cmd_job_urgent_names_the_leads_workflow() {
    let fx = world(None);
    seed(&fx, "k", "b");
    as_lead(&fx, &["job", "urgent", "k", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
}

#[test]
fn working_pr_cmd_job_approve_tests_names_the_leads_workflow() {
    let fx = world_shaped(Some("a"), "tdd");
    add_needing(&fx, "pl", "t", "test", &[], "b").ok();
    add_needing(&fx, "pl", "i", "impl", &["t"], "b").ok();
    lock(&fx.env, "pl");
    as_lead(&fx, &["job", "approve-tests", "i", "--path", "tests/t.rs", "--reason", "Wrong.", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
}

#[test]
fn working_pr_cmd_answer_names_the_leads_workflow() {
    let fx = world(Some("a"));
    seed(&fx, "k", "b");
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "k", "running");
    fixture(
        &fx.env,
        "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
         VALUES ('pl', 'k-a1-q1', 'k-a1', 1, 'Which?', 'open', 1, 't');",
    );
    as_lead(&fx, &["answer", "k-a1-q1", "This one.", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
}

#[test]
fn working_pr_cmd_pr_add_names_the_leads_workflow() {
    let fx = world(Some("a"));
    let repo = fx.keyed_repo("web");
    as_lead(&fx, &["pr", "add", "c", "--repo", repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("c"), "the NEW PR");
}

#[test]
fn working_pr_cmd_pr_edit_names_the_leads_workflow() {
    let fx = world(Some("a"));
    as_lead(&fx, &["pr", "edit", "b", "--title", "Other title", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
}

#[test]
fn working_pr_cmd_pr_describe_names_the_leads_workflow() {
    let fx = world(None);
    let file = fx.env.root().join("body.md");
    std::fs::write(&file, "## What\n\nIt.\n").unwrap();
    as_lead(&fx, &["pr", "describe", "--pr", "b", "--file", file.to_str().unwrap(), "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
}

#[test]
fn working_pr_cmd_pr_cancel_names_the_leads_workflow() {
    let fx = world(Some("a"));
    as_lead(&fx, &["pr", "cancel", "b", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"), "a cancelled PR may be named");
}

// ---------- the rules of the writer ----------

#[test]
fn working_pr_cmd_last_write_wins_between_two_workflows() {
    let fx = world(None);
    seed(&fx, "ja", "a");
    seed(&fx, "jb", "b");
    as_lead(&fx, &["job", "urgent", "ja", "--plan", "pl"]).ok();
    as_lead(&fx, &["job", "urgent", "jb", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
}

#[test]
fn working_pr_cmd_naming_the_named_workflow_again_writes_no_history() {
    let fx = world(None);
    seed(&fx, "k1", "b");
    seed(&fx, "k2", "b");
    as_lead(&fx, &["job", "urgent", "k1", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
    let rows = || fx.env.query::<i64>(&format!("SELECT count(*) FROM history WHERE tbl = 'sessions' AND pk LIKE '%{LEAD}%'"));
    let before = (rows(), count(&fx.env, "events"));
    as_lead(&fx, &["job", "urgent", "k2", "--plan", "pl"]).ok();
    assert_eq!(count(&fx.env, "events"), before.1 + 1, "the second command wrote its own Event");
    assert_eq!(rows(), before.0, "naming the named PR writes no history row");
    assert_eq!(named(&fx).as_deref(), Some("b"));
}

#[test]
fn working_pr_cmd_a_worker_or_the_user_names_nothing() {
    let fx = world(None);
    seed(&fx, "k", "b");
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "k", "running");
    fixture(
        &fx.env,
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
         VALUES ('w1', 'worker', 'ctx', 'pl', 'k-a1', 'k-a1', '/w', 'S2', 'working', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
         VALUES ('mgr', 'manager', 'ctx', NULL, 'Manager', '/', 'O5', 'working', 't', 't');",
    );
    for (n, session) in [None, Some("w1"), Some("mgr")].into_iter().enumerate() {
        // Each run is a real write (a new Job appended to `b`), so a naming would have happened.
        let spec = spec_file(&fx.env);
        let key = format!("n{n}");
        as_session(&fx, session, &["job", "add", &key, "--title", "New", "--tier", "S2", "--spec", &spec, "--pr", "b", "--plan", "pl"]).ok();
        assert_eq!(named(&fx), None, "{session:?}");
    }
}

#[test]
fn working_pr_cmd_a_lead_that_is_not_working_names_nothing_and_the_command_succeeds() {
    let fx = world(None);
    seed(&fx, "k", "b");
    fixture(&fx.env, &format!("UPDATE sessions SET state = 'idle' WHERE id = '{LEAD}';"));
    as_lead(&fx, &["job", "urgent", "k", "--plan", "pl"]).ok();
    assert_eq!(named(&fx), None);
    assert_eq!(fx.env.query::<i64>("SELECT urgent FROM jobs WHERE key = 'k'"), 1);
}

#[test]
fn working_pr_cmd_a_scout_job_names_no_workflow() {
    let fx = world(Some("a"));
    let spec = write_spec(
        &fx.env,
        "scout.json",
        &json!({"goal": "Where is it?", "context": "Look.", "acceptance": ["names the file"], "out_of_scope": []}).to_string(),
    );
    let run = as_lead(&fx, &["job", "add", "s1", "--title", "Scout", "--tier", "H2", "--spec", &spec, "--kind", "scout", "--plan", "pl"]);
    assert_eq!(run.ok()["job"]["pr"], Value::Null);
    assert_eq!(named(&fx).as_deref(), Some("a"), "an explore Job has no PR: the name is untouched");
}

#[test]
fn working_pr_cmd_a_refused_command_names_nothing() {
    let fx = world(Some("a"));
    seed(&fx, "m", "b");
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "m", "merged");
    refused(&as_lead(&fx, &["job", "urgent", "m", "--plan", "pl"]), "illegal_transition", 3);
    assert_eq!(named(&fx).as_deref(), Some("a"));
}

#[test]
fn working_pr_cmd_reads_name_nothing() {
    let fx = world(Some("a"));
    seed(&fx, "k", "b");
    let txns = count(&fx.env, "txns");
    as_lead(&fx, &["job", "show", "k", "--plan", "pl"]).ok();
    as_lead(&fx, &["plan", "show", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("a"));
    assert_eq!(count(&fx.env, "txns"), txns, "a read writes no txn");
}

#[test]
fn working_pr_cmd_the_name_goes_when_the_lead_leaves_working() {
    let fx = world(None);
    seed(&fx, "k", "b");
    as_lead(&fx, &["job", "urgent", "k", "--plan", "pl"]).ok();
    assert_eq!(named(&fx).as_deref(), Some("b"));
    as_lead(&fx, &["session", "state", "idle", "--session", LEAD]).ok();
    assert_eq!(named(&fx), None);
}
