//! `pr add` (commands.md §9.4, contract §3.8): adds a PR to the Plan,
//! `waiting`. Nothing is started here: no git write, no tmux. It also holds
//! what `pr edit` shares with it: the repo, the base and the branch checks.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::json;

use super::{AddArgs, Env, check_needs, plan_phase, sorted_needs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::git::run::{branch_exists, delete_remote_branch, has_origin, ok, remote_has_branch, run};
use crate::git::worktree::list as worktrees;
use crate::ids::{PlanId, PrKey};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::fingerprint::{PrSpec, load_prs};
use crate::store::WriteTxn;

const COMMAND: &str = "pr add";

/// The flags given, resolved.
struct Wanted {
    key: PrKey,
    repo: String,
    base: String,
    /// `--needs` as given (sorted); `Some(vec![])` is `--no-needs`; `None`: the default.
    needs: Option<Vec<String>>,
}

enum Decision {
    /// The PR exists with these values; its status and `lead_branch` as stored, and `appended`
    /// as the Event recorded it.
    Already { status: String, lead_branch: String, appended: bool, needs: Vec<String> },
    /// The PR is new, in a Plan in this phase.
    Insert { phase: Phase, needs: Vec<String> },
}

/// The merged PR whose Lead Branch is the legacy `cpd/<plan>` (contract §3.8, D3).
struct Legacy {
    key: String,
    repo: PathBuf,
}

pub(super) fn add(args: AddArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let needs = if args.no_needs {
        Some(Vec::new())
    } else if args.needs.is_empty() {
        None
    } else {
        Some(sorted_needs(&args.needs)?)
    };
    let mut env = Env::open()?;
    env.store.read(|conn| check_plan(conn, &plan))?;
    let repo = main_worktree(&args.repo)?;
    let wanted = Wanted {
        base: resolve_base(&repo, args.base.as_deref())?,
        repo: repo.display().to_string(),
        key: args.key,
        needs,
    };

    let (status, lead_branch, appended, needs, already) = match env.store.read(|conn| decide(conn, &plan, &wanted))? {
        Decision::Already { status, lead_branch, appended, needs } => (status, lead_branch, appended, needs, true),
        Decision::Insert { .. } => {
            // git, outside any transaction.
            let legacy = env.store.read(|conn| legacy_pr(conn, &plan))?;
            if let Some(legacy) = &legacy {
                move_legacy_branch(&plan, legacy)?;
            }
            no_branch_clash(&repo, &plan, &wanted.key)?;
            env.store.write(COMMAND, |tx| match decide(tx, &plan, &wanted)? {
                Decision::Already { status, lead_branch, appended, needs } => Ok((status, lead_branch, appended, needs, true)),
                Decision::Insert { phase, needs } => {
                    let appended = matches!(phase, Phase::Dispatch | Phase::Pr | Phase::Done);
                    let lead_branch = pr_branch(&plan, &wanted.key);
                    let key = &wanted.key;
                    if matches!(phase, Phase::Pr | Phase::Done) {
                        reopen(tx, &plan, phase, key)?;
                    }
                    if let Some(legacy) = &legacy {
                        rename_legacy(tx, &plan, legacy)?;
                    }
                    insert(tx, &plan, &wanted, &needs, &lead_branch)?;
                    let with = if needs.is_empty() { String::new() } else { format!(", needs {}", needs.join(", ")) };
                    let text = format!(
                        "PR {key} added to Plan {plan} (repo {}, base {}{with}), waiting. Next: cpd plan show --plan {plan}",
                        wanted.repo, wanted.base
                    );
                    let data = json!({"pr": key, "repo": wanted.repo, "base": wanted.base, "needs": needs, "appended": appended});
                    append(tx, NewEvent::new(Kind::PrAdded, text, data).plan(plan.clone()))?;
                    Ok(("waiting".to_owned(), lead_branch, appended, needs, false))
                }
            })?
        }
    };
    let mut out = json!({"pr": {
        "key": wanted.key, "repo": wanted.repo, "base": wanted.base, "needs": needs,
        "status": status, "lead_branch": lead_branch, "appended": appended,
    }});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// `pr add` is for every phase (D1), but not for an archived Plan (`archived`); returns the phase.
fn check_plan(conn: &Connection, plan: &PlanId) -> Result<Phase, CpdError> {
    let phase = plan_phase(conn, plan)?;
    let archived: bool = conn.query_row("SELECT archived_at IS NOT NULL FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0))?;
    if archived {
        return Err(CpdError::new(ErrorCode::Archived, format!("Plan {plan} is archived: unarchive it first."))
            .with("plan", plan.as_str())
            .with("next", format!("cpd plan unarchive {plan}")));
    }
    Ok(phase)
}

fn decide(conn: &Connection, plan: &PlanId, wanted: &Wanted) -> Result<Decision, CpdError> {
    let phase = check_plan(conn, plan)?;
    let prs = load_prs(conn, plan)?;
    if let Some(existing) = prs.iter().find(|p| p.key == wanted.key.as_str()) {
        if same(existing, wanted, &prs) {
            // `appended`: the PR came after the lock.
            let (lead_branch, appended) = conn.query_row(
                "SELECT r.lead_branch, coalesce(p.locked_at IS NOT NULL AND r.created_at >= p.locked_at, 0)
                   FROM prs r JOIN plans p ON p.id = r.plan WHERE r.plan = ?1 AND r.key = ?2",
                [plan.as_str(), wanted.key.as_str()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)),
            )?;
            return Ok(Decision::Already { status: existing.status.clone(), lead_branch, appended, needs: existing.needs.clone() });
        }
        return Err(CpdError::new(ErrorCode::PrExists, format!("Plan {plan} already has a PR {} with other flags.", wanted.key))
            .with("plan", plan.as_str())
            .with("pr", wanted.key.as_str())
            .with("repo", existing.repo.as_str())
            .with("base", existing.base.as_str())
            .with("needs", existing.needs.clone())
            .with("status", existing.status.as_str()));
    }
    let needs = match &wanted.needs {
        Some(needs) => needs.clone(),
        None if matches!(phase, Phase::Pr | Phase::Done) => merged_keys(&prs),
        None => Vec::new(),
    };
    check_needs(&prs, plan, None, &needs)?;
    Ok(Decision::Insert { phase, needs })
}

/// The keys of the `merged` PRs, in key order (A3).
fn merged_keys(prs: &[PrSpec]) -> Vec<String> {
    let mut keys: Vec<String> = prs.iter().filter(|p| p.status == "merged").map(|p| p.key.clone()).collect();
    keys.sort();
    keys
}

/// Without `--needs` or `--no-needs` the needs match when none, or when all merged: a rerun
/// after the reopen sees the Plan in `dispatch`, where the default is none.
fn same(existing: &PrSpec, wanted: &Wanted, prs: &[PrSpec]) -> bool {
    let needs_match = match &wanted.needs {
        Some(needs) => &existing.needs == needs,
        None => existing.needs.iter().all(|n| merged_keys(prs).contains(n)),
    };
    (&existing.repo, &existing.base) == (&wanted.repo, &wanted.base) && needs_match
}

fn insert(tx: &Connection, plan: &PlanId, wanted: &Wanted, needs: &[String], lead_branch: &str) -> Result<(), CpdError> {
    tx.execute(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         SELECT ?1, ?2, ?3, ?4, 'waiting', ?5, id, at FROM txns WHERE id = cpd_txn()",
        rusqlite::params![plan.as_str(), wanted.key.as_str(), wanted.repo, wanted.base, lead_branch],
    )?;
    for need in needs {
        tx.execute("INSERT INTO pr_needs (plan, pr, needs) VALUES (?1, ?2, ?3)", [plan.as_str(), wanted.key.as_str(), need.as_str()])?;
    }
    Ok(())
}

/// In `pr` or `done` the Plan goes back to `dispatch` (D1): the phase is compared and set, then
/// `plan-phase` is appended (before `pr-added`).
fn reopen(tx: &WriteTxn<'_>, plan: &PlanId, from: Phase, key: &PrKey) -> Result<(), CpdError> {
    let moved = tx.execute("UPDATE plans SET phase = 'dispatch', done_at = NULL WHERE id = ?1 AND phase = ?2", [plan.as_str(), from.as_str()])?;
    if moved == 0 {
        return Err(CpdError::new(ErrorCode::CasLost, format!("Plan {plan} left phase {} meanwhile: rerun the command.", from.as_str()))
            .with("plan", plan.as_str()));
    }
    let text = format!("Plan {plan} is back in dispatch: PR {key} was added.");
    append(tx, NewEvent::new(Kind::PlanPhase, text, json!({"from": from.as_str(), "to": "dispatch"})).plan(plan.clone()))?;
    Ok(())
}

/// The Plan's `merged` PR on the legacy Lead Branch `cpd/<plan>`, if any (D3). A PR on it that
/// is not `merged` is not returned: `no_branch_clash` refuses it.
fn legacy_pr(conn: &Connection, plan: &PlanId) -> Result<Option<Legacy>, CpdError> {
    use rusqlite::OptionalExtension;
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT key, repo FROM prs WHERE plan = ?1 AND lead_branch = ?2 AND status = 'merged'",
            [plan.as_str(), &format!("cpd/{plan}")],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.map(|(key, repo)| Legacy { key, repo: PathBuf::from(repo) }))
}

/// D3 steps 1 and 2, in the legacy PR's repo: `git branch -m cpd/<plan> cpd/<plan>/<key>` when
/// `cpd/<plan>` exists, then `push origin --delete cpd/<plan>` when `origin` has it
/// (`push_failed`). Each step is skipped when done, so a rerun completes (A3).
fn move_legacy_branch(plan: &PlanId, legacy: &Legacy) -> Result<(), CpdError> {
    let old = format!("cpd/{plan}");
    let new = format!("{old}/{}", legacy.key);
    let repo = &legacy.repo;
    if branch_exists(repo, &old)? {
        ok(repo, &["branch", "-m", &old, &new])?;
    }
    if has_origin(repo)? && remote_has_branch(repo, &old)? {
        delete_remote_branch(repo, &old)?;
    }
    Ok(())
}

/// D3 step 3: the legacy PR's `lead_branch` follows the rename.
fn rename_legacy(tx: &Connection, plan: &PlanId, legacy: &Legacy) -> Result<(), CpdError> {
    let moved = tx.execute(
        "UPDATE prs SET lead_branch = ?3 WHERE plan = ?1 AND key = ?2 AND lead_branch = ?4 AND status = 'merged'",
        [plan.as_str(), &legacy.key, &format!("cpd/{plan}/{}", legacy.key), &format!("cpd/{plan}")],
    )?;
    if moved == 0 {
        return Err(CpdError::new(ErrorCode::CasLost, format!("PR {} of Plan {plan} changed meanwhile: rerun the command.", legacy.key))
            .with("plan", plan.as_str())
            .with("pr", legacy.key.as_str()));
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
