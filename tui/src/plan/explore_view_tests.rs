//! The Plan view's chart draws one Plan-level ` explore Jobs ` box above the Workflow boxes for a Plan that has an explore Job
//! (Plan `explore-view`, D1-A): the same pills as a Workflow box, no footer and no need-line to a Workflow. The cost/active-time rule
//! is unchanged (pro-d237). Fixed `now_ms` and frame: no wall clock, no sleep, no git, no tmux.

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{JobKey, PlanId, SessionId};
use cpd_core::store::Store;

use crate::palette;
use crate::read::{self, JobStatus};
use crate::test_text::rows;

use super::boxes;
use super::Centre;

const NOW_MS: i64 = 1_800_000_000_000;
const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn pay() -> PlanId {
    PlanId::parse("pay").unwrap()
}

fn run(store: &mut Store, sql: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

/// A store of Context `ctx`, Lead `lead1` and Plan `pay` (phase `dispatch`) with Workflow `api` (`/repo`, base `main`).
fn db() -> (tempfile::TempDir, Store) {
    let (dir, mut store) = db_without_workflow();
    run(
        &mut store,
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'api', '/repo', 'main', 'dispatch', 'cpd/pay/api', '/wt/api', cpd_txn(), '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');",
    );
    (dir, store)
}

/// [`db`] without the Workflow: an explore-only Plan's store (Context `ctx`, Lead `lead1`, Plan `pay`, phase `dispatch`).
fn db_without_workflow() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    run(
        &mut store,
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', NULL, 'lead1', '/repo', 'O3', 'idle', '2026-01-02T00:00:00.000Z', '2026-01-01T00:00:00.000Z');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL);
         UPDATE sessions SET plan = 'pay' WHERE id = 'lead1';",
    );
    (dir, store)
}

/// A Workflow Job `key` of Workflow `pr`, queued or running, Tier S3.
fn workflow_job(workflow: &str, key: &str, status: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, created_at, kind)
         VALUES ('pay', '{workflow}', '{key}', 'Job', 'S3', 'G.', 'W.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, '{FP}', cpd_txn(),
                 '2026-01-02T00:00:00.000Z', 'work');"
    )
}

/// An explore Job `key` (kind `scout`, no Workflow) of Job status `status`, Tier S1, with no Attempt.
fn explore_job(key: &str, status: &str) -> String {
    let failed_cause = if status == "failed" { "'crashed'" } else { "NULL" };
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, created_at, kind, failed_cause)
         VALUES ('pay', NULL, '{key}', 'Job', 'S1', 'G.', 'W.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, '{FP}', cpd_txn(),
                 '2026-01-02T00:00:00.000Z', 'scout', {failed_cause});"
    )
}

/// An explore Job `key` (running) with its Attempt `<key>-a1` (running).
fn explore_running(key: &str) -> String {
    explore_job(key, "running")
        + &format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at)
             VALUES ('pay', '{key}-a1', '{key}', 1, 'S1', 'ladder', 'running', NULL, NULL, 'cpd/pay--{key}-a1', '/run/pay/explore',
                     '2026-01-02T01:00:00.000Z', NULL);"
        )
}

/// The chart of Plan `pay` as the Plan view draws it, at spinner `frame`.
fn chart(store: &mut Store, frame: usize) -> super::flow::Chart {
    let plan = read::plan_boxes_at(store, &pay(), NOW_MS).unwrap();
    boxes::layout(&plan.workflows, &plan.jobs, &plan.needs_user, frame).chart
}

/// A [`Centre`] as the Plan view holds it, its read straight from `store` at spinner `frame`.
fn centre(store: &mut Store, frame: usize) -> Centre {
    let plan = read::plan_boxes_at(store, &pay(), NOW_MS).unwrap();
    let boxes = boxes::layout(&plan.workflows, &plan.jobs, &plan.needs_user, frame);
    let slot = read::ChatSlot { session: SessionId::parse("lead1").unwrap(), name: "lead1".into(), plan: plan.plan.clone(), moved_event: 1 };
    Centre { slot, plan, boxes, frame }
}

