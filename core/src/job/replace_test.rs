//! Lead-only recovery of a locked test Job. The committed intent is the fence;
//! stopping and inspecting refs happen outside the graph transaction.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::plan::jobs::{self, Job};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Request {
    pub source: String,
    pub new_key: String,
    pub tests: Vec<String>,
    pub test_files: Vec<String>,
    pub reason: String,
    pub requester: String,
}

/// External actions are deliberately separate from the atomic graph update.
pub(super) trait Boundary {
    fn owner(
        &mut self,
        env: &mut Env,
        job: &Job,
        request: &Request,
        selector: &str,
    ) -> Result<String, CpdError>;
    fn stop(&mut self, env: &mut Env, plan: &PlanId, request: &Request) -> Result<(), CpdError>;
    /// Confirm teardown without stopping a process or changing an Attempt.
    fn quiescent(&mut self, env: &mut Env, plan: &PlanId, source: &str) -> Result<(), CpdError>;
    fn start(
        &mut self,
        env: &mut Env,
        plan: &PlanId,
        request: &Request,
    ) -> Result<String, CpdError>;
}

fn refused(text: impl Into<String>) -> CpdError {
    CpdError::new(ErrorCode::CasLost, text)
}

pub(crate) fn check_fence(conn: &Connection, plan: &PlanId, job: &str) -> Result<(), CpdError> {
    let pending: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM test_replacements WHERE plan = ?1 AND source = ?2 AND state = 'pending')",
        params![plan.as_str(), job], |r| r.get(0),
    )?;
    if pending {
        return Err(refused(format!(
            "Job {job} has a pending test replacement."
        )));
    }
    Ok(())
}

fn intent(conn: &Connection, plan: &PlanId, request: &Request) -> Result<Option<String>, CpdError> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT request, state FROM test_replacements WHERE plan = ?1 AND source = ?2",
            params![plan.as_str(), request.source],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((stored, state)) = row {
        let wanted = serde_json::to_string(request).expect("request serializes");
        if stored != wanted || state == "abandoned" {
            return Err(refused("A different replacement intent already exists."));
        }
        return Ok(Some(state));
    }
    Ok(None)
}

