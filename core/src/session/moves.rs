//! Lead panes in the cockpit (contract §6.2–§6.4, §6.7; commands.md §10.3–
//! §10.5, with §19 amendments 1–3): `session fork`, `open` and `park`; and
//! `session close` (§10.9, amendment 14).
//!
//! The chat slot always holds a pane: a Lead's, or the placeholder running
//! `cpd tui view chat`. `park` creates the placeholder in a new park window
//! and swaps it into the slot; `open` kills it, with its window, when a Lead
//! takes the slot. So at most one placeholder exists, and only in the slot.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::row::{self, Role, Row};
use super::state::scoped;
use super::archive::archived;
use super::{CloseArgs, Env, ForkArgs, OpenArgs, PROGRAM, ParkArgs, no_cockpit, not_a_lead, pane_gone};
use crate::args::Name;
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::home::CpdHome;
use crate::host::layout::{context_window, park_window};
use crate::host::{Pane, PaneId, Spawn, Tmux, View, WindowId, chat_slot};
use crate::ids::{PlanId, SessionId};
use crate::output::Output;
use crate::store::{Store, WriteTxn};

/// What `tmux new-window` runs for Lead `id`: `cpd session run <id>` in its
/// cwd, with `CPD_HOME`, `CPD_SESSION` and `CPD_PLAN` (commands.md §10.3); a
/// Context's Manager has no Plan, so no `CPD_PLAN` (§3.10).
pub(crate) fn lead_spawn(home: &CpdHome, id: &SessionId, plan: Option<&PlanId>, cwd: &Path) -> Spawn {
    let mut env = BTreeMap::from([
        ("CPD_HOME".to_owned(), home.root().display().to_string()),
        ("CPD_SESSION".to_owned(), id.to_string()),
    ]);
    if let Some(plan) = plan {
        env.insert("CPD_PLAN".to_owned(), plan.to_string());
    }
    Spawn { cwd: Some(cwd.to_path_buf()), env, argv: [PROGRAM, "session", "run", id.as_str()].map(str::to_owned).into() }
}

/// The window a parked Session lives in: `ctx-<context>` for a Manager, else `park-<label>`.
fn parked_window(row: &Row) -> String {
    match row.role {
        Role::Manager => context_window(&row.context),
        _ => park_window(&row.id, row.plan.as_ref()),
    }
}

/// The placeholder's program: `cpd tui view chat` (amendment 2).
pub(super) fn placeholder_spawn() -> Spawn {
    Spawn { cwd: None, env: BTreeMap::new(), argv: [PROGRAM, "tui", "view", "chat"].map(str::to_owned).into() }
}

/// Appends `session-moved` for `row`, and records its pane when it changed.
pub(super) fn moved(tx: &WriteTxn<'_>, row: &Row, pane: &PaneId, window: &WindowId, place: &str) -> Result<(), CpdError> {
    if row.tmux_pane.as_deref() != Some(pane.as_str()) {
        tx.execute("UPDATE sessions SET tmux_pane = ?1 WHERE id = ?2", [pane.as_str(), row.id.as_str()])?;
    }
    let kind = if row.role == Role::Manager { "Manager" } else { "Lead" };
    let text = match place {
        "chat" => format!("{kind} {} is in the chat slot.", row.name),
        _ => format!("{kind} {} is {place}.", row.name),
    };
    let event = NewEvent::new(Kind::SessionMoved, text, json!({"pane": pane.as_str(), "window": window.as_str(), "place": place}));
    append(tx, scoped(event, row))?;
    Ok(())
}

pub(super) fn require_lead(row: &Row) -> Result<(), CpdError> {
    match row.role {
        Role::Lead => Ok(()),
        role => Err(not_a_lead(&row.id, role)),
    }
}

/// `session open` and `park` take a Lead or a Context's Manager (commands.md §10.4, §10.5).
fn require_chat_session(row: &Row) -> Result<(), CpdError> {
    match row.role {
        Role::Lead | Role::Manager => Ok(()),
        role => Err(not_a_lead(&row.id, role)),
    }
}

