//! What every read shares: the typed enums of the rows (parsed once, at the
//! boundary: a value the schema forbids is `store_failed`, never defaulted),
//! Job counts, Workflow state, Sessions and Plan heads.

use std::collections::{BTreeMap, BTreeSet};

use clap::ValueEnum;
use rusqlite::{Connection, ToSql};

use cpd_core::attempt::ledger::Stored;
use cpd_core::config::Tier;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::job::worked_on::failed_worked_on;
use cpd_core::ids::{AttemptId, ContextId, PlanId, SessionId};
use cpd_core::plan::Phase;
use cpd_core::session::{Role, State};

use super::dot::{Dot, Work};

/// A row the database holds in a shape the binary does not accept: it was
/// changed outside cpd, or by a bug.
pub(super) fn bad_row(what: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: {what}."))
}

/// An id read from a row.
pub(super) fn id<T>(what: &str, value: &str, parse: impl FnOnce(&str) -> Result<T, CpdError>) -> Result<T, CpdError> {
    parse(value).map_err(|_| bad_row(format!("{what} {value:?}")))
}

pub(super) fn tier(what: &str, value: &str) -> Result<Tier, CpdError> {
    Tier::parse(value).map_err(|_| bad_row(format!("{what} has Tier {value:?}")))
}

/// A `clap::ValueEnum` of another area (`Phase`, `Role`, `State`), read from a row.
pub(super) fn value_enum<T: ValueEnum>(what: &str, value: &str) -> Result<T, CpdError> {
    T::from_str(value, false).map_err(|_| bad_row(format!("{what} {value:?}")))
}

/// An ended Attempt's cause, read from a row: the Ledger's list is the only one (contract §3.4).
pub(super) fn stored_cause(what: &str, value: &str) -> Result<Stored, CpdError> {
    Stored::parse(value).ok_or_else(|| bad_row(format!("{what} {value:?}")))
}

macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// As stored.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }

            pub(super) fn parse(what: &str, value: &str) -> Result<$name, CpdError> {
                match value {
                    $($text => Ok($name::$variant),)+
                    _ => Err(bad_row(format!("{what} {value:?}"))),
                }
            }
        }
    };
}

string_enum! {
    /// A Job's status (contract §3.2).
    JobStatus {
        Draft = "draft", Queued = "queued", Running = "running", Merged = "merged", Failed = "failed",
        Cancelled = "cancelled",
    }
}

string_enum! {
    /// An Attempt's status (§3.4).
    AttemptStatus {
        Starting = "starting", Running = "running", Checking = "checking", Merging = "merging", Ended = "ended"
    }
}

impl AttemptStatus {
    /// Landing (yellow): checking or merging.
    pub fn is_landing(self) -> bool {
        matches!(self, AttemptStatus::Checking | AttemptStatus::Merging)
    }
}

/// The ONE derivation of an Attempt's shown status, over an `attempts` row aliased `a` (hs-d604, hs-d605): a `running`
/// Attempt whose latest `attempt-status` Event with a `check` field says `start` is in an open check span and shows
/// `checking` (derived, never stored); every other Attempt shows its stored status. The lookup runs only for a
/// `running` Attempt, on the `events_plan` index.
pub(super) const SHOWN_STATUS: &str = "CASE WHEN a.status = 'running' AND (SELECT json_extract(e.data, '$.check')
  FROM events e WHERE e.plan = a.plan AND e.attempt = a.id AND e.kind = 'attempt-status'
   AND json_extract(e.data, '$.check') IS NOT NULL ORDER BY e.id DESC LIMIT 1) = 'start' THEN 'checking' ELSE a.status END";

string_enum! {
    /// How an ended Attempt ended.
    Outcome { Merged = "merged", Failed = "failed", Cancelled = "cancelled" }
}

string_enum! {
    /// Why an Attempt exists: the Tier ladder or an explicit retry.
    Origin { Ladder = "ladder", Retry = "retry" }
}

