//! E23, the PR post bump (contract §3.7): stub, the Step is not implemented yet.

use super::engine::Engine;

// Not on the tick yet: the implementer registers it in `Engine::tick` and removes this allow.
#[allow(dead_code)]
pub(super) fn run(_engine: &mut Engine) {
    todo!("E23 bump")
}

#[cfg(test)]
#[path = "bump_tests.rs"]
mod bump_tests;
