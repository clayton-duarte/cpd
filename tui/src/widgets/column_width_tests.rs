//! Red tests for audit-plan F1 in the Deploy column: a PR's title is cut and laid out in display
//! columns, not `char`s, so a wide title stays inside its row and the merge icon keeps its cell.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::text::{Line, Span};

use cpd_core::ids::SessionId;

use crate::read::{Checks, OpenPr, ReviewDecision};
use crate::widgets::host::Usage;

use super::{MERGE, Machine, draw};

/// Ten `計` (U+8A08): ten `char`s, twenty display columns.
const WIDE: &str = "計計計計計計計計計計";

/// A clean open PR whose title is the wide text twice over (forty display columns), owned by a Lead.
fn pr(title: &str) -> OpenPr {
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

/// The row `y` as it is drawn: a wide symbol takes two cells, its second blank cell is skipped.
fn row(buf: &ratatui::buffer::Buffer, y: u16) -> Line<'static> {
    let mut text = String::new();
    let mut x = 0;
    while x < buf.area.width {
        let symbol = buf[(x, y)].symbol();
        text.push_str(symbol);
        x += u16::try_from(Span::raw(symbol).width().max(1)).unwrap();
    }
    Line::from(text)
}

#[test]
fn a_pr_title_in_wide_characters_stays_in_its_row_and_the_merge_icon_in_its_column() {
    const WIDTH: u16 = 30;
    let machine = Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } };
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 20)).unwrap();
    terminal.draw(|f| drop(draw(f, f.area(), &[pr(&WIDE.repeat(2))], &machine, 0, 0))).unwrap();
    let buf = terminal.backend().buffer();
    // The PR's two rows are rows 6 and 7 of the column: neither is wider than the column it was given.
    for y in 6..8 {
        assert!(row(buf, y).width() <= usize::from(WIDTH), "row {y} is wider than the {WIDTH}-column Deploy column: {:?}", row(buf, y));
    }
    assert_eq!(buf[(WIDTH - 1, 7)].symbol(), MERGE, "a wide title pushes the merge icon out of the second line's last column");
}
