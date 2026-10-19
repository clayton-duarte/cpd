//! The one error type (contract §1.2, commands.md §1.2, §1.3).
//!
//! Every failure is a [`CpdError`]: a [`ErrorCode`], a one-sentence message
//! and a `details` object. The code fixes the exit code, so a code can never be
//! printed with the wrong one.

use std::fmt;

use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::{Map, Value};

/// The process exit codes of contract §1.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Exit {
    /// `0`: ok.
    Ok = 0,
    /// `1`: an external tool failed, or something unexpected.
    Failure = 1,
    /// `2`: usage error.
    Usage = 2,
    /// `3`: refused by an invariant.
    Refused = 3,
    /// `4`: not found.
    NotFound = 4,
    /// `5`: timeout.
    Timeout = 5,
}

impl Exit {
    pub fn code(self) -> u8 {
        self as u8
    }
}

macro_rules! error_codes {
    ($($variant:ident = $name:literal => $exit:ident,)*) => {
        /// Every error code of contract.md and commands.md, with its exit code.
        /// To add one, add a line here (see docs/implementing.md).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum ErrorCode {
            $($variant,)*
        }

        impl ErrorCode {
            /// Every code, in declaration order.
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$variant,)*];

            /// The snake_case code printed in `error.code`.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(ErrorCode::$variant => $name,)*
                }
            }

            /// The exit code this error ends the process with.
            pub fn exit(self) -> Exit {
                match self {
                    $(ErrorCode::$variant => Exit::$exit,)*
                }
            }
        }
    };
}

