//! The rules of the Inbox (contract §2.2 "Inbox item", step E8 in §3.7): from the
//! store, the [`Condition`]s that need
//! the User right now. Each kind is one small function and one query; none writes.
//! [`super::model::reconcile`] opens and closes the items. A stuck Job is no Inbox
//! condition: [`stuck_jobs`] finds the ones the engine tells the Manager of (hs-d318).

use std::time::Duration;

use rusqlite::Connection;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::model::{Condition, Kind, Urgency, open_items};
use super::models;
use super::team::{self, GhPr, Post, Status};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
#[cfg(test)]
use crate::ids::WorkflowKey;
use crate::ids::{ContextId, JobKey, PlanId, SessionId};
use crate::job::home::JobHome;
use crate::job::worked_on::failed_worked_on;
use crate::plan::Phase;
use crate::plan::parked_wake::wake_state;
use crate::ports::Ports;
use crate::session::LOOP_GUARD_LABEL;
use crate::store::Store;
use crate::swr::source::models as source_models;

/// The `state_label` of a `blocked` Session that is not a dialog: a plan review has its own kind. A Worker's question
/// goes to its Lead, and the Lead that waits for the Worker is `waiting`, not `blocked` (hs-d303).
const PLAN_REVIEW: &str = "φ: plan review";

/// The `state_label` prefix of a Verdict dialog (pi adapter): the item is `<who> verdict\n<option>` (Row 1, Row 2).
const VERDICT_PREFIX: &str = "verdict: ";

/// What the team review rule needs besides the cache (contract §2.2 "The team review rule"): `inbox.bot_reviewers`. The posts
/// and the GitHub state of their pull requests are read from SQLite.
pub struct Team<'a> {
    pub bot_reviewers: &'a [String],
}

/// Every condition of the kinds, in the order of the kinds. The engine's own errors go to the Manager (hs-d314); a stalled
/// `durable` Attempt is an `error` Event, not an item. There is no `job-stuck` (hs-d318: a
/// stuck Job goes to the Manager too, see [`stuck_jobs`]). The team review
/// rule is on when `inbox.team_channel` is set (§8.2), and reads GitHub through `gh`; [`conditions_with`] takes the
/// time and both as arguments.
pub fn conditions(store: &mut Store, home: &CpdHome) -> Result<Vec<Condition>, CpdError> {
    conditions_on(&Ports::current(), store, home)
}

/// [`conditions`] on `ports`: `ports.clock.utc()` is the time of the review window.
pub fn conditions_on(
    ports: &Ports,
    store: &mut Store,
    home: &CpdHome,
) -> Result<Vec<Condition>, CpdError> {
    let config = Config::load(home)?;
    let team = (!config.inbox.team_channel.is_empty()).then_some(Team {
        bot_reviewers: &config.inbox.bot_reviewers,
    });
    conditions_with(
        store,
        home,
        ports.clock.utc(),
        config.loop_guard_turns,
        config.context_curation_min_records,
        team,
    )
}

/// [`conditions`] at `now` with `loop_guard_turns` (§8.2) and the team review rule's inputs given; `None` switches that kind off.
pub fn conditions_with(
    store: &mut Store,
    home: &CpdHome,
    now: OffsetDateTime,
    loop_guard_turns: u32,
    floor: u32,
    team: Option<Team<'_>>,
) -> Result<Vec<Condition>, CpdError> {
    let mut all = store.read(dialogs)?;
    all.extend(store.read(|conn| loop_guards(conn, loop_guard_turns))?);
    all.extend(store.read(plan_reviews)?);
    all.extend(store.read(unread)?);
    // No `question` kind (hs-d306 (1)): an open Context question opens nothing, and an item an older engine opened for
    // one closes at the next reconcile, its condition no longer holding. Questions to the User go through `ask_user_question`.
    all.extend(store.read(|conn| super::pr::conditions(conn, home, now))?);
    all.extend(store.read(super::pr::closed_conditions)?);
    all.extend(store.read(|conn| curate(conn, floor))?);
    all.extend(review_requested(store, now, team)?);
    all.extend(models_changed(store, now)?);
    all.extend(deploys(store)?);
    all.extend(store.read(drafts_parked)?);
    Ok(all)
}

/// `deploy` (contract §2.2 "The deploy rule"): one condition per active row of `swr_deploys` (swr-github.md §8), except a run whose
/// approval is queued or not yet proven (the overlay `approving`: the item closes at once).
fn deploys(store: &mut Store) -> Result<Vec<Condition>, CpdError> {
    store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT repo, run_id, run_json, pending_json FROM swr_deploys WHERE inactive_at IS NULL
              ORDER BY json_extract(run_json, '$.created_at'), run_id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::new();
        for (repo, run_id, run, pending) in rows {
            if crate::swr::github::deploy_approving(conn, &repo, run_id)? {
                continue;
            }
            let run: serde_json::Value = serde_json::from_str(&run).unwrap_or_default();
            let mut names: Vec<String> = pending
                .and_then(|p| serde_json::from_str::<Vec<serde_json::Value>>(&p).ok())
                .unwrap_or_default()
                .iter()
                .filter(|p| p["current_user_can_approve"].as_bool() == Some(true))
                .filter_map(|p| p["environment"]["name"].as_str().map(str::to_owned))
                .collect();
            names.sort();
            let text = |k: &str| run[k].as_str().unwrap_or_default().to_owned();
            out.push(Condition {
                kind: Kind::Deploy,
                urgency: Urgency::Yellow,
                subject: format!("deploy:{repo}#{run_id}"),
                label: format!(
                    "{repo} deploy {}\n{}",
                    names.join(", "),
                    text("display_title")
                ),
                url: Some(text("html_url")),
                context: None,
                plan: None,
                job: None,
                session: None,
                record: None,
                event: None,
                opened_at: Some(text("created_at")),
                head_oid: None,
                reviewed_oid: None,
            });
        }
        Ok(out)
    })
}

/// `drafts-parked` (contract §2.2 "Inbox item", hs-d731, hs-d733): a live Plan whose `parked_drafts` is not empty, whose Lead was
/// woken (W, newer than T) and has finished a turn since. The Lead must be `idle` (pro-d197). Closes when S is empty or T moves past W.
fn drafts_parked(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.context, p.lead_session FROM plans p JOIN sessions s ON s.id = p.lead_session
          WHERE p.archived_at IS NULL AND p.deleted_at IS NULL AND s.role = 'lead' AND s.archived_at IS NULL AND s.state = 'idle'
          ORDER BY p.id",
    )?;
    let plans = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (plan, context, lead) in plans {
        let plan = id("Plan id", &plan, PlanId::parse)?;
        let wake = wake_state(conn, &plan, &lead)?;
        let opened = !wake.parked.is_empty()
            && wake.wake.is_some_and(|w| w > wake.t)
            && wake.finished_turn_after_wake;
        if !opened {
            continue;
        }
        let n = wake.parked.len();
        out.push(Condition {
            kind: Kind::DraftsParked,
            urgency: Urgency::Magenta,
            subject: format!("drafts-parked:{plan}"),
            label: format!("{plan}: {n} parked draft{}", if n == 1 { "" } else { "s" }),
            url: None,
            context: Some(id("Context id", &context, ContextId::parse)?),
            plan: Some(plan),
            job: None,
            session: Some(id("Session id", &lead, SessionId::parse)?),
            record: None,
            event: None,
            opened_at: None,
            head_oid: None,
            reviewed_oid: None,
        });
    }
    Ok(out)
}

/// A stored value that does not parse: the database was written outside cpd.
fn corrupt(what: &str, value: &str) -> CpdError {
    CpdError::new(
        ErrorCode::StoreFailed,
        format!("The store holds an invalid {what}: {value:?}."),
    )
    .with("value", value)
}

pub(super) fn id<T>(
    what: &str,
    value: &str,
    parse: fn(&str) -> Result<T, CpdError>,
) -> Result<T, CpdError> {
    parse(value).map_err(|_| corrupt(what, value))
}

pub(super) fn time_of(what: &str, value: &str) -> Result<OffsetDateTime, CpdError> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| corrupt(what, value))
}

// ---- Sessions: dialog and unread ---------------------------------------------------------------------------------

/// The id of a Session's latest `session-state` Event.
const LATEST_STATE: &str =
    "(SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-state')";
/// The id of its latest finished reply turn: its latest `session-state` Event from `working`, `waiting` or `blocked` to `idle`
/// with `data.reply` true, a turn that answered the User (a start or reload, `stopped` to `idle`, is not one; a turn
/// that answered an Event or another Session is not one either; hs-d242, hs-d278).
const LATEST_IDLE: &str = "(SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-state'
                                AND json_extract(e.data, '$.to') = 'idle'
                                AND json_extract(e.data, '$.from') IN ('working', 'waiting', 'blocked')
                                AND json_extract(e.data, '$.reply') = 1)";
/// The id of its latest `session-moved` Event with `place: chat`.
const LATEST_CHAT: &str =
    "(SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-moved'
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
    /// The Session's `state_label`, empty when it has none.
    state_label: String,
    /// Its Plan's title, else its own: `COALESCE(plans.title, sessions.title)`; `None` when both are NULL. Only `unread` reads it.
    title: Option<String>,
    /// The url of the pull request its Plan reviews, when it stores one. Only a `dialog` carries it.
    reviewed_url: Option<String>,
    event: Option<i64>,
}

