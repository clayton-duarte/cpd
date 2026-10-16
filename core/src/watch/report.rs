//! Unexpected failures of a step (commands.md §12): each appends `error`
//! `{source: "watch", code, message, details}` for what it concerns, and the
//! loop continues. The same failure for the same object is appended once
//! until it clears: a step that succeeds for that object clears it. Whatever
//! clears, the same code at the same step for the same object is appended at
//! most once per `error_repeat_seconds` (contract §3.7), so a step that
//! succeeds in one part and fails in another does not append at every tick.

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::SOURCE;
use super::log::Log;
use crate::error::CpdError;
use crate::events::{self, Kind, NewEvent};
use crate::ids::{AttemptId, JobKey, PlanId, SessionId};
use crate::store::Store;

/// The step that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Step {
    Start,
    E1,
    E2,
    E3,
    E4,
    E5,
    E6,
    E7,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Step::Start => "start-up",
            Step::E1 => "E1 (Escalations)",
            Step::E2 => "E2 (liveness)",
            Step::E3 => "E3 (scheduling)",
            Step::E4 => "E4 (completion)",
            Step::E5 => "E5 (PR)",
            Step::E6 => "E6 (Contexts)",
            Step::E7 => "E7 (PR start)",
        })
    }
}

/// What a failure concerns: the scope of its `error` Event.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum About {
    /// The machine: no Context, no Plan.
    #[default]
    Machine,
    Plan(PlanId),
    Job(PlanId, JobKey),
    Attempt(PlanId, AttemptId),
    Session(SessionId),
}

impl About {
    fn describe(&self) -> String {
        match self {
            About::Machine => "the machine".to_owned(),
            About::Plan(p) => format!("Plan {p}"),
            About::Job(p, j) => format!("Job {j} of Plan {p}"),
            About::Attempt(p, a) => format!("Attempt {a} of Plan {p}"),
            About::Session(s) => format!("Session {s}"),
        }
    }

    /// The command the Lead may run to look at it (§5.1: a text ends with one).
    fn see(&self) -> String {
        match self {
            About::Machine => "cpd doctor".to_owned(),
            About::Plan(p) => format!("cpd plan show --plan {p}"),
            About::Session(s) => format!("cpd session show {s}"),
            About::Job(p, j) => format!("cpd job show {j} --plan {p}"),
            About::Attempt(p, a) => format!("cpd attempt show {a} --plan {p}"),
        }
    }

    fn event(&self, text: String, data: Value) -> NewEvent {
        let event = NewEvent::new(Kind::Error, text, data);
        match self {
            About::Machine => event,
            About::Plan(p) => event.plan(p.clone()),
            About::Session(s) => event.session(s.clone()),
            About::Job(p, j) => event.plan(p.clone()).job(j.clone()),
            About::Attempt(p, a) => event.plan(p.clone()).job(a.job().clone()).attempt(a.clone()),
        }
    }
}

/// A failure as an `error` Event carries it.
#[derive(Clone, Debug)]
pub(super) struct Failure {
    pub code: String,
    pub message: String,
    pub details: Value,
}

impl From<&CpdError> for Failure {
    fn from(e: &CpdError) -> Failure {
        Failure { code: e.code().as_str().to_owned(), message: e.message().to_owned(), details: Value::Object(e.details().clone()) }
    }
}

/// The failures reported and not cleared yet: `(step, about)` → code.
#[derive(Default)]
pub(super) struct Report {
    open: HashMap<(Step, About), String>,
    /// When each `(step, about, code)` was last appended.
    appended: HashMap<(Step, About, String), Instant>,
    /// `watch.error_repeat_seconds`.
    repeat: Duration,
}

impl Report {
    pub(super) fn new(repeat: Duration) -> Report {
        Report { open: HashMap::new(), appended: HashMap::new(), repeat }
    }
}

impl Report {
    /// Reports `failure` of `step` for `about`: logs it and appends `error`,
    /// unless the same failure is already reported and not cleared.
    pub(super) fn fail(&mut self, store: &mut Store, log: &mut Log, step: Step, about: About, failure: impl Into<Failure>) {
        let failure = failure.into();
        let key = (step, about);
        if self.open.get(&key) == Some(&failure.code) {
            return;
        }
        let repeated = (key.0, key.1.clone(), failure.code.clone());
        if self.appended.get(&repeated).is_some_and(|at| at.elapsed() < self.repeat) {
            // Appended recently: only remember it is still failing.
            self.open.insert(key, failure.code);
            return;
        }
        let (step, about) = &key;
        log.line(&format!("{step} failed for {}: {} ({})", about.describe(), failure.message, failure.code));
        let text = format!(
            "The engine failed at {step} for {}: {} It tries again at every tick. See: {}",
            about.describe(),
            failure.message,
            about.see()
        );
        let data = json!({"source": SOURCE, "code": failure.code, "message": failure.message, "details": failure.details});
        match store.write(SOURCE, |tx| events::append(tx, about.event(text, data))) {
            Ok(_) => {
                self.appended.insert(repeated, Instant::now());
                self.open.insert(key, failure.code);
            }
            // Not marked as reported: the next tick tries again.
            Err(e) => log.line(&format!("cannot append the error Event: {} ({})", e.message(), e.code().as_str())),
        }
    }

    /// `step` succeeded for `about`: a later failure is reported again.
    pub(super) fn clear(&mut self, step: Step, about: &About) {
        self.open.remove(&(step, about.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::CpdHome;

    fn failure(code: &str) -> Failure {
        Failure { code: code.to_owned(), message: "It failed.".to_owned(), details: json!({}) }
    }

    fn errors(store: &mut Store) -> i64 {
        store.read(|conn| Ok(conn.query_row("SELECT count(*) FROM events WHERE kind = 'error'", [], |r| r.get(0))?)).unwrap()
    }

    /// A step that fails, clears and fails again at every tick (E2 does: its
    /// sub-steps clear the Machine's failure) is appended once per repeat
    /// period; another code or another step is its own Event.
    #[test]
    fn the_same_error_is_appended_once_per_repeat_period_even_when_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut log = Log::open(&home).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();

        let mut report = Report::new(Duration::from_secs(600));
        for _ in 0..3 {
            report.fail(&mut store, &mut log, Step::E2, About::Machine, failure("tmux_failed"));
            report.clear(Step::E2, &About::Machine);
        }
        assert_eq!(errors(&mut store), 1);
        report.fail(&mut store, &mut log, Step::E2, About::Machine, failure("store_failed"));
        report.fail(&mut store, &mut log, Step::E3, About::Machine, failure("tmux_failed"));
        assert_eq!(errors(&mut store), 3);

        // Past the period it is appended again.
        let mut report = Report::new(Duration::ZERO);
        for _ in 0..2 {
            report.fail(&mut store, &mut log, Step::E2, About::Machine, failure("tmux_failed"));
            report.clear(Step::E2, &About::Machine);
        }
        assert_eq!(errors(&mut store), 5);
    }
}
