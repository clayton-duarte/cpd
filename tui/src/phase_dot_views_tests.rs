//! Red tests: the phase colour of a working Lead's dot in the plan view's PR box title and in the Dispatch column's
//! open-PR rows, from docs/commands.md §13.2 ("PR boxes", the open-PR rows of the `PRs` section) and §17 ("The Lead
//! dot rule"), and docs/contract.md §6.1.1, amended by dot-docs: a working Lead's spinner is the bright pair of its
//! Plan's phase colour (grounding grey 15, plan magenta 13, dispatch cyan 14, deploy yellow 11, done green 10), a
//! working Session that leads no Plan is cyan 14, and a Plan with a failed Job shows the red loader whatever its
//! phase. The stubs read and draw no phase, so these tests are red until dot-impl-views reads it.

use cpd_core::config::Tier;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{JobKey, PlanId, PrKey};
use cpd_core::plan::Phase;
use cpd_core::store::Store;
use ratatui::style::Color;

use crate::palette;
use crate::plan::boxes;
use crate::read::{self, Work, JobNode, JobStatus, NeedsUser, PrNode, PrState, PrStatus, Pull};

/// Each Plan of the fixture: its phase, its id and its Lead Session.
const PHASES: [(Phase, &str, &str); 5] = [
    (Phase::Grounding, "p-grounding", "l-grounding"),
    (Phase::Plan, "p-plan", "l-plan"),
    (Phase::Dispatch, "p-dispatch", "l-dispatch"),
    (Phase::Deploy, "p-deploy", "l-deploy"),
    (Phase::Done, "p-done", "l-done"),
];

/// `the_plan_view_reads_its_leads_plan_phase`: `read::plan_boxes` gives a working Plan's Lead the Plan's own phase.
#[test]
fn the_plan_view_reads_its_leads_plan_phase() {
    let (_dir, mut store) = fixture();
    for (phase, plan, _) in PHASES {
        let boxes = read::plan_boxes(&mut store, &PlanId::parse(plan).unwrap()).unwrap();
        assert!(boxes.needs_user.lead_working, "Plan {plan}: its Lead is working");
        assert_eq!(boxes.needs_user.lead_work.is_some(), true, "Plan {plan} {phase:?}");
    }
}

/// pro-d200: the PR box title's Lead dot follows what the Lead works on, and is red for a failed Job or an open Escalation.
#[test]
fn dots_plan_box_title_dot_follows_work() {
    let needs = |work: Option<Work>, escalated: bool| NeedsUser { lead_working: true, lead_work: work, lead_escalated: escalated, ..NeedsUser::default() };
    let pr = pr_node(true);
    for (work, colour) in [
        (Some(Work::Planning), palette::numbered::C13),
        (Some(Work::Dispatch), palette::numbered::C14),
        (Some(Work::Review), palette::numbered::C11),
        (None, palette::numbered::C15),
    ] {
        for frame in [0, 5, 27] {
            assert_eq!(title_dot(&pr, &[], &needs(work, false), frame), Some(colour), "{work:?} frame {frame}");
        }
    }
    for work in [Some(Work::Planning), Some(Work::Review), None] {
        assert_eq!(title_dot(&pr, &[failed_job()], &needs(work, false), 3), Some(palette::numbered::C9), "a failed Job beats {work:?}");
        assert_eq!(title_dot(&pr, &[], &needs(work, true), 3), Some(palette::numbered::C9), "an Escalation beats {work:?}");
    }
    let idle = NeedsUser { lead_working: false, lead_work: Some(Work::Dispatch), ..NeedsUser::default() };
    assert_eq!(title_dot(&pr_node(false), &[], &idle, 0), None, "a Lead not working: no spinner");
}

