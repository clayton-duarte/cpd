//! The rules of the Inbox (contract §2.2 "Inbox item", step E8 in §3.7): from the
//! store, the clock and the engine's active errors, the [`Condition`]s that need
//! the User right now. Each kind is one small function and one query; none writes.
//! [`super::model::reconcile`] opens and closes the items.

use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::model::{Condition, Kind, Urgency};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{ContextId, JobKey, PlanId, RecordId, SessionId};
use crate::plan::Phase;
use crate::store::Store;

/// An `error` the engine reported for a step of its own and has not cleared (§3.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineError {
    pub step: String,
    pub scope: String,
    pub code: String,
    /// The id of its `error` Event.
    pub event: i64,
}

/// The `state_label`s of a `blocked` Session that are not a dialog: a Worker's
/// question goes to its Lead, a plan review has its own kind.
const NOT_A_DIALOG: [&str; 2] = ["cpd: Worker question", "cpd: plan review"];

/// Every condition of the kinds, in the order of the kinds. `now` and
/// `stuck_after` decide `job-stuck`; `errors` are the engine's active ones.
pub fn conditions(
    store: &mut Store,
    home: &CpdHome,
    now: OffsetDateTime,
    stuck_after: Duration,
    errors: &[EngineError],
) -> Result<Vec<Condition>, CpdError> {
    let mut all = store.read(dialogs)?;
    all.extend(store.read(|conn| plan_reviews(conn, home))?);
    all.extend(store.read(unread)?);
    all.extend(store.read(|conn| job_stuck(conn, now, stuck_after))?);
    all.extend(engine_errors(errors));
    all.extend(store.read(questions)?);
    all.extend(store.read(pr_failing)?);
    all.extend(store.read(pr_changes)?);
    all.extend(store.read(pr_ready)?);
    all.extend(store.read(review_requested)?);
    Ok(all)
}

/// A stored value that does not parse: the database was written outside cpd.
fn corrupt(what: &str, value: &str) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The store holds an invalid {what}: {value:?}.")).with("value", value)
}

fn id<T>(what: &str, value: &str, parse: fn(&str) -> Result<T, CpdError>) -> Result<T, CpdError> {
    parse(value).map_err(|_| corrupt(what, value))
}

fn time_of(what: &str, value: &str) -> Result<OffsetDateTime, CpdError> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| corrupt(what, value))
}

// ---- Sessions: dialog and unread ---------------------------------------------------------------------------------

/// The id of a Session's latest `session-state` Event.
const LATEST_STATE: &str = "(SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-state')";
/// The id of its latest `session-state` Event with `to: idle`.
const LATEST_IDLE: &str = "(SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-state'
                                AND json_extract(e.data, '$.to') = 'idle')";
/// The id of its latest `session-moved` Event with `place: chat`.
const LATEST_CHAT: &str = "(SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-moved'
                                AND json_extract(e.data, '$.place') = 'chat')";
/// Where its pane is: the `place` of its latest `session-moved` Event.
const PLACE: &str = "(SELECT json_extract(e.data, '$.place') FROM events e
                       WHERE e.session = s.id AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1)";

/// A Session that can need the User, with what its condition links to.
struct Seat {
    session: SessionId,
    context: ContextId,
    plan: Option<PlanId>,
    phase: Option<Phase>,
    /// The tree's label of the Session: its Plan id, `<plan>·<suffix>` for a fork, the Context id for a Manager.
    who: String,
    event: Option<i64>,
}

/// The live Lead, fork and Manager Sessions (never a Worker) that match `filter`, a SQL condition over `s`, oldest
/// first. `event` is the SQL for the id of the Event that causes the condition.
fn seats(conn: &Connection, event: &str, filter: &str) -> Result<Vec<Seat>, CpdError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT s.id, s.context, s.plan, p.phase,
                EXISTS (SELECT 1 FROM plans q WHERE q.lead_session = s.id),
                s.role = 'manager',
                {event}
           FROM sessions s LEFT JOIN plans p ON p.id = s.plan
          WHERE s.role <> 'worker' AND s.archived_at IS NULL AND {filter}
          ORDER BY s.created_at, s.id"
    ))?;
    let mut rows = stmt.query([])?;
    let mut seats = Vec::new();
    while let Some(row) = rows.next()? {
        let (session, context, plan, phase): (String, String, Option<String>, Option<String>) =
            (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
        let (plan_lead, manager): (bool, bool) = (row.get(4)?, row.get(5)?);
        let plan = plan.map(|p| id("Plan id", &p, PlanId::parse)).transpose()?;
        let who = match (&plan, plan_lead, manager) {
            (Some(plan), true, _) => plan.to_string(),
            (_, _, true) => context.clone(),
            (Some(plan), false, false) => format!("{plan}·{}", session.chars().take(4).collect::<String>()),
            (None, _, false) => session.chars().take(8).collect(),
        };
        seats.push(Seat {
            session: id("Session id", &session, SessionId::parse)?,
            context: id("Context id", &context, ContextId::parse)?,
            plan,
            phase: phase.map(|p| Phase::parse(&p).ok_or_else(|| corrupt("Plan phase", &p))).transpose()?,
            who,
            event: row.get(6)?,
        });
    }
    Ok(seats)
}

