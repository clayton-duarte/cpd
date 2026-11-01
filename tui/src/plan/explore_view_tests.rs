//! The Plan view's chart draws one Plan-level ` explore Jobs ` box above the Workflow boxes for a Plan that has an explore Job
//! (Plan `explore-view`, D1-A): the same pills as a Workflow box, no footer and no need-line to a Workflow. The cost/active-time rule
//! is unchanged (pro-d237). Fixed `now_ms` and frame: no wall clock, no sleep, no git, no tmux.

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{JobKey, PlanId, SessionId};
use cpd_core::store::Store;

use cpd_core::plan::jobs::{KINDS, STATUSES, is_explore};
use ratatui::style::Modifier;

use crate::link::Links;
use crate::palette;
use crate::read::{self, JobStatus};
use crate::test_text::rows;

use super::Centre;
use super::boxes;

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
    explore_job_of("scout", key, status)
}

/// An explore Job `key` of kind `kind` (no PR) of Job status `status`, Tier S1, with no Attempt: it satisfies the CHECKs of
/// `jobs` for the status (`failed_cause`; `cancel_reason` with `cancel_requested_at`; `merged_sha` with a `summary`).
fn explore_job_of(kind: &str, key: &str, status: &str) -> String {
    let failed_cause = if status == "failed" {
        "'crashed'"
    } else {
        "NULL"
    };
    let (cancel_reason, cancel_at) = if status == "cancelled" {
        ("'stop'", "'2026-01-03T00:00:00.000Z'")
    } else {
        ("NULL", "NULL")
    };
    let (merged_sha, summary) = if status == "merged" {
        ("'abc123'", "'Found it.'")
    } else {
        ("NULL", "NULL")
    };
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, created_at, kind, failed_cause)
         VALUES ('pay', NULL, '{key}', 'Job', 'S1', 'G.', 'W.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, '{FP}', cpd_txn(),
                 '2026-01-02T00:00:00.000Z', '{kind}', {failed_cause}, {cancel_reason}, {cancel_at}, {merged_sha}, {summary});"
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
    let slot = read::ChatSlot {
        session: SessionId::parse("lead1").unwrap(),
        name: "lead1".into(),
        plan: plan.plan.clone(),
        moved_event: 1,
    };
    Centre {
        slot,
        plan,
        boxes,
        frame,
    }
}

/// The pill of `key` in `chart`, as (its icon cell's glyph, its colours).
fn pill(chart: &super::flow::Chart, key: &str) -> (String, ratatui::style::Style) {
    let p = chart
        .pills
        .iter()
        .find(|p| p.key == key)
        .unwrap_or_else(|| {
            panic!(
                "the {key} pill: {:?}",
                chart.pills.iter().map(|p| &p.key).collect::<Vec<_>>()
            )
        });
    let cell = &chart.buf[(p.rect.x + 1, p.rect.y)];
    (
        cell.symbol().to_owned(),
        ratatui::style::Style::new().fg(cell.fg).bg(cell.bg),
    )
}

#[test]
fn the_chart_draws_an_explore_jobs_box_above_the_pr_boxes() {
    let (_dir, mut store) = db();
    run(
        &mut store,
        &(workflow_job("api", "api-1", "queued") + &explore_job("look", "queued")),
    );
    let chart = chart(&mut store, 0);
    let lines = rows(&chart.buf);

    let explore = lines
        .iter()
        .position(|l| l.contains(" explore Jobs "))
        .expect("the explore Jobs box");
    let api = lines
        .iter()
        .position(|l| l.contains(" api · "))
        .expect("the api Workflow box");
    assert!(
        explore < api,
        "the explore box is above the Workflow box: {lines:?}"
    );
    assert!(
        lines[explore + 1].contains("look"),
        "the look pill is inside the explore box: {:?}",
        lines[explore + 1]
    );
    assert!(
        lines[api + 1].contains("api-1"),
        "the api pill stays inside its Workflow box: {:?}",
        lines[api + 1]
    );
}

