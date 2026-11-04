//! `context manager` (commands.md §3.10, contract §3.1 "The Context's Manager"):
//! registers a Context's Manager and starts it in its parked window
//! `ctx-<context>`, or starts it again on its session file; never moves the
//! chat slot. `session run` starts pi (§10.8).

use std::path::Path;

use clap::Args;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::{ContextRow, Select};
use crate::agent_key::AgentKey;
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
    /// The Manager's runner; default the config key `manager_runner`
    #[arg(long, value_name = "pi|durable")]
    runner: Option<WorkerRunner>,
}

/// The Context's live Manager, as `context manager` needs it.
pub(crate) struct Live {
    pub(crate) id: SessionId,
    name: String,
    cwd: String,
    state: String,
    pane: Option<String>,
    runner: String,
}

pub(crate) fn live(
    conn: &rusqlite::Connection,
    context: &ContextId,
) -> Result<Option<Live>, CpdError> {
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
    if let Some(permanent) = context.permanent() {
        super::permanent::ensure(&home, &config, &mut store, permanent)?;
    }
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
    // Manager has no pane; the engine's `hosts` step starts its host at the next tick. An existing Manager's runner
    // wins: a `--runner` that differs is refused before any write.
    let runner = match &found {
        Some(m) => {
            let stored = WorkerRunner::parse(&m.runner).unwrap_or(config.manager_runner);
            if let Some(wanted) = args.runner
                && wanted != stored
            {
                return Err(CpdError::new(
                        ErrorCode::Usage,
                        format!(
                            "Manager {} of Context {context} runs on the {} runner; --runner {} differs.",
                            m.name,
                            stored.as_str(),
                            wanted.as_str()
                        ),
                    )
                    .with("context", context.as_str())
                    .with("runner", stored.as_str()));
            }
            stored
        }
        None => args.runner.unwrap_or(config.manager_runner),
    };
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
            register(
                &home,
                &config,
                &mut store,
                &context,
                &row,
                args.by_user,
                runner,
            )?,
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
pub(crate) fn register(
    home: &crate::home::CpdHome,
    config: &crate::config::Config,
    store: &mut Store,
    context: &ContextId,
    row: &ContextRow,
    by_user: bool,
    runner: WorkerRunner,
) -> Result<Live, CpdError> {
    let topics = store.read(|conn| crate::context::curated::topic_counts(conn, context))?;
    let prepared = prepare_with(home, config, context, &row.title, by_user, &topics)?;
    store.write("context manager", |tx| {
        insert(tx, context, &prepared, by_user, runner)
    })?;
    Ok(prepared.into_live(runner))
}

/// What `register` computes before its transaction: the cwd, the id and the name; the first message is written.
pub(crate) struct Prepared {
    id: SessionId,
    name: String,
    cwd: String,
    tier: String,
}

impl Prepared {
    pub(crate) fn id(&self) -> SessionId {
        self.id.clone()
    }

    fn into_live(self, runner: WorkerRunner) -> Live {
        Live {
            id: self.id,
            name: self.name,
            cwd: self.cwd,
            state: "stopped".to_owned(),
            pane: None,
            runner: runner.as_str().to_owned(),
        }
    }
}

/// `prepare` for a Context with no curated topics (a Context just created).
pub(crate) fn prepare(
    home: &crate::home::CpdHome,
    config: &crate::config::Config,
    context: &ContextId,
    title: &str,
    by_user: bool,
) -> Result<Prepared, CpdError> {
    prepare_with(home, config, context, title, by_user, &[])
}

/// The cwd, id and name of a new Manager, and its first message written unless `by_user`; `topics` are the
/// curated topics in force, read once by the caller.
pub(crate) fn prepare_with(
    home: &crate::home::CpdHome,
    config: &crate::config::Config,
    context: &ContextId,
    title: &str,
    by_user: bool,
    topics: &[(String, usize)],
) -> Result<Prepared, CpdError> {
    let cwd = crate::session::agent_cwd()?;
    if !by_user {
        write_file(
            &home.manager_message(context),
            &manager_message(context, title, topics),
        )?;
    }
    Ok(Prepared {
        id: SessionId::new_v4(),
        name: AgentKey::manager(context.clone()).to_string(),
        cwd,
        tier: config.manager_tier.to_string(),
    })
}

/// The `sessions` row and `session-registered` Event of a prepared Manager, inside `tx`.
pub(crate) fn insert(
    tx: &crate::store::WriteTxn<'_>,
    context: &ContextId,
    p: &Prepared,
    by_user: bool,
    runner: WorkerRunner,
) -> Result<(), CpdError> {
    let Prepared {
        id,
        name,
        cwd,
        tier,
    } = p;
    tx.execute(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                               state, state_label, state_at, created_at, by_user, runner)
         SELECT ?1, 'manager', ?2, NULL, NULL, ?3, NULL, NULL, ?4, ?5, NULL, 'stopped', NULL, at, at, ?6, ?7
           FROM txns WHERE id = cpd_txn()",
        rusqlite::params![id.as_str(), context.as_str(), name, cwd, tier, by_user, runner.as_str()],
    )?;
    let text =
        format!("Manager {name} of Context {context} registered. Next: cpd session open {id}");
    let event = NewEvent::session_registered(text, "manager", name, Value::Null, tier);
    append(tx, event.session(id.clone()).context(context.clone()))?;
    Ok(())
}

/// A Manager's first message (commands.md §3.10): the header, the `Topics:` line when `topics` is not empty,
/// then the pointer line. No store read: the caller passes the topics. Nothing Context-wide is
/// pushed; the Manager reads it on demand or at its turn-start message (docs/research/context-retrieval-design.md
/// "First messages"). The one rendering `context manager` and `session first-message` share.
pub(crate) fn manager_message(
    context: &ContextId,
    title: &str,
    topics: &[(String, usize)],
) -> String {
    let topics = if topics.is_empty() {
        String::new()
    } else {
        format!(
            "Topics: {}\n\n",
            crate::context::curated::topics_line(topics)
        )
    };
    format!(
        "You are the Manager of Context {context} \"{title}\". Follow the cpd-manager skill.\n\n{topics}\
         Context on demand: cpd context search <query> --context {context} · cpd context show --context {context} --since <seq> · cpd context show --context {context} --record <id>\n"
    )
}
