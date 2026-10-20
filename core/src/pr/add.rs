//! `pr add` (commands.md §9.4, contract §3.8): adds a PR to the Plan,
//! `waiting`. Nothing is started here: no git write, no tmux. It also holds
//! what `pr edit` shares with it: the repo, the base, the PR references of
//! `--base <plan>/<key>` and `--needs` (contract §2.2 "A PR reference") with
//! their validation, and the branch checks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use rusqlite::Connection;
use serde_json::json;

use super::{AddArgs, Env, plan_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git::run::{branch_exists, delete_remote_branch, has_origin, ok, remote_has_branch, run};
use crate::git::worktree::list as worktrees;
use crate::ids::{JobKey, OwnerLogin, PlanId, PrKey, RepoName};
use crate::policy::{PolicyKey, owner_mismatch, owner_unset, repo_key, resolve};
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
/// implementation (needs the tests); `docs` its docs; `config` one `work` Job.
fn template(shape: &str, pr: &PrKey, title: &str) -> Vec<Stub> {
    let stub = |suffix: &str, kind: &'static str, prefix: &str, needs: Option<&str>| Stub {
        key: format!("{pr}-{suffix}"),
        kind,
        title: format!("{prefix}: {title}"),
        needs: needs.map(|n| format!("{pr}-{n}")),
    };
    match shape {
        "tdd" => vec![
            stub("docs", "docs", "Docs", None),
            stub("tests", "test", "Red tests", Some("docs")),
            stub("impl", "impl", "Implement", Some("tests")),
        ],
        "docs" => vec![stub("docs", "docs", "Docs", None)],
        _ => vec![stub("config", "work", "Config", None)],
    }
}

/// The flags given, resolved.
struct Wanted {
    key: PrKey,
    repo: String,
    /// The repo's GitHub name, from its `origin` (§8.3).
    name: RepoName,
    base: BaseWanted,
    title: String,
    /// `--shape` (§9.4): `tdd` by default.
    shape: String,
    /// `--needs` as given, as sorted PR references (`needs_refs`); `Some(vec![])` is `--no-needs`; `None`: the default.
    needs: Option<Vec<String>>,
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

/// A PR as `(plan, key)`.
type Node = (String, String);

/// A reference as the views print it (§2.2): the bare key in the Plan it is of, else `<plan>/<key>`.
pub(super) fn reference(own: &str, node: &Node) -> String {
    if node.0 == own { node.1.clone() } else { format!("{}/{}", node.0, node.1) }
}

/// `needs` as the sorted references of `own`'s Plan, a PR of the Plan in its bare form whichever way it was
/// written, refusing a repeat with `usage` (§9.4).
pub(super) fn needs_refs(own: &PlanId, needs: &[PrRef]) -> Result<Vec<String>, CpdError> {
    let mut refs: Vec<String> = needs
        .iter()
        .map(|n| reference(own.as_str(), &(n.plan.as_ref().unwrap_or(own).to_string(), n.key.to_string())))
        .collect();
    refs.sort();
    if let Some(pair) = refs.windows(2).find(|w| w[0] == w[1]) {
        return Err(CpdError::new(ErrorCode::Usage, format!("--needs lists {} twice.", pair[0])).with("pr", pair[0].as_str()));
    }
    Ok(refs)
}

/// A stored reference of `own`'s Plan as `(plan, key)`.
pub(super) fn node_of(own: &PlanId, reference: &str) -> Node {
    match reference.split_once('/') {
        Some((plan, key)) => (plan.to_owned(), key.to_owned()),
        None => (own.to_string(), reference.to_owned()),
    }
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
    /// A base PR named as `<plan>/<key>`: its Lead Branch becomes the base.
    Pr(PlanId, PrKey),
}

/// Reads `--base` (§2.2 "A PR reference"): a base PR exactly when it has the form `<plan>/<key>` and a
/// Plan `<plan>` exists, else a branch.
pub(super) fn classify_base(conn: &Connection, base: Option<&str>) -> Result<BaseArg, CpdError> {
    let Some(value) = base else { return Ok(BaseArg::Branch(None)) };
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

/// A PR of any Plan, as the reference checks read it.
#[derive(Clone, Debug)]
pub(super) struct Known {
    plan: String,
    key: String,
    repo: String,
    base: String,
    lead_branch: String,
    status: String,
    context: String,
    archived: bool,
}

impl Known {
    fn node(&self) -> Node {
        (self.plan.clone(), self.key.clone())
    }
}

/// Every PR of every Plan.
pub(super) fn known_prs(conn: &Connection) -> Result<Vec<Known>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT r.plan, r.key, r.repo, r.base, r.lead_branch, r.status, p.context, p.archived_at IS NOT NULL
           FROM prs r JOIN plans p ON p.id = r.plan ORDER BY r.plan, r.key",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Known {
                plan: r.get(0)?,
                key: r.get(1)?,
                repo: r.get(2)?,
                base: r.get(3)?,
                lead_branch: r.get(4)?,
                status: r.get(5)?,
                context: r.get(6)?,
                archived: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// The base PR of a PR of `repo` targeting `base` (§2.2 "The base PR"): derived, never stored.
pub(super) fn derived_base_pr<'a>(known: &'a [Known], repo: &str, base: &str) -> Option<&'a Known> {
    known.iter().find(|q| q.repo == repo && q.lead_branch == base)
}

