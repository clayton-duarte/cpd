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

/// The `number` and `url` of a `pr-base-changed` Event (§5.2): the pull request's, for the poll's follow of GitHub's
/// repoint; explicit nulls for the poll's unstack of a PR with no pull request (hs-d301); absent for every other `by`.
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

#[allow(clippy::too_many_arguments)]
impl NewEvent {
    pub fn context_created(text: impl Into<String>, title: &str, jira: Option<&str>, created_on: &str) -> NewEvent {
        make(Kind::ContextCreated, text, json!({"title": title, "jira": jira, "created_on": created_on}))
    }

    pub fn context_record(text: impl Into<String>, record: impl Serialize, kind: &str, machine: impl Serialize, seq: i64) -> NewEvent {
        make(Kind::ContextRecord, text, json!({"record": record, "kind": kind, "machine": machine, "seq": seq}))
    }

    pub fn context_grounded(text: impl Into<String>, seq: i64, forked_from: impl Serialize) -> NewEvent {
        make(Kind::ContextGrounded, text, json!({"seq": seq, "forked_from": forked_from}))
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

    /// `plan lock`: the Jobs it queued and the PRs it started.
    pub fn plan_phase_locked(text: impl Into<String>, jobs: impl Serialize, prs: impl Serialize) -> NewEvent {
        make(Kind::PlanPhase, text, json!({"from": "plan", "to": "dispatch", "jobs": jobs, "prs": prs}))
    }

    /// The last PR opened: the PRs that are open.
    pub fn plan_phase_deployed(text: impl Into<String>, prs: impl Serialize) -> NewEvent {
        make(Kind::PlanPhase, text, json!({"from": "dispatch", "to": "deploy", "prs": prs}))
    }

    /// A review that was just requested.
    pub fn plan_review_requested(text: impl Into<String>, review: impl Serialize, fingerprint: &str) -> NewEvent {
        make(Kind::PlanReview, text, json!({"review": review, "fingerprint": fingerprint, "status": "pending"}))
    }

    /// A review cpd closed because nobody can or needs to decide it any more.
    pub fn plan_review_superseded(text: impl Into<String>, review: impl Serialize, fingerprint: &str) -> NewEvent {
        make(Kind::PlanReview, text, json!({"review": review, "fingerprint": fingerprint, "status": "superseded"}))
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

    pub fn plan_done(text: impl Into<String>, prs: impl Serialize) -> NewEvent {
        make(Kind::PlanDone, text, json!({"prs": prs}))
    }

    pub fn job_added(
        text: impl Into<String>,
        pr: impl Serialize,
        kind: &str,
        status: &str,
        appended: bool,
        tier: Option<&str>,
        needs: impl Serialize,
        review: bool,
        urgent: bool,
    ) -> NewEvent {
        make(
            Kind::JobAdded,
            text,
            json!({"pr": pr, "kind": kind, "status": status, "appended": appended, "tier": tier, "needs": needs, "review": review, "urgent": urgent}),
        )
    }

    /// The `job-added` of a Job cpd adds itself (§5.2, §3.7 E4): an `open` Job (`by` `engine`). It is `queued` (`draft` while a draft of its PR remains) and `appended`, and its
    /// `needs` is the empty list: its Ready rule is derived (§3.2). `reason` is for an `update` Job (`conflict`).
    pub fn job_added_by_cpd(text: impl Into<String>, pr: impl Serialize, kind: &str, status: &str, tier: &str, by: &str, reason: Option<&str>) -> NewEvent {
        let mut data = json!({
            "pr": pr, "kind": kind, "status": status, "appended": true, "tier": tier, "needs": [], "review": false, "urgent": false, "by": by,
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
    pub fn job_merged(text: impl Into<String>, sha: &str, tier: &str, kind: &str, turn: bool, cancel_too_late: bool, pull: Option<(&str, i64, &str)>) -> NewEvent {
        let mut data = json!({"sha": sha, "tier": tier, "kind": kind, "turn": turn});
        if cancel_too_late {
            data["cancel_too_late"] = json!(true);
        }
        if let Some((pr, number, url)) = pull {
            data["pr"] = json!(pr);
            data["number"] = json!(number);
            data["url"] = json!(url);
        }
        make(Kind::JobMerged, text, data)
    }

    pub fn job_failed(text: impl Into<String>, cause: &str, detail: &str, tier: &str, attempts: i64) -> NewEvent {
        make(Kind::JobFailed, text, json!({"cause": cause, "detail": detail, "tier": tier, "attempts": attempts}))
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

    pub fn attempt_started(text: impl Into<String>, number: impl Serialize, tier: &str, origin: &str) -> NewEvent {
        make(Kind::AttemptStarted, text, json!({"number": number, "tier": tier, "origin": origin}))
    }

    pub fn attempt_status(text: impl Into<String>, from: &str, to: &str) -> NewEvent {
        make(Kind::AttemptStatus, text, json!({"from": from, "to": to}))
    }

    /// The start of a check span of `attempt submit` (contract §7.7): `from` = `to` = `running`, `what` `checks`.
    pub fn attempt_check_start(text: impl Into<String>, what: &str) -> NewEvent {
        make(Kind::AttemptStatus, text, json!({"from": "running", "to": "running", "check": "start", "what": what}))
    }

    /// The end of a check span; `died` (E2's `end` for a submit that died, §3.7) is present only when true.
    pub fn attempt_check_end(text: impl Into<String>, what: &str, died: bool) -> NewEvent {
        let mut data = json!({"from": "running", "to": "running", "check": "end", "what": what});
        if died {
            data["died"] = json!(true);
        }
        make(Kind::AttemptStatus, text, data)
    }

    pub fn attempt_retry(text: impl Into<String>, outcome: &str, cause: &str, next_tier: &str, feedback: &str) -> NewEvent {
        make(
            Kind::AttemptRetry,
            text,
            json!({"outcome": outcome, "cause": cause, "next_tier": next_tier, "feedback": feedback}),
        )
    }

    pub fn attempt_nudge(text: impl Into<String>, prompt: &str, turns: u64) -> NewEvent {
        make(Kind::AttemptNudge, text, json!({"prompt": prompt, "turns": turns}))
    }

    pub fn all_merged(text: impl Into<String>, pr: impl Serialize, merged: impl Serialize, cancelled: impl Serialize) -> NewEvent {
        make(Kind::AllMerged, text, json!({"pr": pr, "merged": merged, "cancelled": cancelled}))
    }

    pub fn pr_feedback(
        text: impl Into<String>,
        pr: &str,
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
                "pr": pr, "number": number, "url": url, "state": state, "checks": checks,
                "review_decision": review_decision, "mergeable": mergeable, "merge_state": merge_state,
                "conflict": conflict, "reasons": reasons,
            }),
        )
    }

    pub fn pr_added(text: impl Into<String>, pr: impl Serialize, repo: &str, base: &str, title: &str, head_ref: Option<&str>, author: Option<&str>, needs: impl Serialize, appended: bool) -> NewEvent {
        make(Kind::PrAdded, text, json!({"pr": pr, "repo": repo, "base": base, "title": title, "head_ref": head_ref, "author": author, "needs": needs, "appended": appended}))
    }

    pub fn pr_edited(text: impl Into<String>, pr: impl Serialize, fields: impl Serialize) -> NewEvent {
        make(Kind::PrEdited, text, json!({"pr": pr, "fields": fields}))
    }

    pub fn pr_described(text: impl Into<String>, pr: impl Serialize) -> NewEvent {
        make(Kind::PrDescribed, text, json!({"pr": pr}))
    }

    pub fn pr_cancelled(text: impl Into<String>, pr: impl Serialize) -> NewEvent {
        make(Kind::PrCancelled, text, json!({"pr": pr}))
    }

    /// `pr-cancelled` of a `dispatch` PR whose work another PR carried (§3.8, hs-d301): `superseded_by` is that PR as a reference.
    pub fn pr_cancelled_superseded(text: impl Into<String>, pr: impl Serialize, superseded_by: &str) -> NewEvent {
        make(Kind::PrCancelled, text, json!({"pr": pr, "superseded_by": superseded_by}))
    }

    pub fn pr_started(
        text: impl Into<String>,
        pr: impl Serialize,
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
            Kind::PrStarted,
            text,
            json!({
                "pr": pr, "repo": repo, "base": base, "lead_branch": lead_branch, "lead_worktree": lead_worktree,
                "base_sha": base_sha, "reused": reused, "pushed": pushed, "by": by,
            }),
        )
    }

    /// `pr-base-changed` (§5.2): `number` and `url` are in `data` for `by` `poll` only, see [`BasePull`].
    pub fn pr_base_changed(
        text: impl Into<String>,
        pr: impl Serialize,
        (from, to): (&str, &str),
        (from_pr, to_pr): (Option<&str>, Option<&str>),
        by: &str,
        pull_request: BasePull<'_>,
    ) -> NewEvent {
        let mut data = json!({"pr": pr, "from": from, "to": to, "from_pr": from_pr, "to_pr": to_pr, "by": by});
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
        make(Kind::PrBaseChanged, text, data)
    }

    pub fn pr_opened(text: impl Into<String>, pr: impl Serialize, number: i64, url: &str) -> NewEvent {
        make(Kind::PrOpened, text, json!({"pr": pr, "number": number, "url": url}))
    }

    pub fn pr_merged(text: impl Into<String>, pr: impl Serialize, number: i64, url: &str, merged_at: Option<&str>) -> NewEvent {
        make(Kind::PrMerged, text, json!({"pr": pr, "number": number, "url": url, "merged_at": merged_at}))
    }

    pub fn escalation(text: impl Into<String>, escalation: impl Serialize, question: &str) -> NewEvent {
        make(Kind::Escalation, text, json!({"escalation": escalation, "question": question}))
    }

    pub fn escalation_answered(text: impl Into<String>, escalation: impl Serialize) -> NewEvent {
        make(Kind::EscalationAnswered, text, json!({"escalation": escalation}))
    }

    pub fn escalation_expired(text: impl Into<String>, escalation: impl Serialize, reason: &str) -> NewEvent {
        make(Kind::EscalationExpired, text, json!({"escalation": escalation, "reason": reason}))
    }

    pub fn session_registered(text: impl Into<String>, role: &str, name: &str, forked_from: impl Serialize, tier: &str) -> NewEvent {
        make(Kind::SessionRegistered, text, json!({"role": role, "name": name, "forked_from": forked_from, "tier": tier}))
    }

    /// `reply` is in `data` only when true (`session state idle --reply`).
    pub fn session_state(text: impl Into<String>, from: &str, to: &str, label: Option<&str>, reply: bool) -> NewEvent {
        let mut data = json!({"from": from, "to": to, "label": label});
        if reply {
            data["reply"] = json!(true);
        }
        make(Kind::SessionState, text, data)
    }

    /// `pane` and `window` are null when the Session has no pane (`place` is then `none`).
    pub fn session_moved(text: impl Into<String>, pane: Option<&str>, window: Option<&str>, place: &str) -> NewEvent {
        make(Kind::SessionMoved, text, json!({"pane": pane, "window": window, "place": place}))
    }

    pub fn session_renamed(text: impl Into<String>, name: &str) -> NewEvent {
        make(Kind::SessionRenamed, text, json!({"name": name}))
    }

    pub fn review_opened(text: impl Into<String>, pn_id: &str, kind: &str, subject: &str, port: u16) -> NewEvent {
        make(Kind::ReviewOpened, text, json!({"pn_id": pn_id, "kind": kind, "subject": subject, "port": port}))
    }

    pub fn review_closed(text: impl Into<String>, pn_id: &str, ended: &str) -> NewEvent {
        make(Kind::ReviewClosed, text, json!({"pn_id": pn_id, "ended": ended}))
    }

    pub fn session_titled(text: impl Into<String>, title: &str) -> NewEvent {
        make(Kind::SessionTitled, text, json!({"title": title}))
    }

    pub fn session_retiered(text: impl Into<String>, from: &str, to: &str) -> NewEvent {
        make(Kind::SessionRetiered, text, json!({"from": from, "to": to}))
    }

    pub fn mcp_attached(text: impl Into<String>, name: &str, server: impl Serialize) -> NewEvent {
        make(Kind::McpAttached, text, json!({"name": name, "server": server}))
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

    pub fn records_retired(text: impl Into<String>, context: &str, approved: impl Serialize, rejected: impl Serialize) -> NewEvent {
        make(Kind::RecordsRetired, text, json!({"context": context, "approved": approved, "rejected": rejected}))
    }

    pub fn docs_decided(text: impl Into<String>, context: &str, sent: impl Serialize, dismissed: impl Serialize) -> NewEvent {
        make(Kind::DocsDecided, text, json!({"context": context, "sent": sent, "dismissed": dismissed}))
    }

    pub fn context_curated(text: impl Into<String>, context: &str, curated: u64, rejected: u64, retire: u64, docs: u64) -> NewEvent {
        let mut data = json!({"context": context, "curated": curated, "retire": retire, "docs": docs});
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

    pub fn session_message(text: impl Into<String>, from: impl Serialize, message: &str, wake: bool) -> NewEvent {
        make(Kind::SessionMessage, text, json!({"from": from, "text": message, "wake": wake}))
    }

    /// A `session-message` of `cpd session send`: it always wakes, so it carries `unblocks` and no `wake` (hs-d514).
    pub fn session_message_unblocking(text: impl Into<String>, from: impl Serialize, message: &str, unblocks: &str) -> NewEvent {
        make(Kind::SessionMessage, text, json!({"from": from, "text": message, "unblocks": unblocks}))
    }

    pub fn inbox_opened(text: impl Into<String>, item: i64, kind: &str, urgency: &str, label: &str) -> NewEvent {
        make(Kind::InboxOpened, text, json!({"item": item, "kind": kind, "urgency": urgency, "label": label}))
    }

    pub fn inbox_closed(text: impl Into<String>, item: i64, kind: &str) -> NewEvent {
        make(Kind::InboxClosed, text, json!({"item": item, "kind": kind}))
    }

    pub fn inbox_snoozed(text: impl Into<String>, item: i64, until: &str) -> NewEvent {
        make(Kind::InboxSnoozed, text, json!({"item": item, "until": until}))
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

    pub fn error(text: impl Into<String>, source: &str, code: &str, message: &str, details: impl Serialize) -> NewEvent {
        make(Kind::Error, text, json!({"source": source, "code": code, "message": message, "details": details}))
    }

    /// `incident-started` (contract §5.2, commands.md §25.1): `plans` is the array of Plan ids in
    /// the incident; `until_pr` (the `<plan>/<key>` auto-end PR) is in `data` only when set.
    pub fn incident_started(text: impl Into<String>, by: &str, reason: &str, started_at: &str, plans: impl Serialize, until_pr: Option<&str>) -> NewEvent {
        let mut data = json!({"by": by, "reason": reason, "started_at": started_at, "plans": plans});
        if let Some(until_pr) = until_pr {
            data["until_pr"] = json!(until_pr);
        }
        make(Kind::IncidentStarted, text, data)
    }

    /// `incident-plans` (contract §5.2, hs-d664): a `cpd incident start --plan` on a live incident
    /// that changed the Plan set or `until_pr`. `plans` is the new set; `until_pr` (the
    /// `<plan>/<key>` auto-end PR) is in `data` only when set, mirroring `incident-started`.
    pub fn incident_plans(text: impl Into<String>, by: &str, plans: impl Serialize, until_pr: Option<&str>) -> NewEvent {
        let mut data = json!({"by": by, "plans": plans});
        if let Some(until_pr) = until_pr {
            data["until_pr"] = json!(until_pr);
        }
        make(Kind::IncidentPlans, text, data)
    }

    /// `incident-ended` (contract §5.2, commands.md §25.2): `cause` is `"manual"` or `"until-pr"`.
    pub fn incident_ended(text: impl Into<String>, by: &str, ended_at: &str, cause: &str) -> NewEvent {
        make(Kind::IncidentEnded, text, json!({"by": by, "ended_at": ended_at, "cause": cause}))
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
            Kind::PlanDone => vec![NewEvent::plan_done(T, [s])],
            Kind::JobAdded => vec![
                NewEvent::job_added(T, s, s, s, b, Some(s), [s], b, b),
                NewEvent::job_added(T, s, s, s, b, None, [s], b, b),
                NewEvent::job_added_by_cpd(T, s, s, s, s, s, None),
                NewEvent::job_added_by_cpd(T, s, s, s, s, s, Some(s)),
            ],
            Kind::JobEdited => vec![NewEvent::job_edited(T, [s])],
            Kind::JobMerged => vec![NewEvent::job_merged(T, s, s, s, false, false, None), NewEvent::job_merged(T, s, s, s, true, true, Some((s, n, s)))],
            Kind::JobFailed => vec![NewEvent::job_failed(T, s, s, s, n)],
            Kind::JobRetried => vec![NewEvent::job_retried(T, s)],
            Kind::JobCancelRequested => vec![NewEvent::job_cancel_requested(T, s)],
            Kind::JobCancelled => vec![NewEvent::job_cancelled(T, s)],
            Kind::AttemptStarted => vec![NewEvent::attempt_started(T, n, s, s)],
            Kind::AttemptStatus => vec![
                NewEvent::attempt_status(T, s, s),
                NewEvent::attempt_check_start(T, s),
                NewEvent::attempt_check_end(T, s, false),
                NewEvent::attempt_check_end(T, s, true),
            ],
            Kind::AttemptRetry => vec![NewEvent::attempt_retry(T, s, s, s, s)],
            Kind::AttemptNudge => vec![NewEvent::attempt_nudge(T, s, 1)],
            Kind::AllMerged => vec![NewEvent::all_merged(T, s, [s], [s])],
            Kind::PrFeedback => vec![NewEvent::pr_feedback(T, s, n, s, s, s, s, s, s, b, &[s])],
            Kind::PrAdded => vec![NewEvent::pr_added(T, s, s, s, s, None, None, [s], b)],
            Kind::PrEdited => vec![NewEvent::pr_edited(T, s, [s])],
            Kind::PrDescribed => vec![NewEvent::pr_described(T, s)],
            Kind::PrCancelled => vec![NewEvent::pr_cancelled(T, s), NewEvent::pr_cancelled_superseded(T, s, s)],
            Kind::PrStarted => vec![NewEvent::pr_started(T, s, s, s, s, s, s, b, b, s)],
            Kind::PrBaseChanged => vec![
                NewEvent::pr_base_changed(T, s, (s, s), (None, Some(s)), s, BasePull::Absent),
                NewEvent::pr_base_changed(T, s, (s, s), (Some(s), None), s, BasePull::Of(1, s)),
                NewEvent::pr_base_changed(T, s, (s, s), (Some(s), None), s, BasePull::Null),
            ],
            Kind::PrOpened => vec![NewEvent::pr_opened(T, s, n, s)],
            Kind::PrMerged => vec![NewEvent::pr_merged(T, s, n, s, None)],
            Kind::Escalation => vec![NewEvent::escalation(T, s, s)],
            Kind::EscalationAnswered => vec![NewEvent::escalation_answered(T, s)],
            Kind::EscalationExpired => vec![NewEvent::escalation_expired(T, s, s)],
            Kind::SessionRegistered => vec![NewEvent::session_registered(T, s, s, s, s)],
            Kind::SessionState => vec![NewEvent::session_state(T, s, s, None, false), NewEvent::session_state(T, s, s, None, true)],
            Kind::SessionMoved => vec![NewEvent::session_moved(T, Some(s), Some(s), s), NewEvent::session_moved(T, None, None, s)],
            Kind::SessionRenamed => vec![NewEvent::session_renamed(T, s)],
            Kind::SessionTitled => vec![NewEvent::session_titled(T, s)],
            Kind::SessionRetiered => vec![NewEvent::session_retiered(T, s, s)],
            Kind::McpAttached => vec![NewEvent::mcp_attached(T, s, json!({}))],
            Kind::SessionAsleep => vec![NewEvent::session_asleep(T)],
            Kind::SessionResumed => vec![NewEvent::session_resumed(T)],
            Kind::PlanDeleted => vec![NewEvent::plan_deleted(T, &Counts::default())],
            Kind::ContextDeleted => vec![NewEvent::context_deleted(T, 0, &Counts::default())],
            Kind::RecordsRetired => vec![NewEvent::records_retired(T, s, [s], Vec::<String>::new())],
            Kind::DocsDecided => vec![NewEvent::docs_decided(T, s, [s], Vec::<String>::new())],
            Kind::ContextCurated => vec![NewEvent::context_curated(T, s, 0, 0, 0, 0), NewEvent::context_curated(T, s, 1, 1, 0, 0)],
            Kind::ContextRenamed => vec![NewEvent::context_renamed(T, s)],
            Kind::ContextOwner => vec![NewEvent::context_owner(T, s)],
            Kind::PlanRenamed => vec![NewEvent::plan_renamed(T, s)],
            Kind::ContextArchived => vec![NewEvent::context_archived(T, b)],
            Kind::PlanArchived => vec![NewEvent::plan_archived(T, b)],
            Kind::SessionArchived => vec![NewEvent::session_archived(T, b)],
            Kind::SessionMessage => vec![NewEvent::session_message(T, s, s, b), NewEvent::session_message_unblocking(T, s, s, s)],
            Kind::InboxOpened => vec![NewEvent::inbox_opened(T, 1, s, s, s)],
            Kind::InboxClosed => vec![NewEvent::inbox_closed(T, 1, s)],
            Kind::InboxSnoozed => vec![NewEvent::inbox_snoozed(T, 1, s)],
            Kind::InboxDismissed => vec![NewEvent::inbox_dismissed(T, 1)],
            Kind::InboxClaimed => vec![NewEvent::inbox_claimed(T, 1)],
            Kind::WatchStarted => vec![NewEvent::watch_started(T, 1)],
            Kind::WatchStopped => vec![NewEvent::watch_stopped(T, 1)],
            Kind::Error => vec![NewEvent::error(T, s, s, s, serde_json::Value::Null)],
            Kind::ReviewOpened => vec![NewEvent::review_opened(T, s, s, s, 1)],
            Kind::ReviewClosed => vec![NewEvent::review_closed(T, s, s)],
            Kind::IncidentStarted => vec![
                NewEvent::incident_started(T, s, s, s, [s], None),
                NewEvent::incident_started(T, s, s, s, [s], Some(s)),
            ],
            Kind::IncidentPlans => vec![
                NewEvent::incident_plans(T, s, [s], None),
                NewEvent::incident_plans(T, s, [s], Some(s)),
            ],
            Kind::IncidentEnded => vec![NewEvent::incident_ended(T, s, s, s)],
        }
    }

    /// `kind.rs`'s table is the oracle: every constructor of a kind carries the kind's required
    /// `data` fields and no field the kind does not list.
    #[test]
    fn every_constructor_makes_the_data_its_kind_lists() {
        for &kind in Kind::ALL {
            let spec = kind.spec();
            let samples = samples(kind);
            assert!(!samples.is_empty(), "no constructor for {kind}");
            for event in samples {
                assert_eq!(event.kind(), kind);
                let keys: BTreeSet<&str> = event.data().as_object().expect("data is an object").keys().map(String::as_str).collect();
                let missing: Vec<_> = spec.required.iter().filter(|k| !keys.contains(**k)).collect();
                let unknown: Vec<_> = keys.iter().filter(|k| !spec.required.contains(k) && !spec.optional.contains(k)).collect();
                assert!(missing.is_empty() && unknown.is_empty(), "{kind}: missing {missing:?}, unknown {unknown:?}");
            }
        }
    }

    /// The optional fields can all be present: each is made by some constructor.
    #[test]
    fn every_optional_field_has_a_constructor() {
        for &kind in Kind::ALL {
            let made: BTreeSet<String> =
                samples(kind).iter().flat_map(|e| e.data().as_object().unwrap().keys().cloned().collect::<Vec<_>>()).collect();
            // `plan-phase`'s `pr` is the contract's, for a phase change about one PR; no command makes it yet.
            for field in kind.spec().optional.iter().filter(|f| !(kind == Kind::PlanPhase && **f == "pr")) {
                assert!(made.contains(*field), "{kind}: no constructor makes the optional field {field:?}");
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
            for (i, _) in text.lines().enumerate().filter(|(_, l)| l.contains(&needle)) {
                found.push(format!("{}:{}", file.strip_prefix(&src).unwrap().display(), i + 1));
            }
        }
        assert!(found.is_empty(), "build these Events with the constructor of their kind (events/typed.rs): {found:#?}");
    }
}
