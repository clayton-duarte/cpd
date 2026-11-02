//! E3, scheduling (contract §3.7), the cases that end BEFORE `driver::start` starts anything: a Job in its back-off, on a Tier
//! outside the Worker range, tampered. One `schedule::run` per tick on a prepared store; no Attempt is made, no driver, no tmux.
//! (`driver::start` refuses the last two in its read-only `decide`, before it takes the driver lock; the first never reaches it.)
//! The allocation test crosses that line on the fake ports: E3 writes the Attempt rows through a `FakeSpawn`, so no process
//! starts, and reads back the branch and worktree it recorded.

use cpd_core::ports::Fakes;
use serde_json::json;

use super::schedule;
use super::testkit::{Fx, ago};

/// One tick of E3.
fn tick(fx: &mut Fx) {
    schedule::run(&mut fx.engine);
}

fn ticks(fx: &mut Fx, n: usize) {
    for _ in 0..n {
        tick(fx);
    }
}

fn texts(fx: &mut Fx, kind: &str) -> Vec<String> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT text FROM events WHERE kind = ?1 ORDER BY id")?;
            Ok(stmt
                .query_map([kind], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

fn attempts(fx: &mut Fx) -> i64 {
    fx.query("SELECT count(*) FROM attempts")
}

/// SQL for Attempt `<key>-a<number>` ended `failed` / `crashed` at `ended_at`; `driver_pid` is null when its driver never started.
fn ended_attempt(
    key: &str,
    number: u32,
    tier: &str,
    driver_pid: Option<i64>,
    ended_at: &str,
) -> String {
    let pid = driver_pid.map_or("NULL".to_owned(), |p| p.to_string());
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, driver_pid)
           VALUES ('pay', '{key}-a{number}', '{key}', {number}, '{tier}', 'ladder', 'ended', 'failed', 'crashed',
                   'cpd/pay--{key}-a{number}', '/w', '{ended_at}', '{ended_at}', {pid});"
    )
}

#[test]
fn e3_reports_a_tampered_job_once_and_does_not_start_it() {
    let mut fx = Fx::with_plan("");
    fx.add_job(
        "api",
        &[],
        &format!(
            "UPDATE jobs SET fingerprint = '{}' WHERE key = 'api';",
            "0".repeat(64)
        ),
    );

    ticks(&mut fx, 3);

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!(
        (plan.as_deref(), job.as_deref(), attempt.as_deref()),
        (Some("pay"), Some("api"), None)
    );
    assert_eq!(
        (&data["source"], &data["code"]),
        (&json!("watch"), &json!("locked_job_tampered"))
    );
    assert!(
        texts(&mut fx, "error")[0].ends_with("See: cpd job show api --plan pay"),
        "{:?}",
        texts(&mut fx, "error")
    );
    assert_eq!(attempts(&mut fx), 0);
    assert_eq!(fx.job_status("api"), "queued");
}

/// pro-d82 (hs-d554): a store written before the rule may hold a queued Job on a Tier outside H0-H3, S0-S3. E3 never starts it:
/// one `error` Event (`tier_not_allowed`) per Job, once, the Job stays `queued`, no Attempt is made.
#[test]
fn e3_does_not_start_a_job_on_a_tier_outside_the_worker_range_and_raises_one_error_per_job() {
    let mut fx = Fx::with_plan("");
    for (key, tier) in [
        ("api", "O3"),
        ("web", "O4"),
        ("app", "S4"),
        ("ui", "O2"),
        ("cli", "H4"),
    ] {
        fx.add_job_on(tier, "repo", key, &[], "");
    }

    ticks(&mut fx, 3);

    let errors = fx.events("error");
    assert_eq!(errors.len(), 5, "{errors:?}");
    let mut jobs_named: Vec<_> = errors
        .iter()
        .map(|(plan, job, attempt, _)| (plan.clone(), job.clone().unwrap(), attempt.clone()))
        .collect();
    jobs_named.sort();
    assert_eq!(
        jobs_named,
        [
            ("api", None),
            ("app", None),
            ("cli", None),
            ("ui", None),
            ("web", None)
        ]
        .map(|(job, attempt): (&str, Option<String>)| (
            Some("pay".to_string()),
            job.to_string(),
            attempt
        ))
    );
    for (_, _, _, data) in &errors {
        assert_eq!(
            (&data["source"], &data["code"]),
            (&json!("watch"), &json!("tier_not_allowed")),
            "{data}"
        );
    }
    assert!(
        texts(&mut fx, "error")
            .iter()
            .all(|t| t.contains("pro-d82")),
        "{:?}",
        texts(&mut fx, "error")
    );
    assert_eq!(attempts(&mut fx), 0);
    for key in ["api", "web", "app", "ui", "cli"] {
        assert_eq!(fx.job_status(key), "queued", "{key}");
    }
}

