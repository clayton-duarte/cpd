//! `session retitle` (commands.md §10.14; contract §2.2 Session `title`, §3.7
//! E10; hs-d257, hs-n95): a Lead's or a Manager's **dynamic title**, a 3–7 word
//! title of what its conversation is about now, written by a one-shot pi.
//!
//! The run holds `locks/title-<session>.lock`, a `flock(2)` lock the kernel
//! frees when the run dies, so a Session never has two runs and a crash leaves
//! no stale lock.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use super::archive::archived;
use super::row::{self, Role, Row};
use super::state::scoped;
use super::{Env, RetitleArgs, not_a_lead};
use crate::args::Title;
use crate::config::{Letter, Tier, load_catalog};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::home::CpdHome;
use crate::ids::SessionId;
use crate::output::Output;

/// The messages and characters of the conversation pi reads (step 2).
const MESSAGES: usize = 30;
const CHARACTERS: usize = 8000;

/// The longest title (contract §2.2): a longer answer is cut to this, ending with `…`.
const TITLE_CHARS: usize = 60;

/// How many lines of pi's stderr a refusal carries.
const TAIL_LINES: usize = 20;

/// The prompt (step 3): the conversation text comes before it, on stdin (pi `cli.md`: piped stdin
/// is prepended to the first prompt).
const PROMPT: &str = "The text above is a conversation between a user and an AI agent. \
Write a title of 3 to 7 words for the conversation's current subject: what it is about now, not what it started with. \
Answer with the title only: plain text, one line, no quotes.";

/// `locks/title-<session>.lock`: held by the Session's one retitle run (contract §1.4).
pub(crate) fn lock_path(home: &CpdHome, session: &SessionId) -> PathBuf {
    home.locks_dir().join(format!("title-{session}.lock"))
}

/// `run/titles/`: pi's working directory (contract §1.4).
fn run_dir(home: &CpdHome) -> PathBuf {
    home.run_dir().join("titles")
}

/// Why a run wrote nothing (`details.reason` of `title_failed`).
#[derive(Debug, PartialEq, Eq)]
enum Reason {
    Running,
    Spawn,
    Exit,
    Empty,
    Invalid,
}

impl Reason {
    fn as_str(&self) -> &'static str {
        match self {
            Reason::Running => "running",
            Reason::Spawn => "spawn",
            Reason::Exit => "exit",
            Reason::Empty => "empty",
            Reason::Invalid => "invalid",
        }
    }
}

fn title_failed(session: &SessionId, reason: Reason, why: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::TitleFailed, format!("The title of Session {session} was not written: {why}."))
        .with("session", session.as_str())
        .with("reason", reason.as_str())
}

fn io_failed(what: &str, path: &Path, e: &io::Error) -> CpdError {
    CpdError::new(ErrorCode::FileUnreadable, format!("Cannot {what} {}: {e}.", path.display())).with("path", path.display().to_string())
}

/// Step 1: the lock, without waiting; `title_failed` with reason `running` when another run holds it.
fn take_lock(home: &CpdHome, session: &SessionId) -> Result<File, CpdError> {
    let path = lock_path(home, session);
    let failed = |e: &dyn std::fmt::Display| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot use the title lock {}: {e}.", path.display()))
            .with("lock", path.display().to_string())
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| failed(&e))?;
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).map_err(|e| failed(&e))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => {
            Err(title_failed(session, Reason::Running, "another retitle of it is running").with("lock", path.display().to_string()))
        }
        Err(TryLockError::Error(e)) => Err(failed(&e)),
    }
}

/// `session retitle` (commands.md §10.14).
pub(super) fn retitle(args: RetitleArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let id = env.resolve(&args.id)?;
    let read = env.store.read(|conn| row::load(conn, &id))?;
    check(&read)?;
    let _lock = take_lock(&env.home, &id)?;
    let Some(text) = conversation(read.file.as_deref())? else {
        return Ok(Output::success(json!({"session": id, "title": null, "skipped": "no_messages"})));
    };
    let title = run_pi(&env, &id, &text)?;
    let (event, titled_at) = env.store.write("session retitle", |tx| {
        // The Session may have been archived since it was read.
        check(&row::load(tx, &id)?)?;
        tx.execute(
            "UPDATE sessions SET title = ?1, titled_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?2",
            [title.as_str(), id.as_str()],
        )?;
        let titled_at: String = tx.query_row("SELECT at FROM txns WHERE id = cpd_txn()", [], |r| r.get(0))?;
        let text = format!("Session {id} titled {:?}.", title.as_str());
        let event = append(tx, scoped(NewEvent::new(Kind::SessionTitled, text, json!({"title": title.as_str()})), &read))?;
        Ok((event, titled_at))
    })?;
    let pane = set_header(&env, &id, &title);
    Ok(Output::success(json!({"session": id, "title": title.as_str(), "titled_at": titled_at, "event": event, "pane": pane})))
}

