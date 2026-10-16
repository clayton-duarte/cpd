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
    Escalation = "escalation",
    EscalationAnswered = "escalation-answered",
    EscalationExpired = "escalation-expired",
    SessionRegistered = "session-registered",
    SessionState = "session-state",
    SessionMoved = "session-moved",
    SessionRenamed = "session-renamed",
    ContextRenamed = "context-renamed",
    PlanRenamed = "plan-renamed",
    ContextArchived = "context-archived",
    PlanArchived = "plan-archived",
    SessionArchived = "session-archived",
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
            PlanCreated => {
                spec([Y, Y, N, N, N], &["title", "repo", "base", "jira", "lead_session", "forked_from"], &[])
            }
            PlanPhase => spec([Y, Y, N, N, N], &["from", "to"], &["jobs", "pr"]),
            PlanReview => spec([Y, Y, N, N, N], &["review", "fingerprint", "status"], &["feedback"]),
            PlanDone => spec([Y, Y, N, N, N], &["pr", "merged_at"], &[]),
            JobAdded => spec([Y, Y, Y, N, N], &["status", "appended", "tier", "needs"], &[]),
            JobEdited => spec([Y, Y, Y, N, N], &["fields"], &[]),
            JobMerged => spec([Y, Y, Y, Y, N], &["sha", "tier"], &["cancel_too_late"]),
            JobFailed => spec([Y, Y, Y, M, N], &["cause", "detail", "tier", "attempts"], &[]),
            JobRetried => spec([Y, Y, Y, N, N], &["tier"], &[]),
            JobCancelRequested => spec([Y, Y, Y, Y, N], &["reason"], &[]),
            JobCancelled => spec([Y, Y, Y, M, N], &["reason"], &[]),
            AttemptStarted => spec([Y, Y, Y, Y, N], &["number", "tier", "origin"], &[]),
            AttemptStatus => spec([Y, Y, Y, Y, N], &["from", "to"], &[]),
            AttemptRetry => spec([Y, Y, Y, Y, N], &["outcome", "cause", "next_tier"], &["feedback"]),
            AttemptNudge => spec([Y, Y, Y, Y, Y], &["prompt"], &[]),
            AllMerged => spec([Y, Y, N, N, N], &["merged", "cancelled"], &[]),
            PrFeedback => spec([Y, Y, N, N, N], &["checks", "reviews", "comments", "state"], &[]),
            Escalation => spec([Y, Y, Y, Y, N], &["escalation", "question"], &[]),
            EscalationAnswered => spec([Y, Y, Y, Y, N], &["escalation"], &[]),
            EscalationExpired => spec([Y, Y, Y, Y, N], &["escalation", "reason"], &[]),
            SessionRegistered => spec([Y, M, N, N, Y], &["role", "name", "forked_from", "tier"], &[]),
            SessionState => spec([Y, M, N, N, Y], &["from", "to", "label"], &[]),
            SessionMoved => spec([Y, M, N, N, Y], &["pane", "window", "place"], &[]),
            SessionRenamed => spec([Y, M, N, N, Y], &["name"], &[]),
            ContextRenamed => spec([Y, N, N, N, N], &["title"], &[]),
            PlanRenamed => spec([Y, Y, N, N, N], &["title"], &[]),
            ContextArchived => spec([Y, N, N, N, N], &["archived"], &[]),
            PlanArchived => spec([Y, Y, N, N, N], &["archived"], &[]),
            SessionArchived => spec([Y, M, N, N, Y], &["archived"], &[]),
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
