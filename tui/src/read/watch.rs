//! The commit watcher of the store's directory.

use std::path::Path;

use cpd_core::error::{CpdError, ErrorCode};
use notify::event::ModifyKind;
use notify::{EventKind, RecursiveMode, Watcher};

use crate::pump::Waker;

/// True for a data change, create or remove of `cpd.db` or `cpd.db-wal`.
/// inotify reports a WAL `write` as `Modify(Data(_))`; other backends give `Modify(Any)`.
pub fn is_commit(event: &notify::Event) -> bool {
    let kind = matches!(
        event.kind,
        EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Any)
            | EventKind::Create(_)
            | EventKind::Remove(_)
    );
    kind && event.paths.iter().any(|p| {
        p.file_name()
            .is_some_and(|n| n == "cpd.db" || n == "cpd.db-wal")
    })
}

/// One non-recursive watcher on the store's directory; dropping it stops it.
pub struct Watch {
    _watcher: notify::RecommendedWatcher,
}

impl Watch {
    pub fn start(dir: &Path, waker: Waker) -> Result<Watch, CpdError> {
        let fail = |e: notify::Error| {
            CpdError::new(
                ErrorCode::StoreFailed,
                format!("Cannot watch {}: {e}.", dir.display()),
            )
        };
        // An `Err` event is not a store fact: the safety read covers a lost event.
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok_and(|e| is_commit(&e)) {
                    waker.commit();
                }
            })
            .map_err(fail)?;
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(fail)?;
        Ok(Watch { _watcher: watcher })
    }
}