/// hs-d514: an engine error is an Event only; no message goes to the Context's Manager.
#[test]
fn an_engine_error_sends_no_message_to_a_manager() {
    let mut fx = Fx::with_plan("");
    fx.fixture(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('mgr1', 'manager', 'ctx', NULL, 'Manager', '/', 'O4', 'idle', 't', 't');",
    );
    fx.add_job_on("O3", "repo", "api", &[], "");

    ticks(&mut fx, 3);

    assert_eq!(fx.events("error").len(), 1);
    assert_eq!(
        fx.count("session-message"),
        0,
        "an engine error is an Event only (hs-d514)"
    );
}

/// A Job whose latest 9 Attempts never had a driver, the last just ended: E3 waits 300 s, so no tick starts it.
#[test]
fn e3_does_not_start_a_job_in_its_backoff() {
    let mut fx = Fx::with_plan("");
    let now = ago(0);
    let ended: String = (1..=9)
        .map(|n| ended_attempt("api", n, "S2", None, &now))
        .collect();
    fx.add_job("api", &[], &ended);

    ticks(&mut fx, 3);

    assert_eq!(attempts(&mut fx), 9);
    assert_eq!(fx.job_status("api"), "queued");
    assert_eq!(fx.count("error"), 0, "{:?}", texts(&mut fx, "error"));
}

/// One tick of E3 whose starter runs on the fake spawn: no process starts.
fn tick_on_fake_ports(fx: &mut Fx) {
    let fakes = Fakes::new();
    fakes.spawn.set_self_exe(std::env::current_exe().unwrap());
    fx.engine.ports = fakes.ports.clone();
    tick(fx);
}

