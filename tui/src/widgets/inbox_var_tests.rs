//! The variable-height Inbox item (pro-d177): an item with no Row 2 takes one row, one with a Row 2 takes two, with no blank
//! row inside the list; a capped list never splits an item; the Link covers Row 1 only; a click on either row hits the item;
//! the scroll offset counts items. Written against `column::draw` and its `Drawn` only.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Position;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::column::{self, Drawn, Machine};
use super::host::Usage;
use super::inbox::{Hit, item};
use crate::link::Links;
use crate::palette;
use crate::test_text::row;
use cpd_core::inbox::model::{Item, Kind, Urgency};

const CAP_L: &str = "\u{E0B6}";
const CAP_R: &str = "\u{E0B4}";
const W: u16 = 30;
const OPENED: &str = "2026-01-02T23:55:00.000Z";
const U1: &str = "https://github.com/acme/api/pull/3";
const U2: &str = "https://github.com/acme/api/pull/4";

fn machine() -> Machine {
    Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } }
}

/// Failing red items, one per label.
fn items(labels: &[&str]) -> Vec<Item> {
    labels.iter().enumerate().map(|(i, l)| item(i as i64 + 1, Kind::PrFailing, Urgency::Red, l, OPENED)).collect()
}

fn draw(items: &[Item], scroll: usize, height: u16) -> (Vec<String>, Drawn) {
    let now = OffsetDateTime::parse("2026-01-03T00:00:00.000Z", &Rfc3339).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(W, height)).unwrap();
    let mut drawn = None;
    terminal.draw(|f| drawn = Some(column::draw(f, f.area(), &machine(), 0, items, scroll, now, &Default::default()))).unwrap();
    let buf = terminal.backend().buffer().clone();
    ((0..height).map(|y| row(&buf, y)).collect(), drawn.unwrap())
}

fn blank() -> String {
    String::new()
}

fn header(count: usize) -> String {
    format!("───{CAP_L}Inbox{CAP_R}{}{CAP_L}{count}{CAP_R}─", "─".repeat(30 - 3 - 7 - 2 - count.to_string().len() - 1))
}

/// Row 1 of item `n`: the glyph at column 1, the label from column 3, `5m` ending at column 28.
fn r1(n: usize) -> String {
    let label = format!("item {n}");
    format!(" {} {label}{}5m", palette::TIER_RED, " ".repeat(24 - label.len()))
}

fn r2(n: usize) -> String {
    format!("   sub {n}")
}

fn cue(text: &str) -> String {
    format!("{}{text}", " ".repeat(29 - text.chars().count()))
}

/// The rows of `got` from `first` on.
fn from(got: &[String], first: usize) -> Vec<String> {
    got[first..].to_vec()
}

/// Labels of the heights 2, 1, 2.
const MIXED: [&str; 3] = ["item 1\nsub 1", "item 2", "item 3\nsub 3"];

#[test]
fn an_item_without_a_row_two_takes_one_row_and_one_with_a_row_two_takes_two() {
    let (got, drawn) = draw(&items(&MIXED), 0, 20);
    assert_eq!(drawn.inbox.height, 9);
    let want = vec![blank(), header(3), blank(), r1(1), r2(1), r1(2), r1(3), r2(3), blank()];
    assert_eq!(from(&got, 11), want);
}

#[test]
fn a_label_with_nothing_after_its_newline_is_a_one_row_item() {
    let (got, drawn) = draw(&items(&["item 1\n", "item 2"]), 0, 20);
    assert_eq!(drawn.inbox.height, 6);
    assert_eq!(from(&got, 14), vec![blank(), header(2), blank(), r1(1), r1(2), blank()]);
}

#[test]
fn a_list_of_one_row_items_is_one_row_each_and_every_row_is_its_hit() {
    let (got, drawn) = draw(&items(&["item 1", "item 2", "item 3"]), 0, 20);
    assert_eq!(drawn.inbox.height, 7);
    assert_eq!(from(&got, 13), vec![blank(), header(3), blank(), r1(1), r1(2), r1(3), blank()]);
    let shape = drawn.inbox_shape;
    assert_eq!([shape.hit(3), shape.hit(4), shape.hit(5)], [Some(Hit::Item(0)), Some(Hit::Item(1)), Some(Hit::Item(2))]);
    assert_eq!(shape.hit(6), None);
}

