//! The `widgets` view's snapshot (commands.md §17), the Dispatch column: the
//! machine-wide numbers (slots, the engine), every Plan's PR and feedback,
//! every open Escalation and the latest Events (`plan list`, with the rows the
//! column draws from).

use std::collections::BTreeMap;

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use cpd_core::error::CpdError;
use cpd_core::events::{Event, highest_id};
use cpd_core::ids::{AttemptId, ContextId, JobKey, PlanId};
use cpd_core::store::Store;

use super::flow::{OpenEscalation, open_escalation};
use super::shared::{AttemptStatus, Checks, Place, PlanHead, PrState, bad_row, id, plan_heads, tier};

/// What the Dispatch column shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Widgets {
    pub slots: Slots,
    pub engine: Engine,
    /// Every Plan, oldest first; the view picks the ones with a PR or in Dispatch.
    pub plans: Vec<PlanCard>,
    /// Every `open` Escalation of the machine, oldest first.
    pub escalations: Vec<MachineEscalation>,
    /// The latest Events, oldest first.
    pub events: Vec<Event>,
    /// The highest Event id in this snapshot's read transaction (§17).
    pub events_cursor: i64,
}

/// The machine-wide concurrency slots (§3.4): an Attempt holds one from `starting` until `ended`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slots {
    /// `concurrency` of the config, which the caller reads (this module reads only the store).
    pub limit: u32,
    /// The Attempts not `ended`, oldest first.
    pub held: Vec<HeldSlot>,
}

impl Slots {
    pub fn used(&self) -> usize {
        self.held.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldSlot {
    pub plan: PlanId,
    pub attempt: AttemptId,
    pub tier: cpd_core::config::Tier,
    pub status: AttemptStatus,
    pub started_at: String,
}

/// The engine (§3.7), from its `watch-started` and `watch-stopped` Events. A
/// `watch-started` whose pid is gone, with no `watch-stopped` after it, is a
/// crash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Engine {
    /// No engine has ever started on this machine.
    Never,
    Running { pid: u32, since: String },
    Stopped { at: String },
    Crashed { pid: u32, since: String },
}

/// A Plan, for the Dispatch column: its head (counts, PR), its label, its PR's row and its latest PR feedback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanCard {
    pub head: PlanHead,
    /// The Lead Session's name: the Plan's label, as its tab's (commands.md §13.2).
    pub label: String,
    /// Where the Lead Session's pane is (its latest `session-moved`; `None` before any): the Dispatch column lists
    /// the Plans whose Lead is in the cockpit, in the chat slot or parked (commands.md §17).
    pub lead_place: Place,
    /// The PR as the column draws it; `None` when the Plan has no PR.
    pub pr: Option<PrRow>,
    pub feedback: Option<PrFeedback>,
}

/// A Plan's PR: number, state, and what `pr poll` last stored (contract §4.5, §7.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrRow {
    pub number: i64,
    pub state: PrState,
    /// `None` until the PR's first poll: the three columns are stored together.
    pub polled: Option<PolledPr>,
}

/// What a poll read from the PR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolledPr {
    pub title: String,
    pub decision: ReviewDecision,
    pub checks: Checks,
}

/// The PR's review decision, from `plans.pr_review_decision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewDecision {
    None,
    Approved,
    ChangesRequested,
}

/// The Plan's latest `pr-feedback` Event (§5.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrFeedback {
    pub event: i64,
    pub at: String,
    /// `OPEN` or `CLOSED`, as the Event carries it.
    pub state: String,
    pub failing_checks: Vec<String>,
    pub reviews: usize,
    pub comments: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineEscalation {
    pub context: ContextId,
    pub plan: PlanId,
    pub job: JobKey,
    pub escalation: OpenEscalation,
}

