//! `context manager` (commands.md §3.10, contract §3.1 "The Context's Manager"):
//! registers a Context's Manager and starts it in its parked window
//! `ctx-<context>`, or starts it again on its session file; never moves the
//! chat slot. `session run` starts pi (§10.8).

use std::path::Path;

use clap::Args;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::{ContextRow, Select};
use crate::config::WorkerRunner;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::host::Tmux;
use crate::host::layout::context_window;
use crate::ids::{ContextId, SessionId};
use crate::output::Output;
use crate::session::{lead_spawn, no_cockpit};
use crate::store::Store;

#[derive(Args, Debug)]
pub struct ManagerArgs {
    #[command(flatten)]
    select: Select,
    /// The User creates the Manager: it starts with no first message and runs no turn until the User's first prompt
    #[arg(long = "by-user")]
    by_user: bool,
}

/// The Context's live Manager, as `context manager` needs it.
struct Live {
    id: SessionId,
    name: String,
    cwd: String,
    state: String,
    pane: Option<String>,
    runner: String,
}

fn live(conn: &rusqlite::Connection, context: &ContextId) -> Result<Option<Live>, CpdError> {
    conn.query_row(
        "SELECT id, name, cwd, state, tmux_pane, runner FROM sessions
              WHERE context = ?1 AND role = 'manager' AND archived_at IS NULL",
        [context.as_str()],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        },
    )
    .optional()?
    .map(|(id, name, cwd, state, pane, runner)| {
        Ok::<_, CpdError>(Live {
            id: SessionId::parse(&id)?,
            name,
            cwd,
            state,
            pane,
            runner,
        })
    })
    .transpose()
}

