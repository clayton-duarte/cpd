//! What the driver and the merge read from and write to the Ledger (contract
//! §3.2, §3.4, §5.2): the Attempt, with its Job's PR and its Plan, and the
//! transactions that end it. The driver ends an Attempt before it is
//! `merging`; the merge (`crate::merge`) ends a `merging` one.

use std::path::PathBuf;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::config::{Catalog, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::events::repeat::{self, Same, Scope};
use crate::events::{self, NewEvent};
use crate::ids::{AttemptId, ContextId, JobKey, PlanId, SessionId};
use crate::store::WriteTxn;

/// An Attempt with what the driver and the merge need of its Job, PR and Plan.
#[derive(Clone, Debug)]
pub struct Attempt {
    pub plan: PlanId,
    pub context: ContextId,
    /// The repo of the Job's PR: the realpath of its main worktree.
    pub repo: PathBuf,
    /// The Lead Branch of the Job's PR (R34 A5).
    pub lead_branch: String,
    /// The branch every push and `gh pr list --head` of the PR names: the pull request's head of an adopted PR, else the Lead Branch.
    pub remote_branch: String,
    /// Whether the PR is an adopted pull request (`prs.head_ref` is set) that has not opened: only its `open` and `update` Jobs publish.
    pub adopted: bool,
    pub lead_worktree: PathBuf,
    pub id: AttemptId,
    pub job: JobKey,
    pub tier: Tier,
    pub status: String,
    /// `cpd/<plan>--<attempt>`.
    pub branch: String,
    pub worktree: PathBuf,
    pub worker_session: Option<SessionId>,
    pub nudged: bool,
    pub stop_request: Option<String>,
    /// The Attempt branch's tip when `attempt submit` checked it; set from `merging` on.
    pub submitted_sha: Option<String>,
    /// The Job's kind (§2.2 "Job kinds"): `open` and `update` Jobs are merged without a rebase and with a push (§7.4).
    pub kind: String,
    /// The Job's PR (R34 A7).
    pub pr: Option<String>,
}

impl Attempt {
    /// The key of the Job's PR; empty for a Job with none (an explore Job), which no PR key equals.
    pub fn pr_str(&self) -> &str {
        self.pr.as_deref().unwrap_or_default()
    }
}

pub(crate) fn corrupt(what: String) -> CpdError {
    CpdError::new(
        ErrorCode::StoreFailed,
        format!("The Ledger holds an invalid value: {what}."),
    )
}

/// Attempt `id` of `plan`: `plan_not_found`, else `attempt_not_found` (exit 4).
pub fn load(conn: &Connection, plan: &PlanId, id: &AttemptId) -> Result<Attempt, CpdError> {
    let found: Option<String> = conn
        .query_row(
            "SELECT context FROM plans WHERE id = ?1",
            [plan.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    let context = found.ok_or_else(|| CpdError::plan_not_found(plan))?;
    // The repo and Lead worktree are those of the Job's PR (R34 A7).
    let row = conn
        .query_row(
            "SELECT a.job, a.tier, a.status, a.branch, a.worktree, a.worker_session, a.nudged, a.stop_request,
                    r.repo, r.lead_worktree, r.lead_branch, a.submitted_sha, j.kind, j.pr, r.head_ref, r.status
               FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
               LEFT JOIN prs r ON r.plan = j.plan AND r.key = j.pr
              WHERE a.plan = ?1 AND a.id = ?2",
            params![plan.as_str(), id.to_string()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, bool>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    (r.get::<_, Option<String>>(8)?, r.get::<_, Option<String>>(9)?, r.get::<_, Option<String>>(10)?),
                    r.get::<_, Option<String>>(11)?,
                    (r.get::<_, String>(12)?, r.get::<_, Option<String>>(13)?, r.get::<_, Option<String>>(14)?, r.get::<_, Option<String>>(15)?),
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            CpdError::new(ErrorCode::AttemptNotFound, format!("No Attempt {id} in Plan {plan}."))
                .with("plan", plan.as_str())
                .with("attempt", id.to_string())
        })?;
    let (
        job,
        tier,
        status,
        branch,
        worktree,
        worker_session,
        nudged,
        stop_request,
        (repo, lead_worktree, lead_branch),
        submitted_sha,
        (kind, pr, head_ref, pr_status),
    ) = row;
    // An explore Attempt has no PR: its repo and Lead Branch are none (empty), its Lead worktree is its own directory.
    let (repo, lead_worktree, lead_branch) = match (pr.is_some(), repo, lead_worktree, lead_branch)
    {
        (true, Some(repo), Some(lead_worktree), Some(lead_branch)) => {
            (repo, lead_worktree, lead_branch)
        }
        (true, ..) => {
            return Err(corrupt(format!(
                "Plan {plan} has an Attempt but no Lead worktree"
            )));
        }
        (false, ..) => (String::new(), worktree.clone(), String::new()),
    };
    Ok(Attempt {
        plan: plan.clone(),
        context: ContextId::parse(&context)
            .map_err(|_| corrupt(format!("context id {context:?}")))?,
        repo: PathBuf::from(repo),
        remote_branch: head_ref.clone().unwrap_or_else(|| lead_branch.clone()),
        adopted: head_ref.is_some() && pr_status.as_deref() != Some("open"),
        lead_branch,
        lead_worktree: PathBuf::from(lead_worktree),
        id: id.clone(),
        job: JobKey::parse(&job).map_err(|_| corrupt(format!("job key {job:?}")))?,
        tier: Tier::parse(&tier).map_err(|_| corrupt(format!("tier {tier:?} of Attempt {id}")))?,
        status,
        branch,
        worktree: PathBuf::from(worktree),
        worker_session: worker_session
            .map(|s| SessionId::parse(&s).map_err(|_| corrupt(format!("session id {s:?}"))))
            .transpose()?,
        nudged,
        stop_request,
        submitted_sha,
        kind,
        pr,
    })
}

/// The Job's cancel reason when a cancel was requested (§3.2 "Cancel").
pub fn cancel_request(
    conn: &Connection,
    plan: &PlanId,
    job: &JobKey,
) -> Result<Option<String>, CpdError> {
    Ok(conn
        .query_row(
            "SELECT cancel_reason FROM jobs WHERE plan = ?1 AND key = ?2 AND cancel_requested_at IS NOT NULL",
            params![plan.as_str(), job.as_str()],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

/// The causes an Attempt ends `failed` with (§3.4), declared once: a variant cannot exist without being listed in
/// [`Cause::ALL`], spelled by [`Cause::as_str`] and read by [`Cause::parse`].
macro_rules! causes {
    ($($(#[$meta:meta])* $variant:ident = $text:literal),+ $(,)?) => {
        /// The causes an Attempt ends `failed` with (§3.4).
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Cause {
            $($(#[$meta])* $variant),+
        }

        impl Cause {
            /// Every cause, in declaration order.
            pub const ALL: &'static [Cause] = &[$(Cause::$variant),+];

            /// As the `attempts` table spells it.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Cause::$variant => $text),+
                }
            }
        }
    };
}

causes! {
    /// A1–A2: the worktree or `pre-start` failed.
    SetupFailed = "setup_failed",
    /// A5 after E2 set `stop_request = timed_out`.
    TimedOut = "timed_out",
    /// `attempt abort` (commands.md §6.5, contract §3.4, hs-a8): the Lead ended a running Attempt. A no-ladder
    /// cause, like `timed_out` and `setup_failed`: `next_rung` returns `None`, so the Job goes `failed`.
    Aborted = "aborted",
    /// The Worker pane is dead, pi stopped badly, the driver failed, or (R3)
    /// the driver died before the merge's compare-and-swap.
    Crashed = "crashed",
    /// A5: idle twice without a submit, nudged in between.
    NoSubmit = "no_submit",
    /// Merge step 5: the rebase conflicted.
    MergeConflict = "merge_conflict",
    /// Merge step 8: the Lead Branch kept moving.
    CasLost = "cas_lost",
    /// Declared, never produced since hs-d243 (merge step 3, the clean Lead worktree check, was removed).
    DirtyTarget = "dirty_target",
    /// Merge step 4: the diff touches protected paths (§7.5).
    Tampered = "tampered",
    /// The merge Gate (`gate.merge` or `gate.pr`) exited non-zero (§7.4 step 6).
    GateFailed = "gate_failed",
    /// The merge Gate could not be judged: killed, at the timeout, or hook_invalid (§7.4 step 6).
    GateError = "gate_error",
    /// A5: pi stopped on a provider error ("Connection error.", "Request timed out.", HTTP 5xx): a retry cause, not a ladder cause.
    ProviderError = "provider_error",
}

/// What `attempts.cause` stores (§3.4): `merged`, `cancel_requested` or the [`Cause`] of a failed Attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stored {
    Merged,
    CancelRequested,
    Failed(Cause),
}

impl Stored {
    /// As the `attempts` table spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Stored::Merged => "merged",
            Stored::CancelRequested => "cancel_requested",
            Stored::Failed(cause) => cause.as_str(),
        }
    }

    /// The stored text, or `None` for a value outside the table's CHECK. Never panics.
    pub fn parse(value: &str) -> Option<Stored> {
        match value {
            "merged" => Some(Stored::Merged),
            "cancel_requested" => Some(Stored::CancelRequested),
            other => Cause::parse(other).map(Stored::Failed),
        }
    }

    /// `merged`, `cancelled` or `failed` (the `attempts` table's CHECKs).
    pub fn outcome(self) -> &'static str {
        match self {
            Stored::Merged => "merged",
            Stored::CancelRequested => "cancelled",
            Stored::Failed(_) => "failed",
        }
    }
}

impl Cause {
    /// The cause spelled `value`, or `None`.
    pub fn parse(value: &str) -> Option<Cause> {
        Cause::ALL.iter().copied().find(|c| c.as_str() == value)
    }

    /// A ladder cause climbs one rung when the Job is requeued (§3.4, §9): `no_submit` only.
    pub fn is_ladder(self) -> bool {
        matches!(self, Cause::NoSubmit)
    }

    /// The causes whose retry keeps the Attempt's own Tier: `provider_error`, `crashed`, `merge_conflict`
    /// (pro-d102, pro-d265). The ledger and the starter both rely on this one rule.
    pub fn retries_same_tier(self) -> bool {
        matches!(
            self,
            Cause::ProviderError | Cause::Crashed | Cause::MergeConflict
        )
    }
}

/// How many times a Job is requeued at the same Tier after a `provider_error`, counted since its last `job retry` (pro-d102 A1).
pub const PROVIDER_RETRIES: u32 = 3;

/// How many times a Job is retried at the same Tier after a `gate_failed`, counted since its last `job retry` (pro-d280).
pub const GATE_RETRIES: u32 = 1;

/// How long E3 holds a Job after an Attempt that ended `provider_error` (pro-d102 A2).
pub const PROVIDER_RETRY_SECONDS: u64 = 30;

/// How an Attempt ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// `merged` / `merged`: the Lead Branch is at `sha` (the merge only).
    Merged { sha: String },
    /// `cancelled` / `cancel_requested`.
    Cancelled,
    /// The stop incident mode requested (contract §3.4/§3.7): the Attempt ended
    /// `cancelled` / `cancel_requested` and its Job returns to `queued` at the same
    /// Tier, as an Interrupted Attempt. Stub: the inc-engine Job implements it.
    Interrupted,
    /// `failed` / `cause`; `detail` explains it (for `merge_conflict`, the files).
    Failed { cause: Cause, detail: String },
    /// `failed` / `gate_failed`: a Gate that exited non-zero, with the facts written in the same UPDATE as the end.
    GateFailed { detail: String, facts: GateFacts },
}

/// What a failing Gate leaves on its Attempt (`gate_hook`, `gate_command`, `gate_exit`, `check_tail`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateFacts {
    /// `pre-commit` or `pre-merge`.
    pub hook: String,
    /// The rendered text of the first command that exited non-zero.
    pub command: String,
    pub exit: i32,
    /// The Gate output's last lines.
    pub tail: String,
}

impl End {
    pub fn outcome(&self) -> &'static str {
        match self {
            End::Merged { .. } => "merged",
            End::Cancelled | End::Interrupted => "cancelled",
            End::Failed { .. } | End::GateFailed { .. } => "failed",
        }
    }

    pub fn cause(&self) -> &'static str {
        match self {
            End::Merged { .. } => "merged",
            End::Cancelled | End::Interrupted => "cancel_requested",
            End::Failed { cause, .. } => cause.as_str(),
            End::GateFailed { .. } => Cause::GateFailed.as_str(),
        }
    }

    fn failure(&self) -> Option<(Cause, &str)> {
        match self {
            End::Failed { cause, detail } => Some((*cause, detail)),
            End::GateFailed { detail, .. } => Some((Cause::GateFailed, detail)),
            _ => None,
        }
    }
}

