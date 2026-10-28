//! The flat Inbox section (commands.md §13.2, §17): a blank row, the header, a ▲ line, two rows per item, a ▼ line.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::inbox::{self, Hit, Shape, item};
use crate::palette;
use crate::test_text::row;
use cpd_core::inbox::model::{Item, Kind, Urgency};

const CAP_L: &str = "\u{E0B6}";
const CAP_R: &str = "\u{E0B4}";
const OPENED: &str = "2026-01-02T23:55:00.000Z";

fn now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-01-03T00:00:00.000Z", &Rfc3339).unwrap()
}

fn failing(id: i64, label: &str) -> Item {
    item(id, Kind::PrFailing, Urgency::Red, label, OPENED)
}

fn numbered(n: usize) -> Vec<Item> {
    (1..=n).map(|i| failing(i as i64, &format!("item {i}"))).collect()
}

fn draw(items: &[Item], shape: Shape, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| {
            inbox::draw(f, Rect::new(0, 0, width, height), items, shape, now(), &|_| None);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn flat(items: &[Item], shape: Shape) -> Buffer {
    draw(items, shape, 40, shape.height())
}

fn header(width: usize, count: usize) -> String {
    format!("───{CAP_L}Inbox{CAP_R}{}{CAP_L}{count}{CAP_R}─", "─".repeat(width - 3 - 7 - 2 - count.to_string().len() - 1))
}

/// The cell at `x` of row `y`, as its symbol.
fn at(buf: &Buffer, x: u16, y: u16) -> &str {
    buf[(x, y)].symbol()
}

#[test]
fn the_header_is_the_flush_title_in_the_sections_second_row() {
    let buf = flat(&numbered(2), Shape::new(2));
    assert_eq!(row(&buf, 0), "");
    assert_eq!(row(&buf, 1), format!("───{CAP_L}Inbox{CAP_R}{}{CAP_L}2{CAP_R}─", "─".repeat(26)));
    assert_eq!(row(&buf, 1).chars().count(), 40);
    assert_eq!(at(&buf, 0, 1), "─");
}

#[test]
fn an_item_row_has_one_blank_column_on_each_side() {
    let items = [failing(1, "acme/api#12 checks failed")];
    let buf = flat(&items, Shape::new(1));
    assert_eq!(at(&buf, 0, 3), " ");
    assert_eq!(at(&buf, 1, 3), palette::TIER_RED);
    assert_eq!(at(&buf, 2, 3), " ");
    let label: String = (3..3 + 25).map(|x| at(&buf, x, 3)).collect();
    assert_eq!(label, "acme/api#12 checks failed");
    assert_eq!((at(&buf, 37, 3), at(&buf, 38, 3), at(&buf, 39, 3)), ("5", "m", " "), "the age ends at column 38");
}

#[test]
fn a_long_label_is_cut_with_an_ellipsis_before_the_age() {
    let items = [failing(1, &"x".repeat(80))];
    let buf = flat(&items, Shape::new(1));
    let cut: String = (3..37).map(|x| at(&buf, x, 3)).collect();
    assert!(cut.contains('…'), "the label is cut with an ellipsis before the age: {cut:?}");
    assert_eq!((at(&buf, 37, 3), at(&buf, 38, 3), at(&buf, 39, 3)), ("5", "m", " "));
}

#[test]
fn an_item_with_wide_text_fills_its_row_to_the_width() {
    let items = [item(1, Kind::PrFailing, Urgency::Red, "計計計計計計計計計計", "2026-01-02T23:57:00.000Z")];
    let buf = draw(&items, Shape::new(1), 41, 5);
    let line = row(&buf, 3);
    assert!(line.ends_with("3m"), "{line:?}");
    assert_eq!(at(&buf, 39, 3), "m", "the text ends in column 39 (W-2)");
    assert_eq!(at(&buf, 40, 3), " ");
}

#[test]
fn the_section_is_a_blank_row_the_header_a_cue_line_the_items_and_a_cue_line() {
    let shape = Shape::new(2);
    assert_eq!(shape.height(), 8);
    let buf = flat(&numbered(2), shape);
    for y in [0, 2, 4, 6, 7] {
        assert_eq!(row(&buf, y), "", "row {y} is blank (no cue)");
    }
    assert_eq!(row(&buf, 1), header(40, 2));
    assert_eq!(row(&buf, 3).split_whitespace().nth(2), Some("1"));
    assert_eq!(row(&buf, 5).split_whitespace().nth(2), Some("2"));
}

#[test]
fn with_no_item_the_list_is_the_up_to_date_row_under_the_cue_line() {
    let shape = Shape::new(0);
    assert_eq!(shape.height(), 5);
    let buf = flat(&[], shape);
    for y in [0, 2, 4] {
        assert_eq!(row(&buf, y), "", "row {y}");
    }
    assert_eq!(row(&buf, 1), header(40, 0));
    assert_eq!(row(&buf, 3), " up to date, drink some water");
    assert_eq!(at(&buf, 0, 3), " ");
    let muted = palette::muted_text().fg.unwrap();
    for x in 1.." up to date, drink some water".chars().count() as u16 {
        assert_eq!(buf[(x, 3)].fg, muted, "cell {x}");
    }
}

#[test]
fn an_overflowing_section_scrolls_between_its_cue_lines() {
    let items = numbered(12);
    let drawn = |scroll| draw(&items, Shape::new(12).capped(9).scrolled(scroll), 30, 9);
    // 5 list rows: 2 items (rows 3-6), row 7 blank, the cue on row 8
    let cue = |text: &str| format!("{}{text}", " ".repeat(29 - text.chars().count()));
    let items_at = |buf: &Buffer, first: usize| {
        for (i, y) in [3, 5].into_iter().enumerate() {
            let words: Vec<String> = row(buf, y).split_whitespace().map(str::to_owned).collect();
            assert_eq!(words[1..], ["item".to_owned(), (first + i + 1).to_string(), "5m".to_owned()], "row {y}");
            assert_eq!(row(buf, y + 1), "", "Row 2 of a one-row label is blank");
        }
        assert_eq!(row(buf, 7), "", "the odd row stays blank");
    };
    let top = drawn(0);
    assert_eq!(row(&top, 2), "");
    items_at(&top, 0);
    assert_eq!(row(&top, 8), cue("▼ 10 more"));
    let middle = drawn(3);
    assert_eq!(row(&middle, 2), cue("▲ 3 more"));
    items_at(&middle, 3);
    assert_eq!(row(&middle, 8), cue("▼ 7 more"));
    let end = drawn(100);
    assert_eq!(row(&end, 2), cue("▲ 10 more"));
    items_at(&end, 10);
    assert_eq!(row(&end, 8), "");
}

#[test]
fn the_shape_is_four_rows_plus_its_list_capped_at_the_pane() {
    assert_eq!(Shape::new(0).height(), 5);
    assert_eq!(Shape::new(1).height(), 6);
    assert_eq!(Shape::new(2).height(), 8);
    assert_eq!(Shape::new(12).height(), 28);
    assert_eq!(Shape::new(12).capped(9).height(), 9);
    assert_eq!(Shape::new(12).capped(3).height(), 3);
}

#[test]
fn only_the_header_and_the_item_rows_are_hits() {
    let hits = |shape: Shape, rows: std::ops::Range<u16>| rows.map(|r| shape.hit(r)).collect::<Vec<_>>();
    assert_eq!(
        hits(Shape::new(2), 0..9),
        [None, Some(Hit::Header), None, Some(Hit::Item(0)), Some(Hit::Item(0)), Some(Hit::Item(1)), Some(Hit::Item(1)), None, None]
    );
    let scrolled = Shape::new(12).capped(9).scrolled(3);
    assert_eq!((scrolled.hit(2), scrolled.hit(7), scrolled.hit(8)), (None, None, None));
    assert_eq!(hits(scrolled, 3..7), [Some(Hit::Item(3)), Some(Hit::Item(3)), Some(Hit::Item(4)), Some(Hit::Item(4))]);
    assert_eq!(Shape::new(0).hit(3), None);
}

#[test]
fn the_wheel_moves_three_items_a_notch_and_clamps_at_the_last_item() {
    let shape = Shape::new(12).capped(9);
    assert_eq!(shape.max_scroll(), 10);
    assert_eq!(shape.wheeled(true), 3);
    assert_eq!(shape.scrolled(3).wheeled(true), 6);
    assert_eq!(shape.scrolled(9).wheeled(true), 10);
    assert_eq!(shape.scrolled(2).wheeled(false), 0);
    assert_eq!(shape.scrolled(10).wheeled(false), 7);
    assert_eq!(Shape::new(2).max_scroll(), 0);
}
