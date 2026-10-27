//! The `plan` view's snapshot (commands.md §17): a Plan's PRs, each with its
//! Jobs, their Dependencies, Attempts and Tiers ([`plan_boxes`], what the view
//! draws); a Plan's Jobs with its open Escalations, its latest Plan review and
//! its first PR ([`flow`], `plan show`); and which Plan the chat slot holds.

use cpd_core::attempt::ledger::Stored;
use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};

use cpd_core::config::Tier;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::highest_id;
use cpd_core::inbox::model::{Item, Kind};
use cpd_core::job::open::shown_needs;
use cpd_core::ids::{AttemptId, ContextId, EscalationId, JobKey, PlanId, PrKey, SessionId};
use cpd_core::plan::display::Display;
use cpd_core::plan::fingerprint::load_prs;
use cpd_core::plan::{jobs, plan_fingerprint, Phase};
use cpd_core::session::State;
use cpd_core::store::Store;

use super::tree::visible_items;
use super::shared::{
    AttemptStatus, JobStatus, Origin, Outcome, PlanHead, PrState, ReviewStatus, SHOWN_STATUS, bad_row, id, stored_cause, plan_heads, tier,
    value_enum,
};

/// A Plan, for the flow chart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flow {
    pub head: PlanHead,
    /// In creation order.
    pub jobs: Vec<JobNode>,
    /// The Plan's `open` Escalations, oldest first.
    pub escalations: Vec<OpenEscalation>,
    /// The latest Plan review, if there is one.
    pub review: Option<Review>,
    /// The highest Event id in this snapshot's read transaction (§17).
    pub events_cursor: i64,
}

/// A Plan as the plan view draws it (commands.md §13.2, §17): its PRs, each a box, `cancelled` ones too, and the Jobs
/// inside them. A Plan with no PR yet has none, and no Jobs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanBoxes {
    pub plan: PlanId,
    pub context: ContextId,
    /// The PRs, in key order.
    pub prs: Vec<PrNode>,
    /// Every Job of the Plan, in creation order; each names its PR.
    pub jobs: Vec<JobNode>,
    /// What of the Plan needs the User, from the same transaction.
    pub needs_user: NeedsUser,
    /// The highest Event id in this snapshot's read transaction (§17).
    pub events_cursor: i64,
}

/// What of a Plan needs the User, from the Inbox's visible items (hs-d303): the plan view draws a failed Job, or a closed
/// pull request, in the waiting colour while an agent handles it, and in red only for these.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NeedsUser {
    /// The Jobs with an open `job-stuck` item (no longer drawn by the plan view, hs-d586; the guarded tests still build it).
    pub stuck_jobs: Vec<JobKey>,
    /// The Plan's Lead has an open `dialog` item: it asks the User (about a closed pull request, say).
    pub lead_dialog: bool,
    /// The Plan's Lead is working (`sessions.state = 'working'`, hs-d586): every failed pill turns while it does.
    pub lead_working: bool,
    /// The phase of the Plan its Lead works in (`None`: no Plan, or not a Lead). Read from the Plan by dot-impl-views.
    pub lead_phase: Option<Phase>,
}

impl NeedsUser {
    /// Of the `visible` items (not snoozed, not dismissed) of `plan`, whose Lead is `lead` and whose phase is `lead_phase`.
    fn of(visible: &[Item], plan: &PlanId, lead: &SessionId, lead_working: bool, lead_phase: Option<Phase>) -> NeedsUser {
        NeedsUser {
            stuck_jobs: visible
                .iter()
                .filter(|i| i.kind == Kind::JobStuck && i.plan.as_ref() == Some(plan))
                .filter_map(|i| i.job.clone())
                .collect(),
            lead_dialog: visible.iter().any(|i| i.kind == Kind::Dialog && i.session.as_ref() == Some(lead)),
            lead_working,
            lead_phase,
        }
    }
}

/// A PR's status (contract §3.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrStatus {
    Waiting,
    Dispatch,
    Open,
    Merged,
    Cancelled,
}

impl PrStatus {
    /// As stored.
    pub fn as_str(self) -> &'static str {
        match self {
            PrStatus::Waiting => "waiting",
            PrStatus::Dispatch => "dispatch",
            PrStatus::Open => "open",
            PrStatus::Merged => "merged",
            PrStatus::Cancelled => "cancelled",
        }
    }
}

