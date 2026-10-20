//! Starting a PR (contract §3.8 "The start", §7.2; commands.md §9.7): one
//! routine for `plan lock`, engine E7 and `pr start`. It creates the PR's Lead
//! worktree on its Lead Branch (R35's remote rules), runs `pre-start`, pushes a
//! new branch, then writes `status = dispatch` and `pr-started` in one
//! transaction. `plan lock` uses the steps one by one (all or nothing across
//! PRs); E7 and `pr start` use [`start`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use super::{Env, StartArgs, plan_phase, wrong_phase};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git::{self, Setup, Step};
use crate::home::{CpdHome, Hook};
use crate::ids::{PlanId, PrKey};
use crate::output::Output;
use crate::plan::{Phase, PrRow};

/// Every PR of a Plan in key order (`plan::fingerprint::load_prs`), as E7 and `pr start` read them.
pub use crate::plan::fingerprint::load_prs as load;
use crate::store::{Store, WriteTxn};

const COMMAND: &str = "pr start";

/// Who started the PR: the `by` of `pr-started`, and the `source` of the `error`
/// Events the start appends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    Lock,
    Engine,
    PrStart,
}

impl By {
    pub fn as_str(self) -> &'static str {
        match self {
            By::Lock => "lock",
            By::Engine => "engine",
            By::PrStart => COMMAND,
        }
    }

    fn source(self) -> &'static str {
        match self {
            By::Lock => "plan lock",
            By::Engine => "watch",
            By::PrStart => COMMAND,
        }
    }
}

/// What holds a `waiting` PR `r` back (§3.7 E7, §3.8), as the `WHERE` conditions over the aliases `r` (the PR),
/// `n` and `q` (a need and the PR it names, any Plan) and `b` (the PR's base PR): the one definition of "not ready",
/// which engine E7's query, `pr start`'s refusal and its details are all built from.
macro_rules! needs_unmerged {
    () => {
        "n.plan = r.plan AND n.pr = r.key AND q.plan = n.needs_plan AND q.key = n.needs AND q.status <> 'merged'"
    };
}
/// The base PR of `r` (§2.2 "The base PR": the PR of its repo whose Lead Branch is its base) has not started.
macro_rules! base_pr_waiting {
    () => {
        "b.repo = r.repo AND b.lead_branch = r.base AND b.status = 'waiting'"
    };
}

/// E7's condition over `prs r`: every need `merged` (a need of another Plan too) and the base PR, when the PR is
/// stacked, started (`dispatch`, `open`; a `merged` one is not waited for).
pub const READY: &str = concat!(
    "NOT EXISTS (SELECT 1 FROM pr_needs n, prs q WHERE ",
    needs_unmerged!(),
    ") AND NOT EXISTS (SELECT 1 FROM prs b WHERE ",
    base_pr_waiting!(),
    ")"
);

/// A PR reference (§2.2): the bare key of a PR of `plan`, `<plan>/<key>` of another's.
fn reference(plan: &PlanId, of_plan: &str, key: &str) -> String {
    if of_plan == plan.as_str() { key.to_owned() } else { format!("{of_plan}/{key}") }
}

/// What holds PR `key` of `plan` back, read in the database (the same conditions as [`READY`]).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct NotReady {
    /// The needs that are not `merged`, as PR references.
    pub needs: Vec<String>,
    /// The base PR as `<plan>/<key>` when it has not started.
    pub base_pr: Option<String>,
}

impl NotReady {
    fn is_empty(&self) -> bool {
        self.needs.is_empty() && self.base_pr.is_none()
    }
}

