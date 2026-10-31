//! `session history` (commands.md §10.12): an archived Session's history, read-only, in the chat slot.

use std::collections::BTreeMap;

use serde_json::json;

use super::moves::{park_lead, require_lead, require_pane, slot};
use super::row;
use super::{Env, HistoryArgs, PROGRAM};
use crate::error::{CpdError, ErrorCode};
use crate::host::{Spawn, View};
use crate::output::Output;

/// The history pane's program: `cpd tui view history <id>` (§13.2).
fn history_spawn(id: &str) -> Spawn {
    Spawn { cwd: None, env: BTreeMap::new(), argv: [PROGRAM, "tui", "view", "history", id].map(str::to_owned).into() }
}

pub(super) fn history(args: HistoryArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let id = env.resolve(&args.id)?;
    let session = env.store.read(|conn| row::load(conn, &id))?;
    require_lead(&session)?;
    require_pane(&session)?;
    let file = session.file.clone().ok_or_else(|| {
        CpdError::new(ErrorCode::FileUnreadable, format!("Session {} has no session file.", session.id))
            .with("session", session.id.as_str())
    })?;
    std::fs::File::open(&file).map_err(|e| CpdError::file_unreadable(std::path::Path::new(&file), e).with("session", session.id.as_str()))?;

    let tmux = env.tmux();
    let cockpit = env.config.tmux.cockpit_session.clone();
    let panes = tmux.list_panes()?;
    let slot = slot(&panes, &cockpit)?;

    // A Lead in the slot is parked first, as `session park` does; the placeholder that takes its place
    // is the pane to respawn. A placeholder or an earlier history pane is respawned in place.
    let (pane, parked) = match &slot.cpd_session {
        Some(id) => {
            let lead = env.store.read(|conn| row::load(conn, id))?;
            let (_, placeholder) = park_lead(&mut env, &tmux, &cockpit, &lead, &slot, "session history")?;
            (placeholder, Some(lead.id))
        }
        None => (slot.pane.clone(), None),
    };
    tmux.respawn_pane_killing(&pane, &history_spawn(session.id.as_str()))?;
    tmux.set_pane_view(&pane, View::History)?;
    tmux.set_pane_history(&pane, &session.id)?;
    Ok(Output::success(json!({"session": session.id, "pane": pane.as_str(), "parked": parked})))
}
