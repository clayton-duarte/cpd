//! The rail's spacing (commands.md §17, contract §6.1.1): a fold's rows end at column W-4 of the pane, the columns W-3 to
//! W-1 being blank (the padding, the box's wall column, the margin). Red until the Inbox rows, its cue rows and the
//! Context block's rows leave that room.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::fixture::{context, plan, session, tree};
use super::inbox::{Shape, draw, item};
use super::view::View;
use cpd_core::inbox::model::{Item, Kind, Urgency};
use cpd_core::plan::Phase;
use cpd_core::session::State;
use crate::read::Place;

const W: u16 = 40;
const OPENED: &str = "2026-01-02T23:55:00.000Z";

fn now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-01-03T00:00:00.000Z", &Rfc3339).unwrap()
}

fn cells(buf: &Buffer, y: u16) -> Vec<String> {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_owned()).collect()
}

/// The column of the last cell of row `y` that is not blank.
fn last_column(buf: &Buffer, y: u16) -> usize {
    cells(buf, y).iter().rposition(|c| c.trim() != "").expect("a row with text")
}

fn blank_after(buf: &Buffer, y: u16, last: usize) -> bool {
    cells(buf, y)[last + 1..].iter().all(|c| c.trim().is_empty())
}

fn inbox_buffer(items: &[Item], shape: Shape, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(W, height)).unwrap();
    terminal.draw(|f| draw(f, f.area(), items, shape, None, now(), &|_| None)).unwrap();
    terminal.backend().buffer().clone()
}

/// The rows of the section: the empty row (0), the title (1), then the items from row 2.
const FIRST_ITEM: u16 = 2;

#[test]
fn the_inbox_row_ends_three_columns_before_the_panes_edge() {
    let items = [item(1, Kind::PrFailing, Urgency::Red, "acme/api#12 checks failed", OPENED)];
    let buf = inbox_buffer(&items, Shape::new(1, false), 5);
    let end = usize::from(W) - 4;
    assert_eq!(cells(&buf, FIRST_ITEM)[end], "m", "the age `5m` ends at column W-4: {:?}", cells(&buf, FIRST_ITEM));
    assert_eq!(last_column(&buf, FIRST_ITEM), end, "nothing after it");
    assert!(blank_after(&buf, FIRST_ITEM, end), "columns W-3 to W-1 are blank");
}

#[test]
fn a_long_inbox_label_is_cut_with_an_ellipsis_before_the_age() {
    let items = [item(1, Kind::PrFailing, Urgency::Red, &"x".repeat(80), OPENED)];
    let buf = inbox_buffer(&items, Shape::new(1, false), 5);
    let row = cells(&buf, FIRST_ITEM);
    let end = usize::from(W) - 4;
    let ellipsis = row.iter().position(|c| c == "…").expect("the label is cut with an ellipsis");
    let age = row.iter().position(|c| c == "5").expect("the age");
    assert_eq!(row[end], "m", "the age still ends at column W-4: {row:?}");
    assert!(ellipsis < age, "the `…` (column {ellipsis}) is before the age's first cell (column {age}): {row:?}");
    assert!(blank_after(&buf, FIRST_ITEM, end), "columns W-3 to W-1 are blank");
}

#[test]
fn the_inbox_scroll_cue_row_ends_where_the_rows_do() {
    let items: Vec<Item> = (0..50).map(|i| item(i, Kind::PrFailing, Urgency::Red, "label", OPENED)).collect();
    // expanded and capped at 9 rows: the empty row, the title, 4 item rows, the `▼ n more` cue, the toggle, the empty row
    let buf = inbox_buffer(&items, Shape::new(50, true).capped(9), 9);
    let cue = (0..9).find(|&y| cells(&buf, y).iter().any(|c| c == "▼")).expect("a ▼ cue row");
    assert_eq!(last_column(&buf, cue), usize::from(W) - 4, "the cue's text ends at column W-4: {:?}", cells(&buf, cue));
}

#[test]
fn a_context_block_row_is_cut_three_columns_before_the_panes_edge() {
    // a Plan row of the Context block is the one that is cut (its name) and right-aligned (its counters)
    let long = plan(
        "a-very-long-plan-id-that-cannot-fit-in-forty-columns",
        "cc",
        Phase::Dispatch,
        [0, 36, 0, 0, 1, 5],
        session("l-long", "lead", State::Idle, None, Place::Parked),
        vec![],
    );
    let mut view = View::new(tree(vec![context("cc", vec![long])]), true);
    let mut terminal = Terminal::new(TestBackend::new(W, 12)).unwrap();
    terminal.draw(|f| view.draw_at(f, 0, now())).unwrap();
    let buf = terminal.backend().buffer().clone();
    let y = (0..12).find(|&y| cells(&buf, y).iter().any(|c| c == "…")).expect("the Plan's name is cut with an ellipsis");
    assert_eq!(last_column(&buf, y), usize::from(W) - 4, "the row ends at column W-4: {:?}", cells(&buf, y));
}
