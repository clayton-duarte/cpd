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
    AddArgs, Env, check_kind_not_reserved, check_needs_available, check_needs_known, check_needs_same_pr, check_pr_live, check_shape_allows,
    check_impl_job_required, check_test_job_required, plan_phase, sorted_needs,
};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::output::Output;
use crate::args::PrArg;
use crate::plan::fingerprint::load_prs;
use crate::plan::jobs::{self, Job};
use crate::plan::locked_fingerprint;
use crate::plan::PrRow;

const COMMAND: &str = "job add";

/// What `job add` was asked for, parsed.
struct Wanted {
    key: JobKey,
    pr: PrArg,
    title: String,
    /// Given always, but for a stub draft.
    tier: Option<String>,
    spec: Option<Spec>,
    needs: Vec<String>,
    /// `--review` (B2): the needs are set from the Plan, not given.
    review: bool,
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
    let plan = args.plan.require()?.clone();
    let needs = sorted_needs(&args.needs)?;
    let kind = args.kind.clone().unwrap_or_else(|| "work".to_owned());
    check_kind_not_reserved(&kind)?;
    if args.review && kind != "work" {
        return Err(CpdError::new(ErrorCode::Usage, format!("--review is a work Job: it cannot be --kind {kind}.")).with("kind", kind));
    }
    require_tier_and_spec(&args)?;
    let spec = args.spec.as_deref().map(spec::read).transpose()?;
    if let Some(spec) = &spec {
        spec.check_kind(&kind)?;
    }
    let mut env = Env::open()?;
    if let Some(tier) = args.tier {
        let prs = env.store.read(|conn| {
            super::plan_phase(conn, &plan)?;
            load_prs(conn, &plan)
        })?;
        let pr = args.pr.select(&plan, &prs)?.key.clone();
        env.check_tier(&plan, &args.key, Some(&pr), tier)?;
    }
    let wanted = Wanted {
        key: args.key,
        pr: args.pr,
        title: args.title.as_str().to_owned(),
        tier: args.tier.map(|t| t.to_string()),
        spec,
        needs,
        review: args.review,
        draft: args.draft,
        urgent: args.urgent,
        kind,
    };

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
                    NewEvent::job_added(
                        text,
                        &job.pr,
                        &job.kind,
                        &job.status,
                        job.appended,
                        job.tier.as_deref(),
                        &job.needs,
                        wanted.review,
                        job.urgent,
                    )
                    .plan(plan.clone())
                    .job(wanted.key.clone()),
                )?;
                Ok((job, false))
            }
        })?,
    };
    let mut out = json!({"job": {"key": job.key, "pr": job.pr, "kind": job.kind, "status": job.status, "appended": job.appended, "tier": job.tier, "needs": job.needs, "review": wanted.review, "urgent": job.urgent}});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// `--tier` and `--spec` are required, except with `--draft` (a stub draft, §5.1);
