//! E24, the SWR step (contract §4.7; docs/architecture/swr.md §12): a paused endpoint skips its source, due keys are
//! fetched once per pass, pending mutations drain through `execute`, `gc::sweep` runs at most hourly, an outage raises
//! one `error` Event after ten minutes, and the `github` source is never run by E24. The sources are test probes
//! registered through [`swr::set_sources_for_test`]; the clock is a `ManualClock` and no `gh` or helper runs.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;
use time::OffsetDateTime;

use super::swr;
use super::testkit::Fx;
use cpd_core::clock::timestamp;
use cpd_core::ports::Fakes;
use cpd_core::swr::cache;
use cpd_core::swr::endpoint::{self, Kind};
use cpd_core::swr::outbox::{self, Coalesce, Mutation, Op};
use cpd_core::swr::source::{Cx, ExecOutcome, QueryKey, Source};

/// What a probe was asked to do.
#[derive(Default)]
struct Log {
    due_calls: usize,
    /// `(keys, poll_started_at)` of each `fetch`.
    fetches: Vec<(Vec<String>, OffsetDateTime)>,
    /// The id of each executed mutation, in order.
    executed: Vec<i64>,
}

/// A test source: it returns the keys it was given, records its calls and answers `execute` from a script.
struct Probe {
    system: &'static str,
    endpoints: &'static [&'static str],
    due: Vec<String>,
    /// Each `fetch` records a transient failure of the first endpoint.
    failing: bool,
    outcomes: Mutex<VecDeque<ExecOutcome>>,
    log: Mutex<Log>,
}

impl Probe {
    /// A probe of `system` on endpoint `endpoint` whose `due` is `keys`.
    fn leak(
        system: &'static str,
        endpoint: &'static [&'static str],
        keys: &[&str],
    ) -> &'static Probe {
        Box::leak(Box::new(Probe {
            system,
            endpoints: endpoint,
            due: keys.iter().map(|k| (*k).to_owned()).collect(),
            failing: false,
            outcomes: Mutex::default(),
            log: Mutex::default(),
        }))
    }

    fn failing(
        system: &'static str,
        endpoint: &'static [&'static str],
        keys: &[&str],
    ) -> &'static Probe {
        Box::leak(Box::new(Probe {
            system,
            endpoints: endpoint,
            due: keys.iter().map(|k| (*k).to_owned()).collect(),
            failing: true,
            outcomes: Mutex::default(),
            log: Mutex::default(),
        }))
    }

    fn script(&self, outcomes: Vec<ExecOutcome>) {
        *self.outcomes.lock().unwrap() = outcomes.into();
    }

    fn fetches(&self) -> Vec<(Vec<String>, OffsetDateTime)> {
        self.log.lock().unwrap().fetches.clone()
    }

    fn executed(&self) -> Vec<i64> {
        self.log.lock().unwrap().executed.clone()
    }

    fn due_calls(&self) -> usize {
        self.log.lock().unwrap().due_calls
    }
}

impl Source for Probe {
    fn system(&self) -> &'static str {
        self.system
    }
    fn endpoints(&self) -> &'static [&'static str] {
        self.endpoints
    }
    fn due(
        &self,
        _cx: &mut Cx<'_>,
        _now: OffsetDateTime,
    ) -> Result<Vec<QueryKey>, cpd_core::error::CpdError> {
        self.log.lock().unwrap().due_calls += 1;
        Ok(self.due.clone())
    }
    fn fetch(
        &self,
        cx: &mut Cx<'_>,
        keys: &[QueryKey],
        poll_started_at: OffsetDateTime,
    ) -> Result<(), cpd_core::error::CpdError> {
        self.log
            .lock()
            .unwrap()
            .fetches
            .push((keys.to_vec(), poll_started_at));
        if self.failing {
            endpoint::record_failure(
                cx.store,
                self.endpoints[0],
                cx.ports.clock.utc(),
                "upstream is down",
                Kind::Transient,
            )?;
        }
        Ok(())
    }
    fn execute(&self, _cx: &mut Cx<'_>, m: &Mutation) -> ExecOutcome {
        self.log.lock().unwrap().executed.push(m.id);
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(ExecOutcome::Done)
    }
    fn coalesce(&self, _op_kind: &str) -> Coalesce {
        Coalesce::LatestWins
    }
}

