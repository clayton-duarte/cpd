//! E3, scheduling (contract §3.7), the cases that end BEFORE `driver::start` starts anything: a Job in its back-off, on a Tier
//! outside the Worker range, tampered. One `schedule::run` per tick on a prepared store; no Attempt is made, no driver, no tmux.
//! (`driver::start` refuses the last two in its read-only `decide`, before it takes the driver lock; the first never reaches it.)

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
            Ok(stmt.query_map([kind], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

fn attempts(fx: &mut Fx) -> i64 {
    fx.query("SELECT count(*) FROM attempts")
}

/// SQL for Attempt `<key>-a<number>` ended `failed` / `crashed` at `ended_at`; `driver_pid` is null when its driver never started.
fn ended_attempt(key: &str, number: u32, tier: &str, driver_pid: Option<i64>, ended_at: &str) -> String {
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
    fx.add_job("api", &[], &format!("UPDATE jobs SET fingerprint = '{}' WHERE key = 'api';", "0".repeat(64)));

    ticks(&mut fx, 3);

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!((plan.as_deref(), job.as_deref(), attempt.as_deref()), (Some("pay"), Some("api"), None));
    assert_eq!((&data["source"], &data["code"]), (&json!("watch"), &json!("locked_job_tampered")));
    assert!(texts(&mut fx, "error")[0].ends_with("See: cpd job show api --plan pay"), "{:?}", texts(&mut fx, "error"));
    assert_eq!(attempts(&mut fx), 0);
    assert_eq!(fx.job_status("api"), "queued");
}

/// hs-d554: a store written before the rule may hold a queued Job on a Tier outside H3, S0-S3. E3 never starts it:
/// one `error` Event (`tier_not_allowed`) per Job, once, the Job stays `queued`, no Attempt is made.
#[test]
fn e3_does_not_start_a_job_on_a_tier_outside_the_worker_range_and_raises_one_error_per_job() {
    let mut fx = Fx::with_plan("");
    for (key, tier) in [("api", "O3"), ("web", "S5"), ("app", "S4"), ("ui", "H2")] {
        fx.add_job_on(tier, "repo", key, &[], "");
    }

    ticks(&mut fx, 3);

    let errors = fx.events("error");
    assert_eq!(errors.len(), 4, "{errors:?}");
    let mut jobs_named: Vec<_> = errors.iter().map(|(plan, job, attempt, _)| (plan.clone(), job.clone().unwrap(), attempt.clone())).collect();
    jobs_named.sort();
    assert_eq!(
        jobs_named,
        [("api", None), ("app", None), ("ui", None), ("web", None)].map(|(job, attempt): (&str, Option<String>)| (Some("pay".to_string()), job.to_string(), attempt))
    );
    for (_, _, _, data) in &errors {
        assert_eq!((&data["source"], &data["code"]), (&json!("watch"), &json!("tier_not_allowed")), "{data}");
    }
    assert!(texts(&mut fx, "error").iter().all(|t| t.contains("hs-d554")), "{:?}", texts(&mut fx, "error"));
    assert_eq!(attempts(&mut fx), 0);
    for key in ["api", "web", "app", "ui"] {
        assert_eq!(fx.job_status(key), "queued", "{key}");
    }
}

/// hs-d514: an engine error is an Event only; no message goes to the Context's Manager.
#[test]
fn an_engine_error_sends_no_message_to_a_manager() {
    let mut fx = Fx::with_plan("");
    fx.fixture(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('mgr1', 'manager', 'ctx', NULL, 'Manager', '/', 'O5', 'idle', 't', 't');",
    );
    fx.add_job_on("O3", "repo", "api", &[], "");

    ticks(&mut fx, 3);

    assert_eq!(fx.events("error").len(), 1);
    assert_eq!(fx.count("session-message"), 0, "an engine error is an Event only (hs-d514)");
}

/// A Job whose latest 9 Attempts never had a driver, the last just ended: E3 waits 300 s, so no tick starts it.
#[test]
fn e3_does_not_start_a_job_in_its_backoff() {
    let mut fx = Fx::with_plan("");
    let now = ago(0);
    let ended: String = (1..=9).map(|n| ended_attempt("api", n, "S2", None, &now)).collect();
    fx.add_job("api", &[], &ended);

    ticks(&mut fx, 3);

    assert_eq!(attempts(&mut fx), 9);
    assert_eq!(fx.job_status("api"), "queued");
    assert_eq!(fx.count("error"), 0, "{:?}", texts(&mut fx, "error"));
}