/// The live Lead, fork and Manager Sessions (never a Worker) that match `filter`, a SQL condition over `s`, oldest
/// first. `event` is the SQL for the id of the Event that causes the condition.
fn seats(conn: &Connection, event: &str, filter: &str) -> Result<Vec<Seat>, CpdError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT s.id, s.context, s.plan, p.phase,
                EXISTS (SELECT 1 FROM plans q WHERE q.lead_session = s.id),
                s.role = 'manager',
                {event}, COALESCE(s.state_label, ''), COALESCE(p.title, s.title),
                p.reviewed_repo, p.reviewed_number
           FROM sessions s LEFT JOIN plans p ON p.id = s.plan
          WHERE s.role <> 'worker' AND s.archived_at IS NULL AND p.deleted_at IS NULL AND {filter}
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
            (Some(plan), false, false) => {
                format!("{plan}·{}", session.chars().take(4).collect::<String>())
            }
            (None, _, false) => session.chars().take(8).collect(),
        };
        seats.push(Seat {
            session: id("Session id", &session, SessionId::parse)?,
            context: id("Context id", &context, ContextId::parse)?,
            plan,
            phase: phase
                .map(|p| Phase::parse(&p).ok_or_else(|| corrupt("Plan phase", &p)))
                .transpose()?,
            who,
            event: row.get(6)?,
            state_label: row.get(7)?,
            title: row.get(8)?,
            reviewed_url: match (
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<i64>>(10)?,
            ) {
                (Some(repo), Some(number)) => {
                    Some(super::review_plan::pr_url(&repo, number as u64))
                }
                _ => None,
            },
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
        url: if kind == Kind::Dialog {
            seat.reviewed_url
        } else {
            None
        },
        context: Some(seat.context),
        plan: seat.plan,
        job: None,
        session: Some(seat.session),
        record: None,
        event: seat.event,
        opened_at: None,
        head_oid: None,
        reviewed_oid: None,
    }
}

/// `dialog`: a Lead, fork or Manager asks the User something. Red when its Plan is in `dispatch` or `deploy`, else magenta.
fn dialogs(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    let filter = format!(
        "s.state = 'blocked' AND s.state_label NOT IN ('{PLAN_REVIEW}', '{LOOP_GUARD_LABEL}')"
    );
    Ok(seats(conn, LATEST_STATE, &filter)?
        .into_iter()
        .map(|seat| {
            let urgency = match seat.phase {
                Some(Phase::Dispatch | Phase::Deploy) => Urgency::Red,
                _ => Urgency::Magenta,
            };
            let label = if seat.state_label.starts_with(VERDICT_PREFIX) {
                format!(
                    "{} verdict\n{}",
                    seat.who,
                    &seat.state_label[VERDICT_PREFIX.len()..]
                )
            } else {
                format!("{} asks you", seat.who)
            };
            seat_condition(seat, Kind::Dialog, urgency, label)
        })
        .collect())
}

/// `loop-guard` (hs-d552, hs-d571): a Lead, fork or Manager is `blocked` with the loop guard's label. Always red, in every
/// phase. The label counts the turn that tripped the breaker, `loop_guard_turns + 1`.
fn loop_guards(conn: &Connection, loop_guard_turns: u32) -> Result<Vec<Condition>, CpdError> {
    let filter = format!("s.state = 'blocked' AND s.state_label = '{LOOP_GUARD_LABEL}'");
    Ok(seats(conn, LATEST_STATE, &filter)?
        .into_iter()
        .map(|seat| {
            let label = format!(
                "{}: loop guard, {} turns in 10 min",
                seat.who,
                loop_guard_turns + 1
            );
            seat_condition(seat, Kind::LoopGuard, Urgency::Red, label)
        })
        .collect())
}

/// `unread`: the rule of the tree's white dot. A Lead, fork or Manager is `idle`, not in the chat slot, and its
/// latest finished reply turn (`working`, `waiting` or `blocked` to `idle`, `data.reply` true) is newer than the last time its chat was in the slot
/// (never in the slot counts as older than everything). Label `<who> replied`, then `\n` and the title (the Plan's, else the
/// Session's, as is) when there is one: Row 2. A reply turn already dismissed in a closed `unread` item never reopens
/// (pro-d425): a reload or a non-reply turn returns the session to `idle` on the same reply Event, and only a newer reply turn opens a fresh item.
fn unread(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    let filter = format!(
        "s.state = 'idle' AND {PLACE} IS NOT 'chat' AND {LATEST_IDLE} > COALESCE({LATEST_CHAT}, 0) \
         AND NOT EXISTS (SELECT 1 FROM inbox_items i WHERE i.kind = 'unread' AND i.session = s.id AND i.closed_at IS NOT NULL AND i.dismissed_at IS NOT NULL AND COALESCE(i.event, 0) >= {LATEST_IDLE})"
    );
    Ok(seats(conn, LATEST_IDLE, &filter)?
        .into_iter()
        .map(|seat| {
            let label = match &seat.title {
                Some(title) => format!("{} replied\n{title}", seat.who),
                None => format!("{} replied", seat.who),
            };
            seat_condition(seat, Kind::Unread, Urgency::Cyan, label)
        })
        .collect())
}

// ---- Plans and Jobs: plan-review, and the stuck Jobs -------------------------------------------------------------

/// `curate-retire` (hs-d549, hs-d551): one cyan item per Context while the Context has pending retirements; the label
/// carries the count. Dismissing one decides nothing.
fn curate(conn: &Connection, floor: u32) -> Result<Vec<Condition>, CpdError> {
    let mut out = Vec::new();
    // No curate-docs kind (pro-d327): an item an older engine opened for one closes at the next reconcile, its condition no longer holding.
    for (kind, name, noun, sql) in [(
        Kind::CurateRetire,
        "curate-retire",
        "to retire",
        "SELECT context, count(*) FROM record_retirements WHERE status = 'pending' AND context IN (SELECT id FROM contexts WHERE archived_at IS NULL AND deleted_at IS NULL AND (SELECT count(*) FROM context_records WHERE context = contexts.id) >= ?1) GROUP BY context ORDER BY context",
    )] {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map([floor], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (raw, n) in rows {
            let context = id("context id", &raw, ContextId::parse)?;
            out.push(Condition {
                kind,
                urgency: Urgency::White,
                subject: format!("{name}:{context}"),
                label: format!("{context}: {n} {noun}"),
                url: None,
                context: Some(context),
                plan: None,
                job: None,
                session: None,
                record: None,
                event: None,
                opened_at: None,
                head_oid: None,
                reviewed_oid: None,
            });
        }
    }
    Ok(out)
}

/// `plan-review`: a Plan's latest review is `pending`. Label `<plan> plan review\n<title>`, `<plan> plan review` with no title. Its subject is Plan-qualified because review ids (`r1`)
/// repeat across Plans.
fn plan_reviews(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT v.plan, v.id, p.context, p.lead_session, v.title
           FROM plan_reviews v JOIN plans p ON p.id = v.plan
          WHERE v.status = 'pending' AND v.number = (SELECT max(w.number) FROM plan_reviews w WHERE w.plan = v.plan)
          ORDER BY v.requested_at, v.plan",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let (plan, review, context, lead, title): (
            String,
            String,
            String,
            Option<String>,
            Option<String>,
        ) = (
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
        );
        let plan = id("Plan id", &plan, PlanId::parse)?;
        out.push(Condition {
            kind: Kind::PlanReview,
            urgency: Urgency::Magenta,
            subject: format!("review:{plan}/{review}"),
            // A review requested before `plan_reviews.title` has none (hs-d291, R3).
            label: match title {
                Some(title) => format!("{plan} plan review\n{title}"),
                None => format!("{plan} plan review"),
            },
            url: None,
            context: Some(id("Context id", &context, ContextId::parse)?),
            plan: Some(plan),
            job: None,
            session: lead
                .as_deref()
                .map(|lead| id("Session id", lead, SessionId::parse))
                .transpose()?,
            record: None,
            event: None,
            opened_at: None,
            head_oid: None,
            reviewed_oid: None,
        });
    }
    Ok(out)
}

/// A stuck Job (hs-d318): what the engine tells its Context's Manager. Not an Inbox condition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StuckJob {
    pub plan: PlanId,
    /// Where the Job belongs: E8 groups the stuck Jobs of one Workflow, or of the Plan's explore Jobs, into one message (hs-d647).
    pub home: JobHome,
    pub job: JobKey,
    /// The Plan's Context.
    pub context: ContextId,
    /// The id of the Job's latest `job-failed` Event: a new failure is a new one.
    pub event: i64,
    /// Whole minutes since that Event.
    pub minutes: i64,
    /// The Job's `failed_cause`.
    pub cause: String,
}

/// The stuck Jobs, oldest failure first: a Job is `failed`, its latest `job-failed` Event is at least `stuck_after` old, and
/// it is not being worked on (pro-d88, [`failed_worked_on`]: the Job is stuck from the next tick after
/// that stops). Read only: E8 tells the Manager.
pub fn stuck_jobs(
    store: &mut Store,
    now: OffsetDateTime,
    stuck_after: Duration,
) -> Result<Vec<StuckJob>, CpdError> {
    store.read(|conn| {
        let worked_on = failed_worked_on(conn, None)?;
        let mut stmt = conn.prepare(
            "SELECT j.plan, j.workflow, j.key, p.context, e.id, e.at, j.failed_cause
               FROM jobs j
               JOIN plans p ON p.id = j.plan
               JOIN events e ON e.id = (SELECT max(f.id) FROM events f WHERE f.plan = j.plan AND f.job = j.key AND f.kind = 'job-failed')
              WHERE j.status = 'failed'
              ORDER BY e.id",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let (plan, workflow, key, context, event, at, cause): (String, Option<String>, String, String, i64, String, String) =
                (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?);
            let age = now - time_of("Event time", &at)?;
            if age < stuck_after {
                continue;
            }
            let (plan, job) = (id("Plan id", &plan, PlanId::parse)?, id("Job key", &key, JobKey::parse)?);
            if worked_on.contains(&(plan.clone(), job.clone())) {
                continue;
            }
            out.push(StuckJob {
                plan,
                home: JobHome::of("Workflow key", workflow.as_deref())?,
                job,
                context: id("Context id", &context, ContextId::parse)?,
                event,
                minutes: age.whole_minutes(),
                cause,
            });
        }
        Ok(out)
    })
}

