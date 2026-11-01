//! `session retitle` (commands.md §10.14; contract §2.2 Session `title`, §3.7
//! E10; hs-d257, hs-n95, hs-d543): a Lead's or a Manager's **dynamic title**, a glance
//! reminder of what was last happening in its last 2 turns, at most the chat slot's
//! width less 8 characters, written by a one-shot pi.
//!
//! The run holds `locks/title-<session>.lock`, a `flock(2)` lock the kernel
//! frees when the run dies, so a Session never has two runs and a crash leaves
//! no stale lock.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::archive::archived;
use super::row::{self, Role, Row};
use super::state::scoped;
use super::{Env, RetitleArgs, not_a_lead};
use crate::args::Title;
use crate::config::{Letter, Tier, load_catalog};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::home::CpdHome;
use crate::host::deadline::Failed;
use crate::host::runner::{Call, Runner};
use crate::ids::SessionId;
use crate::output::Output;
use crate::ports::Ports;

/// The turns and characters of the conversation pi reads (step 2).
const TURNS: usize = 2;
const CHARACTERS: usize = 8000;

/// What the chat slot's width is cut by to make the limit (step 3), and the longest limit: a Title's maximum.
const MARGIN: u32 = 8;
const TITLE_MAX: u32 = 200;

/// The marker of the answer's one line (step 4).
const MARKER: &str = "TITLE:";

/// How many lines of pi's stderr a refusal carries.
const TAIL_LINES: usize = 20;

/// The prompt (step 3): the conversation text comes before it, on stdin (pi `cli.md`: piped stdin
/// is prepended to the first prompt).
fn prompt(limit: usize) -> String {
    format!(
        "The text above is the last two turns of a conversation between a user and an AI agent. \
Write a short title that reminds what was last happening in it, not what it started with. \
Answer with one line, `TITLE: <what was last happening, at most {limit} characters>`, and nothing else: plain text, no quotes, no sentences."
    )
}

/// The longest title for a chat slot `width` columns wide: the width less [`MARGIN`], at most [`TITLE_MAX`].
/// The command's `--width` is at least `MARGIN + 1`, so the limit is at least 1.
fn limit(width: u32) -> usize {
    (width - MARGIN).min(TITLE_MAX) as usize
}

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
    /// hs-d356: no single `TITLE:` line, or one that does not pass step 4.
    Malformed,
}

impl Reason {
    fn as_str(&self) -> &'static str {
        match self {
            Reason::Running => "running",
            Reason::Spawn => "spawn",
            Reason::Exit => "exit",
            Reason::Empty => "empty",
            Reason::Malformed => "malformed",
        }
    }
}

fn title_failed(session: &SessionId, reason: Reason, why: impl std::fmt::Display) -> CpdError {
    CpdError::new(
        ErrorCode::TitleFailed,
        format!("The title of Session {session} was not written: {why}."),
    )
    .with("session", session.as_str())
    .with("reason", reason.as_str())
}

fn io_failed(what: &str, path: &Path, e: &io::Error) -> CpdError {
    CpdError::new(
        ErrorCode::FileUnreadable,
        format!("Cannot {what} {}: {e}.", path.display()),
    )
    .with("path", path.display().to_string())
}

/// Step 1: the lock, without waiting; `title_failed` with reason `running` when another run holds it.
fn take_lock(home: &CpdHome, session: &SessionId) -> Result<File, CpdError> {
    let path = lock_path(home, session);
    let failed = |e: &dyn std::fmt::Display| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("Cannot use the title lock {}: {e}.", path.display()),
        )
        .with("lock", path.display().to_string())
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| failed(&e))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| failed(&e))?;
    match crate::lock::try_lock(&file) {
        Ok(true) => Ok(file),
        Ok(false) => {
            Err(
                title_failed(session, Reason::Running, "another retitle of it is running")
                    .with("lock", path.display().to_string()),
            )
        }
        Err(e) => Err(failed(&e)),
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
        return Ok(Output::success(
            json!({"session": id, "title": null, "skipped": "no_messages"}),
        ));
    };
    let title = run_pi(&mut env, &read, &text, limit(args.width))?;
    let (event, titled_at) = env.store.write("session retitle", |tx| {
        // The Session may have been archived since it was read.
        check(&row::load(tx, &id)?)?;
        tx.execute(
            "UPDATE sessions SET title = ?1, titled_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?2",
            [title.as_str(), id.as_str()],
        )?;
        let titled_at: String = tx.query_row("SELECT at FROM txns WHERE id = cpd_txn()", [], |r| r.get(0))?;
        let text = format!("Session {id} titled {:?}.", title.as_str());
        let event = append(tx, scoped(NewEvent::session_titled(text, title.as_str()), &read))?;
        Ok((event, titled_at))
    })?;
    let pane = set_header(&env, &id, &title);
    Ok(Output::success(
        json!({"session": id, "title": title.as_str(), "titled_at": titled_at, "event": event, "pane": pane}),
    ))
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
        let Some(pane) = panes
            .into_iter()
            .find(|p| !p.dead && p.cpd_session.as_ref() == Some(id))
        else {
            return Ok(None);
        };
        tmux.set_pane_title(&pane.pane, title.as_str())?;
        Ok(Some(pane.pane.as_str().to_owned()))
    });
    set.unwrap_or_else(|e| {
        eprintln!("cpd session retitle: the title is written, but the pane header of Session {id} was not set: {}", e.message());
        None
    })
}

