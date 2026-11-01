//! `workflow add` (commands.md §9.4, contract §3.8): adds a Workflow to the Plan,
//! `waiting`. Nothing is started here: no git write, no tmux. It also holds
//! what `workflow edit` shares with it: the repo, the base, the Workflow references of
//! `--base <key>`, `--base <plan>/<key>` and `--needs` (contract §2.2 "A Workflow reference") with
//! their validation, and the branch checks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::AddArgs;
use crate::env::Env;
use crate::pr::{Adoptable, adoptable, plan_phase};
use crate::config::Tier;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::home::CpdHome;
use crate::git::run::{branch_exists, has_origin, run};
use crate::git::worktree::list as worktrees;
use crate::ids::{JobKey, OwnerLogin, PlanId, WorkflowKey, RepoName};
use crate::job::open;
use crate::job::spec::{Spec, parse as parse_spec};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::WorkflowRow;
use crate::plan::fingerprint::load_workflows;
use crate::plan::jobs::Job;
use crate::plan::locked_fingerprint;
use crate::policy::{Policy, PolicyKey, owner_mismatch, owner_unset, repo_key, resolve};
use crate::store::WriteTxn;

const COMMAND: &str = "workflow add";

/// The shapes `workflow add` writes (contract §2.2 "Job kinds"); `free` is only what the migration gave an older Workflow.
pub(crate) const SHAPES: [&str; 3] = ["tdd", "docs", "config"];

/// One stub draft of a shape's template.
struct Stub {
    key: String,
    /// The template suffix (`docs`, `tests`, `impl`, `config`; `open` for the opener).
    suffix: &'static str,
    kind: &'static str,
    title: String,
    /// The stub it needs: the step before it.
    needs: Option<String>,
}

/// The stub drafts a Workflow of `shape` starts with (commands.md §9.4): `tdd` docs, red tests (needs docs) and
/// implementation (needs the tests); `docs` its docs; `config` one `work` Job. Every shape ends with the Workflow's opener stub
/// (`open-<pr>`, kind `open`, no needs): its Ready rule is derived (contract §2.2 "Job kinds", §3.2, hs-d573).
fn template(shape: &str, workflow: &WorkflowKey, title: &str) -> Vec<Stub> {
    let stub = |suffix: &'static str, kind: &'static str, prefix: &str, needs: Option<&str>| Stub {
        key: format!("{workflow}-{suffix}"),
        suffix,
        kind,
        title: format!("{prefix}: {title}"),
        needs: needs.map(|n| format!("{workflow}-{n}")),
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
    stubs.push(Stub { key: format!("open-{workflow}"), suffix: "open", kind: "open", title: format!("Open the pull request: {title}"), needs: None });
    stubs
}

/// The flags given, resolved.
struct Wanted {
    key: WorkflowKey,
    repo: String,
    /// The repo's GitHub name, from its `origin` (§8.3).
    name: RepoName,
    base: BaseWanted,
    title: String,
    /// `--jira`: the Workflow's ticket.
    jira: Option<String>,
    /// `--shape` (§9.4): `tdd` by default.
    shape: String,
    /// `--needs` as given, as sorted Workflow references (`needs_refs`); `Some(vec![])` is `--no-needs`; `None`: the default.
    needs: Option<Vec<String>>,
    /// `--adopt`: the pull request to adopt, with `--confirm-author`.
    adopt: Option<Adopt>,
    /// `--jobs`: the template's non-open Jobs as decided work (§9.4), by suffix; read and checked once, at the flag.
    jobs: Option<BTreeMap<&'static str, (Tier, Spec)>>,
}

/// `--jobs <file.json>` (§9.4): one object keyed by the suffixes of `shape`'s template (every one, no other), each
/// `{"tier", "spec"}`. A fault is the code `job add` uses for the same one (`spec_invalid`, `tier_invalid`, ...), never `usage`.
fn read_jobs(path: &Path, shape: &str, workflow: &WorkflowKey, title: &str) -> Result<BTreeMap<&'static str, (Tier, Spec)>, CpdError> {
    let invalid = |at: &str, why: String| {
        CpdError::new(
            ErrorCode::SpecInvalid,
            format!("The --jobs file is invalid at {at}: {why}."),
        )
        .with("path", at)
        .with("file", path.display().to_string())
    };
    let text = std::fs::read_to_string(path).map_err(|e| CpdError::file_unreadable(path, e))?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| invalid("$", format!("not JSON: {e}")))?;
    let serde_json::Value::Object(mut object) = value else { return Err(invalid("$", "expected an object".to_owned())) };
    let stubs: Vec<Stub> = template(shape, workflow, title).into_iter().filter(|s| s.kind != "open").collect();
    if let Some(unknown) = object.keys().find(|k| !stubs.iter().any(|s| s.suffix == k.as_str())) {
        return Err(invalid(&format!("$.{unknown}"), format!("unknown key: the {shape} template has {}", suffixes(&stubs))));
    }
    let mut jobs = BTreeMap::new();
    for stub in &stubs {
        let at = format!("$.{}", stub.suffix);
        let Some(serde_json::Value::Object(mut entry)) = object.remove(stub.suffix) else {
            return Err(invalid(
                &at,
                format!(
                    "missing key (or not an object): every key of the {shape} template is required, {}",
                    suffixes(&stubs)
                ),
            ));
        };
        if let Some(unknown) = entry
            .keys()
            .find(|k| !matches!(k.as_str(), "tier" | "spec"))
        {
            return Err(invalid(
                &format!("{at}.{unknown}"),
                "unknown field".to_owned(),
            ));
        }
        let Some(serde_json::Value::String(tier)) = entry.remove("tier") else {
            return Err(invalid(
                &format!("{at}.tier"),
                "missing field: a Tier name".to_owned(),
            ));
        };
        let tier = Tier::parse(&tier)?;
        tier.check_worker()?;
        let spec = entry.remove("spec").ok_or_else(|| {
            invalid(
                &format!("{at}.spec"),
                "missing field: the Job's Spec object".to_owned(),
            )
        })?;
        let spec = parse_spec(&spec.to_string()).map_err(|e| {
            e.with("job", stub.key.as_str())
                .with("file", path.display().to_string())
        })?;
        spec.check_kind(stub.kind)?;
        jobs.insert(stub.suffix, (tier, spec));
    }
    Ok(jobs)
}

