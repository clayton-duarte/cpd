//! The Tier a Job pill draws is the Job's shown Tier (docs/commands.md 13.2, `Display::job_tier`), not `jobs.tier`.

use cpd_core::config::Tier;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::store::Store;

use crate::plan::flow::{flow_jobs, layout, Live};
use crate::plan::sample_job;
use crate::read;

const CAP_L: &str = "\u{e0b6}";
const CAP_R: &str = "\u{e0b4}";

fn attempt(n: u32, tier: &str, origin: &str, ended: bool) -> String {
    let (status, outcome, tail) = if ended { ("ended", "'failed'", ", 'timed_out', '2026-01-03T00:01:00.000Z'") } else { ("running", "NULL", ", NULL, NULL") };
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, ended_at, branch, worktree, started_at)
         VALUES ('pl', 'a-a{n}', 'a', {n}, '{tier}', '{origin}', '{status}', {outcome}{tail}, 'cpd/pl--a-a{n}', '/w', '2026-01-03T00:00:00.000Z');"
    )
}

/// The report (Job open-claim-2 of Plan review-intake): tier H3, retry_tier S1, a1 H3 (ladder), a2, a3, a4 S1 (retry), a4 running.
fn reported_store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let attempts = [attempt(1, "H3", "ladder", true), attempt(2, "S1", "retry", true), attempt(3, "S1", "retry", true), attempt(4, "S1", "retry", false)].join("\n");
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES ('pl', 'ctx', 'P', 'dispatch', 'lead', 't', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pl', 'r', '/r', 'main', 'dispatch', 'cpd/pl', '/w', cpd_txn(), 't', 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, retry_tier, created_txn, created_at)
                   VALUES ('pl', 'r', 'a', 'Retried', 'H3', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{}', 'S1', cpd_txn(), 't');
                 {attempts}
                 INSERT INTO events (txn, at, kind, context, plan, job, text, data) VALUES (cpd_txn(), 't', 'job-retried', 'ctx', 'pl', 'a', 'x', '{{}}');
                 INSERT INTO events (txn, at, kind, context, plan, job, text, data) VALUES (cpd_txn(), 't', 'attempt-started', 'ctx', 'pl', 'a', 'x', '{{}}');",
                "f".repeat(64)
            ))?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

/// The cells of the first pill of `jobs`, cap to cap.
fn pill_cells(jobs: &[read::JobNode]) -> String {
    let c = layout(&flow_jobs(jobs, &[]), 0);
    let r = c.pills[0].rect;
    (r.x..r.right()).map(|x| c.buf[(x, r.y)].symbol()).collect()
}

#[test]
fn a_retried_job_loaded_from_the_ledger_draws_its_retry_tier() {
    let (_dir, mut store) = reported_store();
    let boxes = read::plan_boxes(&mut store, &PlanId::parse("pl").unwrap()).unwrap();
    assert_eq!(boxes.jobs.len(), 1);
    let icon = Live::of(&boxes.jobs[0], false).icon(0).0;
    assert_eq!(pill_cells(&boxes.jobs), format!("{CAP_L}{icon} a│#4│S1{CAP_R}"));
}

#[test]
fn a_pill_draws_the_shown_tier_not_the_stored_tier() {
    let job = read::JobNode { tier: Some(Tier::parse("H3").unwrap()), shown_tier: Some(Tier::parse("S1").unwrap()), ..sample_job("r", "a") };
    let icon = Live::Queued.icon(0).0;
    assert_eq!(pill_cells(&[job]), format!("{CAP_L}{icon} a│S1{CAP_R}"));
}
