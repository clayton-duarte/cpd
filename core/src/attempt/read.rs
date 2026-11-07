//! `attempt read` (commands.md §6.2): a readable transcript of the Worker's pi session.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{Value, json};

use super::row;
use super::{Env, ReadArgs};
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;

pub(super) fn read(args: ReadArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let id = args.attempt;
    let mut env = Env::open()?;
    let _ = env
        .store
        .read(|conn| crate::plan::shaping::guard(conn, &plan))?;
    let (attempt, session, file) = env.store.read(|conn| {
        let attempt = row::load(conn, &plan, &id)?;
        let session = attempt.worker_session.clone();
        let file: Option<Option<String>> = match &session {
            Some(s) => conn
                .query_row("SELECT file FROM sessions WHERE id = ?1", [s], |r| r.get(0))
                .optional()?,
            None => None,
        };
        Ok((attempt, session, file.flatten()))
    })?;
    let durable = attempt.runner == "durable";
    let missing = |why: &str| {
        CpdError::new(
            ErrorCode::SessionNotFound,
            format!("Attempt {id} has no Worker session file: {why}."),
        )
        .with("attempt", id.to_string())
        .with("session", session.clone())
        .with("file", file.clone())
    };
    if durable {
        let path = env.home.durable_store(&plan, &id);
        let shown = path.display().to_string();
        if !path.exists() {
            return Err(missing("its durable store does not exist (yet)").with("file", shown));
        }
        let records = read_records(&path)?;
        let all = durable_transcript(&path, &records)?;
        let keep = all.len().saturating_sub(args.lines as usize);
        return Ok(Output::success(json!({
            "attempt": attempt.id, "session": session, "file": shown, "total": all.len(), "lines": &all[keep..],
        })));
    }
    let (Some(session), Some(file)) = (&session, &file) else {
        return Err(missing(
            "no Session is registered for it, or its file is not known yet",
        ));
    };
    let path = Path::new(file);
    let text = fs::read_to_string(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => missing("the file does not exist (yet)"),
        _ => malformed(path, None, format!("cannot be read: {e}")),
    })?;
    let all = transcript(path, &text)?;
    let keep = all.len().saturating_sub(args.lines as usize);
    Ok(Output::success(json!({
        "attempt": attempt.id, "session": session, "file": file, "total": all.len(), "lines": &all[keep..],
    })))
}

fn read_records(path: &Path) -> Result<Vec<String>, CpdError> {
    let fail = |e: rusqlite::Error| malformed(path, None, format!("cannot be read: {e}"));
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(fail)?;
    let mut stmt = conn
        .prepare("SELECT record FROM entries ORDER BY id")
        .map_err(fail)?;
    stmt.query_map([], |r| r.get::<_, String>(0))
        .map_err(fail)?
        .collect::<Result<_, _>>()
        .map_err(fail)
}

fn malformed(path: &Path, line: Option<usize>, why: String) -> CpdError {
    let place = line.map_or(String::new(), |n| format!(":{n}"));
    let e = CpdError::new(
        ErrorCode::SessionMalformed,
        format!("{}{place}: {why}.", path.display()),
    )
    .with("file", path.display().to_string());
    match line {
        Some(n) => e.with("line", n),
        None => e,
    }
}

/// One session line: only its `type` is needed to know it is an entry.
#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type")]
    kind: String,
}

/// A `message` entry.
#[derive(Deserialize)]
struct MessageEntry {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    role: String,
    content: Option<Content>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Blocks(Vec<Block>),
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
    name: Option<String>,
}

/// One line per assistant text line (`assistant: …`) and per tool call (`tool: <name>`),
/// in session order. A last line without its newline is still being written: if it is not
/// JSON yet, it is skipped; every other line must be a session entry.
fn transcript(path: &Path, text: &str) -> Result<Vec<String>, CpdError> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    let partial = (!text.ends_with('\n')).then(|| lines.len() - 1);
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut out = Vec::new();
    for (i, line) in lines.into_iter().enumerate() {
        let n = i + 1;
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) if partial == Some(i) => continue,
            Err(e) => return Err(malformed(path, Some(n), format!("invalid JSON: {e}"))),
        };
        let entry = Entry::deserialize(&value)
            .map_err(|_| malformed(path, Some(n), "not a session entry".to_owned()))?;
        if entry.kind != "message" {
            continue;
        }
        let message = MessageEntry::deserialize(&value)
            .map_err(|_| {
                malformed(
                    path,
                    Some(n),
                    "message entry without a role and content".to_owned(),
                )
            })?
            .message;
        if message.role != "assistant" {
            continue;
        }
        push_content(&mut out, message.content);
    }
    Ok(out)
}

