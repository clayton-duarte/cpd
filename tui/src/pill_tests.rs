//! The red tests of the pill atom (docs/research/pill-spec.md).
use crate::palette::{self, numbered::*, Meaning};
use crate::pill::{cap_style, pill};
use cpd_core::pill::spec;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Widget;

fn n(index: u8) -> Color {
    [C1, C1, C2, C3, C4, C5, C6, C7, C8, C9, C10, C11, C12, C13, C14, C15][index as usize]
}

fn fg(style: Style) -> Color {
    style.fg.expect("a block has a foreground")
}

#[test]
fn pill_atom_is_the_left_cap_the_text_and_the_right_cap() {
    let spans = pill("done", palette::pill_static());
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0].content, spec().caps.left);
    assert_eq!(spans[1].content, "done");
    assert_eq!(spans[2].content, spec().caps.right);
}

#[test]
fn pill_atom_caps_are_the_spec_glyphs_for_any_text() {
    for text in ["two words", "x", "wait…"] {
        let spans = pill(text, palette::pill_interactive());
        assert_eq!(spans[0].content, spec().caps.left, "{text}");
        assert_eq!(spans[2].content, spec().caps.right, "{text}");
        assert_eq!(spans[0].content, "\u{e0b6}");
        assert_eq!(spans[2].content, "\u{e0b4}");
    }
}

#[test]
fn pill_atom_caps_take_the_block_colour_as_foreground_and_are_not_reversed() {
    let block = palette::pill_focus();
    let spans = pill("x", block);
    for cap in [&spans[0], &spans[2]] {
        assert_eq!(cap.style.fg, block.fg);
        assert_eq!(cap.style.bg, None);
        assert!(!cap.style.add_modifier.contains(Modifier::REVERSED));
    }
}

#[test]
fn pill_atom_text_carries_the_block_style_unchanged() {
    let block = palette::pill_interactive();
    assert_eq!(pill("x", block)[1].style, block);
}

#[test]
fn pill_atom_painted_in_a_buffer_is_inverted_text_between_plain_caps() {
    let block = palette::pill_static();
    let area = Rect::new(0, 0, 6, 1);
    let mut buf = Buffer::empty(area);
    Line::from(pill("abcd", block)).render(area, &mut buf);
    for x in [0, 5] {
        let cell = &buf[(x, 0)];
        assert_eq!(cell.fg, fg(block));
        assert_eq!(cell.bg, Cell::EMPTY.bg);
        assert!(!cell.modifier.contains(Modifier::REVERSED));
    }
    for x in 1..5 {
        let cell = &buf[(x, 0)];
        assert_eq!(cell.fg, fg(block));
        assert!(cell.modifier.contains(Modifier::REVERSED));
    }
}

#[test]
fn pill_cap_style_is_the_block_foreground_and_nothing_else() {
    let block = palette::pill_interactive();
    assert_eq!(cap_style(block), Style::new().fg(fg(block)));
}

#[test]
fn pill_atom_follows_the_spec_for_every_meaning() {
    let s = spec();
    let m = &s.meanings;
    let all = [
        (m.plain, Meaning::NotStarted),
        (m.in_progress, Meaning::InProgress),
        (m.review, Meaning::Review),
        (m.done, Meaning::Done),
        (m.failed, Meaning::Failed),
        (m.planning, Meaning::Planning),
        (m.nothing, Meaning::Nothing),
        (m.in_progress, Meaning::Notice),
    ];
    for (index, meaning) in all {
        assert_eq!(fg(palette::pill(meaning)), n(index), "{meaning:?}");
    }
}