/// The window of the Session's latest `session-moved` Event, if any.
fn last_window(
    conn: &rusqlite::Connection,
    session: &SessionId,
) -> Result<Option<String>, CpdError> {
    Ok(conn
        .query_row(
            "SELECT json_extract(data, '$.window') FROM events
              WHERE session = ?1 AND kind = 'session-moved' ORDER BY id DESC LIMIT 1",
            [session.as_str()],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

fn write_file(path: &Path, text: &str) -> Result<(), CpdError> {
    let failed = |e: std::io::Error| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("Cannot write {}: {e}.", path.display()),
        )
        .with("path", path.display().to_string())
    };
    std::fs::create_dir_all(
        path.parent()
            .expect("a file under $CPD_HOME has a directory"),
    )
    .map_err(failed)?;
    std::fs::write(path, text).map_err(failed)
}

/// `context manager` (§3.10).
pub(super) fn manager(args: ManagerArgs) -> Result<Output, CpdError> {
    let Env {
        home,
        config,
        mut store,
        ..
    } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    let (row, found) =
        store.read(|conn| Ok((ContextRow::require(conn, &context)?, live(conn, &context)?)))?;
    if row.archived_at.is_some() {
        return Err(CpdError::new(
            ErrorCode::Archived,
            format!("Context {context} is archived: unarchive it before starting its Manager."),
        )
        .with("context", context.as_str())
        .with("next", format!("cpd context unarchive {context}")));
    }
    // The runner of the Manager (contract §6.19, Q10): the stored one, or the config key for a fresh one. A durable
    // Manager has no pane; the engine's `hosts` step starts its host at the next tick.
    let runner = found.as_ref().map_or(config.manager_runner, |m| {
        WorkerRunner::parse(&m.runner).unwrap_or(config.manager_runner)
    });
    let durable = runner == WorkerRunner::Durable;
    let needs_pane = !durable
        && found
            .as_ref()
            .is_none_or(|m| m.pane.is_none() && m.state == "stopped");
    let tmux = Tmux::new(config.tmux.socket.clone());
    let cockpit = config.tmux.cockpit_session.clone();
    if needs_pane && !tmux.has_session(&cockpit)? {
        return Err(no_cockpit(&cockpit));
    }

    let (manager, registered) = match found {
        Some(manager) => (manager, false),
        None => (
            register(&home, &config, &mut store, &context, &row, args.by_user)?,
            true,
        ),
    };
    let mut started = registered;
    let mut pane = manager.pane.clone();
    let mut window = None;
    if needs_pane {
        let spawn = lead_spawn(&home, &manager.id, None, Path::new(&manager.cwd));
        let (new_window, new_pane) =
            tmux.new_window(&cockpit, &context_window(&context), &spawn)?;
        tmux.set_pane_session(&new_pane, &manager.id)?;
        store.write("context manager", |tx| {
            tx.execute(
                "UPDATE sessions SET tmux_pane = ?1 WHERE id = ?2",
                [new_pane.as_str(), manager.id.as_str()],
            )?;
            let text = format!(
                "Manager {} is parked in window {}.",
                manager.name,
                new_window.as_str()
            );
            let event = NewEvent::session_moved(
                text,
                Some(new_pane.as_str()),
                Some(new_window.as_str()),
                "parked",
            );
            append(
                tx,
                event.session(manager.id.clone()).context(context.clone()),
            )?;
            Ok(())
        })?;
        started = true;
        pane = Some(new_pane.as_str().to_owned());
        window = Some(new_window.as_str().to_owned());
    }
    let (state, window) = store.read(|conn| {
        let state: String = conn.query_row(
            "SELECT state FROM sessions WHERE id = ?1",
            [manager.id.as_str()],
            |r| r.get(0),
        )?;
        let window = match window {
            Some(window) => Some(window),
            None => last_window(conn, &manager.id)?,
        };
        Ok((state, window))
    })?;
    Ok(Output::success(json!({
        "manager": {"session": manager.id, "name": manager.name, "state": state, "pane": pane, "window": window},
        "started": started,
    })))
}

/// Writes the Manager's first message, then registers its `stopped` Session and
/// the `session-registered` Event in one transaction (§3.10).
fn register(
    home: &crate::home::CpdHome,
    config: &crate::config::Config,
    store: &mut Store,
    context: &ContextId,
    row: &ContextRow,
    by_user: bool,
) -> Result<Live, CpdError> {
    let cwd = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .map_err(|e| {
            CpdError::new(
                ErrorCode::SetupFailed,
                format!("Cannot resolve the current directory: {e}."),
            )
        })?
        .display()
        .to_string();
    if !by_user {
        write_file(
            &home.manager_message(context),
            &manager_message(context, &row.title),
        )?;
    }

    let id = SessionId::new_v4();
    let name = format!("{context}-manager");
    let tier = config.manager_tier.to_string();
    store.write("context manager", |tx| {
        tx.execute(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                                   state, state_label, state_at, created_at, by_user, runner)
             SELECT ?1, 'manager', ?2, NULL, NULL, ?3, NULL, NULL, ?4, ?5, NULL, 'stopped', NULL, at, at, ?6, ?7
               FROM txns WHERE id = cpd_txn()",
            rusqlite::params![id.as_str(), context.as_str(), name, cwd, tier, by_user, config.manager_runner.as_str()],
        )?;
        let text = format!("Manager {name} of Context {context} registered. Next: cpd session open {id}");
        let event = NewEvent::session_registered(text, "manager", &name, Value::Null, &tier);
        append(tx, event.session(id.clone()).context(context.clone()))?;
        Ok(())
    })?;
    Ok(Live {
        id,
        name,
        cwd,
        state: "stopped".to_owned(),
        pane: None,
        runner: config.manager_runner.as_str().to_owned(),
    })
}

/// A Manager's first message (commands.md §3.10): the header, then the pointer line. Nothing Context-wide is
/// pushed; the Manager reads it on demand or at its turn-start message (docs/research/context-retrieval-design.md
/// "First messages"). The one rendering `context manager` and `session first-message` share.
pub(crate) fn manager_message(context: &ContextId, title: &str) -> String {
    format!(
        "You are the Manager of Context {context} \"{title}\". Follow the cpd-manager skill.\n\n\
         Context on demand: cpd context search <query> --context {context} · cpd context show --context {context} --since <seq> · cpd context show --context {context} --record <id>\n"
    )
}