string_enum! {
    /// A Plan review's status.
    ReviewStatus { Pending = "pending", Approved = "approved", Rejected = "rejected", Superseded = "superseded" }
}

string_enum! {
    /// Where a Session's pane is (`session-moved`'s `place`, §6.4); `None` before any move.
    Place { Chat = "chat", Parked = "parked", Work = "work", None = "none" }
}

/// Jobs per status; a `running` Job whose Attempt is checking or merging (landing) counts as `merging`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JobCounts {
    pub draft: usize,
    pub queued: usize,
    pub running: usize,
    pub merging: usize,
    pub failed: usize,
    pub merged: usize,
    pub cancelled: usize,
}

impl JobCounts {
    pub fn total(&self) -> usize {
        self.draft + self.queued + self.running + self.merging + self.failed + self.merged + self.cancelled
    }

    /// The tree's four counters (hs-d636, commands.md §13.2): planned (the drafts and the queued), active (the running
    /// and the merging), failed and done (the merged). Cancelled Jobs are not counted.
    pub fn counters(&self) -> [usize; 4] {
        [self.draft + self.queued, self.running + self.merging, self.failed, self.merged]
    }

    /// The Job status and the merging flag that colour counter `index` (hs-d636): the planned role is the draft colour
    /// while the Plan has any draft, else the queued colour; the active role is the review colour while any of its Jobs
    /// lands (checking or merging). The one place the draft-or-queued decision is made.
    pub fn counter_role(&self, index: usize) -> (JobStatus, bool) {
        match index {
            0 => (if self.draft > 0 { JobStatus::Draft } else { JobStatus::Queued }, false),
            1 => (JobStatus::Running, self.merging > 0),
            2 => (JobStatus::Failed, false),
            _ => (JobStatus::Merged, false),
        }
    }
}

/// The state of a Workflow as cpd knows it (it never asks GitHub: the engine polls).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrState {
    Open,
    /// Closed without a merge, as `pr poll` reported.
    Closed,
    Merged,
}

impl PrState {
    /// The state of a Workflow that is `merged` or not, whose pull request row says `row_state`
    /// (`pull_requests.state`; none before `pr open`): merged once the Workflow is, closed while the pull
    /// request is closed without a merge (A11).
    pub(super) fn of(merged: bool, row_state: Option<&str>) -> PrState {
        match row_state {
            _ if merged => PrState::Merged,
            Some("closed") => PrState::Closed,
            _ => PrState::Open,
        }
    }
}

/// The summary of a Workflow's checks, cpd-ui's `pr.rs`: any failure wins over pending, pending over passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checks {
    None,
    Passed,
    Pending,
    Failed,
}

impl Checks {
    pub(super) fn parse(what: &str, value: &str) -> Result<Checks, CpdError> {
        match value {
            "none" => Ok(Checks::None),
            "passed" => Ok(Checks::Passed),
            "pending" => Ok(Checks::Pending),
            "failed" => Ok(Checks::Failed),
            _ => Err(bad_row(format!("{what} has checks {value:?}"))),
        }
    }
}

/// A Plan's Pull Request: the columns of `workflows`, with what `pr poll` last stored in its `pull_requests` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pr {
    pub number: i64,
    pub url: String,
    pub opened_at: String,
    pub state: PrState,
    /// `pull_requests.checks`, as of the last poll.
    pub checks: Checks,
}

/// A Session as the views show it.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionNode {
    pub id: SessionId,
    pub role: Role,
    pub context: ContextId,
    pub plan: Option<PlanId>,
    pub attempt: Option<AttemptId>,
    pub name: String,
    pub file: Option<String>,
    pub forked_from: Option<SessionId>,
    pub tier: Tier,
    pub tmux_pane: Option<String>,
    pub state: State,
    pub state_label: Option<String>,
    pub state_at: String,
    pub created_at: String,
    pub place: Place,
    /// It is its Plan's `lead_session`.
    pub plan_lead: bool,
    /// The dot of the tree (§17): the glyph of its state as `sessions` reads it; the tree's snapshot fills the `●` from
    /// the Inbox (`tree()`).
    pub dot: Dot,
    /// When it was archived (contract §4.5); archived Sessions are in the model.
    pub archived_at: Option<String>,
    /// When engine E13 put it to sleep (contract §2.2 "Asleep"): it keeps its place among the live rows, drawn muted.
    pub asleep_at: Option<String>,
    /// Its own cost in USD (`tree_at` fills it); `None`: no data.
    pub cost_usd: Option<f64>,
}

