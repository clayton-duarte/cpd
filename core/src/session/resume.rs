//! `session resume` (commands.md §10.17; contract §3.7 L6; hs-d552, hs-d571): a Lead or a Manager blocked by the
//! loop guard is resumed by one `session-resumed` Event, which its adapter acts on. Nothing is written to the Session.

use serde_json::json;

use super::moves::start_parked;
use super::row::{self, State};
use super::send::check;
use super::state::{scoped, set};
use super::{Env, ResumeArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::output::Output;

/// The label of a Session the loop guard blocked (contract §3.6).
pub const LOOP_GUARD_LABEL: &str = "φ: loop guard";

pub(super) fn resume(args: ResumeArgs) -> Result<Output, CpdError> {
    crate::plan::archive::refuse_agent_caller("session resume")?;
    let mut env = Env::open()?;
    let id = env.resolve(&args.id)?;
    let target = env.store.read(|conn| row::load(conn, &id))?;
    check(&target)?;
    match target.state {
        State::Stopped => restart(&mut env, &target),
        State::Blocked => loop_guard(&mut env, &id),
        _ => Ok(Output::success(
            json!({"session": target.id, "pane": null, "state": target.state.as_str(), "already": true}),
        )),
    }
}

/// A stopped Session: started as a parked `session run <id>` window (the cockpit is created when missing), then `idle`.
/// Its id and transcript are kept (issue #1207).
fn restart(env: &mut Env, target: &row::Row) -> Result<Output, CpdError> {
    let pane = start_parked(env, target, "session resume")?;
    if pane.is_some() {
        set(
            &mut env.store,
            "session resume",
            &target.id,
            State::Idle,
            None,
        )?;
    }
    Ok(Output::success(json!({
        "session": target.id,
        "pane": pane.as_ref().map(|p| p.as_str()),
        "state": if pane.is_some() { "idle" } else { target.state.as_str() },
        "already": pane.is_none(),
    })))
}

/// A Session blocked by the loop guard: resumed by one `session-resumed` Event; any other block is refused.
fn loop_guard(env: &mut Env, id: &crate::ids::SessionId) -> Result<Output, CpdError> {
    let (event, target) = env.store.write("session resume", |tx| {
        let target = row::load(tx, id)?;
        check(&target)?;
        if target.state != State::Blocked || target.state_label.as_deref() != Some(LOOP_GUARD_LABEL)
        {
            return Err(CpdError::new(
                ErrorCode::NotLoopGuarded,
                format!("Session {} is not blocked by the loop guard.", target.id),
            )
            .with("state", target.state.as_str())
            .with("label", target.state_label.clone()));
        }
        let event = append(
            tx,
            scoped(
                NewEvent::session_resumed(format!("Loop guard of Session {} resumed.", target.id)),
                &target,
            ),
        )?;
        Ok((event, target))
    })?;
    Ok(Output::success(
        json!({"event": event, "session": target.id}),
    ))
}
