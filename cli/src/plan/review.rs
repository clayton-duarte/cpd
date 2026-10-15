//! `plan render`, `plan review` and `plan decide` (commands.md §4.5–§4.7;
//! contract §2.2 Plan review, §2.3), with the pi adapter's side in §15.5.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::jobs::{self, Job};
use super::row::{self, Phase, PlanRow};
use super::{DecideArgs, Env, RenderArgs, ReviewArgs, read_text_file, render, write_file};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
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

/// The Plan, its Jobs and its latest review, in one read transaction.
fn snapshot(env: &mut Env, id: &PlanId) -> Result<(PlanRow, Vec<Job>, Option<Review>), CpdError> {
    env.store.read(|conn| Ok((row::load(conn, id)?, jobs::load(conn, id)?, latest(conn, id)?)))
}

fn plan_markdown(plan: &PlanRow, jobs: &[Job]) -> String {
    render::markdown(&plan.title, plan.context.as_str(), plan.phase.as_str(), jobs)
}

/// `plan render` (§4.5).
pub(super) fn render(args: RenderArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let (plan, jobs, _) = snapshot(&mut env, &id)?;
    let path = env.home.plan_md(&id);
    write_file(&path, &plan_markdown(&plan, &jobs))?;
    Ok(Output::success(json!({
        "path": path.display().to_string(),
        "fingerprint": jobs::plan_fingerprint(&jobs),
        "draft_jobs": jobs.iter().filter(|j| j.is_draft()).count(),
    })))
}

/// `plan review` (§4.6).
pub(super) fn review(args: ReviewArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let (plan, jobs, latest_review) = snapshot(&mut env, &id)?;
    if plan.phase != Phase::Plan {
        return Err(CpdError::new(
            ErrorCode::IllegalPhase,
            format!("Plan {id} is in phase {}: a Plan review is requested in phase plan.", plan.phase.as_str()),
        )
        .with("plan", id.as_str())
        .with("phase", plan.phase.as_str()));
    }
    let drafts = jobs.iter().filter(|j| j.is_draft()).count();
    if drafts == 0 {
        return Err(no_draft_jobs(&id));
    }
    let fingerprint = jobs::plan_fingerprint(&jobs);
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
    let text = plan_markdown(&plan, &jobs);
    write_file(&env.home.plan_md(&id), &text)?;
    let path = env.home.plan_review_md(&id, review);
    write_file(&path, &text)?;

    let requested_at = env.store.write("plan review", |tx| {
        // Compare-and-swap on what was read: the phase, the Jobs and the latest review.
        let (now, jobs_now, latest_now) = (row::load(tx, &id)?, jobs::load(tx, &id)?, latest(tx, &id)?);
        let latest_number = latest_now.map_or(0, |r| r.id.number().get());
        if now.phase != Phase::Plan || jobs::plan_fingerprint(&jobs_now) != fingerprint || latest_number + 1 != number {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Plan {id} changed while its review was rendered: rerun the command."))
                .with("plan", id.as_str()));
        }
        tx.execute(
            "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at)
             SELECT ?1, ?2, ?3, ?4, 'pending', at FROM txns WHERE id = cpd_txn()",
            rusqlite::params![id.as_str(), review.to_string(), number, fingerprint],
        )?;
        let text = format!("Plan review {review} of Plan {id} requested: the User reviews {drafts} draft Jobs in plannotator.");
        let data = json!({"review": review, "fingerprint": fingerprint, "status": "pending"});
        append(tx, NewEvent::new(Kind::PlanReview, text, data).plan(id.clone()))?;
        Ok(tx.query_row("SELECT at FROM txns WHERE id = cpd_txn()", [], |r| r.get::<_, String>(0))?)
    })?;
    Ok(Output::success(json!({
        "review": {"id": review, "fingerprint": fingerprint, "status": "pending", "requested_at": requested_at},
        "path": path.display().to_string(),
        "next": REVIEW_NEXT,
    })))
}

pub(super) fn no_draft_jobs(id: &PlanId) -> CpdError {
    CpdError::new(ErrorCode::NoDraftJobs, format!("Plan {id} has no draft Job: add Jobs with cpd2 job add first."))
        .with("plan", id.as_str())
}

/// `plan decide` (§4.7).
pub(super) fn decide(args: DecideArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let status = if args.approve { "approved" } else { "rejected" };
    let feedback = args.feedback_file.as_deref().map(read_text_file).transpose()?;
    let mut env = Env::open()?;
    let (review, jobs) = env.store.read(|conn| {
        row::load(conn, &id)?;
        let review = find(conn, &id, args.review)?.ok_or_else(|| {
            CpdError::new(ErrorCode::ReviewNotFound, format!("Plan {id} has no review {}.", args.review))
                .with("plan", id.as_str())
                .with("review", args.review.to_string())
        })?;
        Ok((review, jobs::load(conn, &id)?))
    })?;
    let current = review.fingerprint == jobs::plan_fingerprint(&jobs);
    if review.status == status {
        return Ok(decided(&review, current, true));
    }
    if review.status != "pending" {
        return Err(review_decided(&id, &review));
    }

    let review = env.store.write("plan decide", |tx| {
        let changed = tx.execute(
            "UPDATE plan_reviews SET status = ?1, feedback = ?2, decided_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE plan = ?3 AND id = ?4 AND status = 'pending'",
            rusqlite::params![status, feedback, id.as_str(), review.id.to_string()],
        )?;
        let now = find(tx, &id, review.id)?.expect("a review is never deleted");
        if changed == 0 {
            return Err(review_decided(&id, &now));
        }
        let verdict = status.to_uppercase();
        let text = match status {
            "approved" => format!("Plan review {} of Plan {id} was {verdict}. Next: cpd2 plan lock", review.id),
            _ => format!("Plan review {} of Plan {id} was {verdict}. Revise the plan (job edit, job add, job cancel), then cpd2 plan review", review.id),
        };
        let data = json!({"review": review.id, "fingerprint": review.fingerprint, "status": status, "feedback": feedback});
        append(tx, NewEvent::new(Kind::PlanReview, text, data).plan(id.clone()))?;
        Ok(now)
    })?;
    Ok(decided(&review, current, false))
}

fn review_decided(id: &PlanId, review: &Review) -> CpdError {
    CpdError::new(ErrorCode::ReviewDecided, format!("Plan review {} of Plan {id} is already {}.", review.id, review.status))
        .with("plan", id.as_str())
        .with("review", review.id.to_string())
        .with("status", review.status.clone())
}

/// `plan decide`'s output: `next` is `cpd2 plan lock` only when the review is
/// approved and current.
fn decided(review: &Review, current: bool, already: bool) -> Output {
    let next = if review.status == "approved" && current { json!("cpd2 plan lock") } else { Value::Null };
    let mut out = json!({
        "review": {"id": review.id, "status": review.status, "fingerprint": review.fingerprint,
                   "feedback": review.feedback, "decided_at": review.decided_at},
        "current": current,
        "next": next,
    });
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
