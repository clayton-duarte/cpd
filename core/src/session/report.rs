//! `session report` (commands.md §10.18; hs-d573 M3/M4): an adapter error for the Manager, or (with
//! `--kind protocol-breach`) the Ledger leg of a guard's refusal (contract §5.2, D14, air-d119). The
//! `error` leg appends one `error` Event with no Plan and no Session, so no Lead's follower receives
//! it back; the breach leg appends one `protocol-breach` Event that names the refused Session's own
//! Context, Plan and id.

use std::str::FromStr;
use std::time::Duration;

use rusqlite::{OptionalExtension, params};
use serde_json::{Map, Value, json};

use super::row;
use super::{Env, ReportArgs};
use crate::args::SessionArg;
use crate::error::{CpdError, ErrorCode};
use crate::events::repeat::{self, Same, Scope};
use crate::events::{NewEvent, append};
use crate::ids::SessionId;
use crate::output::Output;
use crate::store::WriteTxn;

const TEXT_MAX: usize = 2000;
const CODE_MAX: usize = 64;
const COMMAND_MAX: usize = 200;

fn usage(message: String) -> CpdError {
    CpdError::new(ErrorCode::Usage, message)
}

/// 1 to 2000 characters (`--what`, `--message`).
#[derive(Clone, Debug)]
pub struct Short(pub(super) String);

impl FromStr for Short {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Short, CpdError> {
        let chars = s.chars().count();
        if chars == 0 || chars > TEXT_MAX {
            return Err(
                usage(format!("The text must be 1 to {TEXT_MAX} characters.")).with("chars", chars),
            );
        }
        Ok(Short(s.to_owned()))
    }
}

/// `^[a-z][a-z0-9_]{0,63}$`.
#[derive(Clone, Debug)]
pub struct Code(pub(super) String);

impl FromStr for Code {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Code, CpdError> {
        let mut bytes = s.bytes();
        let ok = s.len() <= CODE_MAX
            && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
            && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !ok {
            return Err(
                usage("The code must match ^[a-z][a-z0-9_]{0,63}$.".to_owned()).with("code", s),
            );
        }
        Ok(Code(s.to_owned()))
    }
}

/// A JSON object (`--details`).
#[derive(Clone, Debug)]
pub struct Details(Map<String, Value>);

impl FromStr for Details {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Details, CpdError> {
        match serde_json::from_str::<Value>(s) {
            Ok(Value::Object(map)) => Ok(Details(map)),
            _ => Err(usage("--details must be a JSON object.".to_owned())),
        }
    }
}

/// `--kind protocol-breach` only (commands.md §10.18). Any other value is `usage`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind(pub(super) ());

impl FromStr for Kind {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Kind, CpdError> {
        if s == "protocol-breach" {
            Ok(Kind(()))
        } else {
            Err(usage("--kind is protocol-breach, nothing else.".to_owned()).with("kind", s))
        }
    }
}

/// The guarded protocol names (contract §5.2): `bug report`, `primary checkout`, `secrets`,
/// `external approval`, `questions stay short`.
#[derive(Clone, Debug)]
pub struct Protocol(pub(super) String);

const PROTOCOLS: [&str; 5] = [
    "bug report",
    "primary checkout",
    "secrets",
    "external approval",
    "questions stay short",
];

impl FromStr for Protocol {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Protocol, CpdError> {
        if PROTOCOLS.contains(&s) {
            Ok(Protocol(s.to_owned()))
        } else {
            Err(usage(format!("{s:?} is not a guarded protocol.")).with("protocol", s))
        }
    }
}

/// The emitting guard: `mainguard`, `secretguard`, `externalguard` or `askguard` (commands.md §10.18).
#[derive(Clone, Debug)]
pub struct Source(pub(super) String);

const SOURCES: [&str; 4] = ["mainguard", "secretguard", "externalguard", "askguard"];

impl FromStr for Source {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Source, CpdError> {
        if SOURCES.contains(&s) {
            Ok(Source(s.to_owned()))
        } else {
            Err(usage(format!("{s:?} is not a guard.")).with("source", s))
        }
    }
}

/// The refused tool's name: `bash`, `edit`, `write` or `ask_user_question` (commands.md §10.18).
#[derive(Clone, Debug)]
pub struct Tool(pub(super) String);

const TOOLS: [&str; 4] = ["bash", "edit", "write", "ask_user_question"];

impl FromStr for Tool {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Tool, CpdError> {
        if TOOLS.contains(&s) {
            Ok(Tool(s.to_owned()))
        } else {
            Err(usage(format!("{s:?} is not a refused tool.")).with("tool", s))
        }
    }
}

/// The refused call's first line, 1 to 200 characters (`--command`).
#[derive(Clone, Debug)]
pub struct Command(pub(super) String);

impl FromStr for Command {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Command, CpdError> {
        let chars = s.chars().count();
        if chars == 0 || chars > COMMAND_MAX {
            return Err(usage(format!(
                "The command must be 1 to {COMMAND_MAX} characters."
            ))
            .with("chars", chars));
        }
        Ok(Command(s.to_owned()))
    }
}

