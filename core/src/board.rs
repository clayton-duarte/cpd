//! `cpd board post | list | take` (commands.md §10a, hs-d514): the message board. A post names who it is for; it appends no
//! Event and never starts a turn. The adapter takes a Session's unread posts at its turn start.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use crate::args::SessionRef;
use crate::clock;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::ids::SessionId;
use crate::output::Output;
use crate::session;

const TEXT_MAX_CHARS: usize = 2000;

/// The `board` commands.
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Post a message to a Session (it never starts a turn)
    Post(PostArgs),
    /// List the posts to a Session
    List(ListArgs),
    /// Return and mark as read the unread posts to a Session (internal)
    #[command(hide = true)]
    Take(TakeArgs),
}

#[derive(clap::Args, Debug)]
pub struct PostArgs {
    /// The message (1–2000 characters)
    #[arg(value_name = "text")]
    pub text: String,
    /// The target: `manager`, a Plan id (its Lead) or a Session id
    #[arg(long, value_name = "ref")]
    pub to: String,
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// The target Session (default: $CPD_SESSION)
    #[arg(long, value_name = "id")]
    pub session: Option<SessionRef>,
    /// Include the read posts
    #[arg(long)]
    pub all: bool,
}

#[derive(clap::Args, Debug)]
pub struct TakeArgs {
    /// The target Session
    #[arg(long, value_name = "id")]
    pub session: SessionRef,
}

fn session_not_found(id: &str) -> CpdError {
    CpdError::new(ErrorCode::SessionNotFound, format!("No Session {id}.")).with("session", id)
}

/// The Session a post is for: `manager`, a Plan id (a live Plan's Lead) or a Session reference.
fn target(conn: &Connection, to: &str, from: Option<&SessionId>) -> Result<String, CpdError> {
    if to == "manager" {
        let found: Option<String> = match from {
            Some(from) => conn
                .query_row(
                    "SELECT m.id FROM sessions m JOIN sessions s ON s.context = m.context
                     WHERE s.id = ?1 AND m.role = 'manager' AND m.archived_at IS NULL ORDER BY m.created_at, m.id LIMIT 1",
                    [from.as_str()],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };
        return found.ok_or_else(|| {
            CpdError::new(
                ErrorCode::NoManager,
                "The sender's Context has no live Manager.",
            )
        });
    }
    let id = session::resolve(conn, &to.parse::<SessionRef>()?)?;
    let lead_plan: Option<String> = conn
        .query_row(
            "SELECT plan FROM sessions WHERE id = ?1 AND role = 'lead'",
            [id.as_str()],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if let Some(plan) = lead_plan {
        session::plan_inactive(conn, &plan)?;
    }
    Ok(id.to_string())
}

fn post(args: PostArgs) -> Result<Output, CpdError> {
    let chars = args.text.chars().count();
    if chars == 0 || chars > TEXT_MAX_CHARS || args.text.contains('\0') {
        return Err(CpdError::new(
            ErrorCode::Usage,
            format!("A post is 1 to {TEXT_MAX_CHARS} characters with no NUL."),
        )
        .with("chars", chars));
    }
    let from = session::sender()?;
    let mut env = Env::open()?;
    let (session, context, at) = env.store.write("board post", |tx| {
        let session = target(tx, &args.to, from.as_ref())?;
        let context: String = tx
            .query_row("SELECT context FROM sessions WHERE id = ?1", [&session], |r| r.get(0))
            .optional()?
            .ok_or_else(|| session_not_found(&session))?;
        let at = clock::now();
        tx.execute(
            "INSERT INTO board_posts (at, context, from_session, to_session, text) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![at, context, from.as_ref().map(SessionId::as_str), session, args.text],
        )?;
        Ok((session, context, at))
    })?;
    Ok(Output::success(
        json!({"session": session, "context": context, "posted_at": at}),
    ))
}

/// The Session `reference` names, which must exist.
fn existing(conn: &Connection, reference: &SessionRef) -> Result<String, CpdError> {
    Ok(session::resolve(conn, reference)?.to_string())
}

fn rows(conn: &Connection, session: &str, all: bool) -> Result<Vec<Value>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.at, p.from_session, p.text, p.read_at, s.name FROM board_posts p LEFT JOIN sessions s ON s.id = p.from_session WHERE p.to_session = ?1 AND (?2 OR p.read_at IS NULL) ORDER BY p.id",
    )?;
    let posts = stmt
        .query_map(rusqlite::params![session, all], |r| {
            Ok(json!({"id": r.get::<_, i64>(0)?, "at": r.get::<_, String>(1)?, "from_session": r.get::<_, Option<String>>(2)?,
                      "from_name": r.get::<_, Option<String>>(5)?,
                      "text": r.get::<_, String>(3)?, "read_at": r.get::<_, Option<String>>(4)?}))
        })?
        .collect::<rusqlite::Result<Vec<Value>>>()?;
    Ok(posts)
}

fn list(args: ListArgs) -> Result<Output, CpdError> {
    let reference = match args.session {
        Some(reference) => reference,
        None => match std::env::var("CPD_SESSION") {
            Ok(id) if !id.is_empty() => id.parse::<SessionRef>()?,
            _ => {
                return Err(CpdError::new(
                    ErrorCode::Usage,
                    "Name the Session with --session, or run inside one ($CPD_SESSION).",
                ));
            }
        },
    };
    let mut env = Env::open()?;
    let (session, posts) = env.store.read(|conn| {
        let session = existing(conn, &reference)?;
        let posts = rows(conn, &session, args.all)?;
        Ok((session, posts))
    })?;
    Ok(Output::success(json!({"session": session, "posts": posts})))
}

fn take(args: TakeArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let (session, taken) = env.store.write("board take", |tx| {
        let session = existing(tx, &args.session)?;
        let mut taken = rows(tx, &session, false)?;
        tx.execute(
            "UPDATE board_posts SET read_at = ?1 WHERE to_session = ?2 AND read_at IS NULL",
            rusqlite::params![clock::now(), session],
        )?;
        for post in &mut taken {
            post.as_object_mut()
                .expect("a post is an object")
                .remove("read_at");
        }
        Ok((session, taken))
    })?;
    Ok(Output::success(json!({"session": session, "taken": taken})))
}

/// Runs a `board` command.
pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Post(a) => post(a),
        Command::List(a) => list(a),
        Command::Take(a) => take(a),
    }
}