/// What the ladder needs (§9): pi's catalog and `retry_cap`.
pub struct Ladder<'a> {
    pub catalog: &'a Catalog,
    pub retry_cap: u32,
}

/// Who ends the Attempt, and so which status it must still be in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The driver or the engine, before the merge: `starting` or `running`.
    /// Never [`End::Merged`].
    Driver,
    /// The merge, or R: `merging`.
    Merge,
}

impl Stage {
    fn accepts(self, status: &str) -> bool {
        match self {
            Stage::Driver => matches!(status, "starting" | "running"),
            Stage::Merge => status == "merging",
        }
    }

    /// Who the `cas_lost` message blames.
    fn ran(self) -> &'static str {
        match self {
            Stage::Driver => "driver",
            Stage::Merge => "merge",
        }
    }
}

/// Ends `attempt`, which must still be in the status it was read in (and in
/// one `stage` accepts), and moves its Job out of `running`, in the caller's
/// transaction (§3.2, §3.4): `cas_lost` when either changed. Records
/// `metrics` (A9) and appends the Event of §3.4's table: `job-merged`,
/// `job-cancelled`, `attempt-retry` or `job-failed`.
pub fn end(
    tx: &WriteTxn<'_>,
    attempt: &Attempt,
    end: &End,
    metrics: Option<&Value>,
    ladder: &Ladder<'_>,
    stage: Stage,
) -> Result<(), CpdError> {
    end_opened(tx, attempt, end, metrics, ladder, stage, None)
}

