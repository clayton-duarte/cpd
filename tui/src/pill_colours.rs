//! Red tests for the pill colours of contract §6.1.1 (hs-d530, hs-d531): written from the docs alone.
//! R1: a non-interactive pill is bg 8, text 7, caps fg 8. R2: an interactive pill is bg 15, text 0, caps fg 15. R3: the focused pill is bg 4, text 0.
//! Colours are named through `palette::numbered` (no colour outside the palette).

use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Terminal;

use crate::palette::numbered::{C0, C15, C4, C7, C8};
use crate::palette::{self};
use crate::plan::flow::{layout, FlowJob, Live, CAP_L, CAP_R};
use crate::section;
use crate::widgets::legend;

const RESET: ratatui::style::Color = Cell::EMPTY.bg;

#[test]
fn static_pill_is_bg8_fg7() {
    let pill = palette::pill_static();
    assert_eq!((pill.bg, pill.fg), (Some(C8), Some(C7)));
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
        assert!(!cell.modifier.contains(Modifier::REVERSED), "a cap is not reversed: {cell:?}");
    }
    for at in x + 1..=x + width {
        let cell = &buf[(at, y)];
        assert_eq!((cell.fg, cell.bg), (fg, bg), "the text at ({at}, {y}): {cell:?}");
        assert!(!cell.modifier.contains(Modifier::REVERSED), "the text is not reversed (the colours are explicit): {cell:?}");
    }
}

fn row(line: ratatui::text::Line<'static>, width: u16) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
    Paragraph::new(line).render(buf.area, &mut buf);
    buf
}

#[test]
fn section_title_draws_bg8_fg7_with_caps_8() {
    // ` ╭─<Inbox>────…─<3> `: the title's pill and the count pill
    for (line, text, count) in [
        (section::collapsible_title("Inbox", Some(3), 30), "Inbox", true),
        (section::title("Logs", None, 30), "Logs", false),
    ] {
        let buf = row(line, 30);
        let cap_at = |nth: usize| (0..30).filter(|x| buf[(*x, 0)].symbol() == CAP_L).nth(nth);
        assert_pill(&buf, cap_at(0).expect("a title pill"), 0, text, C8, C7);
        if count {
            assert_pill(&buf, cap_at(1).expect("a count pill"), 0, "3", C8, C7);
        }
    }
}

#[test]
fn job_pill_draws_bg15_fg0_with_caps_15() {
    let jobs: Vec<FlowJob> = [Live::Running, Live::Draft]
        .into_iter()
        .map(|status| FlowJob { key: format!("{status:?}").to_lowercase(), status, tier: None, needs: vec![], attempt: 2, aside: None })
        .collect();
    let chart = layout(&jobs, 0);
    assert_eq!(chart.pills.len(), 2);
    for p in &chart.pills {
        let r = p.rect;
        for cap in [r.x, r.right() - 1] {
            let cell = &chart.buf[(cap, r.y)];
            assert_eq!((cell.fg, cell.bg), (C15, RESET), "{} cap at {cap}: {cell:?}", p.key);
        }
        // the icon, the key, `│`, the tier and `aN`: every cell of the body
        for x in r.x + 1..r.right() - 1 {
            let cell = &chart.buf[(x, r.y)];
            assert_eq!((cell.fg, cell.bg), (C0, C15), "{} body at {x} ({:?}): {cell:?}", p.key, cell.symbol());
        }
    }
}

#[test]
fn legend_title_and_sample_pills_draw_bg8_fg7() {
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
                assert_eq!(cap(x), (C8, RESET), "left cap at ({x}, {y})");
                assert_eq!(cap(end), (C8, RESET), "right cap at ({end}, {y})");
                for at in x + 1..end {
                    let cell = &buf[(at, y)];
                    assert_eq!((cell.fg, cell.bg), (C7, C8), "pill text at ({at}, {y}): {cell:?}");
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
