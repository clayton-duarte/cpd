//! `session resume` (commands.md §10.17; contract §3.7 L6; hs-d552, hs-d571): a Lead or a Manager blocked by the
//! loop guard is resumed by one `session-resumed` Event, which its adapter acts on. Nothing is written to the Session.

use serde_json::json;

use super::moves::{park_lead, require_pane, start_parked};
use super::row::{self, State};
use super::send::check;
use super::state::scoped;
use super::{Env, ResumeArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::host::chat_slot;
use crate::output::Output;

/// The label of a Session the loop guard blocked (contract §3.6).
pub const LOOP_GUARD_LABEL: &str = "φ: loop guard";

pub(super) fn resume(args: ResumeArgs) -> Result<Output, CpdError> {
    crate::plan::archive::refuse_agent_caller("session resume")?;
    let mut env = Env::open()?;
    let id = env.resolve(&args.id)?;
    let target = env.store.read(|conn| row::load(conn, &id))?;
    check(&target)?;
    // A durable Session has no pane: the engine's hosts step supervises it, so nothing here may start a `session run`.
    require_pane(&target)?;
    match target.state {
        State::Stopped => restart(&mut env, &target),
        State::Blocked => loop_guard(&mut env, &id),
        _ => Ok(Output::success(
            json!({"session": target.id, "pane": null, "state": target.state.as_str(), "already": true}),
        )),
    }
}

/// A stopped Session: started as a parked `session run <id>` window (the cockpit is created when missing). Its id and
/// transcript are kept (issue #1207). A live pane tagged with the Session is the wrapper `session run` leaves waiting at
/// "Press Enter to resume" after pi exited (pi is not running: the state is `stopped`): it is replaced. The state stays
/// `stopped` until the restarted pi's adapter reports `idle`, so the adapter watchdog (`adapter_missing_with`) still holds.
fn restart(env: &mut Env, target: &row::Row) -> Result<Output, CpdError> {
    let tmux = env.tmux();
    let cockpit = env.config.tmux.cockpit_session.clone();
    let panes = tmux.list_panes()?;
    let mut target = target.clone();
    // A wrapper in the chat slot: a placeholder takes the slot first, so killing the wrapper leaves the slot (`session
    // open` finds it later). The wrapper is then in a parked window of its own.
    if let Some(slot) = chat_slot(&panes, &cockpit)
        .filter(|p| !p.dead && p.cpd_session.as_ref() == Some(&target.id))
    {
        park_lead(env, &tmux, &cockpit, &target, slot, "session resume")?;
        target = env.store.read(|conn| row::load(conn, &target.id))?;
    }
    for wrapper in panes
        .iter()
        .filter(|p| !p.dead && p.cpd_session.as_ref() == Some(&target.id))
    {
        tmux.kill_pane(&wrapper.pane)?;
    }
    let pane = start_parked(env, &target, "session resume")?;
    Ok(Output::success(json!({
        "session": target.id,
        "pane": pane.as_ref().map(|p| p.as_str()),
        "state": target.state.as_str(),
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