fn suffixes(stubs: &[Stub]) -> String {
    stubs
        .iter()
        .map(|s| s.suffix)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The pull request `--adopt` names, read from GitHub, and the author the Lead confirmed.
struct Adopt {
    pull: Adoptable,
    confirm: Option<String>,
}

enum Decision {
    /// The Workflow exists with these values; its status and `lead_branch` as stored, and `appended`
    /// as the Event recorded it.
    Already { status: String, lead_branch: String, appended: bool, base: String, base_workflow: Option<String>, needs: Vec<String> },
    /// The Workflow is new, in a Plan in this phase.
    Insert { phase: Phase, base: String, base_workflow: Option<String>, needs: Vec<String> },
}

/// A Workflow reference (contract §2.2): a bare `<key>`, a Workflow of the Plan the command is about, or
/// `<plan>/<key>`, a Workflow of that Plan, the Plan's own included. Parsed once, at the flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowRef {
    plan: Option<PlanId>,
    key: WorkflowKey,
}

impl FromStr for WorkflowRef {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Self, CpdError> {
        match s.split_once('/') {
            Some((plan, key)) => Ok(WorkflowRef { plan: Some(PlanId::parse(plan)?), key: WorkflowKey::parse(key)? }),
            None => Ok(WorkflowRef { plan: None, key: WorkflowKey::parse(s)? }),
        }
    }
}

/// `needs` as the sorted references of `own`'s Plan: the bare key for a Workflow of the Plan however it was written, `<plan>/<key>`
/// for another Plan's (which `resolve_refs` refuses, `workflow_other_plan`); a repeat is refused with `usage` (§9.4).
pub(super) fn needs_refs(own: &PlanId, needs: &[WorkflowRef]) -> Result<Vec<String>, CpdError> {
    let mut refs: Vec<String> = needs
        .iter()
        .map(|n| match &n.plan {
            Some(plan) if plan != own => format!("{plan}/{}", n.key),
            _ => n.key.to_string(),
        })
        .collect();
    refs.sort();
    if let Some(pair) = refs.windows(2).find(|w| w[0] == w[1]) {
        return Err(CpdError::new(ErrorCode::Usage, format!("--needs lists {} twice.", pair[0])).with("workflow", pair[0].as_str()));
    }
    Ok(refs)
}

/// What `--base` names: a branch (`None`: the default branch), or a base Workflow.
pub(super) enum BaseArg {
    Branch(Option<String>),
    Workflow(PlanId, WorkflowKey),
}

/// What the Workflow targets, once the git checks passed.
pub(super) enum BaseWanted {
    /// A branch, checked to exist (`resolve_base`).
    Branch(String),
    /// A base Workflow named as `<key>` or `<plan>/<key>`: its Lead Branch becomes the base.
    Workflow(PlanId, WorkflowKey),
}

/// Reads `--base` (§2.2 "A Workflow reference"): a base Workflow when it is a bare `<key>` of a Workflow of `plan` in `repo`
/// (it wins over a same-named branch), or has the form `<plan>/<key>` and a Plan `<plan>` exists; else a branch.
pub(super) fn classify_base(
    conn: &Connection,
    plan: &PlanId,
    repo: &Path,
    base: Option<&str>,
) -> Result<BaseArg, CpdError> {
    let Some(value) = base else {
        return Ok(BaseArg::Branch(None));
    };
    if !value.contains('/')
        && let Ok(key) = WorkflowKey::parse(value)
    {
        let repo = repo.display().to_string();
        if known_workflows(conn, plan)?.iter().any(|q| q.key == key.as_str() && q.repo == repo) {
            return Ok(BaseArg::Workflow(plan.clone(), key));
        }
    }
    if let Some((plan, key)) = value.split_once('/')
        && let (Ok(plan), Ok(key)) = (PlanId::parse(plan), WorkflowKey::parse(key))
    {
        let exists: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM plans WHERE id = ?1)",
            [plan.as_str()],
            |r| r.get(0),
        )?;
        if exists {
            return Ok(BaseArg::Workflow(plan, key));
        }
    }
    Ok(BaseArg::Branch(Some(value.to_owned())))
}

/// The base of `--base` in `repo`: a branch must exist there (`base_not_found`), a base Workflow needs no
/// branch yet (its Workflow may be `waiting`).
pub(super) fn wanted_base(repo: &Path, arg: BaseArg) -> Result<BaseWanted, CpdError> {
    match arg {
        BaseArg::Branch(branch) => Ok(BaseWanted::Branch(resolve_base(repo, branch.as_deref())?)),
        BaseArg::Workflow(plan, key) => Ok(BaseWanted::Workflow(plan, key)),
    }
}

/// A Workflow of the Plan, as the reference checks read it.
#[derive(Clone, Debug)]
pub(super) struct Known {
    key: String,
    repo: String,
    base: String,
    lead_branch: String,
    status: String,
}

