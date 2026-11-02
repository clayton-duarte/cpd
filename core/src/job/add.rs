//! `job add` (commands.md §5.1): `[*] → draft` in phase `plan`; in a phase
//! after it, an appended `queued` Job, fingerprint stored at once, or with
//! `--draft` an appended `draft`, which only an approved Plan review queues.
//! `--tier` and `--spec` are optional with `--draft` only: a stub draft (R1).

use rusqlite::Connection;
use serde_json::{Value, json};

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::spec::{self, Spec};
use super::{
    AddArgs, Env, check_impl_job_required, check_kind_not_reserved, check_needs_available,
    check_needs_known, check_needs_same_workflow, check_shape_allows, check_test_job_required,
    check_workflow_live, plan_phase, sorted_needs,
};
use crate::args::WorkflowArg;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::home::CpdHome;
use crate::ids::{JobKey, PlanId, WorkflowKey};
use crate::output::Output;
use crate::plan::WorkflowRow;
use crate::plan::fingerprint::load_workflows;
use crate::plan::jobs::{self, Job};
use crate::plan::locked_fingerprint;

const COMMAND: &str = "job add";

/// What `job add` was asked for, parsed.
struct Wanted {
    key: JobKey,
    workflow: WorkflowArg,
    title: String,
    /// Given always, but for a stub draft.
    tier: Option<String>,
    spec: Option<Spec>,
    needs: Vec<String>,
    /// `--draft`: a draft in a phase after `plan` too.
    draft: bool,
    /// `--urgent` (§2.2): not a Locked field, so a rerun without it is `already`.
    urgent: bool,
    /// `--kind` (§2.2 "Job kinds"), `work` by default.
    kind: String,
}

enum Decision {
    /// The Job exists with these Locked fields and needs.
    Already(Job),
    Insert(Job),
}

pub(super) fn add(args: AddArgs) -> Result<Output, CpdError> {
    add_in(&mut Env::open()?, args)
}