/// One PR of a Plan, for its box's title and its place among the boxes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrNode {
    pub key: PrKey,
    /// The repo's directory name: the last component of `prs.repo`.
    pub repo_name: String,
    pub base: String,
    pub status: PrStatus,
    /// The PRs it needs, sorted (none is `cancelled`: `pr cancel` refuses a PR another one needs).
    pub needs: Vec<PrKey>,
    /// Its pull request, once it has one.
    pub pull: Option<Pull>,
    /// What a PR that has not started waits for, sorted (PR references): empty when it can start (hs-d521).
    pub waiting_on: Vec<String>,
    /// For a `cancelled` PR, the PR that carried its work (a reference), from its `pr-cancelled` Event.
    pub superseded_by: Option<String>,
}

/// The state of a PR's update Job (`jobs.kind = 'update'`) that is not ended (hs-d532).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Queued,
    Running,
}

/// A PR's pull request, for its box's title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pull {
    pub number: i64,
    pub state: PrState,
    /// `pull_requests.url`: what the title's `󱆭` links to.
    pub url: String,
    /// `pull_requests.mergeable = 'conflicting'` as of the last poll; `unknown` is not a conflict.
    pub conflicting: bool,
    /// `pull_requests.merge_state = 'clean'` (GitHub's mergeStateStatus CLEAN): the PR is ready to merge; its `open` is yellow.
    pub clean: bool,
    /// The Plan's Lead Session has `state = 'working'` (contract §3.6), read with the rest: the title shows the Lead's
    /// working dot beside the state (hs-d335).
    pub lead_working: bool,
    /// The PR's not-ended update Job, if any: a conflict reads `updating` while it is `Some` (hs-d532).
    /// `pull_requests.draft`.
    pub draft: bool,
    pub update: Option<UpdateState>,
}

/// The [`UpdateState`] of a `jobs.status` read for a not-ended update Job (`None`: there is none).
pub(super) fn update_state(what: &str, status: Option<&str>) -> Result<Option<UpdateState>, CpdError> {
    match status {
        None => Ok(None),
        Some("queued") => Ok(Some(UpdateState::Queued)),
        Some("running") => Ok(Some(UpdateState::Running)),
        Some(other) => Err(bad_row(format!("{what} has an update Job with status {other:?}"))),
    }
}

