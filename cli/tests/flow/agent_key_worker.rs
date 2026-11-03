//! The Worker's agent key: its Session is named `<context>:<plan>:<job>`, one Session per Attempt, the same name for
//! every Attempt of a Job. The Attempt runs through the driver harness (fake tmux, `ManualClock`).

use super::driver::{JOB, PLAN, fixture};
use super::driver_open::live;

fn timed_out(live: &super::driver_open::Live, _id: &str, nap: usize) {
    if nap == 1 {
        fixture(
            &live.fx.env,
            "UPDATE attempts SET stop_request = 'timed_out' WHERE plan = 'pay' AND status = 'running';",
        );
    }
}

fn worker_names(live: &super::driver_open::Live) -> Vec<String> {
    let count: i64 = live.value("SELECT count(*) FROM sessions WHERE role = 'worker'");
    (0..count)
        .map(|n| {
            live.value(&format!(
                "SELECT name FROM sessions WHERE role = 'worker' ORDER BY rowid LIMIT 1 OFFSET {n}"
            ))
        })
        .collect()
}

#[test]
fn agent_key_names_worker_is_named_context_colon_plan_colon_job() {
    let live = live("");
    live.drive(JOB, timed_out);
    assert_eq!(worker_names(&live), ["ctx:pay:api"]);
}

#[test]
fn agent_key_names_every_attempt_of_a_job_is_the_same_worker_name() {
    let live = live("");
    live.drive(JOB, timed_out);
    live.fx
        .cpd(&["job", "retry", JOB, "--tier", "S1", "--plan", PLAN])
        .ok();
    let (id, _) = live.drive(JOB, timed_out);
    assert_eq!(id, "api-a2");
    assert_eq!(worker_names(&live), ["ctx:pay:api", "ctx:pay:api"]);
}