/// An engine on the fake clock (2026-01-01T00:00:00Z) with `sources` registered for E24.
fn fx(sources: Vec<&'static dyn Source>) -> (Fx, Fakes) {
    let mut fx = Fx::new("");
    let fakes = Fakes::new();
    fx.engine.ports = fakes.ports.clone();
    swr::set_sources_for_test(sources);
    (fx, fakes)
}

fn tick(fx: &mut Fx) {
    swr::run(&mut fx.engine);
}

fn now(fakes: &Fakes) -> OffsetDateTime {
    fakes.ports.clock.utc()
}

fn enqueue(fx: &mut Fx, fakes: &Fakes, system: &str, target: &str) -> i64 {
    outbox::enqueue(
        &mut fx.engine.store,
        now(fakes),
        Op {
            system: system.into(),
            target_key: target.into(),
            op_kind: "post".into(),
            vars: json!({"text": "hi"}),
            patch: None,
            context: None,
        },
        Coalesce::LatestWins,
    )
    .unwrap()
}

fn status_of(fx: &mut Fx, id: i64) -> String {
    outbox::get(&mut fx.engine.store, id)
        .unwrap()
        .unwrap()
        .status
}

#[test]
fn swr_step_skips_a_source_with_a_paused_endpoint() {
    let paused = Probe::leak("slack", &["ep-slack"], &["slack:channel:C1"]);
    let open = Probe::leak("jira", &["ep-jira"], &["jira:issue:EEC-1"]);
    let (mut fx, fakes) = fx(vec![paused, open]);
    let pending = enqueue(&mut fx, &fakes, "slack", "slack:channel:C1");
    endpoint::pause_until(
        &mut fx.engine.store,
        "ep-slack",
        now(&fakes) + time::Duration::hours(1),
    )
    .unwrap();

    tick(&mut fx);

    assert_eq!(
        open.fetches().len(),
        1,
        "the source without a paused endpoint is fetched"
    );
    assert!(
        paused.fetches().is_empty(),
        "no fetch while an endpoint of the source is paused"
    );
    assert!(
        paused.executed().is_empty(),
        "no mutation executes while it is paused"
    );
    assert_eq!(status_of(&mut fx, pending), "pending");

    fakes.clock.advance(Duration::from_secs(3601));
    tick(&mut fx);
    assert_eq!(
        paused.fetches().len(),
        1,
        "after the pause the source runs again"
    );
    assert_eq!(paused.executed(), [pending]);
}

#[test]
fn swr_step_fetches_due_queries_once_per_pass() {
    let slack = Probe::leak(
        "slack",
        &["ep-slack"],
        &["slack:channel:C1", "slack:channel:C2"],
    );
    let (mut fx, fakes) = fx(vec![slack]);

    let started = now(&fakes);
    tick(&mut fx);
    assert_eq!(slack.due_calls(), 1);
    assert_eq!(
        slack.fetches(),
        [(
            vec!["slack:channel:C1".to_owned(), "slack:channel:C2".to_owned()],
            started
        )],
        "one fetch for both keys, with the instant the pass began as poll_started_at"
    );

    fakes.clock.advance(Duration::from_secs(60));
    tick(&mut fx);
    let fetches = slack.fetches();
    assert_eq!(fetches.len(), 2, "the next pass fetches again, once");
    assert_eq!(fetches[1].1, started + time::Duration::seconds(60));
}