/// The Dispatch column's snapshot. `concurrency` is the config's limit; `recent`
/// is how many Events to return (the latest ones).
pub fn widgets(store: &mut Store, concurrency: u32, recent: u32) -> Result<Widgets, CpdError> {
    store.read(|conn| {
        Ok(Widgets {
            slots: slots(conn, concurrency)?,
            engine: engine(conn)?,
            plans: plan_cards(conn)?,
            escalations: escalations(conn)?,
            events: recent_events(conn, recent)?,
            events_cursor: highest_id(conn)?,
        })
    })
}

fn plan_cards(conn: &Connection) -> Result<Vec<PlanCard>, CpdError> {
    let labels = labels(conn)?;
    let mut polled = polled(conn)?;
    plan_heads(conn, None)?
        .into_iter()
        .map(|head| {
            let (label, lead_place) = labels
                .get(head.id.as_str())
                .cloned()
                .ok_or_else(|| bad_row(format!("Plan {} has no Lead Session {}", head.id, head.lead_session)))?;
            let pr = head.pr.as_ref().map(|pr| PrRow {
                number: pr.number,
                state: pr.state,
                polled: polled.remove(head.id.as_str()),
            });
            // Only a Plan with a PR has feedback: one query per PR, not per Plan.
            let feedback = if head.pr.is_some() { feedback(conn, &head.id)? } else { None };
            Ok(PlanCard { label, lead_place, pr, feedback, head })
        })
        .collect()
}

/// Plan id → its Lead Session's name and place.
fn labels(conn: &Connection) -> Result<BTreeMap<String, (String, Place)>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT p.id, s.name,
                (SELECT json_extract(e.data, '$.place') FROM events e
                  WHERE e.session = s.id AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1)
           FROM plans p JOIN sessions s ON s.id = p.lead_session",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = BTreeMap::new();
    while let Some(r) = rows.next()? {
        let (plan, name, place): (String, String, Option<String>) = (r.get(0)?, r.get(1)?, r.get(2)?);
        let place = place.map_or(Ok(Place::None), |p| Place::parse("Session place", &p))?;
        out.insert(plan, (name, place));
    }
    Ok(out)
}

/// Plan id → what `pr poll` stored, for the Plans it has polled.
fn polled(conn: &Connection) -> Result<BTreeMap<String, PolledPr>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, pr_title, pr_review_decision, pr_checks FROM plans
          WHERE pr_title IS NOT NULL OR pr_review_decision IS NOT NULL OR pr_checks IS NOT NULL",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = BTreeMap::new();
    while let Some(r) = rows.next()? {
        let plan: String = r.get(0)?;
        let (title, decision, checks): (Option<String>, Option<String>, Option<String>) = (r.get(1)?, r.get(2)?, r.get(3)?);
        let (Some(title), Some(decision), Some(checks)) = (title, decision, checks) else {
            return Err(bad_row(format!("Plan {plan} has only some of pr_title, pr_review_decision and pr_checks")));
        };
        let decision = match decision.as_str() {
            "none" => ReviewDecision::None,
            "approved" => ReviewDecision::Approved,
            "changes_requested" => ReviewDecision::ChangesRequested,
            _ => return Err(bad_row(format!("Plan {plan} has pr_review_decision {decision:?}"))),
        };
        let checks = match checks.as_str() {
            "none" => Checks::None,
            "passed" => Checks::Passed,
            "pending" => Checks::Pending,
            "failed" => Checks::Failed,
            _ => return Err(bad_row(format!("Plan {plan} has pr_checks {checks:?}"))),
        };
        out.insert(plan, PolledPr { title, decision, checks });
    }
    Ok(out)
}

fn slots(conn: &Connection, limit: u32) -> Result<Slots, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT plan, id, tier, status, started_at FROM attempts WHERE status <> 'ended' ORDER BY started_at, plan, id",
    )?;
    let mut rows = stmt.query([])?;
    let mut held = Vec::new();
    while let Some(r) = rows.next()? {
        let (plan, aid, tier_text, status): (String, String, String, String) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
        held.push(HeldSlot {
            plan: id("Plan id", &plan, PlanId::parse)?,
            tier: tier(&format!("Attempt {plan}/{aid}"), &tier_text)?,
            status: AttemptStatus::parse(&format!("Attempt {plan}/{aid} has status"), &status)?,
            started_at: r.get(4)?,
            attempt: id("Attempt id", &aid, AttemptId::parse)?,
        });
    }
    Ok(Slots { limit, held })
}