/// a review Job is never a stub.
fn require_tier_and_spec(args: &AddArgs) -> Result<(), CpdError> {
    let missing: Vec<&str> = [("--tier", args.tier.is_none()), ("--spec", args.spec.is_none())]
        .into_iter()
        .filter_map(|(flag, lacks)| lacks.then_some(flag))
        .collect();
    if missing.is_empty() || (args.draft && !args.review) {
        return Ok(());
    }
    let why = if args.review { "a --review Job" } else { "a Job that is not a --draft" };
    Err(CpdError::new(ErrorCode::Usage, format!("{} required for {why}: only a stub draft (--draft) may leave them out.", missing.join(" and "))))
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
    let pr = wanted.pr.select(plan, &prs)?.key.to_string();
    check_pr_live(&prs, plan, &pr)?;
    check_shape_allows(&prs, plan, &pr, &wanted.kind, wanted.review)?;
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
        spec: wanted.spec.as_ref().map(Spec::stored),
        status: if draft { "draft" } else { "queued" }.to_owned(),
        appended,
        review: wanted.review,
        kind: wanted.kind.clone(),
        tests: wanted.spec.as_ref().and_then(|s| s.tests.as_ref()).map(|t| json!(t)),
        test_files: wanted.spec.as_ref().and_then(|s| s.test_files.as_ref()).map(|t| json!(t)),
        test_approved: None,
        urgent: wanted.urgent,
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
    // The Spec names no primary checkout of a PR of the Plan (commands.md §5.1 "A Spec names no primary checkout", §5.2).
    if let Some(spec) = &wanted.spec {
        spec.check_primary_roots(&primary_roots(&prs), &std::env::home_dir().unwrap_or_default())?;
        spec.check_files_relative()?;
    }
    // A review Job's `touches` is informational (commands.md §5.1).
    if let (Some(spec), false) = (&wanted.spec, wanted.review) {
        let pr = prs.iter().find(|p| p.key.as_str() == job.pr).expect("check_pr_live found the PR");
        check_touches(pr, &spec.touches)?;
    }
    check_needs_known(&all, plan, &job.needs)?;
    check_needs_same_pr(&all, plan, &job.pr, &job.needs)?;
    if !draft {
        check_impl_job_required(&all, plan, &job)?;
    }
    check_needs_available(&all, plan, &job.needs)?;
    if !draft {
        check_test_job_required(&all, plan, &job)?;
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
/// not `cancelled` nor an `open` or `update` Job (nothing waits for them), itself excepted (a rerun of the same add). A `failed` Job
/// refuses with `needs_unavailable` and `details.jobs`: retry or cancel it first.
fn review_needs(all: &[Job], plan: &PlanId, pr: &str, key: &str) -> Result<Vec<String>, CpdError> {
    let of_pr = || all.iter().filter(|j| j.pr == pr && j.key != key && j.status != "cancelled" && !matches!(j.kind.as_str(), "open" | "update"));
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

/// The Locked fields (§2.2: the Spec, `title`, `tier`, `pr`, `kind`, `tests`, `test_files`) and the needs; `review` is
/// compared by the caller.
fn same_locked_fields(a: &Job, b: &Job) -> bool {
    (&a.pr, &a.title, &a.tier, &a.spec, &a.needs, &a.kind, &a.tests, &a.test_files)
        == (&b.pr, &b.title, &b.tier, &b.spec, &b.needs, &b.kind, &b.tests, &b.test_files)
}

fn insert(tx: &Connection, plan: &PlanId, job: &Job, review: bool) -> Result<(), CpdError> {
    let array = |v: Option<&Value>| v.map(Value::to_string);
    let spec = job.spec.as_ref();
    tx.execute(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                           status, appended, fingerprint, review, urgent, kind, tests, test_files, created_txn, created_at)
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
            job.pr,
            review,
            job.urgent,
            job.kind,
            array(job.tests.as_ref()),
            array(job.test_files.as_ref()),
        ],
    )?;
    for need in &job.needs {
        tx.execute("INSERT INTO dependencies (plan, job, needs) VALUES (?1, ?2, ?3)", [plan.as_str(), job.key.as_str(), need.as_str()])?;
    }
    Ok(())
}

/// The `(PR key, root)` pairs the primary-check searches: each PR of the Plan and its stored repo path.
pub(super) fn primary_roots(prs: &[PrRow]) -> Vec<(String, PathBuf)> {
    prs.iter().map(|p| (p.key.as_str().to_owned(), PathBuf::from(&p.repo))).collect()
}

/// Every `touches` entry against the tree the Job will start from (contract §2.1, hs-d512): the PR's Lead Branch head once
/// the PR started, else its base. A literal path must exist, or its parent directory must (a new file); a glob must match a
/// file, or its fixed prefix directory must exist. `touches_missing`, `details: {missing, tree}`.
pub(super) fn check_touches(pr: &PrRow, touches: &[String]) -> Result<(), CpdError> {
    // A PR whose repo path is not on this machine has no tree to check against (a Plan written elsewhere, a test's placeholder).
    if touches.is_empty() || !std::path::Path::new(&pr.repo).is_dir() {
        return Ok(());
    }
    let tree = if pr.lead_worktree.is_some() { pr.lead_branch.as_str() } else { pr.base.as_str() };
    let listing = crate::git::run::ok(std::path::Path::new(&pr.repo), &["ls-tree", "-r", "--name-only", "-z", tree])?;
    let files: Vec<&str> = listing.split('\0').filter(|f| !f.is_empty()).collect();
    let dirs: BTreeSet<&str> = files.iter().flat_map(|f| f.match_indices('/').map(move |(i, _)| &f[..i])).collect();
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
        format!("The Spec's touches name paths that are not in {tree}: {}.", missing.iter().map(|m| m.as_str()).collect::<Vec<_>>().join(", ")),
    )
    .with("missing", missing.iter().map(|m| m.as_str()).collect::<Vec<_>>())
    .with("tree", tree))
}

/// Whether `path` matches `pattern`: `?` is one character but `/`, `*` any run but `/`, `**` any run, `[...]` one of a set.
fn glob_matches(pattern: &str, path: &str) -> bool {
    fn go(p: &[char], s: &[char]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some('*') if p.get(1) == Some(&'*') => {
                let rest = &p[2..];
                let rest = rest.strip_prefix(&['/']).unwrap_or(rest);
                (0..=s.len()).any(|i| go(rest, &s[i..]))
            }
            Some('*') => (0..=s.len()).take_while(|&i| i == 0 || s[i - 1] != '/').any(|i| go(&p[1..], &s[i..])),
            Some('?') => s.first().is_some_and(|c| *c != '/') && go(&p[1..], &s[1..]),
            Some('[') => match p.iter().position(|c| *c == ']') {
                Some(end) if end > 1 => s.first().is_some_and(|c| p[1..end].contains(c)) && go(&p[end + 1..], &s[1..]),
                _ => s.first() == Some(&'[') && go(&p[1..], &s[1..]),
            },
            Some(c) => s.first() == Some(c) && go(&p[1..], &s[1..]),
        }
    }
    go(&pattern.chars().collect::<Vec<_>>(), &path.chars().collect::<Vec<_>>())
}
