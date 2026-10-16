//! `context archive` and `context unarchive` (commands.md §3.8, §3.9): the
//! cascade of the `plan` area over the Context's Plans, Sessions and grounded Lead.

use super::{ArchiveArgs, ContextRow, UnarchiveArgs, open};
use crate::error::CpdError;
use crate::output::Output;
use crate::plan::archive::{self, Objects};

pub(super) fn archive(args: ArchiveArgs) -> Result<Output, CpdError> {
    let (_, config, mut store) = open()?;
    let objects = store.read(|conn| {
        ContextRow::require(conn, &args.id)?;
        Objects::of_context(conn, &args.id)
    })?;
    archive::archive(&mut store, &config, "context archive", &objects)
}

pub(super) fn unarchive(args: UnarchiveArgs) -> Result<Output, CpdError> {
    let (_, _, mut store) = open()?;
    let objects = store.read(|conn| {
        ContextRow::require(conn, &args.id)?;
        Objects::of_context(conn, &args.id)
    })?;
    archive::unarchive(&mut store, "context unarchive", &objects, None)
}