/// The stuck Jobs of one Workflow (hs-d647): the manager is told once per pull request, not once per Job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StuckGroup {
    pub plan: PlanId,
    pub home: JobHome,
    /// The Plan's Context, and its Manager's.
    pub context: ContextId,
    /// The group's newest `job-failed` Event id: the once-record must be newer than it.
    pub event: i64,
    /// The group's Jobs, oldest `job-failed` Event first.
    pub jobs: Vec<StuckJob>,
}

/// Groups the stuck Jobs by their Plan and Workflow, keeping `jobs`' oldest-first order inside a group and ordering the groups
/// by their first Job's `job-failed` Event id (hs-d647). `jobs` is the oldest-first result of [`stuck_jobs`].
#[must_use]
pub fn stuck_groups(jobs: Vec<StuckJob>) -> Vec<StuckGroup> {
    let mut groups: Vec<StuckGroup> = Vec::new();
    for job in jobs {
        match groups
            .iter_mut()
            .find(|g| g.plan == job.plan && g.home == job.home)
        {
            Some(group) => {
                group.event = group.event.max(job.event);
                group.jobs.push(job);
            }
            None => groups.push(StuckGroup {
                plan: job.plan.clone(),
                home: job.home.clone(),
                context: job.context.clone(),
                event: job.event,
                jobs: vec![job],
            }),
        }
    }
    groups
}

// ---- Pull requests: pr-ready, review-requested -------------------------------------------

