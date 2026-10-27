//! Red tests: the phase colour of a working Lead's dot in the plan view's PR box title and in the Dispatch column's
//! open-PR rows, from docs/commands.md §13.2 ("PR boxes", the open-PR rows of the `PRs` section) and §17 ("The Lead
//! dot rule"), and docs/contract.md §6.1.1, amended by dot-docs: a working Lead's spinner is the bright pair of its
//! Plan's phase colour (grounding grey 15, plan magenta 13, dispatch cyan 14, deploy yellow 11, done green 10), a
//! working Session that leads no Plan is cyan 14, and a Plan with a failed Job shows the red loader whatever its
//! phase. The stubs read and draw no phase, so these tests are red until dot-impl-views reads it.

use cpd_core::config::Tier;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{JobKey, PlanId, PrKey, SessionId};
use cpd_core::plan::Phase;
use cpd_core::store::Store;
use ratatui::style::Color;

use crate::palette;
use crate::palette::numbered::{C9, C10, C11, C13, C14, C15};
use crate::plan::boxes;
use crate::read::{self, JobNode, JobStatus, NeedsUser, OpenPr, PrNode, PrState, PrStatus, Pull};
use crate::widgets::column;

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
        assert_eq!(boxes.needs_user.lead_phase, Some(phase), "Plan {plan}");
    }
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
    }
}

/// A failed Job of `api`, so the box title's Lead dot is the red loader.
fn failed_job() -> JobNode {
    JobNode {
        key: JobKey::parse("j1").unwrap(),
        pr: PrKey::parse("api").unwrap(),
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

/// `the_pr_box_title_dot_follows_the_plans_phase`: the Lead's dot after the state word is the bright pair of the
/// Plan's phase colour, cyan with no Plan, the red loader with a failed Job, and absent when the Lead is not working.
#[test]
fn the_pr_box_title_dot_follows_the_plans_phase() {
    for (phase, colour) in [
        (Phase::Grounding, C15),
        (Phase::Plan, C13),
        (Phase::Dispatch, C14),
        (Phase::Deploy, C11),
        (Phase::Done, C10),
    ] {
        let needs = NeedsUser { lead_working: true, lead_phase: Some(phase), ..NeedsUser::default() };
        assert_eq!(title_dot(&pr_node(true), &[], &needs, 3), Some(colour), "{phase:?}");
    }
    let cyan = NeedsUser { lead_working: true, lead_phase: None, ..NeedsUser::default() };
    assert_eq!(title_dot(&pr_node(true), &[], &cyan, 3), Some(C14), "a working Session that leads no Plan is cyan");
    let failed = NeedsUser { lead_working: true, lead_phase: Some(Phase::Deploy), ..NeedsUser::default() };
    assert_eq!(title_dot(&pr_node(true), &[failed_job()], &failed, 3), Some(C9), "a failed Job: the red loader whatever the phase");
    let idle = NeedsUser { lead_working: true, lead_phase: Some(Phase::Deploy), ..NeedsUser::default() };
    assert_eq!(title_dot(&pr_node(false), &[], &idle, 3), None, "an idle Lead draws no dot");
}

/// `the_open_pr_rows_read_their_owners_plan_phase`: the Dispatch column's read gives a Plan-owned pull request the
/// phase of the Plan that owns it, and `None` to one no Plan owns.
#[test]
fn the_open_pr_rows_read_their_owners_plan_phase() {
    let (_dir, mut store) = fixture();
    let (prs, _) = read::open_prs(&mut store).unwrap();
    for (i, (phase, _, _)) in PHASES.iter().enumerate() {
        let number = i as i64 + 1;
        let pr = prs.iter().find(|p| p.number == number).unwrap_or_else(|| panic!("pull request #{number}"));
        assert_eq!(pr.plan_phase, Some(*phase), "pull request #{number}");
    }
    let unowned = prs.iter().find(|p| p.number == 6).expect("the pull request no Plan owns");
    assert_eq!(unowned.plan_phase, None, "a pull request no Plan owns has no phase");
}

/// An open pull request of `acme/repo#7` a Plan owns, with the new `plan_phase`.
fn open_pr(lead_working: bool, plan_failed: bool, plan_phase: Option<Phase>) -> OpenPr {
    OpenPr {
        repo: "acme/repo".to_owned(),
        number: 7,
        url: "https://github.com/acme/repo/pull/7".to_owned(),
        title: "A title".to_owned(),
        draft: false,
        checks: read::Checks::Passed,
        review_decision: read::ReviewDecision::None,
        conflicting: false,
        update: None,
        clean: false,
        active: false,
        lead: Some(SessionId::parse("l-a").unwrap()),
        jira: None,
        lead_working,
        plan_failed,
        plan_phase,
        merging: false,
    }
}

/// The foreground of the Lead's dot in the second line of an open-PR row, found by its spinner glyph.
fn row_dot(pr: &OpenPr, frame: usize) -> Option<Color> {
    let [_, second] = column::pr_lines(pr, 60, frame);
    let dot = second.spans.iter().find(|span| palette::SPINNER.iter().any(|glyph| span.content.as_ref() == *glyph))?;
    dot.style.fg
}

/// `the_open_pr_row_dot_follows_the_plans_phase`: the second line of a working Lead's open-PR row shows the dot in
/// the bright pair of the Plan's phase colour, cyan with no Plan, the red loader with a failed Job, and none when
/// the Lead is not working.
#[test]
fn the_open_pr_row_dot_follows_the_plans_phase() {
    for (phase, colour) in [(Phase::Dispatch, C14), (Phase::Deploy, C11), (Phase::Done, C10)] {
        assert_eq!(row_dot(&open_pr(true, false, Some(phase)), 3), Some(colour), "{phase:?}");
    }
    assert_eq!(row_dot(&open_pr(true, false, None), 3), Some(C14), "a working Session that leads no Plan is cyan");
    assert_eq!(row_dot(&open_pr(true, true, Some(Phase::Deploy)), 3), Some(C9), "a failed Job: the red loader whatever the phase");
    assert_eq!(row_dot(&open_pr(false, false, Some(Phase::Deploy)), 3), None, "an idle Lead draws no dot");
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
