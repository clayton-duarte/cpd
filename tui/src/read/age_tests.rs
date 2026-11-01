//! The age text is one rule for the tree and Pulse.

use cpd_core::ids::{PlanId, SessionId};

use super::{Dot, PulseRow, age_text};
use crate::plan::pulse::age_cell;

fn row(age_minutes: Option<i64>) -> PulseRow {
    PulseRow {
        plan: PlanId::parse("p-a").unwrap(),
        lead: SessionId::parse("l-p-a").unwrap(),
        dot: Dot::Idle,
        buckets: vec![],
        merges: 0,
        age_minutes,
        quiet: false,
    }
}

#[test]
fn an_age_is_in_the_largest_whole_unit() {
    for (s, t) in [
        (0, "0s"),
        (59, "59s"),
        (60, "1m"),
        (3599, "59m"),
        (3600, "1h"),
        (86399, "23h"),
        (86400, "1d"),
        (172800, "2d"),
    ] {
        assert_eq!(age_text(s), t, "{s}");
    }
}

#[test]
fn a_negative_age_is_zero_seconds() {
    assert_eq!(age_text(-5), "0s");
}

#[test]
fn thirty_hours_read_1d_in_pulse_as_in_the_tree() {
    assert_eq!(age_text(108_000), "1d");
    assert_eq!(age_cell(&row(Some(1800))).content.trim(), "1d");
}

#[test]
fn an_age_under_a_minute_reads_in_seconds_in_pulse() {
    assert!(age_cell(&row(Some(0))).content.contains("0s"));
}