/// The lines of one assistant message's content: `assistant: …` per text line, `tool: <name>` per tool call.
fn push_content(out: &mut Vec<String>, content: Option<Content>) {
    match content {
        None => {}
        Some(Content::Text(text)) => {
            out.extend(text.split('\n').map(|l| format!("assistant: {l}")))
        }
        Some(Content::Blocks(blocks)) => {
            for block in blocks {
                match (block.kind.as_str(), block.text, block.name) {
                    ("text", Some(text), _) => {
                        out.extend(text.split('\n').map(|l| format!("assistant: {l}")))
                    }
                    ("toolCall", _, Some(name)) => out.push(format!("tool: {name}")),
                    _ => {}
                }
            }
        }
    }
}

/// A `durable` store record: `kind` and the first model message.
#[derive(Deserialize)]
struct DurableRecord {
    kind: String,
    model: Option<Vec<DurableMessage>>,
}

#[derive(Deserialize)]
struct DurableMessage {
    content: Option<Content>,
}

/// The transcript lines of a `durable` Attempt's store: `records` are the `entries.record` texts in `id` order.
fn durable_transcript(path: &Path, records: &[String]) -> Result<Vec<String>, CpdError> {
    let mut out = Vec::new();
    for (i, record) in records.iter().enumerate() {
        let n = i + 1;
        let value: Value = serde_json::from_str(record)
            .map_err(|e| malformed(path, Some(n), format!("invalid JSON: {e}")))?;
        let entry = DurableRecord::deserialize(&value)
            .map_err(|_| malformed(path, Some(n), "not a store record".to_owned()))?;
        if entry.kind != "pi.assistant" {
            continue;
        }
        let first = entry
            .model
            .and_then(|m| m.into_iter().next())
            .ok_or_else(|| {
                malformed(
                    path,
                    Some(n),
                    "assistant record without a model message".to_owned(),
                )
            })?;
        push_content(&mut out, first.content);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Result<Vec<String>, CpdError> {
        transcript(Path::new("/s.jsonl"), text)
    }

    #[test]
    fn assistant_text_and_tool_calls_in_order() {
        let text = concat!(
            r#"{"type":"session","id":"x"}"#,
            "\n",
            r#"{"type":"message","message":{"role":"user","content":"hi"}}"#,
            "\n",
            r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"a\nb"},{"type":"toolCall","name":"bash","arguments":{}}]}}"#,
            "\n",
            r#"{"type":"message","message":{"role":"assistant","content":"plain"}}"#,
            "\n",
        );
        assert_eq!(
            lines(text).unwrap(),
            [
                "assistant: a",
                "assistant: b",
                "tool: bash",
                "assistant: plain"
            ]
        );
    }

    #[test]
    fn a_partial_last_line_is_skipped_but_a_bad_complete_one_is_not() {
        let good = r#"{"type":"session"}"#;
        assert_eq!(
            lines(&format!("{good}\n{{\"type\":\"mess")).unwrap(),
            Vec::<String>::new()
        );
        let err = lines(&format!("{{\"type\":\"mess\n{good}\n")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::SessionMalformed);
        assert_eq!(err.details()["line"], 1);
        assert_eq!(lines("").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_line_that_is_not_an_entry_is_malformed() {
        for bad in [
            "[]\n",
            "{\"x\":1}\n",
            "{\"type\":\"message\"}\n",
            "{\"type\":\"message\",\"message\":{\"content\":[]}}\n",
        ] {
            assert_eq!(
                lines(bad).unwrap_err().code(),
                ErrorCode::SessionMalformed,
                "{bad}"
            );
        }
    }

    fn record(kind: &str, role: &str, content: &str) -> String {
        format!(
            r#"{{"kind":"{kind}","id":1,"conversationId":1,"model":[{{"role":"{role}","content":{content}}}]}}"#
        )
    }

    #[test]
    fn durable_assistant_text_and_tool_calls_in_order() {
        let records = [
            record("pi.system", "system", r#""s""#),
            record("pi.user", "user", r#"[{"type":"text","text":"hi"}]"#),
            record(
                "pi.assistant",
                "assistant",
                r#"[{"type":"thinking","thinking":"hm"},{"type":"text","text":"a\nb"},{"type":"toolCall","id":"t","name":"bash","arguments":{}}]"#,
            ),
            record(
                "pi.tool-result",
                "toolResult",
                r#"[{"type":"text","text":"ok"}]"#,
            ),
            record(
                "pi.assistant",
                "assistant",
                r#"[{"type":"text","text":"done"},{"type":"toolCall","id":"u","name":"read","arguments":{}}]"#,
            ),
        ];
        assert_eq!(
            durable_transcript(Path::new("/s.sqlite"), &records).unwrap(),
            [
                "assistant: a",
                "assistant: b",
                "tool: bash",
                "assistant: done",
                "tool: read"
            ]
        );
    }

    #[test]
    fn a_durable_record_that_is_not_an_assistant_entry_is_malformed() {
        for bad in [
            r#"{"kind":"pi.assistant","id":1,"conversationId":1}"#.to_owned(),
            "not json".to_owned(),
        ] {
            let err = durable_transcript(Path::new("/s.sqlite"), std::slice::from_ref(&bad))
                .expect_err(&bad);
            assert_eq!(err.code(), ErrorCode::SessionMalformed, "{bad}");
        }
    }
}
