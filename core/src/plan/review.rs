//! `plan render`, `plan review` and `plan decide` (commands.md §4.5–§4.7;
//! contract §2.2 Plan review, §2.3), with the pi adapter's side in §15.5.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::fingerprint::{PrSpec, load_prs, plan_fingerprint};
use super::jobs::{self, Job};
use super::overlap::{self, Overlap};
use super::row::Phase;
use super::show::{Head, load_head};
use super::{DecideArgs, Env, RenderArgs, ReviewArgs, read_text_file, render, write_file};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{PlanId, ReviewId};
use crate::output::Output;

/// What the Lead does after requesting a review (commands.md §4.6).
const REVIEW_NEXT: &str = "The User reviews the plan in plannotator. Stop and wait for the decision message.";

/// A `plan_reviews` row.
#[derive(Clone, Debug)]
struct Review {
    id: ReviewId,
    status: String,
    fingerprint: String,
    feedback: Option<String>,
    requested_at: String,
    decided_at: Option<String>,
}

const REVIEW_COLUMNS: &str = "id, status, fingerprint, feedback, requested_at, decided_at";

fn review_row(r: &rusqlite::Row) -> rusqlite::Result<(String, String, String, Option<String>, String, Option<String>)> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
}

fn parse_review(raw: Option<(String, String, String, Option<String>, String, Option<String>)>) -> Result<Option<Review>, CpdError> {
    raw.map(|(id, status, fingerprint, feedback, requested_at, decided_at)| {
        Ok(Review { id: ReviewId::parse(&id)?, status, fingerprint, feedback, requested_at, decided_at })
    })
    .transpose()
}

/// The Plan's latest review: the highest number.
fn latest(conn: &Connection, plan: &PlanId) -> Result<Option<Review>, CpdError> {
    let raw = conn
        .query_row(
            &format!("SELECT {REVIEW_COLUMNS} FROM plan_reviews WHERE plan = ?1 ORDER BY number DESC LIMIT 1"),
            [plan.as_str()],
            review_row,
        )
        .optional()?;
    parse_review(raw)
}

fn find(conn: &Connection, plan: &PlanId, review: ReviewId) -> Result<Option<Review>, CpdError> {
    let raw = conn
        .query_row(
            &format!("SELECT {REVIEW_COLUMNS} FROM plan_reviews WHERE plan = ?1 AND id = ?2"),
            [plan.as_str(), &review.to_string()],
            review_row,
        )
        .optional()?;
    parse_review(raw)
}

/// What `plan render` and `plan review` read, in one read transaction.
struct Snapshot {
    plan: Head,
    prs: Vec<PrSpec>,
    jobs: Vec<Job>,
    overlaps: Vec<Overlap>,
    review: Option<Review>,
    fingerprint: String,
}

fn snapshot(env: &mut Env, id: &PlanId) -> Result<Snapshot, CpdError> {
    env.store.read(|conn| {
        let (prs, jobs) = (load_prs(conn, id)?, jobs::load(conn, id)?);
        let overlaps = overlap::find(&prs, &jobs, &overlap::load_others(conn, id)?);
        Ok(Snapshot {
            plan: load_head(conn, id)?,
            review: latest(conn, id)?,
            fingerprint: plan_fingerprint(conn, id)?,
            prs,
            jobs,
            overlaps,
        })
    })
}

fn plan_markdown(s: &Snapshot) -> String {
    render::markdown(&s.plan.title, s.plan.context.as_str(), s.plan.phase.as_str(), &s.prs, &s.jobs, &s.overlaps)
}

/// `plan render` (§4.5).
pub(super) fn render(args: RenderArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let s = snapshot(&mut env, &id)?;
    let path = env.home.plan_md(&id);
    write_file(&path, &plan_markdown(&s))?;
    Ok(Output::success(json!({
        "path": path.display().to_string(),
        "fingerprint": s.fingerprint,
        "draft_jobs": s.jobs.iter().filter(|j| j.is_draft()).count(),
        "prs": s.prs.iter().filter(|p| p.status != "cancelled").count(),
        "overlaps": s.overlaps.iter().map(Overlap::to_json).collect::<Vec<_>>(),
    })))
}

