//! The cockpit reads every cause the `attempts` table admits (contract §4.4; Plan `cause-parse`, A1, A2).

use crate::common;

use common::causes::{attempt_causes, outcome_of};
use common::fx::Fx;
use common::sql;
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::store::Store;
use cpd_tui::read;

#[test]
fn the_cockpit_reads_every_cause_the_attempts_table_admits() {
    let fx = Fx::new();
    let causes = attempt_causes(&fx.env);
    assert!(causes.iter().any(|c| c == "merged") && causes.iter().any(|c| c == "gate_failed"), "the cause list read is {causes:?}");

    let mut store = Store::open(&CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap(), 5000).unwrap();
    sql(&mut store, "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '2026-01-01T00:00:00.000Z', 'm1');");

    let id = AttemptId::parse("api-a1").unwrap();
    for (i, cause) in causes.iter().enumerate() {
        // One Plan per cause, with one ended Attempt: the flow of a Plan names the first unreadable cause.
        let (pl, plan) = (format!("p{i}"), PlanId::parse(&format!("p{i}")).unwrap());
        let outcome = outcome_of(cause);
        let merged = if outcome == "merged" { "'abc123'" } else { "NULL" };
        sql(
            &mut store,
            &format!(
                "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead-{pl}', 'lead', 'ctx', '{pl}', 'Lead', '/', 'O3', 'idle', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('{pl}', 'ctx', 'Pay', 'dispatch', 'lead-{pl}', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z');
                 INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('{pl}', 'repo', '/repos/pay', 'pay', 'main', 'dispatch', 'cpd/{pl}', '/worktrees/lead', cpd_txn(), 't', 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('{pl}', 'repo', 'api', 'Job api', 'S2', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', 'running', 0, '{fp}', cpd_txn(), '2026-01-02T00:00:00.000Z');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, merged_sha, started_at, ended_at)
                   VALUES ('{pl}', 'api-a1', 'api', 1, 'S2', 'ladder', 'ended', '{outcome}', '{cause}', 'cpd/{pl}--api-a1', '/w/api-a1', {merged},
                           '2026-01-03T00:00:00.000Z', '2026-01-03T01:00:00.000Z');",
                fp = "0".repeat(64)
            ),
        );

        let flow = read::flow(&mut store, &plan).unwrap_or_else(|e| panic!("the flow read fails for cause {cause:?}: {e:?}"));
        let node = flow.jobs.iter().flat_map(|j| &j.attempts).find(|a| a.id == id).expect("the Attempt is in the flow");
        assert_eq!(node.cause.map(|c| c.as_str()), Some(cause.as_str()));
    }
}
