//! The Dispatch column's Inbox: the flat section pinned to the bottom of the rows under the machine (commands.md §13.2).

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::column::{self, Drawn, Machine};
use super::host::Usage;
use super::inbox::{Hit, item};
use crate::palette;
use crate::test_text::row;
use cpd_core::inbox::model::{Item, Kind, Urgency};

const CAP_L: &str = "\u{E0B6}";
const CAP_R: &str = "\u{E0B4}";
const W: u16 = 30;

fn machine() -> Machine {
    Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } }
}

fn items(n: usize) -> Vec<Item> {
    (1..=n).map(|i| item(i as i64, Kind::PrFailing, Urgency::Red, &format!("item {i}"), "2026-01-02T23:55:00.000Z")).collect()
}

fn draw(n: usize, scroll: usize, height: u16) -> (Vec<String>, Drawn, Buffer) {
    let now = OffsetDateTime::parse("2026-01-03T00:00:00.000Z", &Rfc3339).unwrap();
    let items = items(n);
    let mut terminal = Terminal::new(TestBackend::new(W, height)).unwrap();
    let mut drawn = None;
    terminal.draw(|f| drawn = Some(column::draw(f, f.area(), &machine(), 0, &items, scroll, now, &Default::default()))).unwrap();
    let buf = terminal.backend().buffer().clone();
    ((0..height).map(|y| row(&buf, y)).collect(), drawn.unwrap(), buf)
}

fn header(count: usize) -> String {
    format!("───{CAP_L}Inbox{CAP_R}{}{CAP_L}{count}{CAP_R}─", "─".repeat(30 - 3 - 7 - 2 - count.to_string().len() - 1))
}

/// An item's row: the glyph at column 1, the label from column 3, `5m` ending at column 28.
fn item_row(n: usize) -> String {
    let label = format!("item {n}");
    format!(" {} {label}{}5m", palette::TIER_RED, " ".repeat(24 - label.len()))
}

fn cue(text: &str) -> String {
    format!("{}{text}", " ".repeat(29 - text.chars().count()))
}

fn machine_rows() -> [String; 3] {
    [String::new(), " homeserver".to_owned(), " \u{F4BC} 23%  \u{EFC5} 41%".to_owned()]
}

#[test]
fn the_inbox_is_pinned_to_the_bottom_and_ends_on_the_panes_last_row() {
    let (got, _, _) = draw(0, 0, 12);
    let mut want: Vec<String> = machine_rows().into();
    want.extend(vec![String::new(); 5]); // rows 3-7: the blank rows above the section (row 7 is its blank first row)
    want.extend([header(0), String::new(), " up to date, drink some water".to_owned(), String::new()]);
    assert_eq!(got, want, "no item");
    let (got, _, _) = draw(2, 0, 12);
    let mut want: Vec<String> = machine_rows().into();
    want.extend(vec![String::new(); 4]);
    want.extend([header(2), String::new(), item_row(1), item_row(2), String::new()]);
    assert_eq!(got, want, "two items");
}

#[test]
fn the_inbox_grows_upward_as_items_arrive() {
    for (n, header_row) in [(0, 8), (1, 8), (2, 7)] {
        let (got, _, _) = draw(n, 0, 12);
        assert_eq!(got[header_row], header(n), "{n} items: {got:#?}");
        assert_eq!(got[11], "", "{n} items: the last row stays blank");
    }
}

#[test]
fn an_overflowing_inbox_fills_the_rows_under_the_machine_lines() {
    for (scroll, above, first, below) in [(0, "", 1, "▼ 7 more"), (3, "▲ 3 more", 4, "▼ 4 more"), (10, "▲ 7 more", 8, "")] {
        let (got, _, _) = draw(12, scroll, 12);
        let mut want: Vec<String> = machine_rows().into();
        want.extend([String::new(), header(12)]);
        want.push(if above.is_empty() { String::new() } else { cue(above) });
        want.extend((first..first + 5).map(item_row));
        want.push(if below.is_empty() { String::new() } else { cue(below) });
        assert_eq!(got[..2 + 1 + 1], want[..4], "scroll {scroll}");
        assert_eq!(got[3], "", "scroll {scroll}: the section's blank first row");
        assert_eq!(got[4..], want[4..], "scroll {scroll}");
    }
}

#[test]
fn the_drawn_inbox_rect_is_the_bottom_aligned_section() {
    let (_, drawn, _) = draw(2, 0, 12);
    assert_eq!(drawn.inbox, Rect::new(0, 6, 30, 6));
    assert_eq!(drawn.inbox_shape.hit(1), Some(Hit::Header));
    let (_, drawn, _) = draw(12, 0, 12);
    assert_eq!(drawn.inbox, Rect::new(0, 3, 30, 9));
    assert_eq!(drawn.inbox_shape.hit(1), Some(Hit::Header));
}

#[test]
fn a_pane_too_short_for_the_section_cuts_it_from_the_bottom() {
    let (got, drawn, _) = draw(2, 0, 5);
    assert_eq!(got[3], "");
    assert_eq!(got[4], header(2));
    assert_eq!(drawn.inbox, Rect::new(0, 3, 30, 2));
}
