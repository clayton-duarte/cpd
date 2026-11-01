//! The Event log rows show the local time of day, bare (PLAN local-time, pro-d49). The zone is `ManualClock`'s.

use cpd_core::clock::{Clock, ManualClock};
use cpd_core::events::Event;

use super::view::line;

fn event(at: &str, text: &str) -> Event {
    Event {
        id: 1,
        at: at.to_owned(),
        kind: "plan-phase".into(),
        context: None,
        plan: None,
        session: None,
        job: None,
        attempt: None,
        text: text.to_owned(),
        data: serde_json::json!({}),
    }
}

fn row(at: &str, text: &str) -> String {
    let zone = ManualClock::new().zone();
    line(&event(at, text), 60, &zone)
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect()
}

#[test]
fn logs_rows_show_the_local_time_of_day() {
    assert!(
        row("2026-10-26T03:05:11.907Z", "Message for italic")
            .starts_with("23:05:11 Message for italic"),
        "{}",
        row("2026-10-26T03:05:11.907Z", "Message for italic")
    );
}

#[test]
fn logs_row_keeps_an_unparsable_time_as_written() {
    let drawn = row("not an RFC 3339 time", "Message for italic");
    assert!(
        drawn.starts_with("not an RFC 3339 time Message for italic"),
        "{drawn}"
    );
}
