//! `job edit` (commands.md §5.2): the Locked fields and Dependencies of a
//! `draft` Job, and its PR; a Locked Job is refused with `job_locked` (§3.2).

use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::json;

use super::spec::{self, Spec};
use super::{EditArgs, Env, check_needs_available, check_needs_known, check_needs_same_pr, check_pr_live, find, plan_phase, sorted_needs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::output::Output;
use crate::plan::fingerprint::load_prs;
use crate::plan::jobs::{self, Job};

const COMMAND: &str = "job edit";

/// The flags given, parsed.
struct Edits {
    title: Option<String>,
    tier: Option<String>,
    spec: Option<Spec>,
    pr: Option<String>,
    needs: Option<Vec<String>>,
}

enum Decision {
    /// No value changes.
    Already(Job),
    Update { before: Job, after: Job, fields: Vec<&'static str> },
}

pub(super) fn edit(args: EditArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    if args.title.is_none() && args.tier.is_none() && args.spec.is_none() && args.pr.is_none() && args.needs.is_none() && !args.no_needs {
        return Err(CpdError::new(
            ErrorCode::NothingToEdit,
            "Nothing to edit: pass --title, --tier, --spec, --pr, --needs or --no-needs.",
        ));
    }
    let needs = match (&args.needs, args.no_needs) {
        (Some(keys), _) => Some(sorted_needs(keys)?),
        (None, true) => Some(Vec::new()),
        (None, false) => None,
    };
    let spec = args.spec.as_ref().map(|path: &PathBuf| spec::read(path)).transpose()?;
    let mut env = Env::open()?;
    if let Some(tier) = args.tier {
        env.check_tier(tier)?;
    }
    let edits = Edits { title: args.title.map(|t| t.as_str().to_owned()), tier: args.tier.map(|t| t.to_string()), spec, pr: args.pr.map(|p| p.as_str().to_owned()), needs };
    let key = args.key;

    let decision = env.store.read(|conn| decide(conn, &plan, &key, &edits))?;
    let (job, fields, already) = match decision {
        Decision::Already(job) => (job, Vec::new(), true),
        Decision::Update { .. } => env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, &edits)? {
            Decision::Already(job) => Ok((job, Vec::new(), true)),
            Decision::Update { before, after, fields } => {
                update(tx, &plan, &before, &after)?;
                let text = format!("Job {key} of Plan {plan} edited ({}). Next: cpd plan render --plan {plan}", fields.join(", "));
                append(tx, NewEvent::job_edited(text, &fields).plan(plan.clone()).job(key.clone()))?;
                Ok((after, fields, false))
            }
        })?,
    };
    let mut out = json!({"job": {"key": job.key, "status": job.status}, "fields": fields});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

fn decide(conn: &Connection, plan: &PlanId, key: &JobKey, edits: &Edits) -> Result<Decision, CpdError> {
    plan_phase(conn, plan)?;
    let all = jobs::load(conn, plan)?;
    let before = find(&all, plan, key)?;
    if !before.is_draft() {
        return Err(CpdError::new(
            ErrorCode::JobLocked,
            format!("Job {key} is {}: only a draft Job can be edited.", before.status),
        )
        .with("plan", plan.as_str())
        .with("job", key.as_str())
        .with("status", before.status.as_str()));
    }
    let mut after = before.clone();
    if let Some(title) = &edits.title {
        after.title = title.clone();
    }
    if let Some(tier) = &edits.tier {
        after.tier = tier.clone();
    }
    if let Some(spec) = &edits.spec {
        after.goal = spec.goal.clone();
        after.context = spec.context.clone();
        after.touches = json!(spec.touches);
        after.acceptance = json!(spec.acceptance);
        after.out_of_scope = json!(spec.out_of_scope);
    }
    if let Some(pr) = &edits.pr {
        let prs = load_prs(conn, plan)?;
        if !prs.iter().any(|p| p.key.as_str() == pr.as_str()) {
            return Err(CpdError::new(ErrorCode::PrNotFound, format!("No PR {pr} in Plan {plan}."))
                .with("plan", plan.as_str())
                .with("pr", pr.as_str()));
        }
        check_pr_live(&prs, plan, pr)?;
        after.pr = pr.clone();
    }
    let moved = after.pr != before.pr;
    // A Job that changes PR keeps none of its old needs: they are checked against the new PR.
    if let Some(needs) = &edits.needs {
        check_needs_known(&all, plan, needs)?;
        check_needs_available(&all, plan, needs)?;
    }
    let needs = edits.needs.clone().unwrap_or_else(|| before.needs.clone());
    if edits.needs.is_some() || moved {
        check_needs_same_pr(&all, plan, &after.pr, &needs)?;
    }
    after.needs = needs;
    if moved {
        let needing: Vec<&str> = all.iter().filter(|j| j.needs.contains(&before.key)).map(|j| j.key.as_str()).collect();
        if !needing.is_empty() {
            return Err(CpdError::new(
                ErrorCode::NeedsOtherPr,
                format!("Job {key} cannot change PR: {} need{} it.", needing.join(", "), if needing.len() == 1 { "s" } else { "" }),
            )
            .with("plan", plan.as_str())
            .with("pr", after.pr.as_str())
            .with("jobs", needing));
        }
    }
    if edits.needs.is_some() {
        let others = all.iter().filter(|j| j.key != after.key).cloned().chain([after.clone()]).collect::<Vec<_>>();
        if let Some(cycle) = jobs::cycle(&others) {
            return Err(CpdError::new(
                ErrorCode::DependencyCycle,
                format!("The new needs of {key} make a cycle: {}.", cycle.join(" → ")),
            )
            .with("plan", plan.as_str())
            .with("cycle", cycle));
        }
    }
    let fields: Vec<&'static str> = [
        ("title", before.title != after.title),
        ("tier", before.tier != after.tier),
        ("goal", before.goal != after.goal),
        ("context", before.context != after.context),
        ("touches", before.touches != after.touches),
        ("acceptance", before.acceptance != after.acceptance),
        ("out_of_scope", before.out_of_scope != after.out_of_scope),
        ("pr", before.pr != after.pr),
        ("needs", before.needs != after.needs),
    ]
    .into_iter()
    .filter_map(|(name, changed)| changed.then_some(name))
    .collect();
    if fields.is_empty() {
        return Ok(Decision::Already(before.clone()));
    }
    Ok(Decision::Update { before: before.clone(), after, fields })
}