/// `<source name> (fork)`, cut to 80 characters.
fn fork_name(source: &str) -> String {
    format!("{source} (fork)").chars().take(Name::MAX_CHARS).collect()
}

pub(super) fn unreadable(path: &Path, e: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::FileUnreadable, format!("Cannot read {} as UTF-8 text: {e}.", path.display()))
        .with("path", path.display().to_string())
}

fn write_file(path: &Path, text: &str) -> Result<(), CpdError> {
    let failed = |e: std::io::Error| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot write {}: {e}.", path.display()))
            .with("path", path.display().to_string())
    };
    std::fs::create_dir_all(path.parent().expect("a file under $CPD_HOME has a directory")).map_err(failed)?;
    std::fs::write(path, text).map_err(failed)
}

/// `session fork` (commands.md §10.3, contract §6.7).
pub(super) fn fork(args: ForkArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let source_id = env.resolve(&args.source)?;
    let source = env.store.read(|conn| row::load(conn, &source_id))?;
    require_lead(&source)?;
    let plan = args.plan.clone().or_else(|| source.plan.clone()).ok_or_else(|| {
        CpdError::new(
            ErrorCode::PlanRequired,
            format!("Session {} has no Plan: pass --plan <id>.", source.id),
        )
        .with("source", source.id.as_str())
    })?;
    if source.archived_at.is_some() {
        return Err(archived(&source));
    }
    let context = env
        .store
        .read(|conn| row::plan_lead(conn, &plan))?
        .map(|(context, _)| context)
        .ok_or_else(|| CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {plan}.")).with("plan", plan.as_str()))?;
    let message = match &args.message_file {
        Some(path) => Some(std::fs::read_to_string(path).map_err(|e| unreadable(path, e))?),
        None => None,
    };
    let tmux = env.tmux();
    let cockpit = env.config.tmux.cockpit_session.clone();
    if !tmux.has_session(&cockpit)? {
        return Err(no_cockpit(&cockpit));
    }

    let id = SessionId::new_v4();
    let name = args.name.as_ref().map_or_else(|| fork_name(&source.name), |n| n.as_str().to_owned());
    if let Some(message) = &message {
        write_file(&env.home.session_message(&plan, &id), message)?;
    }
    let tier = env.config.lead_tier.to_string();
    let row = env.store.write("session fork", |tx| {
        tx.execute(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                                   state, state_label, state_at, created_at)
             SELECT ?1, 'lead', ?2, ?3, NULL, ?4, NULL, ?5, ?6, ?7, NULL, 'stopped', NULL, at, at
               FROM txns WHERE id = cpd_txn()",
            rusqlite::params![id.as_str(), context.as_str(), plan.as_str(), name, source.id.as_str(), source.cwd, tier],
        )?;
        let row = row::load(tx, &id)?;
        let text = format!("Lead {name} registered, forked from {}. Next: cpd session open {id}", source.name);
        let data = json!({"role": "lead", "name": name, "forked_from": source.id, "tier": tier});
        append(tx, scoped(NewEvent::new(Kind::SessionRegistered, text, data), &row))?;
        Ok(row)
    })?;

    let spawn = lead_spawn(&env.home, &id, Some(&plan), Path::new(&source.cwd));
    let (window, pane) = tmux.new_window(&cockpit, &park_window(&id, Some(&plan)), &spawn)?;
    kill_window_on_error(&tmux, &window, || {
        tmux.set_pane_session(&pane, &id)?;
        tmux.set_pane_title(&pane, &name)?;
        env.store.write("session fork", |tx| moved(tx, &row, &pane, &window, "parked"))
    })?;
    Ok(Output::success(json!({"session": {
        "id": id, "name": name, "plan": plan, "forked_from": source.id, "pane": pane.as_str(), "window": window.as_str(),
    }})))
}