/// `plan review` (§4.6).
pub(super) fn review(args: ReviewArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let s = snapshot(&mut env, &id)?;
    let (plan, jobs, latest_review, fingerprint) = (&s.plan, &s.jobs, s.review.clone(), s.fingerprint.clone());
    if plan.phase == Phase::Grounding {
        return Err(CpdError::new(
            ErrorCode::IllegalPhase,
            format!("Plan {id} is in phase {}: a Plan review is requested from phase plan on.", plan.phase.as_str()),
        )
        .with("plan", id.as_str())
        .with("phase", plan.phase.as_str()));
    }
    let drafts = jobs.iter().filter(|j| j.is_draft()).count();
    if drafts == 0 {
        return Err(no_draft_jobs(&id));
    }
    if let Some(r) = latest_review.as_ref().filter(|r| r.status == "pending" && r.fingerprint == fingerprint) {
        return Ok(Output::success(json!({
            "review": {"id": r.id, "fingerprint": r.fingerprint, "status": r.status, "requested_at": r.requested_at},
            "path": env.home.plan_review_md(&id, r.id).display().to_string(),
            "next": REVIEW_NEXT,
            "already": true,
        })));
    }

    // The files first, so the review row never names a file that is missing.
    let number = latest_review.as_ref().map_or(0, |r| r.id.number().get()) + 1;
    let review = ReviewId::new(std::num::NonZeroU32::new(number).expect("one more than a count"));
    let text = plan_markdown(&s);
    write_file(&env.home.plan_md(&id), &text)?;
    let path = env.home.plan_review_md(&id, review);
    write_file(&path, &text)?;

    let requested_at = env.store.write("plan review", |tx| {
        // Compare-and-swap on what was read: the phase, the plan fingerprint and the latest review.
        let (now, fingerprint_now, latest_now) = (load_head(tx, &id)?, plan_fingerprint(tx, &id)?, latest(tx, &id)?);
        let latest_number = latest_now.map_or(0, |r| r.id.number().get());
        if now.phase != plan.phase || fingerprint_now != fingerprint || latest_number + 1 != number {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Plan {id} changed while its review was rendered: rerun the command."))
                .with("plan", id.as_str()));
        }
        tx.execute(
            "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at)
             SELECT ?1, ?2, ?3, ?4, 'pending', at FROM txns WHERE id = cpd_txn()",
            rusqlite::params![id.as_str(), review.to_string(), number, fingerprint],
        )?;
        let text = format!("Plan review {review} of Plan {id} requested: the User reviews {drafts} draft Jobs in plannotator.");
        append(tx, NewEvent::plan_review_requested(text, review, &fingerprint).plan(id.clone()))?;
        Ok(tx.query_row("SELECT at FROM txns WHERE id = cpd_txn()", [], |r| r.get::<_, String>(0))?)
    })?;
    Ok(Output::success(json!({
        "review": {"id": review, "fingerprint": fingerprint, "status": "pending", "requested_at": requested_at},
        "path": path.display().to_string(),
        "next": REVIEW_NEXT,
    })))
}

pub(super) fn no_draft_jobs(id: &PlanId) -> CpdError {
    CpdError::new(ErrorCode::NoDraftJobs, format!("Plan {id} has no draft Job: add Jobs with cpd job add first."))
        .with("plan", id.as_str())
}

/// `plan decide` (§4.7). In a phase after `plan`, a current approval also
/// queues the drafts it covered, in the same transaction.
pub(super) fn decide(args: DecideArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let status = if args.approve { "approved" } else { "rejected" };
    let feedback = args.feedback_file.as_deref().map(read_text_file).transpose()?;
    let mut env = Env::open()?;
    let (phase, review, fingerprint, queued) = env.store.read(|conn| {
        let phase = load_head(conn, &id)?.phase;
        let review = find(conn, &id, args.review)?.ok_or_else(|| {
            CpdError::new(ErrorCode::ReviewNotFound, format!("Plan {id} has no review {}.", args.review))
                .with("plan", id.as_str())
                .with("review", args.review.to_string())
        })?;
        let queued = queued_by(conn, &id, review.id)?;
        Ok((phase, review, plan_fingerprint(conn, &id)?, queued))
    })?;
    if review.status == status {
        let current = review.fingerprint == fingerprint;
        return Ok(decided(&review, phase, current, queued.as_deref(), true));
    }
    if review.status != "pending" {
        return Err(review_decided(&id, &review));
    }

    let (review, phase, current, queued) = env.store.write("plan decide", |tx| {
        // The phase and the drafts as they are now: an approval queues what it covered only.
        let phase = load_head(tx, &id)?.phase;
        let all = jobs::load(tx, &id)?;
        let current = review.fingerprint == plan_fingerprint(tx, &id)?;
        let changed = tx.execute(
            "UPDATE plan_reviews SET status = ?1, feedback = ?2, decided_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE plan = ?3 AND id = ?4 AND status = 'pending'",
            rusqlite::params![status, feedback, id.as_str(), review.id.to_string()],
        )?;
        let now = find(tx, &id, review.id)?.expect("a review is never deleted");
        if changed == 0 {
            return Err(review_decided(&id, &now));
        }
        let queues = status == "approved" && current && phase != Phase::Plan;
        let mut queued: Vec<String> = Vec::new();
        if queues {
            for job in all.iter().filter(|j| j.is_draft()) {
                tx.execute(
                    "UPDATE jobs SET status = 'queued', fingerprint = ?1 WHERE plan = ?2 AND key = ?3 AND status = 'draft'",
                    [jobs::locked_fingerprint(job).as_str(), id.as_str(), job.key.as_str()],
                )?;
                queued.push(job.key.clone());
            }
            queued.sort();
        }
        let verdict = status.to_uppercase();
        let text = match (status, phase) {
            ("approved", Phase::Plan) => format!("Plan review {} of Plan {id} was {verdict}. Next: cpd plan lock", review.id),
            ("approved", _) if queues => format!(
                "Plan review {} of Plan {id} was {verdict}: the draft Jobs {} are queued for the engine; no lock is needed. Next: cpd plan show --plan {id}",
                review.id,
                queued.join(", ")
            ),
            ("approved", _) => format!(
                "Plan review {} of Plan {id} was {verdict}, but the draft Jobs changed since it was requested: nothing is queued. Next: cpd plan review --plan {id}",
                review.id
            ),
            _ => format!("Plan review {} of Plan {id} was {verdict}. Revise the plan (job edit, job add, job cancel), then cpd plan review", review.id),
        };
        let jobs = queues.then_some(&queued);
        append(tx, NewEvent::plan_review_decided(text, review.id, &review.fingerprint, status, feedback, jobs).plan(id.clone()))?;
        Ok((now, phase, current, queues.then_some(queued)))
    })?;
    Ok(decided(&review, phase, current, queued.as_deref(), false))
}

