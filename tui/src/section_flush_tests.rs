//! The flush title (`section::flush_title`, commands.md §13.2): `───`, the title pill, a `─` fill, the count pill and one `─`,
//! with no padding cell on either side. The expected rows are spelled out; none is built by calling the function.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Widget;

use crate::palette;
use crate::section::{check, flush_title};

const CAP_L: &str = "\u{E0B6}";
const CAP_R: &str = "\u{E0B4}";

fn pill(text: &str) -> String {
    format!("{CAP_L}{text}{CAP_R}")
}

fn dashes(n: usize) -> String {
    "─".repeat(n)
}

fn text(line: &Line<'static>) -> String {
    line.spans.iter().map(|s| s.content.to_string()).collect()
}

fn drawn(title: &str, count: Option<usize>, width: u16) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
    flush_title(title, count, usize::from(width)).render(Rect::new(0, 0, width, 1), &mut buf);
    buf
}

#[test]
fn the_flush_title_is_three_lines_the_pill_a_fill_the_count_pill_and_one_line() {
    let line = flush_title("Inbox", Some(2), 30);
    assert_eq!(
        text(&line),
        format!("───{}{}{}─", pill("Inbox"), dashes(16), pill("2"))
    );
    assert_eq!(line.width(), 30);
}

#[test]
fn the_flush_title_has_no_padding_cell_on_either_side() {
    let muted = palette::muted_text().fg.unwrap();
    for width in [22u16, 30] {
        let buf = drawn("Inbox", Some(2), width);
        for x in [0, 1, 2, width - 1] {
            assert_eq!(
                (buf[(x, 0)].symbol(), buf[(x, 0)].fg),
                ("─", muted),
                "width {width}, cell {x}"
            );
        }
        assert_eq!(
            buf[(width - 2, 0)].symbol(),
            CAP_R,
            "width {width}: the count pill's right cap"
        );
    }
}

#[test]
fn the_flush_title_without_a_count_fills_to_the_last_line() {
    assert_eq!(
        text(&flush_title("Inbox", None, 20)),
        format!("───{}{}", pill("Inbox"), dashes(10))
    );
}

#[test]
fn a_narrow_flush_title_drops_the_count_then_cuts_the_title_then_the_pill() {
    let cases = [
        (15, format!("───{}─{}─", pill("Inbox"), pill("3"))),
        (14, format!("───{}{}", pill("Inbox"), dashes(4))),
        (9, format!("───{}─", pill("Inb"))),
        (7, format!("───{}─", pill("I"))),
        (6, dashes(6)),
        (4, dashes(4)),
        (1, dashes(1)),
        (0, String::new()),
    ];
    for (width, want) in cases {
        let line = flush_title("Inbox", Some(3), width);
        assert_eq!(text(&line), want, "width {width}");
        assert_eq!(line.width(), width, "width {width}: exactly the width");
    }
}

#[test]
fn a_flush_titles_pills_are_grey_and_its_lines_are_muted() {
    let buf = drawn("Inbox", Some(35), 22);
    check::grey_pill(&buf, 3, 0, "Inbox");
    check::grey_pill(&buf, 17, 0, "35");
    let muted = palette::muted_text().fg.unwrap();
    for x in (0..3).chain(10..17).chain(21..22) {
        assert_eq!(
            (buf[(x, 0)].symbol(), buf[(x, 0)].fg),
            ("─", muted),
            "cell {x}"
        );
    }
}