#[test]
fn swr_step_drains_pending_mutations_through_the_source() {
    let slack = Probe::leak("slack", &["ep-slack"], &[]);
    slack.script(vec![
        ExecOutcome::Done,
        ExecOutcome::Retry {
            after: Duration::from_secs(300),
        },
        ExecOutcome::Failed {
            error: "boom".into(),
        },
    ]);
    let (mut fx, fakes) = fx(vec![slack]);
    let done = enqueue(&mut fx, &fakes, "slack", "slack:channel:C1");
    let retry = enqueue(&mut fx, &fakes, "slack", "slack:channel:C2");
    let failed = enqueue(&mut fx, &fakes, "slack", "slack:channel:C3");

    tick(&mut fx);

    assert_eq!(
        slack.executed(),
        [done, retry, failed],
        "oldest first, each once: the retried one waits for next_attempt_at"
    );
    assert_eq!(
        status_of(&mut fx, done),
        "revalidating",
        "Done completes it"
    );
    let queued = cache::get(&mut fx.engine.store, "slack:channel:C1")
        .unwrap()
        .unwrap();
    assert!(queued.invalidated, "complete invalidates the target key");

    let retried = outbox::get(&mut fx.engine.store, retry).unwrap().unwrap();
    assert_eq!(retried.status, "pending", "Retry puts it back");
    assert_eq!(
        retried.next_attempt_at,
        Some(now(&fakes) + time::Duration::seconds(300))
    );

    let failed = outbox::get(&mut fx.engine.store, failed).unwrap().unwrap();
    assert_eq!(failed.status, "failed", "Failed never retries");
    assert_eq!(failed.error.as_deref(), Some("boom"));
    assert_eq!(
        fx.events("error").len(),
        1,
        "a failed mutation raises the error Event"
    );
}

#[test]
fn swr_step_runs_gc_at_most_hourly() {
    let (mut fx, fakes) = fx(Vec::new());
    let old = timestamp(now(&fakes) - time::Duration::days(8));
    let insert = |fx: &mut Fx, provider: &str| {
        fx.fixture(&format!(
            "INSERT INTO swr_models (provider, inactive_at) VALUES ('{provider}', '{old}');"
        ));
    };
    let rows = |fx: &mut Fx| fx.query::<i64>("SELECT count(*) FROM swr_models");

    insert(&mut fx, "a");
    tick(&mut fx);
    assert_eq!(
        rows(&mut fx),
        0,
        "the first pass of an engine's life sweeps"
    );

    insert(&mut fx, "b");
    fakes.clock.advance(Duration::from_secs(59 * 60));
    tick(&mut fx);
    assert_eq!(rows(&mut fx), 1, "a pass within the hour does not sweep");

    fakes.clock.advance(Duration::from_secs(60));
    tick(&mut fx);
    assert_eq!(rows(&mut fx), 0, "a pass an hour after the last sweep does");
}

#[test]
fn swr_step_raises_one_error_event_after_ten_minutes_of_failure() {
    let slack = Probe::failing("slack", &["ep-slack"], &["slack:channel:C1"]);
    let (mut fx, fakes) = fx(vec![slack]);

    tick(&mut fx);
    assert!(
        fx.events("error").is_empty(),
        "no Event at the first failure"
    );

    fakes.clock.advance(Duration::from_secs(9 * 60 + 59));
    tick(&mut fx);
    assert!(fx.events("error").is_empty(), "none before ten minutes");

    fakes.clock.advance(Duration::from_secs(1));
    tick(&mut fx);
    assert_eq!(fx.events("error").len(), 1, "one at ten minutes");

    fakes.clock.advance(Duration::from_secs(5 * 60));
    tick(&mut fx);
    assert_eq!(fx.events("error").len(), 1, "one report per outage");
}

#[test]
fn swr_step_never_runs_the_github_source() {
    let github = Probe::leak("github", &["github"], &["github:pr:acme/pay#1"]);
    let jira = Probe::leak("jira", &["ep-jira"], &["jira:issue:EEC-1"]);
    let (mut fx, fakes) = fx(vec![github, jira]);
    let pending = enqueue(&mut fx, &fakes, "github", "github:pr:acme/pay#1");

    tick(&mut fx);

    assert_eq!(jira.fetches().len(), 1, "every other source runs");
    assert_eq!(github.due_calls(), 0, "E5 runs github, not E24");
    assert!(github.fetches().is_empty());
    assert!(github.executed().is_empty());
    assert_eq!(status_of(&mut fx, pending), "pending");
}