/// The PRs, not `cancelled`, of any Plan stacked on the PR whose Lead Branch is `lead_branch` in `repo`,
/// as qualified references.
pub(super) fn stacked_on(known: &[Known], repo: &str, lead_branch: &str) -> Vec<String> {
    known
        .iter()
        .filter(|p| p.status != "cancelled" && p.repo == repo && p.base == lead_branch)
        .map(|p| format!("{}/{}", p.plan, p.key))
        .collect()
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

fn in_context(ctx: &str, pr: &Known, node: &Node) -> Result<(), CpdError> {
    if pr.context == ctx {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::PrOtherContext,
        format!("PR {}/{} is in Context {}, not in {ctx}: PRs reference only PRs of their Context.", node.0, node.1, pr.context),
    )
    .with("pr", format!("{}/{}", node.0, node.1))
    .with("context", ctx)
    .with("other_context", pr.context.as_str()))
}

fn archived(pr: &Known) -> CpdError {
    CpdError::new(ErrorCode::Archived, format!("Plan {} is archived: its PR {} cannot be referenced.", pr.plan, pr.key))
        .with("plan", pr.plan.as_str())
        .with("next", format!("cpd plan unarchive --plan {}", pr.plan))
}

/// The validation of contract §2.2 "Validation of a reference" for the base and the needs of `subject`, and the
/// base it makes: the references exist (`base_pr_unknown`, `needs_unknown`), are of the Plan's Context
/// (`pr_other_context`), are not of an archived Plan unless a `merged` need (`archived`), are not `cancelled`
/// (`needs_unavailable`), a base PR is not `merged` (`base_pr_unavailable`) and is of the PR's repo
/// (`base_pr_other_repo`); no PR is both base and need (`usage`); the graph of needs and base PRs stays
/// acyclic (`dependency_cycle`). `needs` are references of the Plan, as `needs_refs` makes them.
pub(super) fn resolve_refs(conn: &Connection, subject: &Subject<'_>, base: &BaseWanted, needs: &[String]) -> Result<Resolved, CpdError> {
    let plan = subject.plan;
    let known = known_prs(conn)?;
    let context: String = conn.query_row("SELECT context FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0))?;
    let find = |node: &Node| known.iter().find(|p| p.plan == node.0 && p.key == node.1);

    let (branch, base_pr) = match base {
        BaseWanted::Pr(base_plan, base_key) => {
            let node: Node = (base_plan.to_string(), base_key.to_string());
            let Some(pr) = find(&node) else {
                return Err(CpdError::new(ErrorCode::BasePrUnknown, format!("Plan {base_plan} has no PR {base_key} to stack on."))
                    .with("plan", base_plan.as_str())
                    .with("pr", base_key.as_str()));
            };
            in_context(&context, pr, &node)?;
            if pr.archived {
                return Err(archived(pr));
            }
            check_base_pr_state(pr)?;
            if pr.repo != subject.repo {
                return Err(CpdError::new(
                    ErrorCode::BasePrOtherRepo,
                    format!("PR {}/{} is in {}, not in {}: a PR stacks only on a PR of its repo.", pr.plan, pr.key, pr.repo, subject.repo),
                )
                .with("pr", format!("{}/{}", pr.plan, pr.key))
                .with("repo", subject.repo)
                .with("base_repo", pr.repo.as_str()));
            }
            (pr.lead_branch.clone(), Some(pr))
        }
        BaseWanted::Branch(branch) => {
            // A branch that is already a Lead Branch of a PR of the repo is stacked all the same.
            let derived = derived_base_pr(&known, subject.repo, branch);
            if let Some(pr) = derived {
                in_context(&context, pr, &pr.node())?;
                if pr.archived {
                    return Err(archived(pr));
                }
                check_base_pr_state(pr)?;
            }
            (branch.clone(), derived)
        }
    };
    let base_node = base_pr.map(Known::node);

    let own = plan.to_string();
    let need_nodes: Vec<Node> = needs.iter().map(|n| node_of(plan, n)).collect();
    let unknown: Vec<String> = need_nodes
        .iter()
        .filter(|n| (n.0 == own && n.1 == subject.key.as_str()) || find(n).is_none())
        .map(|n| reference(&own, n))
        .collect();
    if !unknown.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnknown, format!("Plan {plan} has no PR {} to need.", unknown.join(", ")))
            .with("plan", plan.as_str())
            .with("needs", unknown));
    }
    for node in &need_nodes {
        let pr = find(node).expect("known above");
        in_context(&context, pr, node)?;
        if pr.archived && pr.status != "merged" {
            return Err(archived(pr));
        }
    }
    let unavailable: Vec<String> =
        need_nodes.iter().filter(|n| find(n).is_some_and(|p| p.status == "cancelled")).map(|n| reference(&own, n)).collect();
    if !unavailable.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnavailable, format!("A PR cannot need the cancelled PR {}.", unavailable.join(", ")))
            .with("plan", plan.as_str())
            .with("needs", unavailable));
    }
    if let Some(node) = base_node.as_ref().filter(|b| need_nodes.contains(b)) {
        let reference = reference(&own, node);
        return Err(CpdError::new(
            ErrorCode::Usage,
            format!("{reference} is the base PR and a need: a need must be merged before this PR starts, a base PR only started."),
        )
        .with("pr", reference));
    }

    let this: Node = (own.clone(), subject.key.to_string());
    let edges = edges(conn, &known, &this)?;
    let mut out: Vec<Node> = need_nodes;
    out.extend(base_node);
    if let Some(cycle) = find_cycle(&this, &out, &edges) {
        let cycle: Vec<String> = cycle.iter().map(|n| reference(&own, n)).collect();
        return Err(CpdError::new(ErrorCode::DependencyCycle, format!("The needs and base of {} make a cycle: {}.", subject.key, cycle.join(" → ")))
            .with("plan", plan.as_str())
            .with("cycle", cycle));
    }
    Ok(Resolved { base: branch, base_pr: base_pr.map(|p| format!("{}/{}", p.plan, p.key)) })
}

