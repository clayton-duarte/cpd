//! E24, the SWR step (contract §4.7; docs/architecture/swr.md §12): for every source except `github` it skips the
//! source while an endpoint is paused, fetches the keys `due` returns, drains the outbox through `execute`, and runs
//! `gc::sweep` at most once an hour (the last run is kept in engine memory).

use std::time::{Duration, Instant};

use serde_json::json;

use super::engine::Engine;
use super::report::{About, Failure, Step};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{self, NewEvent};
use cpd_core::ids::{ContextId, PlanId, SessionId};
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
    let mut machine = Vec::new();
    let failed = if source.system() == "jira" {
        jira_drain(&mut cx, source, now, &mut machine)?;
        Vec::new()
    } else {
        drain_outbox(&mut cx, source, now)?
    };
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
    for failure in machine {
        engine.fail(Step::E24, About::Machine, failure);
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
    wake_review_leads(engine);
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

/// Guarantee (b) of swr-github.md §10: wakes the Review Lead of every claimed pull request whose cached `head_oid` differs from the
/// claimed one while its cached `checks` are `passed`, once per head: the wake is a `session-message` Event (`from` null, `wake`
/// true, `data.head_oid`) and the same transaction moves the item's `head_oid` to the woken head, so that head wakes no more.
pub(super) fn wake_review_leads(engine: &mut Engine) {
    if let Err(e) = wake(engine) {
        engine.fail(Step::E24, About::Machine, Failure::from(&e));
    }
}

/// One claimed review item whose pull request moved to a green head.
struct Moved {
    item: i64,
    plan: String,
    lead: String,
    context: String,
    url: String,
    claimed: String,
    head: String,
}

fn wake(engine: &mut Engine) -> Result<(), CpdError> {
    let moved = engine.store.read(|c| {
        let mut stmt = c.prepare(
            "SELECT i.id, pl.id, pl.lead_session, pl.context, p.url, i.head_oid, p.head_oid
               FROM inbox_items i
               JOIN plans pl ON pl.id = i.plan
               JOIN pull_requests p ON p.repo = pl.reviewed_repo AND p.number = pl.reviewed_number
              WHERE i.claimed_at IS NOT NULL AND i.closed_at IS NULL AND i.head_oid IS NOT NULL
                AND pl.archived_at IS NULL AND pl.deleted_at IS NULL AND pl.lead_session IS NOT NULL
                AND p.state = 'open' AND p.checks = 'passed'
                AND p.head_oid IS NOT NULL AND p.head_oid <> i.head_oid
              ORDER BY i.id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Moved {
                    item: r.get(0)?,
                    plan: r.get(1)?,
                    lead: r.get(2)?,
                    context: r.get(3)?,
                    url: r.get(4)?,
                    claimed: r.get(5)?,
                    head: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    for m in moved {
        engine.store.write(super::SOURCE, |tx| {
            let moved = tx.execute(
                "UPDATE inbox_items SET head_oid = ?2 WHERE id = ?1 AND head_oid <> ?2",
                rusqlite::params![m.item, m.head],
            )?;
            if moved == 0 {
                return Ok(());
            }
            let text = format!(
                "re-review requested: {url}\nreviewed_oid: {claimed}\nhead_oid: {head}\n\
                 The pull request moved and its checks are green. Please re-review {claimed}..{head}.",
                url = m.url,
                claimed = m.claimed,
                head = m.head,
            );
            let preview: String = text.chars().take(120).collect();
            let event = NewEvent::session_message_head(
                format!("Message from the User: {preview}"),
                &text,
                &m.head,
            )?
            .plan(PlanId::parse(&m.plan)?)
            .session(SessionId::parse(&m.lead)?)
            .context(ContextId::parse(&m.context)?);
            events::append(tx, event)?;
            Ok(())
        })?;
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
