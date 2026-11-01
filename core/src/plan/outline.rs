//! The Plan's outline and `plan outline` (commands.md §4.13; contract §2.2 Plan `outline`, §2.3; hs-d502): what the
//! Lead has settled with the User, the source of a Plan review's summary (§4.5). The file is parsed once, here, into
//! [`Outline`]; inside, the types are true.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::row::{self, PlanRow};
use super::{Env, OutlineArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::output::Output;

/// A line: 1 to this many characters.
const LINE_MAX: usize = 200;
const RISKS_MAX: usize = 3;
const OUT_OF_SCOPE_MAX: usize = 5;
const DONE_WHEN_MAX: usize = 3;

/// The seven fields of the outline file, in the order they are checked.
const FIELDS: [&str; 7] = [
    "goal",
    "decisions",
    "assumptions",
    "risks",
    "done_when",
    "out_of_scope",
    "decided",
];

/// One question the User answered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub answer: String,
    pub question: String,
}

/// A valid outline. The fields are declared in key order, so serialising them is the canonical JSON of §4.13.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outline {
    pub assumptions: Vec<String>,
    pub decided: Vec<String>,
    pub decisions: Vec<Decision>,
    pub done_when: BTreeMap<String, Vec<String>>,
    pub goal: String,
    pub out_of_scope: Vec<String>,
    pub risks: Vec<String>,
}

fn invalid(field: &str, why: impl std::fmt::Display) -> CpdError {
    CpdError::new(
        ErrorCode::OutlineInvalid,
        format!("The outline is invalid at {field}: {why}."),
    )
    .with("field", field)
}

/// A line: 1–200 characters, no control character (so no line break and no tab).
fn line(field: &str, value: &Value) -> Result<String, CpdError> {
    let Value::String(text) = value else {
        return Err(invalid(field, "expected a string"));
    };
    match text.chars().count() {
        0 => Err(invalid(field, "must not be empty")),
        n if n > LINE_MAX => Err(invalid(
            field,
            format!("{n} characters, at most {LINE_MAX}"),
        )),
        _ if text.chars().any(char::is_control) => {
            Err(invalid(field, "must be one line with no control character"))
        }
        _ => Ok(text.clone()),
    }
}

/// `value` (absent: empty) as an array of at most `max` lines; `field[i]` is the failure of an element, and the
/// element `max` is the one over the limit.
fn lines(fields: &Map<String, Value>, name: &str, max: usize) -> Result<Vec<String>, CpdError> {
    let Some(value) = fields.get(name) else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = value else {
        return Err(invalid(name, "expected an array"));
    };
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let path = format!("{name}[{i}]");
            if i >= max {
                return Err(invalid(&path, format!("at most {max} entries")));
            }
            line(&path, item)
        })
        .collect()
}

fn decisions(fields: &Map<String, Value>) -> Result<Vec<Decision>, CpdError> {
    let Some(value) = fields.get("decisions") else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = value else {
        return Err(invalid("decisions", "expected an array"));
    };
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let path = format!("decisions[{i}]");
            let Value::Object(pair) = item else {
                return Err(invalid(&path, "expected an object {question, answer}"));
            };
            if let Some(unknown) = pair
                .keys()
                .find(|k| !matches!(k.as_str(), "question" | "answer"))
            {
                return Err(invalid(&format!("{path}.{unknown}"), "unknown field"));
            }
            let part = |name: &str| {
                let field = format!("{path}.{name}");
                line(
                    &field,
                    pair.get(name)
                        .ok_or_else(|| invalid(&field, "missing field"))?,
                )
            };
            Ok(Decision {
                question: part("question")?,
                answer: part("answer")?,
            })
        })
        .collect()
}

fn done_when(fields: &Map<String, Value>) -> Result<BTreeMap<String, Vec<String>>, CpdError> {
    let Some(value) = fields.get("done_when") else { return Ok(BTreeMap::new()) };
    let Value::Object(by_workflow) = value else { return Err(invalid("done_when", "expected an object of Workflow key to lines")) };
    by_workflow
        .iter()
        .map(|(key, value)| {
            let path = format!("done_when.{key}");
            let Value::Array(items) = value else {
                return Err(invalid(&path, "expected an array of 1 to 3 lines"));
            };
            if items.is_empty() {
                return Err(invalid(&path, "needs at least one line"));
            }
            let lines = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let at = format!("{path}[{i}]");
                    if i >= DONE_WHEN_MAX {
                        return Err(invalid(&at, format!("at most {DONE_WHEN_MAX} lines")));
                    }
                    line(&at, item)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok((key.clone(), lines))
        })
        .collect()
}

impl Outline {
    /// The outline file's text, checked in the order of §4.13: not JSON or not an object (`$`), an unknown field (the
    /// first in byte order), then the seven fields and each element in order. What it says of the Plan (the Workflows of
    /// `done_when`, the records of `decided`) is checked against the Plan by [`check_against_plan`].
    pub fn parse(text: &str) -> Result<Outline, CpdError> {
        let value: Value =
            serde_json::from_str(text).map_err(|e| invalid("$", format!("not JSON: {e}")))?;
        let Value::Object(fields) = value else {
            return Err(invalid("$", "expected an object"));
        };
        let mut names: Vec<&String> = fields.keys().collect();
        names.sort();
        if let Some(unknown) = names.into_iter().find(|k| !FIELDS.contains(&k.as_str())) {
            return Err(invalid(unknown, "unknown field"));
        }
        let goal = line(
            "goal",
            fields
                .get("goal")
                .ok_or_else(|| invalid("goal", "missing field"))?,
        )?;
        let decisions = decisions(&fields)?;
        let assumptions = lines(&fields, "assumptions", usize::MAX)?;
        let risks = lines(&fields, "risks", RISKS_MAX)?;
        let done_when = done_when(&fields)?;
        let out_of_scope = lines(&fields, "out_of_scope", OUT_OF_SCOPE_MAX)?;
        let decided = lines(&fields, "decided", usize::MAX)?;
        Ok(Outline {
            assumptions,
            decided,
            decisions,
            done_when,
            goal,
            out_of_scope,
            risks,
        })
    }