fn validate(conn: &Connection, plan: &PlanId, request: &Request) -> Result<Job, CpdError> {
    let lead: String = conn.query_row(
        "SELECT lead_session FROM plans WHERE id = ?1",
        [plan.as_str()],
        |r| r.get(0),
    )?;
    if lead != request.requester {
        return Err(CpdError::new(
            ErrorCode::NotALead,
            "Only the Plan's Lead can replace a test Job.",
        ));
    }
    JobKey::parse(&request.new_key)?;
    if request.reason.trim().is_empty()
        || request.tests.is_empty()
        || request.test_files.is_empty()
        || request
            .tests
            .iter()
            .any(|s| s.trim().is_empty() || s.contains('\n'))
        || request.test_files.iter().any(|s| {
            s.is_empty() || s.starts_with('/') || s.split('/').any(|p| p == ".." || p == ".")
        })
    {
        return Err(CpdError::new(
            ErrorCode::Usage,
            "Replacement selectors, test files and reason must be valid and nonempty.",
        ));
    }
    let all = jobs::load(conn, plan)?;
    let old = all
        .iter()
        .find(|j| j.key == request.source)
        .ok_or_else(|| refused("Source Job does not exist."))?;
    if old.kind != "test"
        || !matches!(old.status.as_str(), "queued" | "running" | "failed")
        || old.workflow.is_none()
    {
        return Err(refused("Source must be a locked, nonterminal test Job."));
    }
    let attempts: i64 = conn.query_row(
        "SELECT count(*) FROM attempts WHERE plan = ?1 AND job = ?2",
        params![plan.as_str(), request.source],
        |r| r.get(0),
    )?;
    if attempts == 0 || all.iter().any(|j| j.key == request.new_key) {
        return Err(refused(
            "Source needs an Attempt and replacement key must be unused.",
        ));
    }
    if old.cancel_requested_at.is_some() {
        return Err(refused("Source has a cancellation request."));
    }
    let workflow: (String, String) = conn.query_row(
        "SELECT shape, status FROM workflows WHERE plan = ?1 AND key = ?2",
        params![plan.as_str(), old.workflow_str()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if workflow.1 == "open"
        || (matches!(workflow.1.as_str(), "waiting" | "dispatch")
            && !super::kinds_allowed(&workflow.0).contains(&"test"))
    {
        return Err(CpdError::new(
            ErrorCode::ShapeViolation,
            "This Workflow's shape or status does not permit a replacement test Job.",
        ));
    }
    for need in &old.needs {
        let needed = all
            .iter()
            .find(|j| j.key == *need)
            .ok_or_else(|| refused("Source dependency no longer exists."))?;
        if needed.workflow != old.workflow
            || matches!(needed.status.as_str(), "cancelled" | "failed" | "draft")
            || matches!(needed.kind.as_str(), "open" | "update")
        {
            return Err(refused("Source has an unavailable dependency."));
        }
    }
    for dependent in all.iter().filter(|j| j.needs.contains(&request.source)) {
        if matches!(
            dependent.status.as_str(),
            "merged" | "cancelled" | "running"
        ) || dependent.workflow != old.workflow
        {
            return Err(refused(
                "A downstream Job is terminal, running, or in another Workflow.",
            ));
        }
        let live: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE plan = ?1 AND job = ?2 AND status <> 'ended')", params![plan.as_str(), dependent.key], |r| r.get(0))?;
        if live {
            return Err(refused("A downstream Attempt is live."));
        }
    }
    Ok(old.clone())
}

pub(super) fn replace(
    env: &mut Env,
    boundary: &mut dyn Boundary,
    plan: &PlanId,
    request: &Request,
) -> Result<(), CpdError> {
    let complete = env.store.read(|c| {
        let lead: Option<String> = c.query_row(
            "SELECT lead_session FROM plans WHERE id = ?1",
            [plan.as_str()],
            |r| r.get(0),
        )?;
        if lead.as_deref() != Some(request.requester.as_str()) {
            return Err(CpdError::new(
                ErrorCode::NotALead,
                "Only the Plan's Lead can replace a test Job.",
            ));
        }
        Ok(intent(c, plan, request)?.as_deref() == Some("complete"))
    })?;
    if complete {
        return Ok(());
    }
    let job = env.store.read(|c| validate(c, plan, request))?;
    validate_selectors(env, boundary, &job, request)?;
    if env.store.read(|c| intent(c, plan, request))?.is_none() {
        env.store.write("job replace-test intent", |tx| {
            if intent(tx, plan, request)?.is_none() {
                validate(tx, plan, request)?;
                tx.execute("INSERT INTO test_replacements (plan, source, new_key, request, state, created_txn) VALUES (?1, ?2, ?3, ?4, 'pending', cpd_txn())", params![plan.as_str(), request.source, request.new_key, serde_json::to_string(request).expect("request serializes")])?;
            }
            Ok(())
        })?;
    }
    boundary.stop(env, plan, request)?;
    let sha = boundary.start(env, plan, request)?;
    env.store.write("job replace-test", |tx| {
        if intent(tx, plan, request)?.as_deref() == Some("complete") { return Ok(()); }
        let old = validate(tx, plan, request)?;
        let live: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE plan = ?1 AND job = ?2 AND status <> 'ended')", params![plan.as_str(), request.source], |r| r.get(0))?;
        if live { return Err(refused("Source Attempt is still live.")); }
        let mut new = old.clone();
        new.key = request.new_key.clone();
        new.tests = Some(json!(request.tests));
        new.test_files = Some(json!(request.test_files));
        let fp = jobs::locked_fingerprint(&new);
        tx.execute("INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, tests, test_files, resources, created_txn, created_at) SELECT plan, workflow, ?3, title, tier, goal, context, touches, acceptance, out_of_scope, kind, 'queued', 1, ?4, ?5, ?6, resources, cpd_txn(), (SELECT at FROM txns WHERE id = cpd_txn()) FROM jobs WHERE plan = ?1 AND key = ?2", params![plan.as_str(), request.source, request.new_key, fp, json!(request.tests).to_string(), json!(request.test_files).to_string()])?;
        for need in &old.needs { tx.execute("INSERT INTO dependencies (plan, job, needs) VALUES (?1, ?2, ?3)", params![plan.as_str(), request.new_key, need])?; }
        let dependents: Vec<String> = { let mut s = tx.prepare("SELECT job FROM dependencies WHERE plan = ?1 AND needs = ?2 ORDER BY job")?; let rows = s.query_map(params![plan.as_str(), request.source], |r| r.get(0))?; rows.collect::<Result<_, _>>()? };
        for dependent in dependents {
            tx.execute("DELETE FROM dependencies WHERE plan = ?1 AND job = ?2 AND needs = ?3", params![plan.as_str(), dependent, request.source])?;
            tx.execute("INSERT INTO dependencies (plan, job, needs) VALUES (?1, ?2, ?3)", params![plan.as_str(), dependent, request.new_key])?;
        }
        // Redirecting is only valid while the complete graph still satisfies TDD.
        let graph = jobs::load(tx, plan)?;
        for dependent in graph.iter().filter(|j| j.needs.contains(&request.new_key)) {
            super::check_test_job_required(&graph, plan, dependent)?;
            super::check_impl_job_required(&graph, plan, dependent)?;
        }
        let reason = format!("Replaced by {}: {}", request.new_key, request.reason);
        tx.execute("UPDATE jobs SET status = 'cancelled', cancel_reason = ?3, cancel_requested_at = (SELECT at FROM txns WHERE id = cpd_txn()), failed_cause = NULL WHERE plan = ?1 AND key = ?2", params![plan.as_str(), request.source, reason])?;
        tx.execute("UPDATE test_replacements SET state = 'complete', start_sha = ?3 WHERE plan = ?1 AND source = ?2 AND state = 'pending'", params![plan.as_str(), request.source, sha])?;
        let text = format!("Test Job {} replaced by {} with selectors {}: {}", request.source, request.new_key, request.tests.join(", "), request.reason);
        append(tx, NewEvent::job_edited(text, ["replacement", "tests", "test_files"]).plan(plan.clone()).job(JobKey::parse(&request.source)?))?;
        Ok(())
    })
}