/// The pill of `key` in `chart`, as (its icon cell's glyph, its colours).
fn pill(chart: &super::flow::Chart, key: &str) -> (String, ratatui::style::Style) {
    let p = chart.pills.iter().find(|p| p.key == key).unwrap_or_else(|| panic!("the {key} pill: {:?}", chart.pills.iter().map(|p| &p.key).collect::<Vec<_>>()));
    let cell = &chart.buf[(p.rect.x + 1, p.rect.y)];
    (cell.symbol().to_owned(), ratatui::style::Style::new().fg(cell.fg).bg(cell.bg))
}

#[test]
fn the_chart_draws_an_explore_jobs_box_above_the_pr_boxes() {
    let (_dir, mut store) = db();
    run(&mut store, &(workflow_job("api", "api-1", "queued") + &explore_job("look", "queued")));
    let chart = chart(&mut store, 0);
    let lines = rows(&chart.buf);

    let explore = lines.iter().position(|l| l.contains(" explore Jobs ")).expect("the explore Jobs box");
    let api = lines.iter().position(|l| l.contains(" api · ")).expect("the api Workflow box");
    assert!(explore < api, "the explore box is above the Workflow box: {lines:?}");
    assert!(lines[explore + 1].contains("look"), "the look pill is inside the explore box: {:?}", lines[explore + 1]);
    assert!(lines[api + 1].contains("api-1"), "the api pill stays inside its Workflow box: {:?}", lines[api + 1]);
}

#[test]
fn an_explore_only_plans_chart_is_the_explore_box_alone() {
    let (_dir, mut store) = db_without_workflow();
    run(&mut store, &explore_job("look", "queued"));
    let chart = chart(&mut store, 0);
    let lines = rows(&chart.buf);

    assert_eq!(lines.iter().filter(|l| l.contains('╭')).count(), 1, "one box only: {lines:?}");
    assert!(lines.iter().any(|l| l.contains(" explore Jobs ")), "the explore box: {lines:?}");
    assert!(lines.iter().any(|l| l.contains("look")), "its pill: {lines:?}");
    assert!(!lines.iter().any(|l| l.contains("no Jobs")), "no Workflow box and no `no Jobs` box: {lines:?}");
}

#[test]
fn a_running_explore_job_spins_and_a_failed_one_turns() {
    // a running explore Job spins and its pill's icon turns with the frame
    let (_dir, mut store) = db();
    run(&mut store, &explore_running("look"));
    assert!(centre(&mut store, 0).spins(), "a running explore Job spins");
    let (at0, _) = pill(&chart(&mut store, 0), "look");
    let (at1, _) = pill(&chart(&mut store, 1), "look");
    assert_ne!(at0, at1, "the explore pill's icon differs between frames");

    // a failed explore Job draws red and turns while the Lead works on it
    let (_dir, mut store) = db();
    run(&mut store, &explore_job("look", "failed"));
    let plan = read::plan_boxes_at(&mut store, &pay(), NOW_MS).unwrap();
    let mut needs = plan.needs_user.clone();
    needs.worked_on = vec![JobKey::parse("look").unwrap()];
    let working = boxes::layout(&plan.workflows, &plan.jobs, &needs, 0).chart;
    let turning = boxes::layout(&plan.workflows, &plan.jobs, &needs, 1).chart;
    let (icon0, style0) = pill(&working, "look");
    let (icon1, _) = pill(&turning, "look");
    let red = palette::job_pill(JobStatus::Failed, false, true);
    assert_eq!(style0, ratatui::style::Style::new().fg(red.fg.unwrap()).bg(red.bg.unwrap()), "a failed explore pill is red");
    assert_ne!(icon0, icon1, "a failed explore pill turns while its Lead works");
}

#[test]
fn exact_rendered_rows_for_one_pr_box_and_one_explore_box() {
    let (_dir, mut store) = db();
    run(&mut store, &(workflow_job("api", "api-1", "queued") + &explore_job("look", "queued")));
    let chart = chart(&mut store, 0);
    let got = rows(&chart.buf).join("\n");
    let want = "\
╭─ explore Jobs ─╮
│ \u{e0b6}\u{f0766} look│S1\u{e0b4}    │
╰────────────────╯


╭─ api · repo \u{f19b0} main · dispatch ─╮
│ \u{e0b6}\u{f0766} api-1│S3\u{e0b4}                   │
╰────────────────────────────────╯";
    assert_eq!(got, want, "\n{got}");
}
