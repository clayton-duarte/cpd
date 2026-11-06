//! E24, the SWR step (contract §4.7; docs/architecture/swr.md §12): for every source except `github` it skips the
//! source while an endpoint is paused, fetches the keys `due` returns, drains the outbox through `execute`, and runs
//! `gc::sweep` at most once an hour (the last run is kept in engine memory). Stub: a no-op until its Job.

use std::time::Instant;

use super::engine::Engine;
#[cfg(test)]
use cpd_core::swr::source::Source;

/// What E24 remembers between ticks.
#[derive(Default)]
pub(super) struct State {
    /// When `gc::sweep` last ran in this engine's life.
    gc_last: Option<Instant>,
}

#[cfg(test)]
thread_local! {
    static SOURCES: std::cell::RefCell<Option<Vec<&'static dyn Source>>> =
        const { std::cell::RefCell::new(None) };
}

/// A test registers its own sources for this thread; E24 then runs these instead of `registry()`.
#[cfg(test)]
pub(super) fn set_sources_for_test(sources: Vec<&'static dyn Source>) {
    SOURCES.with(|s| *s.borrow_mut() = Some(sources));
}

pub(super) fn run(engine: &mut Engine) {
    // Stub: reads its state so the field is live; the implementation Job replaces this.
    let _ = engine.swr.gc_last;
}
