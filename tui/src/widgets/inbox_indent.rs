//! An Inbox item row has one space after the rail (commands.md §13.2, hs-d557 item 7): the rail `│`, ONE space, the tier
//! glyph, a space, the label, the age right-aligned. Red until `inbox::item_line` drops the second space.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::inbox::{Shape, draw, item};
use ratatui::layout::Rect;
use crate::palette;
use cpd_core::inbox::model::{Kind, Urgency};

fn now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-01-03T00:00:00.000Z", &Rfc3339).unwrap()
}

/// The rows of a view with one open item (age `5m` at [`now`]) drawn at 43 columns, as text.
fn rows_with(label: &str) -> Vec<String> {
    let items = [item(1, Kind::PrFailing, Urgency::Red, label, "2026-01-02T23:55:00.000Z")];
    let mut terminal = Terminal::new(TestBackend::new(43, 16)).unwrap();
    terminal.draw(|f| draw(f, Rect::new(0, 0, 43, 16), &items, Shape::new(1, false), None, now(), &|_| None)).unwrap();
    text_rows(terminal.backend().buffer())
}

fn text_rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned())
        .collect()
}

/// The item row of the only item: the one right under the title.
fn item_row_of(rows: &[String]) -> &String {
    let title = rows.iter().position(|r| r.starts_with(" ╭─\u{E0B6}Inbox")).expect("the Inbox title");
    &rows[title + 1]
}

#[test]
fn an_item_row_is_the_rail_one_space_the_glyph_a_space_the_label_and_the_age() {
    let label = "acme/api#12 checks failed";
    let rows = rows_with(label);
    let glyph = palette::TIER_RED;
    // 41 columns of inner area: `│`, space, glyph, space (4), the label, the gap, the age
    let want = format!(" │ {glyph} {label}{}5m", " ".repeat(39 - 4 - label.chars().count() - 2));
    assert_eq!(item_row_of(&rows), &want, "{rows:?}");
    assert_eq!(item_row_of(&rows).chars().count(), 40, "the age ends in column 39 (W-4: two blank columns of padding after it)");
}

#[test]
fn a_label_that_does_not_fit_is_cut_with_an_ellipsis_one_cell_later() {
    let rows = rows_with(&"x".repeat(60));
    let glyph = palette::TIER_RED;
    // the label gets 39 - 4 - 1 (the gap) - 2 (the age) = 32 cells: 31 letters and the `…`
    let want = format!(" │ {glyph} {}… 5m", "x".repeat(31));
    assert_eq!(item_row_of(&rows), &want, "{rows:?}");
    assert_eq!(item_row_of(&rows).chars().count(), 40, "still ending at column 39");
}