/// Step 2: the text of the last [`TURNS`] turns of the session file (from the second-to-last `user` message
/// to the end; the whole file when it has fewer), its `user` and `assistant` messages in order,
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
    let users: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.starts_with("user: "))
        .map(|(i, _)| i)
        .collect();
    let first = users.len().checked_sub(TURNS).map_or(0, |i| users[i]);
    let text = messages[first..].join("\n\n");
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
    let role = message["role"]
        .as_str()
        .filter(|r| matches!(*r, "user" | "assistant"))?;
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

/// Steps 3 and 4: pi on Tier `GX` (the G model, no effort, hs-d562), the conversation on stdin, in `run/titles/`;
/// its answer sanitised. pi's stderr is passed on to this run's stderr (the engine's log).
fn run_pi(env: &mut Env, row: &Row, text: &str, limit: usize) -> Result<Title, CpdError> {
    let id = &row.id;
    let models = row::models_of(env, row)?;
    let catalog = load_catalog(&env.config, &models)?;
    let model = catalog.model_arg(Tier::new(Letter::G, 0).expect("GX"))?;
    let dir = run_dir(&env.home);
    fs::create_dir_all(&dir).map_err(|e| io_failed("create", &dir, &e))?;
    let environ: Vec<String> = std::env::vars_os()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    run_pi_with(
        &*Ports::current().runner,
        id,
        &model,
        &dir,
        text,
        limit,
        &environ,
    )
}

pub(crate) use crate::host::runner::NO_DEADLINE;

/// Steps 3 and 4 on `runner`: pi on `model` in `dir` (its stderr is the `Output`'s, passed on to this run's stderr), the conversation
/// `text` as stdin, under [`NO_DEADLINE`]. The variables of `environ` that start with `CPD_` are removed.
pub(crate) fn run_pi_with(
    runner: &dyn Runner,
    id: &SessionId,
    model: &str,
    dir: &Path,
    text: &str,
    limit: usize,
    environ: &[String],
) -> Result<Title, CpdError> {
    let mut envs: Vec<(String, Option<String>)> = environ
        .iter()
        .filter(|k| k.starts_with("CPD_"))
        .map(|k| (k.clone(), None))
        .collect();
    envs.push(("PI_SKIP_VERSION_CHECK".to_owned(), Some("1".to_owned())));
    let call = Call {
        tool: "pi".to_owned(),
        program: "pi".to_owned(),
        args: [
            "--print",
            "--no-session",
            "--no-extensions",
            "--model",
            model,
            "--",
            &prompt(limit),
        ]
        .map(str::to_owned)
        .to_vec(),
        envs,
        cwd: Some(dir.to_path_buf()),
        deadline: NO_DEADLINE,
        stdin: Some(text.as_bytes().to_vec()),
        stderr_to: None,
    };
    let out = runner.run(&call).map_err(|e| match e {
        Failed::Io(e) => title_failed(id, Reason::Spawn, format_args!("pi could not start: {e}")),
        Failed::Timeout(e) => e,
    })?;
    let _ = io::stderr().write_all(&out.stderr);
    let tail = tail(&out.stderr);
    if !out.status.success() {
        let how = out.status.code().map_or_else(
            || format!("was ended by {}", out.status),
            |c| format!("exited with status {c}"),
        );
        return Err(title_failed(id, Reason::Exit, format_args!("pi {how}"))
            .with("exit", out.status.code())
            .with("tail", tail));
    }
    sanitise(&String::from_utf8_lossy(&out.stdout), limit)
        .map_err(|(reason, why)| title_failed(id, reason, why).with("tail", tail))
}

