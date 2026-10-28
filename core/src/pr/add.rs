//! `pr add` (commands.md §9.4, contract §3.8): adds a PR to the Plan,
//! `waiting`. Nothing is started here: no git write, no tmux. It also holds
//! what `pr edit` shares with it: the repo, the base, the PR references of
//! `--base <key>`, `--base <plan>/<key>` and `--needs` (contract §2.2 "A PR reference") with
//! their validation, and the branch checks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::{AddArgs, Env, gh, plan_phase};
use crate::config::Tier;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git::run::{branch_exists, has_origin, run};
use crate::git::worktree::list as worktrees;
use crate::ids::{JobKey, OwnerLogin, PlanId, PrKey, RepoName};
use crate::job::open;
use crate::policy::{Policy, PolicyKey, owner_mismatch, owner_unset, repo_key, resolve};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::PrRow;
use crate::plan::fingerprint::load_prs;
use crate::store::WriteTxn;

const COMMAND: &str = "pr add";

/// The shapes `pr add` writes (contract §2.2 "Job kinds"); `free` is only what the migration gave an older PR.
pub(super) const SHAPES: [&str; 3] = ["tdd", "docs", "config"];

/// One stub draft of a shape's template.
struct Stub {
    key: String,
    kind: &'static str,
    title: String,
    /// The stub it needs: the step before it.
    needs: Option<String>,
}

/// The stub drafts a PR of `shape` starts with (commands.md §9.4): `tdd` docs, red tests (needs docs) and
/// implementation (needs the tests); `docs` its docs; `config` one `work` Job. Every shape ends with the PR's opener stub
/// (`open-<pr>`, kind `open`, no needs): its Ready rule is derived (contract §2.2 "Job kinds", §3.2, hs-d573).
fn template(shape: &str, pr: &PrKey, title: &str) -> Vec<Stub> {
    let stub = |suffix: &str, kind: &'static str, prefix: &str, needs: Option<&str>| Stub {
        key: format!("{pr}-{suffix}"),
        kind,
        title: format!("{prefix}: {title}"),
        needs: needs.map(|n| format!("{pr}-{n}")),
    };
    let mut stubs = match shape {
        "tdd" => vec![
            stub("docs", "docs", "Docs", None),
            stub("tests", "test", "Red tests", Some("docs")),
            stub("impl", "impl", "Implement", Some("tests")),
        ],
        "docs" => vec![stub("docs", "docs", "Docs", None)],
        _ => vec![stub("config", "work", "Config", None)],
    };
    stubs.push(Stub { key: format!("open-{pr}"), kind: "open", title: format!("Open the pull request: {title}"), needs: None });
    stubs
}

/// The flags given, resolved.
struct Wanted {
    key: PrKey,
    repo: String,
    /// The repo's GitHub name, from its `origin` (§8.3).
    name: RepoName,
    base: BaseWanted,
    title: String,
    /// `--jira`: the PR's ticket.
    jira: Option<String>,
    /// `--shape` (§9.4): `tdd` by default.
    shape: String,
    /// `--needs` as given, as sorted PR references (`needs_refs`); `Some(vec![])` is `--no-needs`; `None`: the default.
    needs: Option<Vec<String>>,
    /// `--adopt`: the pull request to adopt, with `--confirm-author`.
    adopt: Option<Adopt>,
}

/// The pull request `--adopt` names, read from GitHub, and the author the Lead confirmed.
struct Adopt {
    pull: gh::Adoptable,
    confirm: Option<String>,
}

enum Decision {
    /// The PR exists with these values; its status and `lead_branch` as stored, and `appended`
    /// as the Event recorded it.
    Already { status: String, lead_branch: String, appended: bool, base: String, base_pr: Option<String>, needs: Vec<String> },
    /// The PR is new, in a Plan in this phase.
    Insert { phase: Phase, base: String, base_pr: Option<String>, needs: Vec<String> },
}

/// A PR reference (contract §2.2): a bare `<key>`, a PR of the Plan the command is about, or
/// `<plan>/<key>`, a PR of that Plan, the Plan's own included. Parsed once, at the flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrRef {
    plan: Option<PlanId>,
    key: PrKey,
}

impl FromStr for PrRef {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Self, CpdError> {
        match s.split_once('/') {
            Some((plan, key)) => Ok(PrRef { plan: Some(PlanId::parse(plan)?), key: PrKey::parse(key)? }),
            None => Ok(PrRef { plan: None, key: PrKey::parse(s)? }),
        }
    }
}

/// `needs` as the sorted references of `own`'s Plan: the bare key for a PR of the Plan however it was written, `<plan>/<key>`
/// for another Plan's (which `resolve_refs` refuses, `pr_other_plan`); a repeat is refused with `usage` (§9.4).
pub(super) fn needs_refs(own: &PlanId, needs: &[PrRef]) -> Result<Vec<String>, CpdError> {
    let mut refs: Vec<String> = needs
        .iter()
        .map(|n| match &n.plan {
            Some(plan) if plan != own => format!("{plan}/{}", n.key),
            _ => n.key.to_string(),
        })
        .collect();
    refs.sort();
    if let Some(pair) = refs.windows(2).find(|w| w[0] == w[1]) {
        return Err(CpdError::new(ErrorCode::Usage, format!("--needs lists {} twice.", pair[0])).with("pr", pair[0].as_str()));
    }
    Ok(refs)
}

