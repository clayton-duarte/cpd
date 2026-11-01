//! Parity of the two Rust renderers of the pill: for every meaning of the spec (base and clickable) and for three
//! texts, the tuple decoded from the ratatui atom equals the one decoded from the tmux adapter. The pi tuple is pinned by
//! `pi/test/pill-spec.test.ts`. No tmux process: the tmux string is decoded by a pure parser.

use cpd_core::pill::spec;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Widget;

/// (left cap, right cap, colour index, text reversed, caps reversed, text).
type Tuple = (String, String, u8, bool, bool, String);

const COLOURS: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

fn index_of(c: Color) -> u8 {
    match c {
        Color::Indexed(i) => i,
        c => COLOURS
            .iter()
            .position(|k| *k == c)
            .unwrap_or_else(|| panic!("a colour outside the palette: {c:?}")) as u8,
    }
}

/// The ratatui tuple: the atom painted into a `Buffer`, one cell per character.
fn from_ratatui(index: u8, text: &str) -> Tuple {
    let block = Style::new()
        .fg(COLOURS[index as usize])
        .add_modifier(Modifier::REVERSED);
    let line = Line::from(cpd_tui::pill::pill(text, block));
    let width = text.chars().count() as u16 + 2;
    let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
    line.render(Rect::new(0, 0, width, 1), &mut buf);
    let cells: Vec<_> = (0..width).map(|x| buf[(x, 0)].clone()).collect();
    let (first, last) = (&cells[0], &cells[cells.len() - 1]);
    let mid = &cells[1..cells.len() - 1];
    let reversed = |c: &ratatui::buffer::Cell| c.modifier.contains(Modifier::REVERSED);
    assert!(
        cells.iter().all(|c| index_of(c.fg) == index),
        "one colour on every cell"
    );
    (
        first.symbol().to_string(),
        last.symbol().to_string(),
        index_of(first.fg),
        mid.iter().all(reversed),
        reversed(first) || reversed(last),
        mid.iter().map(|c| c.symbol()).collect(),
    )
}

/// A pure parser of `#[fg=colourN]`, `#[reverse]`, `#[noreverse]`, `#[default]`: (char, fg, reverse) per cell.
fn decode(text: &str) -> Vec<(char, Option<u8>, bool)> {
    let (mut fg, mut reverse) = (None, false);
    let (mut out, mut rest) = (Vec::new(), text);
    while let Some(ch) = rest.chars().next() {
        if let Some(inner) = rest.strip_prefix("#[") {
            let end = inner
                .find(']')
                .unwrap_or_else(|| panic!("an unclosed style in {text}"));
            for token in inner[..end].split(',') {
                match token.split_once('=') {
                    Some(("fg", c)) => {
                        fg = Some(
                            c.strip_prefix("colour")
                                .unwrap_or_else(|| panic!("fg={c}"))
                                .parse()
                                .unwrap(),
                        )
                    }
                    _ if token == "default" => (fg, reverse) = (None, false),
                    _ if token == "reverse" => reverse = true,
                    _ if token == "noreverse" => reverse = false,
                    _ => {}
                }
            }
            rest = &inner[end + 1..];
        } else {
            out.push((ch, fg, reverse));
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

/// The tmux tuple: the adapter's string, decoded.
fn from_tmux(index: u8, text: &str) -> Tuple {
    use cpd_core::host::tmux::pill as tmux;
    let cells = decode(&tmux::pill(&tmux::fg(index), "", text));
    let (first, last) = (cells[0], cells[cells.len() - 1]);
    let mid = &cells[1..cells.len() - 1];
    assert!(
        cells.iter().all(|c| c.1 == Some(index)),
        "one colour on every cell"
    );
    (
        first.0.to_string(),
        last.0.to_string(),
        first.1.unwrap(),
        mid.iter().all(|c| c.2),
        first.2 || last.2,
        mid.iter().map(|c| c.0).collect(),
    )
}

#[test]
fn the_ratatui_and_tmux_pills_decode_to_the_same_tuple_for_every_meaning() {
    let s = spec();
    let m = &s.meanings;
    let bases = [
        m.plain,
        m.in_progress,
        m.review,
        m.done,
        m.failed,
        m.planning,
        m.nothing,
        m.focus,
    ];
    for index in bases.into_iter().flat_map(|i| [i, s.clickable(i)]) {
        for text in ["two words", "x", "wait…"] {
            let tmux = from_tmux(index, text);
            assert_eq!(
                from_ratatui(index, text),
                tmux,
                "colour {index}, text {text:?}"
            );
            assert_eq!(
                (tmux.0.as_str(), tmux.1.as_str()),
                (s.caps.left.as_str(), s.caps.right.as_str())
            );
        }
    }
}