/// A8 and the end of an explore Attempt (contract §7.7 "Explore kinds at submit"), in the caller's transaction: the Attempt
/// `running` to `ended` / `merged`, the Job `merged` with its `summary`, and the `job-merged` Event of §5.2 with the three-line
/// text of §5.3. `sha` is the artefact's SHA-256 (what was accepted; no commit exists). No compare-and-swap: `cas_lost` only when
/// the Attempt or the Job is no longer `running`.
pub fn end_explore(
    tx: &WriteTxn<'_>,
    attempt: &Attempt,
    sha: &str,
    summary: &str,
) -> Result<(), CpdError> {
    let (plan, job, id) = (
        attempt.plan.as_str(),
        attempt.job.as_str(),
        attempt.id.to_string(),
    );
    let cas_lost = |what: &str| {
        CpdError::new(
            ErrorCode::CasLost,
            format!("{what} changed while the submit ran; nothing was written."),
        )
        .with("plan", plan)
        .with("attempt", id.clone())
    };
    let changed = tx.execute(
        "UPDATE attempts SET status = 'ended', outcome = 'merged', cause = 'merged', merged_sha = ?3, submitted_sha = ?3, actual_files = '[]',
                submitted_at = (SELECT at FROM txns WHERE id = cpd_txn()), ended_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE plan = ?1 AND id = ?2 AND status = 'running'",
        params![plan, id, sha],
    )?;
    if changed == 0 {
        return Err(cas_lost("The Attempt's status"));
    }
    let changed = tx.execute(
        "UPDATE jobs SET status = 'merged', merged_sha = ?3, summary = ?4 WHERE plan = ?1 AND key = ?2 AND status = 'running'",
        params![plan, job, sha, summary],
    )?;
    if changed == 0 {
        return Err(cas_lost("The Job's status"));
    }
    let artefact = format!("run/{plan}/explore/{job}.md");
    let shown = summary.trim_end();
    let text = format!(
        "Job {job} ({kind}) merged: findings in $CPD_HOME/{artefact}.\nSummary: {shown}\nNext: cite that path in the Specs that need it; add another explore Job when the summary is not enough. cpd job show {job} --plan {plan}",
        kind = attempt.kind
    );
    events::append(
        tx,
        NewEvent::job_merged_explore(
            text,
            sha,
            &attempt.tier.to_string(),
            &attempt.kind,
            summary,
            &artefact,
        )
        .plan(attempt.plan.clone())
        .job(attempt.job.clone())
        .attempt(attempt.id.clone()),
    )?;
    Ok(())
}