pub(super) fn report(args: ReportArgs) -> Result<Output, CpdError> {
    let reference = SessionArg {
        session: args.session,
    }
    .require()?
    .clone();
    let mut env = Env::open()?;
    let id = env.resolve(&reference)?;
    let ReportArgs {
        what,
        code,
        message,
        details,
        kind,
        protocol,
        source,
        tool,
        command,
        ..
    } = args;
    match kind {
        Some(_) => breach(&mut env, &id, message, protocol, source, tool, command),
        None => error(&mut env, &id, what, code, message, details),
    }
}

/// The `error` leg (commands.md §10.18): one `error` Event with no Plan and no Session.
#[allow(clippy::too_many_arguments)]
fn error(
    env: &mut Env,
    id: &SessionId,
    what: Option<Short>,
    code: Option<Code>,
    message: Option<Short>,
    details: Option<Details>,
) -> Result<Output, CpdError> {
    let what = what.ok_or_else(|| usage("--what is required without --kind.".to_owned()))?;
    let code = code.ok_or_else(|| usage("--code is required without --kind.".to_owned()))?;
    let message = message.ok_or_else(|| usage("--message is required.".to_owned()))?;
    let window = env.config.watch.error_repeat();
    let event = env.store.write("session report", |tx| {
        let session = row::load(tx, id)?;
        let mut details = details.map_or_else(Map::new, |d| d.0);
        details.insert("what".into(), json!(what.0));
        details.insert("session".into(), json!(session.id));
        details.insert("plan".into(), json!(session.plan));
        let text = format!("φ adapter: {}: {}: {}", what.0, code.0, message.0);
        let event = NewEvent::error(text, "adapter", &code.0, &message.0, Value::Object(details))
            .context(session.context.clone())
            .session(session.id.clone());
        let same = Same::Message {
            source: "adapter",
            code: &code.0,
            message: &message.0,
            scope: Scope::Session(&session.id),
        };
        repeat::append_once(tx, window, &same, event)
    })?;
    Ok(Output::success(json!({"event": event})))
}

/// The breach leg (contract §5.2, D14, air-d119; commands.md §10.18): one `protocol-breach` Event,
/// `context`/`plan`/`session` the reporting Session's own, `role`/`model`/`tier` read from its row.
#[allow(clippy::too_many_arguments)]
fn breach(
    env: &mut Env,
    id: &SessionId,
    message: Option<Short>,
    protocol: Option<Protocol>,
    source: Option<Source>,
    tool: Option<Tool>,
    command: Option<Command>,
) -> Result<Output, CpdError> {
    let message = message.ok_or_else(|| usage("--message is required with --kind.".to_owned()))?;
    let protocol =
        protocol.ok_or_else(|| usage("--protocol is required with --kind.".to_owned()))?;
    let source = source.ok_or_else(|| usage("--source is required with --kind.".to_owned()))?;
    let tool = tool.ok_or_else(|| usage("--tool is required with --kind.".to_owned()))?;
    let session = env.store.read(|conn| row::load(conn, id))?;
    let models = row::models_of(env, &session)?;
    let (model, _) = row::model_and_level(&models, &session.id, &session.tier)?;
    let window = env.config.watch.error_repeat();
    let command = command.map(|c| c.0);
    let event = env.store.write("session report", |tx| {
        let text = message.0.clone();
        let mut event = NewEvent::protocol_breach(
            text,
            &protocol.0,
            &source.0,
            session.role.as_str(),
            &model,
            &session.tier,
            Some(&tool.0),
            command.as_deref(),
        )
        .context(session.context.clone())
        .session(session.id.clone());
        if let Some(plan) = session.plan.clone() {
            event = event.plan(plan);
        }
        // The window compares protocol, tool and command (§10.18), scoped to the reporting Session.
        if let Some(earlier) = recent_breach(
            tx,
            window,
            &protocol.0,
            &tool.0,
            command.as_deref(),
            &session.id,
        )? {
            return Ok(earlier);
        }
        append(tx, event)
    })?;
    Ok(Output::success(json!({"event": event})))
}

/// The newest identical `protocol-breach` Event appended less than `window` ago: same `protocol`,
/// `tool` and `command`, in the same Session (commands.md §10.18).
fn recent_breach(
    tx: &WriteTxn<'_>,
    window: Duration,
    protocol: &str,
    tool: &str,
    command: Option<&str>,
    session: &SessionId,
) -> Result<Option<i64>, CpdError> {
    if window.is_zero() {
        return Ok(None);
    }
    let window = format!("-{} seconds", window.as_secs_f64());
    Ok(tx
        .query_row(
            "SELECT id FROM events
              WHERE kind = 'protocol-breach'
                AND json_extract(data, '$.protocol') = ?1
                AND json_extract(data, '$.tool') = ?2
                AND json_extract(data, '$.command') IS ?3
                AND session = ?4
                AND at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?5)
              ORDER BY id DESC LIMIT 1",
            params![protocol, tool, command, session.to_string(), window],
            |r| r.get(0),
        )
        .optional()?)
}
