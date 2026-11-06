//! `workflow edit` (commands.md §9.5, contract §3.8): changes a `waiting` Workflow's repo,
//! base (a branch, or a base Workflow to stack on) or needs before the lock. The plan
//! fingerprint changes with them. After the lock only a restack is allowed (§3.8 step 10): a
//! `waiting` Workflow stacks on a Workflow it needs (`--base <A>` with `--needs <rest>` or `--no-needs`). With `--title` it also changes a Workflow's own title
//! in any phase and any status but `merged` and `cancelled` (hs-d285): the title is
//! not a Locked field and not in the fingerprint.

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::EditArgs;
use super::add::{
    BaseArg, BaseWanted, Subject, classify_base, insert_needs, key_repo, key_repo_policy,
    known_workflows, main_worktree, needs_refs, no_branch_clash, resolve_refs, stacked_on,
    wanted_base,
};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{PlanId, RepoName, WorkflowKey};
use crate::output::Output;
use crate::plan::WorkflowRow;
use crate::plan::fingerprint::load_workflows;
use crate::pr::retitle_pr;
use crate::pr::{find_workflow, plan_phase, workflow_locked};

const COMMAND: &str = "workflow edit";

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
    /// Whether a flag other than `--title` was given: only these need a `waiting` Workflow before the lock.
    fn others(&self) -> bool {
        self.repo.is_some() || self.base.is_some() || self.needs.is_some()
    }

    /// Whether the flags have the shape of a restack: `--base` with `--needs` or `--no-needs`, no `--repo`.
    fn restack_shape(&self) -> bool {
        self.repo.is_none() && self.base.is_some() && self.needs.is_some()
    }

    /// Whether the flags are a retarget: `--base <branch>` (not a Workflow) alone (pro-d379).
    fn retarget_shape(&self) -> bool {
        self.repo.is_none()
            && self.needs.is_none()
            && matches!(self.base, Some(BaseWanted::Branch(_)))
    }
}

/// The flag shapes `editable_workflow` lets through after the lock.
#[derive(Clone, Copy)]
struct Shape {
    restack: bool,
    retarget: bool,
}

// One variant holds a WorkflowRow, the other two; boxing would change the code that builds and matches them.
#[allow(clippy::large_enum_variant)]
enum Decision {
    /// No value changes.
    Already(WorkflowRow),
    Update {
        before: WorkflowRow,
        after: WorkflowRow,
        fields: Vec<&'static str>,
    },
}

