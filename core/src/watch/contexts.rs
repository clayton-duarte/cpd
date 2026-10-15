//! E6, Contexts (contract §3.7, §4.6): every `context_sync_seconds`, `context
//! sync` each Context store. A store with no remote syncs as a no-op (and its
//! import, which is idempotent). A sync failure is recorded by the sync itself
//! (`sync_failing` and one `error` Event, commands.md §3.6), so it is only
//! logged here.

use std::time::{Duration, Instant};

use super::engine::Engine;
use super::report::{About, Step};
use crate::context::{Contexts, Trigger, store_id, store_names};

pub(super) fn run(engine: &mut Engine) {
    let every = Duration::from_secs(u64::from(engine.config.watch.context_sync_seconds));
    if engine.synced.is_some_and(|at| at.elapsed() < every) {
        return;
    }
    engine.synced = Some(Instant::now());
    let names = match store_names(&engine.home) {
        Ok(names) => {
            engine.clear(Step::E6, &About::Machine);
            names
        }
        Err(e) => return engine.fail(Step::E6, About::Machine, &e),
    };
    for name in names {
        let id = match store_id(&engine.home, &name) {
            Ok(id) => id,
            Err(e) => {
                engine.log.line(&format!("E6: skipped contexts/{name}: {}", e.message()));
                continue;
            }
        };
        let result = Contexts::new(&engine.home, &engine.config, &mut engine.store).sync(&id, Trigger::Engine);
        match result {
            Ok(synced) if synced.pushed || synced.imported > 0 => {
                engine.log.line(&format!("E6: synced Context {id}: pushed {}, imported {}", synced.pushed, synced.imported));
            }
            Ok(_) => {}
            Err(e) => engine.log.line(&format!("E6: Context {id} did not sync: {} ({})", e.message(), e.code().as_str())),
        }
    }
}
