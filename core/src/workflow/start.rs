//! Starting a Workflow (contract §3.8 "The start", §7.2; commands.md §9.7): one
//! routine for `plan lock`, engine E7 and `workflow start`. It creates the Workflow's Lead
//! worktree on its Lead Branch (R35's remote rules), runs `pre-start`, pushes a
//! new branch, then writes `status = dispatch` and `workflow-started` in one
//! transaction. `plan lock` uses the steps one by one (all or nothing across
//! Workflows); E7 and `workflow start` use [`start`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use super::StartArgs;
use crate::config::{Config, Tier};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{BasePull, NewEvent, append};
use crate::git::{self, Setup, Step};
use crate::home::{CpdHome, Hook};
use crate::ids::{JobKey, PlanId, WorkflowKey};
use crate::output::Output;
use crate::plan::{Phase, WorkflowRow};
use crate::pr::{plan_phase, wrong_phase};

/// Every Workflow of a Plan in key order (`plan::fingerprint::load_workflows`), as E7 and `workflow start` read them.
pub use crate::plan::fingerprint::load_workflows as load;
use crate::store::{Store, WriteTxn};

const COMMAND: &str = "workflow start";

/// Who started the Workflow: the `by` of `workflow-started`, and the `source` of the `error`
/// Events the start appends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    Lock,
    Engine,
    WorkflowStart,
}

impl By {
    pub fn as_str(self) -> &'static str {
        match self {
            By::Lock => "lock",
            By::Engine => "engine",
            By::WorkflowStart => COMMAND,
        }
    }

    fn source(self) -> &'static str {
        match self {
            By::Lock => "plan lock",
            By::Engine => "watch",
            By::WorkflowStart => COMMAND,
        }
    }
}

/// What holds a `waiting` Workflow `r` back (§3.7 E7, §3.8), as the `WHERE` conditions over the aliases `r` (the Workflow),
/// `n` and `q` (a need and the Workflow it names) and `b` (the Workflow's base Workflow): the one definition of "not ready",
/// which engine E7's query, `workflow start`'s refusal and its details are all built from.
macro_rules! needs_unmerged {
    () => {
        "n.plan = r.plan AND n.workflow = r.key AND q.plan = n.plan AND q.key = n.needs AND q.status <> 'merged'"
    };
}
/// The base Workflow of `r` (§2.2 "The base Workflow": the Workflow of its repo whose Lead Branch is its base) has not opened: it is
/// `waiting` or `dispatch` (the implied need; a `merged` one is unstacked first).
macro_rules! base_workflow_not_open {
    () => {
        "b.plan = r.plan AND b.repo = r.repo AND b.lead_branch = r.base AND b.status IN ('waiting', 'dispatch')"
    };
}

/// E7's condition over `workflows r`: every need `merged` and the base Workflow, when the Workflow is
/// stacked, its base Workflow `open` (the implied need; a `merged` one is not waited for).
pub const READY: &str = concat!(
    "NOT EXISTS (SELECT 1 FROM workflow_needs n, workflows q WHERE ",
    needs_unmerged!(),
    ") AND NOT EXISTS (SELECT 1 FROM workflows b WHERE ",
    base_workflow_not_open!(),
    ")"
);

/// What holds Workflow `key` of `plan` back, read in the database (the same conditions as [`READY`]).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct NotReady {
    /// The needs that are not `merged`, as keys.
    pub needs: Vec<String>,
    /// The base Workflow as `<plan>/<key>` when it has not opened.
    pub base_workflow: Option<String>,
}

impl NotReady {
    pub(crate) fn is_empty(&self) -> bool {
        self.needs.is_empty() && self.base_workflow.is_none()
    }
}