/// Runs `steps`, the ones after `tmux.new_window` made `window`. When one fails, kills `window`
/// (a failure of that is ignored) and returns the original error (G11).
fn kill_window_on_error<T>(tmux: &Tmux, window: &WindowId, steps: impl FnOnce() -> Result<T, CpdError>) -> Result<T, CpdError> {
    steps().inspect_err(|_| {
        let _ = tmux.kill_window(window);
    })
}

/// The chat slot, else `no_cockpit`.
pub(super) fn slot(panes: &[Pane], cockpit: &str) -> Result<Pane, CpdError> {
    chat_slot(panes, cockpit).cloned().ok_or_else(|| no_cockpit(cockpit))
}

/// `session open` (commands.md §10.4, contract §6.4).
pub(super) fn open(args: OpenArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let id = env.resolve(&args.id)?;
    let lead = env.store.read(|conn| row::load(conn, &id))?;
    require_chat_session(&lead)?;
    if lead.archived_at.is_some() {
        return Err(archived(&lead));
    }
    // A Lead has a Plan; a Context's Manager has none.
    if (lead.role == Role::Lead) == lead.plan.is_none() {
        return Err(super::bad_row(format!("{} {} has plan {:?}", lead.role.as_str(), lead.id, lead.plan)));
    }
    let tmux = env.tmux();
    let cockpit = env.config.tmux.cockpit_session.clone();
    let panes = tmux.list_panes()?;
    let slot = slot(&panes, &cockpit)?;
    if slot.cpd_session.as_ref() == Some(&lead.id) && !slot.dead {
        return Ok(Output::success(json!({"session": lead.id, "pane": slot.pane.as_str(), "already": true})));
    }

    // The Lead the slot holds now, if not the placeholder: known before any tmux work.
    let displaced = match &slot.cpd_session {
        None => None,
        Some(other) => Some(env.store.read(|conn| row::find(conn, other))?.ok_or_else(|| {
            CpdError::new(ErrorCode::SessionNotFound, format!("The chat slot runs Session {other}, which cpd does not know."))
                .with("session", other.as_str())
        })?),
    };
    // A dead pane of the Lead outside the slot is killed with its window first, so a new window
    // never makes a second one (G2); one in the slot is displaced below, and dies there.
    for dead in panes.iter().filter(|p| p.dead && p.cpd_session.as_ref() == Some(&lead.id) && p.pane != slot.pane) {
        tmux.kill_pane(&dead.pane)?;
    }
    // Its live pane (a dead one never hides it), else a new park window resuming it.
    let live = panes.iter().find(|p| !p.dead && p.cpd_session.as_ref() == Some(&lead.id));
    let (pane, from_window) = match live {
        Some(p) => (p.pane.clone(), p.window.clone()),
        None => {
            let spawn = lead_spawn(&env.home, &lead.id, lead.plan.as_ref(), Path::new(&lead.cwd));
            let (window, pane) = tmux.new_window(&cockpit, &parked_window(&lead), &spawn)?;
            kill_window_on_error(&tmux, &window, || {
                tmux.set_pane_session(&pane, &lead.id)?;
                tmux.set_pane_title(&pane, &env.store.read(|conn| lead.title(conn))?)
            })?;
            (pane, window)
        }
    };
    tmux.swap_pane(&pane, &slot.pane)?;
    env.store.write("session open", |tx| {
        if let (Some(other), true) = (&displaced, slot.dead) {
            // Its pane is gone: what E2 writes for it (§3.6), before the Lead's new pane is recorded.
            let why = format!("its pane {} is dead", slot.pane);
            pane_gone(tx, &other.id, other.state.as_str(), other.tmux_pane.as_deref(), &why)?;
        }
        moved(tx, &lead, &pane, &slot.window, "chat")?;
        if let (Some(other), false) = (&displaced, slot.dead) {
            moved(tx, other, &slot.pane, &from_window, "parked")?;
        }
        Ok(())
    })?;
    // The swap is the fact and is recorded; a failure below only leaves a window name or a pane.
    // The displaced pane is now in `from_window`: a dead one dies there, like the placeholder.
    match &displaced {
        Some(other) if !slot.dead => tmux.rename_window(&from_window, &parked_window(other))?,
        _ => tmux.kill_pane(&slot.pane)?,
    }
    let displaced = displaced.filter(|_| !slot.dead).map_or(Value::Null, |d| json!(d.id));
    Ok(Output::success(json!({"session": lead.id, "pane": pane.as_str(), "displaced": displaced})))
}

