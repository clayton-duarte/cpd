//! Red tests: `pr_look`, `Dot::working` and the two surfaces that must call them.

use ratatui::buffer::CellWidth;
use cpd_core::config::Tier;
use cpd_core::ids::{JobKey, PrKey};
use cpd_core::session::State;

use super::{Dot, JobNode, JobStatus, NeedsUser, PrFacts, PrNode, PrState, PrStatus, Pull, UpdateState, pr_look};
use crate::palette::{self, Meaning};
use crate::plan::boxes;

fn facts(state: PrState) -> PrFacts {
    PrFacts { state, draft: false, clean: false, conflicting: false, owned: false, update: None, asked: false }
}

#[test]
fn an_open_clean_pr_reads_open_in_the_ready_colour() {
    let look = pr_look(&PrFacts { clean: true, ..facts(PrState::Open) });
    assert_eq!(look.state, ("open", palette::pr_open(true)));
    assert_eq!(look.conflict, None);
}

#[test]
fn an_open_unclean_pr_reads_open_in_the_open_colour() {
    let look = pr_look(&facts(PrState::Open));
    assert_eq!(look.state, ("open", palette::pr_open(false)));
    assert_eq!(look.conflict, None);
}

#[test]
fn a_draft_pr_reads_draft_in_the_planning_colour() {
    let look = pr_look(&PrFacts { draft: true, ..facts(PrState::Open) });
    assert_eq!(look.state, ("draft", Meaning::Planning));
}

#[test]
fn a_conflicting_owned_pr_adds_conflict_in_the_owned_colour() {
    let look = pr_look(&PrFacts { conflicting: true, owned: true, ..facts(PrState::Open) });
    assert_eq!(look.state, ("open", palette::pr_open(false)));
    assert_eq!(look.conflict, Some(palette::PR_CONFLICT_OWNED));
}

#[test]
fn a_conflicting_unowned_pr_adds_conflict_in_the_plain_colour() {
    let look = pr_look(&PrFacts { conflicting: true, owned: false, ..facts(PrState::Open) });
    assert_eq!(look.conflict, Some(palette::PR_CONFLICT));
}

#[test]
fn an_update_job_turns_conflict_into_updating() {
    for owned in [true, false] {
        let running = pr_look(&PrFacts { conflicting: true, owned, update: Some(UpdateState::Running), ..facts(PrState::Open) });
        assert_eq!(running.conflict, Some(palette::PR_UPDATING_RUNNING), "owned {owned}");
        let queued = pr_look(&PrFacts { conflicting: true, owned, update: Some(UpdateState::Queued), ..facts(PrState::Open) });
        assert_eq!(queued.conflict, Some(palette::PR_UPDATING_QUEUED), "owned {owned}");
    }
}

#[test]
fn a_closed_pr_the_user_asked_to_close_reads_closed_in_its_asked_colour() {
    let asked = pr_look(&PrFacts { asked: true, conflicting: true, ..facts(PrState::Closed) });
    assert_eq!(asked.state, palette::PR_CLOSED_ASKED);
    assert_eq!(asked.conflict, None, "only an open PR conflicts");
    let plain = pr_look(&facts(PrState::Closed));
    assert_eq!(plain.state, ("closed", palette::pr_state(PrState::Closed)));
}

#[test]
fn a_merged_pr_reads_merged() {
    let look = pr_look(&PrFacts { conflicting: true, ..facts(PrState::Merged) });
    assert_eq!(look.state, ("merged", palette::pr_state(PrState::Merged)));
    assert_eq!(look.conflict, None);
}

fn pull(state: PrState) -> Pull {
    Pull {
        number: 12,
        state,
        url: "https://github.com/acme/repo/pull/12".into(),
        conflicting: false,
        clean: false,
        lead_working: false,
        update: None,
        draft: false,
    }
}

fn pr_node(pull: Pull) -> PrNode {
    PrNode {
        key: PrKey::parse("api").unwrap(),
        repo_name: "api-repo".into(),
        base: "main".into(),
        status: PrStatus::Open,
        needs: Vec::new(),
        pull: Some(pull),
        waiting_on: Vec::new(),
        superseded_by: None,
        elapsed_seconds: None,
    }
}

