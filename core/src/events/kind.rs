//! The Event kinds of contract §5.2: their names, which of `context`, `plan`,
//! `job`, `attempt` and `session` each sets, and the fields of its `data`.

use std::fmt;
use std::str::FromStr;

use crate::error::{CpdError, ErrorCode};

/// Whether an Event of a kind sets one of its scope columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Set {
    /// The column is NULL.
    No,
    /// The column may be set (the table's `plan?`).
    Maybe,
    /// The column is set.
    Yes,
}

/// What an Event of one kind carries (§5.2's "Set" and "`data`" columns).
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub context: Set,
    pub plan: Set,
    pub job: Set,
    pub attempt: Set,
    pub session: Set,
    /// The `data` keys that must be present (a value may be `null`).
    pub required: &'static [&'static str],
    /// The `data` keys that may be present (the table's `jobs?`, `feedback?`).
    pub optional: &'static [&'static str],
}

/// The adapter/cpd protocol number (commands.md §1.2, contract §5.2 "The protocol number"),
/// reported by `cpd --version`. Raise it whenever an Event kind, an Event `data` field the pi
/// adapter reads, or the output of a command the adapter runs changes in a way the adapter depends on.
/// The pi adapter's `PROTOCOL` (`pi/lib/event.ts`) must equal it; `pi/test/protocol.test.ts` checks.
pub const PROTOCOL: u32 = 22;

use Set::{Maybe as M, No as N, Yes as Y};

const fn spec(
    scope: [Set; 5],
    required: &'static [&'static str],
    optional: &'static [&'static str],
) -> Spec {
    Spec {
        context: scope[0],
        plan: scope[1],
        job: scope[2],
        attempt: scope[3],
        session: scope[4],
        required,
        optional,
    }
}

macro_rules! kinds {
    ($($variant:ident = $name:literal,)*) => {
        /// An Event kind: exactly the kinds of §5.2, so an unknown kind cannot be held.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Kind {
            $($variant,)*
        }

        impl Kind {
            /// Every kind, in §5.2's order.
            pub const ALL: &'static [Kind] = &[$(Kind::$variant,)*];

            /// The kind as stored and printed.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Kind::$variant => $name,)*
                }
            }
        }
    };
}

kinds! {
    ContextCreated = "context-created",
    ContextRecord = "context-record",
    ContextGrounded = "context-grounded",
    PlanCreated = "plan-created",
    PlanPhase = "plan-phase",
    PlanReview = "plan-review",
    PlanOutlined = "plan-outlined",
    PlanDone = "plan-done",
    JobAdded = "job-added",
    JobEdited = "job-edited",
    JobMerged = "job-merged",
    JobFailed = "job-failed",
    JobRetried = "job-retried",
    JobCancelRequested = "job-cancel-requested",
    JobCancelled = "job-cancelled",
    JobAddendum = "job-addendum",
    JobBlocked = "job-blocked",
    AttemptStarted = "attempt-started",
    AttemptStatus = "attempt-status",
    AttemptRetry = "attempt-retry",
    AttemptAborted = "attempt-aborted",
    AttemptNudge = "attempt-nudge",
    AttemptEnded = "attempt-ended",
    AllMerged = "all-merged",
    PrFeedback = "pr-feedback",
    WorkflowAdded = "workflow-added",
    WorkflowEdited = "workflow-edited",
    WorkflowDescribed = "workflow-described",
    WorkflowCancelled = "workflow-cancelled",
    WorkflowReopened = "workflow-reopened",
    WorkflowStarted = "workflow-started",
    WorkflowBaseChanged = "workflow-base-changed",
    PrOpened = "pr-opened",
    PrMerged = "pr-merged",
    Escalation = "escalation",
    EscalationAnswered = "escalation-answered",
    EscalationExpired = "escalation-expired",
    SessionRegistered = "session-registered",
    SessionState = "session-state",
    SessionMoved = "session-moved",
    SessionRenamed = "session-renamed",
    SessionTitled = "session-titled",
    SessionRetiered = "session-retiered",
    SessionModelChanged = "session-model-changed",
    McpAttached = "mcp-attached",
    SessionAsleep = "session-asleep",
    SessionResumed = "session-resumed",
    PlanDeleted = "plan-deleted",
    ContextDeleted = "context-deleted",
    RecordsRetired = "records-retired",
    // Retired (pro-d327): `cpd context docs` is gone; past Events keep the kind, nothing writes it.
    DocsDecided = "docs-decided",
    ContextCurated = "context-curated",
    ContextRenamed = "context-renamed",
    ContextOwner = "context-owner",
    PlanRenamed = "plan-renamed",
    ContextArchived = "context-archived",
    PlanArchived = "plan-archived",
    SessionArchived = "session-archived",
    SessionMessage = "session-message",
    InboxOpened = "inbox-opened",
    InboxClosed = "inbox-closed",
    InboxSnoozed = "inbox-snoozed",
    InboxDismissed = "inbox-dismissed",
    InboxClaimed = "inbox-claimed",
    WatchStarted = "watch-started",
    WatchStopped = "watch-stopped",
    Error = "error",
    ReviewOpened = "review-opened",
    ReviewClosed = "review-closed",
    IncidentStarted = "incident-started",
    IncidentPlans = "incident-plans",
    IncidentEnded = "incident-ended",
}

