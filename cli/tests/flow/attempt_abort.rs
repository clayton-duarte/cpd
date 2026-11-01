//! `cpd attempt abort` (commands.md §6.5; contract §3.4 "attempt abort"): the Lead ends a `running` Attempt
//! `failed` / `aborted` in one write transaction, which also fails its Job with the `attempt-aborted` and
//! `job-failed` Events, and the surface (`attempt show` / `attempt list` / the TUI read) carries the new cause.
//!
//! Everything runs in this process on the fakes: the rows are written by SQL fixtures (reusing the locked-Job
//! helpers of [`super::job`]), the Tier catalog of the fake pi install is seeded, and no tmux, git or pi process
//! is ever started (the Attempt's stored runner is `durable` with no pid, so its stop is a no-op). The command
//! does not exist yet in the base, so every test below is red until the feature lands.

use crate::common;

use std::path::Path;

use common::fx::Fx;
use common::{Run, refused};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::store::Store;
use serde_json::json;

use super::job::{engine, events, job_add, job_row, lock, plan, setup};

const PLAN: &str = "pl";
const JOB: &str = "a";
const ATTEMPT: &str = "a-a1";
/// The Lead Session of Plan `pl`, as [`plan`] writes it.
const LEAD: &str = "lead-pl";

// ---------- fixtures ----------

/// A locked Plan `pl` whose Job `a` is `queued`, with its Tier catalog and fake pi install seeded, in an [`Fx`]
/// whose commands run on the fake ports.
fn locked_plan() -> Fx {
    let fx = Fx::with_ports();
    setup(&fx.env);
    plan(&fx.env, PLAN, "plan");
    job_add(&fx, PLAN, JOB, "S2", &[]).ok();
    lock(&fx.env, PLAN);
    fx
}

/// The Attempt `a-a1` of Job `a` before any Worker: no process, so the abort's stop is a no-op.
fn no_worker(fx: &Fx) {
    fx.env.sql(&format!("UPDATE attempts SET runner = 'durable', worker_pid = NULL WHERE plan = '{PLAN}' AND id = '{ATTEMPT}'"));
}

/// A locked Plan `pl` whose Job `a` is `running` with its Attempt `a-a1` (as `engine` leaves it): the Attempt runs
/// the `durable` runner with no pid, so the abort ends it and stops no process.
fn running() -> Fx {
    let fx = locked_plan();
    engine(&fx.env, PLAN, JOB, "running");
    no_worker(&fx);
    fx
}

/// A locked Plan `pl` whose Job `a` is `running` with its Attempt `a-a1` still `starting` (no Worker started yet).
fn starting() -> Fx {
    let fx = locked_plan();
    fx.env.sql(&format!(
        "UPDATE jobs SET status = 'running' WHERE plan = '{PLAN}' AND key = '{JOB}';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, runner)
           VALUES ('{PLAN}', '{ATTEMPT}', '{JOB}', 1, 'S2', 'ladder', 'starting', 'cpd/{PLAN}--{ATTEMPT}', '/w', 't', 'durable');"
    ));
    fx
}

/// A locked Plan `pl` whose Attempt `a-a1` and Job `a` the engine already ended `failed` / `timed_out`: the natural
/// end the abort races.
fn ended_by_the_engine() -> Fx {
    let fx = locked_plan();
    engine(&fx.env, PLAN, JOB, "failed");
    fx
}

/// `cpd attempt abort <attempt> --plan pl`, as the Lead's Session runs it, in this process.
fn abort(fx: &Fx) -> Run {
    fx.cpd_with(Path::new(&fx.env.root()), &["attempt", "abort", ATTEMPT, "--plan", PLAN], &[("CPD_SESSION", LEAD)])
}

/// The number of write transactions and of Events in the store.
fn writes(fx: &Fx) -> (i64, i64) {
    (fx.env.query("SELECT count(*) FROM txns"), fx.env.query("SELECT count(*) FROM events"))
}

/// The txn of the one `history` row that satisfies `where_`.
fn txn_of(fx: &Fx, where_: &str) -> i64 {
    fx.env.query(&format!("SELECT txn FROM history WHERE {where_} ORDER BY id LIMIT 1"))
}

/// The txn of the one `events` row of `kind` (the Events' `kind`, not the history table's `tbl`).
fn event_txn_of(fx: &Fx, kind: &str) -> i64 {
    fx.env.query(&format!("SELECT txn FROM events WHERE kind = '{kind}' ORDER BY id LIMIT 1"))
}

// ---------- (a) a running Attempt ends failed / aborted, in one store ----------