pub(crate) fn not_ready(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    key: &WorkflowKey,
) -> Result<NotReady, CpdError> {
    let mut stmt = conn.prepare(concat!(
        "SELECT n.needs FROM workflows r, workflow_needs n, workflows q WHERE r.plan = ?1 AND r.key = ?2 AND ",
        needs_unmerged!(),
        " ORDER BY n.needs"
    ))?;
    let needs = stmt
        .query_map([plan.as_str(), key.as_str()], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stmt = conn.prepare(concat!(
        "SELECT b.plan, b.key FROM workflows r, workflows b WHERE r.plan = ?1 AND r.key = ?2 AND ",
        base_workflow_not_open!()
    ))?;
    let base_workflow = stmt
        .query_map([plan.as_str(), key.as_str()], |r| {
            Ok(format!(
                "{}/{}",
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .pop();
    Ok(NotReady {
        needs,
        base_workflow,
    })
}

/// The Workflow's base Workflow as `<plan>/<key>` (§2.2 "The base Workflow": derived, the Workflow of its repo whose Lead Branch is its
/// `base`), `None` when it is not stacked.
pub(crate) fn base_workflow(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    workflow: &WorkflowRow,
) -> Result<Option<String>, CpdError> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row("SELECT plan || '/' || key FROM workflows WHERE plan = ?1 AND repo = ?2 AND lead_branch = ?3", [plan.as_str(), workflow.repo.as_str(), workflow.base.as_str()], |r| r.get(0))
        .optional()?)
}

/// A merged base Workflow a `waiting` or `dispatch` Workflow is still stacked on (§3.8 "Stacked Workflows" step 7, hs-d301).
struct MergedBase {
    repo: String,
    /// The Workflow's `base`: the merged base Workflow's Lead Branch.
    from: String,
    /// The merged base Workflow as `<plan>/<key>`.
    from_workflow: String,
    /// The merged base Workflow's own `base`.
    to: String,
}

fn merged_base(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    key: &WorkflowKey,
) -> Result<Option<MergedBase>, CpdError> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT r.repo, r.base, b.plan || '/' || b.key, b.base FROM workflows r, workflows b
              WHERE r.plan = ?1 AND r.key = ?2 AND r.status IN ('waiting', 'dispatch')
                AND b.plan = r.plan AND b.repo = r.repo AND b.lead_branch = r.base AND b.status = 'merged'",
            [plan.as_str(), key.as_str()],
            |r| Ok(MergedBase { repo: r.get(0)?, from: r.get(1)?, from_workflow: r.get(2)?, to: r.get(3)? }),
        )
        .optional()?)
}

