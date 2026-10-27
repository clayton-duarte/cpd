//! The `widgets` view's snapshot (commands.md §17), the Dispatch column: the
//! machine-wide numbers (slots, the engine), every open pull request of the
//! User, every open Escalation and the latest Events (`plan list`, with the
//! rows the column draws from).

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use cpd_core::error::CpdError;
use cpd_core::events::{Event, highest_id};
use cpd_core::host::spawn::Pid;
use cpd_core::inbox::model::{self, Item};
use cpd_core::ids::{AttemptId, ContextId, JobKey, PlanId, PrKey, SessionId};
use cpd_core::plan::Phase;
use cpd_core::ports::Ports;
use cpd_core::store::Store;

use super::flow::{OpenEscalation, open_escalation};
use super::tree::{db_now, inbox_in, store_home};
use super::shared::{AttemptStatus, SHOWN_STATUS, Checks, Place, PlanHead, PrState, bad_row, id, plan_heads, tier};

/// What the Dispatch column shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Widgets {
    pub slots: Slots,
    pub engine: Engine,
    /// Every Plan, oldest first.
    pub plans: Vec<PlanCard>,
    /// Every `open` row of `pull_requests` with `authored` 1 (review requests are Inbox items), oldest `created_at` first (then `repo`, `number`): what the PRs section lists.
    pub prs: Vec<OpenPr>,
    /// Every `open` Escalation of the machine, oldest first.
    pub escalations: Vec<MachineEscalation>,
    /// The latest Events, oldest first.
    pub events: Vec<Event>,
    /// The highest Event id in this snapshot's read transaction (§17).
    pub events_cursor: i64,
    /// The Inbox's open, visible items in the order of `cpd inbox list`, `engine-down` first when no engine runs and no live
    /// Manager exists: what the Inbox section draws, read as the tree's ([`super::tree::Tree::inbox`]).
    pub inbox: Vec<Item>,
    /// Whether an engine holds `locks/watch.lock` (§1.4).
    pub engine_running: bool,
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

/// An open pull request of the User, as the PRs section draws it (commands.md §13.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenPr {
    /// GitHub's `owner/name`.
    pub repo: String,
    pub number: i64,
    /// `pull_requests.url`: what the `󱆭` links to.
    pub url: String,
    pub title: String,
    pub draft: bool,
    pub checks: Checks,
    pub review_decision: ReviewDecision,
    /// `pull_requests.mergeable` is `'conflicting'` (`'unknown'` is not a conflict).
    pub conflicting: bool,
    /// The owning PR's not-ended update Job, if any: a conflict reads `updating` while it is `Some` (hs-d532).
    pub update: Option<super::UpdateState>,
    /// The PR is ready to merge (pro-d118: `cpd_core::pr::pulls::ready_to_merge_sql`, not GitHub's CLEAN alone): the merge icon shows (commands.md §13.2).
    pub clean: bool,
    /// A PR of the Plan whose Lead is in the chat slot points to this pull request.
    pub active: bool,
    /// The Lead of the owner: `plans.lead_session` of the Plan of the `prs` row that points to this pull request (the most
    /// recently created Plan when several do). `None` when no Plan owns it.
    pub lead: Option<SessionId>,
    /// The Jira key of the owner's `prs` row (the same Plan `lead` comes from); `None` when no Plan owns it or it has none.
    pub jira: Option<String>,
    /// The owner's Lead Session has `state = 'working'` (contract §3.6): the row shows the Lead's working dot (hs-d335).
    /// False when no Plan owns it.
    pub lead_working: bool,
    /// The Plan of the owner has a failed Job (the tree's `failed` rule).
    pub plan_failed: bool,
    /// The phase of the Plan that owns the pull request (`None` when no Plan owns it). Read from the Plan by
    /// dot-impl-views, by the same subquery that reads `plan_failed`.
    pub plan_phase: Option<Phase>,
    /// A merge of this pull request is running (a live merge marker, `crate::merge::marker`, hs-d495): the merge
    /// button shows the spinner instead and cannot be clicked. Not in the store: the view sets it from the markers.
    pub merging: bool,
}

/// A Plan: its head (counts), its label and the rows of its PRs that have a pull request. The PRs section no longer draws it
/// (it draws [`OpenPr`]s); `cli/tests/pr.rs` still reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanCard {
    pub head: PlanHead,
    /// The Lead Session's name: the Plan's label, as its tab's (commands.md §13.2).
    pub label: String,
    /// Where the Lead Session's pane is (its latest `session-moved`; `None` before any).
    pub lead_place: Place,
    /// Its PRs that have a pull request, in key order (R34), as the column draws them; empty when none has one.
    pub prs: Vec<PrRow>,
}