/// `session park` (commands.md §10.5, contract §6.4 with amendment 3).
pub(super) fn park(args: ParkArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let tmux = env.tmux();
    let cockpit = env.config.tmux.cockpit_session.clone();
    let panes = tmux.list_panes()?;
    let slot = slot(&panes, &cockpit)?;
    let named = args.id.as_ref().map(|reference| env.resolve(reference)).transpose()?;
    let Some(id) = named.or_else(|| slot.cpd_session.clone()) else {
        // The placeholder holds the slot: there is no Lead to park.
        return Ok(Output::success(json!({"session": null, "already": true})));
    };
    let lead = env.store.read(|conn| row::load(conn, &id))?;
    require_chat_session(&lead)?;
    if slot.cpd_session.as_ref() != Some(&lead.id) {
        return Ok(Output::success(json!({"session": lead.id, "already": true})));
    }

    let (window, _) = park_lead(&mut env, &tmux, &cockpit, &lead, &slot, "session park")?;
    Ok(Output::success(json!({"session": lead.id, "pane": slot.pane.as_str(), "window": window.as_str()})))
}

/// Parks `lead`, which holds the chat slot `slot`: a new `park-<label>` window with a placeholder,
/// `swap-pane`, then `session-moved` as `command`. Returns the window and the placeholder, now in the slot.
pub(super) fn park_lead(
    env: &mut Env,
    tmux: &Tmux,
    cockpit: &str,
    lead: &Row,
    slot: &Pane,
    command: &str,
) -> Result<(WindowId, PaneId), CpdError> {
    let (window, placeholder) = tmux.new_window(cockpit, &parked_window(lead), &placeholder_spawn())?;
    // Until the swap, the window holds only the placeholder: a failure kills it (G11).
    kill_window_on_error(tmux, &window, || {
        tmux.set_pane_view(&placeholder, View::Chat)?;
        tmux.swap_pane(&placeholder, &slot.pane)
    })?;
    env.store.write(command, |tx| moved(tx, lead, &slot.pane, &window, "parked"))?;
    Ok((window, placeholder))
}

/// What [`close_lead`] found.
pub struct Closed {
    /// The Lead's pane, or none when it had none.
    pub pane: Option<String>,
    /// Nothing was killed: no live pane, or its rows were already written.
    pub already: bool,
}