error_codes! {
    // Exit 1: an external tool failed, or something unexpected.
    ConfigMissing = "config_missing" => Failure,
    ConfigInvalid = "config_invalid" => Failure,
    StoreTooNew = "store_too_new" => Failure,
    MigrationFailed = "migration_failed" => Failure,
    MigrationChanged = "migration_changed" => Failure,
    MigrationOrder = "migration_order" => Failure,
    StoreTampered = "store_tampered" => Failure,
    StoreFailed = "store_failed" => Failure,
    Internal = "internal" => Failure,
    GitFailed = "git_failed" => Failure,
    TmuxFailed = "tmux_failed" => Failure,
    GhFailed = "gh_failed" => Failure,
    GateError = "gate_error" => Failure,
    SetupFailed = "setup_failed" => Failure,
    WatchStartFailed = "watch_start_failed" => Failure,
    SpawnFailed = "spawn_failed" => Failure,
    CheckError = "check_error" => Failure,
    SessionMalformed = "session_malformed" => Failure,
    DriverLockMissing = "driver_lock_missing" => Failure,
    DoctorFailed = "doctor_failed" => Failure,
    PiNotFound = "pi_not_found" => Failure,
    CatalogFailed = "catalog_failed" => Failure,
    TierModelUnknown = "tier_model_unknown" => Failure,
    HookInvalid = "hook_invalid" => Failure,
    LeadWorktreeStale = "lead_worktree_stale" => Failure,
    GhRateLimited = "gh_rate_limited" => Failure,
    AdapterMissing = "adapter_missing" => Failure,
    MergeCheckFailed = "merge_check_failed" => Failure,
    PushFailed = "push_failed" => Failure,
    CleanupFailed = "cleanup_failed" => Failure,
    DistillInvalid = "distill_invalid" => Failure,
    DistillFailed = "distill_failed" => Failure,
    // Policies (contract §8.3, hs-d288).
    PolicyInvalid = "policy_invalid" => Failure,
    OwnerUnknown = "owner_unknown" => Failure,
    RepoUnkeyed = "repo_unkeyed" => Failure,

    // Exit 2: usage error.
    Usage = "usage" => Usage,
    PlanRequired = "plan_required" => Usage,
    ContextRequired = "context_required" => Usage,
    SessionRequired = "session_required" => Usage,
    InvalidContextId = "invalid_context_id" => Usage,
    InvalidPlanId = "invalid_plan_id" => Usage,
    InvalidJobId = "invalid_job_id" => Usage,
    InvalidAttemptId = "invalid_attempt_id" => Usage,
    InvalidEscalationId = "invalid_escalation_id" => Usage,
    InvalidReviewId = "invalid_review_id" => Usage,
    InvalidSessionId = "invalid_session_id" => Usage,
    InvalidRecordId = "invalid_record_id" => Usage,
    InvalidName = "invalid_name" => Usage,
    FileUnreadable = "file_unreadable" => Usage,
    SpecInvalid = "spec_invalid" => Usage,
    TierInvalid = "tier_invalid" => Usage,
    NothingToEdit = "nothing_to_edit" => Usage,

    // Exit 3: refused by an invariant.
    CasLost = "cas_lost" => Refused,
    DistillRunning = "distill_running" => Refused,
    IllegalTransition = "illegal_transition" => Refused,
    LockedJobTampered = "locked_job_tampered" => Refused,
    RecordNotFound = "record_not_found" => Refused,
    ContextExists = "context_exists" => Refused,
    QuestionAnswered = "question_answered" => Refused,
    NotAGitRepo = "not_a_git_repo" => Refused,
    PlanExists = "plan_exists" => Refused,
    BranchExists = "branch_exists" => Refused,
    IllegalPhase = "illegal_phase" => Refused,
    NoDraftJobs = "no_draft_jobs" => Refused,
    ReviewDecided = "review_decided" => Refused,
    PlanNotApproved = "plan_not_approved" => Refused,
    DependencyCycle = "dependency_cycle" => Refused,
    WorktreeExists = "worktree_exists" => Refused,
    JobKeyExists = "job_key_exists" => Refused,
    DraftIncomplete = "draft_incomplete" => Refused,
    NeedsUnknown = "needs_unknown" => Refused,
    NeedsUnavailable = "needs_unavailable" => Refused,
    JobLocked = "job_locked" => Refused,
    JobMerged = "job_merged" => Refused,
    JobNotFailed = "job_not_failed" => Refused,
    JobNotReady = "job_not_ready" => Refused,
    NoSlot = "no_slot" => Refused,
    NoPort = "no_port" => Refused,
    AttemptNotRunning = "attempt_not_running" => Refused,
    WrongWorktree = "wrong_worktree" => Refused,
    DirtyTree = "dirty_tree" => Refused,
    NoCommits = "no_commits" => Refused,
    ReviewCommits = "review_commits" => Refused,
    ReviewMissing = "review_missing" => Refused,
    GateTampered = "gate_tampered" => Refused,
    CheckFailed = "check_failed" => Refused,
    NotAwaitingReview = "not_awaiting_review" => Refused,
    EscalationAnswered = "escalation_answered" => Refused,
    EscalationExpired = "escalation_expired" => Refused,
    JobsNotDone = "jobs_not_done" => Refused,
    DirtyTarget = "dirty_target" => Refused,
    GateFailed = "gate_failed" => Refused,
    RebaseConflict = "rebase_conflict" => Refused,
    PushRejected = "push_rejected" => Refused,
    SessionAmbiguous = "session_ambiguous" => Refused,
    NotALead = "not_a_lead" => Refused,
    NoCockpit = "no_cockpit" => Refused,
    WrongPane = "wrong_pane" => Refused,
    InChatSlot = "in_chat_slot" => Refused,
    CursorAhead = "cursor_ahead" => Refused,
    WatchRunning = "watch_running" => Refused,
    NotATerminal = "not_a_terminal" => Refused,
    ArchiveBlocked = "archive_blocked" => Refused,
    Archived = "archived" => Refused,
    ItemClosed = "item_closed" => Refused,
    MergeNotClean = "merge_not_clean" => Refused,
    OwnerMismatch = "owner_mismatch" => Refused,
    OwnerUnset = "owner_unset" => Refused,
    OwnerSet = "owner_set" => Refused,

    // Exit 4: not found.
    ContextNotFound = "context_not_found" => NotFound,
    PlanNotFound = "plan_not_found" => NotFound,
    JobNotFound = "job_not_found" => NotFound,
    AttemptNotFound = "attempt_not_found" => NotFound,
    EscalationNotFound = "escalation_not_found" => NotFound,
    ReviewNotFound = "review_not_found" => NotFound,
    SessionNotFound = "session_not_found" => NotFound,
    BaseNotFound = "base_not_found" => NotFound,
    ItemNotFound = "item_not_found" => NotFound,

    // Exit 5: timeout.
    StoreBusy = "store_busy" => Timeout,
    LockTimeout = "lock_timeout" => Timeout,
    ToolTimeout = "tool_timeout" => Timeout,
    AskTimeout = "ask_timeout" => Timeout,
    CancelTimeout = "cancel_timeout" => Timeout,

    // R34, Plans across repos (contract §1.2, §3.8). `branch_exists` is above.
    PrStartFailed = "pr_start_failed" => Failure,
    PrRequired = "pr_required" => Usage,
    InvalidPrId = "invalid_pr_id" => Usage,
    PrExists = "pr_exists" => Refused,
    PrInUse = "pr_in_use" => Refused,
    PrEmpty = "pr_empty" => Refused,
    PrLocked = "pr_locked" => Refused,
    PrNotReady = "pr_not_ready" => Refused,
    PrEnded = "pr_ended" => Refused,
    PrClosed = "pr_closed" => Refused,
    NeedsOtherPr = "needs_other_pr" => Refused,
    PrNotFound = "pr_not_found" => NotFound,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// A failure: printed as `{"ok": false, "error": {"code", "message", "details"}}`.
#[derive(Clone, Debug, PartialEq)]
pub struct CpdError {
    code: ErrorCode,
    message: String,
    details: Map<String, Value>,
}

impl CpdError {
    /// An error with empty `details`.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        CpdError { code, message: message.into(), details: Map::new() }
    }

    /// Adds one `details` field (replacing a field of the same name).
    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.details.insert(key.to_owned(), value.into());
        self
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn details(&self) -> &Map<String, Value> {
        &self.details
    }

    pub fn exit(&self) -> Exit {
        self.code.exit()
    }

    /// The `error` object: `{"code", "message", "details"}`. Also the shape of
    /// an `error` Event's data (contract §5.2), with `source` added there.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(ErrorObject(self)).expect("JSON values always serialise")
    }
}

