//! Agent states (contract §3.6; commands.md §10.6) and names (§10.7).

use std::fmt;
use std::str::FromStr;

use serde_json::json;

use super::row::{self, Role, Row, State};
use super::{Env, RenameArgs, StateArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::SessionId;
use crate::output::Output;
use crate::store::{Store, WriteTxn};

/// Why a Session is `blocked` or `waiting`: 1–80 characters, no control characters; else
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
                format!(
                    "A label must be 1 to {} characters with no control characters.",
                    Label::MAX_CHARS
                ),
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

/// §3.6's table. `blocked → blocked` and `waiting → waiting` are a change of label, which is a
/// `session-state` Event too (§5.2: "a Session's state or label changes"). `waiting` is left for
/// `blocked` when a human reason opens and entered from `blocked` when the last one closes while
/// an agent is still waited for.
fn legal(from: State, to: State) -> bool {
    use State::*;
    matches!(
        (from, to),
        (_, Stopped)
            | (Stopped, Idle)
            | (Idle | Blocked | Waiting, Working)
            | (Working | Blocked | Waiting, Idle)
            | (Working | Idle | Blocked | Waiting, Blocked)
            | (Working | Idle | Blocked | Waiting, Waiting)
    )
}

/// What [`set`] did.
pub(super) struct Change {
    pub from: State,
    /// The state and label were already these: nothing was written.
    pub already: bool,
}

/// Moves Session `id` to `to` with `label` (set exactly when `to` is
/// `waiting` or `blocked`), as `command`: one transaction with its `session-state` Event,
/// or nothing when unchanged (§3.6).
pub(super) fn set(
    store: &mut Store,
    command: &str,
    id: &SessionId,
    to: State,
    label: Option<&Label>,
) -> Result<Change, CpdError> {
    set_reply(store, command, id, to, label, false)
}

/// Whether the Session is in Context `codereview` and its Plan is `done`: its idle reply is not recorded (pro-d391).
fn is_done_codereview_plan(conn: &rusqlite::Connection, read: &Row) -> Result<bool, CpdError> {
    let Some(plan) = read.plan.as_ref() else {
        return Ok(false);
    };
    if read.context.as_str() != "codereview" {
        return Ok(false);
    }
    Ok(crate::pr::plan_phase(conn, plan)? == crate::plan::row::Phase::Done)
}

/// [`set`], with `reply` (only with `idle`) as `reply: true` in the Event's `data` (§5.2), as `session state idle --reply` gives it.
pub(super) fn set_reply(
    store: &mut Store,
    command: &str,
    id: &SessionId,
    to: State,
    label: Option<&Label>,
    reply: bool,
) -> Result<Change, CpdError> {
    let label = label.map(Label::as_str);
    let read = store.read(|conn| row::load(conn, id))?;
    if read.state == to && read.state_label.as_deref() == label {
        return Ok(Change {
            from: read.state,
            already: true,
        });
    }
    if to == State::Blocked
        && matches!(
            label,
            Some(crate::ask::question::LABEL | crate::ask::question::WORKER_QUESTION_LABEL)
        )
    {
        return Err(CpdError::new(
            ErrorCode::Usage,
            "These labels are agent waits, not the User's: the state is `waiting`, never `blocked` (contract §3.6, hs-d303).",
        )
        .with("label", label.unwrap_or_default()));
    }
    if !legal(read.state, to) {
        return Err(illegal(id, read.state, to));
    }
    store.write(command, |tx| {
        let changed = tx.execute(
            "UPDATE sessions SET state = ?1, state_label = ?2, state_at = (SELECT at FROM txns WHERE id = cpd_txn()),
                asleep_at = iif(state = 'stopped', NULL, asleep_at)
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
        let reply = reply && !(to == State::Idle && is_done_codereview_plan(tx, &read)?);
        let event = NewEvent::session_state(text, read.state.as_str(), to.as_str(), label, reply);
        append(tx, scoped(event, &read))?;
        Ok(())
    })?;
    Ok(Change {
        from: read.state,
        already: false,
    })
}

/// Session `id`'s pane is known gone, dead or missing (contract §3.6, §5.2;
/// Escalation `e2e-a1-q1`): the one writer that leaves a Session with no pane.
/// In `tx`, with a compare-and-swap on the `state` and `pane` its caller read,
/// the Session becomes `stopped` with `tmux_pane` null; it appends
/// `session-state` when it was not `stopped`, and `session-moved`
/// `{pane: null, window: null, place: "none"}` when it had a pane. Returns
/// false, writing nothing, when the row is no longer as read (it got a new
/// pane, or changed state, meanwhile). `why` ends each Event's text.
pub fn pane_gone(
    tx: &WriteTxn<'_>,
    id: &SessionId,
    state: &str,
    pane: Option<&str>,
    why: &str,
) -> Result<bool, CpdError> {
    let read = row::load(tx, id)?;
    if read.state.as_str() != state || read.tmux_pane.as_deref() != pane {
        return Ok(false);
    }
    let changed = tx.execute(
        "UPDATE sessions SET state = 'stopped', state_label = NULL, tmux_pane = NULL,
                state_at = iif(state = 'stopped', state_at, (SELECT at FROM txns WHERE id = cpd_txn()))
          WHERE id = ?1 AND state = ?2 AND tmux_pane IS ?3",
        rusqlite::params![id.as_str(), state, pane],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    if read.state != State::Stopped {
        let text = format!("Session {} is stopped: {why}.", read.name);
        append(
            tx,
            scoped(
                NewEvent::session_state(text, read.state.as_str(), "stopped", None, false),
                &read,
            ),
        )?;
    }
    if pane.is_some() {
        let text = format!("Session {} has no pane: {why}.", read.name);
        append(
            tx,
            scoped(NewEvent::session_moved(text, None, None, "none"), &read),
        )?;
    }
    Ok(true)
}

fn illegal(id: &SessionId, from: State, to: State) -> CpdError {
    CpdError::new(
        ErrorCode::IllegalTransition,
        format!(
            "Session {id} cannot go from {} to {} (contract §3.6).",
            from.as_str(),
            to.as_str()
        ),
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
    match (args.state.has_label(), &args.label) {
        (true, None) => {
            let state = args.state.as_str();
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!("--label is required with {state}."),
            )
            .with("state", state));
        }
        (false, Some(_)) => {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!(
                    "--label is only allowed with waiting or blocked, not {}.",
                    args.state.as_str()
                ),
            )
            .with("state", args.state.as_str()));
        }
        _ => {}
    }
    if args.reply && args.state != State::Idle {
        return Err(CpdError::new(
            ErrorCode::Usage,
            format!(
                "--reply is only allowed with idle, not {}.",
                args.state.as_str()
            ),
        )
        .with("state", args.state.as_str()));
    }
    let reference = args.session.require()?.clone();
    let mut env = Env::open()?;
    let id = env.resolve(&reference)?;
    let change = set_reply(
        &mut env.store,
        "session state",
        &id,
        args.state,
        args.label.as_ref(),
        args.reply,
    )?;
    let label = args.label.as_ref().map(Label::as_str);
    let mut out = json!({"session": id, "state": args.state.as_str(), "label": label, "from": change.from.as_str()});
    if change.already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// `session rename` (commands.md §10.7).
pub(super) fn rename(args: RenameArgs) -> Result<Output, CpdError> {
    let reference = args.session.require()?.clone();
    let name = args.name.as_str();
    let mut env = Env::open()?;
    let id = env.resolve(&reference)?;
    let read = env.store.read(|conn| row::load(conn, &id))?;
    let fixed = match read.role {
        Role::Manager | Role::Worker => true,
        Role::Lead => read.forked_from.is_none(),
    };
    if fixed {
        return Err(CpdError::new(
            ErrorCode::KeyFixed,
            format!(
                "Session {id} is named by its key {:?}: it is not renamed.",
                read.name
            ),
        )
        .with("session", id.as_str())
        .with("name", read.name.as_str()));
    }
    if read.name == name {
        row::retitle(&id)?;
        return Ok(Output::success(
            json!({"session": id, "name": name, "already": true}),
        ));
    }
    env.store.write("session rename", |tx| {
        tx.execute(
            "UPDATE sessions SET name = ?1 WHERE id = ?2",
            [name, id.as_str()],
        )?;
        let text = format!("Session {id} renamed to {name:?}.");
        append(tx, scoped(NewEvent::session_renamed(text, name), &read))?;
        Ok(())
    })?;
    row::retitle(&id)?;
    Ok(Output::success(json!({"session": id, "name": name})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transitions_of_section_3_6() {
        use State::*;
        let all = [Working, Waiting, Blocked, Idle, Stopped];
        let allowed: Vec<(State, State)> = all
            .iter()
            .flat_map(|f| all.iter().map(move |t| (*f, *t)))
            .filter(|(f, t)| legal(*f, *t))
            .collect();
        assert_eq!(
            allowed,
            [
                (Working, Waiting),
                (Working, Blocked),
                (Working, Idle),
                (Working, Stopped),
                (Waiting, Working),
                (Waiting, Waiting),
                (Waiting, Blocked),
                (Waiting, Idle),
                (Waiting, Stopped),
                (Blocked, Working),
                (Blocked, Waiting),
                (Blocked, Blocked),
                (Blocked, Idle),
                (Blocked, Stopped),
                (Idle, Working),
                (Idle, Waiting),
                (Idle, Blocked),
                (Idle, Stopped),
                (Stopped, Idle),
                (Stopped, Stopped),
            ]
        );
    }

    /// A Store in a temporary `$CPD_HOME` with Session `s1` (`idle`, pane `%5`).
    fn store_with_session() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let home =
            crate::home::CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        store
            .write("fixture", |tx| {
                tx.execute_batch(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, tmux_pane, state, state_at, created_at)
                       VALUES ('s1', 'lead', 'ctx', 'pay', 'One', '/', 'O3', '%5', 'idle', 't', 't');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 's1', 't');",
                )?;
                Ok(())
            })
            .unwrap();
        (dir, store)
    }

    fn events(store: &mut Store) -> Vec<(String, String)> {
        store
            .read(|c| {
                let mut stmt = c.prepare("SELECT kind, data FROM events ORDER BY id")?;
                Ok(stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    #[test]
    fn pane_gone_stops_the_session_with_no_pane_and_place_none() {
        let (_dir, mut store) = store_with_session();
        let id = SessionId::parse("s1").unwrap();
        assert!(
            store
                .write("test", |tx| pane_gone(
                    tx,
                    &id,
                    "idle",
                    Some("%5"),
                    "its pane %5 is gone"
                ))
                .unwrap()
        );
        let row = store.read(|c| row::load(c, &id)).unwrap();
        assert_eq!(
            (row.state, row.tmux_pane, row.place.as_str()),
            (State::Stopped, None, "none")
        );
        assert_eq!(
            events(&mut store),
            [
                (
                    "session-state".to_owned(),
                    r#"{"from":"idle","label":null,"to":"stopped"}"#.to_owned()
                ),
                (
                    "session-moved".to_owned(),
                    r#"{"pane":null,"place":"none","window":null}"#.to_owned()
                ),
            ]
        );
    }

    #[test]
    fn pane_gone_writes_nothing_when_the_row_changed_since_it_was_read() {
        let (_dir, mut store) = store_with_session();
        let id = SessionId::parse("s1").unwrap();
        // A new pane meanwhile, or another state.
        assert!(
            !store
                .write("test", |tx| pane_gone(tx, &id, "idle", Some("%4"), "gone"))
                .unwrap()
        );
        assert!(
            !store
                .write("test", |tx| pane_gone(
                    tx,
                    &id,
                    "working",
                    Some("%5"),
                    "gone"
                ))
                .unwrap()
        );
        let row = store.read(|c| row::load(c, &id)).unwrap();
        assert_eq!(
            (row.state, row.tmux_pane.as_deref()),
            (State::Idle, Some("%5"))
        );
        assert!(events(&mut store).is_empty());
    }

    #[test]
    fn label_bounds() {
        assert_eq!(
            "φ: Worker question".parse::<Label>().unwrap().as_str(),
            "φ: Worker question"
        );
        for bad in [String::new(), "a".repeat(81), "a\nb".to_owned()] {
            assert_eq!(
                bad.parse::<Label>().unwrap_err().code(),
                ErrorCode::Usage,
                "{bad:?}"
            );
        }
    }
}
