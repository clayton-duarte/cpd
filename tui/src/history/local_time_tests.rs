//! The History view rows show the local time of day, bare (PLAN local-time, pro-d49). The zone is `ManualClock`'s.

use cpd_core::clock::{Clock, ManualClock};

use super::render::render;

#[test]
fn history_rows_show_the_local_time_of_day() {
    let file = serde_json::json!({
        "type": "message",
        "timestamp": "2026-10-26T02:54:10.000Z",
        "message": {"role": "user", "content": [{"type": "text", "text": "Do it."}]}
    })
    .to_string();
    let lines = render(&file, &ManualClock::new().zone());
    let first: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(first.starts_with("22:54:10 you"), "{first:?}");
}