impl SessionNode {
    /// The Session shows a `●`, of any colour.
    pub fn needs_you(&self) -> bool {
        self.dot.needs_you()
    }

    /// Its chat is in the cockpit's chat slot.
    pub fn is_active(&self) -> bool {
        self.place == Place::Chat
    }

    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }

    pub fn is_asleep(&self) -> bool {
        self.asleep_at.is_some()
    }
}

const SESSION_SELECT: &str = "SELECT s.id, s.role, s.context, s.plan, s.attempt, s.name, s.file, s.forked_from, s.tier,
       s.tmux_pane, s.state, s.state_label, s.state_at, s.created_at,
       (SELECT json_extract(e.data, '$.place') FROM events e
         WHERE e.session = s.id AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1),
       l.lead_session IS NOT NULL,
       s.archived_at, s.asleep_at
  FROM sessions s
  LEFT JOIN (SELECT DISTINCT lead_session FROM plans) l ON l.lead_session = s.id";

/// The Sessions matching `filter` (SQL after `WHERE`, over `s`), oldest first.
pub(super) fn sessions(conn: &Connection, filter: &str, params: &[&dyn ToSql]) -> Result<Vec<SessionNode>, CpdError> {
    let mut stmt = conn.prepare(&format!("{SESSION_SELECT} WHERE {filter} ORDER BY s.created_at, s.id"))?;
    let mut rows = stmt.query(params)?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let sid: String = r.get(0)?;
        let session = id("Session id", &sid, SessionId::parse)?;
        let field = |name: &str, value: &str| bad_row(format!("Session {session} has {name} {value:?}"));
        let (role, context, plan, attempt, forked_from): (String, String, Option<String>, Option<String>, Option<String>) =
            (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(7)?);
        let (state, tier_text, place): (String, String, Option<String>) = (r.get(10)?, r.get(8)?, r.get(14)?);
        let state: State = value_enum("Session state", &state)?;
        let place = place.map_or(Ok(Place::None), |p| Place::parse("Session place", &p))?;
        let plan_lead: bool = r.get(15)?;
        out.push(SessionNode {
            dot: Dot::glyph(state),
            role: value_enum("Session role", &role)?,
            context: ContextId::parse(&context).map_err(|_| field("context", &context))?,
            plan: plan.map(|p| PlanId::parse(&p).map_err(|_| field("plan", &p))).transpose()?,
            attempt: attempt.map(|a| AttemptId::parse(&a).map_err(|_| field("attempt", &a))).transpose()?,
            name: r.get(5)?,
            file: r.get(6)?,
            forked_from: forked_from.map(|f| SessionId::parse(&f).map_err(|_| field("forked_from", &f))).transpose()?,
            tier: tier(&format!("Session {session}"), &tier_text)?,
            tmux_pane: r.get(9)?,
            state,
            state_label: r.get(11)?,
            state_at: r.get(12)?,
            created_at: r.get(13)?,
            place,
            plan_lead,
            archived_at: r.get(16)?,
            asleep_at: r.get(17)?,
            cost_usd: None,
            id: session,
        });
    }
    Ok(out)
}