/// `review-requested` (label `review <repo>#<n>\n<GitHub title>`): a pull request posted in the team channel in the last 7 days that hermes' rule calls
/// `NEEDS_REVIEW` (contract §2.2 "The team review rule", hs-d320). GitHub's own review requests open nothing.
///
/// The posts are `swr_slack_posts` (the `slack` source); a channel never read is "unknown", not "empty", and so is a GitHub that
/// cannot be read (the call fails, or no pull request of the posts can be read, hermes `main`): the open items stay
/// as they are, and no `engine-error` is opened (hs-d314: the failure goes to an `error` Event). A single
/// pull request GitHub cannot find is dropped from this run, as hermes' `build` drops it.
fn review_requested(
    store: &mut Store,
    now: OffsetDateTime,
    team: Option<Team<'_>>,
) -> Result<Vec<Condition>, CpdError> {
    let Some(team) = team else {
        return Ok(Vec::new());
    };
    let cache = match team::read_cache(store) {
        Ok(Some(cache)) => cache,
        Ok(None) => return kept_review_items(store),
        Err(_) => return kept_review_items(store),
    };
    let cutoff = now.unix_timestamp() as f64 - team::WINDOW.as_secs_f64();
    let posts: Vec<&Post> = cache
        .posts
        .iter()
        .filter(|p| p.primary && p.ts >= cutoff)
        .collect();
    let plans = super::review_plan::open_plan_refs(store)?;
    if posts.is_empty() && plans.is_empty() {
        return Ok(Vec::new());
    }
    let mut refs: Vec<(String, u64)> = posts.iter().map(|p| (p.repo.clone(), p.number)).collect();
    for (_, repo, number) in &plans {
        if !refs.contains(&(repo.clone(), *number)) {
            refs.push((repo.clone(), *number));
        }
    }
    let lookup = team::lookup_from_store(store, &refs)?;
    super::review_plan::finish_closed(store, &lookup)?;
    if posts.is_empty() {
        return Ok(Vec::new());
    }
    let read = |p: &Post| -> Option<&GhPr> {
        lookup
            .workflows
            .get(&(p.repo.clone(), p.number))
            .and_then(Option::as_ref)
    };
    if posts.iter().all(|p| read(p).is_none()) {
        return kept_review_items(store);
    }
    let settled = settled_review_subjects(store)?;
    let requested: std::collections::HashSet<(String, u64)> = store.read(|c| {
        let mut stmt = c.prepare(
            "SELECT repo, number FROM pull_requests WHERE review_requested = 1 AND state = 'open'",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    })?;
    Ok(posts
        .into_iter()
        .filter(|post| {
            let subject = format!("pr:{}#{}", post.repo, post.number);
            match read(post).and_then(|w| team::followup_oids(w, &lookup.me)) {
                Some((head, _)) => !settled.heads.contains(&(subject, head.to_owned())),
                None => !settled.subjects.contains(&subject),
            }
        })
        .filter_map(|post| {
            let workflow = read(post)?;
            // A pull request nobody has asked the User to review yet opens nothing, whatever the channel says: the
            // reasons that name the User's own part (a claim, a follow-up, a tag) need no request.
            match team::status(post, workflow, &lookup.me, team.bot_reviewers) {
                Status::NeedsReview(reason) => (!matches!(reason, team::NO_REVIEW | team::AI_ONLY)
                    || requested.contains(&(post.repo.clone(), post.number)))
                .then_some((post, workflow)),
                _ => None,
            }
        })
        .map(|(post, workflow)| {
            let name = post
                .repo
                .split_once('/')
                .map_or(post.repo.as_str(), |(_, name)| name);
            let followup = team::followup_oids(workflow, &lookup.me);
            Condition {
                kind: Kind::ReviewRequested,
                urgency: Urgency::Blue,
                subject: format!("pr:{}#{}", post.repo, post.number),
                label: format!("review {name}#{}\n{}", post.number, workflow.title),
                url: Some(format!(
                    "https://github.com/{}/pull/{}",
                    post.repo, post.number
                )),
                context: None,
                plan: None,
                job: None,
                session: None,
                record: None,
                event: None,
                opened_at: None,
                head_oid: workflow.head_oid.clone(),
                reviewed_oid: followup.map(|(_, reviewed)| reviewed.to_owned()),
            }
        })
        .collect())
}

/// The subjects of the `review-requested` items, open or closed, that a dismissal or a claim settled for good (C1): closed items
/// are kept, so the history is the memory.
fn settled_review_subjects(store: &mut Store) -> Result<Settled, CpdError> {
    store.read(|c| {
        let mut stmt = c.prepare(
            "SELECT subject, head_oid FROM inbox_items WHERE kind = 'review-requested' AND (dismissed_at IS NOT NULL OR claimed_at IS NOT NULL)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut settled = Settled::default();
        for row in rows {
            let (subject, head) = row?;
            if let Some(head) = head {
                settled.heads.insert((subject.clone(), head));
            }
            settled.subjects.insert(subject);
        }
        Ok(settled)
    })
}

/// What settled `review-requested` subjects: an initial item is suppressed by any of `subjects`, a follow-up item
/// only by a settled item of its subject at the current head (`heads`).
#[derive(Default)]
struct Settled {
    subjects: std::collections::HashSet<String>,
    heads: std::collections::HashSet<(String, String)>,
}

/// The open `review-requested` items as conditions (nothing changes), so a transient failure does not close them.
fn kept_review_items(store: &mut Store) -> Result<Vec<Condition>, CpdError> {
    Ok(store
        .read(open_items)?
        .iter()
        .filter(|i| i.kind == Kind::ReviewRequested)
        .map(Condition::from)
        .collect())
}

/// `models-changed`: each `swr_models` row whose `changes_json.at` is within [`models::WINDOW`] and whose announced part is
/// not empty. A provider with no row keeps its open items as they are.
fn models_changed(store: &mut Store, now: OffsetDateTime) -> Result<Vec<Condition>, CpdError> {
    let known = source_models::providers(store)?;
    let mut out: Vec<Condition> = store
        .read(open_items)?
        .iter()
        .filter(|i| i.kind == Kind::ModelsChanged)
        .filter(|i| {
            models::parse_subject(&i.subject)
                .is_some_and(|(provider, _)| !known.iter().any(|k| k == provider))
        })
        .map(Condition::from)
        .collect();
    for (provider, changes) in source_models::changed(store, now)? {
        let (new, gone) = models::announced(&provider, &changes);
        out.push(Condition {
            kind: Kind::ModelsChanged,
            urgency: Urgency::White,
            subject: models::subject(&provider, &changes.at),
            label: models::label(&provider, new.len(), gone.len()),
            url: None,
            context: None,
            plan: None,
            job: None,
            session: None,
            record: None,
            event: None,
            opened_at: None,
            head_oid: None,
            reviewed_oid: None,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use serde_json::{Value, json};

    use super::*;
    use crate::inbox::model::reconcile;
    use crate::inbox::team::{GhState, Lookup, Review};

    /// [`super::conditions_with`] with the team review rule off: these tests need no config file.
    fn conditions(store: &mut Store, home: &CpdHome) -> Result<Vec<Condition>, CpdError> {
        conditions_with(store, home, NOW, 20, 1, None)
    }

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
        let mut f = Fixture {
            _dir: dir,
            home,
            store,
        };
        let locked = if matches!(phase, "grounding" | "plan") {
            "NULL".to_owned()
        } else {
            format!("'{T}'")
        };
        let done = if phase == "done" {
            format!("'{T}'")
        } else {
            "NULL".to_owned()
        };
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
        fn event(
            &mut self,
            kind: &str,
            session: Option<&str>,
            job: Option<&str>,
            at: &str,
            data: &str,
        ) -> i64 {
            let session = session.map_or("NULL".to_owned(), |s| format!("'{s}'"));
            let (plan, job) = job.map_or(("NULL".to_owned(), "NULL".to_owned()), |j| {
                ("'pay'".to_owned(), format!("'{j}'"))
            });
            self.sql(&format!(
                "INSERT INTO events (txn, at, kind, context, plan, job, session, text, data)
                 VALUES (cpd_txn(), '{at}', '{kind}', 'ctx', {plan}, {job}, {session}, 'x', '{data}');"
            ));
            self.store
                .read(|c| Ok(c.query_row("SELECT max(id) FROM events", [], |r| r.get(0))?))
                .unwrap()
        }

        /// The Session's state changes: the row and its `session-state` Event (`from` is the state it was in).
        fn state(&mut self, session: &str, state: &str, label: Option<&str>) -> i64 {
            self.state_as(session, state, label, false)
        }

        /// A turn that ended in `idle` as a reply to the User (`--reply`).
        fn reply(&mut self, session: &str) -> i64 {
            self.state_as(session, "idle", None, true)
        }

        fn state_as(
            &mut self,
            session: &str,
            state: &str,
            label: Option<&str>,
            reply: bool,
        ) -> i64 {
            let from: String = self
                .store
                .read(|c| {
                    Ok(
                        c.query_row("SELECT state FROM sessions WHERE id = ?1", [session], |r| {
                            r.get(0)
                        })?,
                    )
                })
                .unwrap();
            let label = label.map_or("NULL".to_owned(), |l| format!("'{l}'"));
            self.sql(&format!("UPDATE sessions SET state = '{state}', state_label = {label} WHERE id = '{session}'"));
            let reply = if reply { r#","reply":true"# } else { "" };
            self.event(
                "session-state",
                Some(session),
                None,
                T,
                &format!(r#"{{"from":"{from}","label":null,"to":"{state}"{reply}}}"#),
            )
        }

        fn moved(&mut self, session: &str, place: &str) -> i64 {
            self.event(
                "session-moved",
                Some(session),
                None,
                T,
                &format!(r#"{{"from":"none","place":"{place}"}}"#),
            )
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
            // A Lead made here is a fork of the Plan's Lead `l-pay`.
            let fork = if role == "lead" { "'l-pay'" } else { "NULL" };
            self.sql(&format!(
                "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, forked_from)
                 VALUES ('{id}', '{role}', 'ctx', {plan}, {attempt}, 'n', '/', 'O3', 'working', '{T}', '{T}', {fork});"
            ));
        }

        fn job(&mut self, key: &str, status: &str) {
            let cause = if status == "failed" {
                "'crashed'"
            } else {
                "NULL"
            };
            self.sql(&format!(
                "INSERT OR IGNORE INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pay', 'p1', '/r', 'main', 'dispatch', 'cpd/pay/p1', '/lw', 1, '{T}', '{T}');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                                   fingerprint, created_txn, created_at, failed_cause)
                   VALUES ('pay', 'p1', '{key}', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, '{FP}', 1, '{T}', {cause});"
            ));
        }

        fn review(&mut self, id: &str, status: &str) {
            let number = &id[1..];
            let decided = if status == "pending" {
                "NULL".to_owned()
            } else {
                format!("'{T}'")
            };
            self.sql(&format!(
                "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at)
                 VALUES ('pay', '{id}', {number}, '{FP}', '{status}', '{T}', {decided});"
            ));
        }

        fn of(&mut self, kind: Kind) -> Vec<Condition> {
            let all = conditions(&mut self.store, &self.home).unwrap();
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
            assert_eq!(
                (
                    c.context.as_ref().map(|c| c.as_str()),
                    c.plan.as_ref().map(|p| p.as_str())
                ),
                (Some("ctx"), Some("pay"))
            );
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
        assert_eq!(
            (by("session:mgr").label.as_str(), by("session:mgr").urgency),
            ("ctx asks you", Urgency::Magenta)
        );
        assert_eq!(
            (
                by("session:fork-1234").label.as_str(),
                by("session:fork-1234").urgency
            ),
            ("pay·fork asks you", Urgency::Red)
        );
        assert_eq!(
            (
                by("session:loose-fork-id").label.as_str(),
                by("session:loose-fork-id").urgency
            ),
            ("loose-fo asks you", Urgency::Magenta)
        );
        assert_eq!(found.len(), 3);
    }

    #[test]
    fn a_lead_waiting_for_a_worker_and_a_plan_review_label_are_not_dialogs() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "waiting", Some("φ: Worker question"));
        f.state("mgr", "blocked", Some("φ: plan review"));
        assert!(f.of(Kind::Dialog).is_empty());
        // `blocked` is the human's whatever its label: the Worker question is `waiting` now (hs-d303).
        f.state("l-pay", "blocked", Some("φ: Worker question"));
        assert_eq!(f.of(Kind::Dialog).len(), 1);
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
        f.state("wk", "waiting", Some("φ: question to the Lead"));
        assert!(conditions(&mut f.store, &f.home).unwrap().is_empty());
    }

    // ---- unread ----

    #[test]
    fn an_idle_turn_outside_the_chat_slot_is_unread_until_the_chat_is_seen() {
        let mut f = fixture("dispatch");
        let idle = f.reply("l-pay");
        let c = one(f.of(Kind::Unread));
        assert_eq!(
            (c.urgency, c.subject.as_str(), c.label.as_str()),
            (Urgency::Cyan, "session:l-pay", "pay replied\nPay")
        );
        assert_eq!(c.event, Some(idle), "the idle Event");
        assert_eq!(c.plan.as_ref().map(|p| p.as_str()), Some("pay"));

        f.moved("l-pay", "parked");
        assert_eq!(f.of(Kind::Unread).len(), 1, "parked is not seen");
        f.moved("l-pay", "chat");
        assert!(f.of(Kind::Unread).is_empty(), "in the slot: seen");
        f.moved("l-pay", "parked");
        assert!(
            f.of(Kind::Unread).is_empty(),
            "out of the slot again, but the turn is not newer than the sight"
        );
        f.state("l-pay", "working", None);
        let idle = f.reply("l-pay");
        assert_eq!(
            one(f.of(Kind::Unread)).event,
            Some(idle),
            "a new idle turn after being seen"
        );
    }

    #[test]
    fn a_turn_that_did_not_answer_the_user_is_not_unread() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "idle", None);
        assert!(f.of(Kind::Unread).is_empty(), "a non-reply turn alone");
        f.state("l-pay", "working", None);
        f.state("l-pay", "idle", None);
        assert!(
            f.of(Kind::Unread).is_empty(),
            "working -> idle without a reply"
        );
    }

    #[test]
    fn a_later_non_reply_turn_does_not_hide_an_unread_reply() {
        let mut f = fixture("dispatch");
        let reply = f.reply("l-pay");
        f.state("l-pay", "working", None);
        f.state("l-pay", "idle", None);
        let c = one(f.of(Kind::Unread));
        assert_eq!(c.event, Some(reply), "the reply Event, not the later turn");
        f.moved("l-pay", "chat");
        f.moved("l-pay", "parked");
        f.state("l-pay", "working", None);
        f.state("l-pay", "idle", None);
        assert!(
            f.of(Kind::Unread).is_empty(),
            "seen, and the later turn was no reply"
        );
    }

    #[test]
    fn a_reload_is_not_a_finished_turn() {
        let mut f = fixture("dispatch");
        f.reply("l-pay");
        f.moved("l-pay", "chat");
        f.moved("l-pay", "parked");
        assert!(f.of(Kind::Unread).is_empty(), "seen");
        f.state("l-pay", "stopped", None);
        f.state("l-pay", "idle", None);
        assert!(
            f.of(Kind::Unread).is_empty(),
            "idle -> stopped -> idle is a reload, not a turn"
        );
        f.state("l-pay", "stopped", None);
        f.reply("l-pay");
        assert!(
            f.of(Kind::Unread).is_empty(),
            "stopped -> idle is a reload even when it says reply"
        );
    }

    #[test]
    fn a_dismissed_reply_turn_never_reopens_as_unread_across_a_reload_or_a_non_reply_turn() {
        const AT: &str = "2026-01-01T01:00:00.000Z";
        fn run(f: &mut Fixture) {
            let found = conditions(&mut f.store, &f.home).unwrap();
            reconcile(&mut f.store, &found, AT).unwrap();
        }
        fn rows(f: &mut Fixture, filter: &str) -> i64 {
            f.store
                .read(|c| {
                    Ok(c.query_row(
                        &format!("SELECT COUNT(*) FROM inbox_items WHERE kind = 'unread' {filter}"),
                        [],
                        |r| r.get(0),
                    )?)
                })
                .unwrap()
        }
        let mut f = fixture("dispatch");
        let first = f.reply("l-pay");
        run(&mut f);
        assert_eq!(rows(&mut f, "AND closed_at IS NULL"), 1, "one open unread");
        // The WebUI read flow: `cpd inbox dismiss`, no session-moved to chat.
        f.store
            .write("dismiss", |tx| {
                tx.execute(
                    "UPDATE inbox_items SET dismissed_at = ?1 WHERE kind = 'unread'",
                    [AT],
                )?;
                Ok(())
            })
            .unwrap();
        run(&mut f);
        assert_eq!(
            rows(&mut f, "AND closed_at IS NULL AND dismissed_at IS NOT NULL"),
            1,
            "the row stays open and dismissed"
        );
        for (mid, why) in [("stopped", "a reload"), ("working", "a non-reply turn")] {
            f.state("l-pay", mid, None);
            run(&mut f);
            f.state("l-pay", "idle", None);
            run(&mut f);
            assert_eq!(
                rows(&mut f, "AND closed_at IS NULL"),
                0,
                "{why} opens no new unread"
            );
            assert!(
                f.of(Kind::Unread).is_empty(),
                "{why} leaves no unread condition"
            );
            assert_eq!(
                rows(&mut f, ""),
                1,
                "{why}: still the one closed, dismissed row"
            );
        }
        f.state("l-pay", "working", None);
        let second = f.reply("l-pay");
        run(&mut f);
        assert_ne!(first, second);
        let open = f
            .store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT event, dismissed_at IS NULL FROM inbox_items \
                     WHERE kind = 'unread' AND closed_at IS NULL",
                    [],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(
            open,
            (second, true),
            "a new reply turn opens a fresh unread"
        );
    }

    #[test]
    fn a_working_waiting_or_blocked_session_going_idle_after_the_visit_is_unread() {
        for before in ["working", "waiting", "blocked"] {
            let mut f = fixture("dispatch");
            f.reply("l-pay");
            f.moved("l-pay", "chat");
            f.moved("l-pay", "parked");
            f.state("l-pay", before, (before != "working").then_some("Which?"));
            let idle = f.reply("l-pay");
            assert_eq!(
                one(f.of(Kind::Unread)).event,
                Some(idle),
                "{before} -> idle"
            );
        }
    }

    #[test]
    fn a_session_in_the_chat_slot_or_not_idle_is_not_unread() {
        let mut f = fixture("dispatch");
        f.reply("l-pay");
        f.moved("l-pay", "chat");
        assert!(f.of(Kind::Unread).is_empty());
        f.state("mgr", "working", None);
        f.state("mgr", "blocked", Some("Which?"));
        assert!(
            f.of(Kind::Unread).is_empty(),
            "blocked is a dialog, not unread"
        );
        f.state("mgr", "waiting", Some("φ: Worker question"));
        assert!(f.of(Kind::Unread).is_empty(), "waiting is not unread");
        f.state("mgr", "stopped", None);
        assert!(f.of(Kind::Unread).is_empty());
    }

    #[test]
    fn a_manager_and_a_fork_can_be_unread() {
        let mut f = fixture("dispatch");
        f.session("fork-1234", "lead", Some("pay"));
        f.reply("mgr");
        f.reply("fork-1234");
        let mut labels: Vec<_> = f.of(Kind::Unread).into_iter().map(|c| c.label).collect();
        labels.sort();
        assert_eq!(labels, ["ctx replied", "pay·fork replied\nPay"]);
    }

    // ---- plan-review ----

    #[test]
    fn a_pending_review_is_a_plan_review() {
        let mut f = fixture("plan");
        f.review("r1", "pending");
        f.sql("UPDATE plan_reviews SET title = 'Add retries' WHERE id = 'r1'");
        let c = one(f.of(Kind::PlanReview));
        assert_eq!(
            (c.urgency, c.subject.as_str(), c.label.as_str()),
            (
                Urgency::Magenta,
                "review:pay/r1",
                "pay plan review\nAdd retries"
            )
        );
        assert_eq!(c.url, None, "a plan-review has no url (hs-d739)");
        assert_eq!(
            c.session.as_ref().map(|s| s.as_str()),
            Some("l-pay"),
            "its Lead"
        );
        assert_eq!(
            (
                c.context.as_ref().map(|c| c.as_str()),
                c.plan.as_ref().map(|p| p.as_str())
            ),
            (Some("ctx"), Some("pay"))
        );
    }

    #[test]
    fn a_plan_review_has_no_url() {
        let mut f = fixture("plan");
        f.review("r1", "pending");
        assert_eq!(one(f.of(Kind::PlanReview)).url, None, "no page");
    }

    #[test]
    fn a_review_without_a_title_keeps_the_plain_label() {
        let mut f = fixture("plan");
        f.review("r1", "pending");
        assert_eq!(one(f.of(Kind::PlanReview)).label, "pay plan review");
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

    // ---- stuck Jobs (hs-d318: a message to the Manager, no Inbox condition) ----

    impl Fixture {
        fn stuck(&mut self) -> Vec<StuckJob> {
            stuck_jobs(&mut self.store, NOW, TEN).unwrap()
        }
    }

    #[test]
    fn a_job_failed_for_the_stuck_time_is_stuck_and_clears_on_retry() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "idle", None);
        f.job("a", "failed");
        f.event(
            "job-failed",
            None,
            Some("a"),
            "2026-01-01T00:00:00.000Z",
            r#"{}"#,
        );
        let latest = f.event(
            "job-failed",
            None,
            Some("a"),
            "2026-01-01T23:50:00.000Z",
            r#"{}"#,
        );
        let stuck = f.stuck();
        assert_eq!(
            stuck,
            [StuckJob {
                plan: PlanId::parse("pay").unwrap(),
                home: JobHome::Workflow(WorkflowKey::parse("p1").unwrap()),
                job: JobKey::parse("a").unwrap(),
                context: ContextId::parse("ctx").unwrap(),
                event: latest,
                minutes: 10,
                cause: "crashed".to_owned(),
            }],
            "the latest job-failed Event, 10 whole minutes ago, with the Job's cause"
        );

        f.sql("UPDATE jobs SET status = 'queued', failed_cause = NULL WHERE key = 'a'");
        assert!(f.stuck().is_empty(), "retried");
    }

    #[test]
    fn stuck_jobs_of_one_pr_group_with_the_newest_event_and_oldest_first() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "idle", None);
        for (key, status) in [("a", "failed"), ("b", "failed")] {
            f.job(key, status);
            f.event(
                "job-failed",
                None,
                Some(key),
                "2026-01-01T00:00:00.000Z",
                r#"{}"#,
            );
        }
        let a = f.event(
            "job-failed",
            None,
            Some("a"),
            "2026-01-01T23:00:00.000Z",
            r#"{}"#,
        );
        let b = f.event(
            "job-failed",
            None,
            Some("b"),
            "2026-01-01T23:50:00.000Z",
            r#"{}"#,
        );
        let groups = stuck_groups(f.stuck());
        assert_eq!(groups.len(), 1, "one Workflow, one group");
        let group = &groups[0];
        assert_eq!(
            (&group.plan, &group.home, group.event),
            (
                &PlanId::parse("pay").unwrap(),
                &JobHome::Workflow(WorkflowKey::parse("p1").unwrap()),
                b
            )
        );
        assert_eq!(
            group
                .jobs
                .iter()
                .map(|j| j.job.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"],
            "oldest Event first"
        );
        assert_eq!(group.jobs[0].event, a);
    }

    #[test]
    fn stuck_jobs_of_different_prs_are_separate_groups_in_first_event_order() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "idle", None);
        f.job("a", "failed");
        f.sql(&format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', 'p2', '/r', 'main', 'dispatch', 'cpd/pay/p2', '/lw', 1, '{T}', '{T}');
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                               fingerprint, created_txn, created_at, failed_cause)
               VALUES ('pay', 'p2', 'b', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'failed', 0, '{FP}', 1, '{T}', 'crashed');"
        ));
        f.event("job-failed", None, Some("a"), T, r#"{}"#);
        f.event("job-failed", None, Some("b"), T, r#"{}"#);
        f.event(
            "job-failed",
            None,
            Some("b"),
            "2026-01-01T22:00:00.000Z",
            r#"{}"#,
        );
        f.event(
            "job-failed",
            None,
            Some("a"),
            "2026-01-01T23:50:00.000Z",
            r#"{}"#,
        );
        let groups = stuck_groups(f.stuck());
        assert_eq!(
            groups.iter().map(|g| g.home.clone()).collect::<Vec<_>>(),
            [
                JobHome::Workflow(WorkflowKey::parse("p2").unwrap()),
                JobHome::Workflow(WorkflowKey::parse("p1").unwrap())
            ],
            "the group of the oldest first Event leads"
        );
    }

    #[test]
    fn a_stuck_job_opens_no_inbox_condition() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "idle", None);
        f.job("a", "failed");
        f.event(
            "job-failed",
            None,
            Some("a"),
            "2026-01-01T00:00:00.000Z",
            r#"{}"#,
        );
        assert_eq!(f.stuck().len(), 1);
        assert!(f.of(Kind::JobStuck).is_empty());
        assert!(conditions(&mut f.store, &f.home).unwrap().is_empty());
    }

    #[test]
    fn a_job_failed_less_than_the_stuck_time_ago_is_not_yet_stuck() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "idle", None);
        f.job("a", "failed");
        f.event(
            "job-failed",
            None,
            Some("a"),
            "2026-01-01T23:50:01.000Z",
            r#"{}"#,
        );
        assert!(f.stuck().is_empty(), "9 min 59 s");
        f.event(
            "job-failed",
            None,
            Some("a"),
            "2026-01-01T23:50:00.000Z",
            r#"{}"#,
        );
        assert_eq!(f.stuck().len(), 1, "exactly the stuck time counts");
        f.job("b", "queued");
        f.event("job-failed", None, Some("b"), T, r#"{}"#);
        assert_eq!(
            f.stuck().len(),
            1,
            "a Job that is not failed is never stuck"
        );
    }

    #[test]
    fn a_failed_job_is_not_stuck_while_its_lead_works_and_is_at_the_next_tick_after_it_stops() {
        let mut f = fixture("dispatch");
        f.job("a", "failed");
        f.event(
            "job-failed",
            None,
            Some("a"),
            "2026-01-01T00:00:00.000Z",
            r#"{}"#,
        );
        assert!(
            f.stuck().is_empty(),
            "the Lead is working: it handles the failure (audit Q1)"
        );
        f.state("l-pay", "idle", None);
        assert_eq!(f.stuck()[0].job.as_str(), "a", "the Lead stopped working");
        f.state("l-pay", "working", None);
        assert!(f.stuck().is_empty(), "working again");
        for state in ["waiting", "blocked", "stopped"] {
            f.state("l-pay", state, (state != "stopped").then_some("Which?"));
            assert_eq!(f.stuck().len(), 1, "a {state} Lead is not working");
        }
    }

    // ---- pull requests ----

    /// An open, authored, non-draft pull request `acme/shop#n` with the given checks, mergeable and review values.
    fn pull_request(f: &mut Fixture, n: i64, checks: &str, mergeable: &str, review: &str) {
        // GitHub's merge state follows mergeable (hs-d481): a conflict is dirty, an unknown mergeable unknown.
        let merge_state = match (mergeable, checks) {
            ("conflicting", _) => "dirty",
            ("unknown", _) => "unknown",
            _ => "clean",
        };
        f.sql(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                        mergeable, authored, review_requested, merge_state, base_ref, default_branch)
             VALUES ('acme/shop', {n}, 'https://github.com/acme/shop/pull/{n}', 'T', 0, 'open', '{review}', '{checks}', '{T}', '{T}',
                     '{mergeable}', 1, 0, '{merge_state}', 'main', 'main');"
        ));
    }

    fn set(f: &mut Fixture, n: i64, assignments: &str) {
        f.sql(&format!(
            "UPDATE pull_requests SET {assignments} WHERE number = {n}"
        ));
    }

    fn pr_kinds(f: &mut Fixture) -> Vec<Kind> {
        let all = conditions(&mut f.store, &f.home).unwrap();
        all.into_iter().map(|c| c.kind).collect()
    }

    #[test]
    fn failed_checks_open_no_pr_failing_condition_hs_d470() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 7, "failed", "mergeable", "none");
        set(&mut f, 7, "merge_state = 'blocked'"); // a required check failed (hs-d481)
        assert!(f.of(Kind::PrFailing).is_empty());
        assert_eq!(
            pr_kinds(&mut f),
            [Kind::PrReady],
            "one pr-ready row, named failing"
        );
    }

    #[test]
    fn a_conflicting_pr_opens_no_pr_failing_condition_and_is_never_ready() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 8, "passed", "conflicting", "approved");
        assert!(f.of(Kind::PrFailing).is_empty());
        assert_eq!(one(f.of(Kind::PrReady)).label, "shop#8 conflict\nT");
        set(&mut f, 8, "checks = 'failed'");
        assert!(f.of(Kind::PrFailing).is_empty());
        set(
            &mut f,
            8,
            "checks = 'passed', mergeable = 'mergeable', merge_state = 'clean'",
        );
        assert!(f.of(Kind::PrFailing).is_empty());
        assert_eq!(one(f.of(Kind::PrReady)).label, "shop#8 ready to merge\nT");
    }

    #[test]
    fn an_unknown_mergeable_is_neither_failing_nor_ready() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 9, "passed", "unknown", "approved");
        assert_eq!(one(f.of(Kind::PrReady)).label, "shop#9 waiting\nT");
    }

    #[test]
    fn a_pr_with_changes_requested_opens_no_pr_changes_condition() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 3, "pending", "mergeable", "changes_requested");
        assert!(f.of(Kind::PrChanges).is_empty());
        assert_eq!(
            one(f.of(Kind::PrReady)).label,
            "shop#3 changes requested\nT"
        );
    }

    #[test]
    fn a_passing_mergeable_pr_is_ready_until_it_is_a_draft_or_changes_are_requested() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 4, "passed", "mergeable", "approved");
        let c = one(f.of(Kind::PrReady));
        assert_eq!(
            (c.urgency, c.subject.as_str(), c.label.as_str()),
            (
                Urgency::Yellow,
                "pr:acme/shop#4",
                "shop#4 ready to merge\nT"
            )
        );
        assert_eq!(
            c.url.as_deref(),
            Some("https://github.com/acme/shop/pull/4")
        );
        set(&mut f, 4, "review_decision = 'none'");
        assert_eq!(
            one(f.of(Kind::PrReady)).label,
            "shop#4 ready to merge\nT",
            "no review is no objection"
        );
        set(&mut f, 4, "draft = 1");
        assert_eq!(
            one(f.of(Kind::PrReady)).label,
            "shop#4 draft\nT",
            "a draft is never ready"
        );
        set(
            &mut f,
            4,
            "draft = 0, review_decision = 'changes_requested'",
        );
        assert_eq!(
            one(f.of(Kind::PrReady)).label,
            "shop#4 changes requested\nT"
        );
        set(
            &mut f,
            4,
            "review_decision = 'approved', checks = 'pending'",
        );
        assert_eq!(one(f.of(Kind::PrReady)).label, "shop#4 checks running\nT");
    }

    /// The exact row of missionlane/cc-api-orc-apollo#2385 (hs-d481): everything looks ready but GitHub says BLOCKED.
    #[test]
    fn the_state_of_cc_api_orc_apollo_2385_is_not_ready() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 5, "passed", "mergeable", "none");
        set(&mut f, 5, "merge_state = 'blocked'");
        assert_eq!(
            one(f.of(Kind::PrReady)).label,
            "shop#5 waiting\nT",
            "unowned"
        );
        own(&mut f, 5);
        assert_eq!(
            one(f.of(Kind::PrReady)).label,
            "shop#5 updating\nT",
            "owned, its Job live"
        );
    }

    #[test]
    fn ready_to_merge_is_decided_by_merge_state_and_review_decision_hs_d481() {
        let ready = |merge_state: &str, review: &str| {
            let mut f = fixture("dispatch");
            pull_request(&mut f, 1, "passed", "mergeable", review);
            set(&mut f, 1, &format!("merge_state = '{merge_state}'"));
            one(f.of(Kind::PrReady)).label == "shop#1 ready to merge\nT"
        };
        // hs-d525: only clean is ready; unstable and has_hooks are not.
        for merge_state in ["clean"] {
            for review in ["approved", "none"] {
                assert!(
                    ready(merge_state, review),
                    "{merge_state} {review} is ready"
                );
            }
            assert!(
                !ready(merge_state, "changes_requested"),
                "{merge_state} with changes requested is not ready"
            );
        }
        for merge_state in [
            "blocked",
            "behind",
            "dirty",
            "unknown",
            "draft",
            "unstable",
            "has_hooks",
        ] {
            for review in ["approved", "none", "changes_requested"] {
                assert!(
                    !ready(merge_state, review),
                    "{merge_state} {review} is not ready"
                );
            }
        }
    }

    #[test]
    fn an_open_pr_ready_item_closes_when_its_pull_request_becomes_blocked_hs_d481() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 5, "passed", "mergeable", "none");
        let found = conditions(&mut f.store, &f.home).unwrap();
        assert_eq!(reconcile(&mut f.store, &found, T).unwrap().opened.len(), 1);
        set(&mut f, 5, "merge_state = 'blocked'");
        let found = conditions(&mut f.store, &f.home).unwrap();
        let changes = reconcile(&mut f.store, &found, T).unwrap();
        assert_eq!(
            (changes.opened.len(), changes.closed.len()),
            (1, 1),
            "a colour change closes the yellow row and opens a cyan one"
        );
    }

    #[test]
    fn only_authored_pull_requests_are_ready_and_a_github_request_opens_nothing() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 1, "failed", "conflicting", "changes_requested");
        pull_request(&mut f, 2, "passed", "mergeable", "approved");
        assert_eq!(
            pr_kinds(&mut f),
            [Kind::PrReady, Kind::PrReady],
            "every authored open pull request has a row"
        );
        f.sql("UPDATE pull_requests SET authored = 0, review_requested = 1");
        assert!(
            pr_kinds(&mut f).is_empty(),
            "review_requested alone is no item (hs-d320)"
        );
    }

    #[test]
    fn a_pull_request_that_is_not_open_is_no_item() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 1, "failed", "conflicting", "changes_requested");
        pull_request(&mut f, 2, "passed", "mergeable", "approved");
        assert_eq!(pr_kinds(&mut f), [Kind::PrReady, Kind::PrReady]);
        f.sql("UPDATE pull_requests SET state = 'closed'");
        assert!(pr_kinds(&mut f).is_empty());
    }

    /// Makes the Plan's Workflow `p1` own the pull request numbered `n`. Its Context's owner keys the Workflow's policy (an unset
    /// one is `owner_unset`, not these tests' subject), with an empty owner policy file.
    fn own(f: &mut Fixture, n: i64) {
        f.job("a", "queued");
        f.sql("UPDATE contexts SET owner = 'acme'");
        let owners = f.home.root().join("policy/acme");
        std::fs::create_dir_all(&owners).unwrap();
        std::fs::write(owners.join("owner.toml"), "").unwrap();
        f.sql(&format!(
            "UPDATE workflows SET status = 'open', pull_request = (SELECT id FROM pull_requests WHERE number = {n}),
                            pr_number = {n}, pr_url = 'https://github.com/acme/shop/pull/{n}', pr_opened_at = 't'"
        ));
    }

    #[test]
    fn a_pull_request_owned_by_a_plan_links_the_plan_its_context_and_its_lead() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 6, "passed", "mergeable", "approved");
        pull_request(&mut f, 7, "passed", "mergeable", "approved");
        own(&mut f, 6);
        {
            let kind = Kind::PrReady;
            let c = one(f
                .of(kind)
                .into_iter()
                .filter(|c| c.subject == "pr:acme/shop#6")
                .collect());
            assert_eq!(
                (
                    c.context.as_ref().map(|c| c.as_str()),
                    c.plan.as_ref().map(|p| p.as_str()),
                    c.session.as_ref().map(|s| s.as_str())
                ),
                (Some("ctx"), Some("pay"), Some("l-pay")),
                "{kind:?}"
            );
        }
        let c = one(f
            .of(Kind::PrReady)
            .into_iter()
            .filter(|c| c.subject == "pr:acme/shop#7")
            .collect());
        assert_eq!(
            (c.plan, c.session),
            (None, None),
            "a pull request no Workflow owns"
        );
    }

    #[test]
    fn a_pull_request_a_plan_owns_opens_no_pr_failing_or_pr_changes_item() {
        for (checks, mergeable, review) in [
            ("failed", "mergeable", "none"),
            ("passed", "conflicting", "approved"),
            ("pending", "mergeable", "changes_requested"),
            ("failed", "conflicting", "changes_requested"),
        ] {
            let mut f = fixture("dispatch");
            pull_request(&mut f, 6, checks, mergeable, review);
            if checks == "failed" {
                set(&mut f, 6, "merge_state = 'blocked'"); // a required check failed (hs-d481)
            }
            own(&mut f, 6);
            assert_eq!(
                pr_kinds(&mut f),
                [Kind::PrReady],
                "{checks} {mergeable} {review}"
            );
        }
    }

    #[test]
    fn the_same_states_on_an_unowned_authored_pull_request_open_no_item_either() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 1, "failed", "mergeable", "none");
        pull_request(&mut f, 2, "passed", "conflicting", "approved");
        pull_request(&mut f, 3, "pending", "mergeable", "changes_requested");
        pull_request(&mut f, 6, "failed", "conflicting", "changes_requested");
        own(&mut f, 6);
        let seen: Vec<_> = f
            .of(Kind::PrFailing)
            .into_iter()
            .chain(f.of(Kind::PrChanges))
            .map(|c| c.subject)
            .collect();
        assert!(seen.is_empty(), "{seen:?}");
    }

    /// hs-d470: `pr-failing` and `pr-changes` are dropped, so items an older engine opened close at the next reconcile.
    #[test]
    fn open_pr_failing_and_pr_changes_items_an_older_engine_opened_are_closed_by_the_next_reconcile()
     {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 1, "failed", "mergeable", "none");
        pull_request(&mut f, 2, "passed", "mergeable", "changes_requested");
        for (kind, n) in [("pr-failing", 1), ("pr-changes", 2)] {
            f.sql(&format!(
                "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at)
                 VALUES ('{kind}', 'red', 'pr:acme/shop#{n}', 'shop#{n} {kind}', '{T}')"
            ));
        }
        let found = conditions(&mut f.store, &f.home).unwrap();
        let changes = crate::inbox::model::reconcile(&mut f.store, &found, T).unwrap();
        assert_eq!(changes.closed.len(), 2);
        let open: i64 = f.store.read(|c| Ok(c.query_row("SELECT count(*) FROM inbox_items WHERE closed_at IS NULL AND kind IN ('pr-failing','pr-changes')", [], |r| r.get(0))?)).unwrap();
        assert_eq!(open, 0);
    }

    #[test]
    fn an_owned_pull_request_that_is_ready_still_opens_pr_ready() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 6, "passed", "mergeable", "approved");
        own(&mut f, 6);
        assert_eq!(pr_kinds(&mut f), [Kind::PrReady]);
    }

    // ---- review-requested: the team review rule (hs-d320) ----

    const BOTS: [&str; 1] = ["github-actions"];
    const ME_LOGIN: &str = "clayton-duarte";

    /// `ts` of a post an hour before `NOW`.
    fn recent() -> f64 {
        NOW.unix_timestamp() as f64 - 3600.0
    }

    /// A primary post with no reaction, no claim and no tag, as the helper prints it (commands.md §21.6).
    fn post(repo: &str, number: u64) -> Value {
        json!({"repo": repo, "number": number, "ts": recent(), "ts_raw": "1.1", "author": "Ryan Case", "primary": true,
               "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false})
    }

    fn claimed(mut post: Value, eyes_me: Value) -> Value {
        post["reactions"] = json!(["eyes"]);
        post["claim"] = json!({"kind": "claimed", "reason": "claimed with :eyes: in Slack"});
        post["eyes_me"] = eyes_me;
        post
    }

    fn tagged(mut post: Value) -> Value {
        post["thread_tags_me"] = json!(true);
        post
    }

    /// Stores `posts` (the helper's older shape) as the `slack` source's rows, the query read.
    fn write_cache(f: &mut Fixture, posts: &[Value]) {
        team::seed_cache(&mut f.store, "C02JU6C2Z1D", posts);
    }

    /// The channel was never read: no query data and no row, so the posts are "unknown".
    fn clear_cache(f: &mut Fixture) {
        f.sql("DELETE FROM swr_slack_posts; DELETE FROM swr_queries WHERE key LIKE 'slack:channel:%';");
    }

    fn open_pr() -> GhPr {
        GhPr {
            title: "Add retries".to_owned(),
            draft: false,
            state: GhState::Open,
            author: "ryan".to_owned(),
            decision: None,
            reviews: Vec::new(),
            head_oid: None,
            checks_state: None,
        }
    }

    fn reviewed_by(login: &str) -> GhPr {
        GhPr {
            reviews: vec![Review {
                author: login.to_owned(),
                state: "COMMENTED".to_owned(),
                commit_oid: None,
            }],
            ..open_pr()
        }
    }

    fn lookup(workflows: &[(&str, u64, GhPr)]) -> Lookup {
        Lookup {
            me: ME_LOGIN.to_owned(),
            workflows: workflows
                .iter()
                .map(|(repo, n, workflow)| (((*repo).to_owned(), *n), Some(workflow.clone())))
                .collect(),
        }
    }

    fn team() -> Team<'static> {
        static BOT_LIST: std::sync::LazyLock<Vec<String>> =
            std::sync::LazyLock::new(|| BOTS.map(str::to_owned).to_vec());
        Team {
            bot_reviewers: &BOT_LIST,
        }
    }

    /// The `review-requested` conditions for the cache's `posts` and GitHub's `prs`.
    fn reviews(
        f: &mut Fixture,
        posts: &[Value],
        workflows: &[(&str, u64, GhPr)],
    ) -> Vec<Condition> {
        write_cache(f, posts);
        crate::inbox::swr_fixture::lookup(&mut f.store, &lookup(workflows)).unwrap();
        let all = conditions_with(&mut f.store, &f.home, NOW, 20, 1, Some(team())).unwrap();
        all.into_iter()
            .filter(|c| c.kind == Kind::ReviewRequested)
            .collect()
    }

    #[test]
    fn a_post_with_no_review_opens_an_item_with_the_posts_pull_request() {
        let mut f = fixture("dispatch");
        let c = one(reviews(
            &mut f,
            &[post("acme/pay", 812)],
            &[("acme/pay", 812, open_pr())],
        ));
        assert_eq!(
            (c.urgency, c.subject.as_str(), c.label.as_str()),
            (
                Urgency::Blue,
                "pr:acme/pay#812",
                "review pay#812\nAdd retries"
            )
        );
        assert_eq!(
            c.url.as_deref(),
            Some("https://github.com/acme/pay/pull/812")
        );
        assert_eq!(
            (c.context, c.plan, c.session, c.job, c.record, c.event),
            (None, None, None, None, None, None)
        );
    }

    /// One case per hermes rule: what GitHub says, what Slack says, and whether the item opens.
    #[test]
    fn each_hermes_rule_opens_the_item_or_not() {
        let changes = GhPr {
            decision: Some("CHANGES_REQUESTED".to_owned()),
            ..open_pr()
        };
        let approved = GhPr {
            decision: Some("APPROVED".to_owned()),
            ..open_pr()
        };
        let cases: Vec<(&str, Value, GhPr, bool)> = vec![
            ("no review yet", post("acme/pay", 1), open_pr(), true),
            (
                "AI review only",
                post("acme/pay", 1),
                reviewed_by("github-actions"),
                true,
            ),
            (
                "a human is reviewing",
                post("acme/pay", 1),
                reviewed_by("wes"),
                false,
            ),
            (
                "draft",
                post("acme/pay", 1),
                GhPr {
                    draft: true,
                    ..open_pr()
                },
                false,
            ),
            (
                "the User's own Workflow",
                post("acme/pay", 1),
                GhPr {
                    author: ME_LOGIN.to_owned(),
                    ..open_pr()
                },
                false,
            ),
            (
                "merged",
                post("acme/pay", 1),
                GhPr {
                    state: GhState::Merged,
                    ..open_pr()
                },
                false,
            ),
            (
                "closed",
                post("acme/pay", 1),
                GhPr {
                    state: GhState::Closed,
                    ..open_pr()
                },
                false,
            ),
            ("approved", post("acme/pay", 1), approved, false),
            ("changes requested", post("acme/pay", 1), changes, false),
            (
                "claimed by a peer's :eyes:",
                claimed(post("acme/pay", 1), json!(false)),
                open_pr(),
                false,
            ),
            (
                "claimed, attribution unavailable",
                claimed(post("acme/pay", 1), Value::Null),
                open_pr(),
                false,
            ),
            (
                "assigned to someone in Slack",
                {
                    let mut p = post("acme/pay", 1);
                    p["claim"] =
                        json!({"kind": "claimed", "reason": "assigned to someone in Slack"});
                    p
                },
                open_pr(),
                false,
            ),
            (
                "the User's own :eyes: alone",
                claimed(post("acme/pay", 1), json!(true)),
                open_pr(),
                true,
            ),
            (
                "tagged in the thread of a claimed post",
                tagged(claimed(post("acme/pay", 1), json!(false))),
                open_pr(),
                true,
            ),
            (
                "tagged in the thread of a reviewed Workflow",
                tagged(post("acme/pay", 1)),
                reviewed_by("wes"),
                true,
            ),
            (
                "tagged under a merged Workflow",
                tagged(post("acme/pay", 1)),
                GhPr {
                    state: GhState::Merged,
                    ..open_pr()
                },
                false,
            ),
            (
                "tagged under changes requested",
                tagged(post("acme/pay", 1)),
                GhPr {
                    decision: Some("CHANGES_REQUESTED".to_owned()),
                    ..open_pr()
                },
                false,
            ),
            (
                "tagged under a draft",
                tagged(post("acme/pay", 1)),
                GhPr {
                    draft: true,
                    ..open_pr()
                },
                false,
            ),
        ];
        let mut f = fixture("dispatch");
        for (rule, post, workflow, opens) in cases {
            let found = reviews(&mut f, &[post], &[("acme/pay", 1, workflow)]);
            assert_eq!(found.len(), usize::from(opens), "{rule}: {found:?}");
        }
    }

    #[test]
    fn a_github_review_request_alone_opens_nothing() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 5, "none", "unknown", "none");
        set(&mut f, 5, "authored = 0, review_requested = 1");
        assert!(
            reviews(
                &mut f,
                &[post("acme/pay", 1)],
                &[("acme/pay", 1, reviewed_by("wes"))]
            )
            .is_empty(),
            "posts of other Workflows"
        );
        assert!(f.of(Kind::ReviewRequested).is_empty(), "no rule at all");
        clear_cache(&mut f);
        let none = conditions_with(&mut f.store, &f.home, NOW, 20, 1, Some(team())).unwrap();
        assert!(none.is_empty(), "no cache: {none:?}");
    }

    #[test]
    fn only_primary_posts_within_seven_days_are_read_from_the_cache() {
        let mut f = fixture("dispatch");
        let day = 24.0 * 3600.0;
        let old = |n, age: f64| {
            let mut p = post("acme/pay", n);
            p["ts"] = json!(NOW.unix_timestamp() as f64 - age);
            p
        };
        let mut mention = post("acme/pay", 3);
        mention["primary"] = json!(false);
        write_cache(&mut f, &[old(1, 6.9 * day), old(2, 7.1 * day), mention]);
        let answer = lookup(&[("acme/pay", 1, open_pr())]);
        crate::inbox::swr_fixture::lookup(&mut f.store, &answer).unwrap();
        let all = conditions_with(&mut f.store, &f.home, NOW, 20, 1, Some(team())).unwrap();
        assert_eq!(
            all.iter()
                .filter(|c| c.kind == Kind::ReviewRequested)
                .map(|c| c.subject.as_str())
                .collect::<Vec<_>>(),
            ["pr:acme/pay#1"]
        );
    }

    #[test]
    fn with_the_rule_off_nothing_opens_and_open_items_close() {
        let mut f = fixture("dispatch");
        let opened = reviews(
            &mut f,
            &[post("acme/pay", 1)],
            &[("acme/pay", 1, open_pr())],
        );
        reconcile(&mut f.store, &opened, T).unwrap();
        let off = conditions_with(&mut f.store, &f.home, NOW, 20, 1, None).unwrap();
        assert!(off.is_empty());
        reconcile(&mut f.store, &off, T).unwrap();
        assert!(f.store.read(open_items).unwrap().is_empty());
    }

    /// The open items after reconciling `conditions` over them.
    fn open_subjects(f: &mut Fixture) -> Vec<String> {
        f.store
            .read(open_items)
            .unwrap()
            .into_iter()
            .map(|i| i.subject)
            .collect()
    }

    #[test]
    fn a_pull_request_that_leaves_the_posts_closes_its_item() {
        let mut f = fixture("dispatch");
        let two = [post("acme/pay", 1), post("acme/pay", 2)];
        let both = [("acme/pay", 1, open_pr()), ("acme/pay", 2, open_pr())];
        let opened = reviews(&mut f, &two, &both);
        reconcile(&mut f.store, &opened, T).unwrap();
        assert_eq!(open_subjects(&mut f), ["pr:acme/pay#1", "pr:acme/pay#2"]);

        clear_cache(&mut f);
        let unknown = conditions_with(&mut f.store, &f.home, NOW, 20, 1, Some(team())).unwrap();
        assert_eq!(
            unknown.len(),
            2,
            "no file is unknown, not empty: {unknown:?}"
        );
        reconcile(&mut f.store, &unknown, T).unwrap();
        assert_eq!(open_subjects(&mut f), ["pr:acme/pay#1", "pr:acme/pay#2"]);

        let rest = reviews(
            &mut f,
            &[post("acme/pay", 2)],
            &[("acme/pay", 2, open_pr())],
        );
        reconcile(&mut f.store, &rest, T).unwrap();
        assert_eq!(open_subjects(&mut f), ["pr:acme/pay#2"], "#1 left the file");
    }

    #[test]
    fn an_unreadable_channel_keeps_the_items_and_opens_no_engine_error() {
        let mut f = fixture("dispatch");
        let opened = reviews(
            &mut f,
            &[post("acme/pay", 1)],
            &[("acme/pay", 1, open_pr())],
        );
        reconcile(&mut f.store, &opened, T).unwrap();
        // A channel whose read failed has no data: unknown.
        clear_cache(&mut f);
        f.sql("INSERT INTO swr_queries (key, system, status, last_error, error_count) VALUES ('slack:channel:C02JU6C2Z1D', 'slack', 'error', 'slack_failed: down', 1);");
        let all = conditions_with(&mut f.store, &f.home, NOW, 20, 1, Some(team())).unwrap();
        assert_eq!(
            all.iter()
                .filter(|c| c.kind == Kind::ReviewRequested)
                .count(),
            1
        );
        assert!(
            all.iter().all(|c| c.kind != Kind::EngineError),
            "no engine-error for an unreadable cache"
        );
    }

    #[test]
    fn an_unknown_viewer_keeps_the_items_and_opens_no_engine_error() {
        let mut f = fixture("dispatch");
        let opened = reviews(
            &mut f,
            &[post("acme/pay", 1)],
            &[("acme/pay", 1, open_pr())],
        );
        reconcile(&mut f.store, &opened, T).unwrap();
        // The pass has not stored the viewer (or lost it): "unknown", not "empty".
        f.sql("DELETE FROM swr_viewer");
        let all = conditions_with(&mut f.store, &f.home, NOW, 20, 1, Some(team())).unwrap();
        assert_eq!(
            all.iter()
                .filter(|c| c.kind == Kind::ReviewRequested)
                .count(),
            1
        );
        assert!(all.iter().all(|c| c.kind != Kind::EngineError));
    }

    #[test]
    fn a_pull_request_github_cannot_read_is_dropped_and_all_unreadable_keeps_the_queue() {
        let mut f = fixture("dispatch");
        write_cache(&mut f, &[post("acme/pay", 1), post("acme/gone", 2)]);
        let mut answer = lookup(&[("acme/pay", 1, open_pr())]);
        answer.workflows.insert(("acme/gone".to_owned(), 2), None);
        crate::inbox::swr_fixture::lookup(&mut f.store, &answer).unwrap();
        let all = conditions_with(&mut f.store, &f.home, NOW, 20, 1, Some(team())).unwrap();
        assert_eq!(
            all.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(),
            ["pr:acme/pay#1"],
            "the readable one only, and no error"
        );

        let opened = reviews(
            &mut f,
            &[post("acme/pay", 1)],
            &[("acme/pay", 1, open_pr())],
        );
        reconcile(&mut f.store, &opened, T).unwrap();
        let blind = Lookup {
            me: ME_LOGIN.to_owned(),
            workflows: [(("acme/pay".to_owned(), 1), None)].into(),
        };
        crate::inbox::swr_fixture::lookup(&mut f.store, &blind).unwrap();
        let all = conditions_with(&mut f.store, &f.home, NOW, 20, 1, Some(team())).unwrap();
        assert_eq!(
            all.iter()
                .filter(|c| c.kind == Kind::ReviewRequested)
                .count(),
            1,
            "every Workflow unreadable: the queue stays"
        );
        assert!(all.iter().all(|c| c.kind != Kind::EngineError));
    }

    /// A `worker_stalled` Event is an `error` Event that wakes the Lead, and opens no Inbox item: no `engine-error`
    /// condition for a running `durable` Attempt that stalled.
    #[test]
    fn a_worker_stalled_event_opens_no_engine_error_item() {
        let mut f = fixture("dispatch");
        f.job("w", "running");
        f.sql(&format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, runner)
             VALUES ('pay', 'w-a1', 'w', 1, 'S2', 'ladder', 'running', 'cpd/pay--w-a1', '/w', '{T}', 'durable');
             INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
             VALUES (cpd_txn(), '{T}', 'error', 'ctx', 'pay', 'w', 'w-a1', 'x',
                     '{{\"source\":\"driver\",\"code\":\"worker_stalled\",\"details\":{{\"attempt\":\"w-a1\",\"minutes\":1}}}}');"
        ));
        let kinds: Vec<_> = conditions(&mut f.store, &f.home)
            .unwrap()
            .into_iter()
            .map(|c| c.kind)
            .collect();
        assert!(
            !kinds.contains(&Kind::EngineError),
            "a worker_stalled Event opens no engine-error item: {kinds:?}"
        );
    }

    // ---- all together ----

    #[test]
    fn conditions_come_in_the_order_of_the_kinds_and_a_quiet_store_has_none() {
        let mut f = fixture("dispatch");
        assert!(conditions(&mut f.store, &f.home).unwrap().is_empty());
        f.state("l-pay", "blocked", Some("Which?"));
        f.review("r1", "pending");
        f.reply("mgr");
        f.job("a", "failed");
        f.event("job-failed", None, Some("a"), T, r#"{}"#);
        let kinds: Vec<_> = conditions(&mut f.store, &f.home)
            .unwrap()
            .into_iter()
            .map(|c| c.kind)
            .collect();
        assert_eq!(
            kinds,
            [Kind::Dialog, Kind::PlanReview, Kind::Unread],
            "the failed Job is no item"
        );
        pull_request(&mut f, 1, "failed", "mergeable", "changes_requested");
        pull_request(&mut f, 2, "passed", "mergeable", "approved");
        f.sql("INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored, review_requested)
               VALUES ('o/r', 3, 'u', 'T', 0, 'open', 'none', 'none', 't', 't', 0, 1)");
        let tail: Vec<_> = conditions(&mut f.store, &f.home)
            .unwrap()
            .into_iter()
            .map(|c| c.kind)
            .skip(3)
            .collect();
        assert_eq!(tail, [Kind::PrReady, Kind::PrReady]);
        write_cache(&mut f, &[post("acme/pay", 812)]);
        let lookup = lookup(&[("acme/pay", 812, open_pr())]);
        crate::inbox::swr_fixture::lookup(&mut f.store, &lookup).unwrap();
        let kinds: Vec<_> = conditions_with(&mut f.store, &f.home, NOW, 20, 1, Some(team()))
            .unwrap()
            .into_iter()
            .map(|c| c.kind)
            .collect();
        assert_eq!(
            kinds[3..],
            [Kind::PrReady, Kind::PrReady, Kind::ReviewRequested]
        );
    }
}
