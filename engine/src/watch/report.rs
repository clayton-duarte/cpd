//! Unexpected failures of a step (commands.md §12): each appends `error`
//! `{source: "watch", code, message, details}` for what it concerns, and the
//! loop continues. The same failure for the same object is appended once
//! until it clears: a step that succeeds for that object clears it. Whatever
//! clears, the same code at the same step for the same object is appended at
//! most once per `error_repeat_seconds` (contract §3.7), so a step that
//! succeeds in one part and fails in another does not append at every tick.
//! The window also holds across an engine restart: before appending, [`Report::fail`] reads the store for an
//! `error` Event of the same step, scope and code appended less than `error_repeat_seconds` ago.
//! With each `error` Event goes one message to the Context's Manager, or to
//! every live Manager for a machine error (hs-d314). A stuck Job goes to its
//! Context's Manager the same way, once (hs-d318), grouped by Workflow (hs-d647,
//! [`tell_stuck_jobs`]).

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::SOURCE;
use super::log::Log;
use cpd_core::error::CpdError;
use cpd_core::events::repeat::{self, Recent, Same};
use cpd_core::events::{self, NewEvent};
use cpd_core::ids::{AttemptId, ContextId, JobKey, PlanId, SessionId, WorkflowKey};
use cpd_core::inbox::rules::{StuckGroup, StuckJob, stuck_groups};
use cpd_core::job::home::JobHome;
use cpd_core::pr::jira_sync::Target;
use cpd_core::session::tell_managers;
use cpd_core::store::Store;

/// The step that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Step {
    E1,
    E2,
    E3,
    E4,
    E5,
    E7,
    Workers,
    HostAlert,
    E8,
    E9,
    E10,
    E11,
    E12,
    E13,
    E14,
    E15,
    E16,
    E17,
    E18,
    E19,
    E20,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Step::E1 => "E1 (Escalations)",
            Step::E2 => "E2 (liveness)",
            Step::E3 => "E3 (scheduling)",
            Step::E4 => "E4 (open Jobs)",
            Step::E5 => "E5 (pull request poll)",
            Step::E7 => "E7 (Workflow start)",
            Step::Workers => "the @cpd_workers step",
            Step::HostAlert => "the @cpd_host_alert step",
            Step::E8 => "E8 (Inbox)",
            Step::E9 => "E9 (Context digests)",
            Step::E10 => "E10 (Session titles)",
            Step::E11 => "E11 (team review poll)",
            Step::E12 => "E12 (Worker archive)",
            Step::E13 => "E13 (Sleep)",
            Step::E14 => "E14 (Lead archive)",
            Step::E15 => "E15 (model discovery)",
            Step::E16 => "E16 (janitor)",
            Step::E17 => "E17 (merge reactions)",
            Step::E18 => "E18 (auto-merge requests)",
            Step::E19 => "E19 (Jira transitions)",
            Step::E20 => "E20 (parked drafts)",
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
    /// A Session's retitle run (E10): the Inbox scope and the command are the Session's, but the
    /// `error` Event sets no Session, so no Lead gets a turn for it (contract §3.7 E10).
    TitleRun(SessionId),
    /// One E19 queue row (Plan, Workflow, target): scoped to the Plan, deduped per row (contract §3.7 E19).
    Move(PlanId, WorkflowKey, Target),
}

impl About {
    fn describe(&self) -> String {
        match self {
            About::Machine => "the machine".to_owned(),
            About::Plan(p) => format!("Plan {p}"),
            About::Job(p, j) => format!("Job {j} of Plan {p}"),
            About::Attempt(p, a) => format!("Attempt {a} of Plan {p}"),
            About::Session(s) | About::TitleRun(s) => format!("Session {s}"),
            About::Move(p, workflow, t) => {
                format!("the {} move of Workflow {workflow} of Plan {p}", t.as_str())
            }
        }
    }

    /// What the text of an `error` Event begins with: the one place the text is written and looked up by.
    fn lead(&self, step: Step) -> String {
        format!("The engine failed at {step} for {}:", self.describe())
    }