/// The pull request an `open` Job's merge created or adopted (§7.4 step 8b): what `job-merged` says of it (§5.2).
#[derive(Clone, Copy, Debug)]
pub struct Pull<'a> {
    pub number: i64,
    pub url: &'a str,
}

/// [`end`], with the pull request an `open` Job's merge opened, which `job-merged` names (`pr`, `number`, `url`; its `text` says the pull
/// request is open: it is the Lead's only word of it, §5.2).
pub fn end_opened(
    tx: &WriteTxn<'_>,
    attempt: &Attempt,
    end: &End,
    metrics: Option<&Value>,
    ladder: &Ladder<'_>,
    stage: Stage,
    pull: Option<Pull<'_>>,
) -> Result<(), CpdError> {
    debug_assert!(
        stage == Stage::Merge || !matches!(end, End::Merged { .. }),
        "only the merge ends an Attempt merged"
    );
    let (plan, job, id) = (
        attempt.plan.as_str(),
        attempt.job.as_str(),
        attempt.id.to_string(),
    );
    let cas_lost = |what: &str| {
        CpdError::new(
            ErrorCode::CasLost,
            format!(
                "{what} changed while the {} ran; nothing was written.",
                stage.ran()
            ),
        )
        .with("plan", plan)
        .with("attempt", id.clone())
    };
    if !stage.accepts(&attempt.status) {
        return Err(cas_lost("The Attempt's status").with("status", attempt.status.clone()));
    }
    let merged_sha = match end {
        End::Merged { sha } => Some(sha.as_str()),
        _ => None,
    };
    // An Interrupted Attempt does not use up a try (contract §3.7): its `driver_pid` is cleared in the same
    // UPDATE (an `attempts` trigger refuses a second one), so the ladder's `automatic_attempts` leaves it out.
    let interrupted = matches!(end, End::Interrupted);
    let facts = match end {
        End::GateFailed { facts, .. } => Some(facts),
        _ => None,
    };
    let changed = tx.execute(
        "UPDATE attempts SET status = 'ended', outcome = ?4, cause = ?5, merged_sha = ?6, metrics = ?7,
                driver_pid = CASE WHEN ?8 THEN NULL ELSE driver_pid END,
                gate_hook = ?9, gate_command = ?10, gate_exit = ?11, check_tail = ?12,
                ended_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE plan = ?1 AND id = ?2 AND status = ?3",
        params![plan, id, attempt.status, end.outcome(), end.cause(), merged_sha, metrics.map(Value::to_string), interrupted,
            facts.map(|f| f.hook.as_str()), facts.map(|f| f.command.as_str()), facts.map(|f| f.exit), facts.map(|f| f.tail.as_str())
        ],
    )?;
    if changed == 0 {
        return Err(cas_lost("The Attempt's status"));
    }
    // hs-d798: the same transaction that wrote `attempts.metrics` appends the `attempt-ended` Event, whose `data`
    // is the metrics blob: the ten keys copied from it, or all ten null when there are none.
    let tier = attempt.tier.to_string();
    let show = format!("cpd job show {job} --plan {plan}");
    let ended = format!(
        "Attempt {id} of Job {job} ended ({}, {}). Next: {show}",
        end.outcome(),
        end.cause()
    );
    events::append(
        tx,
        NewEvent::attempt_ended(ended, metrics)
            .plan(attempt.plan.clone())
            .job(attempt.job.clone())
            .attempt(attempt.id.clone()),
    )?;
    let (status, failed_cause, event) = match end {
        End::Merged { sha } => {
            // hs-d514: the merge that leaves a draft and no queued or running Job but `open` and `update` Jobs turns the Lead. An `open` Job's own merge never turns it.
            let turn = attempt.kind != "open"
                && tx.query_row(
                    "SELECT EXISTS (SELECT 1 FROM jobs WHERE plan = ?1 AND status = 'draft')
                        AND NOT EXISTS (SELECT 1 FROM jobs WHERE plan = ?1 AND key != ?2 AND status IN ('queued', 'running')
                                          AND kind NOT IN ('open', 'update'))",
                    params![attempt.plan.as_str(), attempt.job.as_str()],
                    |r| r.get::<_, bool>(0),
                )?;
            let cancel_too_late = cancel_request(tx, &attempt.plan, &attempt.job)?.is_some();
            let mut text = format!(
                "Job {job} merged: Attempt {id} fast-forwarded {} to {}.",
                attempt.lead_branch,
                short(sha)
            );
            if cancel_too_late {
                text.push_str(" Its cancel request came too late: the merge had already happened.");
            }
            if let Some(pull) = pull {
                text.push_str(&format!(
                    " Pull request #{} of PR {} is open: {}.",
                    pull.number,
                    attempt.pr_str(),
                    pull.url
                ));
            }
            text.push_str(&format!(" Next: {show}"));
            if attempt.kind == "work" {
                let open: i64 = tx.query_row(
                    "SELECT coalesce((SELECT json_array_length(r.feedback) FROM prs p JOIN pull_requests r ON r.id = p.pull_request
                                       WHERE p.plan = ?1 AND p.key = ?2 AND p.status = 'open'), 0)",
                    params![plan, attempt.pr_str()],
                    |r| r.get(0),
                )?;
                if open > 0 {
                    let key = attempt.pr_str();
                    text.push_str(&format!(" PR {key} still has {open} open review comments: cpd pr view --pr {key}, then cpd pr reply."));
                }
            }
            let pull = pull.map(|p| (attempt.pr_str(), p.number, p.url));
            (
                "merged",
                None,
                NewEvent::job_merged(text, sha, &tier, &attempt.kind, turn, cancel_too_late, pull),
            )
        }
        End::Cancelled => {
            let reason = cancel_request(tx, &attempt.plan, &attempt.job)?
                .ok_or_else(|| cas_lost("The Job's cancel request"))?;
            let text = match stage {
                Stage::Driver => format!(
                    "Job {job} cancelled ({reason}): its driver stopped Attempt {id}. Next: cpd plan show --plan {plan}"
                ),
                Stage::Merge => format!(
                    "Job {job} cancelled ({reason}): Attempt {id} stopped before its merge. Next: cpd plan show --plan {plan}"
                ),
            };
            ("cancelled", None, NewEvent::job_cancelled(text, &reason))
        }
        End::Interrupted => {
            // The Job has no `cancel_requested_at` (the caller decided that): the stop is incident mode's, and the Job
            // returns to `queued` at the same Tier with an `attempt-retry` Event naming incident mode (contract §3.7).
            let next = attempt.tier.to_string();
            let feedback = format!("Incident mode stopped Attempt {id} of Job {job}.");
            let text = format!(
                "Attempt {id} of Job {job} was interrupted by incident mode. The Job is queued again at Tier {next}. Next: {show}"
            );
            (
                "queued",
                None,
                NewEvent::attempt_retry(text, "cancelled", "cancel_requested", &next, &feedback),
            )
        }
        End::Failed { .. } | End::GateFailed { .. } => {
            let (cause, detail) = end.failure().expect("a failed end");
            let (cause, detail) = (&cause, &detail.to_owned());
            match next_tier(tx, attempt, *cause, ladder)? {
                Some(next) => {
                    let said = match cause {
                        Cause::ProviderError => format!(
                            "{detail} Retry {} of {PROVIDER_RETRIES} at the same Tier, not before {PROVIDER_RETRY_SECONDS} s have passed.",
                            ended_with(tx, plan, job, Cause::ProviderError)?
                        ),
                        _ => detail.clone(),
                    };
                    let gate = (*cause == Cause::GateFailed)
                        .then(|| crate::merge::gate_text_body(detail))
                        .flatten();
                    let text = match gate {
                        Some(body) => format!(
                            "Attempt {id} of Job {job} failed ({}): {body}\nThe Job is queued again at Tier {next}. Next: {show}",
                            cause.as_str()
                        ),
                        None => format!(
                            "Attempt {id} of Job {job} failed ({}): {said} The Job is queued again at Tier {next}. Next: {show}",
                            cause.as_str()
                        ),
                    };
                    (
                        "queued",
                        None,
                        NewEvent::attempt_retry(
                            text,
                            "failed",
                            cause.as_str(),
                            &next.to_string(),
                            detail,
                        ),
                    )
                }
                None => {
                    let attempts: i64 = tx.query_row(
                        "SELECT count(*) FROM attempts WHERE plan = ?1 AND job = ?2",
                        params![plan, job],
                        |r| r.get(0),
                    )?;
                    let used_up = match cause {
                        Cause::ProviderError => {
                            format!(" The {PROVIDER_RETRIES} retries are used up.")
                        }
                        _ => String::new(),
                    };
                    let gate = matches!(cause, Cause::GateFailed | Cause::GateError)
                        .then(|| crate::merge::gate_text_body(detail))
                        .flatten();
                    let text = match gate {
                        Some(body) => format!(
                            "Job {job} failed ({}): {body}\nDecide: cpd job retry {job} --tier <T> --plan {plan}, or cpd job cancel {job} --reason <text> --plan {plan}",
                            cause.as_str()
                        ),
                        None => format!(
                            "Job {job} failed ({}): {detail}{used_up} Decide: cpd job retry {job} --tier <T> --plan {plan}, or cpd job cancel {job} --reason <text> --plan {plan}",
                            cause.as_str()
                        ),
                    };
                    (
                        "failed",
                        Some(cause.as_str()),
                        NewEvent::job_failed(text, cause.as_str(), detail, &tier, attempts),
                    )
                }
            }
        }
    };
    let changed = tx.execute(
        "UPDATE jobs SET status = ?3, merged_sha = ?4, failed_cause = ?5 WHERE plan = ?1 AND key = ?2 AND status = 'running'",
        params![plan, job, status, merged_sha, failed_cause],
    )?;
    if changed == 0 {
        return Err(cas_lost("The Job's status"));
    }
    events::append(
        tx,
        event
            .plan(attempt.plan.clone())
            .job(attempt.job.clone())
            .attempt(attempt.id.clone()),
    )?;
    // pro-d89: a merge that is not an `open` or `update` Job's own requeues the failed `open`/`update` Jobs of its PR that it fixes.
    if matches!(end, End::Merged { .. }) && attempt.kind != "open" && attempt.kind != "update" {
        crate::job::fix_retry::retry_after_fix(tx, &attempt.plan, &attempt.job)?;
    }
    // The transaction that makes the Job `cancelled` also fails or cancels its dependents (contract §3.2 "Cancel", hs-d647).
    if matches!(end, End::Cancelled) {
        let reason = cancel_request(tx, &attempt.plan, &attempt.job)?.unwrap_or_default();
        crate::job::cancel::fail_dependents(tx, &attempt.plan, &attempt.job, &reason)?;
    }
    Ok(())
}

