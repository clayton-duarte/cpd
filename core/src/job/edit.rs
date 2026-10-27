//! `job edit` (commands.md §5.2): the Locked fields and Dependencies of a
//! `draft` Job, and its PR; a Locked Job is refused with `job_locked` (§3.2).
//! It is how a stub draft gets its Tier and Spec (R1).

use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::json;

use super::spec::{self, Spec};
use super::{
    EditArgs, Env, check_kind_not_reserved, check_needs_available, check_needs_known, check_needs_same_pr, check_pr_live, check_shape_allows, find,
    plan_phase, sorted_needs,
};
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
    kind: Option<String>,
    /// `CPD_ATTEMPT` is set: a Worker's pane, not the Lead's.
    worker: bool,
}

enum Decision {
    /// No value changes.
    Already(Job),
    Update { before: Job, after: Job, fields: Vec<&'static str> },
}

pub(super) fn edit(args: EditArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    if args.title.is_none() && args.tier.is_none() && args.spec.is_none() && args.pr.is_none() && args.kind.is_none() && args.needs.is_none() && !args.no_needs {
        return Err(CpdError::new(
            ErrorCode::NothingToEdit,
            "Nothing to edit: pass --title, --tier, --spec, --pr, --kind, --needs or --no-needs.",
        ));
    }
    let needs = match (&args.needs, args.no_needs) {
        (Some(keys), _) => Some(sorted_needs(keys)?),
        (None, true) => Some(Vec::new()),
        (None, false) => None,
    };
    if let Some(kind) = &args.kind {
        check_kind_not_reserved(kind)?;
    }
    let spec = args.spec.as_ref().map(|path: &PathBuf| spec::read(path)).transpose()?;
    let mut env = Env::open()?;
    if let Some(tier) = args.tier {
        let pr = args.pr.clone();
        env.check_tier(&plan, &args.key, pr.as_ref(), tier)?;
    }
    let edits = Edits {
        title: args.title.map(|t| t.as_str().to_owned()),
        tier: args.tier.map(|t| t.to_string()),
        spec,
        pr: args.pr.map(|p| p.as_str().to_owned()),
        needs,
        kind: args.kind,
        worker: std::env::var("CPD_ATTEMPT").is_ok_and(|v| !v.is_empty()),
    };
    let key = args.key;

    let decision = env.store.read(|conn| decide(conn, &plan, &key, &edits))?;
    let (job, fields, already) = match decision {
        Decision::Already(job) => (job, Vec::new(), true),
        Decision::Update { .. } => env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, &edits)? {
            Decision::Already(job) => Ok((job, Vec::new(), true)),
            Decision::Update { before, after, fields } => {
                update(tx, &plan, &before, &after)?;
                let state = if before.is_draft() { "" } else { " (queued, Locked)" };
                let text = format!("Job {key} of Plan {plan}{state} edited ({}). Next: cpd plan render --plan {plan}", fields.join(", "));
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
    let locked = !before.is_draft();
    if locked && edits.worker {
        return Err(CpdError::new(ErrorCode::NotALead, "Only the Lead edits a Locked Job: ask it with cpd ask."));
    }
    if locked {
        let attempts: i64 = conn.query_row("SELECT count(*) FROM attempts WHERE plan = ?1 AND job = ?2", [plan.as_str(), key.as_str()], |r| r.get(0))?;
        let editable = before.status == "queued" && !matches!(before.kind.as_str(), "open" | "update") && attempts == 0;
        let spec_only = edits.title.is_none() && edits.tier.is_none() && edits.pr.is_none() && edits.kind.is_none() && edits.needs.is_none();
        if !editable || !spec_only {
            let why = if editable {
                "only its Spec (--spec) can change; title, Tier, PR, kind and Dependencies are Locked".to_owned()
            } else if matches!(before.kind.as_str(), "open" | "update") {
                format!("it is an {} Job: its Spec is cpd's, and cannot be edited", before.kind)
            } else {
                format!("it is {} with {attempts} Attempt(s): only a queued Job with no Attempt can be edited; retry it, or cancel it and append a replacement", before.status)
            };
            return Err(CpdError::new(ErrorCode::JobLocked, format!("Job {key} is Locked: {why}."))
                .with("plan", plan.as_str())
                .with("job", key.as_str())
                .with("status", before.status.as_str())
                .with("attempts", attempts));
        }
    }
    let mut after = before.clone();
    if let Some(title) = &edits.title {
        after.title = title.clone();
    }
    if let Some(tier) = &edits.tier {
        after.tier = Some(tier.clone());
    }
    if let Some(kind) = &edits.kind {
        after.kind = kind.clone();
    }
    if let Some(spec) = &edits.spec {
        spec.check_kind(&after.kind)?;
        // The Spec names no primary checkout of a PR of the Plan (commands.md §5.1 "A Spec names no primary checkout", §5.2).
        spec.check_primary_roots(&super::add::primary_roots(&load_prs(conn, plan)?), &std::env::home_dir().unwrap_or_default())?;
        spec.check_files_relative()?;
        after.spec = Some(spec.stored());
        after.tests = spec.tests.as_ref().map(|t| json!(t));
        after.test_files = spec.test_files.as_ref().map(|t| json!(t));
    }
    // The Spec fits the kind (§5.2): a `test` Job with a Spec has `tests` and `test_files`, every other kind neither.
    if edits.kind.is_some() && after.kind != before.kind {
        if after.spec.is_some() && (after.kind == "test") != after.tests.is_some() {
            let field = if after.kind == "test" { "tests" } else { "test_files" };
            let why = if after.kind == "test" { "a test Job's Spec names its tests" } else { "only a test Job's Spec has tests and test_files" };
            return Err(CpdError::new(ErrorCode::SpecInvalid, format!("The Spec is invalid at $.{field}: {why}; give --spec with the Spec that fits {}.", after.kind))
                .with("path", format!("$.{field}"))
                .with("job", key.as_str()));
        }
        if after.spec.is_none() {
            after.tests = None;
            after.test_files = None;
        }
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
    if let Some(spec) = &edits.spec {
        let prs = load_prs(conn, plan)?;
        if let Some(pr) = prs.iter().find(|p| p.key.as_str() == after.pr) {
            super::add::check_touches(pr, &spec.touches)?;
        }
    }
    if moved || after.kind != before.kind {
        check_shape_allows(&load_prs(conn, plan)?, plan, &after.pr, &after.kind, after.review)?;
    }
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
    let mut fields: Vec<&'static str> = Vec::new();
    if before.title != after.title {
        fields.push("title");
    }
    if before.kind != after.kind {
        fields.push("kind");
    }
    // A stub gets its whole Spec at once: it is one field, `spec`, not the five columns.
    let completed = before.spec.is_none() && after.spec.is_some();
    for (_, (name, _)) in columns(&before).into_iter().zip(columns(&after)).filter(|(b, a)| b.1 != a.1) {
        match name {
            "tier" => fields.push("tier"),
            _ if !completed => fields.push(name),
            _ if !fields.contains(&"spec") => fields.push("spec"),
            _ => {}
        }
    }
    if before.pr != after.pr {
        fields.push("pr");
    }
    if before.needs != after.needs {
        fields.push("needs");
    }
    if fields.is_empty() {
        return Ok(Decision::Already(before.clone()));
    }
    Ok(Decision::Update { before: before.clone(), after, fields })
}

/// The Tier and Spec columns (`tests` and `test_files` of a `test` Job with them) as stored, in column order: `None` is null (a stub).
fn columns(job: &Job) -> [(&'static str, Option<String>); 8] {
    let spec = job.spec.as_ref();
    [
        ("tier", job.tier.clone()),
        ("goal", spec.map(|s| s.goal.clone())),
        ("context", spec.map(|s| s.context.clone())),
        ("touches", spec.map(|s| s.touches.to_string())),
        ("acceptance", spec.map(|s| s.acceptance.to_string())),
        ("out_of_scope", spec.map(|s| s.out_of_scope.to_string())),
        ("tests", job.tests.as_ref().map(|t| t.to_string())),
        ("test_files", job.test_files.as_ref().map(|t| t.to_string())),
    ]
}

/// Writes the changed `jobs` columns, and deletes and inserts `dependencies`
/// rows to match (§4.4 allows deletion only for `draft`): the old
/// Dependencies first, then the columns (`pr` among them), then the new ones.
fn update(tx: &Connection, plan: &PlanId, before: &Job, after: &Job) -> Result<(), CpdError> {
    for need in before.needs.iter().filter(|n| !after.needs.contains(n)) {
        tx.execute("DELETE FROM dependencies WHERE plan = ?1 AND job = ?2 AND needs = ?3", [plan.as_str(), after.key.as_str(), need.as_str()])?;
    }
    let mut written: Vec<(&str, bool, Option<String>)> =
        vec![("title", before.title != after.title, Some(after.title.clone())), ("kind", before.kind != after.kind, Some(after.kind.clone()))];
    written.extend(columns(before).into_iter().zip(columns(after)).map(|(b, a)| (a.0, b.1 != a.1, a.1)));
    written.push(("pr", before.pr != after.pr, Some(after.pr.clone())));
    let changed: Vec<&(&str, bool, Option<String>)> = written.iter().filter(|(_, changed, _)| *changed).collect();
    let fingerprint = (!after.is_draft()).then(|| jobs::locked_fingerprint(after));
    if !changed.is_empty() {
        let mut changed = changed;
        let fp = (fingerprint.is_some()).then(|| ("fingerprint", true, fingerprint.clone()));
        let fp_ref = fp.as_ref();
        changed.extend(fp_ref);
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
