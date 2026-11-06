//! E24, the SWR step (contract §4.7; docs/architecture/swr.md §12): for every source except `github` it skips the
//! source while an endpoint is paused, fetches the keys `due` returns, drains the outbox through `execute`, and runs
//! `gc::sweep` at most once an hour (the last run is kept in engine memory).

use std::time::{Duration, Instant};

use serde_json::json;

use super::engine::{Engine, incident_holds};
use super::report::{About, Failure, Step};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::swr::outbox::{self, Retry};
use cpd_core::swr::source::jira;
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

/// One source for one pass: skip it while an endpoint is paused or incident mode holds the step, drain its outbox, fetch its
/// due keys, report an outage. Incident mode holds the drain of every system and the Slack fetch alike (contract §3.7): both
/// are here, so no source adds its own hold.
fn pass(
    engine: &mut Engine,
    source: &'static dyn Source,
    now: time::OffsetDateTime,
) -> Result<(), CpdError> {
    if incident_holds(engine, Step::E24) {
        engine.log.line("E24: holding: incident mode is on");
        return Ok(());
    }
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
    let mut failed = Vec::new();
    let mut machine = Vec::new();
    if source.system() == "jira" {
        jira_drain(&mut cx, source, now, &mut machine)?;
    } else {
        while let Some(m) = outbox::claim_next(cx.store, now, source.system())? {
            match source.execute(&mut cx, &m) {
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
    }
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
    for failure in machine {
        engine.fail(Step::E24, About::Machine, failure);
    }
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

/// Drain the `jira` outbox (swr-jira.md §8, §9). `execute` settles what it moved or skipped itself (J5). A refusal is parked
/// with one plan-scoped `error` Event; any other failure retries after `watch.error_repeat_seconds`, and a missing sign-in
/// or server is one machine-wide failure, returned in `machine`. The helper runs inside the tick: its calls are bounded by
/// the 120 s deadline, and a test needs one pass to see its answer.
fn jira_drain(
    cx: &mut Cx<'_>,
    source: &'static dyn Source,
    now: time::OffsetDateTime,
    machine: &mut Vec<Failure>,
) -> Result<(), CpdError> {
    jira::reopen_refused(cx.store, cx.home, now)?;
    while let Some(m) = outbox::claim_next(cx.store, now, "jira")? {
        let ExecOutcome::Failed { error } = source.execute(cx, &m) else {
            continue;
        };
        let (code, rest) = error.split_once(": ").unwrap_or((&error, ""));
        if code == ErrorCode::JiraRefused.as_str() {
            let name = jira::target_name(cx.store, cx.home, &m)?;
            jira::park_refused(cx.store, &m, now, super::SOURCE, rest, &name)?;
            continue;
        }
        outbox::fail(
            cx.store,
            m.id,
            now,
            &error,
            Retry::After(cx.config.watch.error_repeat()),
        )?;
        if code == ErrorCode::JiraAuth.as_str() || code == ErrorCode::JiraNotConfigured.as_str() {
            machine.push(failure(code, rest.to_owned(), json!({"id": m.id})));
            // This Event is the report of the outage: the endpoint's own (`swr_endpoint_down`) would repeat it.
            endpoint::due_report(cx.store, "jira", now)?;
        }
    }
    jira::mark_inactive(cx.store, now)
}
