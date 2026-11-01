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
    let missing = |why: &str| {
        CpdError::new(
            ErrorCode::SessionNotFound,
            format!("Attempt {id} has no Worker session file: {why}."),
        )
        .with("attempt", id.to_string())
        .with("session", session.clone())
        .with("file", file.clone())
    };
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
        match message.content {
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
}