/// A Plan without its Jobs: what the tree, the Dispatch column and the flow chart's header show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanHead {
    pub id: PlanId,
    pub context: ContextId,
    pub title: String,
    pub jira: Option<String>,
    pub phase: Phase,
    pub lead_session: SessionId,
    pub pr: Option<Pr>,
    pub counts: JobCounts,
    /// Escalations still `open`.
    pub open_escalations: usize,
    /// A Plan review waits for the User's decision.
    pub review_pending: bool,
    pub created_at: String,
    pub locked_at: Option<String>,
    pub all_merged_at: Option<String>,
    pub done_at: Option<String>,
    /// When it was archived (contract §4.5); archived Plans are in the model.
    pub archived_at: Option<String>,
    /// The Plan has a failed Job being worked on (pro-d88): its Lead's dot is the red loader.
    pub fixing: bool,
    /// Git/Workflow work of the Plan (pro-d200): any Workflow is open or conflicting, or an open/update Job is queued or running.
    pub git_active: bool,
}

impl PlanHead {
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }

    /// The Lead's dot is the red loader (pro-d200): a failed Job is worked on, or a Worker Escalation is open while the Lead is `working`.
    pub fn failing(&self, working: bool) -> bool {
        self.fixing || (working && self.open_escalations > 0)
    }

    /// What the Lead is working on (pro-d200).
    pub fn work(&self) -> Work {
        Work::of(self.phase, self.counts.draft > 0, self.git_active || self.counts.merging > 0)
    }
}