/// A revision or abandonment compares the entire pending request again in its
/// transaction. The fence prevents any new source Attempt across the external
/// liveness check and that transaction.
fn pending_for_lead(
    conn: &Connection,
    plan: &PlanId,
    source: &str,
    requester: &str,
) -> Result<String, CpdError> {
    let lead: Option<String> = conn.query_row(
        "SELECT lead_session FROM plans WHERE id = ?1",
        [plan.as_str()],
        |r| r.get(0),
    )?;
    if lead.as_deref() != Some(requester) {
        return Err(CpdError::new(
            ErrorCode::NotALead,
            "Only the Plan's Lead can recover a replacement.",
        ));
    }
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT request, state, new_key FROM test_replacements WHERE plan = ?1 AND source = ?2",
            params![plan.as_str(), source],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (stored, state, new_key) = row.ok_or_else(|| refused("No pending replacement intent."))?;
    if state != "pending" {
        return Err(refused("Replacement intent is not pending."));
    }
    let prior: Request = serde_json::from_str(&stored)
        .map_err(|e| CpdError::bad_row(format!("Invalid replacement intent: {e}")))?;
    if prior.requester != requester {
        return Err(CpdError::new(
            ErrorCode::NotALead,
            "Only the Lead who requested this intent can recover it.",
        ));
    }
    let unavailable: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM attempts WHERE plan = ?1 AND job = ?2 AND status <> 'ended') OR EXISTS(SELECT 1 FROM jobs WHERE plan = ?1 AND key = ?3)",
        params![plan.as_str(), source, new_key], |r| r.get(0),
    )?;
    if unavailable {
        return Err(refused(
            "Source is live or a replacement Job already exists.",
        ));
    }
    Ok(stored)
}

