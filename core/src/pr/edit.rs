//! `pr edit` (commands.md §9.5, contract §3.8): changes a `waiting` PR's repo,
//! base or needs before the lock. The plan fingerprint changes with them.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde_json::json;

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
}

enum Decision {
    /// No value changes.
    Already(PrSpec),
    Update { before: PrSpec, after: PrSpec, fields: Vec<&'static str> },
}

pub(super) fn edit(args: EditArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    if args.repo.is_none() && args.base.is_none() && args.needs.is_none() && !args.no_needs {
        return Err(CpdError::new(ErrorCode::NothingToEdit, "Nothing to edit: pass --repo, --base, --needs or --no-needs."));
    }
    let needs = match (&args.needs, args.no_needs) {
        (Some(keys), _) => Some(sorted_needs(keys)?),
        (None, true) => Some(Vec::new()),
        (None, false) => None,
    };
    let key = args.key;
    let mut env = Env::open()?;

    // The phase, the PR and its status first; then git on the new repo and base.
    let stored = env.store.read(|conn| waiting_pr(conn, &plan, &key))?;
    let repo = args.repo.as_deref().map(main_worktree).transpose()?;
    if repo.is_some() || args.base.is_some() {
        let in_repo = repo.clone().unwrap_or_else(|| stored.repo.clone().into());
        resolve_base(&in_repo, Some(args.base.as_deref().unwrap_or(&stored.base)))?;
    }
    let edits = Edits { repo: repo.as_ref().map(|r| r.display().to_string()), base: args.base, needs };

    let decision = env.store.read(|conn| decide(conn, &plan, &key, &edits))?;
    let (pr, fields, already) = match decision {
        Decision::Already(pr) => (pr, Vec::new(), true),
        Decision::Update { before, after, .. } => {
            if after.repo != before.repo {
                no_branch_clash(repo.as_deref().expect("the repo changed, so it was given"), &plan, &key)?;
            }
            env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, &edits)? {
                Decision::Already(pr) => Ok((pr, Vec::new(), true)),
                Decision::Update { before, after, fields } => {
                    update(tx, &plan, &before, &after)?;
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

/// The PR `key`, which must be editable: the Plan in `grounding` or `plan`
/// (`pr_locked`), the PR `waiting` (`illegal_transition`).
fn waiting_pr(conn: &Connection, plan: &PlanId, key: &PrKey) -> Result<PrSpec, CpdError> {
    let prs = load_prs(conn, plan)?;
    let pr = find_pr(&prs, plan, key)?;
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
    Ok(pr.clone())
}

fn decide(conn: &Connection, plan: &PlanId, key: &PrKey, edits: &Edits) -> Result<Decision, CpdError> {
    let before = waiting_pr(conn, plan, key)?;
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
    let fields: Vec<&'static str> = [("repo", before.repo != after.repo), ("base", before.base != after.base), ("needs", before.needs != after.needs)]
        .into_iter()
        .filter_map(|(name, changed)| changed.then_some(name))
        .collect();
    if fields.is_empty() {
        return Ok(Decision::Already(before));
    }
    Ok(Decision::Update { before, after, fields })
}

/// Writes the changed `prs` columns, and deletes and inserts `pr_needs` rows
/// to match (§4.4 allows deletion in `grounding` and `plan`).
fn update(tx: &Connection, plan: &PlanId, before: &PrSpec, after: &PrSpec) -> Result<(), CpdError> {
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