/// The preconditions on the row: a Lead or a Manager, not archived.
fn check(row: &Row) -> Result<(), CpdError> {
    match row.role {
        Role::Lead | Role::Manager => {}
        role => return Err(not_a_lead(&row.id, role)),
    }
    match row.archived_at {
        Some(_) => Err(archived(row)),
        None => Ok(()),
    }
}

/// Step 5's tmux part: `@cpd_title` on the Session's live pane, if it has one. A failure is
/// reported on stderr and does not undo the write. Returns the pane set.
fn set_header(env: &Env, id: &SessionId, title: &Title) -> Option<String> {
    let tmux = env.tmux();
    let set = tmux.list_panes().and_then(|panes| {
        let Some(pane) = panes.into_iter().find(|p| !p.dead && p.cpd_session.as_ref() == Some(id)) else { return Ok(None) };
        tmux.set_pane_title(&pane.pane, title.as_str())?;
        Ok(Some(pane.pane.as_str().to_owned()))
    });
    set.unwrap_or_else(|e| {
        eprintln!("cpd session retitle: the title is written, but the pane header of Session {id} was not set: {}", e.message());
        None
    })
}

/// Step 2: the text of the last [`MESSAGES`] `user` and `assistant` messages of the session file,
/// at most [`CHARACTERS`] characters, the newest kept. None when there is no such text (no file
/// yet, or none in it). Lines that are not a message (a partial last line included) are skipped.
fn conversation(file: Option<&str>) -> Result<Option<String>, CpdError> {
    let Some(file) = file else { return Ok(None) };
    let path = Path::new(file);
    let content = match fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_failed("read", path, &e)),
    };
    Ok(conversation_text(&content))
}

fn conversation_text(content: &str) -> Option<String> {
    let messages: Vec<String> = content.lines().filter_map(message_text).collect();
    if messages.is_empty() {
        return None;
    }
    let text = messages[messages.len().saturating_sub(MESSAGES)..].join("\n\n");
    let skip = text.chars().count().saturating_sub(CHARACTERS);
    Some(text.chars().skip(skip).collect())
}

/// `<role>: <text>` for a `message` entry of role `user` or `assistant` with text; its text is
/// its string content or its `text` blocks, joined.
fn message_text(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value["type"] != "message" {
        return None;
    }
    let message = &value["message"];
    let role = message["role"].as_str().filter(|r| matches!(*r, "user" | "assistant"))?;
    let text = match &message["content"] {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| format!("{role}: {text}"))
}

/// Steps 3 and 4: pi on the lowest effort of Tier `H`, the conversation on stdin, in `run/titles/`;
/// its answer sanitised. pi's stderr is passed on to this run's stderr (the engine's log).
fn run_pi(env: &Env, id: &SessionId, text: &str) -> Result<Title, CpdError> {
    let catalog = load_catalog(&env.config)?;
    let effort = catalog.h.efforts.first().copied().ok_or_else(|| {
        CpdError::new(ErrorCode::CatalogFailed, format!("pi's catalog lists no effort for {}.", catalog.h.model))
            .with("model", catalog.h.model.clone())
    })?;
    let model = catalog.model_arg(Tier::new(Letter::H, effort).expect("a catalog effort is 0–5"))?;
    let dir = run_dir(&env.home);
    fs::create_dir_all(&dir).map_err(|e| io_failed("create", &dir, &e))?;
    let mut command = Command::new("pi");
    command.args(["--print", "--no-session", "--no-extensions", "--model", &model, "--", PROMPT]);
    for (key, _) in std::env::vars_os().filter(|(k, _)| k.to_string_lossy().starts_with("CPD_")) {
        command.env_remove(key);
    }
    command.env("PI_SKIP_VERSION_CHECK", "1").current_dir(&dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| title_failed(id, Reason::Spawn, format_args!("pi could not start: {e}")))?;
    // At most 8 000 characters, under a pipe's buffer: the write never waits for pi to read.
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let written = stdin.write_all(text.as_bytes());
    drop(stdin);
    let out = child.wait_with_output().map_err(|e| title_failed(id, Reason::Spawn, format_args!("pi could not be waited for: {e}")))?;
    let _ = io::stderr().write_all(&out.stderr);
    let tail = tail(&out.stderr);
    if !out.status.success() {
        let how = out.status.code().map_or_else(|| format!("was ended by {}", out.status), |c| format!("exited with status {c}"));
        return Err(title_failed(id, Reason::Exit, format_args!("pi {how}")).with("exit", out.status.code()).with("tail", tail));
    }
    if let Err(e) = written {
        return Err(title_failed(id, Reason::Spawn, format_args!("pi did not read the conversation: {e}")).with("tail", tail));
    }
    sanitise(&String::from_utf8_lossy(&out.stdout)).map_err(|(reason, why)| title_failed(id, reason, why).with("tail", tail))
}

