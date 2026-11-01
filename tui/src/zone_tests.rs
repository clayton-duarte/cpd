//! The cockpit views draw their times in the zone the caller gives them, never the machine's (PLAN local-time). The
//! zone here is `<+1245>-12:45`, which no machine is likely to be in, so a view that read the system zone could not pass.

use cpd_core::clock::TimeZone;
use cpd_core::events::Event;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::logs::{Log, draw};
use crate::test_text::rows;

fn zone() -> TimeZone {
    TimeZone::posix("<+1245>-12:45").expect("a valid rule")
}

#[test]
fn a_logs_view_draws_the_zone_it_was_given() {
    let event = Event {
        id: 1,
        at: "2026-10-26T03:05:11.907Z".to_owned(),
        kind: "plan-phase".into(),
        context: None,
        plan: None,
        session: None,
        job: None,
        attempt: None,
        text: "Plan ship: dispatch".into(),
        data: serde_json::json!({}),
    };
    let mut log = Log::new(vec![event], zone());
    let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
    terminal
        .draw(|f| {
            log.fit(f.area().height);
            draw(f, f.area(), &log);
        })
        .unwrap();
    let rows = rows(terminal.backend().buffer());
    assert!(rows[3].starts_with(" 15:50:11 "), "{rows:?}");
}

#[test]
fn a_history_view_draws_the_zone_it_was_given() {
    let entry = serde_json::json!({"type": "message", "timestamp": "2026-10-26T02:54:10.000Z", "message": {"role": "user", "content": [{"type": "text", "text": "Do it."}]}});
    let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
    terminal
        .draw(|f| crate::draw_history("lead", true, &(entry.to_string() + "\n"), &zone(), f))
        .unwrap();
    let rows = rows(terminal.backend().buffer());
    assert!(rows[2].starts_with(" 15:39:10 you"), "{rows:?}");
}
