//! What every read shares: the typed enums of the rows (parsed once, at the
//! boundary: a value the schema forbids is `store_failed`, never defaulted),
//! Job counts, PR state, Sessions and Plan heads.

use std::collections::BTreeMap;

use clap::ValueEnum;
use rusqlite::{Connection, ToSql};
use serde::Deserialize;

use cpd_core::config::Tier;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::ids::{AttemptId, ContextId, PlanId, SessionId};
use cpd_core::plan::Phase;
use cpd_core::session::{Role, State};

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
    AttemptStatus { Starting = "starting", Running = "running", Merging = "merging", Ended = "ended" }
}

string_enum! {
    /// How an ended Attempt ended.
    Outcome { Merged = "merged", Failed = "failed", Cancelled = "cancelled" }
}

string_enum! {
    /// Why an Attempt exists: the Tier ladder or an explicit retry.
    Origin { Ladder = "ladder", Retry = "retry" }
}

string_enum! {
    /// An ended Attempt's cause (§3.4).
    Cause {
        Merged = "merged", CancelRequested = "cancel_requested", SetupFailed = "setup_failed",
        TimedOut = "timed_out", Crashed = "crashed", NoSubmit = "no_submit", MergeConflict = "merge_conflict",
        CasLost = "cas_lost", DirtyTarget = "dirty_target", Tampered = "tampered",
    }
}

string_enum! {
    /// An Escalation's status (§3.5).
    EscalationStatus { Open = "open", Answered = "answered", Expired = "expired" }
}

string_enum! {
    /// A Plan review's status.
    ReviewStatus { Pending = "pending", Approved = "approved", Rejected = "rejected" }
}

string_enum! {
    /// Where a Session's pane is (`session-moved`'s `place`, §6.4); `None` before any move.
    Place { Chat = "chat", Parked = "parked", Work = "work", None = "none" }
}

/// Jobs per status; a `running` Job whose Attempt is `merging` counts as `merging`.
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

    /// The tree's five counters (cpd-ui's `context.rs`): queued (the drafts in
    /// phase `plan`), running, merging, failed, merged. Cancelled Jobs are not counted.
    pub fn counters(&self, phase: Phase) -> [usize; 5] {
        let first = if phase == Phase::Plan { self.draft } else { self.queued };
        [first, self.running, self.merging, self.failed, self.merged]
    }
}

/// `plans.pr_seen` (contract §2.2): what `pr poll` has already reported.
#[derive(Deserialize)]
struct Seen {
    checks: BTreeMap<String, String>,
    #[serde(rename = "reviewIds")]
    review_ids: Vec<String>,
    #[serde(rename = "commentIds")]
    comment_ids: Vec<String>,
    #[serde(default)]
    closed: Option<bool>,
}

/// The state of a PR as cpd knows it (it never asks GitHub: the engine polls).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrState {
    Open,
    /// Closed without a merge, as `pr poll` reported.
    Closed,
    Merged,
}

/// The summary of a PR's checks, cpd-ui's `pr.rs`: any failure wins over pending, pending over passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checks {
    None,
    Passed,
    Pending,
    Failed,
}

/// A Plan's Pull Request: the columns of `plans`, with what `pr poll` last saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pr {
    pub number: i64,
    pub url: String,
    pub opened_at: String,
    pub polled_at: Option<String>,
    pub state: PrState,
    /// Check name → conclusion, as of the last poll.
    pub checks: BTreeMap<String, String>,
    pub reviews: usize,
    pub comments: usize,
}

impl Pr {
    pub fn checks_summary(&self) -> Checks {
        const FAILING: [&str; 6] = ["FAILURE", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE", "ERROR"];
        const PENDING: [&str; 5] = ["", "PENDING", "IN_PROGRESS", "QUEUED", "EXPECTED"];
        let conclusions: Vec<String> = self.checks.values().map(|c| c.to_ascii_uppercase()).collect();
        if conclusions.is_empty() {
            Checks::None
        } else if conclusions.iter().any(|c| FAILING.contains(&c.as_str())) {
            Checks::Failed
        } else if conclusions.iter().any(|c| PENDING.contains(&c.as_str())) {
            Checks::Pending
        } else {
            Checks::Passed
        }
    }
}

/// A Session as the views show it.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// It is its Context's grounded Lead.
    pub grounded_lead: bool,
    /// The ● of the tree: [`needs_you`], derived from the Session's Events.
    pub needs_you: bool,
    /// When it was archived (contract §4.5); archived Sessions are in the model.
    pub archived_at: Option<String>,
}

/// The ● rule (cockpit parity, §1.3): a Session needs the User when it is
/// `blocked`, or when it is `idle`, not in the chat slot, and its latest
/// `session-state → idle` Event is newer than its latest `session-moved
/// {place: chat}` (Event ids order commits; no such Event counts as 0).
fn needs_you(state: State, place: Place, settled: Option<i64>, seen: Option<i64>) -> bool {
    state == State::Blocked
        || (state == State::Idle && place != Place::Chat && settled.unwrap_or(0) > seen.unwrap_or(0))
}

