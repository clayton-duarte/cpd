//! The Permanent Contexts' ensure step (commands.md §3.8; decision pro-d342).

use crate::config::Config;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::{PermanentContext, SessionId};
use crate::store::Store;

/// Idempotent: the Context row, and a live Manager. Returns the Manager's id.
pub fn ensure(
    _home: &CpdHome,
    _config: &Config,
    _store: &mut Store,
    _context: PermanentContext,
) -> Result<SessionId, CpdError> {
    todo!("perm-impl")
}

/// [`ensure`] for each of [`PermanentContext::ALL`], in order.
pub fn ensure_all(
    _home: &CpdHome,
    _config: &Config,
    _store: &mut Store,
) -> Result<Vec<SessionId>, CpdError> {
    todo!("perm-impl")
}
