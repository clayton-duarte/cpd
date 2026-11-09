//! One constructor per Event kind (contract §5.2): the fields of a kind's
//! `data` are the constructor's parameters, so a missing, misspelt or extra
//! field is a compile error, not a `store_failed` after the state change ran.
//!
//! Scope columns (`plan`, `job`, `attempt`, `session`, `context`) stay with the
//! caller (`.plan(..)`, `.job(..)`): they are checked against [`super::Kind::spec`]
//! by [`super::append`]. `kind.rs`'s table is the oracle of this file's tests.
//!
//! `impl Serialize` is a value that serialises as the field does (an id, a list):
//! it keeps the call sites as they were, and the JSON is the one `json!` makes of it.

use serde::Serialize;
use serde_json::{Value, json};

use super::NewEvent;
use super::kind::Kind;
use crate::error::{CpdError, ErrorCode};

/// The `number` and `url` of a `workflow-base-changed` Event (§5.2): the pull request's, for the poll's follow of GitHub's
/// repoint; explicit nulls for the poll's unstack of a Workflow with no pull request (hs-d301); absent for every other `by`.
#[derive(Clone, Copy, Debug)]
pub enum BasePull<'a> {
    Absent,
    Of(i64, &'a str),
    Null,
}

/// What a delete removed (`plan-deleted`, `context-deleted`, contract §5.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub jobs: u64,
    pub attempts: u64,
    pub sessions: u64,
    pub worktrees: u64,
    pub disk_freed_mb: u64,
}

/// An Event about `kind`: only this file calls it.
fn make(kind: Kind, text: impl Into<String>, data: Value) -> NewEvent {
    NewEvent::new(kind, text, data)
}

