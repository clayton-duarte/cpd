//! Unexpected failures of a step (commands.md §12): each appends `error`
//! `{source: "watch", code, message, details}` for what it concerns, and the
//! loop continues. The same failure for the same object is appended once
//! until it clears: a step that succeeds for that object clears it. Whatever
//! clears, the same code at the same step for the same object is appended at
//! most once per `error_repeat_seconds` (contract §3.7), so a step that
//! succeeds in one part and fails in another does not append at every tick.
//! With each `error` Event goes one message to the Context's Manager, or to
//! every live Manager for a machine error (hs-d314).

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::SOURCE;
use super::log::Log;
use crate::error::CpdError;
use crate::events::{self, NewEvent};
use crate::ids::{AttemptId, JobKey, PlanId, SessionId};
use crate::session::tell_managers;
use crate::store::Store;

/// The step that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Step {
    E1,
    E2,
    E3,
    E4,
    E5,
    E7,
    E8,
    E9,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Step::E1 => "E1 (Escalations)",
            Step::E2 => "E2 (liveness)",
            Step::E3 => "E3 (scheduling)",
            Step::E4 => "E4 (completion)",
            Step::E5 => "E5 (PR)",
            Step::E7 => "E7 (PR start)",
            Step::E8 => "E8 (Inbox)",
            Step::E9 => "E9 (Context digests)",
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

    fn event(&self, text: String, failure: &Failure) -> NewEvent {
        let event = NewEvent::error(text, SOURCE, &failure.code, &failure.message, &failure.details);
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

/// The failures reported and not cleared yet: `(step, about)` → code and the id of its `error` Event.
#[derive(Default)]
pub(super) struct Report {
    open: HashMap<(Step, About), (String, i64)>,
    /// When each `(step, about, code)` was last appended, and the id of that Event.
    appended: HashMap<(Step, About, String), (Instant, i64)>,
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
        self.forget_old();
        let key = (step, about);
        if self.open.get(&key).is_some_and(|(code, _)| *code == failure.code) {
            return;
        }
        let repeated = (key.0, key.1.clone(), failure.code.clone());
        if let Some(&(_, event)) = self.appended.get(&repeated).filter(|(at, _)| at.elapsed() < self.repeat) {
            // Appended recently: only remember it is still failing.
            self.open.insert(key, (failure.code, event));
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
        // The Manager hears it in the same transaction, once per `error` Event (hs-d314): the Inbox shows no engine error.
        let to_managers = format!(
            "Engine error at {step} for {}: {}. It tries again at every tick. See: cpd events list --kind error",
            about.describe(),
            failure.message.trim_end_matches('.')
        );
        let appended = store.write(SOURCE, |tx| {
            let event = events::append(tx, about.event(text.clone(), &failure))?;
            tell_managers(tx, event, &to_managers)?;
            Ok(event)
        });
        match appended {
            Ok(event) => {
                self.appended.insert(repeated, (Instant::now(), event));
                self.open.insert(key, (failure.code, event));
            }
            // Not marked as reported: the next tick tries again.
            Err(e) => log.line(&format!("cannot append the error Event: {} ({})", e.message(), e.code().as_str())),
        }
    }

    /// `step` succeeded for `about`: a later failure is reported again.
    pub(super) fn clear(&mut self, step: Step, about: &About) {
        self.open.remove(&(step, about.clone()));
        self.forget_old();
    }

    /// Drops what was appended more than `repeat` ago: it no longer holds a later failure back, so an
    /// object that recovered leaves no entry behind once the period has passed.
    fn forget_old(&mut self) {
        let repeat = self.repeat;
        self.appended.retain(|_, (at, _)| at.elapsed() < repeat);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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

    /// engine-G6: a failure that cleared leaves nothing in the maps once the repeat period has passed.
    #[test]
    fn the_maps_shrink_when_an_object_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut log = Log::open(&home).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        let steps = [Step::E1, Step::E2, Step::E3];

        // Within the period the Event is remembered, to hold a repeat back.
        let mut report = Report::new(Duration::from_secs(600));
        report.fail(&mut store, &mut log, Step::E2, About::Machine, failure("tmux_failed"));
        report.clear(Step::E2, &About::Machine);
        assert_eq!((report.open.len(), report.appended.len()), (0, 1));

        // Past the period, the next success forgets it.
        let mut report = Report::new(Duration::ZERO);
        for step in steps {
            report.fail(&mut store, &mut log, step, About::Machine, failure("tmux_failed"));
        }
        assert_eq!(report.open.len(), 3);
        for step in steps {
            report.clear(step, &About::Machine);
        }
        assert_eq!((report.open.len(), report.appended.len()), (0, 0));
    }

    /// Contexts `ctx-a` and `ctx-b`, each with a Manager (`mgr-a`, `mgr-b`, archived when `archive_b`), a Plan `plan-a` of `ctx-a`, and
    /// a Context `ctx-c` with no Manager and its Plan `plan-c`.
    fn managers(store: &mut Store, archive_b: bool) {
        let t = "2026-01-01T00:00:00.000Z";
        let archived = if archive_b { format!("'{t}'") } else { "NULL".to_owned() };
        store
            .write("test", |tx| {
                for c in ["ctx-a", "ctx-b", "ctx-c"] {
                    tx.execute(
                        "INSERT INTO contexts (id, title, created_at, created_on) VALUES (?1, ?1, ?2, 'm1')",
                        [c, t],
                    )?;
                }
                tx.execute_batch(&format!(
                    "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at, archived_at)
                       VALUES ('mgr-a', 'manager', 'ctx-a', 'Ma', '/', 'O3', 'idle', '{t}', '{t}', NULL),
                              ('mgr-b', 'manager', 'ctx-b', 'Mb', '/', 'O3', 'idle', '{t}', '{t}', {archived}),
                              ('lead-a', 'lead', 'ctx-a', 'La', '/', 'O3', 'idle', '{t}', '{t}', NULL);
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                       VALUES ('plan-a', 'ctx-a', 'Pa', 'grounding', 'lead-a', '{t}'),
                              ('plan-c', 'ctx-c', 'Pc', 'grounding', 'lead-a', '{t}');"
                ))?;
                Ok(())
            })
            .unwrap();
    }

    /// The `session-message` Events: `(session, from, wake, text)`.
    fn messages(store: &mut Store) -> Vec<(String, Option<String>, bool, String)> {
        store
            .read(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT session, json_extract(data, '$.from'), json_extract(data, '$.wake'), json_extract(data, '$.text')
                       FROM events WHERE kind = 'session-message' ORDER BY id",
                )?;
                Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    /// hs-d314: a step's error reaches the Manager of its Context as one message from the User, no wake, once while it stays.
    #[test]
    fn a_step_error_is_sent_once_to_the_manager_of_its_context() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut log = Log::open(&home).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        managers(&mut store, false);
        let mut report = Report::new(Duration::from_secs(600));
        let about = About::Plan(PlanId::parse("plan-a").unwrap());

        for _ in 0..3 {
            report.fail(&mut store, &mut log, Step::E3, about.clone(), failure("store_failed"));
        }
        assert_eq!(errors(&mut store), 1);
        let sent = messages(&mut store);
        assert_eq!(
            sent,
            [(
                "mgr-a".to_owned(),
                None,
                false,
                "Engine error at E3 (scheduling) for Plan plan-a: It failed. It tries again at every tick. See: cpd events list --kind error"
                    .to_owned()
            )]
        );

        // Cleared and failing again within the repeat period: no new `error` Event, so no new message.
        report.clear(Step::E3, &about);
        report.fail(&mut store, &mut log, Step::E3, about.clone(), failure("store_failed"));
        assert_eq!(messages(&mut store).len(), 1);

        // Another Context's Plan reaches that Context's Manager only; the Context `c` has none.
        report.fail(&mut store, &mut log, Step::E3, About::Plan(PlanId::parse("plan-c").unwrap()), failure("store_failed"));
        assert_eq!(messages(&mut store).len(), 1, "a Context with no Manager: nothing is sent");
        assert_eq!(errors(&mut store), 2, "the log keeps it");
    }

    #[test]
    fn a_machine_error_reaches_every_live_manager_and_an_archived_one_gets_none() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut log = Log::open(&home).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        managers(&mut store, false);
        let mut report = Report::new(Duration::from_secs(600));

        report.fail(&mut store, &mut log, Step::E2, About::Machine, failure("tmux_failed"));
        report.fail(&mut store, &mut log, Step::E2, About::Machine, failure("tmux_failed"));
        let sent = messages(&mut store);
        assert_eq!(sent.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["mgr-a", "mgr-b"]);
        assert!(sent.iter().all(|m| m.1.is_none() && !m.2 && m.3.starts_with("Engine error at E2 (liveness) for the machine: It failed.")));

        // An archived Manager is not live.
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut log = Log::open(&home).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        managers(&mut store, true);
        Report::new(Duration::from_secs(600)).fail(&mut store, &mut log, Step::E2, About::Machine, failure("tmux_failed"));
        assert_eq!(messages(&mut store).iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["mgr-a"]);
    }

    #[test]
    fn with_no_manager_nothing_is_sent() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut log = Log::open(&home).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        Report::new(Duration::from_secs(600)).fail(&mut store, &mut log, Step::E2, About::Machine, failure("tmux_failed"));
        assert_eq!((errors(&mut store), messages(&mut store).len()), (1, 0));
    }
}