#[test]
fn an_explore_only_plans_chart_is_the_explore_box_alone() {
    let (_dir, mut store) = db_without_workflow();
    run(&mut store, &explore_job("look", "queued"));
    let chart = chart(&mut store, 0);
    let lines = rows(&chart.buf);

    assert_eq!(
        lines.iter().filter(|l| l.contains('╭')).count(),
        1,
        "one box only: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains(" explore Jobs ")),
        "the explore box: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("look")),
        "its pill: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("no Jobs")),
        "no Workflow box and no `no Jobs` box: {lines:?}"
    );
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
    assert_eq!(
        style0,
        ratatui::style::Style::new()
            .fg(red.fg.unwrap())
            .bg(red.bg.unwrap()),
        "a failed explore pill is red"
    );
    assert_ne!(
        icon0, icon1,
        "a failed explore pill turns while its Lead works"
    );
}

#[test]
fn exact_rendered_rows_for_one_pr_box_and_one_explore_box() {
    let (_dir, mut store) = db();
    run(
        &mut store,
        &(workflow_job("api", "api-1", "queued") + &explore_job("look", "queued")),
    );
    let chart = chart(&mut store, 0);
    let got = rows(&chart.buf).join("\n");
    // the title is ` explore Jobs · 1 scout ` (24 cells); the box is as wide as the title plus 4 (28), the pill row as before
    let inner = "─".repeat(26);
    let want = format!(
        "\
╭─ explore Jobs · 1 scout ─╮
│ \u{e0b6}\u{f0766} look│S1\u{e0b4}{}│
╰{inner}╯


╭─ api · repo \u{f19b0} main · dispatch ─╮
│ \u{e0b6}\u{f0766} api-1│S3\u{e0b4}                   │
╰────────────────────────────────╯",
        " ".repeat(14)
    );
    assert_eq!(got, want, "\n{got}");
}

/// The first row of `chart` that holds ` explore Jobs `: the explore box's top border.
fn explore_border(chart: &super::flow::Chart) -> (usize, String) {
    let lines = rows(&chart.buf);
    let at = lines
        .iter()
        .position(|l| l.contains(" explore Jobs "))
        .unwrap_or_else(|| panic!("the explore box: {lines:?}"));
    (at, lines[at].clone())
}

#[test]
fn the_explore_box_title_counts_the_jobs_per_kind() {
    let (_dir, mut store) = db();
    run(
        &mut store,
        &(pr_job("api", "api-1", "queued")
            + &explore_job_of("scout", "look", "queued")
            + &explore_job_of("scout", "peek", "queued")
            + &explore_job_of("research", "dig", "queued")),
    );
    let chart = chart(&mut store, 0);
    let (explore, border) = explore_border(&chart);
    assert!(
        border.contains(" explore Jobs · 1 research · 2 scout "),
        "the title carries the counts per kind: {border:?}"
    );
    let lines = rows(&chart.buf);
    let api = lines
        .iter()
        .position(|l| l.contains(" api · "))
        .expect("the api PR box");
    assert!(
        explore < api,
        "the explore box is above the PR box: {lines:?}"
    );
}

#[test]
fn an_explore_only_plans_box_title_has_the_counts() {
    let (_dir, mut store) = db_without_pr();
    run(
        &mut store,
        &(explore_job_of("scout", "look", "merged") + &explore_job_of("research", "dig", "failed")),
    );
    let chart = chart(&mut store, 0);
    let first = rows(&chart.buf).remove(0);
    assert!(
        first.contains(" explore Jobs · 1 research · 1 scout "),
        "the first row carries the counts: {first:?}"
    );
}

#[test]
fn the_explore_box_title_counts_every_explore_job_whatever_its_status() {
    let (_dir, mut store) = db_without_pr();
    let sql: String = ["queued", "running", "merged", "failed", "cancelled"]
        .iter()
        .map(|st| explore_job_of("scout", &format!("scout-{st}"), st))
        .collect();
    run(&mut store, &sql);
    let chart = chart(&mut store, 0);
    assert_eq!(
        chart.pills.len(),
        5,
        "one pill per explore Job: {:?}",
        chart.pills.iter().map(|p| &p.key).collect::<Vec<_>>()
    );
    let want = format!(" explore Jobs · {} scout ", chart.pills.len());
    let first = rows(&chart.buf).remove(0);
    assert!(
        first.contains(&want),
        "the number in the title is the number of pills in the box, whatever their status; want {want:?} in {first:?}"
    );
}