/// Whether process `pid` exists: `kill(pid, 0)` sends nothing.
fn alive(pid: u32) -> bool {
    match i32::try_from(pid).map(Pid::from_raw).map(|p| kill(p, None)) {
        Ok(Ok(())) | Ok(Err(Errno::EPERM)) => true,
        _ => false,
    }
}

fn engine(conn: &Connection) -> Result<Engine, CpdError> {
    let latest: Option<(String, String, String)> = conn
        .query_row(
            "SELECT kind, at, data FROM events WHERE kind IN ('watch-started', 'watch-stopped') ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((kind, at, data)) = latest else { return Ok(Engine::Never) };
    if kind == "watch-stopped" {
        return Ok(Engine::Stopped { at });
    }
    let pid = serde_json::from_str::<Value>(&data)
        .ok()
        .and_then(|d| d["pid"].as_u64())
        .and_then(|p| u32::try_from(p).ok())
        .ok_or_else(|| bad_row(format!("a watch-started Event has data {data}")))?;
    Ok(if alive(pid) { Engine::Running { pid, since: at } } else { Engine::Crashed { pid, since: at } })
}

fn feedback(conn: &Connection, plan: &PlanId) -> Result<Option<PrFeedback>, CpdError> {
    let latest: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT id, at, data FROM events WHERE plan = ?1 AND kind = 'pr-feedback' ORDER BY id DESC LIMIT 1",
            [plan.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    latest
        .map(|(event, at, data)| {
            let bad = || bad_row(format!("pr-feedback Event {event} has data {data}"));
            let d: Value = serde_json::from_str(&data).map_err(|_| bad())?;
            let list = |key: &str| d[key].as_array().ok_or_else(bad);
            let failing_checks = list("checks")?
                .iter()
                .map(|c| c["name"].as_str().map(str::to_owned).ok_or_else(bad))
                .collect::<Result<_, _>>()?;
            Ok(PrFeedback {
                event,
                at,
                state: d["state"].as_str().ok_or_else(bad)?.to_owned(),
                failing_checks,
                reviews: list("reviews")?.len(),
                comments: list("comments")?.len(),
            })
        })
        .transpose()
}

fn escalations(conn: &Connection) -> Result<Vec<MachineEscalation>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT p.context, e.plan, a.job, e.id, e.attempt, e.question, e.asked_at
           FROM escalations e JOIN attempts a ON a.plan = e.plan AND a.id = e.attempt
                              JOIN plans p ON p.id = e.plan
          WHERE e.status = 'open' ORDER BY e.asked_at, e.plan, e.id",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let (context, plan, job): (String, String, String) = (r.get(0)?, r.get(1)?, r.get(2)?);
        out.push(MachineEscalation {
            context: id("Context id", &context, ContextId::parse)?,
            plan: id("Plan id", &plan, PlanId::parse)?,
            job: id("Job key", &job, JobKey::parse)?,
            escalation: open_escalation(r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)?,
        });
    }
    Ok(out)
}

/// The latest `n` Events, oldest first.
fn recent_events(conn: &Connection, n: u32) -> Result<Vec<Event>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events ORDER BY id DESC LIMIT ?1",
    )?;
    let mut rows = stmt.query([n])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let (event, data): (i64, String) = (r.get(0)?, r.get(9)?);
        out.push(Event {
            id: event,
            at: r.get(1)?,
            kind: r.get(2)?,
            context: r.get(3)?,
            plan: r.get(4)?,
            job: r.get(5)?,
            attempt: r.get(6)?,
            session: r.get(7)?,
            text: r.get(8)?,
            data: serde_json::from_str(&data).map_err(|_| bad_row(format!("Event {event} has data {data}")))?,
        });
    }
    out.reverse();
    Ok(out)
}