/// A PR's pull request: number, state, and what `pr poll` last stored in its row (contract §4.4, §7.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrRow {
    pub key: PrKey,
    pub number: i64,
    pub state: PrState,
    /// Always set: a PR with a pull request has its row (`prs` CHECK). Job `dispatch-prs2` drops the `Option`.
    pub polled: Option<PolledPr>,
}

/// What a poll read from the PR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolledPr {
    pub title: String,
    pub decision: ReviewDecision,
    pub checks: Checks,
}

/// The PR's review decision, from `pull_requests.review_decision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewDecision {
    None,
    Approved,
    ChangesRequested,
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
    let engine_running = model::engine_running(&store_home(store)?)?;
    store.read(|conn| {
        Ok(Widgets {
            slots: held_slots(conn, concurrency)?,
            engine: engine(conn)?,
            plans: plan_cards(conn)?,
            prs: open_pr_rows(conn)?,
            escalations: escalations(conn)?,
            events: recent_events(conn, recent)?,
            events_cursor: highest_id(conn)?,
            inbox: inbox_in(conn, &db_now(conn, None)?, engine_running)?,
            engine_running,
        })
    })
}

/// The machine-wide slots (§3.4), and the Event id of this read: the StatusBar's whole model (`widgets` reads far
/// more: the Plans' `jobs` alone are most of its cost, tui-F1).
pub fn slots(store: &mut Store, concurrency: u32) -> Result<(Slots, i64), CpdError> {
    store.read(|conn| Ok((held_slots(conn, concurrency)?, highest_id(conn)?)))
}

/// The open pull requests of the User, and the Event id of this read: the Dispatch column's whole model.
pub fn open_prs(store: &mut Store) -> Result<(Vec<OpenPr>, i64), CpdError> {
    store.read(|conn| Ok((open_pr_rows(conn)?, highest_id(conn)?)))
}

