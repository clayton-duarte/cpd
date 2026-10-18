//! Lead windows in the cockpit (contract §6.2, §6.4, §6.5; commands.md §4.1
//! step 4): `plan new` starts a Plan's Lead in `park-<plan>`, and
//! `context manager` a Context's Manager in `ctx-<context>`. Each pane
//! runs `cpd session run <id>`, which starts pi (§6.7).

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

use super::Env;
use crate::error::CpdError;
use crate::events::{Kind, NewEvent, append};
use crate::host::pi::Plannotator;
use crate::host::{PaneId, Spawn, Tmux, WindowId};
use crate::ids::{ContextId, PlanId, SessionId};
use crate::session::PROGRAM;

/// Where a Lead's pane was started.
#[derive(Clone, Debug)]
pub(super) struct Started {
    pub window: WindowId,
    pub pane: PaneId,
}

/// The Lead a window is started for.
pub(super) struct Lead<'a> {
    pub session: &'a SessionId,
    pub context: &'a ContextId,
    /// The Lead's Plan; `None` for a Context's Manager (contract §6.5:
    /// no `CPD_PLAN` and no `PLANNOTATOR_*`).
    pub plan: Option<&'a PlanId>,
    pub name: &'a str,
    pub cwd: &'a Path,
}

/// What tmux runs for `lead`: `cpd session run <id>` in its cwd, with
/// `CPD_HOME`, `CPD_SESSION`, and for a Plan's Lead `CPD_PLAN` plus
/// `PLANNOTATOR_PORT` and `PLANNOTATOR_REMOTE` from cpd's own environment
/// when set (commands.md §15.5, amendment 8): passed with `-e`, so the pane
/// has them whatever environment the tmux server was started with, and
/// several Plan reviews can be open at once on a port range.
fn spawn(env: &Env, lead: &Lead) -> Spawn {
    let mut vars = BTreeMap::from([
        ("CPD_HOME".to_owned(), env.home.root().display().to_string()),
        ("CPD_SESSION".to_owned(), lead.session.to_string()),
    ]);
    if let Some(plan) = lead.plan {
        vars.insert("CPD_PLAN".to_owned(), plan.to_string());
        let plannotator = Plannotator::from_env();
        if let Some(port) = plannotator.port {
            vars.insert("PLANNOTATOR_PORT".to_owned(), port);
        }
        if let Some(remote) = plannotator.remote {
            vars.insert("PLANNOTATOR_REMOTE".to_owned(), remote);
        }
    }
    Spawn {
        cwd: Some(lead.cwd.to_path_buf()),
        env: vars,
        argv: [PROGRAM, "session", "run", lead.session.as_str()].map(str::to_owned).into(),
    }
}

/// Starts `lead` in a new window `window` of the cockpit session, starting the
/// server and the cockpit session when they are missing: the cockpit is then
/// created with this window as its first window, and `cpd tui` builds `main`
/// later (commands.md §4.1; Escalation `rs-plan-a1-q3`). Marks the pane with
/// `@cpd_session`, then records `tmux_pane` and appends `session-moved`
/// `{place: "parked"}` in one transaction.
pub(super) fn start(env: &mut Env, lead: &Lead, window: &str, command: &str) -> Result<Started, CpdError> {
    let tmux = Tmux::new(env.config.tmux.socket.clone());
    let cockpit = env.config.tmux.cockpit_session.clone();
    let spawn = spawn(env, lead);
    let created = if tmux.has_session(&cockpit)? { None } else { tmux.create_session(&cockpit, window, &spawn)? };
    let (window, pane) = match created {
        Some(ids) => ids,
        None => tmux.new_window(&cockpit, window, &spawn)?,
    };
    // A failing step now leaves a pane no row names: kill the window, keep the error (G11).
    let marked = (|| {
        tmux.set_pane_session(&pane, lead.session)?;
        tmux.set_pane_title(&pane, &title(env, lead)?)?;
        env.store.write(command, |tx| {
            tx.execute("UPDATE sessions SET tmux_pane = ?1 WHERE id = ?2", [pane.as_str(), lead.session.as_str()])?;
            let text = format!("Lead {} is parked in window {}.", lead.name, window.as_str());
            let data = json!({"pane": pane.as_str(), "window": window.as_str(), "place": "parked"});
            let mut event = NewEvent::new(Kind::SessionMoved, text, data).session(lead.session.clone()).context(lead.context.clone());
            if let Some(plan) = lead.plan {
                event = event.plan(plan.clone());
            }
            append(tx, event)?;
            Ok(())
        })
    })();
    if let Err(e) = marked {
        let _ = tmux.kill_window(&window);
        return Err(e);
    }
    Ok(Started { window, pane })
}

/// The header of `lead`'s pane: its Plan's title, else (a Context's Manager) its name.
fn title(env: &mut Env, lead: &Lead) -> Result<String, CpdError> {
    let Some(plan) = lead.plan else {
        return Ok(lead.name.to_owned());
    };
    env.store.read(|conn| Ok(conn.query_row("SELECT title FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0))?))
}

/// Sets `@cpd_title` to `title` on `session`'s live pane, if it has one.
pub(super) fn retitle(env: &Env, session: &SessionId, title: &str) -> Result<(), CpdError> {
    let tmux = Tmux::new(env.config.tmux.socket.clone());
    for pane in tmux.list_panes()?.iter().filter(|p| !p.dead && p.cpd_session.as_ref() == Some(session)) {
        tmux.set_pane_title(&pane.pane, title)?;
    }
    Ok(())
}

/// The window of the Session's latest `session-moved` Event, if any.
pub(super) fn last_window(conn: &rusqlite::Connection, session: &SessionId) -> Result<Option<String>, CpdError> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT json_extract(data, '$.window') FROM events WHERE session = ?1 AND kind = 'session-moved' ORDER BY id DESC LIMIT 1",
            [session.as_str()],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}
