//! The commit watcher of the store's directory. Stub: the tests of `watch_tests` are red against it.
#![allow(dead_code, unused_variables)]

use std::path::Path;

use cpd_core::error::CpdError;

use crate::pump::Waker;

/// True for a data change, create or remove of `cpd.db` or `cpd.db-wal`.
pub fn is_commit(event: &notify::Event) -> bool {
    todo!()
}

/// One non-recursive watcher on the store's directory; dropping it stops it.
pub struct Watch {}

impl Watch {
    pub fn start(dir: &Path, waker: Waker) -> Result<Watch, CpdError> {
        todo!()
    }
}
