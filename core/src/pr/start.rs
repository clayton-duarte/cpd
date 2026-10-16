//! Starting a PR (contract §3.8 "The start", §7.2; commands.md §9.7): one
//! routine for `plan lock`, engine E7 and `pr start`. It creates the PR's Lead
//! worktree on its Lead Branch (R35's remote rules), runs `pre-start`, pushes a
//! new branch, then writes `status = dispatch` and `pr-started` in one
//! transaction. `plan lock` uses the steps one by one (all or nothing across
//! PRs); E7 and `pr start` use [`start`].

use std::path::PathBuf;
use std::time::Duration;

use rusqlite::Connection;
use serde_json::{Value, json};

use super::{Env, StartArgs, plan_phase, wrong_phase};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::git::{self, Setup, Step};
use crate::home::{CpdHome, Hook};
use crate::ids::{PlanId, PrKey};
use crate::output::Output;
use crate::plan::Phase;
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

/// A PR as the start reads it.
#[derive(Clone, Debug)]
pub struct Pr {
    pub key: PrKey,
    pub repo: PathBuf,
    pub base: String,
    pub lead_branch: String,
    pub status: String,
    /// Keys of the PRs it needs, sorted.
    pub needs: Vec<String>,
    pub lead_worktree: Option<String>,
}

type Raw = (String, String, String, String, String, Option<String>);

/// Every PR of `plan` in key order, `cancelled` ones included.
pub fn load(conn: &Connection, plan: &PlanId) -> Result<Vec<Pr>, CpdError> {
    let mut stmt = conn.prepare("SELECT key, repo, base, lead_branch, status, lead_worktree FROM prs WHERE plan = ?1 ORDER BY key")?;
    let rows: Vec<Raw> = stmt
        .query_map([plan.as_str()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut stmt = conn.prepare("SELECT needs FROM pr_needs WHERE plan = ?1 AND pr = ?2 ORDER BY needs")?;
    rows.into_iter()
        .map(|(key, repo, base, lead_branch, status, lead_worktree)| {
            let needs = stmt.query_map([plan.as_str(), key.as_str()], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let key = PrKey::parse(&key).map_err(|_| {
                CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: Plan {plan} has a PR key {key:?}."))
            })?;
            Ok(Pr { key, repo: PathBuf::from(repo), base, lead_branch, status, needs, lead_worktree })
        })
        .collect()
}

impl Pr {
    /// The needs that are not `merged` yet.
    pub fn unmerged_needs(&self, prs: &[Pr]) -> Vec<String> {
        self.needs.iter().filter(|n| prs.iter().any(|p| p.key.as_str() == n.as_str() && p.status != "merged")).cloned().collect()
    }
}

/// A PR whose Lead worktree exists on its Lead Branch with `pre-start` passed.
/// Nothing is pushed and nothing is in the database.
#[derive(Debug)]
pub struct Prepared {
    pr: Pr,
    lead: git::Prepared,
}

impl Prepared {
    /// Removes the worktree and branch again. Best effort: the failure to
    /// report is the caller's.
    pub fn discard(&self) {
        let _ = self.lead.discard(&self.pr.repo);
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

/// Creates the PR's Lead worktree and runs `pre-start` in it (§7.2), in the PR's repo.
pub fn prepare(home: &CpdHome, config: &Config, plan: &PlanId, pr: &Pr) -> Result<Prepared, Failed> {
    let key = &pr.key;
    let path = git::pr_lead_worktree_path(&pr.repo, plan, key, &pr.lead_branch).map_err(Failed::Other)?;
    let setup = Setup {
        timeout: Duration::from_secs(u64::from(config.timeouts.setup_minutes) * 60),
        log: pre_start_log(home, plan, key),
    };
    match git::prepare_lead_worktree(&pr.repo, plan, &path, &pr.lead_branch, &pr.base, &setup) {
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
    let data = json!({
        "pr": pr.key, "repo": pr.repo.display().to_string(), "base": pr.base, "lead_branch": lead.branch,
        "lead_worktree": worktree, "base_sha": lead.base_sha, "reused": lead.reused(), "pushed": pushed.pushed,
        "by": by.as_str(),
    });
    append(tx, NewEvent::new(Kind::PrStarted, text, data).plan(plan.clone()))?;
    if let Some(error) = &pushed.failed {
        let text = format!(
            "The Lead Branch {} of PR {} of Plan {plan} was not pushed to origin: {}. The next merge or cpd pr open pushes it again.",
            lead.branch,
            pr.key,
            error.message()
        );
        let data = json!({"source": by.source(), "code": error.code().as_str(), "message": error.message(), "details": error.details()});
        append(tx, NewEvent::new(Kind::Error, text, data).plan(plan.clone()))?;
    }
    Ok(())
}

/// Starts one PR of a Plan in `dispatch` (E7, `pr start`): [`prepare`], push,
/// then [`record`] in a transaction of its own. On failure the worktree and
/// branch are removed and nothing is written.
pub fn start(home: &CpdHome, config: &Config, store: &mut Store, plan: &PlanId, pr: &Pr, by: By) -> Result<Pushed, Failed> {
    let pushed = prepare(home, config, plan, pr)?.push();
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
        let data = json!({"source": source, "code": ErrorCode::PrStartFailed.as_str(), "message": message, "details": details});
        append(tx, NewEvent::new(Kind::Error, text, data).plan(plan.clone()))?;
        Ok(true)
    })
}

/// `pr start` (commands.md §9.7).
pub(super) fn run(args: StartArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let key = args.key;
    let mut env = Env::open()?;
    let (phase, prs) = env.store.read(|conn| Ok((plan_phase(conn, &plan)?, load(conn, &plan)?)))?;
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
    let unmerged = pr.unmerged_needs(&prs);
    if !unmerged.is_empty() {
        return Err(CpdError::new(
            ErrorCode::PrNotReady,
            format!("PR {key} of Plan {plan} needs {}, which {} not merged yet.", unmerged.join(", "), if unmerged.len() == 1 { "is" } else { "are" }),
        )
        .with("plan", plan.as_str())
        .with("pr", key.as_str())
        .with("needs", unmerged));
    }
    let pushed = start(&env.home, &env.config, &mut env.store, &plan, pr, By::PrStart).map_err(Failed::into_error)?;
    Ok(Output::success(json!({"pr": pushed.to_json()})))
}