/// What `--base` names: a branch (`None`: the default branch), or a base PR.
pub(super) enum BaseArg {
    Branch(Option<String>),
    Pr(PlanId, PrKey),
}

/// What the PR targets, once the git checks passed.
pub(super) enum BaseWanted {
    /// A branch, checked to exist (`resolve_base`).
    Branch(String),
    /// A base PR named as `<key>` or `<plan>/<key>`: its Lead Branch becomes the base.
    Pr(PlanId, PrKey),
}

/// Reads `--base` (§2.2 "A PR reference"): a base PR when it is a bare `<key>` of a PR of `plan` in `repo`
/// (it wins over a same-named branch), or has the form `<plan>/<key>` and a Plan `<plan>` exists; else a branch.
pub(super) fn classify_base(conn: &Connection, plan: &PlanId, repo: &Path, base: Option<&str>) -> Result<BaseArg, CpdError> {
    let Some(value) = base else { return Ok(BaseArg::Branch(None)) };
    if !value.contains('/')
        && let Ok(key) = PrKey::parse(value)
    {
        let repo = repo.display().to_string();
        if known_prs(conn, plan)?.iter().any(|q| q.key == key.as_str() && q.repo == repo) {
            return Ok(BaseArg::Pr(plan.clone(), key));
        }
    }
    if let Some((plan, key)) = value.split_once('/')
        && let (Ok(plan), Ok(key)) = (PlanId::parse(plan), PrKey::parse(key))
    {
        let exists: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM plans WHERE id = ?1)", [plan.as_str()], |r| r.get(0))?;
        if exists {
            return Ok(BaseArg::Pr(plan, key));
        }
    }
    Ok(BaseArg::Branch(Some(value.to_owned())))
}

/// The base of `--base` in `repo`: a branch must exist there (`base_not_found`), a base PR needs no
/// branch yet (its PR may be `waiting`).
pub(super) fn wanted_base(repo: &Path, arg: BaseArg) -> Result<BaseWanted, CpdError> {
    match arg {
        BaseArg::Branch(branch) => Ok(BaseWanted::Branch(resolve_base(repo, branch.as_deref())?)),
        BaseArg::Pr(plan, key) => Ok(BaseWanted::Pr(plan, key)),
    }
}

/// A PR of the Plan, as the reference checks read it.
#[derive(Clone, Debug)]
pub(super) struct Known {
    key: String,
    repo: String,
    base: String,
    lead_branch: String,
    status: String,
}

