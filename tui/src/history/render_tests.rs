//! The session renderer: what a pi session file draws as, and in which colours (contract §6.1.1).

use serde_json::json;

use super::render::render;
use crate::palette::numbered::{C6, C7};
use cpd_core::clock::{Clock, ManualClock};

fn entry(role: &str, part: serde_json::Value) -> String {
    json!({"type": "message", "timestamp": "2026-10-15T16:33:44.329Z", "message": {"role": role, "content": [part]}}).to_string() + "\n"
}

fn file() -> String {
    [
        json!({"type": "session", "id": "w1"}).to_string() + "\n",
        entry(
            "user",
            json!({"type": "text", "text": "Implement the API."}),
        ),
        entry("assistant", json!({"type": "text", "text": "On it."})),
        entry(
            "assistant",
            json!({"type": "toolCall", "name": "bash", "arguments": {"command": "cargo test"}}),
        ),
        entry(
            "toolResult",
            json!({"type": "text", "text": "test result: ok"}),
        ),
    ]
    .concat()
}

#[test]
fn a_session_file_renders_one_block_per_message_part() {
    let rows: Vec<String> = render(&file(), &ManualClock::new().zone())
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect();
    assert_eq!(
        rows,
        [
            "12:33:44 you",
            "      Implement the API.",
            "12:33:44 agent",
            "      On it.",
            "12:33:44 → bash cargo test",
            "        \u{F060C} test result: ok"
        ]
    );
}

/// `you` and the User's text are grey (7), `agent` and `→ tool` cyan (6): contract §6.1.1.
#[test]
fn the_roles_are_in_the_palettes_colours() {
    let lines = render(&file(), &ManualClock::new().zone());
    let fg = |row: usize, span: usize| lines[row].spans[span].style.fg.or(lines[row].style.fg);
    assert_eq!(fg(0, 1), Some(C7), "you");
    assert_eq!(fg(1, 0), Some(C7), "the User's text");
    assert_eq!(fg(2, 1), Some(C6), "agent");
    assert_eq!(fg(4, 1), Some(C6), "tool");
}