impl SessionNode {
    /// Its chat is in the cockpit's chat slot.
    pub fn is_active(&self) -> bool {
        self.place == Place::Chat
    }

    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

const SESSION_SELECT: &str = "SELECT s.id, s.role, s.context, s.plan, s.attempt, s.name, s.file, s.forked_from, s.tier,
       s.tmux_pane, s.state, s.state_label, s.state_at, s.created_at,
       (SELECT json_extract(e.data, '$.place') FROM events e
         WHERE e.session = s.id AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1),
       EXISTS (SELECT 1 FROM plans p WHERE p.lead_session = s.id),
       EXISTS (SELECT 1 FROM contexts c WHERE c.lead_session = s.id),
       (SELECT e.id FROM events e WHERE e.session = s.id AND e.kind = 'session-state'
           AND json_extract(e.data, '$.to') = 'idle' ORDER BY e.id DESC LIMIT 1),
       (SELECT e.id FROM events e WHERE e.session = s.id AND e.kind = 'session-moved'
           AND json_extract(e.data, '$.place') = 'chat' ORDER BY e.id DESC LIMIT 1),
       s.archived_at
  FROM sessions s";

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
        out.push(SessionNode {
            needs_you: needs_you(state, place, r.get(17)?, r.get(18)?),
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
            plan_lead: r.get(15)?,
            grounded_lead: r.get(16)?,
            archived_at: r.get(19)?,
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
    pub repo: String,
    pub base: String,
    pub lead_branch: String,
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
}

impl PlanHead {
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }

    /// The Plan waits on the User: an open Escalation, a pending Plan review, or a Session with the ● (`SessionNode::needs_you`).
    pub fn needs_you(&self, sessions: &[SessionNode]) -> bool {
        self.open_escalations > 0 || self.review_pending || sessions.iter().any(|s| s.needs_you)
    }
}

/// The Plans of `only` (all of them when `None`), oldest first, with their counts: four queries, whatever the number of Plans.
pub(super) fn plan_heads(conn: &Connection, only: Option<&PlanId>) -> Result<Vec<PlanHead>, CpdError> {
    let only = only.map(|p| p.as_str().to_owned());

    let mut counts: BTreeMap<String, JobCounts> = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT j.plan, j.status, count(*),
                j.status = 'running' AND EXISTS (SELECT 1 FROM attempts a
                  WHERE a.plan = j.plan AND a.job = j.key AND a.status = 'merging')
           FROM jobs j WHERE ?1 IS NULL OR j.plan = ?1 GROUP BY j.plan, j.status, 4",
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
        "SELECT id, context, title, jira, repo, base, lead_branch, phase, lead_session, pr_number, pr_url,
                pr_opened_at, pr_seen, pr_polled_at, created_at, locked_at, all_merged_at, done_at, archived_at
           FROM plans WHERE ?1 IS NULL OR id = ?1 ORDER BY created_at, id",
    )?;
    let mut rows = stmt.query([&only])?;
    let mut heads = Vec::new();
    while let Some(r) = rows.next()? {
        let pid: String = r.get(0)?;
        let plan = id("Plan id", &pid, PlanId::parse)?;
        let field = |name: &str, value: &str| bad_row(format!("Plan {plan} has {name} {value:?}"));
        let (context, phase, lead, pr_seen): (String, String, String, String) =
            (r.get(1)?, r.get(7)?, r.get(8)?, r.get(12)?);
        let phase: Phase = value_enum(&format!("Plan {plan} has phase"), &phase)?;
        let pr = match (r.get::<_, Option<i64>>(9)?, r.get::<_, Option<String>>(10)?, r.get::<_, Option<String>>(11)?) {
            (Some(number), Some(url), Some(opened_at)) => {
                let seen: Seen = serde_json::from_str(&pr_seen).map_err(|_| field("pr_seen", &pr_seen))?;
                let state = match (phase, seen.closed) {
                    (Phase::Done, _) => PrState::Merged,
                    (_, Some(true)) => PrState::Closed,
                    _ => PrState::Open,
                };
                Some(Pr {
                    number,
                    url,
                    opened_at,
                    polled_at: r.get(13)?,
                    state,
                    checks: seen.checks,
                    reviews: seen.review_ids.len(),
                    comments: seen.comment_ids.len(),
                })
            }
            _ => None,
        };
        heads.push(PlanHead {
            context: ContextId::parse(&context).map_err(|_| field("context", &context))?,
            lead_session: SessionId::parse(&lead).map_err(|_| field("lead_session", &lead))?,
            title: r.get(2)?,
            jira: r.get(3)?,
            repo: r.get(4)?,
            base: r.get(5)?,
            lead_branch: r.get(6)?,
            counts: counts.remove(&pid).unwrap_or_default(),
            open_escalations: open.remove(&pid).unwrap_or(0),
            review_pending: pending.contains(&pid),
            created_at: r.get(14)?,
            locked_at: r.get(15)?,
            all_merged_at: r.get(16)?,
            done_at: r.get(17)?,
            archived_at: r.get(18)?,
            phase,
            pr,
            id: plan,
        });
    }
    Ok(heads)
}
