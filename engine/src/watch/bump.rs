//! E23, the PR post bump (contract §3.7, §4.4): decision pro-d386 stops it sending anything by itself. It stays in the engine
//! wiring so `incident_holds` and the Step list stay valid; its run only clears any old failure.

use super::engine::{Engine, incident_holds};
use super::report::{About, Step};

pub(super) fn run(engine: &mut Engine) {
    if incident_holds(engine, Step::E23) {
        engine.log.line("E23: holding: incident mode is on");
        return;
    }
    engine.clear(Step::E23, &About::Machine);
}

#[cfg(test)]
#[path = "bump_tests.rs"]
mod bump_tests;
