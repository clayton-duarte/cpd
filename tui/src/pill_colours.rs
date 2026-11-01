//! Tests for the pill colours of contract §6.1.1 (hs-d534, which replaces hs-d530's and hs-d531's colours).
//! A pill is inverted (contract §6.1.1, hs-d594): its text is the colour as fg, the terminal's bg, REVERSED; its caps the colour, not reversed.
//! Colours are named through `palette::numbered` (no colour outside the palette).

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::{Paragraph, Widget};

use crate::palette::numbered::{C4, C5, C6, C7, C15};
use crate::palette::{self};
const CAP_L: &str = "\u{e0b6}";
const CAP_R: &str = "\u{e0b4}";
use crate::plan::flow::{FlowJob, Live, layout};
use crate::section;
use crate::widgets::legend;

const RESET: ratatui::style::Color = Cell::EMPTY.bg;

/// A block's text is inverted: fg the block's colour, bg the terminal's default, REVERSED.
fn assert_inverted(style: ratatui::style::Style, colour: ratatui::style::Color) {
    assert_eq!((style.fg, style.bg), (Some(colour), Some(RESET)));
    assert!(style.add_modifier.contains(Modifier::REVERSED), "{style:?}");
}

#[test]
fn static_pill_text_is_inverted_on_colour_7() {
    assert_inverted(palette::pill_static(), C7);
}

#[test]
fn interactive_pill_text_is_inverted_on_colour_15() {
    assert_inverted(palette::pill_interactive(), C15);
}

#[test]
fn focus_pill_text_is_inverted_on_colour_4() {
    assert_inverted(palette::pill_focus(), C4);
}

/// The pill whose left cap is at (`x`, `y`): its caps are `caps` as foreground on the terminal's background, its text `fg` on `bg`, not reversed.
fn assert_pill(buf: &Buffer, x: u16, y: u16, text: &str, bg: ratatui::style::Color) {
    let width = text.chars().count() as u16;
    let drawn: String = (x..x + width + 2).map(|x| buf[(x, y)].symbol()).collect();
    assert_eq!(
        drawn,
        format!("{CAP_L}{text}{CAP_R}"),
        "the pill at ({x}, {y})"
    );
    for cap in [x, x + width + 1] {
        let cell = &buf[(cap, y)];
        assert_eq!(
            (cell.fg, cell.bg),
            (bg, RESET),
            "the cap at ({cap}, {y}): {cell:?}"
        );
        assert!(
            !cell.modifier.contains(Modifier::REVERSED),
            "the cap at ({cap}, {y}) is not reversed: {cell:?}"
        );
    }
    for at in x + 1..=x + width {
        let cell = &buf[(at, y)];
        assert_eq!(
            (cell.fg, cell.bg),
            (bg, RESET),
            "the text at ({at}, {y}): {cell:?}"
        );
        assert!(
            cell.modifier.contains(Modifier::REVERSED),
            "the text at ({at}, {y}) is reversed: {cell:?}"
        );
    }
}

fn row(line: ratatui::text::Line<'static>, width: u16) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
    Paragraph::new(line).render(buf.area, &mut buf);
    buf
}

#[test]
fn section_title_pills_are_inverted_with_caps_in_colour_7() {
    // ` ───<Inbox>────…─<3> `: the title's pill and the count pill
    for (line, text, count) in [
        (section::flush_title("Inbox", Some(3), 30), "Inbox", true),
        (section::title("Logs", None, 30), "Logs", false),
    ] {
        let buf = row(line, 30);
        let cap_at = |nth: usize| (0..30).filter(|x| buf[(*x, 0)].symbol() == CAP_L).nth(nth);
        assert_pill(&buf, cap_at(0).expect("a title pill"), 0, text, C7);
        if count {
            assert_pill(&buf, cap_at(1).expect("a count pill"), 0, "3", C7);
        }
    }
}

