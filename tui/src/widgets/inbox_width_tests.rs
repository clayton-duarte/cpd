//! Red tests for audit-tree F1 in the Inbox: an item's row is laid out in display columns, not `char`s.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::inbox::{self, Shape};
use cpd_core::inbox::model::{Kind, Urgency};

/// Ten `計` (U+8A08): ten `char`s, twenty display columns.
const WIDE: &str = "計計計計計計計計計計";

fn now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-01-01T00:03:00.000Z", &Rfc3339).unwrap()
}

/// Draws one item in a `width` x `height` pane and returns the buffer.
fn drawn(item: &cpd_core::inbox::model::Item, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let pane = Rect::new(0, 0, width, height);
    terminal
        .draw(|f| inbox::draw(f, pane, std::slice::from_ref(item), Shape::new(1, false), None, now(), &|_| None))
        .unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn an_inbox_item_with_wide_text_fills_its_row_to_the_width() {
    // The Rail's body: contain() leaves a margin, the wall and its padding leave columns 3.., so a 41-column pane has a
    // 35-column body, its last column 37.
    let width = 41;
    let item = inbox::item(1, Kind::Unread, Urgency::White, WIDE, "2026-01-01T00:00:00.000Z");
    let buf = drawn(&item, width, 5);
    let body_row = 2;
    let last = 37;
    let age = "3m";
    let tail: String = ((last + 1 - age.len() as u16)..=last).map(|x| buf[(x, body_row)].symbol()).collect();
    assert_eq!(tail, age, "the age ends in the body's last column: the row is laid out in display columns, not chars");
    let row: String = (0..width).map(|x| buf[(x, body_row)].symbol()).collect();
    assert!(row.trim_end().ends_with(age), "the age is on the row, not pushed out of the pane: {row:?}");

}
