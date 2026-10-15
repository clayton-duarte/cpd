//! Unexpected failures of a step (commands.md §12): each appends `error`
//! `{source: "watch", code, message, details}` for what it concerns, and the
//! loop continues. The same failure for the same object is appended once
//! until it clears: a step that succeeds for that object clears it.

use std::collections::HashMap;
use std::fmt;

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
            About::Machine => "cpd2 doctor".to_owned(),
            About::Plan(p) => format!("cpd2 plan show --plan {p}"),
            About::Session(s) => format!("cpd2 session show {s}"),
            About::Job(p, j) => format!("cpd2 job show {j} --plan {p}"),
            About::Attempt(p, a) => format!("cpd2 attempt show {a} --plan {p}"),
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