/// Writes the changed `jobs` columns, and deletes and inserts `dependencies`
/// rows to match (§4.4 allows deletion only for `draft`): the old
/// Dependencies first, then the columns (`pr` among them), then the new ones.
fn update(tx: &Connection, plan: &PlanId, before: &Job, after: &Job) -> Result<(), CpdError> {
    for need in before.needs.iter().filter(|n| !after.needs.contains(n)) {
        tx.execute("DELETE FROM dependencies WHERE plan = ?1 AND job = ?2 AND needs = ?3", [plan.as_str(), after.key.as_str(), need.as_str()])?;
    }
    let columns: [(&str, bool, String); 8] = [
        ("title", before.title != after.title, after.title.clone()),
        ("tier", before.tier != after.tier, after.tier.clone()),
        ("goal", before.goal != after.goal, after.goal.clone()),
        ("context", before.context != after.context, after.context.clone()),
        ("touches", before.touches != after.touches, after.touches.to_string()),
        ("acceptance", before.acceptance != after.acceptance, after.acceptance.to_string()),
        ("out_of_scope", before.out_of_scope != after.out_of_scope, after.out_of_scope.to_string()),
        ("pr", before.pr != after.pr, after.pr.clone()),
    ];
    let changed: Vec<&(&str, bool, String)> = columns.iter().filter(|(_, changed, _)| *changed).collect();
    if !changed.is_empty() {
        let set = changed.iter().enumerate().map(|(i, (name, _, _))| format!("{name} = ?{}", i + 3)).collect::<Vec<_>>().join(", ");
        let plan_s = plan.as_str();
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&plan_s, &after.key];
        params.extend(changed.iter().map(|(_, _, v)| v as &dyn rusqlite::ToSql));
        tx.execute(&format!("UPDATE jobs SET {set} WHERE plan = ?1 AND key = ?2"), params.as_slice())?;
    }
    for need in after.needs.iter().filter(|n| !before.needs.contains(n)) {
        tx.execute("INSERT INTO dependencies (plan, job, needs) VALUES (?1, ?2, ?3)", [plan.as_str(), after.key.as_str(), need.as_str()])?;
    }
    Ok(())
}