/// Ends Lead `id`'s pane as `session close` does (commands.md §10.9, contract
/// §6.4): kills it and writes, as `command`, the rows engine E2 writes for a
/// pane that is gone. Refuses the Lead in the chat slot (`in_chat_slot`).
pub fn close_lead(store: &mut Store, tmux: &Tmux, cockpit: &str, id: &SessionId, command: &str) -> Result<Closed, CpdError> {
    // The rows are read first, then the panes, so a Lead that reached the slot
    // meanwhile shows in at least one of them.
    let mut lead = store.read(|conn| row::load(conn, id))?;
    // A Context's Manager is closed by the archive cascade as a Lead is (contract §2.2 "Archived").
    if lead.role == Role::Worker {
        return Err(not_a_lead(&lead.id, lead.role));
    }
    let panes = tmux.list_panes()?;
    let in_slot = chat_slot(&panes, cockpit).is_some_and(|p| p.cpd_session.as_ref() == Some(&lead.id));
    if lead.place == "chat" || in_slot {
        return Err(CpdError::new(
            ErrorCode::InChatSlot,
            format!("Lead {} is in the chat slot: park it or open another Lead first.", lead.name),
        )
        .with("session", lead.id.as_str())
        .with("next", "cpd session park"));
    }
    let Some(pane) = lead.tmux_pane.clone() else {
        return Ok(Closed { pane: None, already: true });
    };
    let live = |panes: &[Pane]| panes.iter().any(|p| p.pane.as_str() == pane);
    if !live(&panes) {
        return Ok(Closed { pane: Some(pane), already: true });
    }
    let pane_id = PaneId::parse(&pane).ok_or_else(|| super::bad_row(format!("tmux_pane {pane:?}")))?;
    tmux.kill_pane(&pane_id)?;

    // pi's adapter may report `stopped` as the pane dies, which changes the
    // state the write is checked against: read again while the pane is still ours.
    let why = format!("its pane {pane} was closed");
    for _ in 0..3 {
        let changed = store.write(command, |tx| pane_gone(tx, &lead.id, lead.state.as_str(), Some(&pane), &why))?;
        if changed {
            return Ok(Closed { pane: Some(pane), already: false });
        }
        lead = store.read(|conn| row::load(conn, id))?;
        if lead.tmux_pane.as_deref() != Some(pane.as_str()) {
            break;
        }
    }
    Ok(Closed { pane: Some(pane), already: true })
}

/// Writes, as `command`, the rows engine E2 writes for the pane of Lead `id` and then kills that
/// pane: for the caller that runs in it, which must not die before its rows are written.
/// Returns when the pane is not there to kill.
pub(super) fn close_own(env: &mut Env, id: &SessionId, command: &str) -> Result<(), CpdError> {
    let lead = env.store.read(|conn| row::load(conn, id))?;
    let Some(pane) = lead.tmux_pane.clone() else { return Ok(()) };
    let pane_id = PaneId::parse(&pane).ok_or_else(|| super::bad_row(format!("tmux_pane {pane:?}")))?;
    let why = format!("its pane {pane} was closed");
    env.store.write(command, |tx| pane_gone(tx, &lead.id, lead.state.as_str(), Some(&pane), &why))?;
    env.tmux().kill_pane(&pane_id)
}

/// `session close` (commands.md §10.9).
pub(super) fn close(args: CloseArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let tmux = env.tmux();
    let cockpit = env.config.tmux.cockpit_session.clone();
    let id = env.resolve(&args.id)?;
    let closed = close_lead(&mut env.store, &tmux, &cockpit, &id, "session close")?;
    let mut out = json!({"session": id, "pane": closed.pane});
    if closed.already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// The message files a Lead's `session run` reads (commands.md §1.4).
pub(super) fn first_message(home: &CpdHome, row: &Row) -> Result<Option<PathBuf>, CpdError> {
    let Some(plan) = &row.plan else {
        // A Context's Manager: `context manager` wrote its first message (commands.md §3.10).
        if row.role == Role::Manager {
            let path = home.manager_message(&row.context);
            return match std::fs::File::open(&path) {
                Ok(_) => Ok(Some(path)),
                Err(e) => Err(unreadable(&path, e)),
            };
        }
        return Ok(None);
    };
    let own = home.session_message(plan, &row.id);
    let path: Option<PathBuf> = if own.exists() {
        Some(own)
    } else if row.plan_lead {
        Some(home.lead_message(plan))
    } else {
        None
    };
    path.map(|p| match std::fs::File::open(&p) {
        Ok(_) => Ok(p),
        Err(e) => Err(unreadable(&p, e)),
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fork_s_default_name_is_cut_to_80_characters() {
        assert_eq!(fork_name("cpd rust lead"), "cpd rust lead (fork)");
        let long = "é".repeat(78);
        assert_eq!(fork_name(&long), format!("{long} ("));
        assert_eq!(fork_name(&long).chars().count(), 80);
    }
}