/// Every PR of `plan`.
pub(super) fn known_prs(conn: &Connection, plan: &PlanId) -> Result<Vec<Known>, CpdError> {
    let mut stmt = conn.prepare("SELECT key, repo, base, lead_branch, status FROM prs WHERE plan = ?1 ORDER BY key")?;
    let rows = stmt
        .query_map([plan.as_str()], |r| {
            Ok(Known { key: r.get(0)?, repo: r.get(1)?, base: r.get(2)?, lead_branch: r.get(3)?, status: r.get(4)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// The base PR of a PR of `repo` targeting `base` (§2.2 "The base PR"): derived, never stored.
pub(super) fn derived_base_pr<'a>(known: &'a [Known], repo: &str, base: &str) -> Option<&'a Known> {
    known.iter().find(|q| q.repo == repo && q.lead_branch == base)
}

/// The PRs of the Plan, not `cancelled`, stacked on the PR whose Lead Branch is `lead_branch` in `repo`.
pub(super) fn stacked_on(known: &[Known], repo: &str, lead_branch: &str) -> Vec<String> {
    known.iter().filter(|p| p.status != "cancelled" && p.repo == repo && p.base == lead_branch).map(|p| p.key.clone()).collect()
}

/// The PR being added or edited.
pub(super) struct Subject<'a> {
    pub plan: &'a PlanId,
    pub key: &'a PrKey,
    /// Its repo as it will be stored.
    pub repo: &'a str,
}

/// What [`resolve_refs`] settled.
pub(super) struct Resolved {
    /// The branch the pull request targets: a base PR's Lead Branch when stacked.
    pub base: String,
    /// The base PR as `<plan>/<key>`, derived from `base`.
    pub base_pr: Option<String>,
}

/// A reference naming another Plan (`pr_other_plan`): needs and base PRs are PRs of the PR's own Plan.
fn other_plan(subject: &Subject<'_>, plan: &str, key: &str) -> CpdError {
    CpdError::new(
        ErrorCode::PrOtherPlan,
        format!("PR {plan}/{key} is a PR of Plan {plan}, not of {}: needs and base PRs are PRs of the PR's own Plan.", subject.plan),
    )
    .with("pr", subject.key.as_str())
    .with("reference", format!("{plan}/{key}"))
    .with("plan", plan)
}

/// The validation of contract §2.2 "Validation of a reference" for the base and the needs of `subject`, and the
/// base it makes: the references are PRs of the PR's own Plan (`pr_other_plan`), exist (`base_pr_unknown`,
/// `needs_unknown`), are not `cancelled` (`needs_unavailable`), a base PR is not `merged` (`base_pr_unavailable`)
/// and is of the PR's repo (`base_pr_other_repo`); no PR is both base and need (`usage`); the graph of needs and
/// base PRs stays acyclic (`dependency_cycle`). `needs` are references of the Plan, as `needs_refs` makes them.
pub(super) fn resolve_refs(conn: &Connection, subject: &Subject<'_>, base: &BaseWanted, needs: &[String]) -> Result<Resolved, CpdError> {
    let plan = subject.plan;
    if let BaseWanted::Pr(base_plan, base_key) = base
        && base_plan != plan
    {
        return Err(other_plan(subject, base_plan.as_str(), base_key.as_str()));
    }
    for need in needs {
        if let Some((need_plan, key)) = need.split_once('/') {
            return Err(other_plan(subject, need_plan, key));
        }
    }
    let known = known_prs(conn, plan)?;
    let find = |key: &str| known.iter().find(|p| p.key == key);

    let (branch, base_pr) = match base {
        BaseWanted::Pr(_, base_key) => {
            let Some(pr) = find(base_key.as_str()) else {
                return Err(CpdError::new(ErrorCode::BasePrUnknown, format!("Plan {plan} has no PR {base_key} to stack on."))
                    .with("plan", plan.as_str())
                    .with("pr", base_key.as_str()));
            };
            check_base_pr_state(pr)?;
            if pr.repo != subject.repo {
                return Err(CpdError::new(
                    ErrorCode::BasePrOtherRepo,
                    format!("PR {} is in {}, not in {}: a PR stacks only on a PR of its repo.", pr.key, pr.repo, subject.repo),
                )
                .with("pr", pr.key.as_str())
                .with("repo", subject.repo)
                .with("base_repo", pr.repo.as_str()));
            }
            (pr.lead_branch.clone(), Some(pr))
        }
        BaseWanted::Branch(branch) => {
            // A branch that is already a Lead Branch of a PR of the Plan is stacked all the same.
            let derived = derived_base_pr(&known, subject.repo, branch);
            if let Some(pr) = derived {
                check_base_pr_state(pr)?;
            }
            (branch.clone(), derived)
        }
    };
    let base_key = base_pr.map(|p| p.key.clone());

    let unknown: Vec<String> = needs.iter().filter(|n| n.as_str() == subject.key.as_str() || find(n).is_none()).cloned().collect();
    if !unknown.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnknown, format!("Plan {plan} has no PR {} to need.", unknown.join(", ")))
            .with("plan", plan.as_str())
            .with("needs", unknown));
    }
    let unavailable: Vec<String> = needs.iter().filter(|n| find(n).is_some_and(|p| p.status == "cancelled")).cloned().collect();
    if !unavailable.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnavailable, format!("A PR cannot need the cancelled PR {}.", unavailable.join(", ")))
            .with("plan", plan.as_str())
            .with("needs", unavailable));
    }
    if let Some(key) = base_key.as_ref().filter(|b| needs.contains(b)) {
        return Err(CpdError::new(
            ErrorCode::Usage,
            format!("{key} is the base PR and a need: the base PR is already the implied need (this PR waits for it to open), so drop --needs {key}."),
        )
        .with("pr", key.as_str()));
    }

    let this = subject.key.to_string();
    let edges = edges(conn, plan, &known, &this)?;
    let mut out: Vec<String> = needs.to_vec();
    out.extend(base_key);
    if let Some(cycle) = find_cycle(&this, &out, &edges) {
        return Err(CpdError::new(ErrorCode::DependencyCycle, format!("The needs and base of {} make a cycle: {}.", subject.key, cycle.join(" → ")))
            .with("plan", plan.as_str())
            .with("cycle", cycle));
    }
    Ok(Resolved { base: branch, base_pr: base_pr.map(|p| format!("{plan}/{}", p.key)) })
}

/// A base PR is not `cancelled` or `merged` (`base_pr_unavailable`).
fn check_base_pr_state(pr: &Known) -> Result<(), CpdError> {
    if matches!(pr.status.as_str(), "cancelled" | "merged") {
        return Err(CpdError::new(
            ErrorCode::BasePrUnavailable,
            format!("PR {} is {}: there is nothing to stack on, name its base branch instead.", pr.key, pr.status),
        )
        .with("pr", pr.key.as_str())
        .with("status", pr.status.as_str()));
    }
    Ok(())
}