/// Every Workflow of `plan`.
pub(super) fn known_workflows(conn: &Connection, plan: &PlanId) -> Result<Vec<Known>, CpdError> {
    let mut stmt = conn.prepare("SELECT key, repo, base, lead_branch, status FROM workflows WHERE plan = ?1 ORDER BY key")?;
    let rows = stmt
        .query_map([plan.as_str()], |r| {
            Ok(Known {
                key: r.get(0)?,
                repo: r.get(1)?,
                base: r.get(2)?,
                lead_branch: r.get(3)?,
                status: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// The base Workflow of a Workflow of `repo` targeting `base` (§2.2 "The base Workflow"): derived, never stored.
pub(super) fn derived_base_workflow<'a>(known: &'a [Known], repo: &str, base: &str) -> Option<&'a Known> {
    known.iter().find(|q| q.repo == repo && q.lead_branch == base)
}

/// The Workflows of the Plan, not `cancelled`, stacked on the Workflow whose Lead Branch is `lead_branch` in `repo`.
pub(super) fn stacked_on(known: &[Known], repo: &str, lead_branch: &str) -> Vec<String> {
    known
        .iter()
        .filter(|p| p.status != "cancelled" && p.repo == repo && p.base == lead_branch)
        .map(|p| p.key.clone())
        .collect()
}

/// The Workflow being added or edited.
pub(super) struct Subject<'a> {
    pub plan: &'a PlanId,
    pub key: &'a WorkflowKey,
    /// Its repo as it will be stored.
    pub repo: &'a str,
}

/// What [`resolve_refs`] settled.
pub(super) struct Resolved {
    /// The branch the pull request targets: a base Workflow's Lead Branch when stacked.
    pub base: String,
    /// The base Workflow as `<plan>/<key>`, derived from `base`.
    pub base_workflow: Option<String>,
}

/// A reference naming another Plan (`workflow_other_plan`): needs and base Workflows are Workflows of the Workflow's own Plan.
fn other_plan(subject: &Subject<'_>, plan: &str, key: &str) -> CpdError {
    CpdError::new(
        ErrorCode::WorkflowOtherPlan,
        format!("Workflow {plan}/{key} is a Workflow of Plan {plan}, not of {}: needs and base Workflows are Workflows of the Workflow's own Plan.", subject.plan),
    )
    .with("workflow", subject.key.as_str())
    .with("reference", format!("{plan}/{key}"))
    .with("plan", plan)
}

/// The validation of contract §2.2 "Validation of a reference" for the base and the needs of `subject`, and the
/// base it makes: the references are Workflows of the Workflow's own Plan (`workflow_other_plan`), exist (`base_workflow_unknown`,
/// `needs_unknown`), are not `cancelled` (`needs_unavailable`), a base Workflow is not `merged` (`base_workflow_unavailable`)
/// and is of the Workflow's repo (`base_workflow_other_repo`); no Workflow is both base and need (`usage`); the graph of needs and
/// base Workflows stays acyclic (`dependency_cycle`). `needs` are references of the Plan, as `needs_refs` makes them.
pub(super) fn resolve_refs(conn: &Connection, subject: &Subject<'_>, base: &BaseWanted, needs: &[String]) -> Result<Resolved, CpdError> {
    let plan = subject.plan;
    if let BaseWanted::Workflow(base_plan, base_key) = base
        && base_plan != plan
    {
        return Err(other_plan(subject, base_plan.as_str(), base_key.as_str()));
    }
    for need in needs {
        if let Some((need_plan, key)) = need.split_once('/') {
            return Err(other_plan(subject, need_plan, key));
        }
    }
    let known = known_workflows(conn, plan)?;
    let find = |key: &str| known.iter().find(|p| p.key == key);

    let (branch, base_workflow) = match base {
        BaseWanted::Workflow(_, base_key) => {
            let Some(workflow) = find(base_key.as_str()) else {
                return Err(CpdError::new(ErrorCode::BaseWorkflowUnknown, format!("Plan {plan} has no Workflow {base_key} to stack on."))
                    .with("plan", plan.as_str())
                    .with("workflow", base_key.as_str()));
            };
            check_base_workflow_state(workflow)?;
            if workflow.repo != subject.repo {
                return Err(CpdError::new(
                    ErrorCode::BaseWorkflowOtherRepo,
                    format!("Workflow {} is in {}, not in {}: a Workflow stacks only on a Workflow of its repo.", workflow.key, workflow.repo, subject.repo),
                )
                .with("workflow", workflow.key.as_str())
                .with("repo", subject.repo)
                .with("base_repo", workflow.repo.as_str()));
            }
            (workflow.lead_branch.clone(), Some(workflow))
        }
        BaseWanted::Branch(branch) => {
            // A branch that is already a Lead Branch of a Workflow of the Plan is stacked all the same.
            let derived = derived_base_workflow(&known, subject.repo, branch);
            if let Some(workflow) = derived {
                check_base_workflow_state(workflow)?;
            }
            (branch.clone(), derived)
        }
    };
    let base_key = base_workflow.map(|p| p.key.clone());

    let unknown: Vec<String> = needs
        .iter()
        .filter(|n| n.as_str() == subject.key.as_str() || find(n).is_none())
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnknown, format!("Plan {plan} has no Workflow {} to need.", unknown.join(", ")))
            .with("plan", plan.as_str())
            .with("needs", unknown));
    }
    let unavailable: Vec<String> = needs
        .iter()
        .filter(|n| find(n).is_some_and(|p| p.status == "cancelled"))
        .cloned()
        .collect();
    if !unavailable.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnavailable, format!("A Workflow cannot need the cancelled Workflow {}.", unavailable.join(", ")))
            .with("plan", plan.as_str())
            .with("needs", unavailable));
    }
    if let Some(key) = base_key.as_ref().filter(|b| needs.contains(b)) {
        return Err(CpdError::new(
            ErrorCode::Usage,
            format!("{key} is the base Workflow and a need: the base Workflow is already the implied need (this Workflow waits for it to open), so drop --needs {key}."),
        )
        .with("workflow", key.as_str()));
    }

    let this = subject.key.to_string();
    let edges = edges(conn, plan, &known, &this)?;
    let mut out: Vec<String> = needs.to_vec();
    out.extend(base_key);
    if let Some(cycle) = find_cycle(&this, &out, &edges) {
        return Err(CpdError::new(
            ErrorCode::DependencyCycle,
            format!(
                "The needs and base of {} make a cycle: {}.",
                subject.key,
                cycle.join(" → ")
            ),
        )
        .with("plan", plan.as_str())
        .with("cycle", cycle));
    }
    Ok(Resolved { base: branch, base_workflow: base_workflow.map(|p| format!("{plan}/{}", p.key)) })
}

