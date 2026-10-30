//! The cockpit's loader is pi's: the 28 moon_alt phases at 50 ms (hs-d616). Static dots (hs-d620): needs-you is the filled U+E3E3, idle and stopped the hollow U+E3D5. Expected glyphs are codepoints, never the constants'
//! own values, so a wrong table cannot agree with itself.

use std::time::{Duration, UNIX_EPOCH};

use cpd_core::config::Tier;
use ratatui::style::{Color, Style};

use crate::palette::{self, Meaning, dot_span, frame};
use crate::plan::flow::{FlowJob, Live, layout};
use crate::read::{Dot, JobStatus, Work};
use crate::widgets::legend;

const MOONS: [&str; 28] = [
    "\u{E3E3}", "\u{E3C8}", "\u{E3C9}", "\u{E3CA}", "\u{E3CB}", "\u{E3CC}", "\u{E3CD}",
    "\u{E3CE}", "\u{E3CF}", "\u{E3D0}", "\u{E3D1}", "\u{E3D2}", "\u{E3D3}", "\u{E3D4}",
    "\u{E3D5}", "\u{E3D6}", "\u{E3D7}", "\u{E3D8}", "\u{E3D9}", "\u{E3DA}", "\u{E3DB}",
    "\u{E3DC}", "\u{E3DD}", "\u{E3DE}", "\u{E3DF}", "\u{E3E0}", "\u{E3E1}", "\u{E3E2}",
];
const FILLED: &str = "\u{E3E3}";
const WAITING_GLYPH: &str = "\u{F1853}";
const HOLLOW: &str = "\u{E3D5}";

const ALL_DOTS: [Dot; 10] =
    [Dot::Stopped, Dot::Idle, Dot::Waiting, Dot::Working(None), Dot::Fixing, Dot::White, Dot::Cyan, Dot::Red, Dot::Yellow, Dot::Magenta];

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
fn the_spinner_is_the_28_moon_alt_phases_in_order() {
    assert_eq!(palette::SPINNER.to_vec(), MOONS.to_vec());
}

#[test]
fn the_frame_wraps_every_28_fiftieths_of_a_second_on_the_shared_clock() {
    let at = |ms: u64| frame(UNIX_EPOCH + Duration::from_millis(ms));
    assert_eq!([0, 49, 50, 99, 100, 1349, 1350, 1399, 1400, 1449, 1450, 2800].map(at), [0, 0, 1, 1, 2, 26, 27, 27, 0, 0, 1, 0]);
    assert_eq!(frame(UNIX_EPOCH - Duration::from_secs(1)), 0, "before the epoch");
}

#[test]
fn the_needs_you_dot_is_the_filled_moon_alt_new_and_the_idle_and_stopped_dots_the_hollow_moon_alt_full() {
    assert_eq!(dot_span(Dot::Idle, 0, false).content, HOLLOW);
    assert_eq!(dot_span(Dot::Stopped, 0, false).content, HOLLOW);
    for dot in [Dot::Red, Dot::Yellow, Dot::Magenta, Dot::Cyan, Dot::White] {
        assert_eq!(dot_span(dot, 0, false).content, FILLED, "{dot:?}");
    }
    assert_eq!(dot_span(Dot::Idle, 0, false).style, Style::new().fg(Color::White), "idle: 15");
    assert_eq!(dot_span(Dot::Stopped, 0, false).style, Style::new().fg(Color::DarkGray), "stopped: 8");
}

#[test]
fn the_waiting_dot_is_circle_opacity_in_the_waiting_colour() {
    let span = dot_span(Dot::Waiting, 0, false);
    assert_eq!(span.content, WAITING_GLYPH);
    assert_eq!(span.style, Style::new().fg(palette::color_clickable(Meaning::Waiting)));
}

/// pro-d200: the loader of each of the five working colours, from the numbers of the terminal's palette.
#[test]
fn dots_moon_loader_frames_in_every_working_colour() {
    let table = [
        (Dot::Working(Some(Work::Planning)), Color::LightMagenta),
        (Dot::Working(Some(Work::Dispatch)), Color::LightCyan),
        (Dot::Working(Some(Work::Review)), Color::LightYellow),
        (Dot::Working(None), Color::White),
        (Dot::Fixing, Color::LightRed),
    ];
    for (dot, colour) in table {
        for f in 0..57 {
            let span = dot_span(dot, f, false);
            assert_eq!(span.content, MOONS[f % 28], "{dot:?} frame {f}");
            assert_eq!(span.style, Style::new().fg(colour), "{dot:?} frame {f}");
            assert_eq!(span.content, pill_icon(Live::Running, f), "{dot:?} frame {f}: the Job pill's running spinner");
        }
    }
}

#[test]
fn a_running_job_pill_spins_through_the_28_moon_alt_phases() {
    let icons: Vec<String> = (0..29).map(|f| pill_icon(Live::Running, f)).collect();
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
fn the_legend_shows_the_moon_alt_glyphs_for_every_dot() {
    let shown = legend_dots_text();
    assert!(shown.contains(&MOONS.concat()), "the frames row: {shown:?}");
    for glyph in [FILLED, WAITING_GLYPH, HOLLOW] {
        assert!(shown.contains(glyph), "U+{:X}: {shown:?}", glyph.chars().next().unwrap() as u32);
    }
    let precedence = format!("white > {FILLED} > {WAITING_GLYPH} > {HOLLOW} > {HOLLOW}");
    assert!(shown.contains(&precedence), "the precedence line: {shown:?}");
}

#[test]
fn an_archived_working_or_fixing_dot_and_a_running_jobs_still_icon_are_the_hollow_moon_alt_full() {
    let grey = Style::new().fg(Color::DarkGray);
    for f in 0..28 {
        for dot in [Dot::Working(None), Dot::Fixing] {
            assert_eq!(dot_span(dot, f, true), ratatui::text::Span::styled(HOLLOW, grey), "{dot:?} frame {f}: U+E3D5");
        }
    }
    assert_eq!(palette::job_status(JobStatus::Running, false).0, HOLLOW);
    assert_eq!(dot_span(Dot::Working(None), 0, false).content, FILLED);
    assert_eq!(dot_span(Dot::Working(None), 14, false).content, HOLLOW);
}

#[test]
fn no_dot_draws_a_circle_slice_glyph() {
    for dot in ALL_DOTS {
        for f in 0..58 {
            for archived in [false, true] {
                let glyph = dot_span(dot, f, archived).content.into_owned();
                assert!(!glyph.chars().any(is_circle_slice), "{dot:?} frame {f}: U+{:X}", glyph.chars().next().unwrap() as u32);
            }
        }
    }
    let shown = legend_dots_text();
    assert!(!shown.chars().any(is_circle_slice), "the legend: {shown:?}");
}
