//! `session first-message` (commands.md §10.15, hs-d542 A3): the first message of a Plan's Lead or a Context's Manager,
//! rendered now with the functions `plan new` and `context manager` use, written to the file they write, and printed.

use rusqlite::OptionalExtension;
use serde_json::json;

use super::row::{self, Role};
use super::{Env, FirstMessageArgs, not_a_lead};
use crate::args::SessionArg;
use crate::context::{ContextRow, manager};
use crate::error::{CpdError, ErrorCode};
use crate::events::highest_id;
use crate::output::Output;
use crate::plan;

pub(super) fn first_message(args: FirstMessageArgs) -> Result<Output, CpdError> {
    let reference = SessionArg {
        session: args.session,
    }
    .require()?
    .clone();
    let mut env = Env::open()?;
    let id = env.resolve(&reference)?;
    let machine = env.config.machine.clone();
    let home = &env.home;
    let (session, text, path, event) = env.store.read(|conn| {
        let session = row::load(conn, &id)?;
        let (text, path) = match (&session.plan, session.role) {
            (Some(plan_id), Role::Lead) if session.plan_lead && session.forked_from.is_none() => {
                let plan = plan::row::find(conn, plan_id)?.ok_or_else(|| CpdError::plan_not_found(plan_id))?;
                let brief: Option<String> = conn
                    .query_row(
                        "SELECT text FROM context_records WHERE context = ?1 AND kind = 'brief' AND plan_id = ?2 AND plan_machine = ?3
                         ORDER BY seq LIMIT 1",
                        [plan.context.as_str(), plan.id.as_str(), machine.as_str()],
                        |r| r.get(0),
                    )
                    .optional()?;
                (plan::lead_message(&plan, brief.as_deref()), home.lead_message(plan_id))
            }
            (None, Role::Manager) => {
                let context = ContextRow::require(conn, &session.context)?;
                (manager::manager_message(&session.context, &context.title), home.manager_message(&session.context))
            }
            _ => return Err(not_a_lead(&session.id, session.role)),
        };
        Ok((session, text, path, highest_id(conn)?))
    })?;
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
    std::fs::write(&path, &text).map_err(failed)?;
    Ok(Output::success(json!({
        "session": session.id,
        "role": session.role.as_str(),
        "text": text,
        "file": path.display().to_string(),
        "event": event,
    })))
}
