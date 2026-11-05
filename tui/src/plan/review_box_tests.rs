//! The Review PR box of a review Plan with no Workflow (contract §2.6, pro-d412 A5): a bordered box with the repo, the PR
//! title and the link row, which opens the pull request on a click; and the `no Workflow yet` message it replaces.

use std::sync::Arc;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::*;
use crate::link::CUE;
use crate::read::{PrState, ReviewedPr};
use crate::test_text::rows;
use cpd_core::host::runner::{FakeRunner, Reply};
use cpd_core::ports::Fakes;

const URL: &str = "https://github.com/acme/api-server/pull/912";
const NO_WORKFLOW: &str = "no Workflow yet: cpd workflow add";

fn reviewed(state: Option<PrState>) -> ReviewedPr {
    ReviewedPr {
        repo: "acme/api-server".into(),
        number: 912,
        url: URL.into(),
        title: "Fix the retry loop".into(),
        state,
    }
}

fn review_centre(state: Option<PrState>) -> Centre {
    let mut centre = sample_centre(Vec::new(), Vec::new());
    centre.plan.reviewed_pr = Some(reviewed(state));
    centre
}

fn draw(ui: &mut Ui, centre: &Centre) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    terminal.draw(|f| ui.draw(f, Some(centre))).unwrap();
    terminal.backend().buffer().clone()
}

fn screen(centre: &Centre) -> Vec<String> {
    rows(&draw(&mut Ui::default(), centre))
}

/// A body row of the box: the margin column, the wall, one padding column, the text padded to the inner width (34), the same.
fn body(text: &str) -> String {
    format!(" │ {text:<34} │")
}

/// The five rows of the box at 40 columns, from row 1: the chart margin is one column each side, W is 38.
fn box_rows(link: &str) -> Vec<String> {
    vec![
        format!(" ╭─ Review api-server {}╮", "─".repeat(16)),
        body("acme/api-server"),
        body("Fix the retry loop"),
        body(link),
        format!(" ╰{}╯", "─".repeat(36)),
    ]
}

#[test]
fn a_review_plan_renders_a_review_pr_box_with_its_link_row() {
    let got = screen(&review_centre(Some(PrState::Open)));
    let want = box_rows(&format!("#912 open {CUE}"));
    assert_eq!(
        got[1..6],
        want.iter().map(|r| r.trim_end()).collect::<Vec<_>>(),
        "\n{}",
        got.join("\n")
    );
}

#[test]
fn a_review_plan_box_omits_the_state_when_unknown() {
    let got = screen(&review_centre(None));
    let want = box_rows(&format!("#912 {CUE}"));
    assert_eq!(
        got[1..6],
        want.iter().map(|r| r.trim_end()).collect::<Vec<_>>(),
        "\n{}",
        got.join("\n")
    );
}

#[test]
fn a_review_plan_without_workflows_does_not_say_no_workflow_yet() {
    let got = screen(&review_centre(Some(PrState::Open)));
    assert!(
        got.iter().all(|l| !l.contains("no Workflow yet")),
        "{}",
        got.join("\n")
    );
    assert!(
        got.iter().any(|l| l.contains("Review api-server")),
        "the Review PR box is drawn instead:\n{}",
        got.join("\n")
    );
}

#[test]
fn a_plan_without_reviewed_pr_still_says_no_workflow_yet() {
    let plain = screen(&sample_centre(Vec::new(), Vec::new()));
    assert!(
        plain.iter().any(|l| l.trim() == NO_WORKFLOW),
        "{}",
        plain.join("\n")
    );
    assert!(
        plain.iter().all(|l| !l.contains("Review")),
        "no box: {}",
        plain.join("\n")
    );
    // the contrast: the Plan with a reviewed PR does not say it
    let review = screen(&review_centre(Some(PrState::Merged)));
    assert!(
        review.iter().all(|l| l.trim() != NO_WORKFLOW),
        "{}",
        review.join("\n")
    );
}

fn left(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn clicking_the_review_pr_link_opens_the_url() {
    let centre = review_centre(Some(PrState::Open));
    let mut ui = Ui::default();
    let buf = draw(&mut ui, &centre);
    // the link row is the box's fourth row (row 4): its text starts at column 3
    let link_row = rows(&buf)[4].clone();
    assert!(link_row.contains("#912 open"), "{link_row:?}");
    for (x, y) in [(3, 4), (8, 4), (20, 2), (5, 3)] {
        let fakes = Fakes::new();
        let runner =
            Arc::new(FakeRunner::new((*fakes.clock).clone()).on("open", |_| true, Reply::ok("")));
        let mut ports = fakes.ports.clone();
        ports.runner = runner.clone();
        assert!(
            ui.click(&ports, left(x, y)).unwrap(),
            "a click at ({x}, {y}) is the box's"
        );
        let opened: Vec<String> = runner
            .calls()
            .iter()
            .filter(|c| c.program == "open")
            .flat_map(|c| c.args.clone())
            .collect();
        assert_eq!(opened, [URL], "({x}, {y})");
    }
}
