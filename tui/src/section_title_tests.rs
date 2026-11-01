//! Red tests of `SectionTitle`: three rows (blank, the title row, blank), the title row `section::title`'s own.

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::box_widget::SectionTitle;
use crate::section;

const CASES: [(&str, Option<usize>); 2] = [("Workflows", Some(3)), ("Logs", None)];
const WIDTHS: [u16; 2] = [20, 30];

/// A buffer of `x` cells everywhere, so that a cell a widget leaves alone shows.
fn filled(width: u16, height: u16) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    for cell in &mut buf.content {
        cell.set_symbol("x");
    }
    buf
}

fn draw(title: &str, count: Option<usize>, area: Rect, buf: &mut Buffer) {
    SectionTitle::new(title, count).render(area, buf);
}

/// The cells of `section::title(title, count, width)`, as `Line` renders them into an empty buffer.
fn expected(title: &str, count: Option<usize>, width: u16) -> Vec<Cell> {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
    ratatui::widgets::Paragraph::new(section::title(title, count, usize::from(width)))
        .render(buf.area, &mut buf);
    (0..width).map(|x| buf[(x, 0)].clone()).collect()
}

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

#[test]
fn section_title_is_three_rows_a_blank_one_the_rule_and_a_blank_one() {
    assert_eq!(SectionTitle::ROWS, 3);
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 5));
    draw("Workflows", Some(3), Rect::new(0, 1, 20, 3), &mut buf);
    assert_eq!(row(&buf, 0).trim(), "", "above the area");
    assert_eq!(row(&buf, 1).trim(), "", "row 0 is blank");
    assert!(row(&buf, 2).contains("Workflows"), "row 1 is the title: {:?}", row(&buf, 2));
    assert_eq!(row(&buf, 3).trim(), "", "row 2 is blank");
    assert_eq!(row(&buf, 4).trim(), "", "under the area");
}

#[test]
fn section_title_draws_the_rule_with_the_pill_and_the_count_as_section_title_does() {
    for (title, count) in CASES {
        for width in WIDTHS {
            let mut buf = Buffer::empty(Rect::new(0, 0, width, 3));
            draw(title, count, buf.area, &mut buf);
            let want = expected(title, count, width);
            for (x, want) in (0..width).zip(&want) {
                let got = &buf[(x, 1)];
                assert_eq!(
                    (got.symbol(), got.style()),
                    (want.symbol(), want.style()),
                    "{title:?} {count:?} at width {width}, column {x}: {:?}",
                    row(&buf, 1)
                );
            }
        }
    }
}

#[test]
fn section_title_writes_nothing_on_its_two_blank_rows() {
    for (title, count) in CASES {
        let mut buf = filled(20, 3);
        draw(title, count, buf.area, &mut buf);
        for y in [0, 2] {
            assert_eq!(
                row(&buf, y),
                "x".repeat(20),
                "{title:?}: row {y} was written"
            );
        }
        assert_ne!(
            row(&buf, 1),
            "x".repeat(20),
            "{title:?}: the title row was not drawn"
        );
    }
}

#[test]
fn section_title_body_is_the_area_under_its_three_rows() {
    assert_eq!(
        SectionTitle::body(Rect::new(2, 4, 30, 10)),
        Rect::new(2, 7, 30, 7)
    );
    assert_eq!(
        SectionTitle::body(Rect::new(2, 4, 30, 3)).height,
        0,
        "exactly the three rows: no body"
    );
    assert_eq!(
        SectionTitle::body(Rect::new(2, 4, 30, 2)).height,
        0,
        "shorter than the three rows: no body"
    );
    assert_eq!(SectionTitle::body(Rect::new(2, 4, 30, 0)).height, 0);
    // the widget draws the three rows above the body and leaves the body alone
    let area = Rect::new(2, 4, 30, 10);
    let mut buf = filled(40, 16);
    draw("Workflows", Some(3), area, &mut buf);
    let body = SectionTitle::body(area);
    assert_ne!(
        row(&buf, body.y - 2),
        "x".repeat(40),
        "the title row is the row above the body's first blank row"
    );
    for y in body.y..body.bottom() {
        assert_eq!(
            buf.content[usize::from(y) * 40 + 2..usize::from(y) * 40 + 32]
                .iter()
                .map(Cell::symbol)
                .collect::<String>(),
            "x".repeat(30),
            "body row {y} was written"
        );
    }
}

#[test]
fn section_title_never_writes_outside_its_area() {
    let area = Rect::new(3, 2, 14, 3);
    for (title, count) in CASES {
        let mut buf = filled(20, 7);
        draw(title, count, area, &mut buf);
        let outside = (0..7)
            .flat_map(|y| (0..20).map(move |x| (x, y)))
            .filter(|&(x, y)| !area.contains((x, y).into()));
        for (x, y) in outside {
            assert_eq!(
                buf[(x, y)].symbol(),
                "x",
                "{title:?}: cell ({x}, {y}) outside {area:?} was written"
            );
        }
        assert_ne!(
            row(&buf, 3),
            "x".repeat(20),
            "{title:?}: nothing drawn at all"
        );
    }
}