/// Whether a `pull_requests.mergeable` value is a conflict; a value outside the column's set is an error.
fn conflicting(what: &str, mergeable: &str) -> Result<bool, CpdError> {
    match mergeable {
        "conflicting" => Ok(true),
        "mergeable" | "unknown" => Ok(false),
        _ => Err(bad_row(format!("{what} has mergeable {mergeable:?}"))),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobNode {
    pub key: JobKey,
    /// The Job's PR (`jobs.pr`).
    pub pr: PrKey,
    pub seq: i64,
    pub title: String,
    pub status: JobStatus,
    /// None for a stub draft (hs-d241): its Tier is not chosen yet.
    pub tier: Option<Tier>,
    /// Keys of the Jobs it needs, sorted.
    pub needs: Vec<JobKey>,
    /// Added after the Plan was locked (PR feedback).
    pub appended: bool,
    pub retry_tier: Option<Tier>,
    /// The Tier the Job is shown at (the pill, `plan render`): one derivation in `Display::job_tier`.
    pub shown_tier: Option<Tier>,
    pub failed_cause: Option<String>,
    pub cancel_reason: Option<String>,
    pub merged_sha: Option<String>,
    /// What a `queued` Job that cannot start yet waits for, sorted (Job keys, PR references): empty when it is Ready (hs-d521).
    pub waiting_on: Vec<String>,
    /// Oldest first.
    pub attempts: Vec<AttemptNode>,
}

impl JobNode {
    /// The Attempt not `ended`, if there is one (at most one exists).
    pub fn active_attempt(&self) -> Option<&AttemptNode> {
        self.attempts.iter().find(|a| a.status != AttemptStatus::Ended)
    }

    /// The Attempt a pill shows: the active one, else the last.
    pub fn current_attempt(&self) -> Option<&AttemptNode> {
        self.active_attempt().or(self.attempts.last())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptNode {
    pub id: AttemptId,
    pub number: u32,
    pub tier: Tier,
    pub origin: Origin,
    pub status: AttemptStatus,
    pub outcome: Option<Outcome>,
    pub cause: Option<Stored>,
    pub worker: Option<WorkerRef>,
    pub started_at: String,
    pub submitted_at: Option<String>,
    pub ended_at: Option<String>,
}

/// An Attempt's Worker Session and its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerRef {
    pub session: SessionId,
    pub state: State,
    pub state_label: Option<String>,
    pub state_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenEscalation {
    pub id: EscalationId,
    pub attempt: AttemptId,
    pub question: String,
    pub asked_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Review {
    pub id: String,
    pub number: u32,
    pub status: ReviewStatus,
    pub fingerprint: String,
    /// Its fingerprint is still the plan fingerprint of the draft Jobs (§2.3).
    pub current: bool,
    pub feedback: Option<String>,
    pub requested_at: String,
    pub decided_at: Option<String>,
}

fn plan_not_found(plan: &PlanId) -> CpdError {
    CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {plan}.")).with("plan", plan.as_str())
}

/// The snapshot of `plan` (`plan show`): `plan_not_found` when it does not exist.
pub fn flow(store: &mut Store, plan: &PlanId) -> Result<Flow, CpdError> {
    store.read(|conn| {
        let head = plan_heads(conn, Some(plan))?.pop().ok_or_else(|| plan_not_found(plan))?;
        let review = latest_review(conn, plan, &plan_fingerprint(conn, plan)?)?;
        let jobs = job_nodes(conn, plan)?;
        Ok(Flow { head, jobs, escalations: open_escalations(conn, plan)?, review, events_cursor: highest_id(conn)? })
    })
}

/// The plan view's snapshot of `plan`: `plan_not_found` when it does not exist. Unlike [`flow`], a Plan with no PR yet
/// is read too (R34: `plan new` adds none).
pub fn plan_boxes(store: &mut Store, plan: &PlanId) -> Result<PlanBoxes, CpdError> {
    store.read(|conn| {
        let (context, lead, lead_working, phase): (String, String, bool, String) = conn
            .query_row(
                "SELECT p.context, p.lead_session, s.state = 'working', p.phase FROM plans p JOIN sessions s ON s.id = p.lead_session WHERE p.id = ?1",
                [plan.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .ok_or_else(|| plan_not_found(plan))?;
        let phase = Phase::parse(&phase).ok_or_else(|| bad_row(format!("Plan {plan} has phase {phase:?}")))?;
        let now: String = conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |r| r.get(0))?;
        let needs_user =
            NeedsUser::of(&visible_items(conn, &now)?, plan, &id("Session id", &lead, SessionId::parse)?, lead_working, Some(phase));
        Ok(PlanBoxes {
            needs_user,
            context: id("Context id", &context, ContextId::parse)?,
            prs: pr_nodes(conn, plan, lead_working)?,
            jobs: job_nodes(conn, plan)?,
            events_cursor: highest_id(conn)?,
            plan: plan.clone(),
        })
    })
}

/// The PRs of `plan` that are not `cancelled`, in key order, with their needs; `lead_working`: the Plan's Lead is working.
fn pr_nodes(conn: &Connection, plan: &PlanId, lead_working: bool) -> Result<Vec<PrNode>, CpdError> {
    let display = Display::load(conn, plan)?;
    let pr_rows = load_prs(conn, plan)?;
    let mut needs: BTreeMap<String, Vec<PrKey>> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT pr, needs FROM pr_needs WHERE plan = ?1 ORDER BY pr, needs")?;
    for row in stmt.query_map([plan.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (pr, need) = row?;
        needs.entry(pr).or_default().push(id("PR key", &need, PrKey::parse)?);
    }
    let mut stmt = conn.prepare(
        "SELECT r.key, r.repo, r.base, r.status, r.pr_number, q.state, q.url, q.mergeable,
                COALESCE(q.merge_state = 'clean', 0), COALESCE(q.draft, 0),
                (SELECT j.status FROM jobs j WHERE j.plan = r.plan AND j.pr = r.key AND j.kind = 'update'
                    AND j.status IN ('queued', 'running') ORDER BY j.status = 'running' DESC LIMIT 1)
           FROM prs r LEFT JOIN pull_requests q ON q.id = r.pull_request
          WHERE r.plan = ?1 ORDER BY r.key",
    )?;
    let mut rows = stmt.query([plan.as_str()])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let (key, repo, base, status): (String, String, String, String) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
        let (number, row_state, url, mergeable): (Option<i64>, Option<String>, Option<String>, Option<String>) =
            (r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?);
        let clean: bool = r.get(8)?;
        let draft: bool = r.get(9)?;
        let update: Option<String> = r.get(10)?;
        let what = format!("PR {plan}/{key}");
        let status = match status.as_str() {
            "waiting" => PrStatus::Waiting,
            "dispatch" => PrStatus::Dispatch,
            "open" => PrStatus::Open,
            "merged" => PrStatus::Merged,
            "cancelled" => PrStatus::Cancelled,
            _ => return Err(bad_row(format!("{what} has status {status:?}"))),
        };
        // `pr_number` is set with `pull_request` (contract §2.5), so a pull request has its row's url
        let pull = number
            .map(|number| {
                let url = url.ok_or_else(|| bad_row(format!("{what} has pr_number {number} and no pull request row")))?;
                let mergeable = mergeable.ok_or_else(|| bad_row(format!("{what} has no mergeable")))?;
                Ok::<_, CpdError>(Pull {
                    number,
                    state: PrState::of(status == PrStatus::Merged, row_state.as_deref()),
                    url,
                    conflicting: conflicting(&what, &mergeable)?,
                    clean,
                    lead_working,
                    update: update_state(&what, update.as_deref())?,
                    draft,
                })
            })
            .transpose()?;
        let repo_name = std::path::Path::new(&repo)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| bad_row(format!("{what} has repo {repo:?}")))?;
        let row = pr_rows.iter().find(|p| p.key.as_str() == key);
        out.push(PrNode {
            waiting_on: row.map(|p| display.pr_blockers(p)).unwrap_or_default(),
            superseded_by: display.superseded_by(&key).map(str::to_owned),
            needs: {
                let mut n = needs.remove(&key).unwrap_or_default();
                // the implied need: the base PR of the Plan (a base PR of another Plan has no node here)
                if let Some(base) = row.and_then(|p| display.pr_needs(p).into_iter().find(|b| !p.needs.contains(b))).filter(|b| !b.contains('/')) {
                    n.push(id("PR key", &base, PrKey::parse)?);
                    n.sort();
                }
                n
            },
            key: id("PR key", &key, PrKey::parse)?,
            repo_name,
            base,
            status,
            pull,
        });
    }
    Ok(out)
}

/// Every Job of `plan` in creation order, with its Attempts.
fn job_nodes(conn: &Connection, plan: &PlanId) -> Result<Vec<JobNode>, CpdError> {
    let rows = jobs::load(conn, plan)?;
    let display = Display::load(conn, plan)?;
    let waiting_on: Vec<Vec<String>> = rows.iter().map(|j| display.job_blockers(&rows, j)).collect();
    let mut attempts = attempts(conn, plan)?;
    rows.iter()
        .zip(waiting_on)
        .map(|(j, waiting_on)| {
            let what = format!("Job {plan}/{}", j.key);
            let key = id("Job key", &j.key, JobKey::parse)?;
            Ok(JobNode {
                pr: id("PR key", &j.pr, PrKey::parse)?,
                status: JobStatus::parse(&format!("{what} has status"), &j.status)?,
                tier: j.tier.as_deref().map(|t| tier(&what, t)).transpose()?,
                needs: shown_needs(&rows, j).iter().map(|n| id("Job key", n, JobKey::parse)).collect::<Result<_, _>>()?,
                retry_tier: j.retry_tier.as_deref().map(|t| tier(&what, t)).transpose()?,
                shown_tier: display.job_tier(j).as_deref().map(|t| tier(&what, t)).transpose()?,
                attempts: attempts.remove(j.key.as_str()).unwrap_or_default(),
                seq: j.seq,
                waiting_on,
                title: j.title.clone(),
                appended: j.appended,
                failed_cause: j.failed_cause.clone(),
                cancel_reason: j.cancel_reason.clone(),
                merged_sha: j.merged_sha.clone(),
                key,
            })
        })
        .collect()
}

/// Every Attempt of the Plan by Job key, oldest first, with its Worker's state.
fn attempts(conn: &Connection, plan: &PlanId) -> Result<BTreeMap<String, Vec<AttemptNode>>, CpdError> {
    let mut stmt = conn.prepare(
        &format!("SELECT a.job, a.id, a.number, a.tier, a.origin, {SHOWN_STATUS}, a.outcome, a.cause, a.started_at, a.submitted_at,
                a.ended_at, a.worker_session, s.state, s.state_label, s.state_at
           FROM attempts a LEFT JOIN sessions s ON s.id = a.worker_session
          WHERE a.plan = ?1 ORDER BY a.job, a.number"),
    )?;
    let mut rows = stmt.query([plan.as_str()])?;
    let mut by_job: BTreeMap<String, Vec<AttemptNode>> = BTreeMap::new();
    while let Some(r) = rows.next()? {
        let (job, aid): (String, String) = (r.get(0)?, r.get(1)?);
        let attempt_id = id("Attempt id", &aid, AttemptId::parse)?;
        let what = format!("Attempt {plan}/{aid}");
        let (tier_text, origin, status): (String, String, String) = (r.get(3)?, r.get(4)?, r.get(5)?);
        let (outcome, cause): (Option<String>, Option<String>) = (r.get(6)?, r.get(7)?);
        let worker = match r.get::<_, Option<String>>(11)? {
            None => None,
            Some(session) => {
                let state: Option<String> = r.get(12)?;
                let state = state.ok_or_else(|| bad_row(format!("{what} has Worker {session}, which is no Session")))?;
                Some(WorkerRef {
                    session: id("Session id", &session, SessionId::parse)?,
                    state: value_enum(&format!("{what} has Worker state"), &state)?,
                    state_label: r.get(13)?,
                    state_at: r.get(14)?,
                })
            }
        };
        by_job.entry(job).or_default().push(AttemptNode {
            number: attempt_id.number().get(),
            tier: tier(&what, &tier_text)?,
            origin: Origin::parse(&format!("{what} has origin"), &origin)?,
            status: AttemptStatus::parse(&format!("{what} has status"), &status)?,
            outcome: outcome.map(|o| Outcome::parse(&format!("{what} has outcome"), &o)).transpose()?,
            cause: cause.map(|c| stored_cause(&format!("{what} has cause"), &c)).transpose()?,
            started_at: r.get(8)?,
            submitted_at: r.get(9)?,
            ended_at: r.get(10)?,
            worker,
            id: attempt_id,
        });
    }
    Ok(by_job)
}

fn open_escalations(conn: &Connection, plan: &PlanId) -> Result<Vec<OpenEscalation>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, attempt, question, asked_at FROM escalations WHERE plan = ?1 AND status = 'open' ORDER BY asked_at, id",
    )?;
    let mut rows = stmt.query([plan.as_str()])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(open_escalation(r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)?);
    }
    Ok(out)
}

/// An `open` Escalation row, parsed.
pub(super) fn open_escalation(eid: String, attempt: String, question: String, asked_at: String) -> Result<OpenEscalation, CpdError> {
    Ok(OpenEscalation {
        id: id("Escalation id", &eid, EscalationId::parse)?,
        attempt: id("Attempt id", &attempt, AttemptId::parse)?,
        question,
        asked_at,
    })
}

fn latest_review(conn: &Connection, plan: &PlanId, plan_fingerprint: &str) -> Result<Option<Review>, CpdError> {
    let row = conn
        .query_row(
            "SELECT id, number, status, fingerprint, feedback, requested_at, decided_at FROM plan_reviews
              WHERE plan = ?1 ORDER BY number DESC LIMIT 1",
            [plan.as_str()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()?;
    row.map(|(rid, number, status, fingerprint, feedback, requested_at, decided_at)| {
        Ok(Review {
            status: ReviewStatus::parse(&format!("Plan review {plan}/{rid} has status"), &status)?,
            current: fingerprint == plan_fingerprint,
            id: rid,
            number,
            fingerprint,
            feedback,
            requested_at,
            decided_at,
        })
    })
    .transpose()
}

/// The Lead Session in the cockpit's chat slot: the Plan Lead whose latest
/// `session-moved` Event says `place: "chat"` (§13.1, §17); the newest when
/// there are several.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSlot {
    pub session: SessionId,
    pub name: String,
    pub plan: PlanId,
    /// The id of that `session-moved` Event.
    pub moved_event: i64,
}

/// The chat slot's Lead, `None` while the placeholder holds it. The `plan` view
/// switches Plan when this changes.
pub fn chat_slot(store: &mut Store) -> Result<Option<ChatSlot>, CpdError> {
    store.read(|conn| {
        let row: Option<(String, String, String, i64)> = conn
            .query_row(
                "SELECT s.id, s.name, s.plan, m.id FROM sessions s
                   JOIN events m ON m.id = (SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-moved')
                  WHERE s.role = 'lead' AND s.plan IS NOT NULL AND json_extract(m.data, '$.place') = 'chat'
                  ORDER BY m.id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        row.map(|(session, name, plan, moved_event)| {
            Ok(ChatSlot {
                session: id("Session id", &session, SessionId::parse)?,
                plan: id("Plan id", &plan, PlanId::parse)?,
                name,
                moved_event,
            })
        })
        .transpose()
    })
}