/// The open pull requests as `<owner>/<repo>#<n>` (every `open` row of `pull_requests`): what a merged marker is
/// compared with (hs-d550).
pub fn open_pulls(store: &mut Store) -> Result<std::collections::BTreeSet<String>, CpdError> {
    store.read(|conn| {
        let mut stmt = conn.prepare("SELECT repo, number FROM pull_requests WHERE state = 'open'")?;
        let rows = stmt.query_map([], |r| Ok(format!("{}#{}", r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    })
}

fn plan_cards(conn: &Connection) -> Result<Vec<PlanCard>, CpdError> {
    let labels = labels(conn)?;
    let mut pulls = pulls(conn)?;
    plan_heads(conn, None)?
        .into_iter()
        .map(|head| {
            let (label, lead_place) = labels
                .get(head.id.as_str())
                .cloned()
                .ok_or_else(|| bad_row(format!("Plan {} has no Lead Session {}", head.id, head.lead_session)))?;
            let prs = pulls.remove(head.id.as_str()).unwrap_or_default();
            Ok(PlanCard { label, lead_place, prs, head })
        })
        .collect()
}

/// The `open` pull requests the User wrote (`authored` 1), oldest first. `lead`: the owner's (see [`OpenPr::lead`]). `active`: a PR of the Plan of the chat slot's Lead (the Plan Lead whose latest
/// `session-moved` says `chat`, the newest when there are several: `chat_slot`'s rule) points to it.
fn open_pr_rows(conn: &Connection) -> Result<Vec<OpenPr>, CpdError> {
    let ready = cpd_core::pr::pulls::ready_to_merge_sql("q");
    let mut stmt = conn.prepare(&format!(
        "SELECT q.repo, q.number, q.title, q.draft, q.checks, q.review_decision, q.url,
                q.mergeable = 'conflicting', {ready},
                EXISTS (SELECT 1 FROM prs r WHERE r.pull_request = q.id AND r.plan =
                        (SELECT s.plan FROM sessions s
                           JOIN events m ON m.id = (SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-moved')
                          WHERE s.role = 'lead' AND s.plan IS NOT NULL AND json_extract(m.data, '$.place') = 'chat'
                          ORDER BY m.id DESC LIMIT 1)),
                (SELECT p.lead_session FROM prs r JOIN plans p ON p.id = r.plan
                  WHERE r.pull_request = q.id ORDER BY p.created_at DESC, p.id DESC LIMIT 1),
                COALESCE((SELECT s.state = 'working' FROM prs r JOIN plans p ON p.id = r.plan JOIN sessions s ON s.id = p.lead_session
                           WHERE r.pull_request = q.id ORDER BY p.created_at DESC, p.id DESC LIMIT 1), 0),
                (SELECT j.status FROM prs r JOIN jobs j ON j.plan = r.plan AND j.pr = r.key
                  WHERE r.pull_request = q.id AND j.kind = 'update' AND j.status IN ('queued', 'running')
                  ORDER BY j.status = 'running' DESC LIMIT 1),
                -- the tree's rule (`PlanHead::counts.failed >= 1`, tree.rs): the Plan has a failed Job
                COALESCE((SELECT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = p.id AND j.status = 'failed')
                            FROM prs r JOIN plans p ON p.id = r.plan
                           WHERE r.pull_request = q.id ORDER BY p.created_at DESC, p.id DESC LIMIT 1), 0),
                (SELECT p.phase FROM prs r JOIN plans p ON p.id = r.plan
                  WHERE r.pull_request = q.id ORDER BY p.created_at DESC, p.id DESC LIMIT 1),
                (SELECT r.jira FROM prs r JOIN plans p ON p.id = r.plan
                  WHERE r.pull_request = q.id ORDER BY p.created_at DESC, p.id DESC LIMIT 1)
           FROM pull_requests q WHERE q.state = 'open' AND q.authored = 1 ORDER BY q.created_at, q.repo, q.number",
    ))?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let (repo, number): (String, i64) = (r.get(0)?, r.get(1)?);
        let what = format!("Pull request {repo}#{number}");
        let decision: String = r.get(5)?;
        out.push(OpenPr {
            title: r.get(2)?,
            draft: r.get(3)?,
            checks: Checks::parse(&what, &r.get::<_, String>(4)?)?,
            review_decision: match decision.as_str() {
                "none" => ReviewDecision::None,
                "approved" => ReviewDecision::Approved,
                "changes_requested" => ReviewDecision::ChangesRequested,
                _ => return Err(bad_row(format!("{what} has review_decision {decision:?}"))),
            },
            url: r.get(6)?,
            conflicting: r.get(7)?,
            update: super::flow::update_state(&what, r.get::<_, Option<String>>(12)?.as_deref())?,
            clean: r.get(8)?,
            active: r.get(9)?,
            lead: r.get::<_, Option<String>>(10)?.map(|lead| id("Session id", &lead, SessionId::parse)).transpose()?,
            jira: r.get(15)?,
            lead_working: r.get(11)?,
            plan_failed: r.get(13)?,
            plan_phase: r
                .get::<_, Option<String>>(14)?
                .map(|p| Phase::parse(&p).ok_or_else(|| bad_row(format!("{what} has owning Plan phase {p:?}"))))
                .transpose()?,
            merging: false,
            repo,
            number,
        });
    }
    Ok(out)
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

/// Plan id → its PRs that have a pull request, in key order, each with what `pr poll` stored in its `pull_requests` row.
fn pulls(conn: &Connection) -> Result<BTreeMap<String, Vec<PrRow>>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT r.plan, r.key, r.pr_number, r.status, q.state, q.title, q.review_decision, q.checks
           FROM prs r JOIN pull_requests q ON q.id = r.pull_request ORDER BY r.plan, r.key",
    )?;
    let mut rows = stmt.query([])?;
    let mut out: BTreeMap<String, Vec<PrRow>> = BTreeMap::new();
    while let Some(r) = rows.next()? {
        let (plan, key, number, status, row_state): (String, String, i64, String, String) =
            (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
        let what = format!("PR {plan}/{key}");
        let state = PrState::of(status == "merged", Some(&row_state));
        let (title, decision, checks): (String, String, String) = (r.get(5)?, r.get(6)?, r.get(7)?);
        let decision = match decision.as_str() {
            "none" => ReviewDecision::None,
            "approved" => ReviewDecision::Approved,
            "changes_requested" => ReviewDecision::ChangesRequested,
            _ => return Err(bad_row(format!("{what} has review_decision {decision:?}"))),
        };
        let polled = PolledPr { title, decision, checks: Checks::parse(&what, &checks)? };
        let key = id("PR key", &key, PrKey::parse)?;
        out.entry(plan).or_default().push(PrRow { key, number, state, polled: Some(polled) });
    }
    Ok(out)
}

fn held_slots(conn: &Connection, limit: u32) -> Result<Slots, CpdError> {
    let mut stmt = conn.prepare(
        &format!("SELECT a.plan, a.id, a.tier, {SHOWN_STATUS}, a.started_at FROM attempts a WHERE a.status <> 'ended' ORDER BY a.started_at, a.plan, a.id"),
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
    Ok(if Ports::current().spawn.alive(Pid(pid)) { Engine::Running { pid, since: at } } else { Engine::Crashed { pid, since: at } })
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