/// A base PR is not `cancelled` or `merged` (`base_pr_unavailable`).
fn check_base_pr_state(pr: &Known) -> Result<(), CpdError> {
    if matches!(pr.status.as_str(), "cancelled" | "merged") {
        return Err(CpdError::new(
            ErrorCode::BasePrUnavailable,
            format!("PR {}/{} is {}: there is nothing to stack on, name its base branch instead.", pr.plan, pr.key, pr.status),
        )
        .with("pr", format!("{}/{}", pr.plan, pr.key))
        .with("status", pr.status.as_str()));
    }
    Ok(())
}

/// The edges of the graph of needs and base PRs of every PR that is not `cancelled` (an edge from a PR to
/// each PR it needs and to its base PR), `except` the edges leaving that PR: they are the ones being set.
fn edges(conn: &Connection, known: &[Known], except: &Node) -> Result<BTreeMap<Node, Vec<Node>>, CpdError> {
    let live = |node: &Node| known.iter().any(|p| p.status != "cancelled" && p.plan == node.0 && p.key == node.1);
    let mut graph: BTreeMap<Node, Vec<Node>> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT plan, pr, needs_plan, needs FROM pr_needs")?;
    let rows: Vec<(Node, Node)> = stmt
        .query_map([], |r| Ok(((r.get(0)?, r.get(1)?), (r.get(2)?, r.get(3)?))))?
        .collect::<rusqlite::Result<_>>()?;
    for (from, to) in rows {
        if &from != except && live(&from) && live(&to) {
            graph.entry(from).or_default().push(to);
        }
    }
    for p in known.iter().filter(|p| p.status != "cancelled" && &p.node() != except) {
        if let Some(q) = derived_base_pr(known, &p.repo, &p.base).filter(|q| q.status != "cancelled") {
            graph.entry(p.node()).or_default().push(q.node());
        }
    }
    Ok(graph)
}

