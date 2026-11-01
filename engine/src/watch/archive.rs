//! E12, the Worker archive (contract §3.7, §2.2 "Archived"; hs-d516): every stopped Worker Session whose Attempt is
//! `ended` is archived, once, with one `session-archived` Event. Idempotent: it archives the existing ones on its first tick.

use super::engine::Engine;
use super::report::{About, Step};
use cpd_core::session::archive_ended_workers;

pub(super) fn run(engine: &mut Engine) {
    match archive_ended_workers(&mut engine.store) {
        Ok(done) => {
            if !done.is_empty() {
                engine
                    .log
                    .line(&format!("E12: archived {} Worker Session(s)", done.len()));
            }
            engine.clear(Step::E12, &About::Machine);
        }
        Err(e) => engine.fail(Step::E12, About::Machine, &e),
    }
}
