//! `pr edit` (commands.md §9.5, contract §3.8): changes a `waiting` PR's repo,
//! base (a branch, or a base PR to stack on) or needs before the lock. The plan
//! fingerprint changes with them. After the lock only a restack is allowed (§3.8 step 10): a
//! `waiting` PR stacks on a PR it needs (`--base <A>` with `--needs <rest>` or `--no-needs`). With `--title` it also changes a PR's own title
//! in any phase and any status but `merged` and `cancelled` (hs-d285): the title is
//! not a Locked field and not in the fingerprint.


use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::gh;
use super::add::{
    BaseArg, BaseWanted, Subject, classify_base, insert_needs, key_repo, known_prs, main_worktree, needs_refs, no_branch_clash,
    resolve_refs, stacked_on, wanted_base,
};
use super::{EditArgs, Env, find_pr, plan_phase, pr_locked};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{PlanId, PrKey, RepoName};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::PrRow;
use crate::plan::fingerprint::load_prs;

const COMMAND: &str = "pr edit";

/// The flags given: the repo resolved, the needs sorted.
struct Edits {
    repo: Option<String>,
    /// The new repo's name, from its `origin`; set with `repo`.
    name: Option<RepoName>,
    base: Option<BaseWanted>,
    /// The new needs as the sorted references of the Plan (`needs_refs`).
    needs: Option<Vec<String>>,
    title: Option<String>,
    jira: Option<String>,
}

impl Edits {
    /// Whether a flag other than `--title` was given: only these need a `waiting` PR before the lock.
    fn others(&self) -> bool {
        self.repo.is_some() || self.base.is_some() || self.needs.is_some()
    }

    /// Whether the flags have the shape of a restack: `--base` with `--needs` or `--no-needs`, no `--repo`.
    fn restack_shape(&self) -> bool {
        self.repo.is_none() && self.base.is_some() && self.needs.is_some()
    }
}

enum Decision {
    /// No value changes.
    Already(PrRow),
    Update { before: PrRow, after: PrRow, fields: Vec<&'static str> },
}

pub(super) fn edit(args: EditArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    if args.repo.is_none() && args.base.is_none() && args.needs.is_none() && !args.no_needs && args.title.is_none() && args.jira.is_none() {
        return Err(CpdError::new(ErrorCode::NothingToEdit, "Nothing to edit: pass --repo, --base, --needs, --no-needs, --title or --jira."));
    }
    let needs = match (&args.needs, args.no_needs) {
        (Some(refs), _) => Some(needs_refs(&plan, refs)?),
        (None, true) => Some(Vec::new()),
        (None, false) => None,
    };
    let key = args.key;
    let mut env = Env::open()?;

    // The phase, the PR and its status first; then git on the new repo and base.
    let wants_others = args.repo.is_some() || args.base.is_some() || needs.is_some();
    let wants_title = args.title.is_some() || args.jira.is_some();
    let restack_shape = args.repo.is_none() && args.base.is_some() && needs.is_some();
    let stored = env.store.read(|conn| editable_pr(conn, &plan, &key, wants_title, wants_others, restack_shape))?.0;
    if stored.head_ref.is_some()
        && let Some(flag) = [(args.repo.is_some(), "--repo"), (args.base.is_some(), "--base")].iter().find_map(|(given, flag)| given.then_some(*flag))
    {
        return Err(CpdError::new(ErrorCode::PrAdopted, format!("PR {key} is an adopted pull request: its repo and base come from the pull request, so {flag} cannot change."))
            .with("pr", key.as_str())
            .with("flag", flag));
    }
    let repo = args.repo.as_deref().map(main_worktree).transpose()?;
    let name = repo.as_deref().map(|r| key_repo(&mut env, &plan, r)).transpose()?;
    let in_repo = repo.clone().unwrap_or_else(|| stored.repo.clone().into());
    let base = match args.base.as_deref() {
        Some(_) => Some(wanted_base(&in_repo, env.store.read(|conn| classify_base(conn, &plan, &in_repo, args.base.as_deref()))?)?),
        // A new repo keeps the stored base, which must exist there.
        None if repo.is_some() => Some(wanted_base(&in_repo, BaseArg::Branch(Some(stored.base.clone())))?),
        None => None,
    };
    let edits = Edits { repo: repo.as_ref().map(|r| r.display().to_string()), name, base, needs, title: args.title.map(|t| t.to_string()), jira: args.jira.map(|j| j.to_string()) };

    let decision = env.store.read(|conn| decide(conn, &plan, &key, &edits))?;
    let (pr, fields, already) = match decision {
        Decision::Already(pr) => (pr, Vec::new(), true),
        Decision::Update { before, after, fields } => {
            if after.repo != before.repo {
                no_branch_clash(repo.as_deref().expect("the repo changed, so it was given"), &plan, &before.lead_branch)?;
            }
            if let Some(title) = edits.title.as_deref().filter(|_| fields.contains(&"title")) {
                retitle_open_pull_request(&mut env, &plan, &key, title)?;
            }
            env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, &edits)? {
                Decision::Already(pr) => Ok((pr, Vec::new(), true)),
                Decision::Update { before, after, fields } => {
                    update(tx, &plan, &before, &after, edits.name.as_ref(), edits.title.as_deref(), edits.jira.as_deref())?;
                    let text = format!("PR {key} of Plan {plan} edited ({}). Next: cpd plan render --plan {plan}", fields.join(", "));
                    append(tx, NewEvent::pr_edited(text, &key, &fields).plan(plan.clone()))?;
                    crate::session::working_pr::name(tx, &plan, &key)?;
                    Ok((after, fields, false))
                }
            })?
        }
    };
    let mut out = json!({"pr": {"key": pr.key, "status": pr.status}, "fields": fields});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// When the PR has an open pull request, retitles it on GitHub, before the title is stored: a failure stores nothing (§9.5).