    /// The command the Lead may run to look at it (§5.1: a text ends with one).
    fn see(&self) -> String {
        match self {
            About::Machine => "cpd doctor".to_owned(),
            About::Plan(p) | About::Move(p, ..) => format!("cpd plan show --plan {p}"),
            About::Session(s) | About::TitleRun(s) => format!("cpd session show {s}"),
            About::Job(p, j) => format!("cpd job show {j} --plan {p}"),
            About::Attempt(p, a) => format!("cpd attempt show {a} --plan {p}"),
        }
    }

    fn event(&self, text: String, failure: &Failure) -> NewEvent {
        let event = NewEvent::error(
            text,
            SOURCE,
            &failure.code,
            &failure.message,
            &failure.details,
        );
        match self {
            About::Machine | About::TitleRun(_) => event,
            About::Plan(p) | About::Move(p, ..) => event.plan(p.clone()),
            About::Session(s) => event.session(s.clone()),
            About::Job(p, j) => event.plan(p.clone()).job(j.clone()),
            About::Attempt(p, a) => event
                .plan(p.clone())
                .job(a.job().clone())
                .attempt(a.clone()),
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
        Failure {
            code: e.code().as_str().to_owned(),
            message: e.message().to_owned(),
            details: Value::Object(e.details().clone()),
        }
    }
}

/// What every message about a group's stuck Jobs begins with: the record that it was sent ([`tell_stuck_jobs`]).
fn stuck_prefix(group: &StuckGroup) -> String {
    match &group.home {
        JobHome::Workflow(workflow) => {
            format!("Jobs of Workflow {}/{workflow} failed ", group.plan)
        }
        JobHome::Plan => format!("Explore Jobs of Plan {} failed ", group.plan),
    }
}

/// The old spelling of [`stuck_prefix`] a message stored before the Workflow rename begins with (map `stuck_prefix`
/// hazard): the dedupe must match it too, or the Manager is told again of Workflows whose failures predate the rename.
fn stuck_prefix_legacy(group: &StuckGroup) -> Option<String> {
    match &group.home {
        JobHome::Workflow(workflow) => {
            Some(format!("Jobs of PR {}/{workflow} failed ", group.plan))
        }
        JobHome::Plan => None,
    }
}

/// The one message of a Workflow's stuck Jobs to its Context's Manager (contract §3.7 "A stuck Job", hs-d647): each Job with its
/// own minutes and `failed_cause`, oldest first. `<key>` and `<T>` stay as written for the Manager to choose.
fn stuck_text(group: &StuckGroup) -> String {
    let entries = group
        .jobs
        .iter()
        .map(|j| {
            format!(
                "{}/{} ({} min ago, {})",
                group.plan, j.job, j.minutes, j.cause
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "{}and their Lead has not retried or cancelled them: {}. Retry each (cpd job retry <key> --plan {} --tier <T>), nudge the Lead, or ask the User.",
        stuck_prefix(group),
        entries,
        group.plan
    )
}

/// Whether the Context's live Manager is yet to be told of `group`: there is one (role `manager`, not archived) and no
/// message of this Workflow's failure reached it, that is one from the User's side (`from` null) newer than the group's newest
/// `job-failed` Event whose text begins [`stuck_prefix`]. A later failure of a Job of the Workflow is a newer Event, so the group
/// is told again.
fn untold(conn: &Connection, group: &StuckGroup) -> Result<bool, CpdError> {
    let Some(manager) = live_manager(conn, &group.context)? else {
        return Ok(false);
    };
    if message_exists(conn, &manager, Some(group.event), &stuck_prefix(group))? {
        return Ok(false);
    }
    if let Some(legacy) = stuck_prefix_legacy(group)
        && message_exists(conn, &manager, Some(group.event), &legacy)?
    {
        return Ok(false);
    }
    Ok(true)
}

/// The id of the Context's live Manager (role `manager`, not archived), if there is one.
pub(super) fn live_manager(
    conn: &Connection,
    context: &ContextId,
) -> Result<Option<String>, CpdError> {
    Ok(conn
        .query_row(
            "SELECT id FROM sessions WHERE role = 'manager' AND archived_at IS NULL AND context = ?1",
            [context.as_str()],
            |r| r.get(0),
        )
        .optional()?)
}

/// Whether a message from the User's side (`from` null) to `manager`, newer than Event `after` when given, begins with `prefix`.
pub(super) fn message_exists(
    conn: &Connection,
    manager: &str,
    after: Option<i64>,
    prefix: &str,
) -> Result<bool, CpdError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM events WHERE session = ?1 AND id > ?2 AND kind = 'session-message'
                         AND json_extract(data, '$.from') IS NULL AND instr(json_extract(data, '$.text'), ?3) = 1)",
        params![manager, after.unwrap_or(0), prefix],
        |r| r.get(0),
    )?)
}

