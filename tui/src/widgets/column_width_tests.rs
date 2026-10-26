//! Red tests for audit-plan F1 in the Deploy column: a PR's title is cut and laid out in display
//! columns, not `char`s, so a wide title stays inside its row and the merge icon keeps its cell.

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use cpd_core::ids::SessionId;

use crate::read::{Checks, OpenPr, ReviewDecision};
use crate::widgets::host::Usage;

use super::{MERGE, Machine, draw, pr_lines};

/// Ten `計` (U+8A08): ten `char`s, twenty display columns.
const WIDE: &str = "計計計計計計計計計計";

/// A clean open PR whose title is the wide text twice over (forty display columns), owned by a Lead.
fn open_pr(title: &str) -> OpenPr {
    OpenPr {
        repo: "acme/repo".to_owned(),
        number: 7,
        url: "https://github.com/acme/repo/pull/7".to_owned(),
        title: title.to_owned(),
        draft: false,
        checks: Checks::Passed,
        review_decision: ReviewDecision::None,
        conflicting: false,
        update: None,
        clean: true,
        active: false,
        lead: Some(SessionId::parse("l-a").unwrap()),
        lead_working: false,
        plan_failed: false,
        plan_phase: None,
        merging: false,
    }
}

#[test]
fn a_pr_title_in_wide_characters_stays_in_its_row_and_the_merge_icon_in_its_column() {
    const WIDTH: u16 = 30;
    let pr = open_pr(&WIDE.repeat(2));
    // The function's result: neither of the PR's two lines is wider than the column it was given.
    for (i, line) in pr_lines(&pr, usize::from(WIDTH), 0).iter().enumerate() {
        assert!(line.width() <= usize::from(WIDTH), "line {i} is {} columns wide in a {WIDTH}-column column: {line:?}", line.width());
    }
    // The production draw path: the wide title does not push the merge icon out of the second line's last column.
    let machine = Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } };
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 20)).unwrap();
    terminal.draw(|f| drop(draw(f, f.area(), &[pr], &machine, 0, 0, &[], (false, 0), time::OffsetDateTime::UNIX_EPOCH, &Default::default()))).unwrap();
    let buf = terminal.backend().buffer();
    assert_eq!(buf[(WIDTH - 1, 7)].symbol(), MERGE, "the merge icon sits in the last column of the second line, not pushed out");
}
