//! `job add` (commands.md §5.1): `[*] → draft` in phase `plan`; in a phase
//! after it, an appended `queued` Job, fingerprint stored at once, or with
//! `--draft` an appended `draft`, which only an approved Plan review queues.

use rusqlite::Connection;
use serde_json::{Value, json};

use super::spec::{self, Spec};
use super::{AddArgs, Env, check_needs_available, check_needs_known, check_needs_same_pr, check_pr_live, plan_phase, sorted_needs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::output::Output;
use crate::args::PrArg;
use crate::plan::fingerprint::load_prs;
use crate::plan::jobs::{self, Job};
use crate::plan::locked_fingerprint;

const COMMAND: &str = "job add";

/// What `job add` was asked for, parsed.
struct Wanted {
    key: JobKey,
    pr: PrArg,
    title: String,
    tier: String,
    spec: Spec,
    needs: Vec<String>,
    /// `--review` (B2): the needs are set from the Plan, not given.
    review: bool,
    /// `--draft`: a draft in a phase after `plan` too.
    draft: bool,
}

enum Decision {
    /// The Job exists with these Locked fields and needs.
    Already(Job),
    Insert(Job),
}

pub(super) fn add(args: AddArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let needs = sorted_needs(&args.needs)?;
    let spec = spec::read(&args.spec)?;
    let mut env = Env::open()?;
    env.check_tier(args.tier)?;
    let wanted = Wanted { key: args.key, pr: args.pr, title: args.title.as_str().to_owned(), tier: args.tier.to_string(), spec, needs, review: args.review, draft: args.draft };

    let decision = env.store.read(|conn| decide(conn, &plan, &wanted))?;
    let (job, already) = match decision {
        Decision::Already(job) => (job, true),
        Decision::Insert(_) => env.store.write(COMMAND, |tx| match decide(tx, &plan, &wanted)? {
            Decision::Already(job) => Ok((job, true)),
            Decision::Insert(job) => {
                insert(tx, &plan, &job, wanted.review)?;
                let text = format!("Job {} added to Plan {plan} as {}. Next: cpd job show {} --plan {plan}", job.key, job.status, job.key);
                append(
                    tx,
                    NewEvent::new(
                        Kind::JobAdded,
                        text,
                        json!({"pr": job.pr, "status": job.status, "appended": job.appended, "tier": job.tier, "needs": job.needs, "review": wanted.review}),
                    )
                    .plan(plan.clone())
                    .job(wanted.key.clone()),
                )?;
                Ok((job, false))
            }
        })?,
    };
    let mut out = json!({"job": {"key": job.key, "pr": job.pr, "status": job.status, "appended": job.appended, "tier": job.tier, "needs": job.needs, "review": wanted.review}});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

fn decide(conn: &Connection, plan: &PlanId, wanted: &Wanted) -> Result<Decision, CpdError> {
    let phase = plan_phase(conn, plan)?;
    if phase == "grounding" || (phase == "done" && !wanted.draft) {
        return Err(CpdError::new(
            ErrorCode::IllegalPhase,
            format!("Plan {plan} is in phase {phase}: Jobs are added from phase plan on, and in phase done only as drafts (--draft)."),
        )
        .with("plan", plan.as_str())
        .with("phase", phase));
    }
    let prs = load_prs(conn, plan)?;
    let pr = wanted.pr.select(plan, &prs)?.key.clone();
    check_pr_live(&prs, plan, &pr)?;
    let all = jobs::load(conn, plan)?;
    let needs = if wanted.review { review_needs(&all, plan, &pr, wanted.key.as_str())? } else { wanted.needs.clone() };
    let appended = phase != "plan";
    // In phase `plan` every Job is a draft; after it, only with `--draft`.
    let draft = !appended || wanted.draft;
    let mut job = Job {
        seq: 0,
        key: wanted.key.as_str().to_owned(),
        pr,
        title: wanted.title.clone(),
        tier: wanted.tier.clone(),
        goal: wanted.spec.goal.clone(),
        context: wanted.spec.context.clone(),
        touches: json!(wanted.spec.touches),
        acceptance: json!(wanted.spec.acceptance),
        out_of_scope: json!(wanted.spec.out_of_scope),
        status: if draft { "draft" } else { "queued" }.to_owned(),
        appended,
        review: wanted.review,
        fingerprint: None,
        retry_tier: None,
        cancel_reason: None,
        cancel_requested_at: None,
        failed_cause: None,
        merged_sha: None,
        created_at: String::new(),
        needs,
    };
    if let Some(existing) = all.iter().find(|j| j.key == job.key) {
        if same_locked_fields(existing, &job) && existing.review == wanted.review {
            return Ok(Decision::Already(existing.clone()));
        }
        return Err(CpdError::new(
            ErrorCode::JobKeyExists,
            format!("Plan {plan} already has a Job {} with other Locked fields.", job.key),
        )
        .with("plan", plan.as_str())
        .with("job", job.key.as_str()));
    }
    check_needs_known(&all, plan, &job.needs)?;
    check_needs_same_pr(&all, plan, &job.pr, &job.needs)?;
    check_needs_available(&all, plan, &job.needs)?;
    if !draft {
        check_needs_not_draft(&all, plan, &job)?;
        job.fingerprint = Some(locked_fingerprint(&job));
    }
    Ok(Decision::Insert(job))
}

/// A `queued` Job may not need a `draft` (§5.1): it could start before its need
/// exists. `details.jobs` lists the drafts; for a review Job they are every
/// draft of its PR, as its needs are.
fn check_needs_not_draft(all: &[Job], plan: &PlanId, job: &Job) -> Result<(), CpdError> {
    let drafts: Vec<&str> = all.iter().filter(|j| j.is_draft() && job.needs.contains(&j.key)).map(|j| j.key.as_str()).collect();
    if drafts.is_empty() {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::NeedsUnavailable,
        format!(
            "Job {} would be queued but needs the draft {}: it could start before its need exists. Add it with --draft, or have the drafts approved first (cpd plan review).",
            job.key,
            drafts.join(", ")
        ),
    )
    .with("plan", plan.as_str())
    .with("job", job.key.as_str())
    .with("jobs", drafts))
}

/// A review Job's Dependencies (B2): every Job of its PR that exists now and is
/// not `cancelled`, itself excepted (a rerun of the same add). A `failed` Job
/// refuses with `needs_unavailable` and `details.jobs`: retry or cancel it first.
fn review_needs(all: &[Job], plan: &PlanId, pr: &str, key: &str) -> Result<Vec<String>, CpdError> {
    let of_pr = || all.iter().filter(|j| j.pr == pr && j.key != key && j.status != "cancelled");
    let failed: Vec<&str> = of_pr().filter(|j| j.status == "failed").map(|j| j.key.as_str()).collect();
    if !failed.is_empty() {
        return Err(CpdError::new(
            ErrorCode::NeedsUnavailable,
            format!("A review Job needs every Job of PR {pr}, and {} failed: retry or cancel it first.", failed.join(", ")),
        )
        .with("plan", plan.as_str())
        .with("pr", pr)
        .with("jobs", failed));
    }
    let mut needs: Vec<String> = of_pr().map(|j| j.key.clone()).collect();
    needs.sort();
    Ok(needs)
}

/// The Locked fields (§2.2: the Spec, `title`, `tier`, `pr`) and the needs; `review` is compared by the caller.
fn same_locked_fields(a: &Job, b: &Job) -> bool {
    (&a.pr, &a.title, &a.tier, &a.goal, &a.context, &a.touches, &a.acceptance, &a.out_of_scope, &a.needs)
        == (&b.pr, &b.title, &b.tier, &b.goal, &b.context, &b.touches, &b.acceptance, &b.out_of_scope, &b.needs)
}

fn insert(tx: &Connection, plan: &PlanId, job: &Job, review: bool) -> Result<(), CpdError> {
    let array = |v: &Value| v.to_string();
    tx.execute(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                           status, appended, fingerprint, review, created_txn, created_at)
         VALUES (?1, ?13, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?14, cpd_txn(),
                 (SELECT at FROM txns WHERE id = cpd_txn()))",
        rusqlite::params![
            plan.as_str(),
            job.key,
            job.title,
            job.tier,
            job.goal,
            job.context,
            array(&job.touches),
            array(&job.acceptance),
            array(&job.out_of_scope),
            job.status,
            job.appended,
            job.fingerprint,
            job.pr,
            review,
        ],
    )?;
    // An appended Job reopens its PR for E4: `all-merged` is sent again once the Jobs are all merged or cancelled.
    tx.execute("UPDATE prs SET all_merged_at = NULL WHERE plan = ?1 AND key = ?2 AND all_merged_at IS NOT NULL", [plan.as_str(), job.pr.as_str()])?;
    for need in &job.needs {
        tx.execute("INSERT INTO dependencies (plan, job, needs) VALUES (?1, ?2, ?3)", [plan.as_str(), job.key.as_str(), need.as_str()])?;
    }
    Ok(())
}
