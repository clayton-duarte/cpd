//! `session report` (commands.md §10.18; hs-d573 M3/M4): an adapter error for the Manager. One `error` Event with
//! no Plan and no Session, so no Lead's follower receives it back.

use std::str::FromStr;

use serde_json::{Map, Value, json};

use super::{Env, ReportArgs};
use crate::args::SessionArg;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::output::Output;
use super::row;

const TEXT_MAX: usize = 2000;
const CODE_MAX: usize = 64;

fn usage(message: String) -> CpdError {
    CpdError::new(ErrorCode::Usage, message)
}

/// 1 to 2000 characters (`--what`, `--message`).
#[derive(Clone, Debug)]
pub struct Short(String);

impl FromStr for Short {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Short, CpdError> {
        let chars = s.chars().count();
        if chars == 0 || chars > TEXT_MAX {
            return Err(usage(format!("The text must be 1 to {TEXT_MAX} characters.")).with("chars", chars));
        }
        Ok(Short(s.to_owned()))
    }
}

/// `^[a-z][a-z0-9_]{0,63}$`.
#[derive(Clone, Debug)]
pub struct Code(String);

impl FromStr for Code {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Code, CpdError> {
        let mut bytes = s.bytes();
        let ok = s.len() <= CODE_MAX
            && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
            && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !ok {
            return Err(usage("The code must match ^[a-z][a-z0-9_]{0,63}$.".to_owned()).with("code", s));
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
    let reference = SessionArg { session: args.session }.require()?.clone();
    let mut env = Env::open()?;
    let id = env.resolve(&reference)?;
    let ReportArgs { what, code, message, details, .. } = args;
    let event = env.store.write("session report", |tx| {
        let session = row::load(tx, &id)?;
        let mut details = details.map_or_else(Map::new, |d| d.0);
        details.insert("what".into(), json!(what.0));
        details.insert("session".into(), json!(session.id));
        details.insert("plan".into(), json!(session.plan));
        let text = format!("φ adapter: {}: {}: {}", what.0, code.0, message.0);
        let event = NewEvent::error(text, "adapter", &code.0, &message.0, Value::Object(details)).context(session.context.clone()).session(session.id.clone());
        append(tx, event)
    })?;
    Ok(Output::success(json!({"event": event})))
}