fn seat_condition(seat: Seat, kind: Kind, urgency: Urgency, label: String) -> Condition {
    Condition {
        kind,
        urgency,
        subject: format!("session:{}", seat.session),
        label,
        url: None,
        context: Some(seat.context),
        plan: seat.plan,
        job: None,
        session: Some(seat.session),
        record: None,
        event: seat.event,
    }
}

/// `dialog`: a Lead, fork or Manager asks the User something. Red when its Plan is in `dispatch` or `deploy`, else magenta.
fn dialogs(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    let [worker_question, plan_review] = NOT_A_DIALOG;
    let filter = format!("s.state = 'blocked' AND s.state_label NOT IN ('{worker_question}', '{plan_review}')");
    Ok(seats(conn, LATEST_STATE, &filter)?
        .into_iter()
        .map(|seat| {
            let urgency = match seat.phase {
                Some(Phase::Dispatch | Phase::Deploy) => Urgency::Red,
                _ => Urgency::Magenta,
            };
            let label = format!("{} asks you", seat.who);
            seat_condition(seat, Kind::Dialog, urgency, label)
        })
        .collect())
}

/// `unread`: the rule of the tree's white dot. A Lead, fork or Manager is `idle`, not in the chat slot, and its
/// latest idle turn is newer than the last time its chat was in the slot (never in the slot counts as older than
/// everything).
fn unread(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    let filter = format!("s.state = 'idle' AND {PLACE} IS NOT 'chat' AND {LATEST_IDLE} > COALESCE({LATEST_CHAT}, 0)");
    Ok(seats(conn, LATEST_IDLE, &filter)?
        .into_iter()
        .map(|seat| {
            let label = format!("{} replied", seat.who);
            seat_condition(seat, Kind::Unread, Urgency::White, label)
        })
        .collect())
}

// ---- Plans and Jobs: plan-review and job-stuck -------------------------------------------------------------------

/// The Plan's plannotator page, when its port file holds a port (the convention of `session::port`).
fn plannotator_url(home: &CpdHome, plan: &PlanId) -> Result<Option<String>, CpdError> {
    let path: &Path = &home.plannotator_port(plan);
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text.trim().parse::<u16>().ok().map(|port| format!("http://localhost:{port}/"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CpdError::new(ErrorCode::StoreFailed, format!("Cannot read {}: {e}.", path.display()))
            .with("path", path.display().to_string())),
    }
}

/// `plan-review`: a Plan's latest review is `pending`. Its subject is Plan-qualified because review ids (`r1`)
/// repeat across Plans.
fn plan_reviews(conn: &Connection, home: &CpdHome) -> Result<Vec<Condition>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT v.plan, v.id, p.context, p.lead_session
           FROM plan_reviews v JOIN plans p ON p.id = v.plan
          WHERE v.status = 'pending' AND v.number = (SELECT max(w.number) FROM plan_reviews w WHERE w.plan = v.plan)
          ORDER BY v.requested_at, v.plan",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let (plan, review, context, lead): (String, String, String, String) =
            (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
        let plan = id("Plan id", &plan, PlanId::parse)?;
        out.push(Condition {
            kind: Kind::PlanReview,
            urgency: Urgency::Magenta,
            subject: format!("review:{plan}/{review}"),
            label: format!("{plan} plan review"),
            url: plannotator_url(home, &plan)?,
            context: Some(id("Context id", &context, ContextId::parse)?),
            plan: Some(plan),
            job: None,
            session: Some(id("Session id", &lead, SessionId::parse)?),
            record: None,
            event: None,
        });
    }
    Ok(out)
}

/// `job-stuck`: a Job is `failed` and its latest `job-failed` Event is at least `stuck_after` old.
fn job_stuck(conn: &Connection, now: OffsetDateTime, stuck_after: Duration) -> Result<Vec<Condition>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT j.plan, j.key, p.context, p.lead_session, e.id, e.at
           FROM jobs j
           JOIN plans p ON p.id = j.plan
           JOIN events e ON e.id = (SELECT max(f.id) FROM events f WHERE f.plan = j.plan AND f.job = j.key AND f.kind = 'job-failed')
          WHERE j.status = 'failed'
          ORDER BY e.id",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let (plan, key, context, lead, event, at): (String, String, String, String, i64, String) =
            (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?);
        if now - time_of("Event time", &at)? < stuck_after {
            continue;
        }
        let plan = id("Plan id", &plan, PlanId::parse)?;
        out.push(Condition {
            kind: Kind::JobStuck,
            urgency: Urgency::Red,
            subject: format!("job:{plan}/{key}"),
            label: format!("{plan}/{key} failed"),
            url: None,
            context: Some(id("Context id", &context, ContextId::parse)?),
            job: Some(id("Job key", &key, JobKey::parse)?),
            plan: Some(plan),
            session: Some(id("Session id", &lead, SessionId::parse)?),
            record: None,
            event: Some(event),
        });
    }
    Ok(out)
}