fn not_ready(conn: &rusqlite::Connection, plan: &PlanId, key: &PrKey) -> Result<NotReady, CpdError> {
    let mut stmt = conn.prepare(concat!(
        "SELECT n.needs_plan, n.needs FROM prs r, pr_needs n, prs q WHERE r.plan = ?1 AND r.key = ?2 AND ",
        needs_unmerged!(),
        " ORDER BY n.needs_plan, n.needs"
    ))?;
    let needs = stmt
        .query_map([plan.as_str(), key.as_str()], |r| Ok(reference(plan, &r.get::<_, String>(0)?, &r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stmt = conn.prepare(concat!("SELECT b.plan, b.key FROM prs r, prs b WHERE r.plan = ?1 AND r.key = ?2 AND ", base_pr_waiting!()))?;
    let base_pr = stmt
        .query_map([plan.as_str(), key.as_str()], |r| Ok(format!("{}/{}", r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .pop();
    Ok(NotReady { needs, base_pr })
}

/// The PR's base PR as `<plan>/<key>` (§2.2 "The base PR": derived, the PR of its repo whose Lead Branch is its
/// `base`), `None` when it is not stacked.
pub(super) fn base_pr(conn: &rusqlite::Connection, pr: &PrRow) -> Result<Option<String>, CpdError> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row("SELECT plan || '/' || key FROM prs WHERE repo = ?1 AND lead_branch = ?2", [pr.repo.as_str(), pr.base.as_str()], |r| r.get(0))
        .optional()?)
}

/// The root base of a PR of `repo` whose `base` is `base` (§2.2, §7.5): the `base` of the first PR down the chain
/// of base PRs that is not stacked, the branch the stack lands on; `base` itself when the PR is not stacked.
pub fn root_base(conn: &rusqlite::Connection, repo: &str, base: &str) -> Result<String, CpdError> {
    use rusqlite::OptionalExtension;
    let mut base = base.to_owned();
    let mut seen = Vec::new();
    while let Some(next) = conn
        .query_row("SELECT base FROM prs WHERE repo = ?1 AND lead_branch = ?2", [repo, base.as_str()], |r| r.get::<_, String>(0))
        .optional()?
    {
        if seen.contains(&base) {
            return Err(CpdError::bad_row(format!("The base PRs of {repo} form a cycle at {base}")));
        }
        seen.push(base);
        base = next;
    }
    Ok(base)
}

/// The refs a protected path may be brought from by a base merge (§7.5), besides the Lead tip: `origin/<base>` (`<base>`
/// without `origin`), and for a stacked PR `origin/<root>` too, `<root>` the [`root_base`].
pub fn tamper_bases(conn: &rusqlite::Connection, dir: &Path, repo: &str, base: &str) -> Result<Vec<String>, CpdError> {
    let root = root_base(conn, repo, base)?;
    let mut refs = vec![git::tamper::base_ref(dir, base)?];
    if root != base {
        refs.push(git::tamper::base_ref(dir, &root)?);
    }
    Ok(refs)
}

/// A PR whose Lead worktree exists on its Lead Branch with `pre-start` passed.
/// Nothing is pushed and nothing is in the database.
#[derive(Debug)]
pub struct Prepared {
    pr: PrRow,
    lead: git::Prepared,
}

impl Prepared {
    /// Removes the worktree and branch again. Best effort: the failure to
    /// report is the caller's.
    pub fn discard(&self) {
        let _ = self.lead.discard(Path::new(&self.pr.repo));
    }

    /// Pushes a Lead Branch that is new on the remote (R35). A failed push does
    /// not fail the start: it is recorded with the start.
    pub fn push(self) -> Pushed {
        let (pushed, failed) = match self.lead.push() {
            Ok(pushed) => (pushed, None),
            Err(e) => (false, Some(e.with("pr", self.pr.key.as_str()))),
        };
        Pushed { prepared: self, pushed, failed }
    }
}

/// A [`Prepared`] PR after the push.
#[derive(Debug)]
pub struct Pushed {
    prepared: Prepared,
    pushed: bool,
    failed: Option<CpdError>,
}

impl Pushed {
    /// The PR's key.
    pub fn key(&self) -> &str {
        self.prepared.pr.key.as_str()
    }

    pub fn discard(&self) {
        self.prepared.discard();
    }

    /// The key, the Lead Branch and worktree and R35's outcome, as the results
    /// list a started PR (commands.md §4.8 `prs`, §9.7).
    pub fn to_json(&self) -> Value {
        let (pr, lead) = (&self.prepared.pr, &self.prepared.lead);
        json!({
            "key": pr.key, "status": "dispatch", "lead_branch": lead.branch,
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
    /// The database, or the PR changed meanwhile.
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
fn pre_start_log(home: &CpdHome, plan: &PlanId, key: &PrKey) -> PathBuf {
    home.plan_dir(plan).join("logs").join(format!("lead-{key}.{}.log", Hook::PreStart.as_str()))
}

/// Creates the PR's Lead worktree and runs `pre-start` in it (§7.2), in the PR's repo, for a PR that is not
/// stacked: the one `plan lock` starts (§3.1: it starts only PRs with no base PR).
pub fn prepare(home: &CpdHome, config: &Config, plan: &PlanId, pr: &PrRow) -> Result<Prepared, Failed> {
    prepare_as(home, config, plan, pr, false)
}

/// [`prepare`], `stacked` being whether the PR is stacked on a base PR: its Lead Branch is then cut from that
/// PR's Lead Branch head (§7.2).
fn prepare_as(home: &CpdHome, config: &Config, plan: &PlanId, pr: &PrRow, stacked: bool) -> Result<Prepared, Failed> {
    let key = &pr.key;
    let path = git::pr_lead_worktree_path(Path::new(&pr.repo), plan, key, &pr.lead_branch).map_err(Failed::Other)?;
    let setup = Setup {
        timeout: Duration::from_secs(u64::from(config.timeouts.setup_minutes) * 60),
        log: pre_start_log(home, plan, key),
    };
    match git::prepare_lead_worktree(Path::new(&pr.repo), plan, &path, &pr.lead_branch, &pr.base, stacked, &setup) {
        Ok(lead) => Ok(Prepared { pr: pr.clone(), lead }),
        Err(failure) => {
            let mut error = failure.error.with("pr", key.as_str());
            if error.code() == ErrorCode::SetupFailed {
                error = error.with("step", failure.step.as_str());
            }
            Err(Failed::Setup { step: failure.step, error })
        }
    }
}

/// Inside the transaction: the PR `waiting → dispatch` with its worktree, then
/// `pr-started`, and the `error` `push_failed` of a push that failed (R35).
/// Compare-and-swap on `status = 'waiting'`, else `cas_lost`.
pub fn record(tx: &WriteTxn<'_>, plan: &PlanId, pushed: &Pushed, by: By) -> Result<(), CpdError> {
    let (pr, lead) = (&pushed.prepared.pr, &pushed.prepared.lead);
    let worktree = lead.path.display().to_string();
    let moved = tx.execute(
        "UPDATE prs SET status = 'dispatch', lead_worktree = ?1, started_at = (SELECT at FROM txns WHERE id = cpd_txn()), start_failed_at = NULL
          WHERE plan = ?2 AND key = ?3 AND status = 'waiting'",
        [worktree.as_str(), plan.as_str(), pr.key.as_str()],
    )?;
    if moved == 0 {
        return Err(CpdError::new(ErrorCode::CasLost, format!("PR {} of Plan {plan} changed while it was started: rerun the command.", pr.key))
            .with("plan", plan.as_str())
            .with("pr", pr.key.as_str()));
    }
    let text = format!(
        "PR {} of Plan {plan} started: Lead Branch {} in {worktree}. Its Jobs are scheduled. Next: cpd plan show --plan {plan}",
        pr.key, lead.branch
    );
    let event = NewEvent::pr_started(text, &pr.key, &pr.repo, &pr.base, &lead.branch, &worktree, &lead.base_sha, lead.reused(), pushed.pushed, by.as_str());
    append(tx, event.plan(plan.clone()))?;
    if let Some(error) = &pushed.failed {
        let text = format!(
            "The Lead Branch {} of PR {} of Plan {plan} was not pushed to origin: {}. The next merge or cpd pr open pushes it again.",
            lead.branch,
            pr.key,
            error.message()
        );
        let event = NewEvent::error(text, by.source(), error.code().as_str(), error.message(), error.details());
        append(tx, event.plan(plan.clone()))?;
    }
    Ok(())
}

/// Starts one PR of a Plan in `dispatch` (E7, `pr start`): [`prepare`], push,
/// then [`record`] in a transaction of its own. On failure the worktree and
/// branch are removed and nothing is written.
pub fn start(home: &CpdHome, config: &Config, store: &mut Store, plan: &PlanId, pr: &PrRow, by: By) -> Result<Pushed, Failed> {
    let stacked = store.read(|conn| base_pr(conn, pr)).map_err(Failed::Other)?.is_some();
    let pushed = prepare_as(home, config, plan, pr, stacked)?.push();
    match store.write(by.source(), |tx| record(tx, plan, &pushed, by)) {
        Ok(()) => Ok(pushed),
        Err(e) => {
            pushed.discard();
            Err(Failed::Other(e))
        }
    }
}

/// Engine E7 failed to start a `waiting` PR (§3.8 "A failed start"): in one
/// transaction `start_failed_at` is set and one `error` Event `pr_start_failed`
/// is appended for the Plan's Lead, `source` being the engine's. False when the
/// PR is no longer `waiting` or was already marked: nothing is written.
pub fn record_failure(store: &mut Store, source: &str, plan: &PlanId, key: &PrKey, step: Step, error: &CpdError) -> Result<bool, CpdError> {
    store.write(source, |tx| {
        let marked = tx.execute(
            "UPDATE prs SET start_failed_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE plan = ?1 AND key = ?2 AND status = 'waiting' AND start_failed_at IS NULL",
            [plan.as_str(), key.as_str()],
        )?;
        if marked == 0 {
            return Ok(false);
        }
        let mut details = json!({"pr": key, "step": step.as_str()});
        for field in ["tail", "log"] {
            if let Some(value) = error.details().get(field).filter(|v| !v.is_null()) {
                details[field] = value.clone();
            }
        }
        let message = format!("PR {key} of Plan {plan} could not be started ({}): {}", step.as_str(), error.message());
        let text = format!("{message} The engine does not try again. Fix the cause, then: cpd pr start {key} --plan {plan}");
        let event = NewEvent::error(text, source, ErrorCode::PrStartFailed.as_str(), &message, details);
        append(tx, event.plan(plan.clone()))?;
        Ok(true)
    })
}

/// `pr start` (commands.md §9.7).
pub(super) fn run(args: StartArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let key = args.key;
    let mut env = Env::open()?;
    let (phase, prs, held) = env.store.read(|conn| Ok((plan_phase(conn, &plan)?, load(conn, &plan)?, not_ready(conn, &plan, &key)?)))?;
    if phase != Phase::Dispatch {
        return Err(wrong_phase(&plan, phase, COMMAND, "dispatch"));
    }
    let Some(pr) = prs.iter().find(|p| p.key == key) else {
        return Err(CpdError::new(ErrorCode::PrNotFound, format!("No PR {key} in Plan {plan}."))
            .with("plan", plan.as_str())
            .with("pr", key.as_str()));
    };
    match pr.status.as_str() {
        "waiting" => {}
        "cancelled" => {
            return Err(CpdError::new(ErrorCode::IllegalTransition, format!("PR {key} of Plan {plan} is cancelled: it cannot be started."))
                .with("plan", plan.as_str())
                .with("pr", key.as_str())
                .with("from", "cancelled")
                .with("to", "dispatch"));
        }
        _ => {
            return Ok(Output::success(json!({"pr": {
                "key": key, "status": pr.status, "lead_branch": pr.lead_branch, "lead_worktree": pr.lead_worktree,
                "base_sha": null, "reused": null, "pushed": null,
            }, "already": true})));
        }
    }
    if !held.is_empty() {
        let mut why = Vec::new();
        if !held.needs.is_empty() {
            why.push(format!("needs {}, which {} not merged yet", held.needs.join(", "), if held.needs.len() == 1 { "is" } else { "are" }));
        }
        if let Some(base_pr) = &held.base_pr {
            why.push(format!("is stacked on {base_pr}, which has not started yet"));
        }
        let mut error = CpdError::new(ErrorCode::PrNotReady, format!("PR {key} of Plan {plan} {}.", why.join(" and ")))
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
            .with("needs", held.needs);
        if let Some(base_pr) = held.base_pr {
            error = error.with("base_pr", base_pr);
        }
        return Err(error);
    }
    let pushed = start(&env.home, &env.config, &mut env.store, &plan, pr, By::PrStart).map_err(Failed::into_error)?;
    Ok(Output::success(json!({"pr": pushed.to_json()})))
}
