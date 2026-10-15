//! Agent states (contract §3.6; commands.md §10.6) and names (§10.7).

use std::fmt;
use std::str::FromStr;

use serde_json::json;

use super::row::{self, Row, State};
use super::{Env, RenameArgs, StateArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::ids::SessionId;
use crate::output::Output;
use crate::store::Store;

/// Why a Session is `blocked`: 1–80 characters, no control characters; else
/// `usage` (commands.md §10.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label(String);

impl Label {
    pub const MAX_CHARS: usize = 80;

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Label {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Label, CpdError> {
        let chars = s.chars().count();
        if chars == 0 || chars > Label::MAX_CHARS || s.chars().any(char::is_control) {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!("A label must be 1 to {} characters with no control characters.", Label::MAX_CHARS),
            )
            .with("value", s));
        }
        Ok(Label(s.to_owned()))
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// §3.6's table. `blocked → blocked` is a change of label, which is a
/// `session-state` Event too (§5.2: "a Session's state or label changes").
fn legal(from: State, to: State) -> bool {
    use State::*;
    matches!(
        (from, to),
        (_, Stopped) | (Stopped, Idle) | (Idle | Blocked, Working) | (Working | Blocked, Idle) | (Working | Idle | Blocked, Blocked)
    )
}

/// What [`set`] did.
pub(super) struct Change {
    pub from: State,
    /// The state and label were already these: nothing was written.
    pub already: bool,
}

/// Moves Session `id` to `to` with `label` (set exactly when `to` is
/// `blocked`), as `command`: one transaction with its `session-state` Event,
/// or nothing when unchanged (§3.6).
pub(super) fn set(store: &mut Store, command: &str, id: &SessionId, to: State, label: Option<&Label>) -> Result<Change, CpdError> {
    let label = label.map(Label::as_str);
    let read = store.read(|conn| row::load(conn, id))?;
    if read.state == to && read.state_label.as_deref() == label {
        return Ok(Change { from: read.state, already: true });
    }
    if !legal(read.state, to) {
        return Err(illegal(id, read.state, to));
    }
    store.write(command, |tx| {
        let changed = tx.execute(
            "UPDATE sessions SET state = ?1, state_label = ?2, state_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE id = ?3 AND state = ?4 AND state_label IS ?5",
            rusqlite::params![to.as_str(), label, id.as_str(), read.state.as_str(), read.state_label],
        )?;
        if changed == 0 {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Session {id} changed state meanwhile: run the command again."))
                .with("session", id.as_str())
                .with("expected", read.state.as_str()));
        }
        let text = match label {
            Some(label) => format!("Session {} is {} ({label}).", read.name, to.as_str()),
            None => format!("Session {} is {}.", read.name, to.as_str()),
        };
        let event = NewEvent::new(Kind::SessionState, text, json!({"from": read.state.as_str(), "to": to.as_str(), "label": label}));
        append(tx, scoped(event, &read))?;
        Ok(())
    })?;
    Ok(Change { from: read.state, already: false })
}

fn illegal(id: &SessionId, from: State, to: State) -> CpdError {
    CpdError::new(
        ErrorCode::IllegalTransition,
        format!("Session {id} cannot go from {} to {} (contract §3.6).", from.as_str(), to.as_str()),
    )
    .with("session", id.as_str())
    .with("from", from.as_str())
    .with("to", to.as_str())
}

/// An Event about Session `row`: its Session and, when it has one, its Plan.
pub(super) fn scoped(event: NewEvent, row: &Row) -> NewEvent {
    let event = event.session(row.id.clone()).context(row.context.clone());
    match &row.plan {
        Some(plan) => event.plan(plan.clone()),
        None => event,
    }
}

/// `session state` (commands.md §10.6).
pub(super) fn state(args: StateArgs) -> Result<Output, CpdError> {
    match (args.state, &args.label) {
        (State::Blocked, None) => {
            return Err(CpdError::new(ErrorCode::Usage, "--label is required with blocked.").with("state", "blocked"));
        }
        (other, Some(_)) if other != State::Blocked => {
            return Err(CpdError::new(ErrorCode::Usage, format!("--label is only allowed with blocked, not {}.", other.as_str()))
                .with("state", other.as_str()));
        }
        _ => {}
    }
    let id = args.session.require()?.clone();
    let mut env = Env::open()?;
    let change = set(&mut env.store, "session state", &id, args.state, args.label.as_ref())?;
    let label = args.label.as_ref().map(Label::as_str);
    let mut out = json!({"session": id, "state": args.state.as_str(), "label": label, "from": change.from.as_str()});
    if change.already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// `session rename` (commands.md §10.7).
pub(super) fn rename(args: RenameArgs) -> Result<Output, CpdError> {
    let id = args.session.require()?.clone();
    let name = args.name.as_str();
    let mut env = Env::open()?;
    let read = env.store.read(|conn| row::load(conn, &id))?;
    if read.name == name {
        return Ok(Output::success(json!({"session": id, "name": name, "already": true})));
    }
    env.store.write("session rename", |tx| {
        tx.execute("UPDATE sessions SET name = ?1 WHERE id = ?2", [name, id.as_str()])?;
        let text = format!("Session {id} renamed to {name:?}.");
        append(tx, scoped(NewEvent::new(Kind::SessionRenamed, text, json!({"name": name})), &read))?;
        Ok(())
    })?;
    Ok(Output::success(json!({"session": id, "name": name})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transitions_of_section_3_6() {
        use State::*;
        let all = [Working, Blocked, Idle, Stopped];
        let allowed: Vec<(State, State)> =
            all.iter().flat_map(|f| all.iter().map(move |t| (*f, *t))).filter(|(f, t)| legal(*f, *t)).collect();
        assert_eq!(
            allowed,
            [
                (Working, Blocked),
                (Working, Idle),
                (Working, Stopped),
                (Blocked, Working),
                (Blocked, Blocked),
                (Blocked, Idle),
                (Blocked, Stopped),
                (Idle, Working),
                (Idle, Blocked),
                (Idle, Stopped),
                (Stopped, Idle),
                (Stopped, Stopped),
            ]
        );
    }

    #[test]
    fn label_bounds() {
        assert_eq!("cpd: Worker question".parse::<Label>().unwrap().as_str(), "cpd: Worker question");
        for bad in [String::new(), "a".repeat(81), "a\nb".to_owned()] {
            assert_eq!(bad.parse::<Label>().unwrap_err().code(), ErrorCode::Usage, "{bad:?}");
        }
    }
}
