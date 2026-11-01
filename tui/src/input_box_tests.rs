use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::input_box::{boxed, text_rows, text_width};
use crate::palette::{self, numbered};

const L: &str = "\u{E0B6}";
const R: &str = "\u{E0B4}";

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| row(buf, y)).collect()
}

fn dashes(n: usize) -> String {
    "─".repeat(n)
}

/// A text row of a box `w` wide: wall, padding, the text padded to `w - 4`, padding, wall.
fn body(text: &str, w: usize) -> String {
    format!("│ {text:<tw$} │", tw = w - 4)
}

fn top(label: &str, w: usize) -> String {
    if label.is_empty() {
        format!("╭{}╮", dashes(w - 2))
    } else {
        format!("╭─{L}{label}{R}{}╮", dashes(w - label.chars().count() - 5))
    }
}

fn bottom(w: usize) -> String {
    format!("╰{}╯", dashes(w - 2))
}

/// The box of `w` columns, as many rows as the text needs.
fn drawn(label: &str, text: &str, cursor: usize, focused: bool, w: usize) -> Buffer {
    let text = chars(text);
    let area = Rect::new(0, 0, w as u16, (text_rows(text.len(), w) + 2) as u16);
    let mut buf = Buffer::empty(area);
    boxed(label, &text, cursor, focused, 0, w).render(area, &mut buf);
    buf
}

#[test]
fn input_box_draws_the_label_the_borders_and_the_text_inside_the_walls() {
    let buf = drawn("Title", "abc", 3, true, 42);
    assert_eq!(rows(&buf), [top("Title", 42), body("abc ", 42), bottom(42)]);
    assert_eq!(row(&buf, 0).chars().count(), 42);
}

#[test]
fn input_box_without_a_label_has_a_plain_top_border() {
    let buf = drawn("", "abcdefghijklmn", 14, true, 18);
    assert_eq!(
        rows(&buf),
        [
            top("", 18),
            body("abcdefghijklmn", 18),
            body(" ", 18),
            bottom(18)
        ]
    );
}

#[test]
fn input_box_wraps_at_the_text_width_and_grows_down() {
    let buf = drawn("Name", "abcdefghijklmnopqrst", 20, true, 18);
    assert_eq!(
        rows(&buf),
        [
            top("Name", 18),
            body("abcdefghijklmn", 18),
            body("opqrst ", 18),
            bottom(18)
        ]
    );
    let buf = drawn("", "abcdefghijklmn", 14, true, 18);
    assert_eq!(buf.area.height, 4);
    let buf = drawn("Name", "abcdefghijklm", 13, true, 18);
    assert_eq!(
        rows(&buf),
        [top("Name", 18), body("abcdefghijklm ", 18), bottom(18)]
    );
    let text = "abcdefghijklmnopqrstuvwxyz01";
    let buf = drawn("Name", text, 28, true, 18);
    assert_eq!(
        rows(&buf),
        [
            top("Name", 18),
            body("abcdefghijklmn", 18),
            body("opqrstuvwxyz01", 18),
            body(" ", 18),
            bottom(18)
        ]
    );
}

#[test]
fn input_box_text_rows_is_len_over_text_width_plus_one() {
    for ((len, width), rows) in [
        ((0, 42), 1),
        ((37, 42), 1),
        ((38, 42), 2),
        ((76, 42), 3),
        ((200, 58), 4),
        ((5, 4), 6),
        ((5, 0), 6),
    ] {
        assert_eq!(text_rows(len, width), rows, "{len} characters at {width}");
    }
    assert_eq!(text_width(42), 38);
}

#[test]
fn input_box_cursor_is_the_blue_block_and_a_space_past_the_end() {
    let buf = drawn("Title", "abc", 3, true, 42);
    for (x, want) in [(2, "a"), (3, "b"), (4, "c"), (5, " ")] {
        assert_eq!(buf[(x, 1)].symbol(), want);
        assert_eq!(
            palette::check::is_selected(&buf[(x, 1)]),
            x == 5,
            "column {x}"
        );
    }
    let buf = drawn("Title", "abc", 1, true, 42);
    let selected: Vec<u16> = (0..42)
        .filter(|&x| palette::check::is_selected(&buf[(x, 1)]))
        .collect();
    assert_eq!(selected, [3]);
    assert_eq!(buf[(3, 1)].symbol(), "b");
}

#[test]
fn the_label_pill_is_the_border_colour_blue_12_focused_and_white_15_unfocused() {
    for (focused, border) in [(false, numbered::C15), (true, numbered::C12)] {
        let buf = drawn("Title", "abc", 3, focused, 42);
        for x in [0, 1] {
            assert_eq!(buf[(x, 0)].fg, border, "top column {x}");
        }
        for x in 9..41 {
            assert_eq!(
                (buf[(x, 0)].symbol(), buf[(x, 0)].fg),
                ("─", border),
                "fill {x}"
            );
        }
        assert_eq!(buf[(41, 0)].symbol(), "╮");
        assert_eq!(buf[(41, 0)].fg, border);
        for y in [1, 2] {
            assert_eq!(buf[(0, y)].fg, border, "left wall {y}");
            assert_eq!(buf[(41, y)].fg, border, "right wall {y}");
        }
        for x in 0..42 {
            assert_eq!(buf[(x, 2)].fg, border, "bottom {x}");
        }
        // hs-d578 F9: the label pill follows the border: text 0 on the border colour, caps in it on the terminal's background
        let reset = ratatui::buffer::Cell::EMPTY.bg;
        assert_eq!(
            (buf[(2, 0)].symbol(), buf[(2, 0)].fg, buf[(2, 0)].bg),
            (L, border, reset)
        );
        for x in 3..8 {
            let cell = &buf[(x, 0)];
            assert_eq!((cell.fg, cell.bg), (border, reset), "pill {x}");
            assert!(
                cell.modifier.contains(ratatui::style::Modifier::REVERSED),
                "pill {x} is reversed: {cell:?}"
            );
        }
        assert_eq!(
            (buf[(8, 0)].symbol(), buf[(8, 0)].fg, buf[(8, 0)].bg),
            (R, border, reset)
        );
        for x in [2, 3, 4] {
            assert_eq!(buf[(x, 1)].fg, numbered::C15, "text {x}");
        }
    }
}

#[test]
fn input_box_cuts_a_label_wider_than_the_room() {
    let long = "x".repeat(60);
    let buf = drawn(&long, "", 0, true, 42);
    assert_eq!(row(&buf, 0), format!("╭─{L}{}{R}╮", "x".repeat(37)));
    assert_eq!(row(&buf, 0).chars().count(), 42);
    let buf = drawn("Title", "", 0, true, 5);
    assert_eq!(row(&buf, 0), "╭───╮");
    let buf = drawn("Title", "", 0, true, 6);
    assert_eq!(row(&buf, 0), format!("╭─{L}T{R}╮"));
}

#[test]
fn input_box_draws_in_a_tiny_width_without_panicking() {
    let long: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
    for width in 0..=8 {
        for label in ["", "Title"] {
            for text in [&[][..], &long[..]] {
                for cursor in [0, text.len()] {
                    for focused in [false, true] {
                        let area = Rect::new(0, 0, width as u16, 6);
                        let mut buf = Buffer::empty(area);
                        boxed(label, text, cursor, focused, 0, width).render(area, &mut buf);
                        if width < 5 {
                            assert!(
                                rows(&buf).iter().all(|r| r.trim().is_empty()),
                                "width {width}: no half box"
                            );
                        }
                    }
                }
            }
        }
    }
}