fn retitle_open_pull_request(env: &mut Env, plan: &PlanId, key: &PrKey, title: &str) -> Result<(), CpdError> {
    let open: Option<(String, i64)> = env.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT p.repo, p.number FROM workflows r JOIN pull_requests p ON p.id = r.pull_request
              WHERE r.plan = ?1 AND r.key = ?2 AND r.status = 'open' AND p.state = 'open'",
        )?;
        Ok(stmt.query_row([plan.as_str(), key.as_str()], |r| Ok((r.get(0)?, r.get(1)?))).optional()?)
    })?;
    match open {
        Some((repo, number)) => gh::retitle_pr(env.home.root(), &repo, number, title),
        None => Ok(()),
    }
}

/// The PR `key` and its stored title, which must be editable. With `title`, the PR is not
/// `merged` or `cancelled` (`pr_ended`: its title is history). With `others` (a flag other than
/// `--title`), the Plan is in `grounding` or `plan` and the PR `waiting` (`illegal_transition`); `--title` alone
/// meets neither rule. After the lock only a restack (`restack`: the flags have its shape) of a `waiting` PR passes;
/// anything else is `pr_locked`, and `decide` checks the rest of the restack.
fn editable_pr(conn: &Connection, plan: &PlanId, key: &PrKey, title: bool, others: bool, restack: bool) -> Result<(PrRow, Option<String>), CpdError> {
    let prs = load_prs(conn, plan)?;
    let pr = find_pr(&prs, plan, key)?;
    if title && matches!(pr.status.as_str(), "merged" | "cancelled") {
        return Err(CpdError::new(ErrorCode::PrEnded, format!("PR {key} of Plan {plan} is {}: its title and ticket are history.", pr.status))
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
            .with("status", pr.status.as_str()));
    }
    if others {
        let phase = plan_phase(conn, plan)?;
        if !matches!(phase, Phase::Grounding | Phase::Plan) && !(restack && pr.status == "waiting") {
            return Err(pr_locked(plan, phase, COMMAND));
        }
        if pr.status != "waiting" {
            return Err(CpdError::new(ErrorCode::IllegalTransition, format!("PR {key} is {}: only a waiting PR is edited.", pr.status))
                .with("plan", plan.as_str())
                .with("pr", key.as_str())
                .with("from", pr.status.as_str())
                .with("to", "waiting"));
        }
    }
    let stored = conn.query_row("SELECT title FROM workflows WHERE plan = ?1 AND key = ?2", [plan.as_str(), key.as_str()], |r| r.get(0))?;
    Ok((pr.clone(), stored))
}