#[test]
fn aborts_a_running_attempt_failed_aborted() {
    let fx = running();
    let out = abort(&fx).ok();
    assert_eq!(out, json!({"ok": true, "attempt": ATTEMPT, "outcome": "failed", "cause": "aborted"}));

    assert_eq!(fx.env.query::<String>(&format!("SELECT status FROM attempts WHERE id = '{ATTEMPT}'")), "ended");
    assert_eq!(fx.env.query::<String>(&format!("SELECT outcome FROM attempts WHERE id = '{ATTEMPT}'")), "failed");
    assert_eq!(fx.env.query::<String>(&format!("SELECT cause FROM attempts WHERE id = '{ATTEMPT}'")), "aborted");
    assert!(fx.env.query::<Option<String>>(&format!("SELECT ended_at FROM attempts WHERE id = '{ATTEMPT}'")).is_some());
    assert_eq!(job_row(&fx.env, PLAN, JOB, "status"), "failed");

    let aborted = events(&fx, "attempt-aborted");
    assert_eq!(aborted.len(), 1, "exactly one `attempt-aborted` Event: {aborted:#?}");
    assert_eq!((aborted[0]["plan"].as_str(), aborted[0]["attempt"].as_str()), (Some(PLAN), Some(ATTEMPT)));
    assert_eq!(aborted[0]["data"]["tier"], "S2");
    assert_eq!(aborted[0]["data"]["by"], LEAD, "`by` is the calling Lead Session: {}", aborted[0]["data"]);

    let failed = events(&fx, "job-failed");
    assert_eq!(failed.len(), 1, "exactly one `job-failed` Event: {failed:#?}");
    assert_eq!((failed[0]["plan"].as_str(), failed[0]["job"].as_str()), (Some(PLAN), Some(JOB)));
    let text = failed[0]["text"].as_str().unwrap();
    assert!(text.contains("abort"), "the Job's failure names the abort: {text}");
    assert!(text.contains(&format!("cpd job retry {JOB} --tier")), "and the Tier-fix route: {text}");

    // The Attempt's end and both Events are one write transaction.
    let txn = txn_of(&fx, &format!("tbl = 'attempts' AND op = 'update' AND json_extract(after, '$.cause') = 'aborted'"));
    assert_eq!(event_txn_of(&fx, "attempt-aborted"), txn, "the Event shares the Attempt's transaction");
    assert_eq!(event_txn_of(&fx, "job-failed"), txn, "and the Job's failure too");
}

// ---------- (b) the retry route after the abort ----------

#[test]
fn job_retry_after_abort_works() {
    let fx = running();
    abort(&fx).ok();
    assert_eq!(job_row(&fx.env, PLAN, JOB, "status"), "failed");

    let out = fx.cpd(&["job", "retry", JOB, "--tier", "S1", "--plan", PLAN]).ok();
    assert_eq!(out, json!({"ok": true, "job": JOB, "status": "queued", "tier": "S1"}));
    assert_eq!(job_row(&fx.env, PLAN, JOB, "status"), "queued");
    assert_eq!(job_row(&fx.env, PLAN, JOB, "retry_tier"), "S1");
}

// ---------- (c) a non-running Attempt is refused and nothing is written ----------

#[test]
fn refuses_a_non_running_attempt_and_writes_nothing() {
    // A `starting` Attempt: the setup is the only way it ends, the abort never touches it.
    {
        let fx = starting();
        let before = writes(&fx);
        let error = refused(&abort(&fx), "attempt_not_running", 3);
        assert_eq!(error["details"]["status"], "starting");
        assert_eq!(writes(&fx), before, "nothing written");
        assert_eq!(events(&fx, "attempt-aborted").len(), 0);
        assert_eq!(events(&fx, "job-failed").len(), 0);
    }

    // An `ended` Attempt: the natural ending stands, the abort cannot rewrite its cause.
    {
        let fx = ended_by_the_engine();
        let before = writes(&fx);
        let error = refused(&abort(&fx), "attempt_not_running", 3);
        assert_eq!(error["details"]["status"], "ended");
        assert_eq!(error["details"]["cause"], "timed_out");
        assert_eq!(writes(&fx), before, "nothing written");
        assert_eq!(
            fx.env.query::<String>(&format!("SELECT cause FROM attempts WHERE id = '{ATTEMPT}'")),
            "timed_out",
            "the ended cause is untouched"
        );
    }
}

// ---------- (d) the race with the engine's natural end ----------