/// The edges of the graph of needs and base PRs of every PR of the Plan that is not `cancelled` (an edge from a PR to
/// each PR it needs and to its base PR), `except` the edges leaving that PR: they are the ones being set.
fn edges(conn: &Connection, plan: &PlanId, known: &[Known], except: &String) -> Result<BTreeMap<String, Vec<String>>, CpdError> {
    let live = |key: &String| known.iter().any(|p| p.status != "cancelled" && &p.key == key);
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT pr, needs FROM pr_needs WHERE plan = ?1")?;
    let rows: Vec<(String, String)> = stmt.query_map([plan.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    for (from, to) in rows {
        if &from != except && live(&from) && live(&to) {
            graph.entry(from).or_default().push(to);
        }
    }
    for p in known.iter().filter(|p| p.status != "cancelled" && &p.key != except) {
        if let Some(q) = derived_base_pr(known, &p.repo, &p.base).filter(|q| q.status != "cancelled") {
            graph.entry(p.key.clone()).or_default().push(q.key.clone());
        }
    }
    Ok(graph)
}

/// A path from `start` back to `start`, the PR's own `out` edges first and `edges` after, if there is one.
fn find_cycle(start: &String, out: &[String], edges: &BTreeMap<String, Vec<String>>) -> Option<Vec<String>> {
    fn visit(node: &String, start: &String, edges: &BTreeMap<String, Vec<String>>, seen: &mut Vec<String>, path: &mut Vec<String>) -> bool {
        path.push(node.clone());
        if node == start && path.len() > 1 {
            return true;
        }
        if !seen.contains(node) {
            seen.push(node.clone());
            for next in edges.get(node).into_iter().flatten() {
                if visit(next, start, edges, seen, path) {
                    return true;
                }
            }
        }
        path.pop();
        false
    }
    let mut seen = Vec::new();
    for next in out {
        let mut path = vec![start.clone()];
        if visit(next, start, edges, &mut seen, &mut path) {
            return Some(path);
        }
    }
    None
}

pub(super) fn add(args: AddArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let needs = if args.no_needs {
        Some(Vec::new())
    } else if args.needs.is_empty() {
        None
    } else {
        Some(needs_refs(&plan, &args.needs)?)
    };
    let mut env = Env::open()?;
    let open_tier = env.config.open_tier;
    env.store.read(|conn| check_plan(conn, &plan))?;
    let repo = main_worktree(&args.repo)?;
    let (name, policy_file, policy) = key_repo_policy(&mut env, &plan, &repo)?;
    let unconfigured = (args.shape == "tdd" && policy.test_commands.is_none()).then(|| tests_unconfigured(&repo.display().to_string(), &policy_file));
    let adopt = args.adopt.map(|number| gh::adoptable(&repo, number)).transpose()?;
    let base = match &adopt {
        Some(a) => BaseArg::Branch(Some(a.base.clone())),
        None => env.store.read(|conn| classify_base(conn, &plan, &repo, args.base.as_deref()))?,
    };
    // A branch that is a PR's Lead Branch of the Plan and repo is its base PR's: it needs no branch yet (that PR may be `waiting`).
    let lead_branch_of_pr = match &base {
        BaseArg::Branch(Some(branch)) => env.store.read(|conn| Ok(derived_base_pr(&known_prs(conn, &plan)?, &repo.display().to_string(), branch).is_some()))?,
        _ => false,
    };
    let wanted = Wanted {
        base: match base {
            BaseArg::Branch(Some(branch)) if lead_branch_of_pr => BaseWanted::Branch(branch),
            base => wanted_base(&repo, base)?,
        },
        adopt: adopt.map(|pull| Adopt { pull, confirm: args.confirm_author }),
        repo: repo.display().to_string(),
        name,
        title: args.title.to_string(),
        jira: args.jira.map(|j| j.to_string()),
        shape: args.shape,
        key: args.key,
        needs,
    };

    let head_ref = wanted.adopt.as_ref().map(|a| a.pull.head_ref.as_str());
    let (status, lead_branch, appended, base, base_pr, needs, already) = match env.store.read(|conn| decide(conn, &plan, &wanted, unconfigured.as_ref()))? {
        Decision::Already { status, lead_branch, appended, base, base_pr, needs } => (status, lead_branch, appended, base, base_pr, needs, true),
        Decision::Insert { .. } => {
            // git, outside any transaction.
            no_branch_clash(&repo, &plan, &pr_branch(&plan, &wanted.key))?;
            env.store.write(COMMAND, |tx| match decide(tx, &plan, &wanted, None)? {
                Decision::Already { status, lead_branch, appended, base, base_pr, needs } => {
                    Ok((status, lead_branch, appended, base, base_pr, needs, true))
                }
                Decision::Insert { phase, base, base_pr, needs } => {
                    let appended = matches!(phase, Phase::Dispatch | Phase::Deploy | Phase::Done);
                    let lead_branch = pr_branch(&plan, &wanted.key);
                    let key = &wanted.key;
                    if matches!(phase, Phase::Deploy | Phase::Done) {
                        reopen(tx, &plan, phase, key)?;
                    }
                    insert(tx, &plan, &wanted, &base, &needs, &lead_branch)?;
                    let stubs = insert_stubs(tx, &plan, &wanted, appended, open_tier)?;
                    let with = if needs.is_empty() { String::new() } else { format!(", needs {}", needs.join(", ")) };
                    let text = format!(
                        "PR {key} added to Plan {plan} (repo {}, base {base}{with}), waiting. Next: cpd plan show --plan {plan}",
                        wanted.repo
                    );
                    let event = NewEvent::pr_added(
                        text,
                        key,
                        &wanted.repo,
                        &base,
                        &wanted.title,
                        head_ref,
                        wanted.adopt.as_ref().map(|a| a.pull.author.as_str()),
                        &needs,
                        appended,
                    );
                    append(tx, event.plan(plan.clone()))?;
                    for stub in &stubs {
                        let text = format!("Job {} added to Plan {plan} as draft (the {} template of PR {key}). Next: cpd job edit {} --tier <T> --spec <file> --plan {plan}", stub.key, wanted.shape, stub.key);
                        let needs: Vec<&String> = stub.needs.iter().collect();
                        let event = if stub.kind == "open" {
                            NewEvent::job_added_by_cpd(text, key, stub.kind, "draft", &open_tier.to_string(), open::BY_TEMPLATE, None)
                        } else {
                            NewEvent::job_added(text, key, stub.kind, "draft", appended, None, needs, false, false)
                        };
                        append(tx, event.plan(plan.clone()).job(JobKey::parse(&stub.key)?))?;
                    }
                    Ok(("waiting".to_owned(), lead_branch, appended, base, base_pr, needs, false))
                }
            })?
        }
    };
    let jira = env.store.read(|conn| Ok(conn.query_row("SELECT jira FROM prs WHERE plan = ?1 AND key = ?2", [plan.as_str(), wanted.key.as_str()], |r| r.get::<_, Option<String>>(0))?))?;
    let mut out = json!({"pr": {
        "key": wanted.key, "repo": wanted.repo, "name": wanted.name, "base": base, "base_pr": base_pr, "title": wanted.title, "jira": jira, "needs": needs,
        "status": status, "lead_branch": lead_branch, "appended": appended, "shape": wanted.shape, "head_ref": head_ref,
        "jobs": template(&wanted.shape, &wanted.key, &wanted.title).iter().map(|s| s.key.clone()).collect::<Vec<_>>(),
    }});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// Keys the repo (contract §8.3, hs-d288), in the order of commands.md §9.4: the Plan's Context
/// has an owner (`owner_unset`), the repo keys from its `origin` (`repo_unkeyed`), of that owner
/// (`owner_mismatch`), whose policy files resolve (`owner_unconfirmed`, `policy_invalid`). Returns the
/// repo's name. Shared with `pr edit --repo`; reads and writes nothing.
pub(super) fn key_repo(env: &mut Env, plan: &PlanId, repo: &Path) -> Result<RepoName, CpdError> {
    Ok(key_repo_policy(env, plan, repo)?.0)
}

/// [`key_repo`] with the repo's resolved policy and the path of its policy file.
fn key_repo_policy(env: &mut Env, plan: &PlanId, repo: &Path) -> Result<(RepoName, PathBuf, Policy), CpdError> {
    let (context, owner): (String, Option<String>) = env.store.read(|conn| {
        Ok(conn.query_row("SELECT c.id, c.owner FROM plans p JOIN contexts c ON c.id = p.context WHERE p.id = ?1", [plan.as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?)
    })?;
    let Some(owner) = owner else {
        return Err(owner_unset(&crate::ids::ContextId::parse(&context)?));
    };
    let owner = OwnerLogin::parse(&owner)?;
    let (repo_owner, name) = repo_key(repo)?;
    if repo_owner != owner {
        return Err(owner_mismatch(&owner, &repo_owner));
    }
    let file = crate::policy::repo_path(&env.home, &owner, &name);
    let policy = resolve(&env.home, &PolicyKey::Repo(owner, name.clone()))?.policy;
    Ok((name, file, policy))
}

/// `pr add` is for every phase (D1), but not for an archived Plan (`archived`); returns the phase.
fn check_plan(conn: &Connection, plan: &PlanId) -> Result<Phase, CpdError> {
    let phase = plan_phase(conn, plan)?;
    let archived: bool = conn.query_row("SELECT archived_at IS NOT NULL FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0))?;
    if archived {
        return Err(CpdError::new(ErrorCode::Archived, format!("Plan {plan} is archived: unarchive it first."))
            .with("plan", plan.as_str())
            .with("next", format!("cpd plan unarchive --plan {plan}")));
    }
    Ok(phase)
}

/// `unconfigured`: the refusal of a `tdd` PR whose repo policy has no `[test-commands]`; the first decision checks it, the one inside the write transaction does not.
fn decide(conn: &Connection, plan: &PlanId, wanted: &Wanted, unconfigured: Option<&CpdError>) -> Result<Decision, CpdError> {
    let phase = check_plan(conn, plan)?;
    let prs = load_prs(conn, plan)?;
    if let Some(existing) = prs.iter().find(|p| p.key == wanted.key) {
        // `appended`: the PR came after the lock.
        let (lead_branch, appended, title) = conn.query_row(
            "SELECT r.lead_branch, coalesce(p.locked_at IS NOT NULL AND r.created_at >= p.locked_at, 0), r.title
               FROM prs r JOIN plans p ON p.id = r.plan WHERE r.plan = ?1 AND r.key = ?2",
            [plan.as_str(), wanted.key.as_str()],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, Option<String>>(2)?)),
        )?;
        if same(existing, title.as_deref(), wanted, &prs, &base_branch(conn, &wanted.base)?) {
            let base_pr = derived_base_pr(&known_prs(conn, plan)?, &existing.repo, &existing.base).map(|q| format!("{plan}/{}", q.key));
            return Ok(Decision::Already {
                status: existing.status.clone(),
                lead_branch,
                appended,
                base: existing.base.clone(),
                base_pr,
                needs: existing.needs.clone(),
            });
        }
        return Err(CpdError::new(ErrorCode::PrExists, format!("Plan {plan} already has a PR {} with other flags.", wanted.key))
            .with("plan", plan.as_str())
            .with("pr", wanted.key.as_str())
            .with("repo", existing.repo.as_str())
            .with("base", existing.base.as_str())
            .with("title", title)
            .with("needs", existing.needs.clone())
            .with("shape", existing.shape.as_str())
            .with("head_ref", existing.head_ref.clone())
            .with("status", existing.status.as_str()));
    }
    // Stacked (§2.2 "The base PR"): `--base` names a base PR, or a branch that is a PR's Lead Branch; its base is its link.
    let stacked = match &wanted.base {
        BaseWanted::Pr(..) => true,
        BaseWanted::Branch(branch) => derived_base_pr(&known_prs(conn, plan)?, &wanted.repo, branch).is_some(),
    };
    let needs = match &wanted.needs {
        Some(needs) => needs.clone(),
        None if matches!(phase, Phase::Deploy | Phase::Done) => merged_keys(&prs),
        None if phase == Phase::Dispatch && stacked => Vec::new(),
        None if phase == Phase::Dispatch => {
            return Err(CpdError::new(
                ErrorCode::NeedsRequired,
                format!("Plan {plan} is in dispatch: say --needs <pr> to link PR {} to the PR it waits on (merged), --base <pr> to stack it on a PR it waits on to open, or --no-needs. A PR with no needs is started at once.", wanted.key),
            )
            .with("plan", plan.as_str())
            .with("pr", wanted.key.as_str())
            .with("phase", phase.as_str()));
        }
        None => Vec::new(),
    };
    let resolved = resolve_refs(conn, &Subject { plan, key: &wanted.key, repo: &wanted.repo }, &wanted.base, &needs)?;
    if let Some(error) = unconfigured {
        return Err(error.clone());
    }
    check_template_keys_free(conn, plan, wanted)?;
    if let Some(adopt) = &wanted.adopt {
        check_adoptable(conn, wanted, adopt)?;
    }
    Ok(Decision::Insert { phase, base: resolved.base, base_pr: resolved.base_pr, needs })
}

/// The refusals of `--adopt` (contract §3.8), in order, exit 3: the pull request is open, not from a fork, not a cpd one, its head free of
/// every live PR, and its author the User or confirmed.
fn check_adoptable(conn: &Connection, wanted: &Wanted, adopt: &Adopt) -> Result<(), CpdError> {
    let pull = &adopt.pull;
    let number = pull.number;
    if pull.state != "OPEN" {
        return Err(CpdError::new(ErrorCode::AdoptNotOpen, format!("Pull request #{number} is {}: only an open pull request is adopted.", pull.state))
            .with("number", number)
            .with("url", pull.url.as_str())
            .with("state", pull.state.as_str()));
    }
    if pull.fork {
        return Err(CpdError::new(ErrorCode::AdoptFork, format!("Pull request #{number} comes from a fork: cpd cannot push to a fork."))
            .with("number", number)
            .with("url", pull.url.as_str()));
    }
    let is_lead_branch: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM prs WHERE repo = ?1 AND lead_branch = ?2)",
        [wanted.repo.as_str(), pull.head_ref.as_str()],
        |r| r.get(0),
    )?;
    if is_lead_branch {
        return Err(CpdError::new(ErrorCode::AdoptCpdBranch, format!("Pull request #{number} has the head branch {}: it is a cpd pull request.", pull.head_ref))
            .with("number", number)
            .with("head_ref", pull.head_ref.as_str()));
    }
    let holder: Option<(String, String)> = conn
        .query_row(
            "SELECT plan, key FROM prs WHERE repo = ?1 AND head_ref = ?2 AND status IN ('waiting', 'dispatch', 'open')",
            [wanted.repo.as_str(), pull.head_ref.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((plan, pr)) = holder {
        return Err(CpdError::new(
            ErrorCode::AdoptTaken,
            format!("The head branch {} is already adopted by PR {pr} of Plan {plan}.", pull.head_ref),
        )
        .with("head_ref", pull.head_ref.as_str())
        .with("plan", plan)
        .with("pr", pr));
    }
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    if !same(&pull.author, &pull.user) && !adopt.confirm.as_deref().is_some_and(|c| same(c, &pull.author)) {
        return Err(CpdError::new(
            ErrorCode::AdoptForeign,
            format!(
                "Pull request #{number} is {}'s, not the User's ({}). Ask the User first; once they agree, run again with --confirm-author {}.",
                pull.author, pull.user, pull.author
            ),
        )
        .with("number", number)
        .with("url", pull.url.as_str())
        .with("author", pull.author.as_str())
        .with("user", pull.user.as_str())
        .with("confirm", format!("--confirm-author {}", pull.author)));
    }
    Ok(())
}

/// `tests_unconfigured` (contract §7.7): a `tdd` PR needs the repo's policy file (`file`) to have `[test-commands]`.
fn tests_unconfigured(repo: &str, file: &Path) -> CpdError {
    CpdError::new(
        ErrorCode::TestsUnconfigured,
        format!(
            "{repo} has no [test-commands] in its policy file {}, and a tdd PR needs it to check its test and impl Jobs. Add --shape docs or --shape config for a PR with no behaviour, or ask the Lead to add the table (contract §7.7).",
            file.display()
        ),
    )
    .with("repo", repo)
    .with("file", file.display().to_string())
}

/// None of the Job keys the shape lays out exists in the Plan (`job_key_exists`, `details.jobs`, §9.4).
fn check_template_keys_free(conn: &Connection, plan: &PlanId, wanted: &Wanted) -> Result<(), CpdError> {
    let mut taken: Vec<String> = Vec::new();
    for stub in template(&wanted.shape, &wanted.key, &wanted.title) {
        let exists: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM jobs WHERE plan = ?1 AND key = ?2)", [plan.as_str(), stub.key.as_str()], |r| r.get(0))?;
        if exists {
            taken.push(stub.key);
        }
    }
    if taken.is_empty() {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::JobKeyExists,
        format!("Plan {plan} already has the Job {}, which the {} template of PR {} would add.", taken.join(", "), wanted.shape, wanted.key),
    )
    .with("plan", plan.as_str())
    .with("pr", wanted.key.as_str())
    .with("jobs", taken))
}

/// The branch `base` names, before any validation: a base PR is looked up (`base_pr_unknown`).
fn base_branch(conn: &Connection, base: &BaseWanted) -> Result<String, CpdError> {
    match base {
        BaseWanted::Branch(branch) => Ok(branch.clone()),
        BaseWanted::Pr(plan, key) => known_prs(conn, plan)?
            .into_iter()
            .find(|p| p.key == key.as_str())
            .map(|p| p.lead_branch)
            .ok_or_else(|| {
                CpdError::new(ErrorCode::BasePrUnknown, format!("Plan {plan} has no PR {key} to stack on."))
                    .with("plan", plan.as_str())
                    .with("pr", key.as_str())
            }),
    }
}

/// The keys of the `merged` PRs, in key order (A3).
fn merged_keys(prs: &[PrRow]) -> Vec<String> {
    let mut keys: Vec<String> = prs.iter().filter(|p| p.status == "merged").map(|p| p.key.as_str().to_owned()).collect();
    keys.sort();
    keys
}

/// Without `--needs` or `--no-needs` the needs match when none, or when all merged: a rerun
/// after the reopen sees the Plan in `dispatch`, where the default is none. `title` is the
/// stored one (null for a PR that existed before migration `pr-title`: it never matches).
fn same(existing: &PrRow, title: Option<&str>, wanted: &Wanted, prs: &[PrRow], base: &str) -> bool {
    let needs_match = match &wanted.needs {
        Some(needs) => &existing.needs == needs,
        None => existing.needs.iter().all(|n| merged_keys(prs).contains(n)),
    };
    (existing.repo.as_str(), existing.base.as_str(), existing.shape.as_str()) == (wanted.repo.as_str(), base, wanted.shape.as_str())
        && title == Some(wanted.title.as_str())
        && wanted.jira.as_ref().is_none_or(|j| existing.jira.as_ref() == Some(j))
        && existing.head_ref.as_deref() == wanted.adopt.as_ref().map(|a| a.pull.head_ref.as_str())
        && needs_match
}

fn insert(tx: &Connection, plan: &PlanId, wanted: &Wanted, base: &str, needs: &[String], lead_branch: &str) -> Result<(), CpdError> {
    tx.execute(
        "INSERT INTO prs (plan, key, repo, name, base, title, shape, status, lead_branch, head_ref, jira, created_txn, created_at)
         SELECT ?1, ?2, ?3, ?6, ?4, ?7, ?8, 'waiting', ?5, ?9, ?10, id, at FROM txns WHERE id = cpd_txn()",
        rusqlite::params![
            plan.as_str(),
            wanted.key.as_str(),
            wanted.repo,
            base,
            lead_branch,
            wanted.name.as_str(),
            wanted.title,
            wanted.shape,
            wanted.adopt.as_ref().map(|a| a.pull.head_ref.as_str()),
            wanted.jira
        ],
    )?;
    insert_needs(tx, plan, &wanted.key, needs)
}

/// The shape's stub drafts (§9.4): `jobs` rows with a title and a kind and no Tier and no Spec, `draft`, no
/// fingerprint, `appended` as `job add --draft` writes it, and the `dependencies` of the template. The opener stub is
/// the exception: it carries the fixed Spec of [`open::spec`] and the config's `open_tier`, and no Dependency rows.
fn insert_stubs(tx: &Connection, plan: &PlanId, wanted: &Wanted, appended: bool, open_tier: Tier) -> Result<Vec<Stub>, CpdError> {
    let stubs = template(&wanted.shape, &wanted.key, &wanted.title);
    let pr = load_prs(tx, plan)?.into_iter().find(|p| p.key == wanted.key).ok_or_else(|| {
        CpdError::bad_row(format!("PR {} of Plan {plan} vanished while its template was being written", wanted.key))
    })?;
    for stub in &stubs {
        if stub.kind == "open" {
            let spec = open::spec(&pr);
            tx.execute(
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, kind, created_txn, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'draft', ?11, 'open', cpd_txn(), (SELECT at FROM txns WHERE id = cpd_txn()))",
                rusqlite::params![
                    plan.as_str(),
                    wanted.key.as_str(),
                    stub.key,
                    stub.title,
                    open_tier.to_string(),
                    spec.goal,
                    spec.context,
                    spec.touches.to_string(),
                    spec.acceptance.to_string(),
                    spec.out_of_scope.to_string(),
                    appended,
                ],
            )?;
            continue;
        }
        tx.execute(
            "INSERT INTO jobs (plan, pr, key, title, status, appended, kind, created_txn, created_at)
             VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6, cpd_txn(), (SELECT at FROM txns WHERE id = cpd_txn()))",
            rusqlite::params![plan.as_str(), wanted.key.as_str(), stub.key, stub.title, appended, stub.kind],
        )?;
        if let Some(need) = &stub.needs {
            tx.execute("INSERT INTO dependencies (plan, job, needs) VALUES (?1, ?2, ?3)", [plan.as_str(), stub.key.as_str(), need.as_str()])?;
        }
    }
    Ok(stubs)
}

