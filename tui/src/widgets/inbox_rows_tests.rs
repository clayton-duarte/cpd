//! The two-row Inbox item (pro-d171): each item is Row 1 (glyph, label, Link, age) and Row 2 (the muted subtitle, from the
//! label's second line); both rows hit the item, the Link covers Row 1 only, and the list holds half as many items as rows.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::column::{self, Machine};
use super::host::Usage;
use super::inbox::{self, Hit, Shape, item};
use crate::link::Links;
use crate::palette;
use crate::test_text::row;
use cpd_core::inbox::model::{Item, Kind, Urgency};

const OPENED: &str = "2026-01-02T23:55:00.000Z";
const URL: &str = "https://github.com/acme/api/pull/3";

fn now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-01-03T00:00:00.000Z", &Rfc3339).unwrap()
}

fn ready(id: i64, label: &str) -> Item {
    item(id, Kind::PrReady, Urgency::Yellow, label, OPENED)
}

fn drawn(items: &[Item], shape: Shape, width: u16, height: u16) -> (Buffer, Links) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut links = Links::default();
    terminal.draw(|f| links = inbox::draw(f, Rect::new(0, 0, width, height), items, shape, now(), &|_| None)).unwrap();
    (terminal.backend().buffer().clone(), links)
}

fn flat(items: &[Item]) -> (Buffer, Links) {
    let shape = Shape::new(items.len());
    drawn(items, shape, 40, shape.height())
}

fn cells(buf: &Buffer, from: u16, to: u16, y: u16) -> String {
    (from..to).map(|x| buf[(x, y)].symbol()).collect()
}

#[test]
fn an_item_is_two_rows_and_an_empty_list_stays_one_row() {
    assert_eq!(Shape::new(3).height(), 10);
    assert_eq!(Shape::new(0).height(), 5);
    assert_eq!(Shape::new(3).capped(7).height(), 7);
}

#[test]
fn both_rows_of_an_item_are_hits_for_that_item() {
    let shape = Shape::new(3);
    assert_eq!(shape.hit(1), Some(Hit::Header));
    for blank in [0, 2, 9] {
        assert_eq!(shape.hit(blank), None, "row {blank}");
    }
    for (row, i) in [(3, 0), (4, 0), (5, 1), (6, 1), (7, 2), (8, 2)] {
        assert_eq!(shape.hit(row), Some(Hit::Item(i)), "row {row}");
    }
}

#[test]
fn row_two_is_the_muted_subtitle_indented_under_the_label() {
    let (buf, _) = flat(&[ready(1, "acme/api#3 ready\nAdd retries")]);
    let one = row(&buf, 3);
    assert!(one.starts_with(" ") && one.contains(" acme/api#3 ready") && one.ends_with("5m"), "{one:?}");
    assert!(!one.contains("Add retries"), "{one:?}");
    assert_eq!(row(&buf, 4), "   Add retries");
    assert_eq!(buf[(3, 4)].fg, palette::muted_text().fg.unwrap());
}

#[test]
fn a_long_subtitle_is_cut_with_an_ellipsis_before_the_right_margin() {
    let (buf, _) = flat(&[ready(1, &format!("acme/api#3 ready\n{}", "s".repeat(60)))]);
    let cut = cells(&buf, 3, 39, 4);
    assert_eq!(cut.chars().count(), 36);
    assert_eq!(cut, format!("{}…", "s".repeat(35)));
    assert_eq!(buf[(39, 4)].symbol(), " ");
}

#[test]
fn an_item_with_no_subtitle_has_a_blank_second_row() {
    for label in ["acme/api#3 ready", "acme/api#3 ready\n"] {
        let (buf, _) = flat(&[ready(1, label), ready(2, "acme/api#4 ready\nSub")]);
        assert!(row(&buf, 3).contains("acme/api#3 ready"), "{label:?}");
        assert_eq!(row(&buf, 4), "", "{label:?}");
        assert!(row(&buf, 5).contains("acme/api#4 ready"), "the next item starts under the blank row");
    }
}

#[test]
fn the_link_covers_row_one_only() {
    let mut it = ready(1, "acme/api#3 ready\nAdd retries");
    it.url = Some(URL.to_owned());
    let (_, links) = flat(&[it]);
    assert_eq!(links.at(Position::new(3, 3)), Some(URL));
    assert_eq!(links.at(Position::new(3, 4)), None);
    let linked = (0..40).filter(|x| links.at(Position::new(*x, 3)).is_some()).count();
    assert!(linked <= "acme/api#3 ready".chars().count() + 2, "the Link is Row 1's label (and its cue), not the whole label: {linked} cells");
}

#[test]
fn the_list_holds_half_as_many_items_as_it_has_rows_and_an_odd_row_stays_blank() {
    let items: Vec<Item> = (1..=10).map(|i| ready(i, &format!("item {i}\nsub {i}"))).collect();
    let ten = Shape::new(10).capped(10);
    assert_eq!((ten.max_scroll(), ten.wheeled(true)), (7, 3));
    let (buf, _) = drawn(&items, ten, 40, 10);
    assert!(row(&buf, 7).contains("item 3") && row(&buf, 8).contains("sub 3") && row(&buf, 9).contains("▼ 7 more"), "3 items shown");

    let (buf, _) = drawn(&items, Shape::new(10).capped(9), 40, 9);
    assert!(row(&buf, 5).contains("item 2") && row(&buf, 6).contains("sub 2"), "2 items shown");
    assert_eq!(row(&buf, 7), "");
    assert!(row(&buf, 8).ends_with("▼ 8 more"), "{:?}", row(&buf, 8));
}

#[test]
fn the_column_grows_two_rows_for_each_item() {
    let items: Vec<Item> = (1..=2).map(|i| ready(i, &format!("item {i}\nsub {i}"))).collect();
    let machine = Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } };
    let mut terminal = Terminal::new(TestBackend::new(30, 20)).unwrap();
    let mut got = None;
    terminal.draw(|f| got = Some(column::draw(f, f.area(), &machine, 0, &items, 0, now(), &Default::default()))).unwrap();
    let drawn = got.unwrap();
    let buf = terminal.backend().buffer().clone();
    assert_eq!(drawn.inbox.height, 8);
    assert_eq!(drawn.inbox.y + drawn.inbox.height, 20, "pinned to the pane's bottom");
    assert_eq!(row(&buf, 18), "   sub 2", "Row 2 of the last item is above the ▼ line");
}
