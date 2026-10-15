//! A pi session file as lines of text, ported from the spike's `render_session`
//! (branch `cpd/cpd-ui`, `tui/src/main.rs`): what the Worker said, did and was
//! told, one block per message part, in the file's order.
//!
//! Only `message` entries show. A line that is not JSON is skipped: the Worker
//! may be writing it. Colours come from the palette (the spike's `Ui` colours
//! are not in it): the agent is [`Meaning::InProgress`] (green, as in the
//! spike), a tool call [`Meaning::Review`] (yellow, as in the spike), and the
//! User's lines [`Meaning::NotStarted`] (white: the spike's cyan has no meaning
//! of its own).

use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::tui::palette::{self, Meaning};

/// The columns the body of a message is indented by.
const INDENT: usize = 6;

/// The text of a session file, as lines. Continuation rows of a long line are made by [`wrap`].
pub(super) fn render(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(raw) else { continue };
        if entry["type"] != "message" {
            continue;
        }
        let role = entry["message"]["role"].as_str().unwrap_or("");
        let time = entry["timestamp"].as_str().unwrap_or("").get(11..19).unwrap_or("");
        let time = || format!("{time} ").dim();
        let parts = entry["message"]["content"].as_array().map_or(&[][..], Vec::as_slice);
        for part in parts {
            let text = part["text"].as_str().unwrap_or("");
            match (role, part["type"].as_str().unwrap_or("")) {
                ("user", "text") => {
                    let you = palette::fg(Meaning::NotStarted);
                    lines.push(Line::from(vec![time(), Span::styled("you  ", you).bold()]));
                    lines.extend(text.lines().take(12).map(|l| Line::styled(format!("      {l}"), you)));
                }
                ("assistant", "text") => {
                    let agent = palette::fg(Meaning::InProgress);
                    lines.push(Line::from(vec![time(), Span::styled("agent", agent).bold()]));
                    lines.extend(text.lines().map(|l| Line::from(format!("      {l}"))));
                }
                ("assistant", "toolCall") => {
                    let name = part["name"].as_str().unwrap_or("tool");
                    let arguments = &part["arguments"];
                    let arg = arguments["command"].as_str().or(arguments["path"].as_str()).unwrap_or("");
                    let arg: String = arg.lines().next().unwrap_or("").chars().take(160).collect();
                    let tool = palette::fg(Meaning::Review);
                    lines.push(Line::from(vec![time(), Span::styled(format!("→ {name} "), tool), arg.into()]));
                }
                ("toolResult", "text") => {
                    let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
                    lines.push(Line::styled(format!("        ← {}", last.chars().take(160).collect::<String>()), Style::new().dim()));
                }
                _ => {}
            }
        }
    }
    lines
}

/// `lines` broken into rows of at most `width` characters, a continuation row
/// indented by [`INDENT`] (nothing when the pane is narrower than twice that).
pub(super) fn wrap(lines: &[Line<'static>], width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let indent = if width >= 2 * INDENT { INDENT } else { 0 };
    let mut rows = Vec::new();
    for line in lines {
        let cells: Vec<(char, Style)> =
            line.spans.iter().flat_map(|s| s.content.chars().map(|c| (c, line.style.patch(s.style)))).collect();
        if cells.len() <= width {
            rows.push(line.clone());
            continue;
        }
        let (first, rest) = cells.split_at(width);
        rows.push(row(0, first));
        rows.extend(rest.chunks(width - indent).map(|chunk| row(indent, chunk)));
    }
    rows
}

/// One row of styled characters after `indent` blanks, neighbours of one style merged into one span.
fn row(indent: usize, cells: &[(char, Style)]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = if indent > 0 { vec![Span::raw(" ".repeat(indent))] } else { Vec::new() };
    for &(c, style) in cells {
        match spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push(c),
            _ => spans.push(Span::styled(c.to_string(), style)),
        }
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let lines: Vec<String> = render(&file).iter().map(text).collect();
        assert_eq!(
            lines,
            [
                "16:33:44 you  ",
                "      Do it.",
                "      Now.",
                "16:33:44 agent",
                "      Done.",
                "      ",
                "      Really.",
                "16:33:44 → bash ls -l",
                "16:33:44 → read a.rs",
                "        ← two",
            ],
            "thinking, the session header and a half-written line are skipped"
        );
    }

    #[test]
    fn a_user_message_shows_twelve_lines_at_most() {
        let body = (1..=20).map(|n| n.to_string()).collect::<Vec<_>>().join("\n");
        let lines = render(&entry("user", serde_json::json!({"type": "text", "text": body})));
        assert_eq!(lines.len(), 1 + 12);
    }

    #[test]
    fn the_palette_colours_who_speaks() {
        let file = [
            entry("user", serde_json::json!({"type": "text", "text": "q"})),
            entry("assistant", serde_json::json!({"type": "text", "text": "a"})),
            entry("assistant", serde_json::json!({"type": "toolCall", "name": "bash", "arguments": {}})),
        ]
        .join("\n");
        let lines = render(&file);
        let colour = |line: usize, span: usize| lines[line].spans[span].style.fg;
        assert_eq!(colour(0, 1), Some(palette::color(Meaning::NotStarted)), "you");
        assert_eq!(colour(2, 1), Some(palette::color(Meaning::InProgress)), "agent");
        assert_eq!(colour(4, 1), Some(palette::color(Meaning::Review)), "a tool call");
        assert_eq!(lines[1].style.fg, Some(palette::color(Meaning::NotStarted)), "the User's words");
    }

    #[test]
    fn long_lines_wrap_with_the_body_indent() {
        let lines = vec![Line::from(vec!["ab".bold(), "cdefghijklmnopqrstuvwxyz".into()])];
        let rows: Vec<String> = wrap(&lines, 16).iter().map(text).collect();
        assert_eq!(rows, ["abcdefghijklmnop", "      qrstuvwxyz"]);
        let wrapped = wrap(&lines, 16);
        assert!(wrapped[0].spans[0].style.add_modifier.contains(ratatui::style::Modifier::BOLD), "styles survive");
        assert_eq!(wrap(&lines, 40).iter().map(text).collect::<Vec<_>>(), ["abcdefghijklmnopqrstuvwxyz"]);
        let narrow: Vec<String> = wrap(&lines, 8).iter().map(text).collect();
        assert_eq!(narrow, ["abcdefgh", "ijklmnop", "qrstuvwx", "yz"], "no indent in a narrow pane");
    }
}