/// The Plans of `only` (all of them when `None`), oldest first, with their counts: four queries, whatever the number of Plans.
pub(super) fn plan_heads(conn: &Connection, only: Option<&PlanId>) -> Result<Vec<PlanHead>, CpdError> {
    let only_plan = only;
    let only = only.map(|p| p.as_str().to_owned());

    let mut counts: BTreeMap<String, JobCounts> = BTreeMap::new();
    let mut stmt = conn.prepare(
        &format!(
            "SELECT j.plan, j.status, count(*),
                j.status = 'running' AND EXISTS (SELECT 1 FROM attempts a
                  WHERE a.plan = j.plan AND a.job = j.key AND {SHOWN_STATUS} IN ('checking', 'merging'))
           FROM jobs j WHERE ?1 IS NULL OR j.plan = ?1 GROUP BY j.plan, j.status, 4"
        ),
    )?;
    let mut rows = stmt.query([&only])?;
    while let Some(r) = rows.next()? {
        let (plan, status, n, merging): (String, String, i64, bool) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
        let n = n as usize;
        let c = counts.entry(plan).or_default();
        match JobStatus::parse("Job status", &status)? {
            JobStatus::Draft => c.draft += n,
            JobStatus::Queued => c.queued += n,
            JobStatus::Running if merging => c.merging += n,
            JobStatus::Running => c.running += n,
            JobStatus::Failed => c.failed += n,
            JobStatus::Merged => c.merged += n,
            JobStatus::Cancelled => c.cancelled += n,
        }
    }

    let fixing: BTreeSet<PlanId> = failed_worked_on(conn, only_plan)?.into_iter().map(|(plan, _)| plan).collect();

    let mut git: BTreeSet<String> = BTreeSet::new();
    let mut stmt = conn.prepare(
        "SELECT r.plan FROM workflows r LEFT JOIN pull_requests q ON q.id = r.pull_request
          WHERE (?1 IS NULL OR r.plan = ?1) AND (r.status = 'open' OR q.mergeable = 'conflicting')
         UNION
         SELECT j.plan FROM jobs j
          WHERE (?1 IS NULL OR j.plan = ?1) AND j.kind IN ('open', 'update') AND j.status IN ('queued', 'running')",
    )?;
    for row in stmt.query_map([&only], |r| r.get::<_, String>(0))? {
        git.insert(row?);
    }

    let mut open: BTreeMap<String, usize> = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT plan, count(*) FROM escalations WHERE status = 'open' AND (?1 IS NULL OR plan = ?1) GROUP BY plan",
    )?;
    for row in stmt.query_map([&only], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))? {
        let (plan, n) = row?;
        open.insert(plan, n);
    }

    let mut pending: Vec<String> = Vec::new();
    let mut stmt = conn
        .prepare("SELECT DISTINCT plan FROM plan_reviews WHERE status = 'pending' AND (?1 IS NULL OR plan = ?1)")?;
    for row in stmt.query_map([&only], |r| r.get::<_, String>(0))? {
        pending.push(row?);
    }

    let mut stmt = conn.prepare(
        "SELECT p.id, p.context, p.title, p.jira, p.phase, p.lead_session, r.pr_number,
                r.pr_url, r.pr_opened_at, q.state, q.checks, p.created_at, p.locked_at, r.all_merged_at, p.done_at, p.archived_at
           FROM plans p LEFT JOIN workflows r ON r.plan = p.id
                AND r.key = (SELECT key FROM workflows WHERE plan = p.id ORDER BY created_txn, key LIMIT 1)
                LEFT JOIN pull_requests q ON q.id = r.pull_request
          WHERE p.deleted_at IS NULL AND (?1 IS NULL OR p.id = ?1) ORDER BY p.created_at, p.id",
    )?;
    let mut rows = stmt.query([&only])?;
    let mut heads = Vec::new();
    while let Some(r) = rows.next()? {
        let pid: String = r.get(0)?;
        let plan = id("Plan id", &pid, PlanId::parse)?;
        let field = |name: &str, value: &str| bad_row(format!("Plan {plan} has {name} {value:?}"));
        let (context, phase, lead): (String, String, String) = (r.get(1)?, r.get(4)?, r.get(5)?);
        let phase: Phase = value_enum(&format!("Plan {plan} has phase"), &phase)?;
        let (row_state, row_checks): (Option<String>, Option<String>) = (r.get(9)?, r.get(10)?);
        let workflow = match (r.get::<_, Option<i64>>(6)?, r.get::<_, Option<String>>(7)?, r.get::<_, Option<String>>(8)?) {
            (Some(number), Some(url), Some(opened_at)) => {
                // A Workflow with a number has its pull request (`workflows` CHECK).
                let checks = row_checks.ok_or_else(|| bad_row(format!("Workflow of Plan {plan} has no pull request row")))?;
                Some(Pr {
                    number,
                    url,
                    opened_at,
                    state: PrState::of(phase == Phase::Done, row_state.as_deref()),
                    checks: Checks::parse(&format!("Workflow of Plan {plan}"), &checks)?,
                })
            }
            _ => None,
        };
        heads.push(PlanHead {
            context: ContextId::parse(&context).map_err(|_| field("context", &context))?,
            lead_session: SessionId::parse(&lead).map_err(|_| field("lead_session", &lead))?,
            title: r.get(2)?,
            jira: r.get(3)?,
            counts: counts.remove(&pid).unwrap_or_default(),
            open_escalations: open.remove(&pid).unwrap_or(0),
            review_pending: pending.contains(&pid),
            created_at: r.get(11)?,
            locked_at: r.get(12)?,
            all_merged_at: r.get(13)?,
            done_at: r.get(14)?,
            archived_at: r.get(15)?,
            fixing: fixing.contains(&plan),
            git_active: git.contains(&pid),
            phase,
            pr: workflow,
            id: plan,
        });
    }
    Ok(heads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpd_core::home::CpdHome;
    use cpd_core::store::Store;

    /// sql-F5: no table is scanned once per Session. Every `SCAN` of the plan of
    /// `SESSION_SELECT` hangs off the top level, never off a correlated subquery.
    #[test]
    fn session_select_scans_no_table_per_session() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        let plan: Vec<(i64, i64, String)> = store
            .read(|conn| {
                let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {SESSION_SELECT} WHERE 1 ORDER BY s.created_at, s.id"))?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(3)?)))?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap();
        let per_session = |mut parent: i64| loop {
            match plan.iter().find(|(id, ..)| *id == parent) {
                Some((_, _, detail)) if detail.starts_with("CORRELATED") => return true,
                Some((_, up, _)) => parent = *up,
                None => return false,
            }
        };
        let scans: Vec<&str> = plan
            .iter()
            .filter(|(_, parent, detail)| detail.starts_with("SCAN") && per_session(*parent))
            .map(|(.., detail)| detail.as_str())
            .collect();
        assert!(scans.is_empty(), "scanned once per Session: {scans:?}\n{plan:#?}");
    }
}