/// The automatic Attempts of Job `job` since the last `job retry` (that one
/// included), for `retry_cap` (§9). Only an Attempt that ended `merged` or
/// `failed` counts; an Interrupted (or otherwise `cancelled`) one never does
/// (contract §3.7). An ended Attempt whose driver never started (`driver_pid`
/// null, §3.4) is not one either: it does not use up a try.
pub(crate) fn automatic_attempts(
    tx: &WriteTxn<'_>,
    plan: &str,
    job: &str,
) -> Result<i64, CpdError> {
    Ok(tx.query_row(
        "SELECT count(*) FROM attempts WHERE plan = ?1 AND job = ?2
           AND number >= coalesce((SELECT max(number) FROM attempts WHERE plan = ?1 AND job = ?2 AND origin = 'retry'), 1)
           AND status = 'ended' AND outcome IN ('merged', 'failed') AND driver_pid IS NOT NULL
           AND cause <> 'provider_error'",
        params![plan, job],
        |r| r.get(0),
    )?)
}

/// The Attempts of Job `job` that ended with `cause` since the last `job retry` (that one included): the window of
/// [`automatic_attempts`], for the provider-error (pro-d102) and gate (pro-d280) retries.
fn ended_with(tx: &WriteTxn<'_>, plan: &str, job: &str, cause: Cause) -> Result<u32, CpdError> {
    Ok(tx.query_row(
        "SELECT count(*) FROM attempts WHERE plan = ?1 AND job = ?2
           AND number >= coalesce((SELECT max(number) FROM attempts WHERE plan = ?1 AND job = ?2 AND origin = 'retry'), 1)
           AND status = 'ended' AND cause = ?3",
        params![plan, job, cause.as_str()],
        |r| r.get(0),
    )?)
}