pub(crate) fn edit(args: EditArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    if args.repo.is_none()
        && args.base.is_none()
        && args.needs.is_none()
        && !args.no_needs
        && args.title.is_none()
        && args.jira.is_none()
    {
        return Err(CpdError::new(
            ErrorCode::NothingToEdit,
            "Nothing to edit: pass --repo, --base, --needs, --no-needs, --title or --jira.",
        ));
    }
    let needs = match (&args.needs, args.no_needs) {
        (Some(refs), _) => Some(needs_refs(&plan, refs)?),
        (None, true) => Some(Vec::new()),
        (None, false) => None,
    };
    let key = args.key;
    let mut env = Env::open()?;

    // The phase, the Workflow and its status first; then git on the new repo and base.
    let wants_others = args.repo.is_some() || args.base.is_some() || needs.is_some();
    let wants_title = args.title.is_some() || args.jira.is_some();
    let restack_shape = args.repo.is_none() && args.base.is_some() && needs.is_some();
    // Whether `--base` names a Workflow is known only after `classify_base`; `decide` checks it.
    let retarget_shape = args.repo.is_none() && args.base.is_some() && needs.is_none();
    let (stored, stored_title) = env.store.read(|conn| {
        editable_workflow(
            conn,
            &plan,
            &key,
            wants_title,
            wants_others,
            Shape {
                restack: restack_shape,
                retarget: retarget_shape,
            },
        )
    })?;
    if stored.head_ref.is_some()
        && let Some(flag) = [
            (args.repo.is_some(), "--repo"),
            (args.base.is_some(), "--base"),
        ]
        .iter()
        .find_map(|(given, flag)| given.then_some(*flag))
    {
        return Err(CpdError::new(ErrorCode::WorkflowAdopted, format!("Workflow {key} is an adopted pull request: its repo and base come from the pull request, so {flag} cannot change."))
            .with("workflow", key.as_str())
            .with("flag", flag));
    }
    let repo = args.repo.as_deref().map(main_worktree).transpose()?;
    let name = repo
        .as_deref()
        .map(|r| key_repo(&mut env, &plan, r))
        .transpose()?;
    let in_repo = repo.clone().unwrap_or_else(|| stored.repo.clone().into());
    let base = match args.base.as_deref() {
        Some(_) => Some(wanted_base(
            &in_repo,
            env.store
                .read(|conn| classify_base(conn, &plan, &in_repo, args.base.as_deref()))?,
        )?),
        // A new repo keeps the stored base, which must exist there.
        None if repo.is_some() => Some(wanted_base(
            &in_repo,
            BaseArg::Branch(Some(stored.base.clone())),
        )?),
        None => None,
    };
    let mut title = args.title.map(|t| t.to_string());
    if wants_title && stored.head_ref.is_none() {
        let jira = args
            .jira
            .as_ref()
            .map(|j| j.to_string())
            .or_else(|| stored.jira.as_ref().map(|j| j.to_string()));
        let wanted_title = title.clone().or(stored_title);
        if let (Some(jira), Some(wanted_title)) = (jira, wanted_title)
            && key_repo_policy(&mut env, &plan, &in_repo)?
                .2
                .workflow
                .jira_required
        {
            title = Some(
                super::jira_title::conform(&wanted_title, &jira)
                    .ok_or_else(|| crate::pr::open::jira_title_invalid(&plan, &key))?,
            );
        }
    }
    let edits = Edits {
        repo: repo.as_ref().map(|r| r.display().to_string()),
        name,
        base,
        needs,
        title,
        jira: args.jira.map(|j| j.to_string()),
    };

    let decision = env.store.read(|conn| decide(conn, &plan, &key, &edits))?;
    let (workflow, fields, already) = match decision {
        Decision::Already(workflow) => (workflow, Vec::new(), true),
        Decision::Update {
            before,
            after,
            fields,
        } => {
            if after.repo != before.repo {
                no_branch_clash(
                    repo.as_deref().expect("the repo changed, so it was given"),
                    &plan,
                    &before.lead_branch,
                )?;
            }
            if let Some(title) = edits.title.as_deref().filter(|_| fields.contains(&"title")) {
                retitle_open_pull_request(&mut env, &plan, &key, title)?;
            }
            env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, &edits)? {
                Decision::Already(workflow) => Ok((workflow, Vec::new(), true)),
                Decision::Update { before, after, fields } => {
                    update(tx, &plan, &before, &after, edits.name.as_ref(), edits.title.as_deref(), edits.jira.as_deref())?;
                    let text = format!("Workflow {key} of Plan {plan} edited ({}). Next: cpd plan render --plan {plan}", fields.join(", "));
                    append(tx, NewEvent::workflow_edited(text, &key, &fields).plan(plan.clone()))?;
                    crate::session::working_pr::name(tx, &plan, &key)?;
                    Ok((after, fields, false))
                }
            })?
        }
    };
    let mut out =
        json!({"workflow": {"key": workflow.key, "status": workflow.status}, "fields": fields});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// When the Workflow has an open pull request, retitles it on GitHub, before the title is stored: a failure stores nothing (§9.5).
fn retitle_open_pull_request(
    env: &mut Env,
    plan: &PlanId,
    key: &WorkflowKey,
    title: &str,
) -> Result<(), CpdError> {
    let open: Option<(String, i64)> = env.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT p.repo, p.number FROM workflows r JOIN pull_requests p ON p.id = r.pull_request
              WHERE r.plan = ?1 AND r.key = ?2 AND r.status = 'open' AND p.state = 'open'",
        )?;
        Ok(stmt
            .query_row([plan.as_str(), key.as_str()], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?)
    })?;
    match open {
        Some((repo, number)) => retitle_pr(env.home.root(), &repo, number, title),
        None => Ok(()),
    }
}