/// Step 4: the title is the text after the marker of the only line that starts with `TITLE:` (leading
/// spaces allowed), trimmed and unquoted; any other line is ignored. An answer that is blank is `empty`;
/// one with no marked line or two, or whose title is empty, over `limit` characters,
/// reads as sentences or is not a [`Title`] is `malformed`. Nothing is cut.
fn sanitise(answer: &str, limit: usize) -> Result<Title, (Reason, String)> {
    const QUOTES: &[char] = &['"', '\'', '`', '“', '”', '‘', '’', '«', '»'];
    if answer.trim().is_empty() {
        return Err((Reason::Empty, "pi's answer is empty".to_owned()));
    }
    let malformed = |why: String| (Reason::Malformed, why);
    let mut marked = answer
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix(MARKER));
    let (Some(text), None) = (marked.next(), marked.next()) else {
        return Err(malformed(format!(
            "pi's answer has no single line starting with {MARKER}"
        )));
    };
    let text = text.trim().trim_matches(QUOTES).trim();
    let bad = if text.is_empty() {
        Some("it is empty".to_owned())
    } else if text.chars().count() > limit {
        Some(format!("it is over {limit} characters"))
    } else if text.contains(". ") || text.ends_with(['.', '…', ':']) {
        Some("it reads as sentences".to_owned())
    } else {
        None
    };
    if let Some(bad) = bad {
        return Err(malformed(format!(
            "pi's answer {text:?} is not a title: {bad}"
        )));
    }
    text.parse::<Title>().map_err(|e| {
        malformed(format!(
            "pi's answer {text:?} is not a title: {}",
            e.message()
        ))
    })
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
        sanitise(answer, LIMIT)
            .map(|t| t.as_str().to_owned())
            .map_err(|(reason, _)| reason)
    }

    /// The limit the unit tests use: a chat slot of 68 columns.
    const LIMIT: usize = 60;

    /// The answer pi gave E10's retry of Session 4965bfc8 at 15:05:37Z: a preamble, no title (hs-d356).
    const PREAMBLE: &str = "Now I understand. I need to output just the title to stdout…";

    #[test]
    fn the_empty_answer_and_a_blank_one_are_empty() {
        for empty in ["", " ", "\n", " \n\n  \n"] {
            assert_eq!(title(empty), Err(Reason::Empty), "{empty:?}");
        }
    }

    #[test]
    fn the_real_preamble_has_no_marker_and_is_malformed_never_a_title() {
        assert_eq!(title(PREAMBLE), Err(Reason::Malformed));
        assert_eq!(title(&format!("{PREAMBLE}\n")), Err(Reason::Malformed));
    }

    #[test]
    fn a_unmarked_line_is_malformed_even_when_it_would_pass_as_a_title() {
        for unmarked in [
            "Dynamic session titles",
            "\"Plan the titles\"\nmore\n",
            "Title: Dynamic session titles",
            "title: x y z",
        ] {
            assert_eq!(title(unmarked), Err(Reason::Malformed), "{unmarked:?}");
        }
    }

    #[test]
    fn the_marked_line_is_the_title() {
        assert_eq!(
            title("TITLE: Dynamic session titles"),
            Ok("Dynamic session titles".to_owned())
        );
        assert_eq!(
            title("TITLE: Dynamic session titles\n"),
            Ok("Dynamic session titles".to_owned())
        );
        assert_eq!(
            title("  TITLE:   Dynamic session titles  \n"),
            Ok("Dynamic session titles".to_owned())
        );
    }

    #[test]
    fn lines_without_the_marker_are_ignored_so_a_preamble_does_no_harm() {
        assert_eq!(
            title(&format!("{PREAMBLE}\nTITLE: Dynamic session titles")),
            Ok("Dynamic session titles".to_owned())
        );
        assert_eq!(
            title(&format!(
                "{PREAMBLE}\n\nTITLE: Dynamic session titles\nThat is all.\n"
            )),
            Ok("Dynamic session titles".to_owned())
        );
    }

    #[test]
    fn the_marked_text_is_unquoted() {
        assert_eq!(
            title("TITLE: \"Plan the titles\""),
            Ok("Plan the titles".to_owned())
        );
        assert_eq!(
            title("TITLE: “Curly quotes”"),
            Ok("Curly quotes".to_owned())
        );
        assert_eq!(
            title("TITLE: 'Single quotes'"),
            Ok("Single quotes".to_owned())
        );
    }

    #[test]
    fn two_marked_lines_are_malformed() {
        assert_eq!(
            title("TITLE: First title\nTITLE: Second title"),
            Err(Reason::Malformed)
        );
        assert_eq!(
            title("TITLE: Same title\nsome words\n  TITLE: Same title"),
            Err(Reason::Malformed)
        );
    }

    #[test]
    fn an_empty_marked_text_is_malformed() {
        for empty in ["TITLE:", "TITLE:   ", "TITLE: \"\"", "preamble\nTITLE:\n"] {
            assert_eq!(title(empty), Err(Reason::Malformed), "{empty:?}");
        }
    }

    #[test]
    fn a_title_over_the_limit_is_malformed_and_never_cut() {
        let at_limit = "é".repeat(LIMIT);
        assert_eq!(title(&format!("TITLE: {at_limit}")), Ok(at_limit));
        assert_eq!(
            title(&format!("TITLE: {}", "é".repeat(LIMIT + 1))),
            Err(Reason::Malformed)
        );
        assert_eq!(
            title(&format!("TITLE: {}", "é".repeat(300))),
            Err(Reason::Malformed)
        );
    }

    #[test]
    fn the_limit_is_the_width_less_8_at_most_200() {
        assert_eq!(limit(9), 1);
        assert_eq!(limit(68), 60);
        assert_eq!(limit(208), 200);
        assert_eq!(limit(500), 200);
    }

    #[test]
    fn a_title_has_no_word_cap() {
        assert_eq!(
            title("TITLE: a b c d e f g h i j k l"),
            Ok("a b c d e f g h i j k l".to_owned())
        );
    }

    #[test]
    fn the_prompt_names_the_limit() {
        assert!(prompt(115).contains("at most 115 characters"));
    }

    #[test]
    fn a_title_that_reads_as_sentences_is_malformed() {
        for sentences in [
            "TITLE: Fixes the titles.",
            "TITLE: Fixes the titles…",
            "TITLE: The titles:",
            "TITLE: Fixes it. Then more",
            "TITLE: I need to output just the title.",
        ] {
            assert_eq!(title(sentences), Err(Reason::Malformed), "{sentences:?}");
        }
        assert_eq!(
            title("TITLE: Fix v1.2 titles"),
            Ok("Fix v1.2 titles".to_owned()),
            "a dot that is not `. ` or the end is fine"
        );
    }

    #[test]
    fn a_control_character_in_the_marked_text_is_no_title() {
        assert!(title("TITLE: Tab\tinside").is_err());
    }

    #[test]
    fn the_conversation_is_the_last_2_turns_with_text_newest_characters_kept() {
        let line = |role: &str, text: &str| {
            json!({"type": "message", "message": {"role": role, "content": text}}).to_string()
        };
        let mut lines: Vec<String> = (1..=35)
            .map(|i| {
                line(
                    if i % 2 == 0 { "assistant" } else { "user" },
                    &format!("m{i}"),
                )
            })
            .collect();
        lines.push(line("toolResult", "out"));
        lines.push(line("assistant", "   "));
        lines.push("{\"type\":\"mess".to_owned());
        let text = conversation_text(&lines.join("\n")).unwrap();
        assert_eq!(text, "user: m33\n\nassistant: m34\n\nuser: m35");
        let long = line("user", &"x".repeat(9000));
        assert_eq!(conversation_text(&long).unwrap(), "x".repeat(CHARACTERS));
        assert_eq!(conversation_text(&line("toolResult", "only")), None);
    }

    #[test]
    fn fewer_than_2_user_messages_is_the_whole_file() {
        let line = |role: &str, text: &str| {
            json!({"type": "message", "message": {"role": role, "content": text}}).to_string()
        };
        let lines = [
            line("assistant", "hello"),
            line("user", "q"),
            line("assistant", "a"),
        ];
        assert_eq!(
            conversation_text(&lines.join("\n")).unwrap(),
            "assistant: hello\n\nuser: q\n\nassistant: a"
        );
    }
}