/// The next Tier (§3.2, §9) for a `no_submit` (a ladder cause: `catalog.next`, if the
/// ladder has one) or a `crashed` / `merge_conflict` (the Attempt's own Tier), while the
/// automatic Attempts since the last `job retry` are fewer than `1 + retry_cap`. An Attempt whose driver never started
/// (`driver_pid` null, §3.4) does not raise the Tier: its own Tier again.
/// (A `merging` Attempt always had a driver.) A `provider_error` is a retry cause: see below.
fn next_tier(
    tx: &WriteTxn<'_>,
    attempt: &Attempt,
    cause: Cause,
    ladder: &Ladder<'_>,
) -> Result<Option<Tier>, CpdError> {
    let next = next_rung(tx, attempt, cause, ladder)?;
    // An explore Job stays inside the letter of its kind (scout H, research S): a `no_submit` after the top rung fails.
    Ok(next.filter(|t| {
        !crate::plan::jobs::is_explore(&attempt.kind) || t.letter() == attempt.tier.letter()
    }))
}

fn next_rung(
    tx: &WriteTxn<'_>,
    attempt: &Attempt,
    cause: Cause,
    ladder: &Ladder<'_>,
) -> Result<Option<Tier>, CpdError> {
    // pro-d102: a provider error requeues at the Attempt's own Tier, never raising it, up to PROVIDER_RETRIES times.
    if cause == Cause::ProviderError {
        let used = ended_with(tx, attempt.plan.as_str(), attempt.job.as_str(), cause)?;
        return Ok((used <= PROVIDER_RETRIES).then_some(attempt.tier));
    }
    // pro-d280: a failed Gate requeues once at the Attempt's own Tier, and spends `retry_cap`.
    if cause == Cause::GateFailed {
        let (plan, job) = (attempt.plan.as_str(), attempt.job.as_str());
        // A failed Gate proves the Attempt ran: count this one even when its `driver_pid` is null.
        let uncounted: bool = tx.query_row(
            "SELECT driver_pid IS NULL FROM attempts WHERE plan = ?1 AND id = ?2",
            params![plan, attempt.id.to_string()],
            |r| r.get(0),
        )?;
        if automatic_attempts(tx, plan, job)? + i64::from(uncounted) > i64::from(ladder.retry_cap) {
            return Ok(None);
        }
        return Ok((ended_with(tx, plan, job, cause)? <= GATE_RETRIES).then_some(attempt.tier));
    }
    if !cause.is_ladder() && !matches!(cause, Cause::Crashed | Cause::MergeConflict) {
        return Ok(None);
    }
    let never_started: bool = tx.query_row(
        "SELECT driver_pid IS NULL FROM attempts WHERE plan = ?1 AND id = ?2",
        params![attempt.plan.as_str(), attempt.id.to_string()],
        |r| r.get(0),
    )?;
    if never_started {
        return Ok(Some(
            ladder.catalog.step_up(attempt.tier).unwrap_or(attempt.tier),
        ));
    }
    // hs-n465, hs-d632: the catalog refuses the Attempt's Tier: step to the next valid Worker Tier at or above it, never propagate `tier_invalid`.
    if !ladder.catalog.is_valid(attempt.tier) {
        return Ok(ladder.catalog.step_up(attempt.tier));
    }
    if automatic_attempts(tx, attempt.plan.as_str(), attempt.job.as_str())?
        > i64::from(ladder.retry_cap)
    {
        return Ok(None);
    }
    // pro-d265: a crash or a conflict is not a capability problem: same Tier. Only `no_submit` climbs.
    if cause.is_ladder() {
        ladder.catalog.next(attempt.tier)
    } else {
        Ok(Some(attempt.tier))
    }
}

