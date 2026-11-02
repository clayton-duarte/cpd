//! `context archive` and `context unarchive` (commands.md §3.8, §3.9): the
//! cascade of the `plan` area over the Context's Plans, Sessions and Manager.

use super::{ArchiveArgs, ContextRow, UnarchiveArgs};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::plan::archive::{self, Objects};

pub(super) fn archive(args: ArchiveArgs) -> Result<Output, CpdError> {
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
    let Env {
        config, mut store, ..
    } = Env::open()?;
    let objects = store.read(|conn| {
        ContextRow::require(conn, &args.id)?;
        Objects::of_context(conn, &args.id)
    })?;
    archive::archive(&mut store, &config, "context archive", &objects)
}

pub(super) fn unarchive(args: UnarchiveArgs) -> Result<Output, CpdError> {
    let Env { mut store, .. } = Env::open()?;
    let objects = store.read(|conn| {
        ContextRow::require(conn, &args.id)?;
        Objects::of_context(conn, &args.id)
    })?;
    archive::unarchive(&mut store, "context unarchive", &objects, None)
}
