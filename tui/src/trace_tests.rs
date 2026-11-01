//! The frame trace (commands.md §17 "Frame trace"), from the docs alone.

use std::time::Duration;

use cpd_core::home::CpdHome;

use crate::trace::Trace;
use crate::viewlog::ViewLog;

fn us(n: u64) -> Duration {
    Duration::from_micros(n)
}

#[test]
fn a_trace_is_on_only_when_the_variable_is_exactly_1() {
    for (value, on) in [
        (None, false),
        (Some(""), false),
        (Some("0"), false),
        (Some("true"), false),
        (Some(" 1"), false),
        (Some("1"), true),
    ] {
        assert_eq!(Trace::new(value).is_on(), on, "{value:?}");
    }
}

#[test]
fn a_trace_line_reads_gap_read_layout_draw_and_frame_in_microseconds() {
    let mut t = Trace::new(Some("1"));
    t.line(us(1_000), us(5), 0);
    t.add_read(us(120));
    t.add_layout(us(340));
    let line = t.line(us(101_000), us(560), 3).unwrap();
    assert_eq!(
        line,
        "trace gap_us=100000 read_us=120 layout_us=340 draw_us=560 frame=3"
    );
}

#[test]
fn the_first_draw_has_a_gap_of_zero() {
    let mut t = Trace::new(Some("1"));
    let line = t.line(us(777_000), us(10), 1).unwrap();
    assert_eq!(
        line,
        "trace gap_us=0 read_us=0 layout_us=0 draw_us=10 frame=1"
    );
}

#[test]
fn the_gap_runs_from_one_draw_start_to_the_next() {
    let mut t = Trace::new(Some("1"));
    t.line(us(1_000), us(900), 0).unwrap();
    // The next draw starts 100_000 after the previous START, whatever the previous draw took.
    let second = t.line(us(101_000), us(5), 1).unwrap();
    assert!(second.starts_with("trace gap_us=100000 "), "{second}");
    let third = t.line(us(350_000), us(5), 2).unwrap();
    assert!(third.starts_with("trace gap_us=249000 "), "{third}");
}

#[test]
fn reads_and_layouts_are_summed_since_the_previous_draw_and_then_start_again() {
    let mut t = Trace::new(Some("1"));
    t.add_read(us(10));
    t.add_read(us(20));
    t.add_layout(us(1));
    t.add_layout(us(2));
    t.add_layout(us(3));
    let first = t.line(us(0), us(7), 0).unwrap();
    assert_eq!(
        first,
        "trace gap_us=0 read_us=30 layout_us=6 draw_us=7 frame=0"
    );
    let second = t.line(us(100), us(7), 1).unwrap();
    assert_eq!(
        second,
        "trace gap_us=100 read_us=0 layout_us=0 draw_us=7 frame=1"
    );
}

#[test]
fn a_trace_that_is_off_writes_no_line_and_creates_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    let log = ViewLog::of(&home, "tree");
    let mut t = Trace::new(Some("0"));
    t.add_read(us(5));
    assert_eq!(t.line(us(0), us(1), 0), None);
    t.record(&log, us(100), us(1), 1).unwrap();
    assert!(!dir.path().join("run/tui/tree.log").exists());
    assert!(!dir.path().join("run/tui").exists());
}

#[test]
fn a_trace_line_is_appended_to_the_views_log_after_its_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    let log = ViewLog::of(&home, "plan");
    let mut t = Trace::new(Some("1"));
    t.record(&log, us(0), us(4), 2).unwrap();
    t.add_read(us(9));
    t.record(&log, us(100_000), us(5), 3).unwrap();
    let text = std::fs::read_to_string(dir.path().join("run/tui/plan.log")).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text:?}");
    let (at, rest) = lines[0].split_once(' ').unwrap();
    assert!(
        time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).is_ok(),
        "{at}"
    );
    assert_eq!(
        rest,
        "trace gap_us=0 read_us=0 layout_us=0 draw_us=4 frame=2"
    );
    assert!(
        lines[1].ends_with(" trace gap_us=100000 read_us=9 layout_us=0 draw_us=5 frame=3"),
        "{text:?}"
    );
}
