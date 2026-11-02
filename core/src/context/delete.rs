//! `context delete` (commands.md §3.14; contract §2.2 "Deleted", hs-d527): the Context's Plans as `plan delete` deletes
//! them (its pieces are reused), then its records and its Manager. The `contexts` row and the Plans' rows stay, as
//! tombstones.

use rusqlite::OptionalExtension;
use serde_json::json;

use super::DeleteArgs;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{Counts, NewEvent, append};
use crate::host::Tmux;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::Output;
use crate::plan::archive::{Objects, refuse_chat_slot};
use crate::plan::delete::{
    Inventory, Targets, blockers, delete_rows, finish, megabytes, rerun, tombstone,
};
use crate::session::archive_blocked;

const COMMAND: &str = "context delete";

/// The Plans of `context` (`live` only: those not deleted) as ids.
fn plans(
    conn: &rusqlite::Connection,
    context: &ContextId,
    live: bool,
) -> Result<Vec<PlanId>, CpdError> {
    let filter = if live { "AND deleted_at IS NULL" } else { "" };
    let mut stmt = conn.prepare(&format!(
        "SELECT id FROM plans WHERE context = ?1 {filter} ORDER BY created_at, id"
    ))?;
    let ids = stmt
        .query_map([context.as_str()], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.iter().map(|id| PlanId::parse(id)).collect()
}

/// Where the delete's files are: every Plan's (tombstoned ones too), the Managers' Session files, the digests.
fn targets_of(
    conn: &rusqlite::Connection,
    home: &crate::home::CpdHome,
    context: &ContextId,
) -> Result<Targets, CpdError> {
    let mut targets = Targets::of_managers(conn, context)?;
    for plan in plans(conn, context, false)? {
        targets.extend(Targets::of_plan(conn, home, &plan)?);
    }
    targets.push_dir(home.context_digest_dir(context));
    Ok(targets)
}

/// `context delete` (§3.14).
pub(super) fn delete(args: DeleteArgs) -> Result<Output, CpdError> {
    if args.id.permanent().is_some() {
        return Err(CpdError::new(
            ErrorCode::ContextPermanent,
            format!(
                "Context {} is permanent and is never archived, renamed or deleted.",
                args.id
            ),
        )
        .with("context", args.id.as_str()));
    }
    let id = args.id;
    let Env {
        home,
        config,
        mut store,
        ..
    } = Env::open()?;
    let deleted: Option<Option<String>> = store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT deleted_at FROM contexts WHERE id = ?1",
                [id.as_str()],
                |r| r.get(0),
            )
            .optional()?)
    })?;
    match deleted {
        None => Err(
            CpdError::new(ErrorCode::ContextNotFound, format!("No Context {id}."))
                .with("context", id.as_str()),
        ),
        Some(Some(_)) => rerun(&store.read(|conn| targets_of(conn, &home, &id))?, args.yes),
        Some(None) => {
            let (archived, live_plans, managers, objects) = store.read(|conn| {
                let archived: bool = conn.query_row(
                    "SELECT archived_at IS NOT NULL FROM contexts WHERE id = ?1",
                    [id.as_str()],
                    |r| r.get(0),
                )?;
                let mut stmt = conn
                    .prepare("SELECT id FROM sessions WHERE context = ?1 AND role = 'manager'")?;
                let managers = stmt
                    .query_map([id.as_str()], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                    .iter()
                    .map(|m| SessionId::parse(m))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((
                    archived,
                    plans(conn, &id, true)?,
                    managers,
                    Objects::of_context(conn, &id)?,
                ))
            })?;
            let check =
                |conn: &rusqlite::Connection| blockers(conn, &live_plans, &managers, !archived);
            let live = store.read(|conn| check(conn))?;
            if !live.is_empty() {
                return Err(archive_blocked(&live, None));
            }
            let tmux = Tmux::new(config.tmux.socket.clone());
            refuse_chat_slot(
                &mut store,
                &tmux,
                &config.tmux.cockpit_session,
                &objects.sessions,
            )?;

            let (per_plan, sessions, targets) = store.read(|conn| {
                let mut per_plan = Vec::new();
                for plan in &live_plans {
                    per_plan.push((plan.clone(), Inventory::of_plan(conn, plan)?));
                }
                let sessions: i64 = conn.query_row(
                    "SELECT count(*) FROM sessions WHERE context = ?1",
                    [id.as_str()],
                    |r| r.get(0),
                )?;
                let sessions = sessions as u64;
                Ok((per_plan, sessions, targets_of(conn, &home, &id)?))
            })?;
            let (worktrees, bytes) = targets.measure();
            let counts = Counts {
                jobs: per_plan.iter().map(|(_, i)| i.jobs).sum(),
                attempts: per_plan.iter().map(|(_, i)| i.attempts).sum(),
                sessions,
                worktrees,
                disk_freed_mb: megabytes(bytes),
            };
            let out = json!({
                "preview": !args.yes, "already": false,
                "plans": per_plan.iter().map(|(p, i)| json!({"id": p, "jobs": i.jobs, "attempts": i.attempts})).collect::<Vec<_>>(),
                "jobs": counts.jobs, "attempts": counts.attempts, "sessions": counts.sessions,
                "worktrees": counts.worktrees, "disk_freed_mb": counts.disk_freed_mb,
            });
            if !args.yes {
                return Ok(Output::success(out));
            }

            store.write(COMMAND, |tx| {
                let live = check(tx)?;
                if !live.is_empty() {
                    return Err(archive_blocked(&live, None));
                }
                for plan in &live_plans {
                    tombstone(tx, "plans", plan.as_str())?;
                }
                tombstone(tx, "contexts", id.as_str())?;
                // The rows of the whole Context first (an Inbox item points to an Event), then each Plan's, then
                // the records and the Managers.
                for sql in [
                    "DELETE FROM board_posts WHERE to_session IN (SELECT id FROM sessions WHERE context = ?1)",
                    "DELETE FROM inbox_items WHERE context = ?1",
                    "DELETE FROM events WHERE context = ?1",
                ] {
                    tx.execute(sql, [id.as_str()])?;
                }
                for plan in &live_plans {
                    delete_rows(tx, plan)?;
                }
                tx.execute("DELETE FROM context_records WHERE context = ?1", [id.as_str()])?;
                tx.execute("DELETE FROM sessions WHERE context = ?1 AND role = 'manager'", [id.as_str()])?;
                let text = format!(
                    "Context {id} deleted: {} Plans, {} Jobs, {} Attempts, {} Sessions, {} worktrees, ~{} MB.",
                    live_plans.len(),
                    counts.jobs,
                    counts.attempts,
                    counts.sessions,
                    counts.worktrees,
                    counts.disk_freed_mb
                );
                append(tx, NewEvent::context_deleted(text, live_plans.len() as u64, &counts).context(id.clone()))?;
                Ok(())
            })?;
            // The paths again, now from `history`: where a rerun reads them too.
            finish(&store.read(|conn| targets_of(conn, &home, &id))?)?;
            Ok(Output::success(out))
        }
    }
}