fn failed_job() -> JobNode {
    JobNode {
        key: JobKey::parse("api-1").unwrap(),
        pr: PrKey::parse("api").unwrap(),
        seq: 0,
        title: String::new(),
        status: JobStatus::Failed,
        tier: Some(Tier::parse("S1").unwrap()),
        needs: Vec::new(),
        appended: false,
        retry_tier: None,
        shown_tier: Some(Tier::parse("S1").unwrap()),
        failed_cause: None,
        cancel_reason: None,
        merged_sha: None,
        waiting_on: Vec::new(),
        attempts: Vec::new(),
    }
}

/// The plan box's top row (its title) of one PR box, as cells.
fn box_top(pull: Pull, jobs: &[JobNode], frame: usize) -> Vec<ratatui::buffer::Cell> {
    let chart = boxes::layout(&[pr_node(pull)], jobs, &NeedsUser::default(), frame).chart;
    (0..chart.buf.area.width).map(|x| chart.buf[(x, 0)].clone()).collect()
}

/// The row as plain text: the title is drawn as a link, so a link piece holds two cells in its first one and its
/// second cell is covered by it; the OSC 8 sequences are stripped.
fn text(cells: &[ratatui::buffer::Cell]) -> String {
    let mut out = String::new();
    let mut covered = 0;
    for cell in cells {
        if covered > 0 {
            covered -= 1;
            continue;
        }
        covered = cell.cell_width().saturating_sub(1);
        let symbol = cell.symbol();
        let mut rest = symbol;
        while let Some(start) = rest.find("\x1b]8;;") {
            out.push_str(&rest[..start]);
            let end = rest[start..].find('\x07').expect("an OSC 8 sequence ends with BEL");
            rest = &rest[start + end + 1..];
        }
        out.push_str(rest);
    }
    out
}

#[test]
fn the_plan_box_title_of_a_draft_pr_says_draft() {
    let top = text(&box_top(Pull { draft: true, ..pull(PrState::Open) }, &[], 0));
    assert!(top.contains("#12 draft"), "{top}");
    assert!(!top.contains("#12 open"), "{top}");
}

#[test]
fn the_plan_box_uses_the_conflict_word_of_pr_look() {
    for update in [None, Some(UpdateState::Running), Some(UpdateState::Queued)] {
        let look = pr_look(&PrFacts { conflicting: true, owned: true, update, ..facts(PrState::Open) });
        let (word, _) = look.conflict.expect("a conflict word");
        let top = text(&box_top(Pull { conflicting: true, update, ..pull(PrState::Open) }, &[], 0));
        assert!(top.contains(&format!("#12 {word}")), "{update:?}: {top}");
    }
}

fn spinner_fg(cells: &[ratatui::buffer::Cell]) -> Option<ratatui::style::Color> {
    cells.iter().find(|c| palette::SPINNER.iter().any(|g| c.symbol().contains(g))).map(|c| c.fg)
}

#[test]
fn a_working_lead_of_a_plan_with_a_failed_job_shows_the_red_loader_in_the_plan_box() {
    let cells = box_top(Pull { lead_working: true, ..pull(PrState::Open) }, &[failed_job()], 2);
    assert_eq!(spinner_fg(&cells), palette::dot_span(Dot::Fixing, 2, false).style.fg);
}

#[test]
fn a_working_lead_of_a_plan_without_a_failed_job_shows_the_plain_spinner_in_the_plan_box() {
    assert_eq!(Dot::working(false, None), Dot::Working(None));
    let cells = box_top(Pull { lead_working: true, ..pull(PrState::Open) }, &[], 2);
    assert_eq!(spinner_fg(&cells), palette::dot_span(Dot::Working(None), 2, false).style.fg);
}

#[test]
fn dot_working_is_the_working_branch_of_dot_of() {
    assert_eq!(Dot::working(false, None), Dot::Working(None));
    assert_eq!(Dot::working(true, None), Dot::Fixing);
    for failed in [false, true] {
        assert_eq!(Dot::of(State::Working, None, failed, None), Dot::working(failed, None));
    }
}