/// The `(branch, worktree)` the starter recorded for Attempt `id` of Plan `pay`.
fn allocated(fx: &mut Fx, id: &str) -> (String, String) {
    fx.engine
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT branch, worktree FROM attempts WHERE plan = 'pay' AND id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

/// The `(lead_branch, lead_worktree, repo)` of the Plan's Workflow `repo`.
fn lead(fx: &mut Fx) -> (String, String, String) {
    fx.engine
        .store
        .read(|conn| Ok(conn.query_row("SELECT lead_branch, lead_worktree, repo FROM workflows WHERE plan = 'pay' AND key = 'repo'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
        .unwrap()
}

/// An `Fx` with Plan `pay` and its Workflow `repo` (Lead Branch `cpd/pay/repo`, Lead worktree `/repos/r.lead`): the forms the
/// Attempt branch CHECK allows. `add_job` fills the Jobs.
fn new_plan() -> Fx {
    let mut fx = Fx::new("");
    fx.fixture(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '/repos/r', 'main', 'dispatch', 'cpd/pay/repo', '/repos/r.lead', 'Charge the card', 'config', cpd_txn(), 't', 't');",
    );
    fx
}

/// The branch `cpd_core::git::attempt_branch` gives parallel Attempt `id` of Plan `pay`.
fn parallel_branch(id: &str) -> String {
    cpd_core::git::attempt_branch(
        &cpd_core::ids::PlanId::parse("pay").unwrap(),
        &cpd_core::ids::AttemptId::parse(id).unwrap(),
    )
}

/// A live linear Attempt `lin-a1` of the Workflow `repo`: on its Lead Branch, in its Lead worktree.
fn add_live_linear(fx: &mut Fx) {
    fx.add_running("lin", "running", &ago(1));
    fx.fixture("UPDATE attempts SET branch = 'cpd/pay/repo', worktree = '/repos/r.lead' WHERE id = 'lin-a1';");
}

#[test]
fn e3_starts_two_ready_siblings_of_one_pr_both_parallel() {
    let mut fx = new_plan();
    fx.add_job("api", &[], "");
    fx.add_job("web", &[], "");
    let (lead_branch, _, _) = lead(&mut fx);
    tick_on_fake_ports(&mut fx);
    for id in ["api-a1", "web-a1"] {
        let (branch, _) = allocated(&mut fx, id);
        assert_ne!(branch, lead_branch, "{id} must not be linear");
        assert_eq!(branch, parallel_branch(id), "{id} is parallel");
    }
}

#[test]
fn e3_holds_a_ready_job_while_a_linear_attempt_of_its_pr_is_live() {
    let mut fx = new_plan();
    add_live_linear(&mut fx);
    fx.add_job("api", &[], "");
    let (lead_branch, lead_worktree, _) = lead(&mut fx);
    tick_on_fake_ports(&mut fx);
    assert_eq!(fx.job_status("api"), "queued", "held");
    assert_eq!(
        fx.query::<i64>("SELECT count(*) FROM attempts WHERE job = 'api'"),
        0,
        "no Attempt row"
    );
    assert_eq!(fx.count("error"), 0, "{:?}", texts(&mut fx, "error"));

    fx.fixture("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE id = 'lin-a1'; UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE key = 'lin';");
    tick_on_fake_ports(&mut fx);
    assert_eq!(
        allocated(&mut fx, "api-a1"),
        (lead_branch, lead_worktree),
        "alone, it starts linear"
    );
}

#[test]
fn e3_holds_an_urgent_ready_job_while_a_linear_attempt_of_its_pr_is_live() {
    let mut fx = new_plan();
    fx.engine.config.concurrency = 2;
    add_live_linear(&mut fx);
    fx.add_job("api", &[], "UPDATE jobs SET urgent = 1 WHERE key = 'api';");
    tick_on_fake_ports(&mut fx);
    assert_eq!(fx.job_status("api"), "queued", "held");
    assert_eq!(
        fx.query::<i64>("SELECT count(*) FROM attempts WHERE job = 'api'"),
        0,
        "no Attempt row"
    );
    assert_eq!(fx.count("error"), 0, "{:?}", texts(&mut fx, "error"));
}

#[test]
fn e3_starts_a_lone_ready_job_linear() {
    let mut fx = new_plan();
    fx.add_job("api", &[], "");
    let (lead_branch, lead_worktree, _) = lead(&mut fx);
    tick_on_fake_ports(&mut fx);
    assert_eq!(
        allocated(&mut fx, "api-a1"),
        (lead_branch, lead_worktree),
        "a Job started alone is linear"
    );
}

#[test]
fn e3_starts_a_ready_job_parallel_beside_a_live_parallel_attempt() {
    let mut fx = new_plan();
    fx.add_running("busy", "running", &ago(1));
    fx.add_job("api", &[], "");
    tick_on_fake_ports(&mut fx);
    assert_eq!(
        allocated(&mut fx, "api-a1").0,
        parallel_branch("api-a1"),
        "a sibling of a live parallel Attempt is parallel"
    );
}

#[test]
fn e3_starts_a_ready_job_of_another_pr_beside_a_live_linear_attempt() {
    let mut fx = new_plan();
    fx.fixture(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'other', '/repos/o', 'main', 'dispatch', 'cpd/pay/other', '/repos/o.lead', 'Other', 'config', cpd_txn(), 't', 't');",
    );
    add_live_linear(&mut fx);
    fx.add_job_in("other", "web", &[], "");
    tick_on_fake_ports(&mut fx);
    assert_eq!(
        allocated(&mut fx, "web-a1"),
        ("cpd/pay/other".to_owned(), "/repos/o.lead".to_owned()),
        "not held, alone in its Workflow"
    );
}

/// A config.toml whose `worker_runner` is `line` (a whole TOML line, or none), written over the engine's home after it started.
fn write_config(fx: &Fx, line: &str) {
    let text = format!(
        "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n{line}\n\n[tmux]\nsocket = \"testkit\"\n"
    );
    std::fs::write(fx.engine.home.config(), text).unwrap();
}

/// `(id, runner)` of every Attempt, by id.
fn runners(fx: &mut Fx) -> Vec<(String, String)> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT id, runner FROM attempts ORDER BY id")?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// pro-d199: E3 re-reads `worker_runner` before it starts Attempts: the engine started on `pi`, the file now says `durable`.
#[test]
fn e3_starts_an_attempt_with_the_runner_the_config_names_now() {
    let mut fx = new_plan();
    fx.add_job("api", &[], "");
    write_config(&fx, "worker_runner = \"durable\"");
    tick_on_fake_ports(&mut fx);
    assert_eq!(
        runners(&mut fx),
        [("api-a1".to_owned(), "durable".to_owned())]
    );
}

/// pro-d199: a running Attempt keeps its stored runner and its driver; only the Attempt started after the change is `durable`.
#[test]
fn e3_leaves_a_running_attempts_runner_and_process_alone_when_the_config_changes() {
    let mut fx = new_plan();
    fx.add_running("busy", "running", &ago(1));
    fx.add_job("api", &[], "");
    write_config(&fx, "worker_runner = \"durable\"");
    tick_on_fake_ports(&mut fx);
    assert_eq!(
        runners(&mut fx),
        [
            ("api-a1".to_owned(), "durable".to_owned()),
            ("busy-a1".to_owned(), "pi".to_owned())
        ],
        "the new Worker follows the file, the running one keeps `pi`"
    );
    assert_eq!(
        fx.query::<String>("SELECT status FROM attempts WHERE id = 'busy-a1'"),
        "running"
    );
    assert_eq!(
        fx.query::<i64>("SELECT driver_pid FROM attempts WHERE id = 'busy-a1'"),
        4242
    );
}

/// pro-d199: a config that does not parse starts no Attempt this tick and is reported (`engine.fail`), no stale value.
#[test]
fn e3_with_a_config_that_does_not_parse_starts_no_attempt_and_reports_it() {
    let mut fx = new_plan();
    fx.add_job("api", &[], "");
    write_config(&fx, "worker_runner = [");
    tick_on_fake_ports(&mut fx);
    assert_eq!(attempts(&mut fx), 0, "no Attempt starts on a stale config");
    assert_eq!(fx.job_status("api"), "queued");
    assert_eq!(fx.count("error"), 1, "{:?}", texts(&mut fx, "error"));
}