/// [`add`] on an open `env`: what a test hands in.
pub(super) fn add_in(env: &mut Env, args: AddArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let needs = sorted_needs(&args.needs)?;
    let kind = args.kind.clone().unwrap_or_else(|| "work".to_owned());
    check_kind_not_reserved(&kind)?;
    let explore = jobs::is_explore(&kind);
    if explore {
        // An explore Job belongs to the Plan, and is never a draft (R2, R3).
        if let Some(workflow) = &args.workflow.workflow {
            return Err(CpdError::new(ErrorCode::ExploreWorkflowForbidden, format!("A {kind} Job belongs to the Plan, not to a Workflow: drop --workflow {workflow}."))
                .with("kind", kind.as_str())
                .with("pr", workflow.as_str()));
        }
        if args.draft {
            return Err(CpdError::new(ErrorCode::ExploreNoDraft, format!("A {kind} Job has no draft, no review and no lock: it is queued at once; drop --draft."))
                .with("kind", kind.as_str()));
        }
    }
    require_tier_and_spec(&args)?;
    let mut spec = args
        .spec
        .as_deref()
        .map(|p| spec::read_for(p, &kind))
        .transpose()?;
    if let Some(spec) = &mut spec {
        spec.check_kind(&kind)?;
        if explore {
            spec.touches = vec![spec::explore_artefact(plan.as_str(), args.key.as_str())];
        }
    }
    if let (true, Some(tier)) = (explore, args.tier) {
        env.check_explore_tier(&plan, &kind, tier)?;
    } else if let Some(tier) = args.tier {
        let workflows = env.store.read(|conn| {
            super::plan_phase(conn, &plan)?;
            load_workflows(conn, &plan)
        })?;
        let workflow = args.workflow.select(&plan, &workflows)?.key.clone();
        env.check_tier(&plan, &args.key, Some(&workflow), &kind, tier)?;
    }
    let wanted = Wanted {
        key: args.key,
        workflow: args.workflow,
        title: args.title.as_str().to_owned(),
        tier: args.tier.map(|t| t.to_string()),
        spec,
        needs,
        draft: args.draft,
        urgent: args.urgent,
        kind,
    };

    let home = env.home.clone();
    let decision = env.store.read(|conn| decide(conn, &home, &plan, &wanted))?;
    let (job, already) = match decision {
        Decision::Already(job) => (job, true),
        Decision::Insert(_) => env.store.write(COMMAND, |tx| {
            match decide(tx, &home, &plan, &wanted)? {
                Decision::Already(job) => Ok((job, true)),
                Decision::Insert(job) => {
                    insert(tx, &plan, &job)?;
                    let text = format!(
                        "Job {} added to Plan {plan} as {}. Next: cpd job show {} --plan {plan}",
                        job.key, job.status, job.key
                    );
                    append(
                        tx,
                        NewEvent::job_added(
                            text,
                            &job.workflow,
                            &job.kind,
                            &job.status,
                            job.appended,
                            job.tier.as_deref(),
                            &job.needs,
                            job.urgent,
                        )
                        .plan(plan.clone())
                        .job(wanted.key.clone()),
                    )?;
                    if let Some(workflow) = &job.workflow {
                        crate::session::working_pr::name(
                            tx,
                            &plan,
                            &crate::ids::WorkflowKey::parse(workflow)?,
                        )?;
                    }
                    Ok((job, false))
                }
            }
        })?,
    };
    let mut out = json!({"job": {"key": job.key, "pr": job.workflow, "kind": job.kind, "status": job.status, "appended": job.appended, "tier": job.tier, "needs": job.needs, "urgent": job.urgent}});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// `--tier` and `--spec` are required, except with `--draft` (a stub draft, §5.1).
fn require_tier_and_spec(args: &AddArgs) -> Result<(), CpdError> {
    let missing: Vec<&str> = [
        ("--tier", args.tier.is_none()),
        ("--spec", args.spec.is_none()),
    ]
    .into_iter()
    .filter_map(|(flag, lacks)| lacks.then_some(flag))
    .collect();
    if missing.is_empty() || args.draft {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::Usage,
        format!(
            "{} required for a Job that is not a --draft: only a stub draft (--draft) may leave them out.",
            missing.join(" and ")
        ),
    ))
}