#[test]
fn the_explore_box_title_is_text_only_in_the_title_style() {
    let (_dir, mut store) = db_without_pr();
    run(&mut store, &explore_job_of("scout", "look", "queued"));
    let plan = read::plan_boxes_at(&mut store, &pay(), NOW_MS).unwrap();
    let drawn = boxes::layout(&plan.prs, &plan.jobs, &plan.needs_user, 0);
    assert_eq!(drawn.links, Links::default(), "the counts are no link");

    let title = " explore Jobs · 1 scout ";
    let top: Vec<&str> = (0..drawn.chart.buf.area.width)
        .map(|x| drawn.chart.buf[(x, 0)].symbol())
        .collect();
    let text = top.concat();
    let start = text
        .find(title)
        .unwrap_or_else(|| panic!("the title {title:?} on the top border: {text:?}"));
    let first_cell = text[..start].chars().count() as u16;
    let want_fg = palette::read_only_text().fg.unwrap();
    for x in first_cell..first_cell + title.chars().count() as u16 {
        let cell = &drawn.chart.buf[(x, 0)];
        assert_eq!(
            cell.fg,
            want_fg,
            "the title cell {x} ({:?}) has the read-only text colour",
            cell.symbol()
        );
        assert!(
            !cell.modifier.contains(Modifier::BOLD),
            "the title cell {x} ({:?}) is not bold",
            cell.symbol()
        );
    }
}

/// Every Job of the Plan `pay`, for each kind and each status: one per PR for a kind that belongs to a PR, one for the Plan
/// for an explore kind. Satisfies the CHECKs and triggers of `jobs` (a fixture that breaks one is the test's defect).
fn every_job() -> String {
    let mut sql = String::new();
    for status in STATUSES {
        let draft = status == "draft";
        let fingerprint = if draft {
            "NULL".to_owned()
        } else {
            format!("'{FP}'")
        };
        let failed_cause = if status == "failed" {
            "'crashed'"
        } else {
            "NULL"
        };
        let (cancel_reason, cancel_at) = if status == "cancelled" {
            ("'stop'", "'2026-01-03T00:00:00.000Z'")
        } else {
            ("NULL", "NULL")
        };
        for kind in KINDS {
            let explore = is_explore(kind);
            let summary = if explore && status == "merged" {
                "'Found it.'"
            } else {
                "NULL"
            };
            let merged_sha = if status == "merged" {
                "'abc123'"
            } else {
                "NULL"
            };
            let (tests, test_files) = if kind == "test" {
                ("'[\"t\"]'", "'[\"f\"]'")
            } else {
                ("NULL", "NULL")
            };
            let homes: Vec<Option<&str>> = if explore {
                vec![None]
            } else {
                [
                    "p-waiting",
                    "p-dispatch",
                    "p-open",
                    "p-merged",
                    "p-cancelled",
                ]
                .map(Some)
                .to_vec()
            };
            for pr in homes {
                let (key, pr_sql) = match pr {
                    Some(pr) => (format!("{pr}-{kind}-{status}"), format!("'{pr}'")),
                    None => (format!("{kind}-{status}"), "NULL".to_owned()),
                };
                assert!(key.len() <= 32, "key {key}");
                sql += &format!(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                                       fingerprint, created_txn, created_at, kind, failed_cause, cancel_reason, cancel_requested_at,
                                       merged_sha, summary, tests, test_files)
                     VALUES ('pay', {pr_sql}, '{key}', 'Job', 'S1', 'G.', 'W.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, {fingerprint},
                             cpd_txn(), '2026-01-02T00:00:00.000Z', '{kind}', {failed_cause}, {cancel_reason}, {cancel_at}, {merged_sha},
                             {summary}, {tests}, {test_files});"
                );
            }
        }
    }
    sql
}