/// A path from `start` back to `start`, the PR's own `out` edges first and `edges` after, if there is one.
fn find_cycle(start: &Node, out: &[Node], edges: &BTreeMap<Node, Vec<Node>>) -> Option<Vec<Node>> {
    fn visit(node: &Node, start: &Node, edges: &BTreeMap<Node, Vec<Node>>, seen: &mut Vec<Node>, path: &mut Vec<Node>) -> bool {
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
        Some(needs_refs(&plan, &args.needs)?)
    };
    let mut env = Env::open()?;
    env.store.read(|conn| check_plan(conn, &plan))?;
    let repo = main_worktree(&args.repo)?;
    let name = key_repo(&mut env, &plan, &repo)?;
    let base = env.store.read(|conn| classify_base(conn, args.base.as_deref()))?;
    let wanted = Wanted {
        base: wanted_base(&repo, base)?,
        repo: repo.display().to_string(),
        name,
        title: args.title.to_string(),
        shape: args.shape,
        key: args.key,
        needs,
    };

    let (status, lead_branch, appended, base, base_pr, needs, already) = match env.store.read(|conn| decide(conn, &plan, &wanted))? {
        Decision::Already { status, lead_branch, appended, base, base_pr, needs } => (status, lead_branch, appended, base, base_pr, needs, true),
        Decision::Insert { .. } => {
            // git, outside any transaction.
            let legacy = env.store.read(|conn| legacy_pr(conn, &plan))?;
            if let Some(legacy) = &legacy {
                move_legacy_branch(&plan, legacy)?;
            }
            no_branch_clash(&repo, &plan, &wanted.key)?;
            env.store.write(COMMAND, |tx| match decide(tx, &plan, &wanted)? {
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
                    if let Some(legacy) = &legacy {
                        rename_legacy(tx, &plan, legacy)?;
                    }
                    insert(tx, &plan, &wanted, &base, &needs, &lead_branch)?;
                    let stubs = insert_stubs(tx, &plan, &wanted, appended)?;
                    let with = if needs.is_empty() { String::new() } else { format!(", needs {}", needs.join(", ")) };
                    let text = format!(
                        "PR {key} added to Plan {plan} (repo {}, base {base}{with}), waiting. Next: cpd plan show --plan {plan}",
                        wanted.repo
                    );
                    let event = NewEvent::pr_added(text, key, &wanted.repo, &base, &wanted.title, &needs, appended);
                    append(tx, event.plan(plan.clone()))?;
                    for stub in &stubs {
                        let text = format!("Job {} added to Plan {plan} as draft (the {} template of PR {key}). Next: cpd job edit {} --tier <T> --spec <file> --plan {plan}", stub.key, wanted.shape, stub.key);
                        let needs: Vec<&String> = stub.needs.iter().collect();
                        let event = NewEvent::job_added(text, key, stub.kind, "draft", appended, None, needs, false, false);
                        append(tx, event.plan(plan.clone()).job(JobKey::parse(&stub.key)?))?;
                    }
                    Ok(("waiting".to_owned(), lead_branch, appended, base, base_pr, needs, false))
                }
            })?
        }
    };
    let mut out = json!({"pr": {
        "key": wanted.key, "repo": wanted.repo, "name": wanted.name, "base": base, "base_pr": base_pr, "title": wanted.title, "needs": needs,
        "status": status, "lead_branch": lead_branch, "appended": appended, "shape": wanted.shape,
        "jobs": template(&wanted.shape, &wanted.key, &wanted.title).iter().map(|s| s.key.clone()).collect::<Vec<_>>(),
    }});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// Keys the repo (contract §8.3, hs-d288), in the order of commands.md §9.4: the Plan's Context
