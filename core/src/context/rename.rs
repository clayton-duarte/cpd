//! `context rename` (commands.md §3.7): the title only, in the database.

use serde_json::json;

use super::{ContextRow, RenameArgs};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::output::Output;

pub(super) fn rename(args: RenameArgs) -> Result<Output, CpdError> {
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
    let Env { mut store, .. } = Env::open()?;
    let (id, title) = (&args.id, args.title.as_str());
    let row = store.read(|conn| ContextRow::require(conn, id))?;
    let mut out = json!({"context": {"id": id, "title": title}});
    if row.title == title {
        out["already"] = json!(true);
        return Ok(Output::success(out));
    }
    store.write("context rename", |tx| {
        tx.execute(
            "UPDATE contexts SET title = ?1 WHERE id = ?2",
            [title, id.as_str()],
        )?;
        let text = format!("Context {id} renamed to {title:?}.");
        append(
            tx,
            NewEvent::context_renamed(text, title).context(id.clone()),
        )?;
        Ok(())
    })?;
    Ok(Output::success(out))
}