/// A PR of a Plan whose Lead works (or not), with one pull request.
fn pr_node(lead_working: bool) -> PrNode {
    PrNode {
        key: PrKey::parse("api").unwrap(),
        repo_name: "api-repo".into(),
        base: "main".into(),
        status: PrStatus::Dispatch,
        needs: Vec::new(),
        pull: Some(Pull {
            number: 12,
            state: PrState::Open,
            url: "https://github.com/acme/repo/pull/12".into(),
            conflicting: false,
            clean: false,
            lead_working,
            update: None,
            draft: false,
        }),
        waiting_on: Vec::new(),
        superseded_by: None,
        elapsed_seconds: None,
        cost_usd: None,
    }
}

/// A failed Job of `api`, so the box title's Lead dot is the red loader.
fn failed_job() -> JobNode {
    JobNode {
        key: JobKey::parse("j1").unwrap(),
        pr: Some(PrKey::parse("api").unwrap()),
        seq: 0,
        title: String::new(),
        status: JobStatus::Failed,
        tier: Some(Tier::parse("S1").unwrap()),
        needs: Vec::new(),
        appended: false,
        retry_tier: None,
        shown_tier: Some(Tier::parse("S1").unwrap()),
        failed_cause: Some("boom".into()),
        cancel_reason: None,
        merged_sha: None,
        waiting_on: Vec::new(),
        attempts: Vec::new(),
    }
}

/// The foreground of the Lead's dot in the box title row, found by its spinner glyph: the whole title is a link, so
/// the glyph rides in a cell's OSC 8 symbol while the cell keeps the dot's style.
fn title_dot(pr: &PrNode, jobs: &[JobNode], needs: &NeedsUser, frame: usize) -> Option<Color> {
    let chart = boxes::layout(std::slice::from_ref(pr), jobs, needs, frame).chart;
    (0..chart.buf.area.width)
        .map(|x| &chart.buf[(x, 0)])
        .find(|cell| palette::SPINNER.iter().any(|glyph| cell.symbol().contains(glyph)))
        .map(|cell| cell.fg)
}

/// A store of one Context: a Plan in each phase with a working Lead, each owning an open, authored pull request
/// (`acme/repo#1`..`#5`), and one open, authored pull request no Plan owns (`#6`). No tmux, git or gh.
fn fixture() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let mut sql = String::from(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');",
    );
    for (i, (phase, plan, lead)) in PHASES.iter().enumerate() {
        let number = i as i64 + 1;
        let at = format!("'2026-01-0{}T00:00:00.000Z'", i + 2);
        let locked = if matches!(phase, Phase::Grounding | Phase::Plan) { "NULL".to_owned() } else { at.clone() };
        let done = if *phase == Phase::Done { at.clone() } else { "NULL".to_owned() };
        let phase = phase.as_str();
        sql.push_str(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('{lead}', 'lead', 'ctx', NULL, '{lead}', '/repo', 'O3', 'working', {at}, '2026-01-01T00:00:00.000Z');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('{plan}', 'ctx', '{lead}', '{phase}', '{lead}', '2026-01-01T00:00:00.000Z', {locked}, {done});
             UPDATE sessions SET plan = '{plan}' WHERE id = '{lead}';
             INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, authored, created_at, changed_at)
               VALUES ('acme/repo', {number}, 'https://github.com/acme/repo/pull/{number}', 'T', 0, 'open', 'none', 'passed', 1, {at}, {at});
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                              pull_request, created_txn, created_at, started_at)
               VALUES ('{plan}', 'api', '/repo', 'main', 'open', 'cpd/{plan}/api', '/wt', {number},
                       'https://github.com/acme/repo/pull/{number}', {at},
                       (SELECT id FROM pull_requests WHERE number = {number}), cpd_txn(), '2026-01-01T00:00:00.000Z', {at});"
        ));
    }
    sql.push_str(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, authored, created_at, changed_at)
           VALUES ('acme/repo', 6, 'https://github.com/acme/repo/pull/6', 'T', 0, 'open', 'none', 'passed', 1,
                   '2026-01-07T00:00:00.000Z', '2026-01-07T00:00:00.000Z');",
    );
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    (dir, store)
}
