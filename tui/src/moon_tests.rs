//! The cockpit's loader is pi's: the eight moon phases (hs-d579). Expected glyphs are codepoints, never the constants'
//! own values, so a wrong table cannot agree with itself.

use std::time::{Duration, UNIX_EPOCH};

use cpd_core::config::Tier;
use ratatui::style::{Color, Style};

use crate::palette::{self, Meaning, dot_span, frame};
use crate::plan::flow::{FlowJob, Live, layout};
use crate::read::Dot;
use crate::widgets::legend;

const MOONS: [&str; 8] =
    ["\u{F0F64}", "\u{F0F67}", "\u{F0F61}", "\u{F0F68}", "\u{F0F62}", "\u{F0F66}", "\u{F0F63}", "\u{F0F65}"];
const NEW_MOON: &str = "\u{F0F64}";
const FIRST_QUARTER: &str = "\u{F0F61}";
const FULL_MOON: &str = "\u{F0F62}";

const ALL_DOTS: [Dot; 10] =
    [Dot::Stopped, Dot::Idle, Dot::Waiting, Dot::Working, Dot::Fixing, Dot::White, Dot::Blue, Dot::Red, Dot::Yellow, Dot::Magenta];

fn is_circle_slice(c: char) -> bool {
    ('\u{F0A9E}'..='\u{F0AA5}').contains(&c)
}

fn job(status: Live) -> FlowJob {
    FlowJob { key: "a".into(), status, tier: Some(Tier::parse("S1").unwrap()), needs: vec![], attempt: 1 }
}

/// The icon of the only pill of a chart of one Job in `status`, at `frame`.
fn pill_icon(status: Live, frame: usize) -> String {
    let c = layout(&[job(status)], frame);
    c.buf[(c.pills[0].rect.x + 1, c.pills[0].rect.y)].symbol().to_string()
}

fn legend_dots_text() -> String {
    let sections = legend::sections();
    let dots = sections.iter().find(|s| s.title == "Session Dots").expect("the Session Dots section");
    dots.rows.iter().flatten().flat_map(|line| line.spans.iter().map(|s| s.content.to_string())).collect()
}

#[test]
fn the_spinner_is_the_eight_moon_phases_in_order() {
    assert_eq!(palette::SPINNER.to_vec(), MOONS.to_vec());
}

#[test]
fn the_frame_wraps_every_eight_tenths_of_a_second_on_the_shared_clock() {
    let at = |ms: u64| frame(UNIX_EPOCH + Duration::from_millis(ms));
    assert_eq!([0, 99, 100, 199, 700, 799, 800, 899, 900, 1600].map(at), [0, 0, 1, 1, 7, 7, 0, 0, 1, 0]);
}

#[test]
fn the_idle_and_stopped_dots_are_the_new_moon_and_the_needs_you_dot_is_the_full_moon() {
    assert_eq!(dot_span(Dot::Idle, 0, false).content, NEW_MOON);
    assert_eq!(dot_span(Dot::Stopped, 0, false).content, NEW_MOON);
    for dot in [Dot::Red, Dot::Yellow, Dot::Magenta, Dot::Blue, Dot::White] {
        assert_eq!(dot_span(dot, 0, false).content, FULL_MOON, "{dot:?}");
    }
    assert_eq!(dot_span(Dot::Idle, 0, false).style, Style::new().fg(Color::White), "idle: 15");
    assert_eq!(dot_span(Dot::Stopped, 0, false).style, Style::new().fg(Color::DarkGray), "stopped: 8");
}

#[test]
fn the_waiting_dot_is_the_first_quarter_moon_in_the_waiting_colour() {
    let span = dot_span(Dot::Waiting, 0, false);
    assert_eq!(span.content, FIRST_QUARTER);
    assert_eq!(span.style, Style::new().fg(palette::color_clickable(Meaning::Waiting)));
}

#[test]
fn the_working_dot_turns_through_the_eight_moon_phases_in_bright_cyan() {
    let glyphs: Vec<String> = (0..17).map(|f| dot_span(Dot::Working, f, false).content.into_owned()).collect();
    assert_eq!(glyphs, [MOONS, MOONS].concat().into_iter().chain([MOONS[0]]).map(str::to_owned).collect::<Vec<_>>());
    for f in 0..8 {
        assert_eq!(dot_span(Dot::Working, f, false).style, Style::new().fg(Color::LightCyan), "frame {f}");
    }
}

#[test]
fn a_running_job_pill_spins_through_the_eight_moon_phases() {
    let icons: Vec<String> = (0..9).map(|f| pill_icon(Live::Running, f)).collect();
    assert_eq!(icons, [MOONS.to_vec(), vec![MOONS[0]]].concat());
}

#[test]
fn the_queued_job_pill_keeps_the_circle_outline() {
    for f in 0..8 {
        assert_eq!(pill_icon(Live::Queued, f), "\u{F0766}", "frame {f}");
    }
    assert_eq!(palette::QUEUED, "\u{F0766}");
    // and the moon the running pill shows is not it
    assert_ne!(pill_icon(Live::Running, 0), pill_icon(Live::Queued, 0));
}

#[test]
fn the_legend_shows_the_moon_glyphs_for_every_dot() {
    let shown = legend_dots_text();
    assert!(shown.contains(&MOONS.concat()), "the frames row: {shown:?}");
    for glyph in [FULL_MOON, FIRST_QUARTER, NEW_MOON] {
        assert!(shown.contains(glyph), "U+{:X}: {shown:?}", glyph.chars().next().unwrap() as u32);
    }
    let precedence = format!("white > {NEW_MOON} > {FIRST_QUARTER} > {NEW_MOON} > {NEW_MOON}");
    assert!(shown.contains(&precedence), "the precedence line: {shown:?}");
}

#[test]
fn no_dot_draws_a_circle_slice_glyph() {
    for dot in ALL_DOTS {
        for f in 0..18 {
            for archived in [false, true] {
                let glyph = dot_span(dot, f, archived).content.into_owned();
                assert!(!glyph.chars().any(is_circle_slice), "{dot:?} frame {f}: U+{:X}", glyph.chars().next().unwrap() as u32);
            }
        }
    }
    let shown = legend_dots_text();
    assert!(!shown.chars().any(is_circle_slice), "the legend: {shown:?}");
}