impl fmt::Display for CpdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CpdError {}

/// Serialises as the whole failure envelope, `"ok"` first.
impl Serialize for CpdError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("ok", &false)?;
        map.serialize_entry("error", &ErrorObject(self))?;
        map.end()
    }
}

/// `{"code", "message", "details"}`, in that order when printed.
struct ErrorObject<'a>(&'a CpdError);

impl Serialize for ErrorObject<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("code", &self.0.code)?;
        map.serialize_entry("message", &self.0.message)?;
        map.serialize_entry("details", &self.0.details)?;
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn is_snake_case(s: &str) -> bool {
        !s.is_empty()
            && s.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            && !s.starts_with('_')
            && !s.ends_with('_')
            && !s.contains("__")
    }

    #[test]
    fn codes_are_unique_snake_case() {
        let mut seen = HashSet::new();
        for code in ErrorCode::ALL {
            assert!(is_snake_case(code.as_str()), "{code} is not snake_case");
            assert!(seen.insert(code.as_str()), "{code} is declared twice");
        }
    }

    #[test]
    fn exit_codes_follow_contract_1_2() {
        // The examples of contract §1.2's table, and commands.md §1.3's common refusals.
        let cases = [
            (ErrorCode::GitFailed, 1),
            (ErrorCode::TmuxFailed, 1),
            (ErrorCode::GhFailed, 1),
            (ErrorCode::GateError, 1),
            (ErrorCode::StoreTampered, 1),
            (ErrorCode::ConfigInvalid, 1),
            (ErrorCode::ConfigMissing, 1),
            (ErrorCode::StoreTooNew, 1),
            (ErrorCode::Internal, 1),
            (ErrorCode::MigrationFailed, 1),
            (ErrorCode::MigrationChanged, 1),
            (ErrorCode::MigrationOrder, 1),
            (ErrorCode::Usage, 2),
            (ErrorCode::PlanRequired, 2),
            (ErrorCode::ContextRequired, 2),
            (ErrorCode::SessionRequired, 2),
            (ErrorCode::InvalidPlanId, 2),
            (ErrorCode::InvalidRecordId, 2),
            (ErrorCode::JobLocked, 3),
            (ErrorCode::DraftIncomplete, 3),
            (ErrorCode::IllegalTransition, 3),
            (ErrorCode::DirtyTree, 3),
            (ErrorCode::GateFailed, 3),
            (ErrorCode::CasLost, 3),
            (ErrorCode::LockedJobTampered, 3),
            (ErrorCode::RecordNotFound, 3),
            (ErrorCode::PlanNotFound, 4),
            (ErrorCode::ItemNotFound, 4),
            (ErrorCode::ItemClosed, 3),
            (ErrorCode::MergeNotClean, 3),
            (ErrorCode::JobNotFound, 4),
            (ErrorCode::SessionNotFound, 4),
            (ErrorCode::BaseNotFound, 4),
            (ErrorCode::StoreBusy, 5),
            (ErrorCode::LockTimeout, 5),
            (ErrorCode::ToolTimeout, 5),
            (ErrorCode::AskTimeout, 5),
            (ErrorCode::CancelTimeout, 5),
            (ErrorCode::PrStartFailed, 1),
            (ErrorCode::PrRequired, 2),
            (ErrorCode::InvalidPrId, 2),
            (ErrorCode::PrExists, 3),
            (ErrorCode::PrInUse, 3),
            (ErrorCode::PrEmpty, 3),
            (ErrorCode::PrLocked, 3),
            (ErrorCode::PrNotReady, 3),
            (ErrorCode::PrEnded, 3),
            (ErrorCode::PrClosed, 3),
            (ErrorCode::NeedsOtherPr, 3),
            (ErrorCode::PrNotFound, 4),
            (ErrorCode::PolicyInvalid, 1),
            (ErrorCode::OwnerUnknown, 1),
            (ErrorCode::RepoUnkeyed, 1),
            (ErrorCode::OwnerMismatch, 3),
            (ErrorCode::OwnerUnset, 3),
            (ErrorCode::OwnerSet, 3),
        ];
        for (code, exit) in cases {
            assert_eq!(code.exit().code(), exit, "{code}");
        }
    }

    #[test]
    fn no_error_exits_zero() {
        for code in ErrorCode::ALL {
            assert_ne!(code.exit(), Exit::Ok, "{code}");
        }
    }

    #[test]
    fn failure_envelope_has_code_message_details() {
        let err = CpdError::new(ErrorCode::CasLost, "The Attempt changed since it was read.")
            .with("attempt", "api-schema-a1");
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(
            json,
            r#"{"ok":false,"error":{"code":"cas_lost","message":"The Attempt changed since it was read.","details":{"attempt":"api-schema-a1"}}}"#
        );
    }

    #[test]
    fn details_default_to_an_empty_object() {
        let value = serde_json::to_value(CpdError::new(ErrorCode::Usage, "Bad flag.")).unwrap();
        assert_eq!(value["error"]["details"], serde_json::json!({}));
    }
}