#[test]
fn both_rows_of_a_two_row_item_and_the_only_row_of_a_one_row_item_hit_that_item() {
    let (_, drawn) = draw(&items(&MIXED), 0, 20);
    let got: Vec<Option<Hit>> = (0..=8).map(|r| drawn.inbox_shape.hit(r)).collect();
    let (i0, i1, i2) = (Some(Hit::Item(0)), Some(Hit::Item(1)), Some(Hit::Item(2)));
    assert_eq!(got, vec![None, Some(Hit::Header), None, i0, i0, i1, i2, i2, None]);
}

#[test]
fn the_link_covers_row_one_of_each_item_and_never_row_two() {
    let mut list: Vec<Item> = ["acme/api#3 ready\nAdd retries", "acme/api#4 ready"]
        .iter()
        .enumerate()
        .map(|(i, l)| item(i as i64 + 1, Kind::PrReady, Urgency::Yellow, l, OPENED))
        .collect();
    list[0].url = Some(U1.to_owned());
    list[1].url = Some(U2.to_owned());
    let (_, drawn) = draw(&list, 0, 20);
    assert_eq!(drawn.inbox.y, 13);
    let links: &Links = &drawn.links;
    assert_eq!(
        [links.at(Position::new(3, 16)), links.at(Position::new(3, 17)), links.at(Position::new(3, 18))],
        [Some(U1), None, Some(U2)]
    );
}

#[test]
fn a_capped_list_leaves_a_spare_row_blank_rather_than_split_a_two_row_item() {
    let list = items(&["item 1", "item 2", "item 3\nsub 3", "item 4\nsub 4", "item 5\nsub 5"]);
    let (got, _) = draw(&list, 0, 12);
    let want = vec![blank(), header(5), blank(), r1(1), r1(2), r1(3), r2(3), blank(), cue("▼ 2 more")];
    assert_eq!(from(&got, 3), want);
}

#[test]
fn a_one_row_item_fills_the_last_list_row_a_two_row_item_would_not() {
    let list = items(&["item 1\nsub 1", "item 2\nsub 2", "item 3", "item 4"]);
    let (got, _) = draw(&list, 0, 12);
    assert_eq!(from(&got, 6), vec![r1(1), r2(1), r1(2), r2(2), r1(3), cue("▼ 1 more")]);
}

#[test]
fn the_scroll_offset_counts_items_and_the_last_offset_is_the_first_from_which_all_fit() {
    let list = items(&["item 1", "item 2\nsub 2", "item 3\nsub 3", "item 4", "item 5", "item 6"]);
    let (got, drawn) = draw(&list, 0, 12);
    assert_eq!(from(&got, 5), vec![blank(), r1(1), r1(2), r2(2), r1(3), r2(3), cue("▼ 3 more")]);
    assert_eq!((drawn.inbox_shape.max_scroll(), drawn.inbox_shape.wheeled(true)), (2, 2));
    let (got, _) = draw(&list, 1, 12);
    assert_eq!(from(&got, 5), vec![cue("▲ 1 more"), r1(2), r2(2), r1(3), r2(3), r1(4), cue("▼ 2 more")]);
    let (got, _) = draw(&list, 99, 12);
    assert_eq!(from(&got, 5), vec![cue("▲ 2 more"), r1(3), r2(3), r1(4), r1(5), r1(6), blank()]);
}

#[test]
fn one_list_row_shows_a_one_row_item_and_never_a_two_row_one() {
    let (got, drawn) = draw(&items(&["item 1", "item 2\nsub 2"]), 0, 8);
    assert_eq!(from(&got, 6), vec![r1(1), cue("▼ 1 more")]);
    assert_eq!(drawn.inbox_shape.hit(3), Some(Hit::Item(0)));
    let (got, drawn) = draw(&items(&["item 1\nsub 1", "item 2"]), 0, 8);
    assert_eq!(from(&got, 6), vec![blank(), cue("▼ 2 more")]);
    assert_eq!(drawn.inbox_shape.hit(3), None);
}