fn decide(
    conn: &Connection,
    home: &CpdHome,
    plan: &PlanId,
    wanted: &Wanted,
) -> Result<Decision, CpdError> {
    let phase = plan_phase(conn, plan)?;
    if jobs::is_explore(&wanted.kind) {
        return decide_explore(conn, plan, wanted, &phase);
    }
    if phase == "grounding" || (phase == "done" && !wanted.draft) {
        return Err(CpdError::new(
            ErrorCode::IllegalPhase,
            format!("Plan {plan} is in phase {phase}: Jobs are added from phase plan on, and in phase done only as drafts (--draft)."),
        )
        .with("plan", plan.as_str())
        .with("phase", phase));
    }
    let workflows = load_workflows(conn, plan)?;
    let workflow = wanted.workflow.select(plan, &workflows)?.key.to_string();
    check_workflow_live(&workflows, plan, &workflow)?;
    check_shape_allows(&workflows, plan, &workflow, &wanted.kind)?;
    let all = jobs::load(conn, plan)?;
    let workflow_status = &workflows
        .iter()
        .find(|p| p.key.as_str() == workflow)
        .expect("check_pr_live found the Workflow")
        .status;
    let mut needs = wanted.needs.clone();
    needs.extend(
        super::chain::leaves(&all, plan, &workflow, workflow_status, wanted.key.as_str())?
            .unwrap_or_default(),
    );
    needs.sort();
    needs.dedup();
    let appended = phase != "plan";
    // In phase `plan` every Job is a draft; after it, only with `--draft`.
    let draft = !appended || wanted.draft;
    let mut job = Job {
        seq: 0,
        key: wanted.key.as_str().to_owned(),
        workflow: Some(workflow),
        title: wanted.title.clone(),
        tier: wanted.tier.clone(),
        spec: wanted.spec.as_ref().map(Spec::stored),
        status: if draft { "draft" } else { "queued" }.to_owned(),
        appended,
        kind: wanted.kind.clone(),
        tests: wanted
            .spec
            .as_ref()
            .and_then(|s| s.tests.as_ref())
            .map(|t| json!(t)),
        test_files: wanted
            .spec
            .as_ref()
            .and_then(|s| s.test_files.as_ref())
            .map(|t| json!(t)),
        resources: wanted
            .spec
            .as_ref()
            .and_then(|s| s.resources.as_ref())
            .map(|r| json!(r)),
        test_approved: None,
        touches_approved: None,
        urgent: wanted.urgent,
        fingerprint: None,
        retry_tier: None,
        cancel_reason: None,
        cancel_requested_at: None,
        failed_cause: None,
        merged_sha: None,
        summary: None,
        created_at: String::new(),
        needs,
    };
    if let Some(existing) = all.iter().find(|j| j.key == job.key) {
        if same_locked_fields(existing, &job) {
            return Ok(Decision::Already(existing.clone()));
        }
        return Err(CpdError::new(
            ErrorCode::JobKeyExists,
            format!(
                "Plan {plan} already has a Job {} with other Locked fields.",
                job.key
            ),
        )
        .with("plan", plan.as_str())
        .with("job", job.key.as_str()));
    }
    // The Spec names no primary checkout of a Workflow of the Plan (commands.md §5.1 "A Spec names no primary checkout", §5.2).
    if let Some(spec) = &wanted.spec {
        let user_home = std::env::home_dir().unwrap_or_default();
        spec.without_artefacts(home.root(), plan.as_str(), &user_home)
            .check_primary_roots(&primary_roots(&workflows), &user_home)?;
        spec.check_files_relative()?;
        check_resources_declared(
            conn,
            home,
            plan,
            job.workflow_str(),
            &job.key,
            spec.resources.as_deref(),
        )?;
    }
    if let Some(spec) = &wanted.spec {
        let workflow = workflows
            .iter()
            .find(|p| p.key.as_str() == job.workflow_str())
            .expect("check_pr_live found the Workflow");
        check_touches(workflow, &workflows, &spec.touches)?;
    }
    check_needs_known(&all, plan, &job.needs)?;
    check_needs_explore_only(&all, plan, &job.key, false, &job.needs)?;
    check_needs_same_workflow(&all, plan, job.workflow_str(), &job.needs)?;
    if !draft {
        check_impl_job_required(&all, plan, &job)?;
    }
    // Only the chain may need the merged open Job: what was named by hand is checked alone.
    check_needs_available(&all, plan, &wanted.needs)?;
    if !draft {
        check_test_job_required(&all, plan, &job)?;
        check_needs_not_draft(&all, plan, &job)?;
        job.fingerprint = Some(locked_fingerprint(&job));
    }
    if draft {
        crate::plan::parked::door(conn, plan, &phase, &all, &job)?;
    }
    Ok(Decision::Insert(job))
}

