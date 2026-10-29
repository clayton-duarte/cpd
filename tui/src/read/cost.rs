//! The cost read model's cache and total (stub: red tests first).

use std::path::Path;
use std::time::SystemTime;

use cpd_core::error::CpdError;

/// Costs of session files by path, valid while the file's `(mtime, len)` stamp is unchanged.
pub(crate) struct CostCache;

impl CostCache {
    pub(crate) fn new() -> CostCache {
        todo!()
    }

    pub(crate) fn cost(
        &mut self,
        _path: &Path,
        _stat: impl FnOnce(&Path) -> Option<(SystemTime, u64)>,
        _parse: impl FnOnce(&Path) -> Result<f64, CpdError>,
    ) -> Option<f64> {
        todo!()
    }
}

/// The own cost of the session file at `path`, through the process-wide cache.
#[allow(dead_code)]
pub(crate) fn own_cost(_path: &Path) -> Option<f64> {
    todo!()
}

/// `None` when every part is `None`, else the sum of the parts that have a value.
pub(crate) fn total(_parts: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    todo!()
}
