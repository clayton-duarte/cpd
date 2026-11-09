//! `session report` (commands.md §10.18; hs-d573 M3/M4): an adapter error for the Manager, or a refusal of the
//! reporting Session's own tool call or command (contract §5.2, Plan `agent-refusals`), which appends the `guard`
//! kind instead of `error`.

use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use serde_json::{Map, Value, json};

use super::row;
use super::{Env, ReportArgs};
use crate::args::SessionArg;
use crate::error::{CpdError, ErrorCode};
use crate::events::repeat::{self, Same, Scope};
use crate::events::{NewEvent, append};
use crate::output::Output;
use crate::store::WriteTxn;

const TEXT_MAX: usize = 2000;
const CODE_MAX: usize = 64;

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

pub(super) fn report(args: ReportArgs) -> Result<Output, CpdError> {
    let reference = SessionArg {
        session: args.session,
    }
    .require()?
    .clone();
    let mut env = Env::open()?;
    let id = env.resolve(&reference)?;
    let window = env.config.watch.error_repeat();
    let ReportArgs {
        what,
        code,
        message,
        details,
        ..
    } = args;
    let mut details = details.map_or_else(Map::new, |d| d.0);

    // A loop-guard trip (contract §6.16 "The trip's cause and its report"): one `loop-guard-trip` Event, and for a
    // Lead the Context Manager's wake, in one transaction. A failure writes neither.
    if code.0 == "loop_guard_trip" {
        let kind = string_field(&details, "kind");
        let count = details
            .get("count")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                usage("A loop_guard_trip report carries details.count, a number.".to_owned())
            })?;
        let trip_text = loop_guard_trip_text(&kind, count)?;
        let event = env.store.write("session report", |tx| {
            let session = row::load(tx, &id)?;
            let text = format!("Loop guard of Session {} tripped: {trip_text}.", session.id);
            let trip = NewEvent::loop_guard_trip(text, &kind, count)?
                .context(session.context.clone())
                .session(session.id.clone());
            let trip = match session.plan.clone() {
                Some(plan) => trip.plan(plan),
                None => trip,
            };
            let event = append(tx, trip)?;
            if session.role.as_str() == "lead"
                && let Some(manager) = live_manager(tx, session.context.as_str(), session.id.as_str())?
            {
                let wake = format!(
                    "Session {0} tripped: {trip_text}. It waits for the User's message or for cpd session resume {0}.",
                    session.id
                );
                let message = NewEvent::session_message(wake.clone(), None::<String>, &wake, true, None)?;
                append(tx, message.session(crate::ids::SessionId::parse(&manager)?))?;
            }
            Ok(event)
        })?;
        return Ok(Output::success(json!({"event": event})));
    }

    // A refusal report carries the refusal rule in `details.guard`, with the refused call in
    // `details.{tool, excerpt}` (contract §5.2, Plan `agent-refusals`): the adapter's guard refusal, or an
    // agent-caused CLI refusal whose `guard` is the envelope's `code`. The Event is a `guard` Event.
    if let Some(rule) = details
        .get("guard")
        .and_then(Value::as_str)
        .map(str::to_owned)
    {
        let tool = string_field(&details, "tool");
        let excerpt = string_field(&details, "excerpt");
        let session = env.store.read(|c| row::load(c, &id))?;
        let model = row::model_and_level(
            &row::models_of(&mut env, &session)?,
            &session.id,
            &session.tier,
        )?
        .0;
        let role = session.role.as_str();
        let text = format!("{role} {} refused: {rule}: {excerpt}", session.id);
        let guard = NewEvent::guard(text, &rule, role, &session.tier, &model, &tool, &excerpt)
            .context(session.context.clone())
            .session(session.id.clone());
        let guard = match session.plan.clone() {
            Some(plan) => guard.plan(plan),
            None => guard,
        };
        let guard = match session
            .attempt
            .as_deref()
            .and_then(|a| crate::ids::AttemptId::parse(a).ok())
        {
            Some(attempt) => guard.attempt(attempt),
            None => guard,
        };
        let event = env.store.write("session report", |tx| {
            append_guard_once(tx, window, &rule, &excerpt, &session.id, guard)
        })?;
        return Ok(Output::success(json!({"event": event})));
    }

    let event = env.store.write("session report", |tx| {
        let session = row::load(tx, &id)?;
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

/// The trip text of a loop-guard kind and count (contract §6.16 table).
pub(crate) fn loop_guard_trip_text(kind: &str, count: u64) -> Result<String, CpdError> {
    Ok(match kind {
        "inter-turn" => format!("loop guard, {count} turns in 10 min"),
        "read-streak" => {
            format!("loop guard, more than {count} consecutive read-only calls in one turn")
        }
        "turn-tools" => format!("loop guard, more than {count} read-only calls in one turn"),
        "turn-refusals" => format!("loop guard, {count} refused calls in one turn"),
        other => {
            return Err(usage(format!(
                "A loop-guard trip kind is inter-turn, read-streak, turn-tools or turn-refusals, not {other:?}."
            )));
        }
    })
}

/// The id of `context`'s live Manager (role `manager`, not archived) that is not `lead`, the first created.
fn live_manager(tx: &WriteTxn<'_>, context: &str, lead: &str) -> Result<Option<String>, CpdError> {
    Ok(tx
        .query_row(
            "SELECT id FROM sessions WHERE role = 'manager' AND archived_at IS NULL AND context = ?1 AND id <> ?2
             ORDER BY created_at, id LIMIT 1",
            params![context, lead],
            |r| r.get(0),
        )
        .optional()?)
}

/// A string field of the report's `details`, or `""` when it is absent or not a string.
fn string_field(details: &Map<String, Value>, key: &str) -> String {
    details
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

/// Appends the `guard` Event unless an earlier one of the same Session carries the same `code` and `excerpt`
/// inside the repeat window (contract §5.2 "The dedupe is the existing repeat window", §3.7), in which case it
/// returns that Event's id and appends nothing.
fn append_guard_once(
    tx: &WriteTxn<'_>,
    window: std::time::Duration,
    code: &str,
    excerpt: &str,
    session: &crate::ids::SessionId,
    event: NewEvent,
) -> Result<i64, CpdError> {
    if !window.is_zero() {
        let cutoff = format!("-{} seconds", window.as_secs_f64());
        let earlier: Option<i64> = tx
            .query_row(
                "SELECT id FROM events WHERE kind = 'guard' AND session = ?1
                   AND json_extract(data, '$.code') = ?2 AND json_extract(data, '$.excerpt') = ?3
                   AND at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?4)
                 ORDER BY id DESC LIMIT 1",
                params![session.as_str(), code, excerpt, cutoff],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(earlier) = earlier {
            return Ok(earlier);
        }
    }
    append(tx, event)
}