/// The unstack of a Workflow whose base Workflow merged (contract §3.8 "Stacked Workflows" step 7, hs-d301): while Workflow `key` of `plan` is
/// `waiting` or `dispatch` and its base Workflow (§2.2, derived from `base`) is `merged`, its `base` becomes that Workflow's own
/// `base` and `workflow-base-changed` is appended for its Plan, once per step down a chain of merged base Workflows. GitHub deleted
/// the merged branch, so it is never looked up. `by` is who unstacks (`poll`, `engine`, `workflow start`, `workflow open`); the
/// poll's Event has `number` and `url` null. Whether anything changed.
pub(crate) fn unstack(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    key: &WorkflowKey,
    by: &str,
) -> Result<bool, CpdError> {
    use rusqlite::OptionalExtension;
    let mut seen = Vec::new();
    while let Some(MergedBase {
        repo,
        from,
        from_workflow,
        to,
    }) = merged_base(tx, plan, key)?
    {
        if seen.contains(&from) {
            return Err(CpdError::bad_row(format!(
                "The base Workflows of {repo} form a cycle at {from}"
            )));
        }
        let moved = tx.execute(
            "UPDATE workflows SET base = ?3 WHERE plan = ?1 AND key = ?2 AND status IN ('waiting', 'dispatch') AND base = ?4",
            [plan.as_str(), key.as_str(), to.as_str(), from.as_str()],
        )?;
        if moved == 0 {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Workflow {key} of Plan {plan} changed while it was unstacked: rerun the command."))
                .with("plan", plan.as_str())
                .with("workflow", key.as_str()));
        }
        let to_workflow: Option<String> = tx
            .query_row(
                "SELECT plan || '/' || key FROM workflows WHERE repo = ?1 AND lead_branch = ?2",
                [repo.as_str(), to.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        let text = format!(
            "Workflow {key} of Plan {plan} now targets {to} (was {from}): its base Workflow {from_workflow} merged before its pull request existed. \
             Its tree is unchanged: no new Gate is needed and it stays ready. Read it with cpd plan show --plan {plan}."
        );
        let pull = if by == "poll" {
            BasePull::Null
        } else {
            BasePull::Absent
        };
        let event = NewEvent::workflow_base_changed(
            text,
            key,
            (&from, &to),
            (Some(&from_workflow), to_workflow.as_deref()),
            by,
            pull,
        );
        append(tx, event.plan(plan.clone()))?;
        seen.push(from);
    }
    Ok(!seen.is_empty())
}

/// [`unstack`] in a transaction of its own when the Workflow needs it (a Workflow already unstacked writes nothing), then the Workflow as
/// it is stored.
pub(super) fn unstack_stored(
    store: &mut Store,
    source: &str,
    plan: &PlanId,
    key: &WorkflowKey,
    by: &str,
) -> Result<WorkflowRow, CpdError> {
    if store.read(|conn| Ok(merged_base(conn, plan, key)?.is_some()))? {
        store.write(source, |tx| unstack(tx, plan, key, by))?;
    }
    store
        .read(|conn| Ok(load(conn, plan)?.into_iter().find(|p| &p.key == key)))?
        .ok_or_else(|| {
            CpdError::bad_row(format!(
                "Workflow {key} of Plan {plan} vanished while it was unstacked"
            ))
        })
}

/// A2 of an `open` Job (contract §3.4, hs-d340): a Workflow stacked on a base Workflow that has merged is unstacked first (`workflow-base-changed`, `by` `open job`),
/// so the Worker is told a base that exists: GitHub deleted the merged base Workflow's branch.
pub fn unstack_for_open_job(
    store: &mut Store,
    source: &str,
    plan: &PlanId,
    key: &WorkflowKey,
) -> Result<(), CpdError> {
    unstack_stored(store, source, plan, key, "open job").map(|_| ())
}

/// The root base of a Workflow of `repo` whose `base` is `base` (§2.2, §7.5): the `base` of the first Workflow down the chain
/// of base Workflows that is not stacked, the branch the stack lands on; `base` itself when the Workflow is not stacked.
pub fn root_base(conn: &rusqlite::Connection, repo: &str, base: &str) -> Result<String, CpdError> {
    use rusqlite::OptionalExtension;
    let mut base = base.to_owned();
    let mut seen = Vec::new();
    while let Some(next) = conn
        .query_row(
            "SELECT base FROM workflows WHERE repo = ?1 AND lead_branch = ?2",
            [repo, base.as_str()],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        if seen.contains(&base) {
            return Err(CpdError::bad_row(format!(
                "The base Workflows of {repo} form a cycle at {base}"
            )));
        }
        seen.push(base);
        base = next;
    }
    Ok(base)
}

/// The refs a protected path may be brought from by a base merge (§7.5), besides the Lead tip: `origin/<base>` (`<base>`
/// without `origin`), and for a stacked Workflow `origin/<root>` too, `<root>` the [`root_base`].
pub fn tamper_bases(
    conn: &rusqlite::Connection,
    dir: &Path,
    repo: &str,
    base: &str,
) -> Result<Vec<String>, CpdError> {
    let root = root_base(conn, repo, base)?;
    let mut refs = vec![git::tamper::base_ref(dir, base)?];
    if root != base {
        refs.push(git::tamper::base_ref(dir, &root)?);
    }
    Ok(refs)
}

/// A Workflow whose Lead worktree exists on its Lead Branch with `pre-start` passed.
/// Nothing is pushed and nothing is in the database.
#[derive(Debug)]
pub struct Prepared {
    workflow: WorkflowRow,
    lead: git::Prepared,
}

impl Prepared {
    /// Removes the worktree and branch again. Best effort: the failure to
    /// report is the caller's.
    pub fn discard(&self) {
        let _ = self.lead.discard(Path::new(&self.workflow.repo));
    }

    /// Pushes a Lead Branch that is new on the remote (R35). A failed push does
    /// not fail the start: it is recorded with the start.
    pub fn push(self) -> Pushed {
        let (pushed, failed) = match self.lead.push() {
            Ok(pushed) => (pushed, None),
            Err(e) => (false, Some(e.with("workflow", self.workflow.key.as_str()))),
        };
        Pushed {
            prepared: self,
            pushed,
            failed,
        }
    }
}

/// A [`Prepared`] Workflow after the push.
#[derive(Debug)]
pub struct Pushed {
    prepared: Prepared,
    pushed: bool,
    failed: Option<CpdError>,
}

impl Pushed {
    /// The Workflow's key.
    pub fn key(&self) -> &str {
        self.prepared.workflow.key.as_str()
    }

    pub fn discard(&self) {
        self.prepared.discard();
    }

    /// The key, the Lead Branch and worktree and R35's outcome, as the results
    /// list a started Workflow (commands.md §4.8 `workflows`, §9.7).
    pub fn to_json(&self) -> Value {
        let (workflow, lead) = (&self.prepared.workflow, &self.prepared.lead);
        json!({
            "key": workflow.key, "status": "dispatch", "lead_branch": lead.branch,
            "lead_worktree": lead.path.display().to_string(), "base_sha": lead.base_sha,
            "reused": lead.reused(), "pushed": self.pushed,
        })
    }
}

/// The start failed before anything was written; nothing is left behind.
#[derive(Debug)]
pub enum Failed {
    /// The worktree, the remote check or `pre-start`: the error has `details.pr`.
    Setup { step: Step, error: CpdError },
    /// The database, or the Workflow changed meanwhile.
    Other(CpdError),
}

impl Failed {
    pub fn into_error(self) -> CpdError {
        match self {
            Failed::Setup { error, .. } | Failed::Other(error) => error,
        }
    }
}

/// Where `pre-start`'s whole output goes: `run/<plan>/logs/lead-<key>.pre-start.log` (§7.2).
fn pre_start_log(home: &CpdHome, plan: &PlanId, key: &WorkflowKey) -> PathBuf {
    home.plan_dir(plan)
        .join("logs")
        .join(format!("lead-{key}.{}.log", Hook::PreStart.as_str()))
}

/// Creates the Workflow's Lead worktree and runs `pre-start` in it (§7.2), in the Workflow's repo, for a Workflow that is not
/// stacked: the one `plan lock` starts (§3.1: it starts only Workflows with no base Workflow).
pub fn prepare(
    home: &CpdHome,
    config: &Config,
    store: &mut Store,
    plan: &PlanId,
    workflow: &WorkflowRow,
) -> Result<Prepared, Failed> {
    prepare_as(home, config, store, plan, workflow, false)
}

/// [`prepare`], `stacked` being whether the Workflow is stacked on a base Workflow: its Lead Branch is then cut from that
/// Workflow's Lead Branch head (§7.2).
fn prepare_as(
    home: &CpdHome,
    config: &Config,
    store: &mut Store,
    plan: &PlanId,
    workflow: &WorkflowRow,
    stacked: bool,
) -> Result<Prepared, Failed> {
    let key = &workflow.key;
    let policy = store
        .read(|conn| crate::pr::policy_of_workflow(conn, home, plan, key))
        .map_err(Failed::Other)?;
    let path = git::workflow_lead_worktree_path(
        Path::new(&workflow.repo),
        plan,
        key,
        &workflow.lead_branch,
    )
    .map_err(Failed::Other)?;
    let setup = Setup {
        timeout: Duration::from_secs(u64::from(config.timeouts.setup_minutes) * 60),
        log: pre_start_log(home, plan, key),
    };
    let cut = match (&workflow.head_ref, stacked) {
        (Some(head_ref), _) => git::Cut::Adopted(head_ref),
        (None, true) => git::Cut::Stacked,
        (None, false) => git::Cut::Base,
    };
    match git::prepare_workflow_lead_worktree(
        Path::new(&workflow.repo),
        plan,
        &path,
        &workflow.lead_branch,
        &workflow.base,
        cut,
        &setup,
        &policy.hooks.pre_start,
    ) {
        Ok(lead) => Ok(Prepared {
            workflow: workflow.clone(),
            lead,
        }),
        Err(failure) => {
            let mut error = failure.error.with("workflow", key.as_str());
            if error.code() == ErrorCode::SetupFailed {
                error = error.with("step", failure.step.as_str());
            }
            Err(Failed::Setup {
                step: failure.step,
                error,
            })
        }
    }
}

/// Inside the transaction: the Workflow `waiting → dispatch` with its worktree, then
/// `workflow-started`, and the `error` `push_failed` of a push that failed (R35).
/// Compare-and-swap on `status = 'waiting'`, else `cas_lost`. Returns the key of the `open` Job it
/// inserted, `None` when the Workflow already had one.
pub fn record(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    pushed: &Pushed,
    by: By,
    worker_tier: Tier,
) -> Result<Option<JobKey>, CpdError> {
    let (workflow, lead) = (&pushed.prepared.workflow, &pushed.prepared.lead);
    let worktree = lead.path.display().to_string();
    let moved = tx.execute(
        "UPDATE workflows SET status = 'dispatch', lead_worktree = ?1, started_at = (SELECT at FROM txns WHERE id = cpd_txn()), start_failed_at = NULL
          WHERE plan = ?2 AND key = ?3 AND status = 'waiting'",
        [worktree.as_str(), plan.as_str(), workflow.key.as_str()],
    )?;
    if moved == 0 {
        return Err(CpdError::new(
            ErrorCode::CasLost,
            format!(
                "Workflow {} of Plan {plan} changed while it was started: rerun the command.",
                workflow.key
            ),
        )
        .with("plan", plan.as_str())
        .with("workflow", workflow.key.as_str()));
    }
    let text = format!(
        "Workflow {} of Plan {plan} started: Lead Branch {} in {worktree}. Its Jobs are scheduled. Next: cpd plan show --plan {plan}",
        workflow.key, lead.branch
    );
    let event = NewEvent::workflow_started(
        text,
        &workflow.key,
        &workflow.repo,
        &workflow.base,
        &lead.branch,
        &worktree,
        &lead.base_sha,
        lead.reused(),
        pushed.pushed,
        by.as_str(),
    );
    append(tx, event.plan(plan.clone()))?;
    // The open Job exists only from the start; a started Workflow that has one already gets no second.
    let has_open: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM jobs WHERE plan = ?1 AND workflow = ?2 AND kind = 'open')",
        [plan.as_str(), workflow.key.as_str()],
        |r| r.get(0),
    )?;
    let mut opened = None;
    if !has_open {
        opened = Some(crate::job::open::add(
            tx,
            plan,
            workflow,
            worker_tier,
            false,
            crate::job::open::BY_ENGINE,
        )?);
    }
    if let Some(error) = &pushed.failed {
        let text = format!(
            "The Lead Branch {} of Workflow {} of Plan {plan} was not pushed to origin: {}. The next merge or cpd workflow open pushes it again.",
            lead.branch,
            workflow.key,
            error.message()
        );
        let event = NewEvent::error(
            text,
            by.source(),
            error.code().as_str(),
            error.message(),
            error.details(),
        );
        append(tx, event.plan(plan.clone()))?;
    }
    Ok(opened)
}