fn validate_selectors(
    env: &mut Env,
    boundary: &mut dyn Boundary,
    job: &Job,
    request: &Request,
) -> Result<(), CpdError> {
    for selector in &request.tests {
        let owner = boundary.owner(env, job, request, selector)?;
        if !request
            .test_files
            .iter()
            .any(|file| crate::git::tamper::path_matches(file, &owner))
        {
            return Err(refused(format!(
                "Selector {selector} is not owned by the supplied test files."
            )));
        }
    }
    Ok(())
}

pub(super) fn revise_and_replace(
    env: &mut Env,
    boundary: &mut dyn Boundary,
    plan: &PlanId,
    request: &Request,
) -> Result<(), CpdError> {
    let prior = env
        .store
        .read(|c| pending_for_lead(c, plan, &request.source, &request.requester))?;
    let job = env.store.read(|c| validate(c, plan, request))?;
    validate_selectors(env, boundary, &job, request)?;
    boundary.quiescent(env, plan, &request.source)?;
    let revised = serde_json::to_string(request).expect("request serializes");
    if revised != prior {
        env.store.write("job replace-test revise", |tx| {
            if pending_for_lead(tx, plan, &request.source, &request.requester)? != prior {
                return Err(refused("Replacement intent changed while revising."));
            }
            validate(tx, plan, request)?;
            tx.execute("UPDATE test_replacements SET request = ?3, new_key = ?4 WHERE plan = ?1 AND source = ?2 AND state = 'pending'", params![plan.as_str(), request.source, revised, request.new_key])?;
            let text = format!("Revised replacement of {} to {} with selectors {}: {}", request.source, request.new_key, request.tests.join(", "), request.reason);
            append(tx, NewEvent::job_edited(text, ["replacement", "tests", "test_files"]).plan(plan.clone()).job(JobKey::parse(&request.source)?))?;
            Ok(())
        })?;
    }
    replace(env, boundary, plan, request)
}

pub(super) fn abandon(
    env: &mut Env,
    boundary: &mut dyn Boundary,
    plan: &PlanId,
    source: &str,
    requester: &str,
    reason: &str,
) -> Result<(), CpdError> {
    if reason.trim().is_empty() {
        return Err(CpdError::new(
            ErrorCode::Usage,
            "An abandonment reason is required.",
        ));
    }
    let prior = env
        .store
        .read(|c| pending_for_lead(c, plan, source, requester))?;
    boundary.quiescent(env, plan, source)?;
    env.store.write("job abandon-test-replacement", |tx| {
        if pending_for_lead(tx, plan, source, requester)? != prior { return Err(refused("Replacement intent changed while abandoning.")); }
        tx.execute("UPDATE test_replacements SET state = 'abandoned' WHERE plan = ?1 AND source = ?2 AND state = 'pending'", params![plan.as_str(), source])?;
        let text = format!("Abandoned test replacement of {source}: {reason}");
        append(tx, NewEvent::job_edited(text, ["replacement"]).plan(plan.clone()).job(JobKey::parse(source)?))?;
        Ok(())
    })
}

fn caller(env: &mut Env, plan: &PlanId) -> Result<String, CpdError> {
    if std::env::var("CPD_ATTEMPT").is_ok_and(|s| !s.is_empty()) {
        return Err(CpdError::new(
            ErrorCode::NotALead,
            "Only the Lead replaces a test Job.",
        ));
    }
    match std::env::var("CPD_SESSION") {
        Ok(s) if !s.is_empty() => Ok(s),
        _ => env.store.read(|c| {
            Ok(c.query_row(
                "SELECT lead_session FROM plans WHERE id = ?1",
                [plan.as_str()],
                |r| r.get(0),
            )?)
        }),
    }
}

pub(super) fn abandon_command(
    args: super::AbandonTestReplacementArgs,
) -> Result<crate::output::Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let requester = caller(&mut env, &plan)?;
    let ports = env.ports.clone();
    crate::ports::Ports::scope(&ports, || {
        abandon(
            &mut env,
            &mut RealBoundary,
            &plan,
            args.key.as_str(),
            &requester,
            args.reason.as_str(),
        )
    })?;
    Ok(crate::output::Output::success(
        json!({"source": args.key, "abandoned": true}),
    ))
}