/// has an owner (`owner_unset`), the repo keys from its `origin` (`repo_unkeyed`), of that owner
/// (`owner_mismatch`), whose policy files resolve (`owner_unknown`, `policy_invalid`). Returns the
/// repo's name. Shared with `pr edit --repo`; reads and writes nothing.
pub(super) fn key_repo(env: &mut Env, plan: &PlanId, repo: &Path) -> Result<RepoName, CpdError> {
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
    resolve(&env.home, &PolicyKey::Repo(owner, name.clone()))?;
    Ok(name)
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

fn decide(conn: &Connection, plan: &PlanId, wanted: &Wanted) -> Result<Decision, CpdError> {
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
            let base_pr = derived_base_pr(&known_prs(conn)?, &existing.repo, &existing.base).map(|q| format!("{}/{}", q.plan, q.key));
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
            .with("status", existing.status.as_str()));
    }
    let needs = match &wanted.needs {
        Some(needs) => needs.clone(),
        None if matches!(phase, Phase::Deploy | Phase::Done) => merged_keys(&prs),
        None => Vec::new(),
    };
    let resolved = resolve_refs(conn, &Subject { plan, key: &wanted.key, repo: &wanted.repo }, &wanted.base, &needs)?;
    check_template_keys_free(conn, plan, wanted)?;
    Ok(Decision::Insert { phase, base: resolved.base, base_pr: resolved.base_pr, needs })
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
        BaseWanted::Pr(plan, key) => known_prs(conn)?
            .into_iter()
            .find(|p| p.plan == plan.as_str() && p.key == key.as_str())
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
        && needs_match
}

fn insert(tx: &Connection, plan: &PlanId, wanted: &Wanted, base: &str, needs: &[String], lead_branch: &str) -> Result<(), CpdError> {
    tx.execute(
        "INSERT INTO prs (plan, key, repo, name, base, title, shape, status, lead_branch, created_txn, created_at)
         SELECT ?1, ?2, ?3, ?6, ?4, ?7, ?8, 'waiting', ?5, id, at FROM txns WHERE id = cpd_txn()",
        rusqlite::params![plan.as_str(), wanted.key.as_str(), wanted.repo, base, lead_branch, wanted.name.as_str(), wanted.title, wanted.shape],
    )?;
    insert_needs(tx, plan, &wanted.key, needs)
}

/// The shape's stub drafts (§9.4): `jobs` rows with a title and a kind and no Tier and no Spec, `draft`, no
/// fingerprint, `appended` as `job add --draft` writes it, and the `dependencies` of the template.
fn insert_stubs(tx: &Connection, plan: &PlanId, wanted: &Wanted, appended: bool) -> Result<Vec<Stub>, CpdError> {
    let stubs = template(&wanted.shape, &wanted.key, &wanted.title);
    for stub in &stubs {
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

/// One `pr_needs` row per need, `needs_plan` the needed PR's Plan.
pub(super) fn insert_needs(tx: &Connection, plan: &PlanId, pr: &PrKey, needs: &[String]) -> Result<(), CpdError> {
    for need in needs {
        let (needs_plan, needs_key) = node_of(plan, need);
        tx.execute(
            "INSERT INTO pr_needs (plan, pr, needs_plan, needs) VALUES (?1, ?2, ?3, ?4)",
            [plan.as_str(), pr.as_str(), needs_plan.as_str(), needs_key.as_str()],
        )?;
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