/// Starts one Workflow of a Plan in `dispatch` (E7, `workflow start`): [`prepare`], push,
/// then [`record`] in a transaction of its own. On failure the worktree and
/// branch are removed and nothing is written.
pub fn start(
    home: &CpdHome,
    config: &Config,
    store: &mut Store,
    plan: &PlanId,
    workflow: &WorkflowRow,
    by: By,
) -> Result<Pushed, Failed> {
    // A base Workflow that merged unstacks the Workflow first: its branch is gone, the Lead Branch is cut from the new base (hs-d301).
    let workflow = &unstack_stored(store, by.source(), plan, &workflow.key, by.as_str())
        .map_err(Failed::Other)?;
    let stacked = store
        .read(|conn| base_workflow(conn, plan, workflow))
        .map_err(Failed::Other)?
        .is_some();
    let pushed = prepare_as(home, config, store, plan, workflow, stacked)?.push();
    match store.write(by.source(), |tx| {
        record(tx, plan, &pushed, by, config.worker_tier)
    }) {
        Ok(_) => Ok(pushed),
        Err(e) => {
            pushed.discard();
            Err(Failed::Other(e))
        }
    }
}

/// Engine E7 failed to start a `waiting` Workflow (§3.8 "A failed start"): in one
/// transaction `start_failed_at` is set and one `error` Event `workflow_start_failed`
/// is appended for the Plan's Lead, `source` being the engine's. False when the
/// Workflow is no longer `waiting` or was already marked: nothing is written.
pub fn record_failure(
    store: &mut Store,
    source: &str,
    plan: &PlanId,
    key: &WorkflowKey,
    step: Step,
    error: &CpdError,
) -> Result<bool, CpdError> {
    store.write(source, |tx| {
        let marked = tx.execute(
            "UPDATE workflows SET start_failed_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE plan = ?1 AND key = ?2 AND status = 'waiting' AND start_failed_at IS NULL",
            [plan.as_str(), key.as_str()],
        )?;
        if marked == 0 {
            return Ok(false);
        }
        let mut details = json!({"workflow": key, "step": step.as_str()});
        for field in ["tail", "log"] {
            if let Some(value) = error.details().get(field).filter(|v| !v.is_null()) {
                details[field] = value.clone();
            }
        }
        let message = format!("Workflow {key} of Plan {plan} could not be started ({}): {}", step.as_str(), error.message());
        let text = format!("{message} The engine does not try again. Fix the cause, then: cpd workflow start {key} --plan {plan}");
        let event = NewEvent::error(text, source, ErrorCode::WorkflowStartFailed.as_str(), &message, details);
        append(tx, event.plan(plan.clone()))?;
        Ok(true)
    })
}