pub(super) fn command(args: super::ReplaceTestArgs) -> Result<crate::output::Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let requester = caller(&mut env, &plan)?;
    let revise = args.revise;
    let request = Request {
        source: args.key.to_string(),
        new_key: args.new_key.to_string(),
        tests: args.tests,
        test_files: args.test_files,
        reason: args.reason.to_string(),
        requester,
    };
    let ports = env.ports.clone();
    crate::ports::Ports::scope(&ports, || {
        if revise {
            revise_and_replace(&mut env, &mut RealBoundary, &plan, &request)
        } else {
            replace(&mut env, &mut RealBoundary, &plan, &request)
        }
    })?;
    Ok(crate::output::Output::success(
        json!({"source": request.source, "replacement": request.new_key}),
    ))
}

struct RealBoundary;

impl Boundary for RealBoundary {
    fn owner(
        &mut self,
        env: &mut Env,
        job: &Job,
        request: &Request,
        selector: &str,
    ) -> Result<String, CpdError> {
        // Listings run in the source worktree, where the committed red tests live.
        let plan: String = env.store.read(|c| {
            Ok(
                c.query_row("SELECT plan FROM jobs WHERE seq = ?1", [job.seq], |r| {
                    r.get(0)
                })?,
            )
        })?;
        let (worktree, lead, attempt): (String, String, String) = env.store.read(|c| Ok(c.query_row(
            "SELECT a.worktree, w.lead_branch, a.id FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job JOIN workflows w ON w.plan = j.plan AND w.key = j.workflow WHERE a.plan = ?1 AND a.job = ?2 ORDER BY a.number DESC LIMIT 1",
            params![plan, job.key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?))?;
        let root = std::path::Path::new(&worktree);
        if !crate::git::run::ok(root, &["status", "--porcelain"])?
            .trim()
            .is_empty()
        {
            return Err(CpdError::new(
                ErrorCode::DirtyTree,
                "Commit or remove uncommitted changes before replacing a test Job.",
            ));
        }
        let mut errors = Vec::new();
        if crate::testsel::list(&env.ports, root, selector, &mut errors)? != 0 {
            return Err(CpdError::new(
                ErrorCode::TestsNotFound,
                String::from_utf8_lossy(&errors).into_owned(),
            ));
        }
        let plan_id = PlanId::parse(&plan)?;
        let attempt_id = crate::ids::AttemptId::parse(&attempt)?;
        let policy = env
            .store
            .read(|c| crate::pr::policy_of_job(c, &env.home, &plan_id, &job.key))?;
        let commands = policy.test_commands.ok_or_else(|| {
            CpdError::new(
                ErrorCode::TestsUnconfigured,
                "The repository has no test commands.",
            )
        })?;
        let list = commands.list.ok_or_else(|| {
            CpdError::new(
                ErrorCode::TestsUnconfigured,
                "Replacement requires a configured test listing command.",
            )
        })?;
        let log = env
            .home
            .tests_log(&plan_id, &attempt_id, "replacement-list");
        let result = crate::git::hooks::run_command(
            &list,
            &[("test", json!(selector))],
            root,
            &crate::git::Operation { branch: &lead },
            std::time::Duration::from_secs(u64::from(env.config.timeouts.gate_minutes) * 60),
            &log,
            crate::git::Log::Truncate,
        )?;
        if result.outcome != crate::git::HookOutcome::Passed {
            return Err(CpdError::new(
                ErrorCode::TestsNotFound,
                format!("Repository listing rejected {selector}: {}", result.outcome),
            ));
        }
        if let Some((file, _)) = selector.split_once("::")
            && ((file.starts_with("pi/test/") && file.ends_with(".test.ts"))
                || (file.starts_with("web/")
                    && (file.ends_with(".test.ts") || file.ends_with(".test.tsx"))))
        {
            return Ok(file.to_owned());
        }
        // For a Rust test require a unique named declaration in a supplied file,
        // in addition to the configured listing. Ambiguous ownership is refused.
        let (repo, branch): (String, String) = env.store.read(|c| Ok(c.query_row(
            "SELECT w.repo, a.branch FROM attempts a JOIN jobs j ON j.plan=a.plan AND j.key=a.job JOIN workflows w ON w.plan=j.plan AND w.key=j.workflow WHERE a.plan=?1 AND a.job=?2 ORDER BY a.number DESC LIMIT 1",
            params![plan, job.key], |r| Ok((r.get(0)?, r.get(1)?)),
        )?))?;
        let repo = std::path::Path::new(&repo);
        let files = crate::git::run::ok(repo, &["ls-tree", "-r", "--name-only", &branch])?;
        let name = selector.rsplit("::").next().unwrap_or(selector);
        if name.is_empty() || !name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric()) {
            return Err(refused("Rust selector is not an exact test name."));
        }
        let mut owners = Vec::new();
        for file in files.lines().filter(|f| {
            f.ends_with(".rs")
                && request
                    .test_files
                    .iter()
                    .any(|entry| crate::git::tamper::path_matches(entry, f))
        }) {
            let source = crate::git::run::ok(repo, &["show", &format!("{branch}:{file}")])?;
            if source
                .lines()
                .any(|line| line.trim_start().starts_with(&format!("fn {name}(")))
            {
                owners.push(file.to_owned());
            }
        }
        if owners.len() != 1 {
            return Err(refused(format!(
                "Rust selector {selector} has no unique owner in supplied files."
            )));
        }
        Ok(owners.remove(0))
    }

    fn stop(&mut self, env: &mut Env, plan: &PlanId, request: &Request) -> Result<(), CpdError> {
        let latest: Option<(String, String)> = env.store.read(|c| Ok(c.query_row(
            "SELECT id, status FROM attempts WHERE plan = ?1 AND job = ?2 ORDER BY number DESC LIMIT 1",
            params![plan.as_str(), request.source], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?))?;
        let Some((id, status)) = latest else {
            return Err(refused("Source has no Attempt."));
        };
        if status == "merging" {
            return Err(refused(
                "The source merge is in progress; resume after recovery.",
            ));
        }
        let id = crate::ids::AttemptId::parse(&id)?;
        if status != "ended" {
            let roles = env.config.role_tiers();
            let retry_cap = env.config.retry_cap;
            let catalog = crate::attempt::abort::no_catalog();
            env.store.write("job replace-test stop", |tx| {
                if intent(tx, plan, request)?.as_deref() != Some("pending") { return Err(refused("Replacement intent changed.")); }
                let attempt = crate::attempt::ledger::load(tx, plan, &id)?;
                if attempt.status == "ended" { return Ok(()); }
                if attempt.status == "merging" { return Err(refused("The source merge is in progress.")); }
                tx.execute("UPDATE attempts SET stop_request = 'aborted' WHERE plan = ?1 AND id = ?2 AND status IN ('starting', 'running')", params![plan.as_str(), id.to_string()])?;
                let ladder = crate::attempt::ledger::Ladder { catalog: &catalog, retry_cap, roles };
                crate::attempt::ledger::end(tx, &attempt, &crate::attempt::ledger::End::Failed { cause: crate::attempt::ledger::Cause::Aborted, detail: format!("Stopped for replacement by {}", request.new_key) }, None, &ladder, crate::attempt::ledger::Stage::Driver)?;
                Ok(())
            })?;
        }
        // The existing driver owns cleanup. It may still be unwinding after the
        // ledger end; never assume terminal means the runner has stopped.
        let Some(_free) = crate::driver::lock::probe(&env.home, plan, &id)? else {
            return Err(refused(
                "Source driver is still tearing down; resume the replacement.",
            ));
        };
        let tmux = crate::host::tmux::Tmux::new(env.config.tmux.socket.as_str());
        crate::driver::runner::kill_orphan(
            &env.ports,
            &env.config,
            &env.home,
            &tmux,
            &mut env.store,
            plan,
            &id,
        )?;
        confirm_runner_absent(env, plan, &id)
    }

    fn quiescent(&mut self, env: &mut Env, plan: &PlanId, source: &str) -> Result<(), CpdError> {
        let ids: Vec<String> = env.store.read(|c| {
            let mut stmt =
                c.prepare("SELECT id FROM attempts WHERE plan = ?1 AND job = ?2 ORDER BY number")?;
            let rows = stmt.query_map(params![plan.as_str(), source], |r| r.get(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        for id in ids {
            let id = crate::ids::AttemptId::parse(&id)?;
            let Some(_free) = crate::driver::lock::probe(&env.home, plan, &id)? else {
                return Err(refused("Source driver is still live."));
            };
            confirm_runner_absent(env, plan, &id)?;
        }
        Ok(())
    }

    fn start(
        &mut self,
        env: &mut Env,
        plan: &PlanId,
        request: &Request,
    ) -> Result<String, CpdError> {
        let (repo, lead, branches): (String, String, Vec<String>) = env.store.read(|c| {
            let (repo, lead): (String, String) = c.query_row("SELECT w.repo, w.lead_branch FROM jobs j JOIN workflows w ON w.plan = j.plan AND w.key = j.workflow WHERE j.plan = ?1 AND j.key = ?2", params![plan.as_str(), request.source], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let mut stmt = c.prepare("SELECT branch FROM attempts WHERE plan = ?1 AND job = ?2 ORDER BY number")?;
            let branches = stmt.query_map(params![plan.as_str(), request.source], |r| r.get(0))?.collect::<Result<Vec<_>, _>>()?;
            Ok((repo, lead, branches))
        })?;
        let root = std::path::Path::new(&repo);
        // No ref may be moved after teardown, and uncommitted red work cannot
        // be represented by the replacement's starting commit.
        for branch in &branches {
            if let Some(path) = crate::git::worktree::worktree_of_branch(root, branch)?
                && !crate::git::run::ok(&path, &["status", "--porcelain"])?
                    .trim()
                    .is_empty()
            {
                return Err(CpdError::new(
                    ErrorCode::DirtyTree,
                    format!(
                        "Source worktree {} has uncommitted changes.",
                        path.display()
                    ),
                ));
            }
        }
        let tips = branches
            .iter()
            .map(|branch| crate::git::run::rev_parse(root, &format!("refs/heads/{branch}")))
            .collect::<Result<Vec<_>, _>>()?;
        let sha = tips
            .last()
            .ok_or_else(|| refused("Source has no Attempt commit."))?
            .clone();
        for tip in tips
            .iter()
            .chain(std::iter::once(&crate::git::run::rev_parse(
                root,
                &format!("refs/heads/{lead}"),
            )?))
        {
            if crate::git::run::run(root, &["merge-base", "--is-ancestor", tip, &sha])?.code != 0 {
                return Err(refused(
                    "Source Attempt commits and Lead Branch are not all ancestors of one existing commit.",
                ));
            }
        }
        Ok(sha)
    }
}

/// Verify without signalling: PID reuse or unreadable process state is not an
/// acknowledgement, so even an unrelated live PID makes recovery wait.
fn confirm_runner_absent(
    env: &mut Env,
    plan: &PlanId,
    id: &crate::ids::AttemptId,
) -> Result<(), CpdError> {
    let (runner, pid): (String, Option<u32>) = env.store.read(|c| {
        Ok(c.query_row(
            "SELECT runner, worker_pid FROM attempts WHERE plan = ?1 AND id = ?2",
            params![plan.as_str(), id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    })?;
    match runner.as_str() {
        "durable" if pid.is_some_and(|p| env.ports.spawn.alive(crate::host::spawn::Pid(p))) => {
            Err(refused("Source runner is still live."))
        }
        "durable" => Ok(()),
        "pi" => {
            let tmux = crate::host::tmux::Tmux::new(env.config.tmux.socket.as_str());
            let name = crate::host::layout::attempt_window(plan, id);
            if tmux
                .list_panes()?
                .iter()
                .any(|p| p.session_name == env.config.tmux.work_session && p.window_name == name)
            {
                return Err(refused("Source runner window is still live."));
            }
            Ok(())
        }
        _ => Err(CpdError::bad_row(format!("Unknown runner {runner:?}"))),
    }
}