/// The Workflow `key` and its stored title, which must be editable. With `title`, the Workflow is not
/// `merged` or `cancelled` (`workflow_ended`: its title is history). With `others` (a flag other than
/// `--title`), the Plan is in `grounding` or `plan` and the Workflow `waiting` (`illegal_transition`); `--title` alone
/// meets neither rule. After the lock only a restack (`shape.restack`: the flags have its shape) of a `waiting` Workflow, or
/// a retarget (`shape.retarget`: `--base` alone) of a `waiting` or `dispatch` one, passes (pro-d379);
/// anything else is `workflow_locked`, and `decide` checks the rest.
fn editable_workflow(
    conn: &Connection,
    plan: &PlanId,
    key: &WorkflowKey,
    title: bool,
    others: bool,
    shape: Shape,
) -> Result<(WorkflowRow, Option<String>), CpdError> {
    let workflows = load_workflows(conn, plan)?;
    let workflow = find_workflow(&workflows, plan, key)?;
    if title && matches!(workflow.status.as_str(), "merged" | "cancelled") {
        return Err(CpdError::new(
            ErrorCode::WorkflowEnded,
            format!(
                "Workflow {key} of Plan {plan} is {}: its title and ticket are history.",
                workflow.status
            ),
        )
        .with("plan", plan.as_str())
        .with("workflow", key.as_str())
        .with("status", workflow.status.as_str()));
    }
    if others {
        let phase = plan_phase(conn, plan)?;
        let locked = crate::plan::is_locked(conn, plan)?;
        let retarget = shape.retarget && matches!(workflow.status.as_str(), "waiting" | "dispatch");
        if locked && !(shape.restack && workflow.status == "waiting") && !retarget {
            return Err(workflow_locked(plan, phase, COMMAND));
        }
        if workflow.status != "waiting" && !(locked && retarget) {
            return Err(CpdError::new(
                ErrorCode::IllegalTransition,
                format!(
                    "Workflow {key} is {}: only a waiting Workflow is edited.",
                    workflow.status
                ),
            )
            .with("plan", plan.as_str())
            .with("workflow", key.as_str())
            .with("from", workflow.status.as_str())
            .with("to", "waiting"));
        }
    }
    let stored = conn.query_row(
        "SELECT title FROM workflows WHERE plan = ?1 AND key = ?2",
        [plan.as_str(), key.as_str()],
        |r| r.get(0),
    )?;
    Ok((workflow.clone(), stored))
}

fn decide(
    conn: &Connection,
    plan: &PlanId,
    key: &WorkflowKey,
    edits: &Edits,
) -> Result<Decision, CpdError> {
    let (before, stored_title) = editable_workflow(
        conn,
        plan,
        key,
        edits.title.is_some() || edits.jira.is_some(),
        edits.others(),
        Shape {
            restack: edits.restack_shape(),
            retarget: edits.retarget_shape(),
        },
    )?;
    let mut after = before.clone();
    if let Some(repo) = &edits.repo {
        after.repo = repo.clone();
    }
    if let Some(needs) = &edits.needs {
        after.needs = needs.clone();
    }
    // Every reference the Workflow will hold is validated again, so a rule broken by the repo alone (a base Workflow of
    // another repo) is refused too.
    if edits.repo.is_some() || edits.base.is_some() || edits.needs.is_some() {
        let base = match &edits.base {
            Some(BaseWanted::Workflow(p, k)) => BaseWanted::Workflow(p.clone(), k.clone()),
            Some(BaseWanted::Branch(b)) => BaseWanted::Branch(b.clone()),
            None => BaseWanted::Branch(before.base.clone()),
        };
        let resolved = resolve_refs(
            conn,
            &Subject {
                plan,
                key,
                repo: &after.repo,
            },
            &base,
            &after.needs,
        )?;
        after.base = resolved.base;
        let phase = plan_phase(conn, plan)?;
        if crate::plan::is_locked(conn, plan)? && !edits.retarget_shape() {
            check_restack(
                conn,
                plan,
                &before,
                &after,
                resolved.base_workflow.as_deref(),
            )
            .map_err(|()| workflow_locked(plan, phase, COMMAND))?;
        }
    }
    if after.repo != before.repo {
        let in_use = stacked_on(
            &known_workflows(conn, plan)?,
            &before.repo,
            &before.lead_branch,
        );
        if !in_use.is_empty() {
            return Err(CpdError::new(
                ErrorCode::WorkflowInUse,
                format!(
                    "Workflow {key} keeps its repo: {} {} stacked on it.",
                    in_use.join(", "),
                    if in_use.len() == 1 { "is" } else { "are" }
                ),
            )
            .with("plan", plan.as_str())
            .with("workflow", key.as_str())
            .with("workflows", in_use));
        }
    }
    let title_changed = edits
        .title
        .as_ref()
        .is_some_and(|t| stored_title.as_ref() != Some(t));
    let jira_changed = edits
        .jira
        .as_ref()
        .is_some_and(|j| before.jira.as_ref() != Some(j));
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
    Ok(Decision::Update {
        before,
        after,
        fields,
    })
}

