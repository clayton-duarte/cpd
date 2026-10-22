//! Record curation (hs-d549, hs-d551): the curations and retirement proposals the distiller writes, and the User's
//! decisions on them. Stubs: the behaviour is the next Job's.

use clap::Args;

use super::Select;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{ContextId, RecordId};
use crate::output::Output;
use crate::store::Store;

/// Why a record is proposed for retirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Duplicate,
    Superseded,
    Documented,
}

/// One record's curation: its topic and short form, and whether it is a documentation candidate (with the paths where
/// its id was found).
#[derive(Clone, Debug)]
pub struct Curation {
    pub record: RecordId,
    pub topic: String,
    pub short: String,
    pub doc_paths: Option<Vec<String>>,
}

/// A proposal to retire `record`; `target` is a record id or a doc path.
#[derive(Clone, Debug)]
pub struct Proposal {
    pub record: RecordId,
    pub reason: Reason,
    pub target: String,
}

/// Writes `curations` and pending retirements for `proposals` in one transaction. A record that already has a
/// retirement row (any status) gets no new proposal; a record whose doc_candidate is set keeps it.
pub fn propose(_store: &mut Store, _context: &ContextId, _curations: &[Curation], _proposals: &[Proposal]) -> Result<(), CpdError> {
    unimplemented!("curation::propose")
}

#[derive(Args, Debug)]
pub struct CurationArgs {
    #[command(flatten)]
    select: Select,
}

#[derive(Args, Debug)]
pub struct RetireArgs {
    #[command(flatten)]
    select: Select,
    /// The pending retirements to approve
    #[arg(long, value_name = "id,...", value_delimiter = ',')]
    approve: Vec<RecordId>,
    /// The pending retirements to reject
    #[arg(long, value_name = "id,...", value_delimiter = ',')]
    reject: Vec<RecordId>,
}

#[derive(Args, Debug)]
pub struct DocsArgs {
    #[command(flatten)]
    select: Select,
    /// The pending doc candidates to send to the Manager
    #[arg(long, value_name = "id,...", value_delimiter = ',')]
    send: Vec<RecordId>,
    /// The pending doc candidates to dismiss
    #[arg(long, value_name = "id,...", value_delimiter = ',')]
    dismiss: Vec<RecordId>,
}

fn unimplemented_command(name: &str) -> CpdError {
    CpdError::new(ErrorCode::Internal, format!("{name} is not implemented."))
}

pub(super) fn curation(_args: CurationArgs) -> Result<Output, CpdError> {
    Err(unimplemented_command("context curation"))
}

pub(super) fn retire(_args: RetireArgs) -> Result<Output, CpdError> {
    Err(unimplemented_command("context retire"))
}

pub(super) fn docs(_args: DocsArgs) -> Result<Output, CpdError> {
    Err(unimplemented_command("context docs"))
}
