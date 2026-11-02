//! E22, Permanent Contexts (contract §3.7 step E22; pro-d342): every tick ensures the Contexts `cpd`, `host` and
//! `codereview` and a live Manager for each. Each is ensured on its own, so one refusal does not stop the others;
//! the first error is reported once after the loop, and the step clears only when all three succeeded.

use super::engine::Engine;
use super::report::{About, Step};
use cpd_core::context::permanent::ensure;
use cpd_core::ids::PermanentContext;

pub(super) fn run(engine: &mut Engine) {
    let mut first = None;
    for context in PermanentContext::ALL {
        if let Err(e) = ensure(&engine.home, &engine.config, &mut engine.store, context) {
            first.get_or_insert(e);
        }
    }
    match first {
        Some(e) => engine.fail(Step::E22, About::Machine, &e),
        None => engine.clear(Step::E22, &About::Machine),
    }
}