fn decide(conn: &Connection, plan: &PlanId, key: &PrKey, edits: &Edits) -> Result<Decision, CpdError> {
    let (before, stored_title) = editable_pr(conn, plan, key, edits.title.is_some() || edits.jira.is_some(), edits.others(), edits.restack_shape())?;
    let mut after = before.clone();
    if let Some(repo) = &edits.repo {
        after.repo = repo.clone();
    }
    if let Some(needs) = &edits.needs {
        after.needs = needs.clone();
    }
    // Every reference the PR will hold is validated again, so a rule broken by the repo alone (a base PR of
    // another repo) is refused too.
    if edits.repo.is_some() || edits.base.is_some() || edits.needs.is_some() {
        let base = match &edits.base {
            Some(BaseWanted::Pr(p, k)) => BaseWanted::Pr(p.clone(), k.clone()),
            Some(BaseWanted::Branch(b)) => BaseWanted::Branch(b.clone()),
            None => BaseWanted::Branch(before.base.clone()),
        };
        let resolved = resolve_refs(conn, &Subject { plan, key, repo: &after.repo }, &base, &after.needs)?;
        after.base = resolved.base;
        let phase = plan_phase(conn, plan)?;
        if !matches!(phase, Phase::Grounding | Phase::Plan) {
            check_restack(conn, plan, &before, &after, resolved.base_pr.as_deref()).map_err(|()| pr_locked(plan, phase, COMMAND))?;
        }
    }
    if after.repo != before.repo {
        let in_use = stacked_on(&known_prs(conn, plan)?, &before.repo, &before.lead_branch);
        if !in_use.is_empty() {
            return Err(CpdError::new(
                ErrorCode::PrInUse,
                format!("PR {key} keeps its repo: {} {} stacked on it.", in_use.join(", "), if in_use.len() == 1 { "is" } else { "are" }),
            )
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
            .with("workflows", in_use));
        }
    }
    let title_changed = edits.title.as_ref().is_some_and(|t| stored_title.as_ref() != Some(t));
    let jira_changed = edits.jira.as_ref().is_some_and(|j| before.jira.as_ref() != Some(j));
    let fields: Vec<&'static str> = [
        ("repo", before.repo != after.repo),
        ("base", before.base != after.base),
        ("needs", before.needs != after.needs),
        ("title", title_changed),
        ("jira", jira_changed),
    ]
    .into_iter()
        .filter_map(|(name, changed)| changed.then_some(name))
        .collect();
    if fields.is_empty() {
        return Ok(Decision::Already(before));
    }
    Ok(Decision::Update { before, after, fields })
}

/// After the lock, a restack is allowed only onto `A`, the base PR the edit resolves to, when `A` is a current need or
/// already the base PR, and the needs after the edit are the current needs without `A` (§3.8 step 10).
fn check_restack(conn: &Connection, plan: &PlanId, before: &PrRow, after: &PrRow, base_pr: Option<&str>) -> Result<(), ()> {
    let a = base_pr.and_then(|p| p.split_once('/')).map(|(_, k)| k).ok_or(())?;
    let prs = load_prs(conn, plan).map_err(|_| ())?;
    let already = prs.iter().any(|q| q.key.as_str() == a && q.repo == before.repo && q.lead_branch == before.base);
    if !already && !before.needs.iter().any(|n| n == a) {
        return Err(());
    }
    let mut want: Vec<&String> = before.needs.iter().filter(|n| n.as_str() != a).collect();
    let mut got: Vec<&String> = after.needs.iter().collect();
    want.sort();
    got.sort();
    if want == got { Ok(()) } else { Err(()) }
}

/// Writes the changed `prs` columns, and deletes and inserts `pr_needs` rows
/// to match (§4.4 allows deletion in `grounding` and `plan`). `title` is the new
/// title when it was given (a PR that has it already is not touched).
fn update(tx: &Connection, plan: &PlanId, before: &PrRow, after: &PrRow, name: Option<&RepoName>, title: Option<&str>, jira: Option<&str>) -> Result<(), CpdError> {
    if let Some(title) = title {
        tx.execute(
            "UPDATE workflows SET title = ?1 WHERE plan = ?2 AND key = ?3 AND title IS NOT ?1 AND status NOT IN ('merged', 'cancelled')",
            [title, plan.as_str(), after.key.as_str()],
        )?;
    }
    if let Some(jira) = jira {
        tx.execute(
            "UPDATE workflows SET jira = ?1 WHERE plan = ?2 AND key = ?3 AND jira IS NOT ?1 AND status NOT IN ('merged', 'cancelled')",
            [jira, plan.as_str(), after.key.as_str()],
        )?;
    }
    if (&before.repo, &before.base) != (&after.repo, &after.base) {
        tx.execute(
            "UPDATE workflows SET repo = ?1, base = ?2, name = coalesce(?5, name) WHERE plan = ?3 AND key = ?4 AND status = 'waiting'",
            rusqlite::params![after.repo.as_str(), after.base.as_str(), plan.as_str(), after.key.as_str(), name.map(RepoName::as_str)],
        )?;
    }
    for need in before.needs.iter().filter(|n| !after.needs.contains(n)) {
        tx.execute("DELETE FROM workflow_needs WHERE plan = ?1 AND workflow = ?2 AND needs = ?3", [plan.as_str(), after.key.as_str(), need.as_str()])?;
    }
    let added: Vec<String> = after.needs.iter().filter(|n| !before.needs.contains(n)).cloned().collect();
    insert_needs(tx, plan, &after.key, &added)
}
