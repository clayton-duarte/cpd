//! Tests for the pill colours of contract §6.1.1 (hs-d534, which replaces hs-d530's and hs-d531's colours).
//! A read-only pill is bg 7, text 0, caps fg 7. An interactive pill is bg 15, text 0, caps fg 15. The focused pill is bg 4, text 0.
//! A Job pill is its status colour's bright pair, text 0, caps the same colour.
//! Colours are named through `palette::numbered` (no colour outside the palette).

use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Terminal;

use crate::palette::numbered::{C0, C13, C14, C15, C4, C7};
use crate::palette::{self};
use crate::plan::flow::{layout, FlowJob, Live, CAP_L, CAP_R};
use crate::section;
use crate::widgets::legend;

const RESET: ratatui::style::Color = Cell::EMPTY.bg;

#[test]
fn static_pill_is_bg7_fg0() {
    let pill = palette::pill_static();
    assert_eq!((pill.bg, pill.fg), (Some(C7), Some(C0)));
}

#[test]
fn interactive_pill_is_bg15_fg0() {
    let pill = palette::pill_interactive();
    assert_eq!((pill.bg, pill.fg), (Some(C15), Some(C0)));
}

#[test]
fn focus_pill_is_bg4_fg0() {
    let pill = palette::pill_focus();
    assert_eq!((pill.bg, pill.fg), (Some(C4), Some(C0)));
}

/// The pill whose left cap is at (`x`, `y`): its caps are `caps` as foreground on the terminal's background, its text `fg` on `bg`, not reversed.
fn assert_pill(buf: &Buffer, x: u16, y: u16, text: &str, bg: ratatui::style::Color, fg: ratatui::style::Color) {
    let width = text.chars().count() as u16;
    let drawn: String = (x..x + width + 2).map(|x| buf[(x, y)].symbol()).collect();
    assert_eq!(drawn, format!("{CAP_L}{text}{CAP_R}"), "the pill at ({x}, {y})");
    for cap in [x, x + width + 1] {
        let cell = &buf[(cap, y)];
        assert_eq!((cell.fg, cell.bg), (bg, RESET), "the cap at ({cap}, {y}): {cell:?}");
    }
    for at in x + 1..=x + width {
        let cell = &buf[(at, y)];
        assert_eq!((cell.fg, cell.bg), (fg, bg), "the text at ({at}, {y}): {cell:?}");
    }
}

fn row(line: ratatui::text::Line<'static>, width: u16) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
    Paragraph::new(line).render(buf.area, &mut buf);
    buf
}

#[test]
fn section_title_draws_bg7_fg0_with_caps_7() {
    // ` ╭─<Inbox>────…─<3> `: the title's pill and the count pill
    for (line, text, count) in [
        (section::collapsible_title("Inbox", Some(3), 30), "Inbox", true),
        (section::title("Logs", None, 30), "Logs", false),
    ] {
        let buf = row(line, 30);
        let cap_at = |nth: usize| (0..30).filter(|x| buf[(*x, 0)].symbol() == CAP_L).nth(nth);
        assert_pill(&buf, cap_at(0).expect("a title pill"), 0, text, C7, C0);
        if count {
            assert_pill(&buf, cap_at(1).expect("a count pill"), 0, "3", C7, C0);
        }
    }
}

#[test]
fn job_pill_draws_its_status_colours_bright_with_fg0_and_caps_in_the_same_colour() {
    let statuses = [(Live::Running, C14), (Live::Draft, C13), (Live::Queued, C15)];
    let jobs: Vec<FlowJob> = statuses
        .into_iter()
        .map(|(status, _)| status)
        .map(|status| FlowJob { key: format!("{status:?}").to_lowercase(), status, tier: None, needs: vec![], attempt: 2, aside: None })
        .collect();
    let chart = layout(&jobs, 0);
    assert_eq!(chart.pills.len(), 3);
    for (p, (_, bg)) in chart.pills.iter().zip(statuses) {
        let r = p.rect;
        for cap in [r.x, r.right() - 1] {
            let cell = &chart.buf[(cap, r.y)];
            assert_eq!((cell.fg, cell.bg), (bg, RESET), "{} cap at {cap}: {cell:?}", p.key);
        }
        // the icon, the key, `│`, the tier and `aN`: every cell of the body
        for x in r.x + 1..r.right() - 1 {
            let cell = &chart.buf[(x, r.y)];
            assert_eq!((cell.fg, cell.bg), (C0, bg), "{} body at {x} ({:?}): {cell:?}", p.key, cell.symbol());
        }
    }
}

#[test]
fn legend_title_is_bg7_fg0_and_the_sample_pills_are_bright_with_fg0_and_caps_in_their_colour() {
    let (w, h) = legend::size();
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| legend::draw(f, f.area())).unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut pills = 0;
    for y in 0..h {
        let mut x = 0;
        while x < w {
            if buf[(x, y)].symbol() == CAP_L {
                let end = (x + 1..w).find(|e| buf[(*e, y)].symbol() == CAP_R).expect("a pill closes");
                let cap = |at: u16| (buf[(at, y)].fg, buf[(at, y)].bg);
                let colour = buf[(x + 1, y)].bg;
                assert_eq!(cap(x), (colour, RESET), "left cap at ({x}, {y})");
                assert_eq!(cap(end), (colour, RESET), "right cap at ({end}, {y})");
                for at in x + 1..end {
                    let cell = &buf[(at, y)];
                    assert_eq!((cell.fg, cell.bg), (C0, colour), "pill text at ({at}, {y}): {cell:?}");
                }
                pills += 1;
                x = end;
            }
            x += 1;
        }
    }
    // the title and the Job samples at least
    assert!(pills >= 3, "the legend draws its title and sample pills: {pills}");
}