impl Kind {
    /// §5.2's row for this kind. `context` is `Yes` for every kind about a
    /// Context, a Plan or a Session: [`super::append`] fills it in from the
    /// Plan or the Session when the caller does not name it, because every
    /// Event of a Plan carries its Context (§5.3's filters rely on it).
    pub fn spec(self) -> Spec {
        use Kind::*;
        //                                  context plan job attempt session
        match self {
            ContextCreated => spec([Y, N, N, N, N], &["title", "jira", "created_on"], &[]),
            ContextRecord => spec([Y, N, N, N, N], &["record", "kind", "machine", "seq"], &[]),
            ContextGrounded => spec([Y, N, N, N, Y], &["seq", "forked_from"], &[]),
            PlanCreated => spec(
                [Y, Y, N, N, N],
                &["title", "jira", "lead_session", "forked_from", "cwd"],
                &[],
            ),
            PlanPhase => spec([Y, Y, N, N, N], &["from", "to"], &["jobs", "workflows"]),
            PlanReview => spec(
                [Y, Y, N, N, N],
                &["review", "fingerprint", "status"],
                &["feedback", "jobs"],
            ),
            PlanOutlined => spec([Y, Y, N, N, N], &["replaced"], &[]),
            PlanDone => spec([Y, Y, N, N, N], &["workflows"], &["turn"]),
            JobAdded => spec(
                [Y, Y, Y, N, N],
                &[
                    "workflow", "kind", "status", "appended", "tier", "needs", "urgent",
                ],
                &["by", "reason"],
            ),
            JobEdited => spec([Y, Y, Y, N, N], &["fields"], &[]),
            JobMerged => spec(
                [Y, Y, Y, Y, N],
                &["sha", "tier", "kind"],
                &[
                    "turn",
                    "cancel_too_late",
                    "workflow",
                    "number",
                    "url",
                    "summary",
                    "artefact",
                ],
            ),
            JobFailed => spec(
                [Y, Y, Y, M, N],
                &["cause", "detail", "tier", "attempts"],
                &["triage"],
            ),
            JobRetried => spec([Y, Y, Y, N, N], &["tier"], &[]),
            JobCancelRequested => spec([Y, Y, Y, Y, N], &["reason"], &[]),
            JobCancelled => spec([Y, Y, Y, M, N], &["reason"], &[]),
            JobAddendum => spec([Y, Y, Y, M, M], &["text"], &[]),
            JobBlocked => spec([Y, Y, Y, N, N], &["waiting"], &[]),
            AttemptStarted => spec([Y, Y, Y, Y, N], &["number", "tier", "origin"], &[]),
            AttemptStatus => spec([Y, Y, Y, Y, N], &["from", "to"], &["check", "what", "died"]),
            AttemptRetry => spec(
                [Y, Y, Y, Y, N],
                &["outcome", "cause", "next_tier"],
                &["feedback", "triage"],
            ),
            AttemptAborted => spec([Y, Y, Y, Y, N], &["tier", "by"], &[]),
            AttemptNudge => spec([Y, Y, Y, Y, Y], &["prompt"], &["turns"]),
            AttemptEnded => spec(
                [Y, Y, Y, Y, N],
                &[
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
                ],
                &[],
            ),
            AllMerged => spec([Y, Y, N, N, N], &["workflow", "merged", "cancelled"], &[]),
            PrFeedback => spec(
                [Y, Y, N, N, N],
                &[
                    "workflow",
                    "number",
                    "url",
                    "state",
                    "checks",
                    "review_decision",
                    "mergeable",
                    "merge_state",
                    "conflict",
                    "reasons",
                ],
                &[],
            ),
            WorkflowAdded => spec(
                [Y, Y, N, N, N],
                &["workflow", "repo", "base", "needs", "appended"],
                &["title", "head_ref", "author"],
            ),
            WorkflowEdited => spec([Y, Y, N, N, N], &["workflow", "fields"], &[]),
            WorkflowDescribed => spec([Y, Y, N, N, N], &["workflow"], &[]),
            WorkflowCancelled => spec([Y, Y, N, N, N], &["workflow"], &["superseded_by"]),
            WorkflowReopened => spec([Y, Y, N, N, N], &["workflow"], &[]),
            WorkflowStarted => spec(
                [Y, Y, N, N, N],
                &[
                    "workflow",
                    "repo",
                    "base",
                    "lead_branch",
                    "lead_worktree",
                    "base_sha",
                    "reused",
                    "pushed",
                    "by",
                ],
                &[],
            ),
            WorkflowBaseChanged => spec(
                [Y, Y, N, N, N],
                &[
                    "workflow",
                    "from",
                    "to",
                    "from_workflow",
                    "to_workflow",
                    "by",
                ],
                &["number", "url"],
            ),
            PrOpened => spec([Y, Y, N, N, N], &["workflow", "number", "url"], &[]),
            PrMerged => spec(
                [Y, Y, N, N, N],
                &["workflow", "number", "url", "merged_at"],
                &[],
            ),
            Escalation => spec([Y, Y, Y, Y, N], &["escalation", "question"], &[]),
            EscalationAnswered => spec([Y, Y, Y, Y, N], &["escalation"], &[]),
            EscalationExpired => spec([Y, Y, Y, Y, N], &["escalation", "reason"], &[]),
            SessionRegistered => spec(
                [Y, M, N, N, Y],
                &["role", "name", "forked_from", "tier"],
                &[],
            ),
            SessionState => spec([Y, M, N, N, Y], &["from", "to", "label"], &["reply"]),
            SessionMoved => spec([Y, M, N, N, Y], &["pane", "window", "place"], &[]),
            SessionRenamed => spec([Y, M, N, N, Y], &["name"], &[]),
            SessionTitled => spec([Y, M, N, N, Y], &["title"], &[]),
            SessionRetiered => spec([Y, M, N, N, Y], &["from", "to"], &[]),
            SessionModelChanged => spec([Y, M, N, N, Y], &["from", "to"], &[]),
            McpAttached => spec([Y, M, N, N, Y], &["name", "server"], &[]),
            SessionAsleep => spec([Y, M, N, N, Y], &[], &[]),
            SessionResumed => spec([Y, M, N, N, Y], &[], &[]),
            PlanDeleted => spec(
                [Y, Y, N, N, N],
                &["jobs", "attempts", "sessions", "worktrees", "disk_freed_mb"],
                &[],
            ),
            ContextDeleted => spec(
                [Y, N, N, N, N],
                &[
                    "plans",
                    "jobs",
                    "attempts",
                    "sessions",
                    "worktrees",
                    "disk_freed_mb",
                ],
                &[],
            ),
            RecordsRetired => spec([Y, N, N, N, N], &["context", "approved", "rejected"], &[]),
            DocsDecided => spec([Y, N, N, N, N], &["context", "sent", "dismissed"], &[]),
            ContextCurated => spec(
                [Y, N, N, N, N],
                &["context", "curated", "retire"],
                &["rejected"],
            ),
            ContextRenamed => spec([Y, N, N, N, N], &["title"], &[]),
            ContextOwner => spec([Y, N, N, N, N], &["owner"], &[]),
            PlanRenamed => spec([Y, Y, N, N, N], &["title"], &[]),
            ContextArchived => spec([Y, N, N, N, N], &["archived"], &[]),
            PlanArchived => spec([Y, Y, N, N, N], &["archived"], &[]),
            SessionArchived => spec([Y, M, N, N, Y], &["archived"], &[]),
            SessionMessage => spec(
                [Y, M, N, N, Y],
                &["from", "text"],
                &["unblocks", "wake", "head_oid", "origin"],
            ),
            InboxOpened => spec([M, M, M, N, M], &["item", "kind", "urgency", "label"], &[]),
            InboxClosed => spec([M, M, M, N, M], &["item", "kind"], &[]),
            InboxSnoozed => spec([M, M, M, N, M], &["item", "until"], &[]),
            InboxDismissed | InboxClaimed => spec([M, M, M, N, M], &["item"], &[]),
            WatchStarted | WatchStopped => spec([N, N, N, N, N], &["pid"], &[]),
            ReviewOpened => spec([Y, M, N, N, Y], &["pn_id", "kind", "subject", "port"], &[]),
            ReviewClosed => spec([Y, M, N, N, Y], &["pn_id", "ended"], &[]),
            IncidentStarted => spec(
                [N, N, N, N, N],
                &["by", "reason", "started_at", "plans"],
                &["until_workflow"],
            ),
            IncidentPlans => spec([N, N, N, N, N], &["by", "plans"], &["until_workflow"]),
            IncidentEnded => spec([N, N, N, N, N], &["by", "ended_at", "cause"], &[]),
            Error => spec(
                [M, M, M, M, M],
                &["source", "code", "message", "details"],
                &[],
            ),
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A kind that is not in §5.2 is `usage` (commands.md §11).
impl FromStr for Kind {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Kind, CpdError> {
        Kind::ALL
            .iter()
            .copied()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| {
                CpdError::new(
                    ErrorCode::Usage,
                    format!("Unknown Event kind {s:?}: see contract §5.2."),
                )
                .with("kind", s)
                .with(
                    "kinds",
                    Kind::ALL.iter().map(|k| k.as_str()).collect::<Vec<_>>(),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::CpdHome;
    use crate::store::Store;

    const REVIEW_KINDS: [&str; 2] = ["review-opened", "review-closed"];

    /// The protocol history: 8 (`session-retiered`), 10 (`plan-outlined`), 12 (`session-resumed`), 13 (`session report`),
    /// 14 (the incident kinds), 15 (`mcp-attached`), 16 (`session switch-failed`/`switched`), 17 (`workflow-described`),
    /// 18 (`session-model-changed`, the tier-preset switch, commands.md §14.6), 19 (`context pending`'s plan-level
    /// `jobs`, commands.md §3.12, Plan explore-view) and 20 (the Workflow rename: the six object kinds are
    /// `workflow-*` and the object data key is `workflow`) and 21 (agent keys: `key` in `session show`, `from_name` in `board take`).
    #[test]
    fn the_protocol_is_22_with_workflow_reopened() {
        assert_eq!(PROTOCOL, 22);
    }

    #[test]
    fn session_retiered_is_about_a_session_and_carries_from_and_to() {
        let spec = Kind::SessionRetiered.spec();
        assert_eq!(
            (
                spec.context,
                spec.plan,
                spec.job,
                spec.attempt,
                spec.session
            ),
            (Y, M, N, N, Y)
        );
        assert_eq!(spec.required, ["from", "to"]);
    }

    #[test]
    fn session_model_changed_is_about_a_session_and_carries_from_and_to() {
        let spec = Kind::SessionModelChanged.spec();
        assert_eq!(
            (
                spec.context,
                spec.plan,
                spec.job,
                spec.attempt,
                spec.session
            ),
            (Y, M, N, N, Y)
        );
        assert_eq!(spec.required, ["from", "to"]);
    }

    #[test]
    fn the_review_kinds_exist_and_are_about_a_session() {
        for name in REVIEW_KINDS {
            let kind: Kind = name.parse().unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_eq!(kind.as_str(), name);
            let spec = kind.spec();
            assert_eq!(
                (
                    spec.context,
                    spec.plan,
                    spec.job,
                    spec.attempt,
                    spec.session
                ),
                (Y, M, N, N, Y),
                "{name}"
            );
        }
        let data = |name: &str| Kind::from_str(name).unwrap().spec().required;
        assert_eq!(data("review-opened"), ["pn_id", "kind", "subject", "port"]);
        assert_eq!(data("review-closed"), ["pn_id", "ended"]);
    }

    /// A kind exists when its `event_kinds` row does (§4.4 K1): one additive migration of one statement per kind.
    #[test]
    fn a_fresh_store_lists_the_review_kinds_in_event_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        let listed: Vec<String> = store
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT name FROM event_kinds WHERE name LIKE 'review-%' ORDER BY name",
                )?;
                let names = stmt
                    .query_map([], |r| r.get(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                Ok(names)
            })
            .unwrap();
        assert_eq!(listed, ["review-closed", "review-opened"]);
    }
}