#[test]
fn job_pills_are_inverted_base_with_caps_in_their_colour() {
    let statuses = [(Live::Running, C6), (Live::Draft, C5), (Live::Queued, C7)];
    let jobs: Vec<FlowJob> = statuses
        .into_iter()
        .map(|(status, _)| status)
        .map(|status| FlowJob {
            key: format!("{status:?}").to_lowercase(),
            status,
            tier: None,
            needs: vec![],
            attempt: 2,
        })
        .collect();
    let chart = layout(&jobs, 0);
    assert_eq!(chart.pills.len(), 3);
    for (p, (_, bg)) in chart.pills.iter().zip(statuses) {
        let r = p.rect;
        for cap in [r.x, r.right() - 1] {
            let cell = &chart.buf[(cap, r.y)];
            assert_eq!(
                (cell.fg, cell.bg),
                (bg, RESET),
                "{} cap at {cap}: {cell:?}",
                p.key
            );
        }
        // the icon, the key, `│`, `#n` and the tier: every cell of the body
        for x in r.x + 1..r.right() - 1 {
            let cell = &chart.buf[(x, r.y)];
            assert_eq!(
                (cell.fg, cell.bg),
                (bg, RESET),
                "{} body at {x} ({:?}): {cell:?}",
                p.key,
                cell.symbol()
            );
            assert!(
                cell.modifier.contains(Modifier::REVERSED),
                "{} body at {x} is reversed: {cell:?}",
                p.key
            );
        }
    }
}

#[test]
fn legend_pills_are_inverted_with_caps_in_their_colour() {
    let (w, h) = legend::size();
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| legend::draw(f, f.area())).unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut pills = 0;
    for y in 0..h {
        let mut x = 0;
        while x < w {
            if buf[(x, y)].symbol() == CAP_L {
                let end = (x + 1..w)
                    .find(|e| buf[(*e, y)].symbol() == CAP_R)
                    .expect("a pill closes");
                let cap = |at: u16| (buf[(at, y)].fg, buf[(at, y)].bg);
                let colour = buf[(x, y)].fg;
                assert_eq!(cap(x), (colour, RESET), "left cap at ({x}, {y})");
                assert_eq!(cap(end), (colour, RESET), "right cap at ({end}, {y})");
                for at in x + 1..end {
                    let cell = &buf[(at, y)];
                    assert_eq!(
                        (cell.fg, cell.bg),
                        (colour, RESET),
                        "pill text at ({at}, {y}): {cell:?}"
                    );
                    assert!(
                        cell.modifier.contains(Modifier::REVERSED),
                        "pill text at ({at}, {y}) is reversed: {cell:?}"
                    );
                }
                pills += 1;
                x = end;
            }
            x += 1;
        }
    }
    // the title and the Job samples at least
    assert!(
        pills >= 3,
        "the legend draws its title and sample pills: {pills}"
    );
}

#[test]
fn no_tui_source_names_colour_0_or_black() {
    // the needles are built, so that this file names none of them
    let black = format!("Color::{}", "Black");
    let indexed = format!("Indexed({})", 0);
    let c0 = format!("C{}", 0);
    let numbered_c0 = format!("numbered::{c0}");
    let ansi = format!("[{}m", 30);
    let c256 = format!("38;5;{}", 0);
    fn word(line: &str, w: &str) -> bool {
        let ident = |c: char| c.is_alphanumeric() || c == '_';
        line.match_indices(w).any(|(i, _)| {
            !line[..i].chars().next_back().is_some_and(ident)
                && !line[i + w.len()..].chars().next().is_some_and(ident)
        })
    }
    fn walk(dir: &std::path::Path, offenders: &mut Vec<String>, check: &dyn Fn(&str) -> bool) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, offenders, check);
            } else if path.extension().is_some_and(|e| e == "rs")
                && path.file_name().is_some_and(|n| n != "pill_colours.rs")
            {
                let text = std::fs::read_to_string(&path).unwrap();
                for (n, line) in text.lines().enumerate() {
                    if check(line) {
                        offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                    }
                }
            }
        }
    }
    let check = |line: &str| {
        [&black, &indexed, &numbered_c0, &ansi, &c256]
            .iter()
            .any(|n| line.contains(n.as_str()))
            || word(line, &c0)
    };
    let mut offenders = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut offenders,
        &check,
    );
    assert!(
        offenders.is_empty(),
        "colour 0 and black are the terminal's own:\n{}",
        offenders.join("\n")
    );
}
