//! The input box (contract §6.1.1) by its rows and cell colours.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};

use crate::input_box::{lines, text_rows};
use crate::palette::{self, numbered};

const L: &str = "\u{E0B6}";
const R: &str = "\u{E0B4}";

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn buffer(lines: &[Line<'static>], width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, lines.len() as u16);
    let mut buf = Buffer::empty(area);
    Paragraph::new(lines.to_vec()).render(area, &mut buf);
    buf
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

/// A text row of a box `w` wide: margin, wall, padding, the text padded to `w - 6`, padding, wall, margin.
fn body(text: &str, w: usize) -> String {
    format!(" │ {text:<tw$} │ ", tw = w - 6)
}

fn top(label: &str, w: usize) -> String {
    if label.is_empty() {
        format!(" ╭{}╮ ", dashes(w - 4))
    } else {
        format!(" ╭─{L}{label}{R}{}╮ ", dashes(w - label.chars().count() - 7))
    }
}

fn bottom(w: usize) -> String {
    format!(" ╰{}╯ ", dashes(w - 4))
}

fn drawn(label: &str, text: &str, cursor: usize, focused: bool, w: usize) -> Buffer {
    buffer(&lines(label, &chars(text), cursor, focused, w), w as u16)
}

#[test]
fn input_box_draws_the_label_the_borders_and_the_text_inside_the_margins() {
    let buf = drawn("Title", "abc", 3, true, 44);
    assert_eq!(rows(&buf), [top("Title", 44), body("abc ", 44), bottom(44)]);
    assert_eq!(row(&buf, 0).chars().count(), 44);
}

#[test]
fn input_box_without_a_label_has_a_plain_top_border() {
    let buf = drawn("", "abcdefghijklmn", 14, true, 20);
    assert_eq!(rows(&buf), [top("", 20), body("abcdefghijklmn", 20), body(" ", 20), bottom(20)]);
}

#[test]
fn input_box_wraps_at_the_text_width_and_grows_down() {
    let buf = drawn("Name", "abcdefghijklmnopqrst", 20, true, 20);
    assert_eq!(rows(&buf), [top("Name", 20), body("abcdefghijklmn", 20), body("opqrst ", 20), bottom(20)]);
    let buf = drawn("", "abcdefghijklmn", 14, true, 20);
    assert_eq!(buf.area.height, 4);
    let buf = drawn("Name", "abcdefghijklm", 13, true, 20);
    assert_eq!(rows(&buf), [top("Name", 20), body("abcdefghijklm ", 20), bottom(20)]);
    let text = "abcdefghijklmnopqrstuvwxyz01";
    let buf = drawn("Name", text, 28, true, 20);
    assert_eq!(
        rows(&buf),
        [top("Name", 20), body("abcdefghijklmn", 20), body("opqrstuvwxyz01", 20), body(" ", 20), bottom(20)]
    );
}

#[test]
fn input_box_text_rows_is_len_over_text_width_plus_one() {
    for ((len, width), rows) in [((0, 44), 1), ((37, 44), 1), ((38, 44), 2), ((76, 44), 3), ((200, 60), 4), ((5, 6), 6), ((5, 0), 6)] {
        assert_eq!(text_rows(len, width), rows, "{len} characters at {width}");
    }
}

#[test]
fn input_box_cursor_is_the_blue_block_and_a_space_past_the_end() {
    let buf = drawn("Title", "abc", 3, true, 44);
    for (x, want) in [(3, "a"), (4, "b"), (5, "c"), (6, " ")] {
        assert_eq!(buf[(x, 1)].symbol(), want);
        assert_eq!(palette::check::is_selected(&buf[(x, 1)]), x == 6, "column {x}");
    }
    let buf = drawn("Title", "abc", 1, true, 44);
    let selected: Vec<u16> = (0..44).filter(|&x| palette::check::is_selected(&buf[(x, 1)])).collect();
    assert_eq!(selected, [4]);
    assert_eq!(buf[(4, 1)].symbol(), "b");
}

#[test]
fn input_box_border_is_15_unfocused_and_12_focused_and_the_label_is_a_grey_pill() {
    for (focused, border) in [(false, numbered::C15), (true, numbered::C12)] {
        let buf = drawn("Title", "abc", 3, focused, 44);
        for x in [1, 2] {
            assert_eq!(buf[(x, 0)].fg, border, "top column {x}");
        }
        for x in 10..42 {
            assert_eq!((buf[(x, 0)].symbol(), buf[(x, 0)].fg), ("─", border), "fill {x}");
        }
        assert_eq!((buf[(43, 0)].symbol(), buf[(42, 0)].symbol()), (" ", "╮"));
        assert_eq!(buf[(42, 0)].fg, border);
        for y in [1, 2] {
            assert_eq!(buf[(1, y)].fg, border, "left wall {y}");
            assert_eq!(buf[(42, y)].fg, border, "right wall {y}");
        }
        for x in 1..43 {
            assert_eq!(buf[(x, 2)].fg, border, "bottom {x}");
        }
        assert_eq!((buf[(3, 0)].symbol(), buf[(3, 0)].fg), (L, numbered::C7));
        for x in 4..9 {
            assert_eq!((buf[(x, 0)].bg, buf[(x, 0)].fg), (numbered::C7, numbered::C0), "pill {x}");
        }
        assert_eq!((buf[(9, 0)].symbol(), buf[(9, 0)].fg), (R, numbered::C7));
        for x in [3, 4, 5] {
            assert_eq!(buf[(x, 1)].fg, numbered::C15, "text {x}");
        }
    }
}

#[test]
fn input_box_cuts_a_label_wider_than_the_room() {
    let long = "x".repeat(60);
    let buf = drawn(&long, "", 0, true, 44);
    assert_eq!(row(&buf, 0), format!(" ╭─{L}{}{R}╮ ", "x".repeat(37)));
    assert_eq!(row(&buf, 0).chars().count(), 44);
    let buf = drawn("Title", "", 0, true, 7);
    assert_eq!(row(&buf, 0), " ╭───╮ ");
    let buf = drawn("Title", "", 0, true, 8);
    assert_eq!(row(&buf, 0), format!(" ╭─{L}T{R}╮ "));
}

#[test]
fn input_box_draws_in_a_tiny_width_without_panicking() {
    let long: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
    for width in 0..=8 {
        for label in ["", "Title"] {
            for text in [&[][..], &long[..]] {
                for cursor in [0, text.len()] {
                    for focused in [false, true] {
                        let out = lines(label, text, cursor, focused, width);
                        assert!(out.iter().all(|l| l.width() <= width), "width {width}: {out:?}");
                    }
                }
            }
        }
    }
}
