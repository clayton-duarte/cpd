//! The Permanent Contexts' ensure step (commands.md §3.8; decision pro-d342).

use rusqlite::{OptionalExtension, params};

use super::manager::{insert, live, prepare};
use crate::clock::now;
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, NewEvent};
use crate::home::CpdHome;
use crate::ids::{ContextId, PermanentContext, SessionId};
use crate::policy::FALLBACK_OWNER;
use crate::store::Store;

/// Idempotent: the Context row, and a live Manager. Returns the Manager's id.
pub fn ensure(
    home: &CpdHome,
    config: &Config,
    store: &mut Store,
    context: PermanentContext,
) -> Result<SessionId, CpdError> {
    let id = ContextId::parse(context.id())?;
    // (deleted_at set, archived_at set) of the row, if it exists; and the live Manager.
    let (row, found) = store.read(|conn| {
        let row: Option<(bool, bool)> = conn
            .query_row(
                "SELECT deleted_at IS NOT NULL, archived_at IS NOT NULL FROM contexts WHERE id = ?1",
                [id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok((row, live(conn, &id)?))
    })?;
    if let Some((true, _)) = row {
        return Err(CpdError::new(
            ErrorCode::ContextExists,
            format!("Context {id} was deleted: its id is not reused."),
        )
        .with("context", id.as_str()));
    }
    if let (Some((_, false)), Some(manager)) = (row, &found) {
        return Ok(manager.id.clone());
    }
    let prepared = match &found {
        Some(_) => None,
        None => Some(prepare(home, config, &id, context.title(), false)?),
    };
    let machine = config.machine.clone();
    store.write("context manager", |tx| {
        match row {
            None => {
                tx.execute(
                    "INSERT INTO contexts (id, title, jira, created_at, created_on, owner) VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
                    params![id.as_str(), context.title(), now(), machine.as_str(), FALLBACK_OWNER],
                )?;
                events::append(
                    tx,
                    NewEvent::context_created(
                        format!(
                            "Context {id} \"{}\" exists. Next: cpd context show --context {id}",
                            context.title()
                        ),
                        context.title(),
                        None,
                        machine.as_str(),
                    )
                    .context(id.clone()),
                )?;
            }
            Some((_, true)) => {
                tx.execute("UPDATE contexts SET archived_at = NULL WHERE id = ?1", [id.as_str()])?;
            }
            Some((_, false)) => {}
        }
        if let Some(p) = &prepared {
            insert(tx, &id, p, false, config.manager_runner)?;
        }
        Ok(())
    })?;
    match (found, prepared) {
        (Some(m), _) => Ok(m.id),
        (None, Some(p)) => Ok(p.id()),
        (None, None) => unreachable!("a missing Manager is always prepared"),
    }
}

/// [`ensure`] for each of [`PermanentContext::ALL`], in order.
pub fn ensure_all(
    home: &CpdHome,
    config: &Config,
    store: &mut Store,
) -> Result<Vec<SessionId>, CpdError> {
    PermanentContext::ALL
        .into_iter()
        .map(|context| ensure(home, config, store, context))
        .collect()
}
