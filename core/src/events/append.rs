//! Appending Events (contract §5.1): the one function every command calls,
//! inside its own write transaction.

use rusqlite::OptionalExtension;
use serde_json::Value;

use super::kind::{Kind, Set};
use crate::error::{CpdError, ErrorCode};
use crate::ids::{AttemptId, ContextId, JobKey, PlanId, SessionId};
use crate::store::WriteTxn;

/// An Event to append: its kind, what it is about, its `text` and its `data`.
///
/// ```text
/// events::append(tx, NewEvent::job_retried("Job api queued again. Next: cpd job show api", "S2")
///     .plan(plan).job(job))?;
/// ```
#[derive(Clone, Debug)]
pub struct NewEvent {
    kind: Kind,
    context: Option<ContextId>,
    plan: Option<PlanId>,
    job: Option<JobKey>,
    attempt: Option<AttemptId>,
    session: Option<SessionId>,
    text: String,
    data: Value,
}

impl NewEvent {
    /// `text` is written for the Lead and ends with the `cpd` command(s) it
    /// may run (§5.1); for a silent kind it is a one-line description.
    /// `data` is the JSON object of §5.2's `data` column for `kind`.
    ///
    /// Untyped: only the tests of [`append`]'s refusals build an Event this way.
    /// Every command uses the constructor of its kind (`NewEvent::job_retried(..)`,
    /// `typed.rs`); `typed::tests::no_command_builds_an_untyped_event` enforces it.
    pub fn new(kind: Kind, text: impl Into<String>, data: Value) -> NewEvent {
        NewEvent {
            kind,
            context: None,
            plan: None,
            job: None,
            attempt: None,
            session: None,
            text: text.into(),
            data,
        }
    }

    /// The kind of this Event.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The `data` of this Event.
    pub fn data(&self) -> &Value {
        &self.data
    }

    /// The Context. Optional for every kind that has a Plan or a Session:
    /// [`append`] takes the Plan's Context, else the Session's.
    pub fn context(mut self, context: ContextId) -> NewEvent {
        self.context = Some(context);
        self
    }

    pub fn plan(mut self, plan: PlanId) -> NewEvent {
        self.plan = Some(plan);
        self
    }

    pub fn job(mut self, job: JobKey) -> NewEvent {
        self.job = Some(job);
        self
    }

    pub fn attempt(mut self, attempt: AttemptId) -> NewEvent {
        self.attempt = Some(attempt);
        self
    }

    pub fn session(mut self, session: SessionId) -> NewEvent {
        self.session = Some(session);
        self
    }
}

/// A malformed Event is a bug in the calling command, not bad user input, so
/// it is `internal` (exit 1, "something unexpected", contract §1.2): the
/// transaction rolls back and nothing is appended.
fn malformed(kind: Kind, message: String) -> CpdError {
    CpdError::new(
        ErrorCode::Internal,
        format!("Cannot append a {kind} Event: {message}."),
    )
    .with("kind", kind.as_str())
}

/// Appends `event` in the caller's write transaction and returns its `id`
/// (§5.1: every state change appends at least one Event, in the same
/// transaction). `at` is the transaction's time (`txns.at`).
///
/// Refuses, appending nothing, an Event that does not match §5.2: a scope
/// column set that the kind does not set (or not set that it must), a
/// missing `data` field, an unknown one, or an empty `text`.
pub fn append(tx: &WriteTxn<'_>, event: NewEvent) -> Result<i64, CpdError> {
    let NewEvent {
        kind,
        mut context,
        plan,
        job,
        attempt,
        session,
        text,
        data,
    } = event;
    let spec = kind.spec();

    for (column, set, present) in [
        ("context", spec.context, context.is_some()),
        ("plan", spec.plan, plan.is_some()),
        ("job", spec.job, job.is_some()),
        ("attempt", spec.attempt, attempt.is_some()),
        ("session", spec.session, session.is_some()),
    ] {
        // The Context is resolved below, so only "must not be set" is checked here.
        let refused = match set {
            Set::No => present,
            Set::Maybe => false,
            Set::Yes => !present && column != "context",
        };
        if refused {
            let what = if present { "must not set" } else { "must set" };
            return Err(
                malformed(kind, format!("a {kind} Event {what} {column}")).with("column", column)
            );
        }
    }
    if job.is_some() && plan.is_none() {
        return Err(malformed(kind, "a Job needs its Plan".into()).with("column", "job"));
    }
    if attempt.is_some() && job.is_none() {
        return Err(malformed(kind, "an Attempt needs its Job".into()).with("column", "attempt"));
    }

    if spec.context != Set::No && context.is_none() {
        context = match (&plan, &session) {
            (Some(plan), _) => Some(
                tx.query_row(
                    "SELECT context FROM plans WHERE id = ?1",
                    [plan.as_str()],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .ok_or_else(|| CpdError::plan_not_found(plan))?,
            ),
            (None, Some(session)) => Some(
                tx.query_row(
                    "SELECT context FROM sessions WHERE id = ?1",
                    [session.as_str()],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .ok_or_else(|| {
                    CpdError::new(ErrorCode::SessionNotFound, format!("No Session {session}."))
                        .with("session", session.as_str())
                })?,
            ),
            (None, None) => None,
        }
        .map(|c| ContextId::parse(&c))
        .transpose()?;
    }
    if spec.context == Set::Yes && context.is_none() {
        return Err(
            malformed(kind, format!("a {kind} Event must set context")).with("column", "context")
        );
    }

    let fields = data.as_object().ok_or_else(|| {
        malformed(kind, "data must be a JSON object".into()).with("column", "data")
    })?;
    if let Some(missing) = spec.required.iter().find(|k| !fields.contains_key(**k)) {
        return Err(
            malformed(kind, format!("data has no field {missing:?}")).with("field", *missing)
        );
    }
    if let Some(unknown) = fields
        .keys()
        .find(|k| !spec.required.contains(&k.as_str()) && !spec.optional.contains(&k.as_str()))
    {
        return Err(
            malformed(kind, format!("data has an unknown field {unknown:?}"))
                .with("field", unknown.as_str()),
        );
    }
    if text.is_empty() {
        return Err(malformed(kind, "text is empty".into()).with("column", "text"));
    }

    tx.execute(
        "INSERT INTO events (txn, at, kind, context, plan, job, attempt, session, text, data)
         SELECT id, at, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8 FROM txns WHERE id = cpd_txn()",
        rusqlite::params![
            kind.as_str(),
            context.as_ref().map(ContextId::as_str),
            plan.as_ref().map(PlanId::as_str),
            job.as_ref().map(JobKey::as_str),
            attempt.map(|a| a.to_string()),
            session.as_ref().map(SessionId::as_str),
            text,
            data.to_string(),
        ],
    )?;
    Ok(tx.last_insert_rowid())
}