/// An explore Job (R2-R5): of the Plan, no Workflow, queued at once with its fingerprint, in every phase but `done`.
fn decide_explore(
    conn: &Connection,
    plan: &PlanId,
    wanted: &Wanted,
    phase: &str,
) -> Result<Decision, CpdError> {
    if phase == "done" {
        return Err(CpdError::new(
            ErrorCode::IllegalPhase,
            format!("Plan {plan} is in phase done: explore Jobs ({}) are added in the phases grounding, plan, dispatch and deploy.", wanted.kind),
        )
        .with("plan", plan.as_str())
        .with("phase", phase));
    }
    let all = jobs::load(conn, plan)?;
    if wanted.needs.iter().any(|n| n == wanted.key.as_str()) {
        return Err(CpdError::new(
            ErrorCode::DependencyCycle,
            format!("Job {} needs itself.", wanted.key),
        )
        .with("plan", plan.as_str())
        .with("cycle", [wanted.key.as_str(), wanted.key.as_str()]));
    }
    check_needs_known(&all, plan, &wanted.needs)?;
    check_needs_explore_only(&all, plan, wanted.key.as_str(), true, &wanted.needs)?;
    let mut job = Job {
        seq: 0,
        key: wanted.key.as_str().to_owned(),
        workflow: None,
        title: wanted.title.clone(),
        tier: wanted.tier.clone(),
        spec: wanted.spec.as_ref().map(Spec::stored),
        status: "queued".to_owned(),
        appended: true,
        kind: wanted.kind.clone(),
        tests: None,
        test_files: None,
        resources: None,
        test_approved: None,
        touches_approved: None,
        urgent: wanted.urgent,
        fingerprint: None,
        retry_tier: None,
        cancel_reason: None,
        cancel_requested_at: None,
        failed_cause: None,
        merged_sha: None,
        summary: None,
        created_at: String::new(),
        needs: wanted.needs.clone(),
    };
    if let Some(existing) = all.iter().find(|j| j.key == job.key) {
        if same_locked_fields(existing, &job) {
            return Ok(Decision::Already(existing.clone()));
        }
        return Err(CpdError::new(
            ErrorCode::JobKeyExists,
            format!(
                "Plan {plan} already has a Job {} with other Locked fields.",
                job.key
            ),
        )
        .with("plan", plan.as_str())
        .with("job", job.key.as_str()));
    }
    check_needs_available(&all, plan, &wanted.needs)?;
    check_needs_not_draft(&all, plan, &job)?;
    job.fingerprint = Some(locked_fingerprint(&job));
    Ok(Decision::Insert(job))
}

/// An explore Job needs only explore Jobs, and no other Job needs one (`explore_needs_mixed`, exit 3, `details.jobs`
/// the Jobs on the other side). `explore` is whether the Job `key` is one.
pub(super) fn check_needs_explore_only(
    all: &[Job],
    plan: &PlanId,
    key: &str,
    explore: bool,
    needs: &[String],
) -> Result<(), CpdError> {
    let others: Vec<&str> = all
        .iter()
        .filter(|j| needs.contains(&j.key) && jobs::is_explore(&j.kind) != explore)
        .map(|j| j.key.as_str())
        .collect();
    if others.is_empty() {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::ExploreNeedsMixed,
        format!("Job {key} needs {}: an explore Job needs only explore Jobs, and no other Job needs one.", others.join(", ")),
    )
    .with("plan", plan.as_str())
    .with("job", key)
    .with("jobs", others))
}