/// `workflow start` (commands.md §9.7).
pub(crate) fn run(args: StartArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let key = args.key;
    let mut env = Env::open()?;
    let (phase, workflows, held) = env.store.read(|conn| {
        Ok((
            plan_phase(conn, &plan)?,
            load(conn, &plan)?,
            not_ready(conn, &plan, &key)?,
        ))
    })?;
    if phase != Phase::Dispatch {
        return Err(wrong_phase(&plan, phase, COMMAND, "dispatch"));
    }
    let Some(workflow) = workflows.iter().find(|p| p.key == key) else {
        return Err(CpdError::new(
            ErrorCode::WorkflowNotFound,
            format!("No Workflow {key} in Plan {plan}."),
        )
        .with("plan", plan.as_str())
        .with("workflow", key.as_str()));
    };
    match workflow.status.as_str() {
        "waiting" => {}
        "cancelled" => {
            return Err(CpdError::new(
                ErrorCode::IllegalTransition,
                format!("Workflow {key} of Plan {plan} is cancelled: it cannot be started."),
            )
            .with("plan", plan.as_str())
            .with("workflow", key.as_str())
            .with("from", "cancelled")
            .with("to", "dispatch"));
        }
        _ => {
            return Ok(Output::success(json!({"workflow": {
                "key": key, "status": workflow.status, "lead_branch": workflow.lead_branch, "lead_worktree": workflow.lead_worktree,
                "base_sha": null, "reused": null, "pushed": null,
            }, "already": true})));
        }
    }
    if !held.is_empty() {
        let mut why = Vec::new();
        if !held.needs.is_empty() {
            why.push(format!(
                "needs {}, which {} not merged yet",
                held.needs.join(", "),
                if held.needs.len() == 1 { "is" } else { "are" }
            ));
        }
        if let Some(base_workflow) = &held.base_workflow {
            why.push(format!(
                "is stacked on {base_workflow}, which has not opened yet"
            ));
        }
        let mut error = CpdError::new(
            ErrorCode::WorkflowNotReady,
            format!("Workflow {key} of Plan {plan} {}.", why.join(" and ")),
        )
        .with("plan", plan.as_str())
        .with("workflow", key.as_str())
        .with("needs", held.needs);
        if let Some(base_workflow) = held.base_workflow {
            error = error.with("base_workflow", base_workflow);
        }
        return Err(error);
    }
    let pushed = start(
        &env.home,
        &env.config,
        &mut env.store,
        &plan,
        workflow,
        By::WorkflowStart,
    )
    .map_err(Failed::into_error)?;
    Ok(Output::success(json!({"workflow": pushed.to_json()})))
}
