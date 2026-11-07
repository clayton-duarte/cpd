//! `plan rename` (commands.md §4.9): the title only.

use rusqlite::OptionalExtension;
use serde_json::json;

use super::lead::retitle;
use super::{Env, RenameArgs};
use crate::error::CpdError;
use crate::events::{NewEvent, append};
use crate::ids::{ContextId, SessionId};
use crate::output::Output;

pub(super) fn rename(args: RenameArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let title = args.title.as_str();
    let mut env = Env::open()?;
    let (context, old, lead) = env
        .store
        .read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT context, title, lead_session FROM plans WHERE id = ?1",
                    [id.as_str()],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()?)
        })?
        .ok_or_else(|| CpdError::plan_not_found(&id))?;
    let context = ContextId::parse(&context)?;
    let mut out = json!({"plan": {"id": id, "context": context, "title": title}});
    // A Lead-less shaping Plan has no pane to retitle (contract §3.1, air-d19).
    let lead = lead.as_deref().map(SessionId::parse).transpose()?;
    if old == title {
        // A rerun repairs the pane's header too.
        if let Some(lead) = &lead {
            retitle(&mut env, lead, title)?;
        }
        out["already"] = json!(true);
        return Ok(Output::success(out));
    }
    env.store.write("plan rename", |tx| {
        tx.execute(
            "UPDATE plans SET title = ?1 WHERE id = ?2",
            [title, id.as_str()],
        )?;
        let text = format!("Plan {id} renamed to {title:?}.");
        let event = NewEvent::plan_renamed(text, title)
            .context(context.clone())
            .plan(id.clone());
        append(tx, event)?;
        Ok(())
    })?;
    if let Some(lead) = &lead {
        retitle(&mut env, lead, title)?;
    }
    Ok(Output::success(out))
}
