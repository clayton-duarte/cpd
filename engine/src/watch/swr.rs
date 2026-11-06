//! E24, the SWR step (contract §4.7; docs/architecture/swr.md §12): for every source except `github` it skips the
//! source while an endpoint is paused, fetches the keys `due` returns, drains the outbox through `execute`, and runs
//! `gc::sweep` at most once an hour (the last run is kept in engine memory).

use std::time::{Duration, Instant};

use serde_json::json;

use super::engine::Engine;
use super::report::{About, Failure, Step};
use cpd_core::error::CpdError;
use cpd_core::swr::outbox::{self, Retry};
use cpd_core::swr::source::{self, Cx, ExecOutcome, Source};
use cpd_core::swr::{endpoint, gc};

/// `gc::sweep` runs at most this often.
const GC_EVERY: Duration = Duration::from_secs(3600);

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

/// The sources E24 runs: the registry, or a test's own.
fn sources() -> Vec<&'static dyn Source> {
    #[cfg(test)]
    if let Some(sources) = SOURCES.with(|s| s.borrow().clone()) {
        return sources;
    }
    source::registry().to_vec()
}

fn failure(code: &str, message: String, details: serde_json::Value) -> Failure {
    Failure {
        code: code.to_owned(),
        message,
        details,
    }
}

pub(super) fn run(engine: &mut Engine) {
    let now = engine.ports.clock.utc();
    let mut clean = true;
    for source in sources() {
        // E5 runs `github` (its poll and its outbox); E24 never does.
        if source.system() == "github" {
            continue;
        }
        if let Err(e) = pass(engine, source, now) {
            clean = false;
            engine.fail(Step::E24, About::Machine, Failure::from(&e));
        }
    }
    let due = engine
        .swr
        .gc_last
        .is_none_or(|last| engine.ports.clock.now().saturating_duration_since(last) >= GC_EVERY);
    if due {
        engine.swr.gc_last = Some(engine.ports.clock.now());
        let ttl = Duration::from_secs(u64::from(engine.config.swr.gc_days) * 86_400);
        if let Err(e) = gc::sweep(&mut engine.store, now, ttl) {
            clean = false;
            engine.fail(Step::E24, About::Machine, Failure::from(&e));
        }
    }
    if clean {
        engine.clear(Step::E24, &About::Machine);
    }
}

/// One source for one pass: skip it while an endpoint is paused, drain its outbox, fetch its due keys, report an outage.
fn pass(
    engine: &mut Engine,
    source: &'static dyn Source,
    now: time::OffsetDateTime,
) -> Result<(), CpdError> {
    for name in source.endpoints() {
        if endpoint::get(&mut engine.store, name)?.is_some_and(|row| endpoint::is_paused(&row, now))
        {
            return Ok(());
        }
    }
    let mut cx = Cx {
        store: &mut engine.store,
        ports: &engine.ports,
        home: &engine.home,
        config: &engine.config,
    };
    let failed = drain_outbox(&mut cx, source, now)?;
    let keys = source.due(&mut cx, now)?;
    if !keys.is_empty() {
        source.fetch(&mut cx, &keys, now)?;
    }
    let mut outages = Vec::new();
    for name in source.endpoints() {
        if endpoint::due_report(cx.store, name, now)? {
            let error = endpoint::get(cx.store, name)?.and_then(|row| row.last_error);
            outages.push((*name, error));
        }
    }
    report_failed(engine, failed);
    for (name, error) in outages {
        engine.fail(
            Step::E24,
            About::Machine,
            failure(
                "swr_endpoint_down",
                format!(
                    "Endpoint {name} has been failing for 10 minutes: {}",
                    error.as_deref().unwrap_or("no error recorded")
                ),
                json!({"endpoint": name, "error": error}),
            ),
        );
    }
    Ok(())
}

/// Claims every due mutation of `source` and runs it through `execute`; the failed ones with their error.
fn drain_outbox(
    cx: &mut Cx<'_>,
    source: &'static dyn Source,
    now: time::OffsetDateTime,
) -> Result<Vec<(outbox::Mutation, String)>, CpdError> {
    let mut failed = Vec::new();
    while let Some(m) = outbox::claim_next(cx.store, now, source.system())? {
        match source.execute(cx, &m) {
            ExecOutcome::Done => outbox::complete(cx.store, m.id, now)?,
            ExecOutcome::Retry { after } => {
                outbox::fail(cx.store, m.id, now, "retry", Retry::After(after))?;
            }
            ExecOutcome::Failed { error } => {
                outbox::fail(cx.store, m.id, now, &error, Retry::Never)?;
                failed.push((m, error));
            }
        }
    }
    Ok(failed)
}

/// One `error` Event per failed mutation (swr.md §8).
fn report_failed(engine: &mut Engine, failed: Vec<(outbox::Mutation, String)>) {
    for (m, error) in failed {
        engine.fail(
            Step::E24,
            About::Machine,
            failure(
                "swr_mutation_failed",
                format!("Mutation {} ({} on {}) failed: {error}", m.id, m.op_kind, m.target_key),
                json!({"id": m.id, "system": m.system, "target_key": m.target_key, "op_kind": m.op_kind, "error": error}),
            ),
        );
    }
}

/// The `github` outbox drain (swr-github.md §9), run by E5's step each tick: a write command only enqueues, this calls GitHub through
/// the Runner. It does nothing while no mutation is due or the endpoint is paused.
pub(super) fn drain_github(engine: &mut Engine) {
    let now = engine.ports.clock.utc();
    let due = engine.store.read(|c| {
        Ok(c.query_row(
            "SELECT EXISTS (SELECT 1 FROM swr_mutations WHERE system = 'github' AND status = 'pending'
                              AND (next_attempt_at IS NULL OR next_attempt_at <= ?1))",
            [cpd_core::clock::timestamp(now)],
            |r| r.get::<_, bool>(0),
        )?)
    });
    if !matches!(due, Ok(true)) {
        return;
    }
    let paused = endpoint::get(&mut engine.store, "github")
        .ok()
        .flatten()
        .is_some_and(|row| endpoint::is_paused(&row, now));
    if paused {
        return;
    }
    static GITHUB: source::github::GithubSource = source::github::GithubSource {
        excused_labels: Vec::new(),
    };
    let mut cx = Cx {
        store: &mut engine.store,
        ports: &engine.ports,
        home: &engine.home,
        config: &engine.config,
    };
    match drain_outbox(&mut cx, &GITHUB, now) {
        Ok(failed) => report_failed(engine, failed),
        Err(e) => engine.fail(Step::E24, About::Machine, Failure::from(&e)),
    }
}