/// One `pr_needs` row per need.
pub(super) fn insert_needs(tx: &Connection, plan: &PlanId, pr: &PrKey, needs: &[String]) -> Result<(), CpdError> {
    for need in needs {
        tx.execute("INSERT INTO pr_needs (plan, pr, needs) VALUES (?1, ?2, ?3)", [plan.as_str(), pr.as_str(), need.as_str()])?;
    }
    Ok(())
}

/// In `deploy` or `done` the Plan goes back to `dispatch` (D1): the phase is compared and set, then
/// `plan-phase` is appended (before `pr-added`).
fn reopen(tx: &WriteTxn<'_>, plan: &PlanId, from: Phase, key: &PrKey) -> Result<(), CpdError> {
    let moved = tx.execute("UPDATE plans SET phase = 'dispatch', done_at = NULL WHERE id = ?1 AND phase = ?2", [plan.as_str(), from.as_str()])?;
    if moved == 0 {
        return Err(CpdError::new(ErrorCode::CasLost, format!("Plan {plan} left phase {} meanwhile: rerun the command.", from.as_str()))
            .with("plan", plan.as_str()));
    }
    let text = format!("Plan {plan} is back in dispatch: PR {key} was added.");
    append(tx, NewEvent::plan_phase(text, from.as_str(), "dispatch").plan(plan.clone()))?;
    Ok(())
}

/// The Lead Branch of a PR made now (contract §2.1): `<plan>/<key>`, without the `cpd/` prefix.
fn pr_branch(plan: &PlanId, key: &PrKey) -> String {
    format!("{plan}/{key}")
}

/// git's own rule over the Lead Branch `lead_branch` of a PR of `plan` (contract §3.8, pro-d113 A4): neither the branch
/// nor its parent path exists in the repo, else `branch_exists` (exit 3). The parent path is the branch without its last
/// `/<segment>` (`<plan>` for `<plan>/<key>`, `cpd/<plan>` for `cpd/<plan>/<key>`): git cannot hold both. The legacy
/// `cpd/<plan>` has no parent check.
pub(super) fn no_branch_clash(repo: &Path, plan: &PlanId, lead_branch: &str) -> Result<(), CpdError> {
    let mut branches = vec![lead_branch];
    if lead_branch != format!("cpd/{plan}")
        && let Some((parent, _)) = lead_branch.rsplit_once('/')
    {
        branches.push(parent);
    }
    for branch in branches {
        if branch_exists(repo, branch)? {
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