/// The Jobs an approval of `review` queued (its decision Event's `data.jobs`),
/// if it queued any: what a rerun of `plan decide` reports again.
fn queued_by(conn: &Connection, plan: &PlanId, review: ReviewId) -> Result<Option<Vec<String>>, CpdError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT json_extract(data, '$.jobs') FROM events
              WHERE kind = 'plan-review' AND plan = ?1 AND json_extract(data, '$.review') = ?2
                AND json_extract(data, '$.status') = 'approved' AND json_type(data, '$.jobs') = 'array'
              ORDER BY id DESC LIMIT 1",
            [plan.as_str(), &review.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|text| serde_json::from_str(&text).map_err(|e| super::bad_row(format!("plan-review Event of {plan}/{review}: {e}")))).transpose()
}

fn review_decided(id: &PlanId, review: &Review) -> CpdError {
    CpdError::new(ErrorCode::ReviewDecided, format!("Plan review {} of Plan {id} is already {}.", review.id, review.status))
        .with("plan", id.as_str())
        .with("review", review.id.to_string())
        .with("status", review.status.clone())
}

/// `plan decide`'s output (§4.7): in phase `plan`, `next` is `cpd plan lock`
/// only when the review is approved and current. In a later phase, an
/// approval that queued the drafts lists them in `jobs` and has no `next`;
/// one that did not (the drafts changed since) asks for a new review.
fn decided(review: &Review, phase: Phase, current: bool, queued: Option<&[String]>, already: bool) -> Output {
    let approved = review.status == "approved";
    let next = match (phase, queued) {
        (_, Some(_)) => Value::Null,
        (Phase::Plan, None) if approved && current => json!("cpd plan lock"),
        (Phase::Plan | Phase::Grounding, None) => Value::Null,
        (_, None) if approved => json!("cpd plan review"),
        (_, None) => Value::Null,
    };
    let mut out = json!({
        "review": {"id": review.id, "status": review.status, "fingerprint": review.fingerprint,
                   "feedback": review.feedback, "decided_at": review.decided_at},
        "current": current,
        "next": next,
    });
    if let Some(jobs) = queued {
        out["jobs"] = json!(jobs);
    }
    if already {
        out["already"] = json!(true);
    }
    Output::success(out)
}

/// The latest review, for `plan lock`: approved for `fingerprint`.
pub(super) fn latest_approved_for(conn: &Connection, plan: &PlanId, fingerprint: &str) -> Result<Option<String>, CpdError> {
    Ok(latest(conn, plan)?.filter(|r| r.status == "approved" && r.fingerprint == fingerprint).map(|r| r.id.to_string()))
}

/// The latest review as `plan_not_approved` reports it.
pub(super) fn latest_json(conn: &Connection, plan: &PlanId) -> Result<Value, CpdError> {
    Ok(latest(conn, plan)?.map_or(Value::Null, |r| json!({"id": r.id, "status": r.status, "fingerprint": r.fingerprint})))
}
