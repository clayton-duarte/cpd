//! `job add` (commands.md §5.1): `[*] → draft` in phase `plan`, and an
//! appended `queued` Job, fingerprint stored at once, in `dispatch` or `pr`.

use rusqlite::Connection;
use serde_json::{Value, json};

use super::spec::{self, Spec};
use super::{AddArgs, Env, check_needs, plan_phase, sorted_needs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::output::Output;
use crate::plan::jobs::{self, Job};
use crate::plan::locked_fingerprint;

const COMMAND: &str = "job add";

/// What `job add` was asked for, parsed.
struct Wanted {
    key: JobKey,
    title: String,
    tier: String,
    spec: Spec,
    needs: Vec<String>,
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
    let wanted = Wanted { key: args.key, title: args.title.as_str().to_owned(), tier: args.tier.to_string(), spec, needs };

    let decision = env.store.read(|conn| decide(conn, &plan, &wanted))?;
    let (job, already) = match decision {
        Decision::Already(job) => (job, true),
        Decision::Insert(_) => env.store.write(COMMAND, |tx| match decide(tx, &plan, &wanted)? {
            Decision::Already(job) => Ok((job, true)),
            Decision::Insert(job) => {
                insert(tx, &plan, &job)?;
                let text = format!("Job {} added to Plan {plan} as {}. Next: cpd job show {} --plan {plan}", job.key, job.status, job.key);
                append(
                    tx,
                    NewEvent::new(
                        Kind::JobAdded,
                        text,
                        json!({"status": job.status, "appended": job.appended, "tier": job.tier, "needs": job.needs}),
                    )
                    .plan(plan.clone())
                    .job(wanted.key.clone()),
                )?;
                Ok((job, false))
            }
        })?,
    };
    let mut out = json!({"job": {"key": job.key, "status": job.status, "appended": job.appended, "tier": job.tier, "needs": job.needs}});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

fn decide(conn: &Connection, plan: &PlanId, wanted: &Wanted) -> Result<Decision, CpdError> {
    let phase = plan_phase(conn, plan)?;
    if !matches!(phase.as_str(), "plan" | "dispatch" | "pr") {
        return Err(CpdError::new(
            ErrorCode::IllegalPhase,
            format!("Plan {plan} is in phase {phase}: Jobs are added in phase plan, dispatch or pr."),
        )
        .with("plan", plan.as_str())
        .with("phase", phase));
    }
    let all = jobs::load(conn, plan)?;
    let appended = phase != "plan";
    let mut job = Job {
        seq: 0,
        key: wanted.key.as_str().to_owned(),
        title: wanted.title.clone(),
        tier: wanted.tier.clone(),
        goal: wanted.spec.goal.clone(),
        context: wanted.spec.context.clone(),
        touches: json!(wanted.spec.touches),
        acceptance: json!(wanted.spec.acceptance),
        out_of_scope: json!(wanted.spec.out_of_scope),
        status: if appended { "queued" } else { "draft" }.to_owned(),
        appended,
        fingerprint: None,
        retry_tier: None,
        cancel_reason: None,
        cancel_requested_at: None,
        failed_cause: None,
        merged_sha: None,
        created_at: String::new(),
        needs: wanted.needs.clone(),
    };
    if let Some(existing) = all.iter().find(|j| j.key == job.key) {
        if same_locked_fields(existing, &job) {
            return Ok(Decision::Already(existing.clone()));
        }
        return Err(CpdError::new(
            ErrorCode::JobKeyExists,
            format!("Plan {plan} already has a Job {} with other Locked fields.", job.key),
        )
        .with("plan", plan.as_str())
        .with("job", job.key.as_str()));
    }
    check_needs(&all, plan, &wanted.needs)?;
    if appended {
        job.fingerprint = Some(locked_fingerprint(&job));
    }
    Ok(Decision::Insert(job))
}

/// The Locked fields (§2.2: the Spec, `title`, `tier`) and the needs.
fn same_locked_fields(a: &Job, b: &Job) -> bool {
    (&a.title, &a.tier, &a.goal, &a.context, &a.touches, &a.acceptance, &a.out_of_scope, &a.needs)
        == (&b.title, &b.tier, &b.goal, &b.context, &b.touches, &b.acceptance, &b.out_of_scope, &b.needs)
}

fn insert(tx: &Connection, plan: &PlanId, job: &Job) -> Result<(), CpdError> {
    let array = |v: &Value| v.to_string();
    tx.execute(
        "INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                           status, appended, fingerprint, created_txn, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, cpd_txn(),
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
        ],
    )?;
    for need in &job.needs {
        tx.execute("INSERT INTO dependencies (plan, job, needs) VALUES (?1, ?2, ?3)", [plan.as_str(), job.key.as_str(), need.as_str()])?;
    }
    Ok(())
}
