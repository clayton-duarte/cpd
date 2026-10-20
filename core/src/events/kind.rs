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
pub const PROTOCOL: u32 = 5;

use Set::{Maybe as M, No as N, Yes as Y};

const fn spec(scope: [Set; 5], required: &'static [&'static str], optional: &'static [&'static str]) -> Spec {
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
    PlanDone = "plan-done",
    JobAdded = "job-added",
    JobEdited = "job-edited",
    JobMerged = "job-merged",
    JobFailed = "job-failed",
    JobRetried = "job-retried",
    JobCancelRequested = "job-cancel-requested",
    JobCancelled = "job-cancelled",
    AttemptStarted = "attempt-started",
    AttemptStatus = "attempt-status",
    AttemptRetry = "attempt-retry",
    AttemptNudge = "attempt-nudge",
    AllMerged = "all-merged",
    PrFeedback = "pr-feedback",
    PrAdded = "pr-added",
    PrEdited = "pr-edited",
    PrCancelled = "pr-cancelled",
    PrStarted = "pr-started",
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
    WatchStarted = "watch-started",
    WatchStopped = "watch-stopped",
    Error = "error",
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
            PlanCreated => spec([Y, Y, N, N, N], &["title", "jira", "lead_session", "forked_from", "cwd"], &[]),
            PlanPhase => spec([Y, Y, N, N, N], &["from", "to"], &["jobs", "pr", "prs"]),
            PlanReview => spec([Y, Y, N, N, N], &["review", "fingerprint", "status"], &["feedback", "jobs"]),
            PlanDone => spec([Y, Y, N, N, N], &["prs"], &[]),
            JobAdded => spec([Y, Y, Y, N, N], &["pr", "status", "appended", "tier", "needs", "review", "urgent"], &[]),
            JobEdited => spec([Y, Y, Y, N, N], &["fields"], &[]),
            JobMerged => spec([Y, Y, Y, Y, N], &["sha", "tier"], &["cancel_too_late"]),
            JobFailed => spec([Y, Y, Y, M, N], &["cause", "detail", "tier", "attempts"], &[]),
            JobRetried => spec([Y, Y, Y, N, N], &["tier"], &[]),
            JobCancelRequested => spec([Y, Y, Y, Y, N], &["reason"], &[]),
            JobCancelled => spec([Y, Y, Y, M, N], &["reason"], &[]),
            AttemptStarted => spec([Y, Y, Y, Y, N], &["number", "tier", "origin"], &[]),
            AttemptStatus => spec([Y, Y, Y, Y, N], &["from", "to"], &[]),
            AttemptRetry => spec([Y, Y, Y, Y, N], &["outcome", "cause", "next_tier"], &["feedback"]),
            AttemptNudge => spec([Y, Y, Y, Y, Y], &["prompt"], &["turns"]),
            AllMerged => spec([Y, Y, N, N, N], &["pr", "merged", "cancelled"], &[]),
            PrFeedback => spec([Y, Y, N, N, N], &["pr", "number", "url", "state", "checks", "review_decision", "mergeable", "merge_state", "conflict", "reasons"], &[]),
            PrAdded => spec([Y, Y, N, N, N], &["pr", "repo", "base", "needs", "appended"], &["title"]),
            PrEdited => spec([Y, Y, N, N, N], &["pr", "fields"], &[]),
            PrCancelled => spec([Y, Y, N, N, N], &["pr"], &[]),
            PrStarted => spec(
                [Y, Y, N, N, N],
                &["pr", "repo", "base", "lead_branch", "lead_worktree", "base_sha", "reused", "pushed", "by"],
                &[],
            ),
            PrOpened => spec([Y, Y, N, N, N], &["pr", "number", "url"], &[]),
            PrMerged => spec([Y, Y, N, N, N], &["pr", "number", "url", "merged_at"], &[]),
            Escalation => spec([Y, Y, Y, Y, N], &["escalation", "question"], &[]),
            EscalationAnswered => spec([Y, Y, Y, Y, N], &["escalation"], &[]),
            EscalationExpired => spec([Y, Y, Y, Y, N], &["escalation", "reason"], &[]),
            SessionRegistered => spec([Y, M, N, N, Y], &["role", "name", "forked_from", "tier"], &[]),
            SessionState => spec([Y, M, N, N, Y], &["from", "to", "label"], &["reply"]),
            SessionMoved => spec([Y, M, N, N, Y], &["pane", "window", "place"], &[]),
            SessionRenamed => spec([Y, M, N, N, Y], &["name"], &[]),
            SessionTitled => spec([Y, M, N, N, Y], &["title"], &[]),
            ContextRenamed => spec([Y, N, N, N, N], &["title"], &[]),
            ContextOwner => spec([Y, N, N, N, N], &["owner"], &[]),
            PlanRenamed => spec([Y, Y, N, N, N], &["title"], &[]),
            ContextArchived => spec([Y, N, N, N, N], &["archived"], &[]),
            PlanArchived => spec([Y, Y, N, N, N], &["archived"], &[]),
            SessionArchived => spec([Y, M, N, N, Y], &["archived"], &[]),
            SessionMessage => spec([Y, M, N, N, Y], &["from", "text", "wake"], &[]),
            InboxOpened => spec([M, M, M, N, M], &["item", "kind", "urgency", "label"], &[]),
            InboxClosed => spec([M, M, M, N, M], &["item", "kind"], &[]),
            InboxSnoozed => spec([M, M, M, N, M], &["item", "until"], &[]),
            InboxDismissed => spec([M, M, M, N, M], &["item"], &[]),
            WatchStarted | WatchStopped => spec([N, N, N, N, N], &["pid"], &[]),
            Error => spec([M, M, M, M, M], &["source", "code", "message", "details"], &[]),
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
        Kind::ALL.iter().copied().find(|k| k.as_str() == s).ok_or_else(|| {
            CpdError::new(ErrorCode::Usage, format!("Unknown Event kind {s:?}: see contract §5.2."))
                .with("kind", s)
                .with("kinds", Kind::ALL.iter().map(|k| k.as_str()).collect::<Vec<_>>())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// The protocol and the `sha256` of the sorted kinds, joined by `\n`, they were last raised for.
    const PINNED: (u32, &str) = (5, "67500a68aefd137727dd10402fc7387d7bb9c182c2f92431439acb282040b29f");

    fn kinds_hash() -> String {
        let mut names: Vec<&str> = Kind::ALL.iter().map(|k| k.as_str()).collect();
        names.sort_unstable();
        format!("{:x}", Sha256::digest(names.join("\n").as_bytes()))
    }

    #[test]
    fn the_event_kinds_are_pinned_to_the_protocol() {
        assert_eq!(
            (PROTOCOL, kinds_hash().as_str()),
            PINNED,
            "The Event kinds or PROTOCOL changed. Raise PROTOCOL (core/src/events/kind.rs) when the kinds changed, \
             then set PINNED to ({PROTOCOL}, {:?}) and update `PROTOCOL` and `KINDS` in pi/lib/event.ts.",
            kinds_hash()
        );
    }
}