/// Appends an `error` Event about `attempt` (§5.2) for `error`, from `source`.
pub fn append_error(
    tx: &WriteTxn<'_>,
    attempt: &Attempt,
    source: &str,
    error: &CpdError,
    text: String,
    window: Duration,
) -> Result<(), CpdError> {
    let same = Same::Message {
        source,
        code: error.code().as_str(),
        message: error.message(),
        scope: Scope::Plan(&attempt.plan),
    };
    repeat::append_once(
        tx,
        window,
        &same,
        NewEvent::error(
            text,
            source,
            error.code().as_str(),
            error.message(),
            error.details(),
        )
        .plan(attempt.plan.clone())
        .job(attempt.job.clone())
        .attempt(attempt.id.clone()),
    )?;
    Ok(())
}

/// Whether an `error` Event with `code` was already appended for `attempt`.
pub fn has_error(conn: &Connection, attempt: &Attempt, code: ErrorCode) -> Result<bool, CpdError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM events WHERE kind = 'error' AND plan = ?1 AND attempt = ?2
                        AND json_extract(data, '$.code') = ?3)",
        params![attempt.plan.as_str(), attempt.id.to_string(), code.as_str()],
        |r| r.get(0),
    )?)
}

/// The first 12 characters of a SHA, for texts.
pub fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn every_cause_is_listed_and_reads_back() {
        use super::{Cause, Stored};
        assert_eq!(Cause::ALL.len(), 12);
        for cause in Cause::ALL {
            assert_eq!(Cause::parse(cause.as_str()), Some(*cause));
            assert_eq!(Stored::parse(cause.as_str()), Some(Stored::Failed(*cause)));
            assert_eq!(Stored::Failed(*cause).outcome(), "failed");
        }
        assert_eq!(Stored::parse("merged"), Some(Stored::Merged));
        assert_eq!(
            Stored::parse("cancel_requested").map(Stored::outcome),
            Some("cancelled")
        );
        assert_eq!(Stored::parse("review_rejected"), None);
        assert_eq!(Cause::parse("merged"), None);
    }

    /// engine-G1: the ladder, the end transaction, the cancel read, the load of an Attempt and the
    /// Event texts are defined once, here, for the driver, the engine and the merge. (The needles
    /// are built from two halves so that this file does not contain them.)
    #[test]
    fn the_ledger_is_defined_once() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        let texts: Vec<(PathBuf, String)> = files
            .into_iter()
            .map(|f| {
                (
                    f.strip_prefix(&src).unwrap().to_owned(),
                    std::fs::read_to_string(&f).unwrap(),
                )
            })
            .collect();
        let needles = [
            ["struct ", "Ladder"],
            ["enum ", "End "],
            ["fn ", "next_tier("],
            ["fn ", "cancel_request("],
            ["SELECT a.job, a.tier, ", "a.status, a.branch"],
            ["UPDATE jobs SET ", "status = ?3"],
            ["The Job is queued ", "again at Tier"],
        ];
        let mut twice = Vec::new();
        for needle in needles {
            let needle = needle.concat();
            let defined: Vec<_> = texts
                .iter()
                .filter(|(_, text)| text.contains(&needle))
                .map(|(p, _)| p.display().to_string())
                .collect();
            if defined.len() != 1 {
                twice.push(format!("{needle:?}: {defined:?}"));
            }
        }
        assert!(
            twice.is_empty(),
            "each belongs in attempt/ledger.rs only: {twice:#?}"
        );
    }
}