/// After the lock, a restack is allowed only onto `A`, the base Workflow the edit resolves to, when `A` is a current need or
/// already the base Workflow, and the needs after the edit are the current needs without `A` (§3.8 step 10).
fn check_restack(
    conn: &Connection,
    plan: &PlanId,
    before: &WorkflowRow,
    after: &WorkflowRow,
    base_workflow: Option<&str>,
) -> Result<(), ()> {
    let a = base_workflow
        .and_then(|p| p.split_once('/'))
        .map(|(_, k)| k)
        .ok_or(())?;
    let workflows = load_workflows(conn, plan).map_err(|_| ())?;
    let already = workflows
        .iter()
        .any(|q| q.key.as_str() == a && q.repo == before.repo && q.lead_branch == before.base);
    if !already && !before.needs.iter().any(|n| n == a) {
        return Err(());
    }
    let mut want: Vec<&String> = before.needs.iter().filter(|n| n.as_str() != a).collect();
    let mut got: Vec<&String> = after.needs.iter().collect();
    want.sort();
    got.sort();
    if want == got { Ok(()) } else { Err(()) }
}

/// Writes the changed `workflows` columns, and deletes and inserts `workflow_needs` rows
/// to match (§4.4 allows deletion in `grounding` and `plan`). `title` is the new
/// title when it was given (a Workflow that has it already is not touched).
fn update(
    tx: &crate::store::WriteTxn<'_>,
    plan: &PlanId,
    before: &WorkflowRow,
    after: &WorkflowRow,
    name: Option<&RepoName>,
    title: Option<&str>,
    jira: Option<&str>,
) -> Result<(), CpdError> {
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
        // The Workflow's pending Jira transitions follow its ticket (swr-jira.md §7).
        crate::swr::source::jira::adopt_ticket(tx, plan.as_str(), after.key.as_str(), jira)?;
    }
    if (&before.repo, &before.base) != (&after.repo, &after.base) {
        tx.execute(
            "UPDATE workflows SET repo = ?1, base = ?2, name = coalesce(?5, name) WHERE plan = ?3 AND key = ?4 AND status IN ('waiting', 'dispatch')",
            rusqlite::params![after.repo.as_str(), after.base.as_str(), plan.as_str(), after.key.as_str(), name.map(RepoName::as_str)],
        )?;
    }
    for need in before.needs.iter().filter(|n| !after.needs.contains(n)) {
        tx.execute(
            "DELETE FROM workflow_needs WHERE plan = ?1 AND workflow = ?2 AND needs = ?3",
            [plan.as_str(), after.key.as_str(), need.as_str()],
        )?;
    }
    let added: Vec<String> = after
        .needs
        .iter()
        .filter(|n| !before.needs.contains(n))
        .cloned()
        .collect();
    insert_needs(tx, plan, &after.key, &added)
}