/// Every resource of a Spec is declared by the policy of the Workflow's repo (`resource_undeclared`, commands.md §5.1):
/// the first one that is not fails, with the names the repo declares and the file that declares them.
pub(crate) fn check_resources_declared(
    conn: &Connection,
    home: &CpdHome,
    plan: &PlanId,
    workflow: &str,
    job: &str,
    resources: Option<&[String]>,
) -> Result<(), CpdError> {
    let Some(resources) = resources else {
        return Ok(());
    };
    let key = WorkflowKey::parse(workflow)?;
    let declared = crate::pr::policy_of_workflow(conn, home, plan, &key)?
        .resources
        .names;
    let Some(missing) = resources.iter().find(|r| !declared.contains(r)) else {
        return Ok(());
    };
    let (owner, name): (String, Option<String>) = conn.query_row(
        "SELECT c.owner, r.name FROM workflows r JOIN plans p ON p.id = r.plan JOIN contexts c ON c.id = p.context WHERE r.plan = ?1 AND r.key = ?2",
        [plan.as_str(), workflow],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let owner = crate::ids::OwnerLogin::parse(&owner)?;
    let file = match name {
        Some(name) => crate::policy::repo_path(home, &owner, &crate::ids::RepoName::parse(&name)?),
        None => crate::policy::policy_dir(home)
            .join(owner.as_str())
            .join("owner.toml"),
    };
    Err(CpdError::new(
        ErrorCode::ResourceUndeclared,
        format!(
            "Job {job} takes the resource {missing}, which the policy of Workflow {workflow} does not declare ({}). Declare it as [resources.{missing}] in {}, or take one of the declared names.",
            if declared.is_empty() { "none are declared".to_owned() } else { declared.join(", ") },
            file.display()
        ),
    )
    .with("job", job)
    .with("pr", workflow)
    .with("resource", missing.as_str())
    .with("declared", declared.clone())
    .with("file", file.display().to_string()))
}

/// A `queued` Job may not need a `draft` (§5.1): it could start before its need
/// exists. `details.jobs` lists the drafts.
fn check_needs_not_draft(all: &[Job], plan: &PlanId, job: &Job) -> Result<(), CpdError> {
    let drafts: Vec<&str> = all
        .iter()
        .filter(|j| j.is_draft() && job.needs.contains(&j.key))
        .map(|j| j.key.as_str())
        .collect();
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

/// The Locked fields (§2.2: the Spec, `title`, `tier`, `pr`, `kind`, `tests`, `test_files`, `resources`) and the needs.
fn same_locked_fields(a: &Job, b: &Job) -> bool {
    (
        &a.workflow,
        &a.title,
        &a.tier,
        &a.spec,
        &a.needs,
        &a.kind,
        &a.tests,
        &a.test_files,
        &a.resources,
    ) == (
        &b.workflow,
        &b.title,
        &b.tier,
        &b.spec,
        &b.needs,
        &b.kind,
        &b.tests,
        &b.test_files,
        &b.resources,
    )
}

fn insert(tx: &Connection, plan: &PlanId, job: &Job) -> Result<(), CpdError> {
    let array = |v: Option<&Value>| v.map(Value::to_string);
    let spec = job.spec.as_ref();
    tx.execute(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                           status, appended, fingerprint, urgent, kind, tests, test_files, resources, created_txn, created_at)
         VALUES (?1, ?13, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?14, ?15, ?16, ?17, ?18, cpd_txn(),
                 (SELECT at FROM txns WHERE id = cpd_txn()))",
        rusqlite::params![
            plan.as_str(),
            job.key,
            job.title,
            job.tier,
            spec.map(|s| &s.goal),
            spec.map(|s| &s.context),
            array(spec.map(|s| &s.touches)),
            array(spec.map(|s| &s.acceptance)),
            array(spec.map(|s| &s.out_of_scope)),
            job.status,
            job.appended,
            job.fingerprint,
            job.workflow,
            job.urgent,
            job.kind,
            array(job.tests.as_ref()),
            array(job.test_files.as_ref()),
            array(job.resources.as_ref()),
        ],
    )?;
    for need in &job.needs {
        tx.execute(
            "INSERT INTO dependencies (plan, job, needs) VALUES (?1, ?2, ?3)",
            [plan.as_str(), job.key.as_str(), need.as_str()],
        )?;
    }
    Ok(())
}

/// The `(Workflow key, root)` pairs the primary-check searches: each Workflow of the Plan and its stored repo path.
pub(super) fn primary_roots(workflows: &[WorkflowRow]) -> Vec<(String, PathBuf)> {
    workflows
        .iter()
        .map(|p| (p.key.as_str().to_owned(), PathBuf::from(&p.repo)))
        .collect()
}

/// Every `touches` entry against the tree the Job will start from (contract §2.1, hs-d512): the Workflow's Lead Branch head once
/// the Workflow started, else its base. A literal path must exist, or its parent directory must (a new file); a glob must match a
/// file, or its fixed prefix directory must exist. Nothing is read, and the check passes, for a Workflow that has not started and
/// is stacked on a base Workflow that is `waiting` or `dispatch` (its Lead Branch is not in the repo yet). `touches_missing`, `details: {missing, tree}`.
pub(super) fn check_touches(
    workflow: &WorkflowRow,
    workflows: &[WorkflowRow],
    touches: &[String],
) -> Result<(), CpdError> {
    // A Workflow whose repo path is not on this machine has no tree to check against (a Plan written elsewhere, a test's placeholder).
    if touches.is_empty() || !std::path::Path::new(&workflow.repo).is_dir() {
        return Ok(());
    }
    // A Workflow that has not started, stacked on a base Workflow (same Plan and repo, Lead Branch == its base) that is `waiting` or
    // `dispatch`, has no tree to read yet: the base's Lead Branch does not exist.
    let base_unopened = workflow.lead_worktree.is_none()
        && workflows.iter().any(|q| {
            q.repo == workflow.repo
                && q.lead_branch == workflow.base
                && matches!(q.status.as_str(), "waiting" | "dispatch")
        });
    if base_unopened {
        return Ok(());
    }
    let tree = if workflow.lead_worktree.is_some() {
        workflow.lead_branch.as_str()
    } else {
        workflow.base.as_str()
    };
    let listing = crate::git::run::ok(
        std::path::Path::new(&workflow.repo),
        &["ls-tree", "-r", "--name-only", "-z", tree],
    )?;
    let files: Vec<&str> = listing.split('\0').filter(|f| !f.is_empty()).collect();
    let dirs: BTreeSet<&str> = files
        .iter()
        .flat_map(|f| f.match_indices('/').map(move |(i, _)| &f[..i]))
        .collect();
    let dir_exists = |dir: &str| dir.is_empty() || dirs.contains(dir);
    let missing: Vec<&String> = touches
        .iter()
        // `run/<plan>/reviews/<key>.md` is cpd's run directory, not a path of the repo (contract §1.4).
        .filter(|entry| {
            let entry = entry.as_str();
            if entry.starts_with("run/") {
                return false;
            }
            if entry.contains(['*', '?', '[']) {
                let fixed = entry.split(['*', '?', '[']).next().unwrap_or("");
                let prefix = fixed.rsplit_once('/').map_or("", |(dir, _)| dir);
                !(files.iter().any(|f| glob_matches(entry, f)) || dir_exists(prefix))
            } else {
                let entry = entry.trim_end_matches('/');
                let parent = entry.rsplit_once('/').map_or("", |(dir, _)| dir);
                !(files.contains(&entry) || dirs.contains(entry) || dir_exists(parent))
            }
        })
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::TouchesMissing,
        format!(
            "The Spec's touches name paths that are not in {tree}: {}.",
            missing
                .iter()
                .map(|m| m.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )
    .with(
        "missing",
        missing.iter().map(|m| m.as_str()).collect::<Vec<_>>(),
    )
    .with("tree", tree))
}

/// Whether `path` matches `pattern`: `?` is one character but `/`, `*` any run but `/`, `**` any run, `[...]` one of a set.
pub(crate) fn glob_matches(pattern: &str, path: &str) -> bool {
    fn go(p: &[char], s: &[char]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some('*') if p.get(1) == Some(&'*') => {
                let rest = &p[2..];
                let rest = rest.strip_prefix(&['/']).unwrap_or(rest);
                (0..=s.len()).any(|i| go(rest, &s[i..]))
            }
            Some('*') => (0..=s.len())
                .take_while(|&i| i == 0 || s[i - 1] != '/')
                .any(|i| go(&p[1..], &s[i..])),
            Some('?') => s.first().is_some_and(|c| *c != '/') && go(&p[1..], &s[1..]),
            Some('[') => match p.iter().position(|c| *c == ']') {
                Some(end) if end > 1 => {
                    s.first().is_some_and(|c| p[1..end].contains(c)) && go(&p[end + 1..], &s[1..])
                }
                _ => s.first() == Some(&'[') && go(&p[1..], &s[1..]),
            },
            Some(c) => s.first() == Some(c) && go(&p[1..], &s[1..]),
        }
    }
    go(
        &pattern.chars().collect::<Vec<_>>(),
        &path.chars().collect::<Vec<_>>(),
    )
}