/// A base Workflow is not `cancelled` or `merged` (`base_workflow_unavailable`).
fn check_base_workflow_state(workflow: &Known) -> Result<(), CpdError> {
    if matches!(workflow.status.as_str(), "cancelled" | "merged") {
        return Err(CpdError::new(
            ErrorCode::BaseWorkflowUnavailable,
            format!("Workflow {} is {}: there is nothing to stack on, name its base branch instead.", workflow.key, workflow.status),
        )
        .with("workflow", workflow.key.as_str())
        .with("status", workflow.status.as_str()));
    }
    Ok(())
}

/// The edges of the graph of needs and base Workflows of every Workflow of the Plan that is not `cancelled` (an edge from a Workflow to
/// each Workflow it needs and to its base Workflow), `except` the edges leaving that Workflow: they are the ones being set.
fn edges(conn: &Connection, plan: &PlanId, known: &[Known], except: &String) -> Result<BTreeMap<String, Vec<String>>, CpdError> {
    let live = |key: &String| known.iter().any(|p| p.status != "cancelled" && &p.key == key);
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT workflow, needs FROM workflow_needs WHERE plan = ?1")?;
    let rows: Vec<(String, String)> = stmt.query_map([plan.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    for (from, to) in rows {
        if &from != except && live(&from) && live(&to) {
            graph.entry(from).or_default().push(to);
        }
    }
    for p in known.iter().filter(|p| p.status != "cancelled" && &p.key != except) {
        if let Some(q) = derived_base_workflow(known, &p.repo, &p.base).filter(|q| q.status != "cancelled") {
            graph.entry(p.key.clone()).or_default().push(q.key.clone());
        }
    }
    Ok(graph)
}

/// A path from `start` back to `start`, the Workflow's own `out` edges first and `edges` after, if there is one.
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

pub(crate) fn add(args: AddArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let needs = if args.no_needs {
        Some(Vec::new())
    } else if args.needs.is_empty() {
        None
    } else {
        Some(needs_refs(&plan, &args.needs)?)
    };
    let jobs = args
        .jobs
        .as_deref()
        .map(|file| read_jobs(file, &args.shape, &args.key, args.title.as_str()))
        .transpose()?;
    let mut env = Env::open()?;
    let home = env.home.clone();
    let open_tier = env.config.open_tier;
    env.store.read(|conn| check_plan(conn, &plan))?;
    let repo = main_worktree(&args.repo)?;
    let (name, policy_file, policy) = key_repo_policy(&mut env, &plan, &repo)?;
    let unconfigured = (args.shape == "tdd" && policy.test_commands.is_none()).then(|| tests_unconfigured(&repo.display().to_string(), &policy_file));
    let adopt = args.adopt.map(|number| adoptable(&repo, number)).transpose()?;
    let base = match &adopt {
        Some(a) => BaseArg::Branch(Some(a.base.clone())),
        None => env
            .store
            .read(|conn| classify_base(conn, &plan, &repo, args.base.as_deref()))?,
    };
    // A branch that is a Workflow's Lead Branch of the Plan and repo is its base Workflow's: it needs no branch yet (that Workflow may be `waiting`).
    let lead_branch_of_workflow = match &base {
        BaseArg::Branch(Some(branch)) => env.store.read(|conn| Ok(derived_base_workflow(&known_workflows(conn, &plan)?, &repo.display().to_string(), branch).is_some()))?,
        _ => false,
    };
    let wanted = Wanted {
        base: match base {
            BaseArg::Branch(Some(branch)) if lead_branch_of_workflow => BaseWanted::Branch(branch),
            base => wanted_base(&repo, base)?,
        },
        adopt: adopt.map(|pull| Adopt {
            pull,
            confirm: args.confirm_author,
        }),
        repo: repo.display().to_string(),
        name,
        title: args.title.to_string(),
        jira: args.jira.map(|j| j.to_string()),
        shape: args.shape,
        key: args.key,
        needs,
        jobs,
    };

    let head_ref = wanted.adopt.as_ref().map(|a| a.pull.head_ref.as_str());
    let (status, lead_branch, appended, base, base_workflow, needs, already) = match env.store.read(|conn| decide(conn, &plan, &wanted, unconfigured.as_ref()))? {
        Decision::Already { status, lead_branch, appended, base, base_workflow, needs } => (status, lead_branch, appended, base, base_workflow, needs, true),
        Decision::Insert { .. } => {
            // git, outside any transaction.
            no_branch_clash(&repo, &plan, &workflow_branch(&plan, &wanted.key))?;
            env.store.write(COMMAND, |tx| match decide(tx, &plan, &wanted, None)? {
                Decision::Already { status, lead_branch, appended, base, base_workflow, needs } => {
                    Ok((status, lead_branch, appended, base, base_workflow, needs, true))
                }
                Decision::Insert { phase, base, base_workflow, needs } => {
                    let appended = matches!(phase, Phase::Dispatch | Phase::Deploy | Phase::Done);
                    let lead_branch = workflow_branch(&plan, &wanted.key);
                    let key = &wanted.key;
                    if matches!(phase, Phase::Deploy | Phase::Done) {
                        reopen(tx, &plan, phase, key)?;
                    }
                    insert(tx, &plan, &wanted, &base, &needs, &lead_branch)?;
                    let stubs = insert_stubs(tx, &home, &plan, &wanted, appended, open_tier)?;
                    let with = if needs.is_empty() { String::new() } else { format!(", needs {}", needs.join(", ")) };
                    let text = format!(
                        "Workflow {key} added to Plan {plan} (repo {}, base {base}{with}), waiting. Next: cpd plan show --plan {plan}",
                        wanted.repo
                    );
                    let event = NewEvent::workflow_added(
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
                        let needs: Vec<&String> = stub.needs.iter().collect();
                        let event = if let Some((tier, _)) = wanted.jobs.as_ref().and_then(|j| j.get(stub.suffix)) {
                            let text = format!("Job {} added to Plan {plan} as queued (the {} template of Workflow {key}, from --jobs). Next: cpd job show {} --plan {plan}", stub.key, wanted.shape, stub.key);
                            NewEvent::job_added(text, key, stub.kind, "queued", appended, Some(&tier.to_string()), needs, false)
                        } else if stub.kind == "open" {
                            let text = format!("Job {} added to Plan {plan} as draft (the {} template of Workflow {key}). Next: cpd job edit {} --tier <T> --spec <file> --plan {plan}", stub.key, wanted.shape, stub.key);
                            NewEvent::job_added_by_cpd(text, key, stub.kind, "draft", &open_tier.to_string(), open::BY_TEMPLATE, None)
                        } else {
                            let text = format!("Job {} added to Plan {plan} as draft (the {} template of Workflow {key}). Next: cpd job edit {} --tier <T> --spec <file> --plan {plan}", stub.key, wanted.shape, stub.key);
                            NewEvent::job_added(text, key, stub.kind, "draft", appended, None, needs, false)
                        };
                        append(tx, event.plan(plan.clone()).job(JobKey::parse(&stub.key)?))?;
                    }
                    crate::session::working_pr::name(tx, &plan, &wanted.key)?;
                    Ok(("waiting".to_owned(), lead_branch, appended, base, base_workflow, needs, false))
                }
            })?
        }
    };
    let jira = env.store.read(|conn| Ok(conn.query_row("SELECT jira FROM workflows WHERE plan = ?1 AND key = ?2", [plan.as_str(), wanted.key.as_str()], |r| r.get::<_, Option<String>>(0))?))?;
    let mut out = json!({"workflow": {
        "key": wanted.key, "repo": wanted.repo, "name": wanted.name, "base": base, "base_workflow": base_workflow, "title": wanted.title, "jira": jira, "needs": needs,
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
/// repo's name. Shared with `workflow edit --repo`; reads and writes nothing.
pub(super) fn key_repo(env: &mut Env, plan: &PlanId, repo: &Path) -> Result<RepoName, CpdError> {
    Ok(key_repo_policy(env, plan, repo)?.0)
}

/// [`key_repo`] with the repo's resolved policy and the path of its policy file.
fn key_repo_policy(
    env: &mut Env,
    plan: &PlanId,
    repo: &Path,
) -> Result<(RepoName, PathBuf, Policy), CpdError> {
    let (context, owner): (String, Option<String>) = env.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT c.id, c.owner FROM plans p JOIN contexts c ON c.id = p.context WHERE p.id = ?1",
            [plan.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
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

/// `workflow add` is for every phase (D1), but not for an archived Plan (`archived`); returns the phase.
fn check_plan(conn: &Connection, plan: &PlanId) -> Result<Phase, CpdError> {
    let phase = plan_phase(conn, plan)?;
    let archived: bool = conn.query_row(
        "SELECT archived_at IS NOT NULL FROM plans WHERE id = ?1",
        [plan.as_str()],
        |r| r.get(0),
    )?;
    if archived {
        return Err(CpdError::new(
            ErrorCode::Archived,
            format!("Plan {plan} is archived: unarchive it first."),
        )
        .with("plan", plan.as_str())
        .with("next", format!("cpd plan unarchive --plan {plan}")));
    }
    Ok(phase)
}

/// `unconfigured`: the refusal of a `tdd` Workflow whose repo policy has no `[test-commands]`; the first decision checks it, the one inside the write transaction does not.
fn decide(conn: &Connection, plan: &PlanId, wanted: &Wanted, unconfigured: Option<&CpdError>) -> Result<Decision, CpdError> {
    let phase = check_plan(conn, plan)?;
    if wanted.jobs.is_some() && !matches!(phase, Phase::Dispatch | Phase::Deploy | Phase::Done) {
        return Err(CpdError::new(
            ErrorCode::Usage,
            format!("--jobs is for a Plan after the lock: Plan {plan} is in phase {}, where every Job is a draft. Lay out the template and give its Jobs a Tier and Spec (cpd job edit).", phase.as_str()),
        )
        .with("plan", plan.as_str())
        .with("phase", phase.as_str()));
    }
    let workflows = load_workflows(conn, plan)?;
    if let Some(existing) = workflows.iter().find(|p| p.key == wanted.key) {
        // `appended`: the Workflow came after the lock.
        let (lead_branch, appended, title) = conn.query_row(
            "SELECT r.lead_branch, coalesce(p.locked_at IS NOT NULL AND r.created_at >= p.locked_at, 0), r.title
               FROM workflows r JOIN plans p ON p.id = r.plan WHERE r.plan = ?1 AND r.key = ?2",
            [plan.as_str(), wanted.key.as_str()],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, Option<String>>(2)?)),
        )?;
        if same(existing, title.as_deref(), wanted, &workflows, &base_branch(conn, &wanted.base)?) {
            let base_workflow = derived_base_workflow(&known_workflows(conn, plan)?, &existing.repo, &existing.base).map(|q| format!("{plan}/{}", q.key));
            return Ok(Decision::Already {
                status: existing.status.clone(),
                lead_branch,
                appended,
                base: existing.base.clone(),
                base_workflow,
                needs: existing.needs.clone(),
            });
        }
        return Err(CpdError::new(ErrorCode::WorkflowExists, format!("Plan {plan} already has a Workflow {} with other flags.", wanted.key))
            .with("plan", plan.as_str())
            .with("workflow", wanted.key.as_str())
            .with("repo", existing.repo.as_str())
            .with("base", existing.base.as_str())
            .with("title", title)
            .with("needs", existing.needs.clone())
            .with("shape", existing.shape.as_str())
            .with("head_ref", existing.head_ref.clone())
            .with("status", existing.status.as_str()));
    }
    // Stacked (§2.2 "The base Workflow"): `--base` names a base Workflow, or a branch that is a Workflow's Lead Branch; its base is its link.
    let stacked = match &wanted.base {
        BaseWanted::Workflow(..) => true,
        BaseWanted::Branch(branch) => derived_base_workflow(&known_workflows(conn, plan)?, &wanted.repo, branch).is_some(),
    };
    let needs = match &wanted.needs {
        Some(needs) => needs.clone(),
        None if matches!(phase, Phase::Deploy | Phase::Done) => merged_keys(&workflows),
        None if phase == Phase::Dispatch && stacked => Vec::new(),
        None if phase == Phase::Dispatch => {
            return Err(CpdError::new(
                ErrorCode::NeedsRequired,
                format!("Plan {plan} is in dispatch: say --needs <pr> to link Workflow {} to the Workflow it waits on (merged), --base <pr> to stack it on a Workflow it waits on to open, or --no-needs. A Workflow with no needs is started at once.", wanted.key),
            )
            .with("plan", plan.as_str())
            .with("workflow", wanted.key.as_str())
            .with("phase", phase.as_str()));
        }
        None => Vec::new(),
    };
    let resolved = resolve_refs(
        conn,
        &Subject {
            plan,
            key: &wanted.key,
            repo: &wanted.repo,
        },
        &wanted.base,
        &needs,
    )?;
    if let Some(error) = unconfigured {
        return Err(error.clone());
    }
    check_template_keys_free(conn, plan, wanted)?;
    if let Some(adopt) = &wanted.adopt {
        check_adoptable(conn, wanted, adopt)?;
    }
    Ok(Decision::Insert { phase, base: resolved.base, base_workflow: resolved.base_workflow, needs })
}

/// The refusals of `--adopt` (contract §3.8), in order, exit 3: the pull request is open, not from a fork, not a cpd one, its head free of
/// every live Workflow, and its author the User or confirmed.
fn check_adoptable(conn: &Connection, wanted: &Wanted, adopt: &Adopt) -> Result<(), CpdError> {
    let pull = &adopt.pull;
    let number = pull.number;
    if pull.state != "OPEN" {
        return Err(CpdError::new(
            ErrorCode::AdoptNotOpen,
            format!(
                "Pull request #{number} is {}: only an open pull request is adopted.",
                pull.state
            ),
        )
        .with("number", number)
        .with("url", pull.url.as_str())
        .with("state", pull.state.as_str()));
    }
    if pull.fork {
        return Err(CpdError::new(
            ErrorCode::AdoptFork,
            format!("Pull request #{number} comes from a fork: cpd cannot push to a fork."),
        )
        .with("number", number)
        .with("url", pull.url.as_str()));
    }
    let is_lead_branch: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM workflows WHERE repo = ?1 AND lead_branch = ?2)",
        [wanted.repo.as_str(), pull.head_ref.as_str()],
        |r| r.get(0),
    )?;
    if is_lead_branch {
        return Err(CpdError::new(
            ErrorCode::AdoptCpdBranch,
            format!(
                "Pull request #{number} has the head branch {}: it is a cpd pull request.",
                pull.head_ref
            ),
        )
        .with("number", number)
        .with("head_ref", pull.head_ref.as_str()));
    }
    let holder: Option<(String, String)> = conn
        .query_row(
            "SELECT plan, key FROM workflows WHERE repo = ?1 AND head_ref = ?2 AND status IN ('waiting', 'dispatch', 'open')",
            [wanted.repo.as_str(), pull.head_ref.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((plan, workflow)) = holder {
        return Err(CpdError::new(
            ErrorCode::AdoptTaken,
            format!("The head branch {} is already adopted by Workflow {workflow} of Plan {plan}.", pull.head_ref),
        )
        .with("head_ref", pull.head_ref.as_str())
        .with("plan", plan)
        .with("workflow", workflow));
    }
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    if !same(&pull.author, &pull.user)
        && !adopt
            .confirm
            .as_deref()
            .is_some_and(|c| same(c, &pull.author))
    {
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

/// `tests_unconfigured` (contract §7.7): a `tdd` Workflow needs the repo's policy file (`file`) to have `[test-commands]`.
fn tests_unconfigured(repo: &str, file: &Path) -> CpdError {
    CpdError::new(
        ErrorCode::TestsUnconfigured,
        format!(
            "{repo} has no [test-commands] in its policy file {}, and a tdd Workflow needs it to check its test and impl Jobs. Add --shape docs or --shape config for a Workflow with no behaviour, or ask the Lead to add the table (contract §7.7).",
            file.display()
        ),
    )
    .with("repo", repo)
    .with("file", file.display().to_string())
}

/// None of the Job keys the shape lays out exists in the Plan (`job_key_exists`, `details.jobs`, §9.4).
fn check_template_keys_free(
    conn: &Connection,
    plan: &PlanId,
    wanted: &Wanted,
) -> Result<(), CpdError> {
    let mut taken: Vec<String> = Vec::new();
    for stub in template(&wanted.shape, &wanted.key, &wanted.title) {
        let exists: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM jobs WHERE plan = ?1 AND key = ?2)",
            [plan.as_str(), stub.key.as_str()],
            |r| r.get(0),
        )?;
        if exists {
            taken.push(stub.key);
        }
    }
    if taken.is_empty() {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::JobKeyExists,
        format!("Plan {plan} already has the Job {}, which the {} template of Workflow {} would add.", taken.join(", "), wanted.shape, wanted.key),
    )
    .with("plan", plan.as_str())
    .with("workflow", wanted.key.as_str())
    .with("jobs", taken))
}

/// The branch `base` names, before any validation: a base Workflow is looked up (`base_workflow_unknown`).
fn base_branch(conn: &Connection, base: &BaseWanted) -> Result<String, CpdError> {
    match base {
        BaseWanted::Branch(branch) => Ok(branch.clone()),
        BaseWanted::Workflow(plan, key) => known_workflows(conn, plan)?
            .into_iter()
            .find(|p| p.key == key.as_str())
            .map(|p| p.lead_branch)
            .ok_or_else(|| {
                CpdError::new(ErrorCode::BaseWorkflowUnknown, format!("Plan {plan} has no Workflow {key} to stack on."))
                    .with("plan", plan.as_str())
                    .with("workflow", key.as_str())
            }),
    }
}

/// The keys of the `merged` Workflows, in key order (A3).
fn merged_keys(workflows: &[WorkflowRow]) -> Vec<String> {
    let mut keys: Vec<String> = workflows.iter().filter(|p| p.status == "merged").map(|p| p.key.as_str().to_owned()).collect();
    keys.sort();
    keys
}

/// Without `--needs` or `--no-needs` the needs match when none, or when all merged: a rerun
/// after the reopen sees the Plan in `dispatch`, where the default is none. `title` is the
/// stored one (null for a Workflow that existed before migration `pr-title`: it never matches).
fn same(existing: &WorkflowRow, title: Option<&str>, wanted: &Wanted, workflows: &[WorkflowRow], base: &str) -> bool {
    let needs_match = match &wanted.needs {
        Some(needs) => &existing.needs == needs,
        None => existing.needs.iter().all(|n| merged_keys(workflows).contains(n)),
    };
    (
        existing.repo.as_str(),
        existing.base.as_str(),
        existing.shape.as_str(),
    ) == (wanted.repo.as_str(), base, wanted.shape.as_str())
        && title == Some(wanted.title.as_str())
        && wanted
            .jira
            .as_ref()
            .is_none_or(|j| existing.jira.as_ref() == Some(j))
        && existing.head_ref.as_deref() == wanted.adopt.as_ref().map(|a| a.pull.head_ref.as_str())
        && needs_match
}

fn insert(
    tx: &Connection,
    plan: &PlanId,
    wanted: &Wanted,
    base: &str,
    needs: &[String],
    lead_branch: &str,
) -> Result<(), CpdError> {
    tx.execute(
        "INSERT INTO workflows (plan, key, repo, name, base, title, shape, status, lead_branch, head_ref, jira, created_txn, created_at)
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
fn insert_stubs(
    tx: &Connection,
    home: &CpdHome,
    plan: &PlanId,
    wanted: &Wanted,
    appended: bool,
    open_tier: Tier,
) -> Result<Vec<Stub>, CpdError> {
    let stubs = template(&wanted.shape, &wanted.key, &wanted.title);
    let workflow = load_workflows(tx, plan)?.into_iter().find(|p| p.key == wanted.key).ok_or_else(|| {
        CpdError::bad_row(format!("Workflow {} of Plan {plan} vanished while its template was being written", wanted.key))
    })?;
    // After the lock a stub that waits for nothing would be a parked draft (hs-d731): without `--jobs` only a Workflow that waits
    // for another Workflow may lay the stubs out; the error rolls the transaction back, nothing stays written.
    if appended && wanted.jobs.is_none() && super::start::not_ready(tx, plan, &wanted.key)?.is_empty() {
        let jobs: Vec<&str> = stubs.iter().filter(|s| s.kind != "open").map(|s| s.key.as_str()).collect();
        return Err(CpdError::new(
            ErrorCode::DraftParked,
            format!(
                "Workflow {} starts at once, so its template stubs ({}) would be parked drafts that no one can review. Say --jobs <file.json> to add them as decided work \
                 (a Tier and Spec for each: {}), or make the Workflow wait for another Workflow (--needs, --base).",
                wanted.key,
                jobs.join(", "),
                stubs.iter().filter(|s| s.kind != "open").map(|s| s.suffix).collect::<Vec<_>>().join(", ")
            ),
        )
        .with("plan", plan.as_str())
        .with("workflow", wanted.key.as_str())
        .with("jobs", jobs));
    }
    for stub in &stubs {
        if let Some((tier, spec)) = wanted.jobs.as_ref().and_then(|j| j.get(stub.suffix)) {
            let roots: Vec<(String, PathBuf)> = load_workflows(tx, plan)?.iter().map(|p| (p.key.as_str().to_owned(), PathBuf::from(&p.repo))).collect();
            spec.check_primary_roots(&roots, &std::env::home_dir().unwrap_or_default())?;
            spec.check_files_relative()?;
            crate::job::add::check_resources_declared(
                tx,
                home,
                plan,
                wanted.key.as_str(),
                &stub.key,
                spec.resources.as_deref(),
            )?;
            let mut job = Job {
                seq: 0,
                key: stub.key.clone(),
                workflow: Some(wanted.key.as_str().to_owned()),
                title: stub.title.clone(),
                tier: Some(tier.to_string()),
                spec: Some(spec.stored()),
                status: "queued".to_owned(),
                appended,
                kind: stub.kind.to_owned(),
                tests: spec.tests.as_ref().map(|t| json!(t)),
                test_files: spec.test_files.as_ref().map(|t| json!(t)),
                resources: spec.resources.as_ref().map(|r| json!(r)),
                test_approved: None,
                touches_approved: None,
                urgent: false,
                fingerprint: None,
                retry_tier: None,
                cancel_reason: None,
                cancel_requested_at: None,
                failed_cause: None,
                merged_sha: None,
                summary: None,
                created_at: String::new(),
                needs: stub.needs.iter().cloned().collect(),
            };
            job.fingerprint = Some(locked_fingerprint(&job));
            let array = |v: &Vec<String>| json!(v).to_string();
            tx.execute(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, urgent, kind, tests, test_files, resources, created_txn, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'queued', ?11, ?12, 0, ?13, ?14, ?15, ?16, cpd_txn(),
                         (SELECT at FROM txns WHERE id = cpd_txn()))",
                rusqlite::params![
                    plan.as_str(),
                    job.workflow,
                    job.key,
                    job.title,
                    job.tier,
                    spec.goal,
                    spec.context,
                    array(&spec.touches),
                    array(&spec.acceptance),
                    array(&spec.out_of_scope),
                    appended,
                    job.fingerprint,
                    job.kind,
                    spec.tests.as_ref().map(array),
                    spec.test_files.as_ref().map(array),
                    spec.resources.as_ref().map(array),
                ],
            )?;
            if let Some(need) = &stub.needs {
                tx.execute(
                    "INSERT INTO dependencies (plan, job, needs) VALUES (?1, ?2, ?3)",
                    [plan.as_str(), stub.key.as_str(), need.as_str()],
                )?;
            }
            continue;
        }
        if stub.kind == "open" {
            let spec = open::spec(&workflow);
            tx.execute(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
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
            "INSERT INTO jobs (plan, workflow, key, title, status, appended, kind, created_txn, created_at)
             VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6, cpd_txn(), (SELECT at FROM txns WHERE id = cpd_txn()))",
            rusqlite::params![plan.as_str(), wanted.key.as_str(), stub.key, stub.title, appended, stub.kind],
        )?;
        if let Some(need) = &stub.needs {
            tx.execute(
                "INSERT INTO dependencies (plan, job, needs) VALUES (?1, ?2, ?3)",
                [plan.as_str(), stub.key.as_str(), need.as_str()],
            )?;
        }
    }
    Ok(stubs)
}

/// One `workflow_needs` row per need.
pub(super) fn insert_needs(tx: &Connection, plan: &PlanId, workflow: &WorkflowKey, needs: &[String]) -> Result<(), CpdError> {
    for need in needs {
        tx.execute("INSERT INTO workflow_needs (plan, workflow, needs) VALUES (?1, ?2, ?3)", [plan.as_str(), workflow.as_str(), need.as_str()])?;
    }
    Ok(())
}

/// In `deploy` or `done` the Plan goes back to `dispatch` (D1): the phase is compared and set, then
/// `plan-phase` is appended (before `workflow-added`).
fn reopen(tx: &WriteTxn<'_>, plan: &PlanId, from: Phase, key: &WorkflowKey) -> Result<(), CpdError> {
    let moved = tx.execute("UPDATE plans SET phase = 'dispatch', done_at = NULL WHERE id = ?1 AND phase = ?2", [plan.as_str(), from.as_str()])?;
    if moved == 0 {
        return Err(CpdError::new(
            ErrorCode::CasLost,
            format!(
                "Plan {plan} left phase {} meanwhile: rerun the command.",
                from.as_str()
            ),
        )
        .with("plan", plan.as_str()));
    }
    let text = format!("Plan {plan} is back in dispatch: Workflow {key} was added.");
    append(tx, NewEvent::plan_phase(text, from.as_str(), "dispatch").plan(plan.clone()))?;
    Ok(())
}

/// The Lead Branch of a Workflow made now (contract §2.1): `<plan>/<key>`, without the `cpd/` prefix.
fn workflow_branch(plan: &PlanId, key: &WorkflowKey) -> String {
    format!("{plan}/{key}")
}

/// git's own rule over the Lead Branch `lead_branch` of a Workflow of `plan` (contract §3.8, pro-d113 A4): neither the branch
/// nor its parent path exists in the repo, else `branch_exists` (exit 3). The parent path is the branch without its last
/// `/<segment>` (`<plan>` for `<plan>/<key>`, `cpd/<plan>` for `cpd/<plan>/<key>`): git cannot hold both. The legacy
/// `cpd/<plan>` has no parent check.
pub(super) fn no_branch_clash(
    repo: &Path,
    plan: &PlanId,
    lead_branch: &str,
) -> Result<(), CpdError> {
    let mut branches = vec![lead_branch];
    if lead_branch != format!("cpd/{plan}")
        && let Some((parent, _)) = lead_branch.rsplit_once('/')
    {
        branches.push(parent);
    }
    for branch in branches {
        if branch_exists(repo, branch)? {
            return Err(CpdError::new(
                ErrorCode::BranchExists,
                format!("Branch {branch} already exists in {}.", repo.display()),
            )
            .with("branch", branch)
            .with("repo", repo.display().to_string()));
        }
    }
    Ok(())
}

fn not_a_git_repo(path: &Path, why: &str) -> CpdError {
    CpdError::new(
        ErrorCode::NotAGitRepo,
        format!("{} is not inside a git repository: {why}", path.display()),
    )
    .with("repo", path.display().to_string())
}

/// The realpath of the main worktree of the repo `path` is in
/// (`git rev-parse --git-common-dir`, then the first `git worktree list`
/// entry); `not_a_git_repo` when it is in none.
pub(super) fn main_worktree(path: &Path) -> Result<PathBuf, CpdError> {
    let out = run(path, &["rev-parse", "--git-common-dir"])?;
    if out.code != 0 {
        return Err(not_a_git_repo(
            path,
            out.stderr.lines().next().unwrap_or("git rev-parse failed."),
        ));
    }
    let main = worktrees(path)?
        .into_iter()
        .next()
        .ok_or_else(|| not_a_git_repo(path, "it has no worktree."))?;
    main.path
        .canonicalize()
        .map_err(|e| not_a_git_repo(&main.path, &format!("{e}.")))
}

/// `--base`, else the repo's `origin` HEAD branch, else `main`; it must exist
/// as `refs/remotes/origin/<base>`, or `refs/heads/<base>` without `origin`
/// (`base_not_found`, exit 4).
pub(super) fn resolve_base(repo: &Path, base: Option<&str>) -> Result<String, CpdError> {
    let origin = has_origin(repo)?;
    let base = match base {
        Some(base) => base.to_owned(),
        None if origin => {
            let out = run(
                repo,
                &[
                    "symbolic-ref",
                    "--quiet",
                    "--short",
                    "refs/remotes/origin/HEAD",
                ],
            )?;
            match out.stdout.trim().strip_prefix("origin/") {
                Some(branch) if out.code == 0 && !branch.is_empty() => branch.to_owned(),
                _ => "main".to_owned(),
            }
        }
        None => "main".to_owned(),
    };
    let reference = if origin {
        format!("refs/remotes/origin/{base}")
    } else {
        format!("refs/heads/{base}")
    };
    if run(repo, &["rev-parse", "--verify", "--quiet", &reference])?.code != 0 {
        return Err(CpdError::new(
            ErrorCode::BaseNotFound,
            format!("The base {reference} does not exist in {}.", repo.display()),
        )
        .with("base", base)
        .with("ref", reference)
        .with("repo", repo.display().to_string()));
    }
    Ok(base)
}