    /// As stored (`plans.outline`): a row that does not parse is a bad row.
    pub fn stored(plan: &str, text: &str) -> Result<Outline, CpdError> {
        serde_json::from_str(text)
            .map_err(|e| CpdError::bad_row(format!("Plan {plan} has outline {text:?}: {e}")))
    }

    /// The outline as a JSON value (`plan show`, the fingerprint of §2.3).
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("an outline is JSON")
    }

    /// The canonical JSON of §4.13: keys sorted, no whitespace. A `Value`'s objects are sorted maps.
    pub fn canonical(&self) -> String {
        self.to_value().to_string()
    }
}

/// What the outline says of the Plan, checked against it (§4.13): every `done_when` key is a Workflow of the Plan that is not
/// `cancelled`, every `decided` id is a record of the Plan's Context.
fn check_against_plan(
    conn: &Connection,
    plan: &PlanRow,
    outline: &Outline,
) -> Result<(), CpdError> {
    let counts: Vec<String> = {
        let mut stmt = conn.prepare("SELECT key FROM workflows WHERE plan = ?1 AND status <> 'cancelled'")?;
        stmt.query_map([plan.id.as_str()], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    if let Some(key) = outline.done_when.keys().find(|k| !counts.contains(k)) {
        return Err(CpdError::new(
            ErrorCode::OutlineInvalid,
            format!("The outline's done_when names {key}, which is not a Workflow of Plan {} that is not cancelled.", plan.id),
        )
        .with("field", format!("done_when.{key}"))
        .with("key", key.as_str()));
    }
    for (i, id) in outline.decided.iter().enumerate() {
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM context_records WHERE context = ?1 AND id = ?2",
                [plan.context.as_str(), id],
                |r| r.get(0),
            )
            .optional()?;
        if found.is_none() {
            return Err(CpdError::new(
                ErrorCode::OutlineInvalid,
                format!(
                    "The outline's decided names {id}, which is no record of Context {}.",
                    plan.context
                ),
            )
            .with("field", format!("decided[{i}]"))
            .with("record", id.as_str()));
        }
    }
    Ok(())
}

/// The Plan's outline as stored, if set.
pub(super) fn load(
    conn: &Connection,
    plan: &crate::ids::PlanId,
) -> Result<Option<Outline>, CpdError> {
    let text: Option<String> = conn
        .query_row(
            "SELECT outline FROM plans WHERE id = ?1",
            [plan.as_str()],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    text.map(|t| Outline::stored(plan.as_str(), &t)).transpose()
}

/// The Plan, as `plan outline` may write to it: it exists and is not archived.
fn open_plan(conn: &Connection, id: &crate::ids::PlanId) -> Result<PlanRow, CpdError> {
    let plan = row::load(conn, id)?;
    if plan.archived_at.is_some() {
        return Err(CpdError::new(
            ErrorCode::Archived,
            format!("Plan {id} is archived: unarchive it first."),
        )
        .with("plan", id.as_str()));
    }
    Ok(plan)
}

/// `plan outline` (§4.13).
pub(super) fn outline(args: OutlineArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let text = std::fs::read_to_string(&args.file)
        .map_err(|e| CpdError::file_unreadable(&args.file, e))?;
    let parsed = Outline::parse(&text)?;
    let canonical = parsed.canonical();
    let mut env = Env::open()?;
    // The same outline again writes nothing, not even the transaction's own row: it is looked for before the write, and
    // checked against the Plan only when it is new (§4.13: it is checked when it is written).
    let same = |plan: &PlanRow| {
        plan.outline
            .as_ref()
            .is_some_and(|old| old.canonical() == canonical)
    };
    let mut replaced = None;
    if !same(&env.store.read(|conn| open_plan(conn, &id))?) {
        // Checked again inside the transaction, so the outline written is the outline checked.
        replaced = env.store.write("plan outline", |tx| {
            let plan = open_plan(tx, &id)?;
            if same(&plan) {
                return Ok(None);
            }
            check_against_plan(tx, &plan, &parsed)?;
            tx.execute(
                "UPDATE plans SET outline = ?1 WHERE id = ?2",
                [canonical.as_str(), id.as_str()],
            )?;
            let replaced = plan.outline.is_some();
            append(
                tx,
                NewEvent::plan_outlined(format!("Plan {id} outlined."), replaced).plan(id.clone()),
            )?;
            Ok(Some(replaced))
        })?;
    }
    let mut out = json!({"plan": id, "outline": parsed.to_value()});
    match replaced {
        Some(replaced) => out["replaced"] = json!(replaced),
        None => out["already"] = json!(true),
    }
    Ok(Output::success(out))
}