/// Step 4: the first line, trimmed, its surrounding quotes stripped, cut to [`TITLE_CHARS`]
/// characters with `…`; it must then be a [`Title`].
fn sanitise(answer: &str) -> Result<Title, (Reason, String)> {
    const QUOTES: &[char] = &['"', '\'', '`', '“', '”', '‘', '’', '«', '»'];
    let line = answer.trim().lines().next().unwrap_or_default();
    let line = line.trim().trim_matches(QUOTES).trim();
    if line.is_empty() {
        return Err((Reason::Empty, "pi's answer is empty".to_owned()));
    }
    let title = if line.chars().count() > TITLE_CHARS {
        let kept: String = line.chars().take(TITLE_CHARS - 1).collect();
        format!("{}…", kept.trim_end())
    } else {
        line.to_owned()
    };
    title.parse::<Title>().map_err(|e| (Reason::Invalid, format!("pi's answer {title:?} is not a title: {}", e.message())))
}

/// The last [`TAIL_LINES`] lines of `bytes`.
fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(answer: &str) -> Result<String, Reason> {
        sanitise(answer).map(|t| t.as_str().to_owned()).map_err(|(reason, _)| reason)
    }

    #[test]
    fn sanitise_keeps_the_first_line_trimmed_and_unquoted() {
        assert_eq!(title("\"Plan the titles\"\nmore\n"), Ok("Plan the titles".to_owned()));
        assert_eq!(title("\n  “Curly”  \n"), Ok("Curly".to_owned()));
        assert_eq!(title(" `x` "), Ok("x".to_owned()));
    }

    #[test]
    fn sanitise_cuts_at_60_characters_with_an_ellipsis() {
        let cut = title(&"é".repeat(61)).unwrap();
        assert_eq!((cut.chars().count(), cut.ends_with('…')), (60, true));
        assert_eq!(title(&"é".repeat(60)).unwrap(), "é".repeat(60));
    }

    #[test]
    fn sanitise_refuses_an_empty_answer_and_a_control_character() {
        for empty in ["", " \n\n", "\"\"", "''\nsecond"] {
            assert_eq!(title(empty), Err(Reason::Empty), "{empty:?}");
        }
        assert_eq!(title("a\tb"), Err(Reason::Invalid));
    }

    #[test]
    fn the_conversation_is_the_last_30_messages_with_text_newest_characters_kept() {
        let line = |role: &str, text: &str| json!({"type": "message", "message": {"role": role, "content": text}}).to_string();
        let mut lines: Vec<String> = (1..=35).map(|i| line(if i % 2 == 0 { "assistant" } else { "user" }, &format!("m{i}"))).collect();
        lines.push(line("toolResult", "out"));
        lines.push(line("assistant", "   "));
        lines.push("{\"type\":\"mess".to_owned());
        let text = conversation_text(&lines.join("\n")).unwrap();
        assert!(text.starts_with("assistant: m6\n\nuser: m7") && text.ends_with("user: m35"), "{text}");
        let long = line("user", &"x".repeat(9000));
        assert_eq!(conversation_text(&long).unwrap(), "x".repeat(CHARACTERS));
        assert_eq!(conversation_text(&line("toolResult", "only")), None);
    }
}