#[test]
fn race_with_the_natural_end_has_one_winner() {
    // The engine's natural end (the deadline) already ended the Attempt and failed its Job. The abort that
    // follows re-reads the status, finds it no longer `running`, is refused `attempt_not_running` and writes
    // nothing: exactly one ending wins, there is no second Event and no rewritten cause.
    {
        let fx = ended_by_the_engine();
        assert_eq!(fx.env.query::<String>(&format!("SELECT cause FROM attempts WHERE id = '{ATTEMPT}'")), "timed_out");
        let before = writes(&fx);
        let failed_before = events(&fx, "job-failed").len();
        let aborted_before = events(&fx, "attempt-aborted").len();

        let error = refused(&abort(&fx), "attempt_not_running", 3);
        assert_eq!((error["details"]["status"].as_str(), error["details"]["cause"].as_str()), (Some("ended"), Some("timed_out")));
        assert_eq!(writes(&fx), before, "no transaction was written: {error}");
        assert_eq!(events(&fx, "job-failed").len(), failed_before, "no second `job-failed`");
        assert_eq!(events(&fx, "attempt-aborted").len(), aborted_before, "no `attempt-aborted`");
        assert_eq!(fx.env.query::<String>(&format!("SELECT cause FROM attempts WHERE id = '{ATTEMPT}'")), "timed_out", "the natural cause stands");
    }

    // A second abort of the Attempt the first already ended is the same loser: one winner, never two writes.
    {
        let fx = running();
        abort(&fx).ok();
        let before = writes(&fx);
        refused(&abort(&fx), "attempt_not_running", 3);
        assert_eq!(writes(&fx), before, "the losing abort wrote nothing");
        assert_eq!(events(&fx, "attempt-aborted").len(), 1, "exactly one winner");
        assert_eq!(events(&fx, "job-failed").len(), 1);
        assert_eq!(fx.env.query::<String>(&format!("SELECT cause FROM attempts WHERE id = '{ATTEMPT}'")), "aborted");
    }
}

// ---------- (e) a Worker is refused ----------

#[test]
fn refused_for_a_worker_not_a_lead() {
    let fx = running();
    let before = writes(&fx);
    let args = ["attempt", "abort", ATTEMPT, "--plan", PLAN];
    let error = refused(&fx.cpd_with(Path::new(&fx.env.root()), &args, &[("CPD_ATTEMPT", ATTEMPT)]), "not_a_lead", 3);
    assert_eq!(writes(&fx), before, "a Worker writes nothing: {error}");
    assert_eq!(fx.env.query::<String>(&format!("SELECT status FROM attempts WHERE id = '{ATTEMPT}'")), "running");
}

// ---------- (f) `aborted` is no-ladder ----------

#[test]
fn aborted_is_not_a_ladder_cause() {
    let fx = running();
    abort(&fx).ok();
    // The Job fails: the engine requeues nothing and climbs nothing.
    assert_eq!(job_row(&fx.env, PLAN, JOB, "status"), "failed");
    assert_eq!(events(&fx, "attempt-retry").len(), 0, "no ladder retry was queued");
    assert_eq!(events(&fx, "attempt-started").len(), 0, "no new Attempt was started");
    assert_eq!(fx.env.query::<i64>(&format!("SELECT count(*) FROM attempts WHERE plan = '{PLAN}' AND job = '{JOB}'")), 1);
    assert_eq!(fx.env.query::<String>(&format!("SELECT status FROM attempts WHERE id = '{ATTEMPT}'")), "ended");
    assert_eq!(fx.env.query::<String>(&format!("SELECT cause FROM attempts WHERE id = '{ATTEMPT}'")), "aborted");
}

// ---------- (g) the surface carries `aborted` ----------

#[test]
fn list_and_show_surface_aborted() {
    let fx = running();
    abort(&fx).ok();

    let shown = fx.cpd(&["attempt", "show", ATTEMPT, "--plan", PLAN]).ok();
    assert_eq!(shown["attempt"]["outcome"], "failed");
    assert_eq!(shown["attempt"]["cause"], "aborted");

    let listed = fx.cpd(&["attempt", "list", "--plan", PLAN]).ok();
    let row = listed["attempts"].as_array().unwrap().iter().find(|a| a["id"] == ATTEMPT).expect("the Attempt is listed");
    assert_eq!((row["outcome"].as_str(), row["cause"].as_str()), (Some("failed"), Some("aborted")));

    // The TUI fixture's Attempt read carries the same cause (drawn like `timed_out`).
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let flow = cpd_tui::read::flow(&mut store, &PlanId::parse(PLAN).unwrap()).expect("the flow reads");
    let node = flow
        .jobs
        .iter()
        .flat_map(|j| &j.attempts)
        .find(|a| a.id == AttemptId::parse(ATTEMPT).unwrap())
        .expect("the Attempt is in the TUI read");
    assert_eq!(node.cause.map(|c| c.as_str()), Some("aborted"));
}
