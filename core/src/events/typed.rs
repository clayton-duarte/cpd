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

    pub fn plan_done(text: impl Into<String>, prs: impl Serialize) -> NewEvent {
        make(Kind::PlanDone, text, json!({"prs": prs}))
    }

    pub fn job_added(
        text: impl Into<String>,
        pr: impl Serialize,
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
            json!({"pr": pr, "status": status, "appended": appended, "tier": tier, "needs": needs, "review": review, "urgent": urgent}),
        )
    }

    pub fn job_edited(text: impl Into<String>, fields: impl Serialize) -> NewEvent {
        make(Kind::JobEdited, text, json!({"fields": fields}))
    }

    /// `cancel_too_late` is in `data` only when true.
    pub fn job_merged(text: impl Into<String>, sha: &str, tier: &str, cancel_too_late: bool) -> NewEvent {
        let mut data = json!({"sha": sha, "tier": tier});
        if cancel_too_late {
            data["cancel_too_late"] = json!(true);
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

    pub fn pr_added(text: impl Into<String>, pr: impl Serialize, repo: &str, base: &str, title: &str, needs: impl Serialize, appended: bool) -> NewEvent {
        make(Kind::PrAdded, text, json!({"pr": pr, "repo": repo, "base": base, "title": title, "needs": needs, "appended": appended}))
    }

    pub fn pr_edited(text: impl Into<String>, pr: impl Serialize, fields: impl Serialize) -> NewEvent {
        make(Kind::PrEdited, text, json!({"pr": pr, "fields": fields}))
    }

    pub fn pr_cancelled(text: impl Into<String>, pr: impl Serialize) -> NewEvent {
        make(Kind::PrCancelled, text, json!({"pr": pr}))
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

    /// `pr-base-changed` (§5.2): `number` and `url` are the pull request's, set by the poll (`by` `poll`) only.
    pub fn pr_base_changed(
        text: impl Into<String>,
        pr: impl Serialize,
        (from, to): (&str, &str),
        (from_pr, to_pr): (Option<&str>, Option<&str>),
        by: &str,
        pull_request: Option<(i64, &str)>,
    ) -> NewEvent {
        let mut data = json!({"pr": pr, "from": from, "to": to, "from_pr": from_pr, "to_pr": to_pr, "by": by});
        if let Some((number, url)) = pull_request {
            data["number"] = json!(number);
            data["url"] = json!(url);
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

    pub fn session_titled(text: impl Into<String>, title: &str) -> NewEvent {
        make(Kind::SessionTitled, text, json!({"title": title}))
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

    pub fn watch_started(text: impl Into<String>, pid: u32) -> NewEvent {
        make(Kind::WatchStarted, text, json!({"pid": pid}))
    }

    pub fn watch_stopped(text: impl Into<String>, pid: u32) -> NewEvent {
        make(Kind::WatchStopped, text, json!({"pid": pid}))
    }

    pub fn error(text: impl Into<String>, source: &str, code: &str, message: &str, details: impl Serialize) -> NewEvent {
        make(Kind::Error, text, json!({"source": source, "code": code, "message": message, "details": details}))
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
                NewEvent::plan_review_decided(T, s, s, s, s, None::<Vec<&str>>),
                NewEvent::plan_review_decided(T, s, s, s, s, Some([s])),
            ],
            Kind::PlanDone => vec![NewEvent::plan_done(T, [s])],
            Kind::JobAdded => vec![NewEvent::job_added(T, s, s, b, Some(s), [s], b, b), NewEvent::job_added(T, s, s, b, None, [s], b, b)],
            Kind::JobEdited => vec![NewEvent::job_edited(T, [s])],
            Kind::JobMerged => vec![NewEvent::job_merged(T, s, s, false), NewEvent::job_merged(T, s, s, true)],
            Kind::JobFailed => vec![NewEvent::job_failed(T, s, s, s, n)],
            Kind::JobRetried => vec![NewEvent::job_retried(T, s)],
            Kind::JobCancelRequested => vec![NewEvent::job_cancel_requested(T, s)],
            Kind::JobCancelled => vec![NewEvent::job_cancelled(T, s)],
            Kind::AttemptStarted => vec![NewEvent::attempt_started(T, n, s, s)],
            Kind::AttemptStatus => vec![NewEvent::attempt_status(T, s, s)],
            Kind::AttemptRetry => vec![NewEvent::attempt_retry(T, s, s, s, s)],
            Kind::AttemptNudge => vec![NewEvent::attempt_nudge(T, s, 1)],
            Kind::AllMerged => vec![NewEvent::all_merged(T, s, [s], [s])],
            Kind::PrFeedback => vec![NewEvent::pr_feedback(T, s, n, s, s, s, s, s, s, b, &[s])],
            Kind::PrAdded => vec![NewEvent::pr_added(T, s, s, s, s, [s], b)],
            Kind::PrEdited => vec![NewEvent::pr_edited(T, s, [s])],
            Kind::PrCancelled => vec![NewEvent::pr_cancelled(T, s)],
            Kind::PrStarted => vec![NewEvent::pr_started(T, s, s, s, s, s, s, b, b, s)],
            Kind::PrBaseChanged => vec![
                NewEvent::pr_base_changed(T, s, (s, s), (None, Some(s)), s, None),
                NewEvent::pr_base_changed(T, s, (s, s), (Some(s), None), s, Some((1, s))),
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
            Kind::ContextRenamed => vec![NewEvent::context_renamed(T, s)],
            Kind::ContextOwner => vec![NewEvent::context_owner(T, s)],
            Kind::PlanRenamed => vec![NewEvent::plan_renamed(T, s)],
            Kind::ContextArchived => vec![NewEvent::context_archived(T, b)],
            Kind::PlanArchived => vec![NewEvent::plan_archived(T, b)],
            Kind::SessionArchived => vec![NewEvent::session_archived(T, b)],
            Kind::SessionMessage => vec![NewEvent::session_message(T, s, s, b)],
            Kind::InboxOpened => vec![NewEvent::inbox_opened(T, 1, s, s, s)],
            Kind::InboxClosed => vec![NewEvent::inbox_closed(T, 1, s)],
            Kind::InboxSnoozed => vec![NewEvent::inbox_snoozed(T, 1, s)],
            Kind::InboxDismissed => vec![NewEvent::inbox_dismissed(T, 1)],
            Kind::WatchStarted => vec![NewEvent::watch_started(T, 1)],
            Kind::WatchStopped => vec![NewEvent::watch_stopped(T, 1)],
            Kind::Error => vec![NewEvent::error(T, s, s, s, serde_json::Value::Null)],
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
