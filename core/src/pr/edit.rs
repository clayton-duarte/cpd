//! `pr edit` (commands.md §9.5, contract §3.8): changes a `waiting` PR's repo,
//! base or needs before the lock. The plan fingerprint changes with them. With
//! `--title` it also changes a PR's own title in any phase and any status but
//! `merged` and `cancelled` (hs-d285): the title is not a Locked field and not in
//! the fingerprint.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::gh;
use super::add::{main_worktree, no_branch_clash, resolve_base};
use super::{EditArgs, Env, check_needs, find_pr, plan_phase, pr_locked, sorted_needs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::ids::{PlanId, PrKey};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::fingerprint::{PrSpec, load_prs};

const COMMAND: &str = "pr edit";

/// The flags given: the repo resolved, the needs sorted.
struct Edits {
    repo: Option<String>,
    base: Option<String>,
    needs: Option<Vec<String>>,
    title: Option<String>,
}

impl Edits {
    /// Whether a flag other than `--title` was given: only these need a `waiting` PR before the lock.
    fn others(&self) -> bool {
        self.repo.is_some() || self.base.is_some() || self.needs.is_some()
    }
}

enum Decision {
    /// No value changes.
    Already(PrSpec),
    Update { before: PrSpec, after: PrSpec, fields: Vec<&'static str> },
}

pub(super) fn edit(args: EditArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    if args.repo.is_none() && args.base.is_none() && args.needs.is_none() && !args.no_needs && args.title.is_none() {
        return Err(CpdError::new(ErrorCode::NothingToEdit, "Nothing to edit: pass --repo, --base, --needs, --no-needs or --title."));
    }
    let needs = match (&args.needs, args.no_needs) {
        (Some(keys), _) => Some(sorted_needs(keys)?),
        (None, true) => Some(Vec::new()),
        (None, false) => None,
    };
    let key = args.key;
    let mut env = Env::open()?;

    // The phase, the PR and its status first; then git on the new repo and base.
    let wants_others = args.repo.is_some() || args.base.is_some() || needs.is_some();
    let wants_title = args.title.is_some();
    let stored = env.store.read(|conn| editable_pr(conn, &plan, &key, wants_title, wants_others))?.0;
    let repo = args.repo.as_deref().map(main_worktree).transpose()?;
    if repo.is_some() || args.base.is_some() {
        let in_repo = repo.clone().unwrap_or_else(|| stored.repo.clone().into());
        resolve_base(&in_repo, Some(args.base.as_deref().unwrap_or(&stored.base)))?;
    }
    let edits = Edits { repo: repo.as_ref().map(|r| r.display().to_string()), base: args.base, needs, title: args.title.map(|t| t.to_string()) };

    let decision = env.store.read(|conn| decide(conn, &plan, &key, &edits))?;
    let (pr, fields, already) = match decision {
        Decision::Already(pr) => (pr, Vec::new(), true),
        Decision::Update { before, after, fields } => {
            if after.repo != before.repo {
                no_branch_clash(repo.as_deref().expect("the repo changed, so it was given"), &plan, &key)?;
            }
            if let Some(title) = edits.title.as_deref().filter(|_| fields.contains(&"title")) {
                retitle_open_pull_request(&mut env, &plan, &key, title)?;
            }
            env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, &edits)? {
                Decision::Already(pr) => Ok((pr, Vec::new(), true)),
                Decision::Update { before, after, fields } => {
                    update(tx, &plan, &before, &after, edits.title.as_deref())?;
                    let text = format!("PR {key} of Plan {plan} edited ({}). Next: cpd plan render --plan {plan}", fields.join(", "));
                    append(tx, NewEvent::new(Kind::PrEdited, text, json!({"pr": key, "fields": fields})).plan(plan.clone()))?;
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
            "SELECT p.repo, p.number FROM prs r JOIN pull_requests p ON p.id = r.pull_request
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
/// `--title`), the Plan is in `grounding` or `plan` (`pr_locked`) and the PR `waiting`
/// (`illegal_transition`); `--title` alone meets neither rule.
fn editable_pr(conn: &Connection, plan: &PlanId, key: &PrKey, title: bool, others: bool) -> Result<(PrSpec, Option<String>), CpdError> {
    let prs = load_prs(conn, plan)?;
    let pr = find_pr(&prs, plan, key)?;
    if title && matches!(pr.status.as_str(), "merged" | "cancelled") {
        return Err(CpdError::new(ErrorCode::PrEnded, format!("PR {key} of Plan {plan} is {}: its title is history.", pr.status))
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
            .with("status", pr.status.as_str()));
    }
    if others {
        let phase = plan_phase(conn, plan)?;
        if !matches!(phase, Phase::Grounding | Phase::Plan) {
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
    let stored = conn.query_row("SELECT title FROM prs WHERE plan = ?1 AND key = ?2", [plan.as_str(), key.as_str()], |r| r.get(0))?;
    Ok((pr.clone(), stored))
}

fn decide(conn: &Connection, plan: &PlanId, key: &PrKey, edits: &Edits) -> Result<Decision, CpdError> {
    let (before, stored_title) = editable_pr(conn, plan, key, edits.title.is_some(), edits.others())?;
    let mut after = before.clone();
    if let Some(repo) = &edits.repo {
        after.repo = repo.clone();
    }
    if let Some(base) = &edits.base {
        after.base = base.clone();
    }
    if let Some(needs) = &edits.needs {
        let prs = load_prs(conn, plan)?;
        check_needs(&prs, plan, Some(key), needs)?;
        after.needs = needs.clone();
        let others: Vec<PrSpec> = prs.into_iter().filter(|p| p.key != after.key).chain([after.clone()]).collect();
        if let Some(cycle) = cycle(&others) {
            return Err(CpdError::new(ErrorCode::DependencyCycle, format!("The new needs of {key} make a cycle: {}.", cycle.join(" → ")))
                .with("plan", plan.as_str())
                .with("cycle", cycle));
        }
    }
    let title_changed = edits.title.as_ref().is_some_and(|t| stored_title.as_ref() != Some(t));
    let fields: Vec<&'static str> = [
        ("repo", before.repo != after.repo),
        ("base", before.base != after.base),
        ("needs", before.needs != after.needs),
        ("title", title_changed),
    ]
    .into_iter()
        .filter_map(|(name, changed)| changed.then_some(name))
        .collect();
    if fields.is_empty() {
        return Ok(Decision::Already(before));
    }
    Ok(Decision::Update { before, after, fields })
}

/// Writes the changed `prs` columns, and deletes and inserts `pr_needs` rows
/// to match (§4.4 allows deletion in `grounding` and `plan`). `title` is the new
/// title when it was given (a PR that has it already is not touched).
fn update(tx: &Connection, plan: &PlanId, before: &PrSpec, after: &PrSpec, title: Option<&str>) -> Result<(), CpdError> {
    if let Some(title) = title {
        tx.execute(
            "UPDATE prs SET title = ?1 WHERE plan = ?2 AND key = ?3 AND title IS NOT ?1 AND status NOT IN ('merged', 'cancelled')",
            [title, plan.as_str(), after.key.as_str()],
        )?;
    }
    if (&before.repo, &before.base) != (&after.repo, &after.base) {
        tx.execute(
            "UPDATE prs SET repo = ?1, base = ?2 WHERE plan = ?3 AND key = ?4 AND status = 'waiting'",
            [after.repo.as_str(), after.base.as_str(), plan.as_str(), after.key.as_str()],
        )?;
    }
    for need in before.needs.iter().filter(|n| !after.needs.contains(n)) {
        tx.execute("DELETE FROM pr_needs WHERE plan = ?1 AND pr = ?2 AND needs = ?3", [plan.as_str(), after.key.as_str(), need.as_str()])?;
    }
    for need in after.needs.iter().filter(|n| !before.needs.contains(n)) {
        tx.execute("INSERT INTO pr_needs (plan, pr, needs) VALUES (?1, ?2, ?3)", [plan.as_str(), after.key.as_str(), need.as_str()])?;
    }
    Ok(())
}

/// A cycle among the needs of the PRs that are not `cancelled`, as the keys
/// along it (the first repeated at the end), if there is one.
fn cycle(prs: &[PrSpec]) -> Option<Vec<String>> {
    let graph: BTreeMap<&str, &PrSpec> = prs.iter().filter(|p| p.status != "cancelled").map(|p| (p.key.as_str(), p)).collect();
    let mut done: Vec<&str> = Vec::new();
    for start in graph.keys() {
        if let Some(found) = visit(start, &graph, &mut done, &mut Vec::new()) {
            return Some(found);
        }
    }
    None
}

fn visit<'a>(key: &'a str, graph: &BTreeMap<&'a str, &'a PrSpec>, done: &mut Vec<&'a str>, path: &mut Vec<&'a str>) -> Option<Vec<String>> {
    if let Some(at) = path.iter().position(|k| *k == key) {
        let mut found: Vec<String> = path[at..].iter().map(|k| (*k).to_owned()).collect();
        found.push(key.to_owned());
        return Some(found);
    }
    if done.contains(&key) {
        return None;
    }
    let pr = graph.get(key)?;
    path.push(key);
    for need in &pr.needs {
        if let Some(found) = visit(need, graph, done, path) {
            return Some(found);
        }
    }
    path.pop();
    done.push(key);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(key: &str, status: &str, needs: &[&str]) -> PrSpec {
        PrSpec {
            key: key.into(),
            repo: "/r".into(),
            base: "main".into(),
            needs: needs.iter().map(|n| (*n).to_owned()).collect(),
            status: status.into(),
        }
    }

    #[test]
    fn finds_a_cycle_among_the_live_prs_only() {
        assert_eq!(cycle(&[pr("a", "waiting", &[]), pr("b", "waiting", &["a"])]), None);
        let found = cycle(&[pr("a", "waiting", &["c"]), pr("b", "waiting", &["a"]), pr("c", "waiting", &["b"])]).unwrap();
        assert_eq!(found.first(), found.last());
        assert_eq!(found.len(), 4);
        assert_eq!(cycle(&[pr("a", "waiting", &["b"]), pr("b", "cancelled", &["a"])]), None);
    }
}
