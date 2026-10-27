//! E3 and the 30 s provider hold (decision pro-d102), on the fake ports: time moves only on the `ManualClock`.

use std::time::Duration;

use cpd_core::clock::{Clock, timestamp};
use cpd_core::ports::Fakes;

use super::schedule;
use super::testkit::Fx;

#[test]
fn e3_starts_a_job_in_its_provider_hold_only_after_30_seconds_on_the_clock() {
    let mut fx = Fx::new("");
    fx.fixture(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '/repos/r', 'main', 'dispatch', 'cpd/pay/repo', '/repos/r.lead', 'Charge the card', 'config', cpd_txn(), 't', 't');",
    );
    let fakes = Fakes::new();
    fakes.spawn.set_self_exe(std::env::current_exe().unwrap());
    fx.engine.ports = fakes.ports.clone();
    let ended = timestamp(fakes.clock.utc());
    fx.add_job(
        "api",
        &[],
        &format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at, driver_pid)
               VALUES ('pay', 'api-a1', 'api', 1, 'S1', 'ladder', 'ended', 'failed', 'provider_error',
                       'cpd/pay--api-a1', '/w', '{ended}', '{ended}', 7);"
        ),
    );

    fakes.clock.advance(Duration::from_secs(10));
    schedule::run(&mut fx.engine);
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM attempts"), 1);
    assert_eq!(fx.job_status("api"), "queued");
    assert_eq!(fx.count("error"), 0);

    fakes.clock.advance(Duration::from_secs(20));
    schedule::run(&mut fx.engine);
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM attempts"), 2);
}
