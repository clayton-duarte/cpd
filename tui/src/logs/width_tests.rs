//! Red tests for audit-plan F1 in the Logs: an Event's text is cut to its row in display columns, not `char`s.

use super::{Event, line};

#[test]
fn an_event_in_wide_characters_is_cut_to_the_row() {
    // Twenty `計` (U+8A08): twenty `char`s, forty display columns. The row is 30 columns wide (nine for the clock
    // and the space, twenty-one for the text), so the text must lose columns, not `char`s.
    let event = Event {
        id: 1,
        at: "2026-01-03T00:00:00.000Z".to_owned(),
        kind: "plan-phase".to_owned(),
        context: None,
        plan: None,
        session: None,
        job: None,
        attempt: None,
        text: "計".repeat(20),
        data: serde_json::json!({}),
    };
    let drawn = line(
        &event,
        30,
        &<cpd_core::clock::ManualClock as cpd_core::clock::Clock>::zone(
            &cpd_core::clock::ManualClock::new(),
        ),
    );
    assert!(
        drawn.width() <= 30,
        "the Event's text is cut to the row's columns, not its chars: {drawn:?}"
    );
}