/// Tells the live Manager of each group's Context one message per Workflow, by the path of a step's failure (one waking message
/// from the User's side, [`tell_managers`]), once until a Job of the group fails again (contract §3.7 "A stuck Job",
/// hs-d318, hs-d647): the record that it was told is the message itself ([`untold`]), so a later tick or a restarted
/// engine sends nothing. With no live Manager nothing is sent and nothing is written (the next tick looks again). Returns
/// the number of messages sent.
pub(super) fn tell_stuck_jobs(store: &mut Store, stuck: &[StuckJob]) -> Result<usize, CpdError> {
    let mut sent = 0;
    for group in &stuck_groups(stuck.to_vec()) {
        if !store.read(|conn| untold(conn, group))? {
            continue;
        }
        let text = stuck_text(group);
        sent += store.write(SOURCE, |tx| tell_managers(tx, group.event, &text))?;
    }
    Ok(sent)
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
        Report {
            open: HashMap::new(),
            appended: HashMap::new(),
            repeat,
        }
    }
}

impl Report {
    /// Reports `failure` of `step` for `about`: logs it and appends `error`,
    /// unless the same failure is already reported and not cleared, or the store holds an `error` Event of
    /// the same step, scope and code (matched by [`About::lead`]) appended less than `repeat` ago (an engine
    /// restart forgets memory, not the store): then it is only remembered as still open.
    pub(super) fn fail(
        &mut self,
        store: &mut Store,
        log: &mut Log,
        step: Step,
        about: About,
        failure: impl Into<Failure>,
    ) {
        let failure = failure.into();
        self.forget_old();
        let key = (step, about);
        if self
            .open
            .get(&key)
            .is_some_and(|(code, _)| *code == failure.code)
        {
            return;
        }
        let repeated = (key.0, key.1.clone(), failure.code.clone());
        if let Some(&(_, event)) = self
            .appended
            .get(&repeated)
            .filter(|(at, _)| at.elapsed() < self.repeat)
        {
            // Appended recently: only remember it is still failing.
            self.open.insert(key, (failure.code, event));
            return;
        }
        let (step, about) = &key;
        let lead = about.lead(*step);
        if self.repeat > Duration::ZERO {
            let same = Same::Lead {
                source: "watch",
                code: &failure.code,
                lead: &lead,
            };
            match store.read(|conn| repeat::recent(conn, self.repeat, &same)) {
                Ok(Some(Recent { event, age })) => {
                    let at = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
                    self.appended.insert(repeated, (at, event));
                    self.open.insert(key, (failure.code, event));
                    return;
                }
                Ok(None) => {}
                Err(e) => log.line(&format!(
                    "cannot read the error Events: {} ({})",
                    e.message(),
                    e.code().as_str()
                )),
            }
        }
        log.line(&format!(
            "{step} failed for {}: {} ({})",
            about.describe(),
            failure.message,
            failure.code
        ));
        let text = format!(
            "{lead} {} It tries again at every tick. See: {}",
            failure.message,
            about.see()
        );
        let appended = store.write(SOURCE, |tx| {
            events::append(tx, about.event(text.clone(), &failure))
        });
        match appended {
            Ok(event) => {
                self.appended.insert(repeated, (Instant::now(), event));
                self.open.insert(key, (failure.code, event));
            }
            // Not marked as reported: the next tick tries again.
            Err(e) => log.line(&format!(
                "cannot append the error Event: {} ({})",
                e.message(),
                e.code().as_str()
            )),
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
    use cpd_core::home::CpdHome;
    use cpd_core::ids::{ContextId, WorkflowKey};
    use serde_json::json;

    fn failure(code: &str) -> Failure {
        Failure {
            code: code.to_owned(),
            message: "It failed.".to_owned(),
            details: json!({}),
        }
    }

    fn errors(store: &mut Store) -> i64 {
        store
            .read(|conn| {
                Ok(conn.query_row(
                    "SELECT count(*) FROM events WHERE kind = 'error'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
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
            report.fail(
                &mut store,
                &mut log,
                Step::E2,
                About::Machine,
                failure("tmux_failed"),
            );
            report.clear(Step::E2, &About::Machine);
        }
        assert_eq!(errors(&mut store), 1);
        report.fail(
            &mut store,
            &mut log,
            Step::E2,
            About::Machine,
            failure("store_failed"),
        );
        report.fail(
            &mut store,
            &mut log,
            Step::E3,
            About::Machine,
            failure("tmux_failed"),
        );
        assert_eq!(errors(&mut store), 3);

        // Past the period it is appended again.
        let mut report = Report::new(Duration::ZERO);
        for _ in 0..2 {
            report.fail(
                &mut store,
                &mut log,
                Step::E2,
                About::Machine,
                failure("tmux_failed"),
            );
            report.clear(Step::E2, &About::Machine);
        }
        assert_eq!(errors(&mut store), 5);
    }

    /// A retitle failure points at its Session but sets no scope on its `error` Event.
    #[test]
    fn a_title_run_failure_names_its_session_and_sets_none_on_the_event() {
        let about = About::TitleRun(SessionId::parse("lead1").unwrap());
        assert_eq!(about.see(), "cpd session show lead1");
        let event = about.event("x".to_owned(), &failure("title_failed"));
        assert_eq!(
            format!("{event:?}").matches("session: None").count(),
            1,
            "{event:?}"
        );
    }

    /// An E19 row failure is keyed per row: its lead names the Workflow and the target; the Event is scoped to the Plan.
    #[test]
    fn a_move_failure_is_per_row_and_plan_scoped() {
        let plan = PlanId::parse("plan-a").unwrap();
        let about = About::Move(
            plan.clone(),
            WorkflowKey::parse("pr1").unwrap(),
            Target::InReview,
        );
        assert_eq!(about.see(), "cpd plan show --plan plan-a");
        assert!(about.lead(Step::E19).contains(&format!(
            "the {} move of Workflow pr1 of Plan plan-a",
            Target::InReview.as_str()
        )));
        assert_ne!(
            about,
            About::Move(plan, WorkflowKey::parse("pr2").unwrap(), Target::InReview)
        );
        let event = about.event("x".to_owned(), &failure("jira_failed"));
        assert_eq!(
            format!("{event:?}").matches("plan: Some").count(),
            1,
            "{event:?}"
        );
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
        report.fail(
            &mut store,
            &mut log,
            Step::E2,
            About::Machine,
            failure("tmux_failed"),
        );
        report.clear(Step::E2, &About::Machine);
        assert_eq!((report.open.len(), report.appended.len()), (0, 1));

        // Past the period, the next success forgets it.
        let mut report = Report::new(Duration::ZERO);
        for step in steps {
            report.fail(
                &mut store,
                &mut log,
                step,
                About::Machine,
                failure("tmux_failed"),
            );
        }
        assert_eq!(report.open.len(), 3);
        for step in steps {
            report.clear(step, &About::Machine);
        }
        assert_eq!((report.open.len(), report.appended.len()), (0, 0));
    }

    /// Contexts `ctx-a` and `ctx-b`, each with a Manager (`mgr-a`, `mgr-b`, archived when `archive_b`) and a Plan (`plan-a`,
    /// `plan-b`), and a Context `ctx-c` with no Manager and its Plan `plan-c`.
    fn managers(store: &mut Store, archive_b: bool) {
        let t = "2026-01-01T00:00:00.000Z";
        let archived = if archive_b {
            format!("'{t}'")
        } else {
            "NULL".to_owned()
        };
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
                              ('plan-b', 'ctx-b', 'Pb', 'grounding', 'lead-a', '{t}'),
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

    /// hs-d514: an engine error is an Event only, sent to no Manager.
    #[test]
    fn a_step_error_is_an_event_and_sends_no_message() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut log = Log::open(&home).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        managers(&mut store, false);
        let mut report = Report::new(Duration::from_secs(600));
        let about = About::Plan(PlanId::parse("plan-a").unwrap());
        for _ in 0..3 {
            report.fail(
                &mut store,
                &mut log,
                Step::E3,
                about.clone(),
                failure("store_failed"),
            );
        }
        report.fail(
            &mut store,
            &mut log,
            Step::E2,
            About::Machine,
            failure("tmux_failed"),
        );
        assert_eq!((errors(&mut store), messages(&mut store).len()), (2, 0));
    }

    #[test]
    fn with_no_manager_nothing_is_sent() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut log = Log::open(&home).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        Report::new(Duration::from_secs(600)).fail(
            &mut store,
            &mut log,
            Step::E2,
            About::Machine,
            failure("tmux_failed"),
        );
        assert_eq!((errors(&mut store), messages(&mut store).len()), (1, 0));
    }

    // ---- a stuck Job (hs-d318) ----

    /// A store with the Contexts and Managers of [`managers`].
    fn store_with_managers() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        managers(&mut store, false);
        (dir, store)
    }

    /// A `job-failed` Event of Job `job` of `plan` of `context`; returns its id, what a [`StuckJob`]'s `event` is.
    fn job_failed(store: &mut Store, context: &str, plan: &str, job: &str) -> i64 {
        store
            .write("test", |tx| {
                tx.execute(
                    "INSERT INTO events (txn, at, kind, context, plan, job, text, data)
                     VALUES (cpd_txn(), '2026-01-01T00:00:00.000Z', 'job-failed', ?1, ?2, ?3, 'x', '{}')",
                    [context, plan, job],
                )?;
                Ok(())
            })
            .unwrap();
        store
            .read(|conn| Ok(conn.query_row("SELECT max(id) FROM events", [], |r| r.get(0))?))
            .unwrap()
    }

    /// The Job as the stuck query reads it: failed `minutes` ago, whose `job-failed` Event is `event`, for `crashed`.
    fn stuck(
        context: &str,
        plan: &str,
        workflow: &str,
        job: &str,
        event: i64,
        minutes: i64,
    ) -> StuckJob {
        StuckJob {
            plan: PlanId::parse(plan).unwrap(),
            home: JobHome::Workflow(WorkflowKey::parse(workflow).unwrap()),
            job: JobKey::parse(job).unwrap(),
            context: ContextId::parse(context).unwrap(),
            event,
            minutes,
            cause: "crashed".to_owned(),
        }
    }

    /// The text of a group's message: each `(job, minutes)` entry, oldest first, with the group's `<plan>/<pr>` prefix.
    fn text_of(plan: &str, workflow: &str, jobs: &[(&str, i64)]) -> String {
        let entries = jobs
            .iter()
            .map(|(job, minutes)| format!("{plan}/{job} ({minutes} min ago, crashed)"))
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "Jobs of Workflow {plan}/{workflow} failed and their Lead has not retried or cancelled them: {entries}. \
             Retry each (cpd job retry <key> --plan {plan} --tier <T>), nudge the Lead, or ask the User."
        )
    }

    fn txns(store: &mut Store) -> i64 {
        store
            .read(|conn| Ok(conn.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?))
            .unwrap()
    }

    /// hs-d318: a stuck Job reaches the Manager of its Context as one message from the User's side, no wake, and the next
    /// ticks send nothing, whatever the minutes say by then.
    #[test]
    fn a_stuck_job_is_sent_once_to_the_manager_of_its_context() {
        let (_dir, mut store) = store_with_managers();
        let failed = job_failed(&mut store, "ctx-a", "plan-a", "a");

        assert_eq!(
            tell_stuck_jobs(
                &mut store,
                &[stuck("ctx-a", "plan-a", "repo", "a", failed, 12)]
            )
            .unwrap(),
            1
        );
        assert_eq!(
            messages(&mut store),
            [(
                "mgr-a".to_owned(),
                None,
                true,
                text_of("plan-a", "repo", &[("a", 12)])
            )]
        );
        for minutes in [12, 13, 600] {
            assert_eq!(
                tell_stuck_jobs(
                    &mut store,
                    &[stuck("ctx-a", "plan-a", "repo", "a", failed, minutes)]
                )
                .unwrap(),
                0,
                "{minutes} min"
            );
        }
        assert_eq!(messages(&mut store).len(), 1);
    }

    /// hs-d647: the stuck Jobs of one Workflow are one message, oldest `job-failed` Event first, each with its own minutes.
    #[test]
    fn two_stuck_jobs_of_one_pr_are_one_message_oldest_first() {
        let (_dir, mut store) = store_with_managers();
        let old = job_failed(&mut store, "ctx-a", "plan-a", "old");
        let fresh = job_failed(&mut store, "ctx-a", "plan-a", "fresh");

        let jobs = [
            stuck("ctx-a", "plan-a", "repo", "old", old, 30),
            stuck("ctx-a", "plan-a", "repo", "fresh", fresh, 12),
        ];
        assert_eq!(
            tell_stuck_jobs(&mut store, &jobs).unwrap(),
            1,
            "one Workflow, one message"
        );
        assert_eq!(
            messages(&mut store),
            [(
                "mgr-a".to_owned(),
                None,
                true,
                text_of("plan-a", "repo", &[("old", 30), ("fresh", 12)])
            )]
        );
    }

    /// After a retry the Job fails again: a newer `job-failed` Event, so a new message, once more.
    #[test]
    fn a_job_that_fails_again_after_a_retry_is_sent_again() {
        let (_dir, mut store) = store_with_managers();
        let first = job_failed(&mut store, "ctx-a", "plan-a", "a");
        assert_eq!(
            tell_stuck_jobs(
                &mut store,
                &[stuck("ctx-a", "plan-a", "repo", "a", first, 10)]
            )
            .unwrap(),
            1
        );

        let second = job_failed(&mut store, "ctx-a", "plan-a", "a");
        assert_eq!(
            tell_stuck_jobs(
                &mut store,
                &[stuck("ctx-a", "plan-a", "repo", "a", second, 11)]
            )
            .unwrap(),
            1
        );
        assert_eq!(
            tell_stuck_jobs(
                &mut store,
                &[stuck("ctx-a", "plan-a", "repo", "a", second, 12)]
            )
            .unwrap(),
            0
        );
        let sent: Vec<String> = messages(&mut store).into_iter().map(|m| m.3).collect();
        assert_eq!(
            sent,
            [
                text_of("plan-a", "repo", &[("a", 10)]),
                text_of("plan-a", "repo", &[("a", 11)])
            ]
        );
    }

    /// No live Manager: nothing is sent and nothing is written, so a quiet tick stays quiet; a Manager that comes later, or
    /// one that replaces an archived Manager that was told, hears of the Job.
    #[test]
    fn with_no_live_manager_nothing_is_sent_or_written_and_a_later_manager_is_told() {
        let (_dir, mut store) = store_with_managers();
        let failed = job_failed(&mut store, "ctx-c", "plan-c", "c");
        let before = txns(&mut store);
        assert_eq!(
            tell_stuck_jobs(
                &mut store,
                &[stuck("ctx-c", "plan-c", "repo", "c", failed, 10)]
            )
            .unwrap(),
            0
        );
        assert_eq!(
            (messages(&mut store), txns(&mut store)),
            (vec![], before),
            "the Context c has no Manager"
        );

        let t = "2026-01-01T00:00:00.000Z";
        let add = |store: &mut Store, id: &str, context: &str| {
            store
                .write("test", |tx| {
                    tx.execute(
                        "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
                         VALUES (?1, 'manager', ?2, 'M', '/', 'O3', 'idle', ?3, ?3)",
                        [id, context, t],
                    )?;
                    Ok(())
                })
                .unwrap();
        };
        add(&mut store, "mgr-c", "ctx-c");
        assert_eq!(
            tell_stuck_jobs(
                &mut store,
                &[stuck("ctx-c", "plan-c", "repo", "c", failed, 10)]
            )
            .unwrap(),
            1
        );
        assert_eq!(
            messages(&mut store)
                .iter()
                .map(|m| m.0.as_str())
                .collect::<Vec<_>>(),
            ["mgr-c"]
        );

        // The told Manager is archived and another takes its place: the new one is told.
        store
            .write("test", |tx| {
                tx.execute(
                    "UPDATE sessions SET archived_at = ?1 WHERE id = 'mgr-c'",
                    [t],
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            tell_stuck_jobs(
                &mut store,
                &[stuck("ctx-c", "plan-c", "repo", "c", failed, 10)]
            )
            .unwrap(),
            0,
            "an archived Manager is not live"
        );
        add(&mut store, "mgr-c2", "ctx-c");
        assert_eq!(
            tell_stuck_jobs(
                &mut store,
                &[stuck("ctx-c", "plan-c", "repo", "c", failed, 10)]
            )
            .unwrap(),
            1
        );
        assert_eq!(
            messages(&mut store)
                .iter()
                .map(|m| m.0.as_str())
                .collect::<Vec<_>>(),
            ["mgr-c", "mgr-c2"]
        );
    }

    /// Only the Manager of the Job's Context hears of it, and a message counts as the record only when it is about this Workflow
    /// (`plan-a/repo` is not `plan-a/other`) and from the User's side (a Lead's own words are not the engine's).
    #[test]
    fn only_a_message_about_this_pr_from_the_users_side_counts() {
        let (_dir, mut store) = store_with_managers();
        let a = job_failed(&mut store, "ctx-a", "plan-a", "a");
        let other = job_failed(&mut store, "ctx-a", "plan-a", "b");
        let b = job_failed(&mut store, "ctx-b", "plan-b", "a");

        assert_eq!(
            tell_stuck_jobs(&mut store, &[stuck("ctx-a", "plan-a", "repo", "a", a, 10)]).unwrap(),
            1
        );
        assert_eq!(
            tell_stuck_jobs(
                &mut store,
                &[stuck("ctx-a", "plan-a", "other", "b", other, 10)]
            )
            .unwrap(),
            1,
            "another Workflow of the same plan is its own record"
        );
        assert_eq!(
            tell_stuck_jobs(&mut store, &[stuck("ctx-b", "plan-b", "repo", "a", b, 10)]).unwrap(),
            1,
            "another Context's Manager"
        );
        let to: Vec<String> = messages(&mut store).into_iter().map(|m| m.0).collect();
        assert_eq!(to, ["mgr-a", "mgr-a", "mgr-b"]);

        // A Lead's message with the same words, to the Manager of ctx-a, is not the record.
        let z = job_failed(&mut store, "ctx-a", "plan-a", "z");
        store
            .write("test", |tx| {
                tx.execute(
                    "INSERT INTO events (txn, at, kind, context, session, text, data)
                     VALUES (cpd_txn(), '2026-01-01T00:00:00.000Z', 'session-message', 'ctx-a', 'mgr-a', 'x',
                             '{\"from\":\"lead-a\",\"text\":\"Jobs of Workflow plan-a/repo failed by hand\",\"wake\":false}')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            tell_stuck_jobs(&mut store, &[stuck("ctx-a", "plan-a", "repo", "z", z, 10)]).unwrap(),
            1
        );
    }
}
