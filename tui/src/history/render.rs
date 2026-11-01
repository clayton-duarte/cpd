//! A pi session file as lines of text, ported from the spike's `render_session`
//! (branch `cpd/cpd-ui`, `tui/src/main.rs`): what the Worker said, did and was
//! told, one block per message part, in the file's order.
//!
//! Only `message` entries show. A line that is not JSON is skipped: the Worker
//! may be writing it. Colours come from the palette's [`Ui`] roles, as in the
//! reference: `you` and its text cyan, `agent` green, `→ tool` yellow.

use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use serde_json::Value;

/// Starts a tool result's line: `nf-md-subdirectory_arrow_left` (U+F060C).
const RESULT: &str = "\u{F060C}";

use crate::palette::{self, Ui};

/// The text of a session file, as lines. Continuation rows of a long line are made by [`crate::scroll::wrap`].
pub(crate) fn render(text: &str, zone: &cpd_core::clock::TimeZone) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        if entry["type"] != "message" {
            continue;
        }
        let role = entry["message"]["role"].as_str().unwrap_or("");
        let stamp = entry["timestamp"].as_str().unwrap_or("");
        let time = match time::OffsetDateTime::parse(
            stamp,
            &time::format_description::well_known::Rfc3339,
        ) {
            Ok(t) => cpd_core::clock::time_of_day(t, zone),
            Err(_) => String::new(),
        };
        let time = || Span::styled(format!("{time} "), palette::muted_text());
        let parts = entry["message"]["content"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        for part in parts {
            let text = part["text"].as_str().unwrap_or("");
            match (role, part["type"].as_str().unwrap_or("")) {
                ("user", "text") => {
                    let you = Style::new().fg(palette::ui(Ui::You));
                    lines.push(Line::from(vec![time(), Span::styled("you  ", you).bold()]));
                    lines.extend(
                        text.lines()
                            .take(12)
                            .map(|l| Line::styled(format!("      {l}"), you)),
                    );
                }
                ("assistant", "text") => {
                    let agent = Style::new().fg(palette::ui(Ui::Agent));
                    lines.push(Line::from(vec![
                        time(),
                        Span::styled("agent", agent).bold(),
                    ]));
                    lines
                        .extend(text.lines().map(|l| {
                            Line::styled(format!("      {l}"), palette::read_only_text())
                        }));
                }
                ("assistant", "toolCall") => {
                    let name = part["name"].as_str().unwrap_or("tool");
                    let arguments = &part["arguments"];
                    let arg = arguments["command"]
                        .as_str()
                        .or(arguments["path"].as_str())
                        .unwrap_or("");
                    let arg: String = arg.lines().next().unwrap_or("").chars().take(160).collect();
                    let tool = Style::new().fg(palette::ui(Ui::Tool));
                    lines.push(Line::from(vec![
                        time(),
                        Span::styled(format!("→ {name} "), tool),
                        Span::styled(arg, palette::read_only_text()),
                    ]));
                }
                ("toolResult", "text") => {
                    let last = text
                        .lines()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("");
                    lines.push(Line::styled(
                        format!(
                            "        {RESULT} {}",
                            last.chars().take(160).collect::<String>()
                        ),
                        palette::muted_text(),
                    ));
                }
                _ => {}
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_result_glyph_is_one_cell_wide() {
        assert_eq!(Span::raw(RESULT).width(), 1);
    }

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn entry(role: &str, part: Value) -> String {
        serde_json::json!({"type": "message", "timestamp": "2026-10-15T16:33:44.329Z", "message": {"role": role, "content": [part]}})
            .to_string()
    }

    #[test]
    fn a_session_file_is_the_conversation() {
        let file = [
            r#"{"type":"session","id":"x"}"#.to_owned(),
            entry("user", serde_json::json!({"type": "text", "text": "Do it.\nNow."})),
            entry("assistant", serde_json::json!({"type": "thinking", "thinking": "hm"})),
            entry("assistant", serde_json::json!({"type": "text", "text": "Done.\n\nReally."})),
            entry("assistant", serde_json::json!({"type": "toolCall", "name": "bash", "arguments": {"command": "ls -l\nrm x"}})),
            entry("assistant", serde_json::json!({"type": "toolCall", "name": "read", "arguments": {"path": "a.rs"}})),
            entry("toolResult", serde_json::json!({"type": "text", "text": "one\ntwo\n\n"})),
            "{\"type\":\"message\",\"mess".to_owned(),
        ]
        .join("\n");
        let lines: Vec<String> = render(
            &file,
            &<cpd_core::clock::ManualClock as cpd_core::clock::Clock>::zone(
                &cpd_core::clock::ManualClock::new(),
            ),
        )
        .iter()
        .map(text)
        .collect();
        assert_eq!(
            lines,
            [
                "12:33:44 you  ",
                "      Do it.",
                "      Now.",
                "12:33:44 agent",
                "      Done.",
                "      ",
                "      Really.",
                "12:33:44 → bash ls -l",
                "12:33:44 → read a.rs",
                "        󰘌 two",
            ],
            "thinking, the session header and a half-written line are skipped"
        );
    }

    #[test]
    fn a_user_message_shows_twelve_lines_at_most() {
        let body = (1..=20)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render(
            &entry("user", serde_json::json!({"type": "text", "text": body})),
            &<cpd_core::clock::ManualClock as cpd_core::clock::Clock>::zone(
                &cpd_core::clock::ManualClock::new(),
            ),
        );
        assert_eq!(lines.len(), 1 + 12);
    }

    #[test]
    fn the_palette_colours_who_speaks() {
        let file = [
            entry("user", serde_json::json!({"type": "text", "text": "q"})),
            entry(
                "assistant",
                serde_json::json!({"type": "text", "text": "a"}),
            ),
            entry(
                "assistant",
                serde_json::json!({"type": "toolCall", "name": "bash", "arguments": {}}),
            ),
        ]
        .join("\n");
        let lines = render(
            &file,
            &<cpd_core::clock::ManualClock as cpd_core::clock::Clock>::zone(
                &cpd_core::clock::ManualClock::new(),
            ),
        );
        let colour = |line: usize, span: usize| lines[line].spans[span].style.fg;
        assert_eq!(colour(0, 1), Some(palette::ui(Ui::You)), "you");
        assert_eq!(colour(2, 1), Some(palette::ui(Ui::Agent)), "agent");
        assert_eq!(colour(4, 1), Some(palette::ui(Ui::Tool)), "a tool call");
        assert_eq!(
            lines[1].style.fg,
            Some(palette::ui(Ui::You)),
            "the User's words"
        );
    }

    fn muted() -> ratatui::style::Color {
        palette::muted_text().fg.unwrap()
    }

    /// The reference's `render_session` (`tui/src/main.rs` on `main`), verbatim but for reading `text` instead of a path.
    fn reference_render_session(text: &str) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for raw in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(raw) else {
                continue;
            };
            if v["type"] != "message" {
                continue;
            }
            let role = v["message"]["role"].as_str().unwrap_or("");
            // the reference drew the UTC time of day; the fixture is in October, so the local time is EDT, UTC - 4 h
            let utc = v["timestamp"]
                .as_str()
                .unwrap_or("")
                .get(11..19)
                .unwrap_or("");
            let ts = if utc.is_empty() {
                String::new()
            } else {
                format!("{:02}{}", utc[..2].parse::<u32>().unwrap() - 4, &utc[2..])
            };
            let empty = Vec::new();
            let content = v["message"]["content"].as_array().unwrap_or(&empty);
            for part in content {
                match (role, part["type"].as_str().unwrap_or("")) {
                    ("user", "text") => {
                        lines.push(Line::from(vec![
                            format!("{ts} ").fg(muted()),
                            "you  ".fg(palette::ui(Ui::You)).bold(),
                        ]));
                        for l in part["text"].as_str().unwrap_or("").lines().take(12) {
                            lines.push(Line::from(format!("      {l}")).fg(palette::ui(Ui::You)));
                        }
                    }
                    ("assistant", "text") => {
                        lines.push(Line::from(vec![
                            format!("{ts} ").fg(muted()),
                            "agent".fg(palette::ui(Ui::Agent)).bold(),
                        ]));
                        for l in part["text"].as_str().unwrap_or("").lines() {
                            lines.push(
                                Line::from(format!("      {l}"))
                                    .fg(palette::read_only_text().fg.unwrap()),
                            );
                        }
                    }
                    ("assistant", "toolCall") => {
                        let name = part["name"].as_str().unwrap_or("tool");
                        let arg = part["arguments"]["command"]
                            .as_str()
                            .or(part["arguments"]["path"].as_str())
                            .unwrap_or("");
                        let arg: String =
                            arg.lines().next().unwrap_or("").chars().take(160).collect();
                        lines.push(Line::from(vec![
                            format!("{ts} ").fg(muted()),
                            format!("→ {name} ").fg(palette::ui(Ui::Tool)),
                            arg.fg(palette::read_only_text().fg.unwrap()),
                        ]));
                    }
                    ("toolResult", "text") => {
                        let t = part["text"].as_str().unwrap_or("");
                        let last: String = t
                            .lines()
                            .rev()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("")
                            .chars()
                            .take(160)
                            .collect();
                        lines.push(Line::from(format!("        {RESULT} {last}")).fg(muted()));
                    }
                    _ => {}
                }
            }
        }
        lines
    }

    #[test]
    fn a_fixture_session_file_renders_as_the_reference_does() {
        let file = include_str!("fixtures/session.jsonl");
        let ours = render(
            file,
            &<cpd_core::clock::ManualClock as cpd_core::clock::Clock>::zone(
                &cpd_core::clock::ManualClock::new(),
            ),
        );
        assert_eq!(
            ours.len(),
            23,
            "the fixture reaches every branch: {ours:#?}"
        );
        assert_eq!(ours, reference_render_session(file));
    }
}
