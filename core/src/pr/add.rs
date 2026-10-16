//! `pr add` (commands.md §9.4, contract §3.8): adds a PR to the Plan,
//! `waiting`. Nothing is started here: no git write, no tmux. It also holds
//! what `pr edit` shares with it: the repo, the base and the branch checks.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::json;

use super::{AddArgs, Env, check_needs, plan_phase, sorted_needs, wrong_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::git::run::{branch_exists, has_origin, run};
use crate::git::worktree::list as worktrees;
use crate::ids::{PlanId, PrKey};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::fingerprint::{PrSpec, load_prs};

const COMMAND: &str = "pr add";

/// The flags given, resolved.
struct Wanted {
    key: PrKey,
    repo: String,
    base: String,
    needs: Vec<String>,
}

enum Decision {
    /// The PR exists with these values; its status and `lead_branch` as stored, and `appended`
    /// as the Event recorded it.
    Already { status: String, lead_branch: String, appended: bool },
    Insert { appended: bool },
}

pub(super) fn add(args: AddArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let needs = sorted_needs(&args.needs)?;
    let mut env = Env::open()?;
    check_phase(&plan, env.store.read(|conn| plan_phase(conn, &plan))?)?;
    let repo = main_worktree(&args.repo)?;
    let wanted = Wanted {
        base: resolve_base(&repo, args.base.as_deref())?,
        repo: repo.display().to_string(),
        key: args.key,
        needs,
    };

    let (status, lead_branch, appended, already) = match env.store.read(|conn| decide(conn, &plan, &wanted))? {
        Decision::Already { status, lead_branch, appended } => (status, lead_branch, appended, true),
        Decision::Insert { .. } => {
            // git, outside any transaction.
            no_branch_clash(&repo, &plan, &wanted.key)?;
            env.store.write(COMMAND, |tx| match decide(tx, &plan, &wanted)? {
                Decision::Already { status, lead_branch, appended } => Ok((status, lead_branch, appended, true)),
                Decision::Insert { appended } => {
                    let lead_branch = pr_branch(&plan, &wanted.key);
                    insert(tx, &plan, &wanted, &lead_branch)?;
                    let key = &wanted.key;
                    let with = if wanted.needs.is_empty() { String::new() } else { format!(", needs {}", wanted.needs.join(", ")) };
                    let text = format!(
                        "PR {key} added to Plan {plan} (repo {}, base {}{with}), waiting. Next: cpd plan show --plan {plan}",
                        wanted.repo, wanted.base
                    );
                    let data = json!({"pr": key, "repo": wanted.repo, "base": wanted.base, "needs": wanted.needs, "appended": appended});
                    append(tx, NewEvent::new(Kind::PrAdded, text, data).plan(plan.clone()))?;
                    Ok(("waiting".to_owned(), lead_branch, appended, false))
                }
            })?
        }
    };
    let mut out = json!({"pr": {
        "key": wanted.key, "repo": wanted.repo, "base": wanted.base, "needs": wanted.needs,
        "status": status, "lead_branch": lead_branch, "appended": appended,
    }});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// `pr add` is for the phases `grounding`, `plan` and `dispatch` (`illegal_phase`).
fn check_phase(plan: &PlanId, phase: Phase) -> Result<(), CpdError> {
    match phase {
        Phase::Grounding | Phase::Plan | Phase::Dispatch => Ok(()),
        Phase::Pr | Phase::Done => Err(wrong_phase(plan, phase, COMMAND, "grounding, plan or dispatch")),
    }
}

fn decide(conn: &Connection, plan: &PlanId, wanted: &Wanted) -> Result<Decision, CpdError> {
    let phase = plan_phase(conn, plan)?;
    check_phase(plan, phase)?;
    let prs = load_prs(conn, plan)?;
    if let Some(existing) = prs.iter().find(|p| p.key == wanted.key.as_str()) {
        if same(existing, wanted) {
            // `appended`: the PR came after the lock.
            let (lead_branch, appended) = conn.query_row(
                "SELECT r.lead_branch, coalesce(p.locked_at IS NOT NULL AND r.created_at >= p.locked_at, 0)
                   FROM prs r JOIN plans p ON p.id = r.plan WHERE r.plan = ?1 AND r.key = ?2",
                [plan.as_str(), wanted.key.as_str()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)),
            )?;
            return Ok(Decision::Already { status: existing.status.clone(), lead_branch, appended });
        }
        return Err(CpdError::new(ErrorCode::PrExists, format!("Plan {plan} already has a PR {} with other flags.", wanted.key))
            .with("plan", plan.as_str())
            .with("pr", wanted.key.as_str())
            .with("repo", existing.repo.as_str())
            .with("base", existing.base.as_str())
            .with("needs", existing.needs.clone())
            .with("status", existing.status.as_str()));
    }
    check_needs(&prs, plan, None, &wanted.needs)?;
    Ok(Decision::Insert { appended: phase == Phase::Dispatch })
}

fn same(existing: &PrSpec, wanted: &Wanted) -> bool {
    (&existing.repo, &existing.base, &existing.needs) == (&wanted.repo, &wanted.base, &wanted.needs)
}

fn insert(tx: &Connection, plan: &PlanId, wanted: &Wanted, lead_branch: &str) -> Result<(), CpdError> {
    tx.execute(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         SELECT ?1, ?2, ?3, ?4, 'waiting', ?5, id, at FROM txns WHERE id = cpd_txn()",
        rusqlite::params![plan.as_str(), wanted.key.as_str(), wanted.repo, wanted.base, lead_branch],
    )?;
    for need in &wanted.needs {
        tx.execute("INSERT INTO pr_needs (plan, pr, needs) VALUES (?1, ?2, ?3)", [plan.as_str(), wanted.key.as_str(), need.as_str()])?;
    }
    Ok(())
}

/// The Lead Branch of a PR made now (contract §2.1): `cpd/<plan>/<key>`.
fn pr_branch(plan: &PlanId, key: &PrKey) -> String {
    format!("cpd/{plan}/{key}")
}

/// Neither `cpd/<plan>/<key>` nor `cpd/<plan>` exists in the repo (§2.1: the
/// second would make the first impossible), else `branch_exists` (exit 3).
pub(super) fn no_branch_clash(repo: &Path, plan: &PlanId, key: &PrKey) -> Result<(), CpdError> {
    for branch in [pr_branch(plan, key), format!("cpd/{plan}")] {
        if branch_exists(repo, &branch)? {
            return Err(CpdError::new(ErrorCode::BranchExists, format!("Branch {branch} already exists in {}.", repo.display()))
                .with("branch", branch)
                .with("repo", repo.display().to_string()));
        }
    }
    Ok(())
}

fn not_a_git_repo(path: &Path, why: &str) -> CpdError {
    CpdError::new(ErrorCode::NotAGitRepo, format!("{} is not inside a git repository: {why}", path.display()))
        .with("repo", path.display().to_string())
}

/// The realpath of the main worktree of the repo `path` is in
/// (`git rev-parse --git-common-dir`, then the first `git worktree list`
/// entry); `not_a_git_repo` when it is in none.
pub(super) fn main_worktree(path: &Path) -> Result<PathBuf, CpdError> {
    let out = run(path, &["rev-parse", "--git-common-dir"])?;
    if out.code != 0 {
        return Err(not_a_git_repo(path, out.stderr.lines().next().unwrap_or("git rev-parse failed.")));
    }
    let main = worktrees(path)?.into_iter().next().ok_or_else(|| not_a_git_repo(path, "it has no worktree."))?;
    main.path.canonicalize().map_err(|e| not_a_git_repo(&main.path, &format!("{e}.")))
}

/// `--base`, else the repo's `origin` HEAD branch, else `main`; it must exist
/// as `refs/remotes/origin/<base>`, or `refs/heads/<base>` without `origin`
/// (`base_not_found`, exit 4).
pub(super) fn resolve_base(repo: &Path, base: Option<&str>) -> Result<String, CpdError> {
    let origin = has_origin(repo)?;
    let base = match base {
        Some(base) => base.to_owned(),
        None if origin => {
            let out = run(repo, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])?;
            match out.stdout.trim().strip_prefix("origin/") {
                Some(branch) if out.code == 0 && !branch.is_empty() => branch.to_owned(),
                _ => "main".to_owned(),
            }
        }
        None => "main".to_owned(),
    };
    let reference = if origin { format!("refs/remotes/origin/{base}") } else { format!("refs/heads/{base}") };
    if run(repo, &["rev-parse", "--verify", "--quiet", &reference])?.code != 0 {
        return Err(CpdError::new(ErrorCode::BaseNotFound, format!("The base {reference} does not exist in {}.", repo.display()))
            .with("base", base)
            .with("ref", reference)
            .with("repo", repo.display().to_string()));
    }
    Ok(base)
}