/// One PR of Plan `pay` for each `prs.status`, with the `pull_requests` rows `open` and `merged` need.
const EVERY_PR: &str = "
    INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, merge_state, authored,
                               base_ref, default_branch, created_at, changed_at, merged_at)
      VALUES ('acme/repo', 1, 'https://github.com/acme/repo/pull/1', 'T', 0, 'merged', 'none', 'passed', 'clean', 1, 'main', 'main',
              '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
             ('acme/repo', 2, 'https://github.com/acme/repo/pull/2', 'T', 0, 'open', 'none', 'passed', 'clean', 1, 'main', 'main',
              '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL);
    INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
      VALUES ('pay', 'p-dispatch', '/repo', 'main', 'dispatch', 'cpd/pay/p-dispatch', '/wt/d', cpd_txn(), '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
    INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
      VALUES ('pay', 'p-waiting', '/repo', 'main', 'waiting', 'cpd/pay/p-waiting', cpd_txn(), '2026-01-01T00:00:00.000Z'),
             ('pay', 'p-cancelled', '/repo', 'main', 'cancelled', 'cpd/pay/p-cancelled', cpd_txn(), '2026-01-01T00:00:00.000Z');
    INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                     created_txn, created_at, started_at, merged_at)
      VALUES ('pay', 'p-merged', '/repo', 'main', 'merged', 'cpd/pay/p-merged', '/wt/m', 1, 'https://github.com/acme/repo/pull/1',
              '2026-01-02T00:00:00.000Z', (SELECT id FROM pull_requests WHERE number = 1), cpd_txn(), '2026-01-01T00:00:00.000Z',
              '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
    INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                     created_txn, created_at, started_at)
      VALUES ('pay', 'p-open', '/repo', 'main', 'open', 'cpd/pay/p-open', '/wt/o', 2, 'https://github.com/acme/repo/pull/2',
              '2026-01-02T00:00:00.000Z', (SELECT id FROM pull_requests WHERE number = 2), cpd_txn(), '2026-01-01T00:00:00.000Z',
              '2026-01-02T00:00:00.000Z');";

/// Fails, naming the missing and the extra keys, unless the pills drawn are the Jobs of the Plan.
fn assert_draws_every_job(store: &mut Store) {
    let mut stored: Vec<String> = store
        .write("fixture", |tx| {
            let mut stmt = tx.prepare("SELECT key FROM jobs WHERE plan = 'pay'")?;
            let keys = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, CpdError>(keys)
        })
        .unwrap();
    stored.sort();
    assert!(!stored.is_empty(), "the fixture has Jobs");
    let mut drawn: Vec<String> = chart(store, 0)
        .pills
        .iter()
        .map(|p| p.key.clone())
        .collect();
    drawn.sort();
    let missing: Vec<&String> = stored.iter().filter(|k| !drawn.contains(k)).collect();
    let extra: Vec<&String> = drawn.iter().filter(|k| !stored.contains(k)).collect();
    assert!(
        stored == drawn,
        "the Jobs drawn are the Jobs of the Plan; missing: {missing:?}; extra: {extra:?}"
    );
}

#[test]
fn the_plan_view_draws_every_job_of_the_plan() {
    // a Plan of PRs of every status with a Job of every kind and status, and the explore Jobs of the Plan
    let (_dir, mut store) = db_without_pr();
    run(&mut store, EVERY_PR);
    run(&mut store, &every_job());
    assert_draws_every_job(&mut store);

    // a Plan of no PR and the explore Jobs only
    let (_dir, mut store) = db_without_pr();
    run(&mut store, &every_job_explore_only());
    assert_draws_every_job(&mut store);
}

/// [`every_job`] without the Jobs of a PR.
fn every_job_explore_only() -> String {
    every_job()
        .split_inclusive(';')
        .filter(|stmt| stmt.contains("VALUES ('pay', NULL,"))
        .collect()
}