/// The optional `origin` of a `session-message` (contract §5.2, Plan ui-origin): the surface a message was submitted
/// from, exactly `tui` or `webui`, or `None` for absent. Any other value is refused with `usage`.
fn checked_origin(origin: Option<&str>) -> Result<Option<&str>, CpdError> {
    match origin {
        None => Ok(None),
        Some("tui" | "webui") => Ok(origin),
        Some(other) => Err(CpdError::new(
            ErrorCode::Usage,
            format!("An origin is tui or webui, not {other:?}."),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
impl NewEvent {
    pub fn context_created(
        text: impl Into<String>,
        title: &str,
        jira: Option<&str>,
        created_on: &str,
    ) -> NewEvent {
        make(
            Kind::ContextCreated,
            text,
            json!({"title": title, "jira": jira, "created_on": created_on}),
        )
    }

    pub fn context_record(
        text: impl Into<String>,
        record: impl Serialize,
        kind: &str,
        machine: impl Serialize,
        seq: i64,
    ) -> NewEvent {
        make(
            Kind::ContextRecord,
            text,
            json!({"record": record, "kind": kind, "machine": machine, "seq": seq}),
        )
    }

    pub fn context_grounded(
        text: impl Into<String>,
        seq: i64,
        forked_from: impl Serialize,
    ) -> NewEvent {
        make(
            Kind::ContextGrounded,
            text,
            json!({"seq": seq, "forked_from": forked_from}),
        )
    }

    pub fn plan_created(
        text: impl Into<String>,
        title: &str,
        jira: Option<&str>,
        lead_session: impl Serialize,
        forked_from: impl Serialize,
        cwd: &str,
    ) -> NewEvent {
        make(
            Kind::PlanCreated,
            text,
            json!({"title": title, "jira": jira, "lead_session": lead_session, "forked_from": forked_from, "cwd": cwd}),
        )
    }

    /// A phase change that names nothing else.
    pub fn plan_phase(text: impl Into<String>, from: &str, to: &str) -> NewEvent {
        make(Kind::PlanPhase, text, json!({"from": from, "to": to}))
    }

    /// `plan lock`: the Jobs it queued and the Workflows it started.
    pub fn plan_phase_locked(
        text: impl Into<String>,
        jobs: impl Serialize,
        workflows: impl Serialize,
    ) -> NewEvent {
        make(
            Kind::PlanPhase,
            text,
            json!({"from": "plan", "to": "dispatch", "jobs": jobs, "workflows": workflows}),
        )
    }

    /// The last Workflow opened: the Workflows that are open.
    pub fn plan_phase_deployed(text: impl Into<String>, workflows: impl Serialize) -> NewEvent {
        make(
            Kind::PlanPhase,
            text,
            json!({"from": "dispatch", "to": "deploy", "workflows": workflows}),
        )
    }

    /// A review that was just requested.
    pub fn plan_review_requested(
        text: impl Into<String>,
        review: impl Serialize,
        fingerprint: &str,
    ) -> NewEvent {
        make(
            Kind::PlanReview,
            text,
            json!({"review": review, "fingerprint": fingerprint, "status": "pending"}),
        )
    }

    /// A review cpd closed because nobody can or needs to decide it any more.
    pub fn plan_review_superseded(
        text: impl Into<String>,
        review: impl Serialize,
        fingerprint: &str,
    ) -> NewEvent {
        make(
            Kind::PlanReview,
            text,
            json!({"review": review, "fingerprint": fingerprint, "status": "superseded"}),
        )
    }

    /// A review that was decided; `jobs` are the draft Jobs it queued, when it queued any.
    pub fn plan_review_decided(
        text: impl Into<String>,
        review: impl Serialize,
        fingerprint: &str,
        status: &str,
        feedback: impl Serialize,
        jobs: Option<impl Serialize>,
    ) -> NewEvent {
        let mut data = json!({"review": review, "fingerprint": fingerprint, "status": status, "feedback": feedback});
        if let Some(jobs) = jobs {
            data["jobs"] = json!(jobs);
        }
        make(Kind::PlanReview, text, data)
    }

    /// `plan outline`: the Plan's outline was set (`replaced` false) or replaced.
    pub fn plan_outlined(text: impl Into<String>, replaced: bool) -> NewEvent {
        make(Kind::PlanOutlined, text, json!({"replaced": replaced}))
    }

    pub fn plan_done(text: impl Into<String>, workflows: impl Serialize) -> NewEvent {
        make(Kind::PlanDone, text, json!({"workflows": workflows}))
    }

    /// The `plan-done` of `review submit`: no Workflow, and `turn: false` (delivered as nothing to the Lead).
    pub fn plan_done_quiet(text: impl Into<String>) -> NewEvent {
        make(
            Kind::PlanDone,
            text,
            json!({"workflows": [], "turn": false}),
        )
    }

    pub fn job_added(
        text: impl Into<String>,
        workflow: impl Serialize,
        kind: &str,
        status: &str,
        appended: bool,
        tier: Option<&str>,
        needs: impl Serialize,
        urgent: bool,
    ) -> NewEvent {
        make(
            Kind::JobAdded,
            text,
            json!({"workflow": workflow, "kind": kind, "status": status, "appended": appended, "tier": tier, "needs": needs, "urgent": urgent}),
        )
    }

    /// The `job-added` of a Job cpd adds itself (§5.2, §3.7 E4): an `open` Job (`by` `engine`). It is `queued` (`draft` while a draft of its Workflow remains) and `appended`, and its
    /// `needs` is the empty list: its Ready rule is derived (§3.2). `reason` is for an `update` Job (`conflict`).
    pub fn job_added_by_cpd(
        text: impl Into<String>,
        workflow: impl Serialize,
        kind: &str,
        status: &str,
        tier: &str,
        by: &str,
        reason: Option<&str>,
    ) -> NewEvent {
        let mut data = json!({
            "workflow": workflow, "kind": kind, "status": status, "appended": true, "tier": tier, "needs": [], "urgent": false, "by": by,
        });
        if let Some(reason) = reason {
            data["reason"] = json!(reason);
        }
        make(Kind::JobAdded, text, data)
    }

    pub fn job_edited(text: impl Into<String>, fields: impl Serialize) -> NewEvent {
        make(Kind::JobEdited, text, json!({"fields": fields}))
    }

    /// `turn` is whether the Lead should take a turn (hs-d514). `cancel_too_late` is in `data` only when true. `pull` is the pull request an `open` Job's merge opened, `(pr, number, url)`: its
    /// `pr`, `number` and `url` are in `data` only then (§5.2).
    pub fn job_merged(
        text: impl Into<String>,
        sha: &str,
        tier: &str,
        kind: &str,
        turn: bool,
        cancel_too_late: bool,
        pull: Option<(&str, i64, &str)>,
    ) -> NewEvent {
        let mut data = json!({"sha": sha, "tier": tier, "kind": kind, "turn": turn});
        if cancel_too_late {
            data["cancel_too_late"] = json!(true);
        }
        if let Some((workflow, number, url)) = pull {
            data["workflow"] = json!(workflow);
            data["number"] = json!(number);
            data["url"] = json!(url);
        }
        make(Kind::JobMerged, text, data)
    }

    /// The `job-merged` of an explore Job (§5.2): `sha` is the artefact's SHA-256, `artefact` its path relative to `$CPD_HOME`; `turn` is always true.
    pub fn job_merged_explore(
        text: impl Into<String>,
        sha: &str,
        tier: &str,
        kind: &str,
        summary: &str,
        artefact: &str,
    ) -> NewEvent {
        make(
            Kind::JobMerged,
            text,
            json!({"sha": sha, "tier": tier, "kind": kind, "turn": true, "summary": summary, "artefact": artefact}),
        )
    }

    pub fn job_failed(
        text: impl Into<String>,
        cause: &str,
        detail: &str,
        tier: &str,
        attempts: i64,
    ) -> NewEvent {
        Self::job_failed_triaged(text, cause, detail, tier, attempts, None)
    }

    /// `job_failed` with the Gate failure triage's verdict as `data.triage` when there is one.
    pub fn job_failed_triaged(
        text: impl Into<String>,
        cause: &str,
        detail: &str,
        tier: &str,
        attempts: i64,
        triage: Option<Value>,
    ) -> NewEvent {
        let mut data =
            json!({"cause": cause, "detail": detail, "tier": tier, "attempts": attempts});
        if let Some(t) = triage {
            data["triage"] = t;
        }
        make(Kind::JobFailed, text, data)
    }

    pub fn job_retried(text: impl Into<String>, tier: &str) -> NewEvent {
        make(Kind::JobRetried, text, json!({"tier": tier}))
    }

    pub fn job_cancel_requested(text: impl Into<String>, reason: &str) -> NewEvent {
        make(Kind::JobCancelRequested, text, json!({"reason": reason}))
    }

    pub fn job_cancelled(text: impl Into<String>, reason: &str) -> NewEvent {
        make(Kind::JobCancelled, text, json!({"reason": reason}))
    }

    pub fn job_addendum(text: impl Into<String>, addendum: &str) -> NewEvent {
        make(Kind::JobAddendum, text, json!({"text": addendum}))
    }

    /// `job-blocked` (§5.2, hs-d798): E3 skipped a Ready Job for want of a pool slot, one Event per
    /// waiting episode; `waiting` is the number of Ready Jobs skipped that tick.
    pub fn job_blocked(text: impl Into<String>, waiting: u64) -> NewEvent {
        make(Kind::JobBlocked, text, json!({"waiting": waiting}))
    }

    pub fn attempt_started(
        text: impl Into<String>,
        number: impl Serialize,
        tier: &str,
        origin: &str,
    ) -> NewEvent {
        make(
            Kind::AttemptStarted,
            text,
            json!({"number": number, "tier": tier, "origin": origin}),
        )
    }

    pub fn attempt_status(text: impl Into<String>, from: &str, to: &str) -> NewEvent {
        make(Kind::AttemptStatus, text, json!({"from": from, "to": to}))
    }

    /// The start of a check span of `attempt submit` (contract §7.7): `from` = `to` = `running`, `what` `checks`.
    pub fn attempt_check_start(text: impl Into<String>, what: &str) -> NewEvent {
        make(
            Kind::AttemptStatus,
            text,
            json!({"from": "running", "to": "running", "check": "start", "what": what}),
        )
    }

    /// The end of a check span; `died` (E2's `end` for a submit that died, §3.7) is present only when true.
    pub fn attempt_check_end(text: impl Into<String>, what: &str, died: bool) -> NewEvent {
        let mut data = json!({"from": "running", "to": "running", "check": "end", "what": what});
        if died {
            data["died"] = json!(true);
        }
        make(Kind::AttemptStatus, text, data)
    }

    pub fn attempt_retry(
        text: impl Into<String>,
        outcome: &str,
        cause: &str,
        next_tier: &str,
        feedback: &str,
    ) -> NewEvent {
        Self::attempt_retry_triaged(text, outcome, cause, next_tier, feedback, None)
    }

    /// `attempt_retry` with the Gate failure triage's verdict as `data.triage` when there is one.
    pub fn attempt_retry_triaged(
        text: impl Into<String>,
        outcome: &str,
        cause: &str,
        next_tier: &str,
        feedback: &str,
        triage: Option<Value>,
    ) -> NewEvent {
        let mut data = json!({"outcome": outcome, "cause": cause, "next_tier": next_tier, "feedback": feedback});
        if let Some(t) = triage {
            data["triage"] = t;
        }
        make(Kind::AttemptRetry, text, data)
    }

    /// `attempt abort` (contract §5.2, commands.md §6.5): the Lead ended a running Attempt `failed` / `aborted`.
    /// `by` is the calling Session's id.
    pub fn attempt_aborted(text: impl Into<String>, tier: &str, by: &str) -> NewEvent {
        make(Kind::AttemptAborted, text, json!({"tier": tier, "by": by}))
    }

    pub fn attempt_nudge(text: impl Into<String>, prompt: &str, turns: u64) -> NewEvent {
        make(
            Kind::AttemptNudge,
            text,
            json!({"prompt": prompt, "turns": turns}),
        )
    }

    /// `attempt-ended` (§5.2, hs-d798): the end of a `pi` or `durable` Attempt, appended in the same
    /// transaction that writes `attempts.metrics`. `data` is the metrics blob: the ten keys copied
    /// from it; when there are no metrics, all ten keys are present with null.
    pub fn attempt_ended(text: impl Into<String>, metrics: Option<&Value>) -> NewEvent {
        const KEYS: [&str; 10] = [
            "turns",
            "tokens_in",
            "tokens_out",
            "cache_read",
            "cache_write",
            "cost_usd",
            "duration_s",
            "model",
            "stop_reason",
            "thinking_level",
        ];
        // Always all ten keys, null for the missing ones: the blob may be absent or partial.
        let mut data = json!({
            "turns": null, "tokens_in": null, "tokens_out": null, "cache_read": null, "cache_write": null,
            "cost_usd": null, "duration_s": null, "model": null, "stop_reason": null, "thinking_level": null,
        });
        if let Some(blob) = metrics
            && let Some(obj) = blob.as_object()
        {
            for key in KEYS {
                if let Some(value) = obj.get(key) {
                    data[key] = value.clone();
                }
            }
        }
        make(Kind::AttemptEnded, text, data)
    }

    pub fn all_merged(
        text: impl Into<String>,
        workflow: impl Serialize,
        merged: impl Serialize,
        cancelled: impl Serialize,
    ) -> NewEvent {
        make(
            Kind::AllMerged,
            text,
            json!({"workflow": workflow, "merged": merged, "cancelled": cancelled}),
        )
    }

    pub fn pr_feedback(
        text: impl Into<String>,
        workflow: &str,
        number: i64,
        url: &str,
        state: &str,
        checks: &str,
        review_decision: &str,
        mergeable: &str,
        merge_state: &str,
        conflict: bool,
        reasons: &[&str],
    ) -> NewEvent {
        make(
            Kind::PrFeedback,
            text,
            json!({
                "workflow": workflow, "number": number, "url": url, "state": state, "checks": checks,
                "review_decision": review_decision, "mergeable": mergeable, "merge_state": merge_state,
                "conflict": conflict, "reasons": reasons,
            }),
        )
    }

    pub fn workflow_added(
        text: impl Into<String>,
        workflow: impl Serialize,
        repo: &str,
        base: &str,
        title: &str,
        head_ref: Option<&str>,
        author: Option<&str>,
        needs: impl Serialize,
        appended: bool,
    ) -> NewEvent {
        make(
            Kind::WorkflowAdded,
            text,
            json!({"workflow": workflow, "repo": repo, "base": base, "title": title, "head_ref": head_ref, "author": author, "needs": needs, "appended": appended}),
        )
    }

    pub fn workflow_edited(
        text: impl Into<String>,
        workflow: impl Serialize,
        fields: impl Serialize,
    ) -> NewEvent {
        make(
            Kind::WorkflowEdited,
            text,
            json!({"workflow": workflow, "fields": fields}),
        )
    }

    pub fn workflow_described(text: impl Into<String>, workflow: impl Serialize) -> NewEvent {
        make(Kind::WorkflowDescribed, text, json!({"workflow": workflow}))
    }

    pub fn workflow_reopened(text: impl Into<String>, workflow: impl Serialize) -> NewEvent {
        make(Kind::WorkflowReopened, text, json!({"workflow": workflow}))
    }

    pub fn workflow_cancelled(text: impl Into<String>, workflow: impl Serialize) -> NewEvent {
        make(Kind::WorkflowCancelled, text, json!({"workflow": workflow}))
    }

    /// `workflow-cancelled` of a `dispatch` Workflow whose work another Workflow carried (§3.8, hs-d301): `superseded_by` is that Workflow as a reference.
    pub fn workflow_cancelled_superseded(
        text: impl Into<String>,
        workflow: impl Serialize,
        superseded_by: &str,
    ) -> NewEvent {
        make(
            Kind::WorkflowCancelled,
            text,
            json!({"workflow": workflow, "superseded_by": superseded_by}),
        )
    }

    pub fn workflow_started(
        text: impl Into<String>,
        workflow: impl Serialize,
        repo: &str,
        base: &str,
        lead_branch: &str,
        lead_worktree: &str,
        base_sha: &str,
        reused: bool,
        pushed: bool,
        by: &str,
    ) -> NewEvent {
        make(
            Kind::WorkflowStarted,
            text,
            json!({
                "workflow": workflow, "repo": repo, "base": base, "lead_branch": lead_branch, "lead_worktree": lead_worktree,
                "base_sha": base_sha, "reused": reused, "pushed": pushed, "by": by,
            }),
        )
    }

    /// `workflow-base-changed` (§5.2): `number` and `url` are in `data` for `by` `poll` only, see [`BasePull`].
    pub fn workflow_base_changed(
        text: impl Into<String>,
        workflow: impl Serialize,
        (from, to): (&str, &str),
        (from_workflow, to_workflow): (Option<&str>, Option<&str>),
        by: &str,
        pull_request: BasePull<'_>,
    ) -> NewEvent {
        let mut data = json!({"workflow": workflow, "from": from, "to": to, "from_workflow": from_workflow, "to_workflow": to_workflow, "by": by});
        match pull_request {
            BasePull::Absent => {}
            BasePull::Of(number, url) => {
                data["number"] = json!(number);
                data["url"] = json!(url);
            }
            BasePull::Null => {
                data["number"] = Value::Null;
                data["url"] = Value::Null;
            }
        }
        make(Kind::WorkflowBaseChanged, text, data)
    }

    pub fn pr_opened(
        text: impl Into<String>,
        workflow: impl Serialize,
        number: i64,
        url: &str,
    ) -> NewEvent {
        make(
            Kind::PrOpened,
            text,
            json!({"workflow": workflow, "number": number, "url": url}),
        )
    }

    pub fn pr_merged(
        text: impl Into<String>,
        workflow: impl Serialize,
        number: i64,
        url: &str,
        merged_at: Option<&str>,
    ) -> NewEvent {
        make(
            Kind::PrMerged,
            text,
            json!({"workflow": workflow, "number": number, "url": url, "merged_at": merged_at}),
        )
    }

    pub fn escalation(
        text: impl Into<String>,
        escalation: impl Serialize,
        question: &str,
    ) -> NewEvent {
        make(
            Kind::Escalation,
            text,
            json!({"escalation": escalation, "question": question}),
        )
    }

    pub fn escalation_answered(text: impl Into<String>, escalation: impl Serialize) -> NewEvent {
        make(
            Kind::EscalationAnswered,
            text,
            json!({"escalation": escalation}),
        )
    }

    pub fn escalation_expired(
        text: impl Into<String>,
        escalation: impl Serialize,
        reason: &str,
    ) -> NewEvent {
        make(
            Kind::EscalationExpired,
            text,
            json!({"escalation": escalation, "reason": reason}),
        )
    }

    pub fn session_registered(
        text: impl Into<String>,
        role: &str,
        name: &str,
        forked_from: impl Serialize,
        tier: &str,
    ) -> NewEvent {
        make(
            Kind::SessionRegistered,
            text,
            json!({"role": role, "name": name, "forked_from": forked_from, "tier": tier}),
        )
    }

    /// `reply` is in `data` only when true (`session state idle --reply`).
    pub fn session_state(
        text: impl Into<String>,
        from: &str,
        to: &str,
        label: Option<&str>,
        reply: bool,
    ) -> NewEvent {
        let mut data = json!({"from": from, "to": to, "label": label});
        if reply {
            data["reply"] = json!(true);
        }
        make(Kind::SessionState, text, data)
    }

    /// `pane` and `window` are null when the Session has no pane (`place` is then `none`).
    pub fn session_moved(
        text: impl Into<String>,
        pane: Option<&str>,
        window: Option<&str>,
        place: &str,
    ) -> NewEvent {
        make(
            Kind::SessionMoved,
            text,
            json!({"pane": pane, "window": window, "place": place}),
        )
    }

    pub fn session_renamed(text: impl Into<String>, name: &str) -> NewEvent {
        make(Kind::SessionRenamed, text, json!({"name": name}))
    }

    pub fn session_titled(text: impl Into<String>, title: &str) -> NewEvent {
        make(Kind::SessionTitled, text, json!({"title": title}))
    }

    pub fn session_retiered(text: impl Into<String>, from: &str, to: &str) -> NewEvent {
        make(Kind::SessionRetiered, text, json!({"from": from, "to": to}))
    }

    /// `session-model-changed` (contract §5.2, commands.md §14.6): `cpd preset apply` rewrote the machine host
    /// `[model]`; the Session's letter model moved from `from` to `to`. The adapter switches pi at its next idle.
    pub fn session_model_changed(text: impl Into<String>, from: &str, to: &str) -> NewEvent {
        make(
            Kind::SessionModelChanged,
            text,
            json!({"from": from, "to": to}),
        )
    }

    pub fn mcp_attached(text: impl Into<String>, name: &str, server: impl Serialize) -> NewEvent {
        make(
            Kind::McpAttached,
            text,
            json!({"name": name, "server": server}),
        )
    }

    pub fn session_asleep(text: impl Into<String>) -> NewEvent {
        make(Kind::SessionAsleep, text, json!({}))
    }

    pub fn session_resumed(text: impl Into<String>) -> NewEvent {
        make(Kind::SessionResumed, text, json!({}))
    }

    pub fn plan_deleted(text: impl Into<String>, counts: &Counts) -> NewEvent {
        make(
            Kind::PlanDeleted,
            text,
            json!({"jobs": counts.jobs, "attempts": counts.attempts, "sessions": counts.sessions, "worktrees": counts.worktrees, "disk_freed_mb": counts.disk_freed_mb}),
        )
    }

    pub fn context_deleted(text: impl Into<String>, plans: u64, counts: &Counts) -> NewEvent {
        make(
            Kind::ContextDeleted,
            text,
            json!({"plans": plans, "jobs": counts.jobs, "attempts": counts.attempts, "sessions": counts.sessions, "worktrees": counts.worktrees, "disk_freed_mb": counts.disk_freed_mb}),
        )
    }

    pub fn records_retired(
        text: impl Into<String>,
        context: &str,
        approved: impl Serialize,
        rejected: impl Serialize,
    ) -> NewEvent {
        make(
            Kind::RecordsRetired,
            text,
            json!({"context": context, "approved": approved, "rejected": rejected}),
        )
    }

    pub fn context_curated(
        text: impl Into<String>,
        context: &str,
        curated: u64,
        rejected: u64,
        retire: u64,
    ) -> NewEvent {
        let mut data = json!({"context": context, "curated": curated, "retire": retire});
        // `rejected` is there only when a line was rejected (contract §5.2).
        if rejected > 0 {
            data["rejected"] = json!(rejected);
        }
        make(Kind::ContextCurated, text, data)
    }

    pub fn context_owner(text: impl Into<String>, owner: &str) -> NewEvent {
        make(Kind::ContextOwner, text, json!({"owner": owner}))
    }

    pub fn context_renamed(text: impl Into<String>, title: &str) -> NewEvent {
        make(Kind::ContextRenamed, text, json!({"title": title}))
    }

    pub fn plan_renamed(text: impl Into<String>, title: &str) -> NewEvent {
        make(Kind::PlanRenamed, text, json!({"title": title}))
    }

    pub fn context_archived(text: impl Into<String>, archived: bool) -> NewEvent {
        make(Kind::ContextArchived, text, json!({"archived": archived}))
    }

    pub fn plan_archived(text: impl Into<String>, archived: bool) -> NewEvent {
        make(Kind::PlanArchived, text, json!({"archived": archived}))
    }

    pub fn session_archived(text: impl Into<String>, archived: bool) -> NewEvent {
        make(Kind::SessionArchived, text, json!({"archived": archived}))
    }

    pub fn session_message(
        text: impl Into<String>,
        from: impl Serialize,
        message: &str,
        wake: bool,
        origin: Option<&str>,
    ) -> Result<NewEvent, CpdError> {
        let mut data = json!({"from": from, "text": message, "wake": wake});
        if let Some(origin) = checked_origin(origin)? {
            data["origin"] = json!(origin);
        }
        Ok(make(Kind::SessionMessage, text, data))
    }

    /// The engine's Review Lead wake: a waking message from the User's side (`from` null) that names the woken `head_oid`,
    /// stored as given. An empty or whitespace-only `head_oid` is refused with `usage`.
    pub fn session_message_head(
        text: impl Into<String>,
        message: &str,
        head_oid: &str,
    ) -> Result<NewEvent, CpdError> {
        if head_oid.trim().is_empty() {
            return Err(CpdError::new(
                ErrorCode::Usage,
                "A session-message head_oid is never empty.",
            ));
        }
        Ok(make(
            Kind::SessionMessage,
            text,
            json!({"from": null, "text": message, "wake": true, "head_oid": head_oid}),
        ))
    }

    /// A `session-message` of `cpd session send`: it always wakes, so it carries `unblocks` and no `wake` (hs-d514).
    pub fn session_message_unblocking(
        text: impl Into<String>,
        from: impl Serialize,
        message: &str,
        unblocks: &str,
        origin: Option<&str>,
    ) -> Result<NewEvent, CpdError> {
        let mut data = json!({"from": from, "text": message, "unblocks": unblocks});
        if let Some(origin) = checked_origin(origin)? {
            data["origin"] = json!(origin);
        }
        Ok(make(Kind::SessionMessage, text, data))
    }

    pub fn inbox_opened(
        text: impl Into<String>,
        item: i64,
        kind: &str,
        urgency: &str,
        label: &str,
    ) -> NewEvent {
        make(
            Kind::InboxOpened,
            text,
            json!({"item": item, "kind": kind, "urgency": urgency, "label": label}),
        )
    }

    pub fn inbox_closed(text: impl Into<String>, item: i64, kind: &str) -> NewEvent {
        make(Kind::InboxClosed, text, json!({"item": item, "kind": kind}))
    }

    pub fn inbox_snoozed(text: impl Into<String>, item: i64, until: &str) -> NewEvent {
        make(
            Kind::InboxSnoozed,
            text,
            json!({"item": item, "until": until}),
        )
    }

    pub fn inbox_dismissed(text: impl Into<String>, item: i64) -> NewEvent {
        make(Kind::InboxDismissed, text, json!({"item": item}))
    }

    pub fn inbox_claimed(text: impl Into<String>, item: i64) -> NewEvent {
        make(Kind::InboxClaimed, text, json!({"item": item}))
    }

    pub fn watch_started(text: impl Into<String>, pid: u32) -> NewEvent {
        make(Kind::WatchStarted, text, json!({"pid": pid}))
    }

    pub fn watch_stopped(text: impl Into<String>, pid: u32) -> NewEvent {
        make(Kind::WatchStopped, text, json!({"pid": pid}))
    }

    pub fn error(
        text: impl Into<String>,
        source: &str,
        code: &str,
        message: &str,
        details: impl Serialize,
    ) -> NewEvent {
        make(
            Kind::Error,
            text,
            json!({"source": source, "code": code, "message": message, "details": details}),
        )
    }

    /// `guard` (contract §5.2, Plan `agent-refusals`): one of the Session's own tool calls or commands was
    /// refused. `code` is the guard rule (an adapter refusal) or the envelope's code (a CLI refusal), and
    /// `excerpt` the refused command line or path, cut to 200 characters. `role`, `tier` and `model` are the
    /// refusing Session's own, so a refusal is rankable per guard, role, model and day with no join.
    pub fn guard(
        text: impl Into<String>,
        code: &str,
        role: &str,
        tier: &str,
        model: &str,
        tool: &str,
        excerpt: &str,
    ) -> NewEvent {
        make(
            Kind::Guard,
            text,
            json!({
                "code": code,
                "role": role,
                "tier": tier,
                "model": model,
                "tool": tool,
                "excerpt": excerpt,
            }),
        )
    }

    /// `loop-guard-trip` (contract §5.2, §6.16 "The trip's cause and its report"): a loop-guard breaker tripped.
    /// `kind` is `inter-turn`, `read-streak`, `turn-tools` or `turn-refusals`; `count` is the figure the trip text
    /// names. Any other `kind` is refused with `usage`.
    pub fn loop_guard_trip(
        text: impl Into<String>,
        kind: &str,
        count: u64,
    ) -> Result<NewEvent, CpdError> {
        if !matches!(
            kind,
            "inter-turn" | "read-streak" | "turn-tools" | "turn-refusals"
        ) {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!(
                    "A loop-guard trip kind is inter-turn, read-streak, turn-tools or turn-refusals, not {kind:?}."
                ),
            ));
        }
        Ok(make(
            Kind::LoopGuardTrip,
            text,
            json!({"kind": kind, "count": count}),
        ))
    }

    /// `incident-started` (contract §5.2, commands.md §25.1): `plans` is the array of Plan ids in
    /// the incident; `until_workflow` (the `<plan>/<key>` auto-end Workflow) is in `data` only when set.
    pub fn incident_started(
        text: impl Into<String>,
        by: &str,
        reason: &str,
        started_at: &str,
        plans: impl Serialize,
        until_workflow: Option<&str>,
    ) -> NewEvent {
        let mut data =
            json!({"by": by, "reason": reason, "started_at": started_at, "plans": plans});
        if let Some(until_workflow) = until_workflow {
            data["until_workflow"] = json!(until_workflow);
        }
        make(Kind::IncidentStarted, text, data)
    }

    /// `incident-plans` (contract §5.2, hs-d664): a `cpd incident start --plan` on a live incident
    /// that changed the Plan set or `until_workflow`. `plans` is the new set; `until_workflow` (the
    /// `<plan>/<key>` auto-end Workflow) is in `data` only when set, mirroring `incident-started`.
    pub fn incident_plans(
        text: impl Into<String>,
        by: &str,
        plans: impl Serialize,
        until_workflow: Option<&str>,
    ) -> NewEvent {
        let mut data = json!({"by": by, "plans": plans});
        if let Some(until_workflow) = until_workflow {
            data["until_workflow"] = json!(until_workflow);
        }
        make(Kind::IncidentPlans, text, data)
    }

    /// `incident-ended` (contract §5.2, commands.md §25.2): `cause` is `"manual"` or `"until-workflow"`.
    pub fn incident_ended(
        text: impl Into<String>,
        by: &str,
        ended_at: &str,
        cause: &str,
    ) -> NewEvent {
        make(
            Kind::IncidentEnded,
            text,
            json!({"by": by, "ended_at": ended_at, "cause": cause}),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::*;

    /// Events built by the constructors of `kind`, the optional fields present and absent.
    /// No wildcard: a new kind does not compile until it has a constructor and a sample.
    fn samples(kind: Kind) -> Vec<NewEvent> {
        const T: &str = "text";
        let (s, n, b) = ("s", 1, true);
        match kind {
            Kind::ContextCreated => vec![NewEvent::context_created(T, s, None, s)],
            Kind::ContextRecord => vec![NewEvent::context_record(T, s, s, s, n)],
            Kind::ContextGrounded => vec![NewEvent::context_grounded(T, n, s)],
            Kind::PlanCreated => vec![NewEvent::plan_created(T, s, None, s, s, s)],
            Kind::PlanPhase => vec![
                NewEvent::plan_phase(T, s, s),
                NewEvent::plan_phase_locked(T, [s], [s]),
                NewEvent::plan_phase_deployed(T, [s]),
            ],
            Kind::PlanReview => vec![
                NewEvent::plan_review_requested(T, s, s),
                NewEvent::plan_review_superseded(T, s, s),
                NewEvent::plan_review_decided(T, s, s, s, s, None::<Vec<&str>>),
                NewEvent::plan_review_decided(T, s, s, s, s, Some([s])),
            ],
            Kind::PlanOutlined => vec![NewEvent::plan_outlined(T, b)],
            Kind::PlanDone => vec![NewEvent::plan_done(T, [s]), NewEvent::plan_done_quiet(T)],
            Kind::JobAdded => vec![
                NewEvent::job_added(T, s, s, s, b, Some(s), [s], b),
                NewEvent::job_added(T, s, s, s, b, None, [s], b),
                NewEvent::job_added_by_cpd(T, s, s, s, s, s, None),
                NewEvent::job_added_by_cpd(T, s, s, s, s, s, Some(s)),
            ],
            Kind::JobEdited => vec![NewEvent::job_edited(T, [s])],
            Kind::JobMerged => vec![
                NewEvent::job_merged(T, s, s, s, false, false, None),
                NewEvent::job_merged(T, s, s, s, true, true, Some((s, n, s))),
                NewEvent::job_merged_explore(T, s, s, s, s, s),
            ],
            Kind::JobFailed => vec![
                NewEvent::job_failed(T, s, s, s, n),
                NewEvent::job_failed_triaged(T, s, s, s, n, Some(json!({}))),
            ],
            Kind::JobRetried => vec![NewEvent::job_retried(T, s)],
            Kind::JobCancelRequested => vec![NewEvent::job_cancel_requested(T, s)],
            Kind::JobCancelled => vec![NewEvent::job_cancelled(T, s)],
            Kind::JobAddendum => vec![NewEvent::job_addendum(T, s)],
            Kind::JobBlocked => vec![NewEvent::job_blocked(T, 1)],
            Kind::AttemptStarted => vec![NewEvent::attempt_started(T, n, s, s)],
            Kind::AttemptStatus => vec![
                NewEvent::attempt_status(T, s, s),
                NewEvent::attempt_check_start(T, s),
                NewEvent::attempt_check_end(T, s, false),
                NewEvent::attempt_check_end(T, s, true),
            ],
            Kind::AttemptRetry => vec![
                NewEvent::attempt_retry(T, s, s, s, s),
                NewEvent::attempt_retry_triaged(T, s, s, s, s, Some(json!({}))),
            ],
            Kind::AttemptAborted => vec![NewEvent::attempt_aborted(T, s, s)],
            Kind::AttemptNudge => vec![NewEvent::attempt_nudge(T, s, 1)],
            Kind::AttemptEnded => vec![
                NewEvent::attempt_ended(
                    T,
                    Some(
                        &json!({"turns": 1, "tokens_in": 2, "tokens_out": 3, "cache_read": 4, "cache_write": 5, "cost_usd": 0.0, "duration_s": 6.0, "model": "m", "stop_reason": "stop", "thinking_level": "high"}),
                    ),
                ),
                NewEvent::attempt_ended(T, None),
                // a partial blob still yields all ten keys, the rest null
                NewEvent::attempt_ended(T, Some(&json!({"duration_s": 6.0, "cost_usd": 0.5}))),
            ],
            Kind::AllMerged => vec![NewEvent::all_merged(T, s, [s], [s])],
            Kind::PrFeedback => vec![NewEvent::pr_feedback(T, s, n, s, s, s, s, s, s, b, &[s])],
            Kind::WorkflowAdded => {
                vec![NewEvent::workflow_added(T, s, s, s, s, None, None, [s], b)]
            }
            Kind::WorkflowEdited => vec![NewEvent::workflow_edited(T, s, [s])],
            Kind::WorkflowDescribed => vec![NewEvent::workflow_described(T, s)],
            Kind::WorkflowCancelled => vec![
                NewEvent::workflow_cancelled(T, s),
                NewEvent::workflow_cancelled_superseded(T, s, s),
            ],
            Kind::WorkflowReopened => vec![NewEvent::workflow_reopened(T, s)],
            Kind::WorkflowStarted => vec![NewEvent::workflow_started(T, s, s, s, s, s, s, b, b, s)],
            Kind::WorkflowBaseChanged => vec![
                NewEvent::workflow_base_changed(T, s, (s, s), (None, Some(s)), s, BasePull::Absent),
                NewEvent::workflow_base_changed(
                    T,
                    s,
                    (s, s),
                    (Some(s), None),
                    s,
                    BasePull::Of(1, s),
                ),
                NewEvent::workflow_base_changed(T, s, (s, s), (Some(s), None), s, BasePull::Null),
            ],
            Kind::PrOpened => vec![NewEvent::pr_opened(T, s, n, s)],
            Kind::PrMerged => vec![NewEvent::pr_merged(T, s, n, s, None)],
            Kind::Escalation => vec![NewEvent::escalation(T, s, s)],
            Kind::EscalationAnswered => vec![NewEvent::escalation_answered(T, s)],
            Kind::EscalationExpired => vec![NewEvent::escalation_expired(T, s, s)],
            Kind::SessionRegistered => vec![NewEvent::session_registered(T, s, s, s, s)],
            Kind::SessionState => vec![
                NewEvent::session_state(T, s, s, None, false),
                NewEvent::session_state(T, s, s, None, true),
            ],
            Kind::SessionMoved => vec![
                NewEvent::session_moved(T, Some(s), Some(s), s),
                NewEvent::session_moved(T, None, None, s),
            ],
            Kind::SessionRenamed => vec![NewEvent::session_renamed(T, s)],
            Kind::SessionTitled => vec![NewEvent::session_titled(T, s)],
            Kind::SessionRetiered => vec![NewEvent::session_retiered(T, s, s)],
            Kind::SessionModelChanged => vec![NewEvent::session_model_changed(T, s, s)],
            Kind::McpAttached => vec![NewEvent::mcp_attached(T, s, json!({}))],
            Kind::SessionAsleep => vec![NewEvent::session_asleep(T)],
            Kind::SessionResumed => vec![NewEvent::session_resumed(T)],
            Kind::PlanDeleted => vec![NewEvent::plan_deleted(T, &Counts::default())],
            Kind::ContextDeleted => vec![NewEvent::context_deleted(T, 0, &Counts::default())],
            Kind::RecordsRetired => {
                vec![NewEvent::records_retired(T, s, [s], Vec::<String>::new())]
            }
            Kind::ContextCurated => vec![
                NewEvent::context_curated(T, s, 0, 0, 0),
                NewEvent::context_curated(T, s, 1, 1, 0),
            ],
            Kind::ContextRenamed => vec![NewEvent::context_renamed(T, s)],
            Kind::ContextOwner => vec![NewEvent::context_owner(T, s)],
            Kind::PlanRenamed => vec![NewEvent::plan_renamed(T, s)],
            Kind::ContextArchived => vec![NewEvent::context_archived(T, b)],
            Kind::PlanArchived => vec![NewEvent::plan_archived(T, b)],
            Kind::SessionArchived => vec![NewEvent::session_archived(T, b)],
            Kind::SessionMessage => vec![
                NewEvent::session_message(T, s, s, b, None).unwrap(),
                NewEvent::session_message_unblocking(T, s, s, s, None).unwrap(),
                NewEvent::session_message(T, s, s, b, Some("webui")).unwrap(),
                NewEvent::session_message_unblocking(T, s, s, s, Some("tui")).unwrap(),
                NewEvent::session_message_head(T, s, "h2").unwrap(),
            ],
            Kind::InboxOpened => vec![NewEvent::inbox_opened(T, 1, s, s, s)],
            Kind::InboxClosed => vec![NewEvent::inbox_closed(T, 1, s)],
            Kind::InboxSnoozed => vec![NewEvent::inbox_snoozed(T, 1, s)],
            Kind::InboxDismissed => vec![NewEvent::inbox_dismissed(T, 1)],
            Kind::InboxClaimed => vec![NewEvent::inbox_claimed(T, 1)],
            Kind::WatchStarted => vec![NewEvent::watch_started(T, 1)],
            Kind::WatchStopped => vec![NewEvent::watch_stopped(T, 1)],
            Kind::Error => vec![NewEvent::error(T, s, s, s, serde_json::Value::Null)],
            // retired (hs-d738): past Events hold these kinds, nothing writes them, so no constructor (see `RETIRED`)
            Kind::ReviewOpened | Kind::ReviewClosed | Kind::DocsDecided => vec![],
            Kind::IncidentStarted => vec![
                NewEvent::incident_started(T, s, s, s, [s], None),
                NewEvent::incident_started(T, s, s, s, [s], Some(s)),
            ],
            Kind::IncidentPlans => vec![
                NewEvent::incident_plans(T, s, [s], None),
                NewEvent::incident_plans(T, s, [s], Some(s)),
            ],
            Kind::IncidentEnded => vec![NewEvent::incident_ended(T, s, s, s)],
            Kind::Guard => vec![NewEvent::guard(T, s, s, s, s, s, s)],
            Kind::LoopGuardTrip => vec![NewEvent::loop_guard_trip(T, "inter-turn", 21).unwrap()],
        }
    }

    /// The retired kinds (contract §5.2): valid in `event_kinds`, written by nothing, so they have no constructor.
    /// `DocsDecided` is retired too (pro-d327): `cpd context docs` is gone.
    const RETIRED: [Kind; 3] = [Kind::ReviewOpened, Kind::ReviewClosed, Kind::DocsDecided];

    /// `kind.rs`'s table is the oracle: every constructor of a kind carries the kind's required
    /// `data` fields and no field the kind does not list.
    #[test]
    fn every_constructor_makes_the_data_its_kind_lists() {
        for &kind in Kind::ALL {
            let spec = kind.spec();
            let samples = samples(kind);
            assert!(
                samples.is_empty() == RETIRED.contains(&kind),
                "{kind}: a constructor exists exactly for a kind that is not retired"
            );
            for event in samples {
                assert_eq!(event.kind(), kind);
                let keys: BTreeSet<&str> = event
                    .data()
                    .as_object()
                    .expect("data is an object")
                    .keys()
                    .map(String::as_str)
                    .collect();
                let missing: Vec<_> = spec
                    .required
                    .iter()
                    .filter(|k| !keys.contains(**k))
                    .collect();
                let unknown: Vec<_> = keys
                    .iter()
                    .filter(|k| !spec.required.contains(k) && !spec.optional.contains(k))
                    .collect();
                assert!(
                    missing.is_empty() && unknown.is_empty(),
                    "{kind}: missing {missing:?}, unknown {unknown:?}"
                );
            }
        }
    }

    /// A partial (or absent) metrics blob still yields the ten keys, the missing ones null: the
    /// `attempt-ended` schema requires all ten (hs-d798).
    #[test]
    fn attempt_ended_fills_a_partial_metrics_blob_with_null() {
        let event =
            NewEvent::attempt_ended("text", Some(&json!({"duration_s": 6.0, "cost_usd": 0.5})));
        let data = event.data();
        assert_eq!(data["duration_s"], json!(6.0));
        assert_eq!(data["cost_usd"], json!(0.5));
        for key in [
            "turns",
            "tokens_in",
            "tokens_out",
            "cache_read",
            "cache_write",
            "model",
            "stop_reason",
            "thinking_level",
        ] {
            assert_eq!(data[key], Value::Null, "{key} is null for a partial blob");
        }
    }

    /// The optional fields can all be present: each is made by some constructor.
    #[test]
    fn every_optional_field_has_a_constructor() {
        for &kind in Kind::ALL {
            let made: BTreeSet<String> = samples(kind)
                .iter()
                .flat_map(|e| {
                    e.data()
                        .as_object()
                        .unwrap()
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .collect();
            for field in kind.spec().optional.iter() {
                assert!(
                    made.contains(*field),
                    "{kind}: no constructor makes the optional field {field:?}"
                );
            }
        }
    }

    fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// Every append goes through a typed constructor (store-G1): `NewEvent::new` is for the tests of `append`'s refusals.
    #[test]
    fn the_head_wake_stores_the_head_as_given_and_refuses_an_empty_one() {
        let event = NewEvent::session_message_head(T_TEXT, "m", " h2").unwrap();
        assert_eq!(
            event.data(),
            &json!({"from": null, "text": "m", "wake": true, "head_oid": " h2"})
        );
        for empty in ["", "  ", "\t"] {
            assert!(NewEvent::session_message_head(T_TEXT, "m", empty).is_err());
        }
    }

    const T_TEXT: &str = "text";

    #[test]
    fn session_message_accepts_head_oid_and_still_refuses_an_unknown_field() {
        let spec = Kind::SessionMessage.spec();
        assert!(spec.optional.contains(&"head_oid"));
        assert!(!spec.optional.contains(&"other"));
    }

    #[test]
    fn no_command_builds_an_untyped_event() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        let needle = ["NewEvent", "::new("].concat();
        let mut found = Vec::new();
        for file in files {
            if file.starts_with(src.join("events")) {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for (i, _) in text
                .lines()
                .enumerate()
                .filter(|(_, l)| l.contains(&needle))
            {
                found.push(format!(
                    "{}:{}",
                    file.strip_prefix(&src).unwrap().display(),
                    i + 1
                ));
            }
        }
        assert!(
            found.is_empty(),
            "build these Events with the constructor of their kind (events/typed.rs): {found:#?}"
        );
    }
}