// ---- The engine: engine-error ------------------------------------------------------------------------------------

/// `engine-error`: one per error the engine reported and has not cleared.
fn engine_errors(errors: &[EngineError]) -> Vec<Condition> {
    errors
        .iter()
        .map(|e| Condition {
            kind: Kind::EngineError,
            urgency: Urgency::Red,
            subject: format!("error:{}/{}/{}", e.step, e.scope, e.code),
            label: format!("engine {}: {}", e.step, e.code),
            url: None,
            context: None,
            plan: None,
            job: None,
            session: None,
            record: None,
            event: Some(e.event),
        })
        .collect()
}

// ---- Contexts: question ------------------------------------------------------------------------------------------

/// `question`: a Context record of kind `question` with no `answer` record pointing at it. Read from the records'
/// index, as `context list` counts its open questions (`repo` is private to the context area, and the index is one
/// query per tick). Its subject is Context-qualified because record ids (`hs-q1`) repeat across Contexts.
fn questions(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT q.context, q.id FROM context_records q
          WHERE q.kind = 'question'
            AND NOT EXISTS (SELECT 1 FROM context_records a
                            WHERE a.context = q.context AND a.kind = 'answer' AND a.question = q.id)
          ORDER BY q.seq",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let (context, record): (String, String) = (row.get(0)?, row.get(1)?);
        let context = id("Context id", &context, ContextId::parse)?;
        out.push(Condition {
            kind: Kind::Question,
            urgency: Urgency::Magenta,
            subject: format!("record:{context}/{record}"),
            label: format!("{record} open question"),
            url: None,
            context: Some(context),
            plan: None,
            job: None,
            session: None,
            record: Some(id("Record id", &record, RecordId::parse)?),
            event: None,
        });
    }
    Ok(out)
}

// ---- Pull requests: pr-failing, pr-changes, pr-ready, review-requested -------------------------------------------

/// The open `pull_requests` rows that match `filter`, a SQL condition over `r`, oldest first, as conditions of `kind`
/// and `urgency`. `label` makes the label from the row's repo without its owner, its number, and whether checks
/// failed. A `prs` row that points to the pull request links its Plan, Context and Lead.
fn pull_request_conditions(
    conn: &Connection,
    kind: Kind,
    urgency: Urgency,
    filter: &str,
    label: fn(&str, i64, bool) -> String,
) -> Result<Vec<Condition>, CpdError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT r.repo, r.number, r.url, r.checks = 'failed', p.context, p.id, p.lead_session
           FROM pull_requests r
           LEFT JOIN prs ON prs.pull_request = r.id
           LEFT JOIN plans p ON p.id = prs.plan
          WHERE r.state = 'open' AND {filter}
          ORDER BY r.id"
    ))?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let (repo, number, url, checks_failed): (String, i64, String, bool) =
            (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
        let (context, plan, lead): (Option<String>, Option<String>, Option<String>) =
            (row.get(4)?, row.get(5)?, row.get(6)?);
        let name = repo.split_once('/').map_or(repo.as_str(), |(_, name)| name);
        out.push(Condition {
            kind,
            urgency,
            subject: format!("pr:{repo}#{number}"),
            label: label(name, number, checks_failed),
            url: Some(url),
            context: context.map(|c| id("Context id", &c, ContextId::parse)).transpose()?,
            plan: plan.map(|p| id("Plan id", &p, PlanId::parse)).transpose()?,
            job: None,
            session: lead.map(|s| id("Session id", &s, SessionId::parse)).transpose()?,
            record: None,
            event: None,
        });
    }
    Ok(out)
}

/// `pr-failing`: an authored open pull request has failed checks or a merge conflict (`unknown` is no conflict).
/// The label names the conflict only when checks did not fail.
fn pr_failing(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    pull_request_conditions(
        conn,
        Kind::PrFailing,
        Urgency::Red,
        "r.authored = 1 AND (r.checks = 'failed' OR r.mergeable = 'conflicting')",
        |name, n, checks_failed| {
            if checks_failed { format!("{name}#{n} checks failed") } else { format!("{name}#{n} merge conflict") }
        },
    )
}

/// `pr-changes`: an authored open pull request has `changes_requested`.
fn pr_changes(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    pull_request_conditions(
        conn,
        Kind::PrChanges,
        Urgency::Red,
        "r.authored = 1 AND r.review_decision = 'changes_requested'",
        |name, n, _| format!("{name}#{n} changes requested"),
    )
}

/// `pr-ready`: an authored, open, non-draft pull request has passed checks, no conflict and no changes requested.
fn pr_ready(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    pull_request_conditions(
        conn,
        Kind::PrReady,
        Urgency::Yellow,
        "r.authored = 1 AND r.draft = 0 AND r.checks = 'passed' AND r.mergeable = 'mergeable'
         AND r.review_decision <> 'changes_requested'",
        |name, n, _| format!("{name}#{n} ready to merge"),
    )
}

/// `review-requested`: an open pull request requests the User's review.
fn review_requested(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    pull_request_conditions(
        conn,
        Kind::ReviewRequested,
        Urgency::Yellow,
        "r.review_requested = 1",
        |name, n, _| format!("review {name}#{n}"),
    )
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);
    const TEN: Duration = Duration::from_secs(600);
    const T: &str = "2026-01-01T00:00:00.000Z";
    const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    struct Fixture {
        _dir: tempfile::TempDir,
        home: CpdHome,
        store: Store,
    }

    /// A Context `ctx` with its Manager `mgr`, and a Plan `pay` in `phase` with its Lead `l-pay`; both `working`.
    fn fixture(phase: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let store = Store::open(&home, 5000).unwrap();
        let mut f = Fixture { _dir: dir, home, store };
        let locked = if matches!(phase, "grounding" | "plan") { "NULL".to_owned() } else { format!("'{T}'") };
        let done = if phase == "done" { format!("'{T}'") } else { "NULL".to_owned() };
        f.sql(&format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
             INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
               VALUES ('mgr', 'manager', 'ctx', 'Manager', '/', 'O3', 'working', '{T}', '{T}');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('pay', 'ctx', 'Pay', '{phase}', 'l-pay', '{T}', {locked}, {done});"
        ));
        f
    }

    impl Fixture {
        fn sql(&mut self, sql: &str) {
            self.store
                .write("fixture", |tx| {
                    tx.execute_batch(sql)?;
                    Ok(())
                })
                .unwrap();
        }

        /// Appends an Event and returns its id.
        fn event(&mut self, kind: &str, session: Option<&str>, job: Option<&str>, at: &str, data: &str) -> i64 {
            let session = session.map_or("NULL".to_owned(), |s| format!("'{s}'"));
            let (plan, job) = job.map_or(("NULL".to_owned(), "NULL".to_owned()), |j| ("'pay'".to_owned(), format!("'{j}'")));
            self.sql(&format!(
                "INSERT INTO events (txn, at, kind, context, plan, job, session, text, data)
                 VALUES (cpd_txn(), '{at}', '{kind}', 'ctx', {plan}, {job}, {session}, 'x', '{data}');"
            ));
            self.store.read(|c| Ok(c.query_row("SELECT max(id) FROM events", [], |r| r.get(0))?)).unwrap()
        }

        /// The Session's state changes: the row and its `session-state` Event.
        fn state(&mut self, session: &str, state: &str, label: Option<&str>) -> i64 {
            let label = label.map_or("NULL".to_owned(), |l| format!("'{l}'"));
            self.sql(&format!("UPDATE sessions SET state = '{state}', state_label = {label} WHERE id = '{session}'"));
            self.event("session-state", Some(session), None, T, &format!(r#"{{"from":"working","label":null,"to":"{state}"}}"#))
        }

        fn moved(&mut self, session: &str, place: &str) -> i64 {
            self.event("session-moved", Some(session), None, T, &format!(r#"{{"from":"none","place":"{place}"}}"#))
        }

        fn session(&mut self, id: &str, role: &str, plan: Option<&str>) {
            let plan = plan.map_or("NULL".to_owned(), |p| format!("'{p}'"));
            let attempt = if role == "worker" { "'w-a1'" } else { "NULL" };
            if role == "worker" {
                self.job("w", "running");
                self.sql(&format!(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                     VALUES ('pay', 'w-a1', 'w', 1, 'S2', 'ladder', 'running', 'cpd/pay--w-a1', '/w', '{T}');"
                ));
            }
            self.sql(&format!(
                "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
                 VALUES ('{id}', '{role}', 'ctx', {plan}, {attempt}, 'n', '/', 'O3', 'working', '{T}', '{T}');"
            ));
        }

        fn job(&mut self, key: &str, status: &str) {
            let cause = if status == "failed" { "'crashed'" } else { "NULL" };
            self.sql(&format!(
                "INSERT OR IGNORE INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pay', 'p1', '/r', 'main', 'dispatch', 'cpd/pay/p1', '/lw', 1, '{T}', '{T}');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                                   fingerprint, created_txn, created_at, failed_cause)
                   VALUES ('pay', 'p1', '{key}', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, '{FP}', 1, '{T}', {cause});"
            ));
        }

        fn review(&mut self, id: &str, status: &str) {
            let number = &id[1..];
            let decided = if status == "pending" { "NULL".to_owned() } else { format!("'{T}'") };
            self.sql(&format!(
                "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at)
                 VALUES ('pay', '{id}', {number}, '{FP}', '{status}', '{T}', {decided});"
            ));
        }

        fn of(&mut self, kind: Kind) -> Vec<Condition> {
            let all = conditions(&mut self.store, &self.home, NOW, TEN, &[]).unwrap();
            all.into_iter().filter(|c| c.kind == kind).collect()
        }
    }

    fn one(mut found: Vec<Condition>) -> Condition {
        assert_eq!(found.len(), 1, "{found:?}");
        found.remove(0)
    }

    // ---- dialog ----

    #[test]
    fn a_blocked_lead_is_a_dialog_with_its_plans_urgency() {
        for (phase, urgency) in [
            ("grounding", Urgency::Magenta),
            ("plan", Urgency::Magenta),
            ("dispatch", Urgency::Red),
            ("deploy", Urgency::Red),
            ("done", Urgency::Magenta),
        ] {
            let mut f = fixture(phase);
            let event = f.state("l-pay", "blocked", Some("Which repo?"));
            let c = one(f.of(Kind::Dialog));
            assert_eq!(c.urgency, urgency, "{phase}");
            assert_eq!(c.subject, "session:l-pay");
            assert_eq!(c.label, "pay asks you");
            assert_eq!((c.context.as_ref().map(|c| c.as_str()), c.plan.as_ref().map(|p| p.as_str())), (Some("ctx"), Some("pay")));
            assert_eq!(c.session.as_ref().map(|s| s.as_str()), Some("l-pay"));
            assert_eq!(c.event, Some(event));
        }
    }

    #[test]
    fn a_dialog_is_gone_when_the_session_is_no_longer_blocked() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "blocked", Some("Which repo?"));
        assert_eq!(f.of(Kind::Dialog).len(), 1);
        f.state("l-pay", "working", None);
        assert!(f.of(Kind::Dialog).is_empty());
    }

    #[test]
    fn a_manager_and_a_fork_are_dialogs_with_their_own_labels_and_urgency() {
        let mut f = fixture("dispatch");
        f.session("fork-1234", "lead", Some("pay"));
        f.session("loose-fork-id", "lead", None);
        for session in ["mgr", "fork-1234", "loose-fork-id"] {
            f.state(session, "blocked", Some("Which?"));
        }
        let found = f.of(Kind::Dialog);
        let by = |subject: &str| found.iter().find(|c| c.subject == subject).unwrap().clone();
        assert_eq!((by("session:mgr").label.as_str(), by("session:mgr").urgency), ("ctx asks you", Urgency::Magenta));
        assert_eq!((by("session:fork-1234").label.as_str(), by("session:fork-1234").urgency), ("pay·fork asks you", Urgency::Red));
        assert_eq!(
            (by("session:loose-fork-id").label.as_str(), by("session:loose-fork-id").urgency),
            ("loose-fo asks you", Urgency::Magenta)
        );
        assert_eq!(found.len(), 3);
    }

    #[test]
    fn a_worker_question_and_a_plan_review_label_are_not_dialogs() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "blocked", Some("cpd: Worker question"));
        f.state("mgr", "blocked", Some("cpd: plan review"));
        assert!(f.of(Kind::Dialog).is_empty());
    }

    #[test]
    fn an_archived_session_has_no_dialog_and_no_unread() {
        let mut f = fixture("dispatch");
        f.session("fork", "lead", Some("pay"));
        f.state("l-pay", "blocked", Some("Which?"));
        f.state("fork", "idle", None);
        f.sql("UPDATE sessions SET archived_at = 't' WHERE id IN ('l-pay', 'fork')");
        assert!(f.of(Kind::Dialog).is_empty());
        assert!(f.of(Kind::Unread).is_empty());
    }

    #[test]
    fn a_worker_session_never_yields_an_item() {
        let mut f = fixture("dispatch");
        f.session("wk", "worker", Some("pay"));
        f.state("wk", "blocked", Some("Which?"));
        assert!(f.of(Kind::Dialog).is_empty());
        f.state("wk", "idle", None);
        assert!(f.of(Kind::Unread).is_empty());
        f.state("wk", "blocked", Some("cpd: Worker question"));
        assert!(conditions(&mut f.store, &f.home, NOW, TEN, &[]).unwrap().is_empty());
    }

    // ---- unread ----

    #[test]
    fn an_idle_turn_outside_the_chat_slot_is_unread_until_the_chat_is_seen() {
        let mut f = fixture("dispatch");
        let idle = f.state("l-pay", "idle", None);
        let c = one(f.of(Kind::Unread));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::White, "session:l-pay", "pay replied"));
        assert_eq!(c.event, Some(idle), "the idle Event");
        assert_eq!(c.plan.as_ref().map(|p| p.as_str()), Some("pay"));

        f.moved("l-pay", "parked");
        assert_eq!(f.of(Kind::Unread).len(), 1, "parked is not seen");
        f.moved("l-pay", "chat");
        assert!(f.of(Kind::Unread).is_empty(), "in the slot: seen");
        f.moved("l-pay", "parked");
        assert!(f.of(Kind::Unread).is_empty(), "out of the slot again, but the turn is not newer than the sight");
        f.state("l-pay", "working", None);
        let idle = f.state("l-pay", "idle", None);
        assert_eq!(one(f.of(Kind::Unread)).event, Some(idle), "a new idle turn after being seen");
    }

    #[test]
    fn a_session_in_the_chat_slot_or_not_idle_is_not_unread() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "idle", None);
        f.moved("l-pay", "chat");
        assert!(f.of(Kind::Unread).is_empty());
        f.state("mgr", "working", None);
        f.state("mgr", "blocked", Some("Which?"));
        assert!(f.of(Kind::Unread).is_empty(), "blocked is a dialog, not unread");
        f.state("mgr", "stopped", None);
        assert!(f.of(Kind::Unread).is_empty());
    }

    #[test]
    fn a_manager_and_a_fork_can_be_unread() {
        let mut f = fixture("dispatch");
        f.session("fork-1234", "lead", Some("pay"));
        f.state("mgr", "idle", None);
        f.state("fork-1234", "idle", None);
        let mut labels: Vec<_> = f.of(Kind::Unread).into_iter().map(|c| c.label).collect();
        labels.sort();
        assert_eq!(labels, ["ctx replied", "pay·fork replied"]);
    }

    // ---- plan-review ----

    #[test]
    fn a_pending_review_is_a_plan_review_with_its_plannotator_page() {
        let mut f = fixture("plan");
        f.review("r1", "pending");
        let c = one(f.of(Kind::PlanReview));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Magenta, "review:pay/r1", "pay plan review"));
        assert_eq!(c.url, None, "no port file");
        assert_eq!(c.session.as_ref().map(|s| s.as_str()), Some("l-pay"), "its Lead");
        assert_eq!((c.context.as_ref().map(|c| c.as_str()), c.plan.as_ref().map(|p| p.as_str())), (Some("ctx"), Some("pay")));

        let port = f.home.plannotator_port(&PlanId::parse("pay").unwrap());
        std::fs::create_dir_all(port.parent().unwrap()).unwrap();
        std::fs::write(&port, "20000\n").unwrap();
        assert_eq!(one(f.of(Kind::PlanReview)).url.as_deref(), Some("http://localhost:20000/"));
    }

    #[test]
    fn a_decided_review_is_no_plan_review_and_only_the_latest_counts() {
        let mut f = fixture("plan");
        f.review("r1", "rejected");
        assert!(f.of(Kind::PlanReview).is_empty());
        f.review("r2", "pending");
        assert_eq!(one(f.of(Kind::PlanReview)).subject, "review:pay/r2");
        f.sql("UPDATE plan_reviews SET status = 'approved', decided_at = 't' WHERE id = 'r2'");
        assert!(f.of(Kind::PlanReview).is_empty());
    }

    // ---- job-stuck ----

    #[test]
    fn a_job_failed_for_the_stuck_time_is_job_stuck_and_clears_on_retry() {
        let mut f = fixture("dispatch");
        f.job("a", "failed");
        f.event("job-failed", None, Some("a"), "2026-01-01T00:00:00.000Z", r#"{}"#);
        let latest = f.event("job-failed", None, Some("a"), "2026-01-01T23:50:00.000Z", r#"{}"#);
        let c = one(f.of(Kind::JobStuck));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Red, "job:pay/a", "pay/a failed"));
        assert_eq!(c.job.as_ref().map(|j| j.as_str()), Some("a"));
        assert_eq!(c.session.as_ref().map(|s| s.as_str()), Some("l-pay"), "the Plan's Lead");
        assert_eq!(c.event, Some(latest), "the latest job-failed Event");

        f.sql("UPDATE jobs SET status = 'queued', failed_cause = NULL WHERE key = 'a'");
        assert!(f.of(Kind::JobStuck).is_empty(), "retried");
    }

    #[test]
    fn a_job_failed_less_than_the_stuck_time_ago_is_not_yet_stuck() {
        let mut f = fixture("dispatch");
        f.job("a", "failed");
        f.event("job-failed", None, Some("a"), "2026-01-01T23:50:01.000Z", r#"{}"#);
        assert!(f.of(Kind::JobStuck).is_empty(), "9 min 59 s");
        f.event("job-failed", None, Some("a"), "2026-01-01T23:50:00.000Z", r#"{}"#);
        assert_eq!(f.of(Kind::JobStuck).len(), 1, "exactly the stuck time counts");
        f.job("b", "queued");
        f.event("job-failed", None, Some("b"), T, r#"{}"#);
        assert_eq!(f.of(Kind::JobStuck).len(), 1, "a Job that is not failed is never stuck");
    }

    // ---- engine-error ----

    #[test]
    fn each_engine_error_is_an_item_and_none_when_cleared() {
        let mut f = fixture("dispatch");
        let error = EngineError { step: "E3".into(), scope: "plan:pay".into(), code: "store_failed".into(), event: 41 };
        let found = conditions(&mut f.store, &f.home, NOW, TEN, std::slice::from_ref(&error)).unwrap();
        let c = one(found);
        assert_eq!(c.kind, Kind::EngineError);
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Red, "error:E3/plan:pay/store_failed", "engine E3: store_failed"));
        assert_eq!((c.event, c.session, c.plan), (Some(41), None, None));
        assert!(conditions(&mut f.store, &f.home, NOW, TEN, &[]).unwrap().is_empty());
    }

    // ---- question ----

    /// A record of `kind` in `context` (made if need be); an `answer` answers `hs-q1`.
    fn record(f: &mut Fixture, context: &str, id: &str, kind: &str) {
        let a = if kind == "answer" { "'hs-q1'" } else { "NULL" };
        f.sql(&format!(
            "INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('{context}', 'C', '{T}', 'm1');
             INSERT INTO context_records (context, id, kind, at, machine, text, question)
             VALUES ('{context}', '{id}', '{kind}', '{T}', 'hs', 't', {a});"
        ));
    }

    #[test]
    fn a_question_without_an_answer_is_an_item_and_gone_when_answered() {
        let mut f = fixture("dispatch");
        record(&mut f, "ctx", "hs-q1", "question");
        record(&mut f, "ctx", "hs-d1", "decision");
        let c = one(f.of(Kind::Question));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Magenta, "record:ctx/hs-q1", "hs-q1 open question"));
        assert_eq!(c.record.as_ref().map(|r| r.to_string()), Some("hs-q1".to_owned()));
        assert_eq!((c.context.as_ref().map(|c| c.as_str()), c.plan, c.session), (Some("ctx"), None, None));

        record(&mut f, "ctx", "hs-a1", "answer");
        assert!(f.of(Kind::Question).is_empty());
    }

    #[test]
    fn the_same_record_id_in_two_contexts_is_two_questions_and_an_answer_clears_only_its_own() {
        let mut f = fixture("dispatch");
        record(&mut f, "ctx", "hs-q1", "question");
        record(&mut f, "other", "hs-q1", "question");
        let subjects: Vec<_> = f.of(Kind::Question).into_iter().map(|c| c.subject).collect();
        assert_eq!(subjects, ["record:ctx/hs-q1", "record:other/hs-q1"]);
        record(&mut f, "other", "hs-a1", "answer");
        assert_eq!(one(f.of(Kind::Question)).subject, "record:ctx/hs-q1");
    }

    // ---- pull requests ----

    /// An open, authored, non-draft pull request `acme/shop#n` with the given checks, mergeable and review values.
    fn pull_request(f: &mut Fixture, n: i64, checks: &str, mergeable: &str, review: &str) {
        f.sql(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                        mergeable, authored, review_requested)
             VALUES ('acme/shop', {n}, 'https://github.com/acme/shop/pull/{n}', 'T', 0, 'open', '{review}', '{checks}', '{T}', '{T}',
                     '{mergeable}', 1, 0);"
        ));
    }

    fn set(f: &mut Fixture, n: i64, assignments: &str) {
        f.sql(&format!("UPDATE pull_requests SET {assignments} WHERE number = {n}"));
    }

    fn pr_kinds(f: &mut Fixture) -> Vec<Kind> {
        let all = conditions(&mut f.store, &f.home, NOW, TEN, &[]).unwrap();
        all.into_iter().map(|c| c.kind).collect()
    }

    #[test]
    fn failed_checks_open_pr_failing_and_clear_when_they_pass() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 7, "failed", "mergeable", "none");
        let c = one(f.of(Kind::PrFailing));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Red, "pr:acme/shop#7", "shop#7 checks failed"));
        assert_eq!(c.url.as_deref(), Some("https://github.com/acme/shop/pull/7"));
        assert_eq!((c.context, c.plan, c.session, c.job, c.record, c.event), (None, None, None, None, None, None));
        set(&mut f, 7, "checks = 'passed'");
        assert!(f.of(Kind::PrFailing).is_empty());
    }

    #[test]
    fn a_conflicting_pr_is_failing_with_the_conflict_label_and_never_ready() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 8, "passed", "conflicting", "approved");
        let c = one(f.of(Kind::PrFailing));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Red, "pr:acme/shop#8", "shop#8 merge conflict"));
        assert!(f.of(Kind::PrReady).is_empty());
        set(&mut f, 8, "checks = 'failed'");
        assert_eq!(one(f.of(Kind::PrFailing)).label, "shop#8 checks failed", "failed checks name themselves first");
        set(&mut f, 8, "checks = 'passed', mergeable = 'mergeable'");
        assert!(f.of(Kind::PrFailing).is_empty());
        assert_eq!(f.of(Kind::PrReady).len(), 1);
    }

    #[test]
    fn an_unknown_mergeable_is_neither_failing_nor_ready() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 9, "passed", "unknown", "approved");
        assert!(pr_kinds(&mut f).is_empty());
    }

    #[test]
    fn a_pr_with_changes_requested_is_pr_changes_and_clears_with_the_decision() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 3, "pending", "mergeable", "changes_requested");
        let c = one(f.of(Kind::PrChanges));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Red, "pr:acme/shop#3", "shop#3 changes requested"));
        assert_eq!(c.url.as_deref(), Some("https://github.com/acme/shop/pull/3"));
        set(&mut f, 3, "review_decision = 'approved'");
        assert!(f.of(Kind::PrChanges).is_empty());
    }

    #[test]
    fn a_passing_mergeable_pr_is_ready_until_it_is_a_draft_or_changes_are_requested() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 4, "passed", "mergeable", "approved");
        let c = one(f.of(Kind::PrReady));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Yellow, "pr:acme/shop#4", "shop#4 ready to merge"));
        assert_eq!(c.url.as_deref(), Some("https://github.com/acme/shop/pull/4"));
        set(&mut f, 4, "review_decision = 'none'");
        assert_eq!(f.of(Kind::PrReady).len(), 1, "no review is no objection");
        set(&mut f, 4, "draft = 1");
        assert!(f.of(Kind::PrReady).is_empty(), "a draft is never ready");
        set(&mut f, 4, "draft = 0, review_decision = 'changes_requested'");
        assert!(f.of(Kind::PrReady).is_empty());
        set(&mut f, 4, "review_decision = 'approved', checks = 'pending'");
        assert!(f.of(Kind::PrReady).is_empty());
    }

    #[test]
    fn a_review_request_is_an_item_while_it_lasts() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 5, "none", "unknown", "none");
        set(&mut f, 5, "authored = 0, review_requested = 1");
        let c = one(f.of(Kind::ReviewRequested));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Yellow, "pr:acme/shop#5", "review shop#5"));
        assert_eq!(c.url.as_deref(), Some("https://github.com/acme/shop/pull/5"));
        set(&mut f, 5, "review_requested = 0");
        assert!(f.of(Kind::ReviewRequested).is_empty());
    }

    #[test]
    fn only_authored_pull_requests_are_failing_changed_or_ready() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 1, "failed", "conflicting", "changes_requested");
        pull_request(&mut f, 2, "passed", "mergeable", "approved");
        f.sql("UPDATE pull_requests SET authored = 0, review_requested = 1");
        assert_eq!(pr_kinds(&mut f), [Kind::ReviewRequested, Kind::ReviewRequested]);
    }

    #[test]
    fn a_pull_request_that_is_not_open_is_no_item() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 1, "failed", "conflicting", "changes_requested");
        pull_request(&mut f, 2, "passed", "mergeable", "approved");
        set(&mut f, 2, "review_requested = 1");
        set(&mut f, 1, "review_requested = 1");
        assert_eq!(pr_kinds(&mut f).len(), 5);
        f.sql("UPDATE pull_requests SET state = 'closed'");
        assert!(pr_kinds(&mut f).is_empty());
    }

    #[test]
    fn a_pull_request_owned_by_a_plan_links_the_plan_its_context_and_its_lead() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 6, "failed", "mergeable", "changes_requested");
        pull_request(&mut f, 7, "passed", "mergeable", "approved");
        set(&mut f, 6, "review_requested = 1");
        f.job("a", "queued");
        f.sql("UPDATE prs SET status = 'open', pull_request = (SELECT id FROM pull_requests WHERE number = 6),
                              pr_number = 6, pr_url = 'https://github.com/acme/shop/pull/6', pr_opened_at = 't'");
        for kind in [Kind::PrFailing, Kind::PrChanges, Kind::ReviewRequested] {
            let c = one(f.of(kind));
            assert_eq!(
                (c.context.as_ref().map(|c| c.as_str()), c.plan.as_ref().map(|p| p.as_str()), c.session.as_ref().map(|s| s.as_str())),
                (Some("ctx"), Some("pay"), Some("l-pay")),
                "{kind:?}"
            );
        }
        let c = one(f.of(Kind::PrReady));
        assert_eq!((c.subject.as_str(), c.plan, c.session), ("pr:acme/shop#7", None, None), "a pull request no PR owns");
    }

    // ---- all together ----

    #[test]
    fn conditions_come_in_the_order_of_the_kinds_and_a_quiet_store_has_none() {
        let mut f = fixture("dispatch");
        assert!(conditions(&mut f.store, &f.home, NOW, TEN, &[]).unwrap().is_empty());
        f.state("l-pay", "blocked", Some("Which?"));
        f.review("r1", "pending");
        f.state("mgr", "idle", None);
        f.job("a", "failed");
        f.event("job-failed", None, Some("a"), T, r#"{}"#);
        record(&mut f, "ctx", "hs-q1", "question");
        let error = EngineError { step: "E8".into(), scope: "machine".into(), code: "x".into(), event: 1 };
        let kinds: Vec<_> =
            conditions(&mut f.store, &f.home, NOW, TEN, &[error]).unwrap().into_iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            [Kind::Dialog, Kind::PlanReview, Kind::Unread, Kind::JobStuck, Kind::EngineError, Kind::Question]
        );
        pull_request(&mut f, 1, "failed", "mergeable", "changes_requested");
        pull_request(&mut f, 2, "passed", "mergeable", "approved");
        f.sql("INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored, review_requested)
               VALUES ('o/r', 3, 'u', 'T', 0, 'open', 'none', 'none', 't', 't', 0, 1)");
        let tail: Vec<_> = conditions(&mut f.store, &f.home, NOW, TEN, &[]).unwrap().into_iter().map(|c| c.kind).skip(5).collect();
        assert_eq!(tail, [Kind::PrFailing, Kind::PrChanges, Kind::PrReady, Kind::ReviewRequested]);
    }
}
