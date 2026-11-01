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
    SlackFailed = "slack_failed" => Failure,
    SlackRefused = "slack_refused" => Failure,
    SlackUnparseable = "slack_unparseable" => Failure,
    SlackHelperFailed = "slack_helper_failed" => Failure,
    JiraFailed = "jira_failed" => Failure,
    JiraRefused = "jira_refused" => Failure,
    JiraHelperFailed = "jira_helper_failed" => Failure,
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
    TitleFailed = "title_failed" => Failure,
    // Model discovery (contract §3.7 E15, commands.md §14.5).
    ModelsKeyUnset = "models_key_unset" => Failure,
    ModelsUnauthorized = "models_unauthorized" => Failure,
    ModelsUnreachable = "models_unreachable" => Failure,
    ModelsTls = "models_tls" => Failure,
    ModelsTimeout = "models_timeout" => Failure,
    ModelsUnparseable = "models_unparseable" => Failure,
    ModelsEmpty = "models_empty" => Failure,
    ModelsRefreshFailed = "models_refresh_failed" => Failure,
    ModelsNoProvider = "models_no_provider" => Failure,
    ModelsHelperFailed = "models_helper_failed" => Failure,
    InstallStateInvalid = "install_state_invalid" => Failure,
    // The install (commands.md §14.4).
    InstallFailed = "install_failed" => Failure,
    // The holographic engine rail (contract §4.6, hs-d677).
    EngineFailed = "engine_failed" => Failure,
    // A `durable` Worker's transcript has not grown for `timeouts.stall_minutes` (contract §3.4 A4, hs-d798):
    // the code of the driver's informational `error` Event. Never a command's failure (like `cleanup_failed`).
    WorkerStalled = "worker_stalled" => Failure,
    // Policies (contract §8.3, hs-d288).
    PolicyInvalid = "policy_invalid" => Failure,
    OwnerUnconfirmed = "owner_unconfirmed" => Failure,
    FallbackMissing = "fallback_missing" => Failure,
    RepoUnkeyed = "repo_unkeyed" => Failure,
    // Secrets (contract §8.4).
    SecretOpMissing = "secret_op_missing" => Failure,
    SecretOpFailed = "secret_op_failed" => Failure,

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
    OutlineInvalid = "outline_invalid" => Usage,
    TierInvalid = "tier_invalid" => Usage,
    TierNotAllowed = "tier_not_allowed" => Usage,
    NothingToEdit = "nothing_to_edit" => Usage,

    // Exit 3: refused by an invariant.
    CasLost = "cas_lost" => Refused,
    PlanInactive = "plan_inactive" => Refused,
    SessionRestartLoop = "session_restart_loop" => Refused,
    Broadcast = "broadcast" => Refused,
    Duplicate = "duplicate" => Refused,
    NoManager = "no_manager" => Refused,
    ManagerOnly = "manager_only" => Refused,
    UntilPrNotIncident = "until_pr_not_incident" => Refused,
    IncidentHold = "incident_hold" => Refused,
    UserOnly = "user_only" => Refused,
    CurationNotPending = "curation_not_pending" => Refused,
    ManagerMissing = "manager_missing" => Refused,
    ArgsInvalid = "args_invalid" => Usage,
    NotLoopGuarded = "not_loop_guarded" => Refused,
    DistillRunning = "distill_running" => Refused,
    WatchNotStarted = "watch_not_started" => Refused,
    IllegalTransition = "illegal_transition" => Refused,
    LockedJobTampered = "locked_job_tampered" => Refused,
    RecordNotFound = "record_not_found" => Refused,
    SupersedesInvalid = "supersedes_invalid" => Refused,
    ContextExists = "context_exists" => Refused,
    QuestionAnswered = "question_answered" => Refused,
    NotAGitRepo = "not_a_git_repo" => Refused,
    PlanExists = "plan_exists" => Refused,
    BranchExists = "branch_exists" => Refused,
    BasePrUnknown = "base_pr_unknown" => Refused,
    BasePrUnavailable = "base_pr_unavailable" => Refused,
    BasePrOtherRepo = "base_pr_other_repo" => Refused,
    PrOtherPlan = "pr_other_plan" => Refused,
    IllegalPhase = "illegal_phase" => Refused,
    ExplorePrForbidden = "explore_pr_forbidden" => Refused,
    ExploreNoDraft = "explore_no_draft" => Refused,
    ExploreNeedsMixed = "explore_needs_mixed" => Refused,
    ArtefactMissing = "artefact_missing" => Refused,
    SummaryMissing = "summary_missing" => Refused,
    SummaryTooLong = "summary_too_long" => Refused,
    NoDraftJobs = "no_draft_jobs" => Refused,
    NoFailedRun = "no_failed_run" => Refused,
    ReviewDecided = "review_decided" => Refused,
    PlanNotApproved = "plan_not_approved" => Refused,
    DependencyCycle = "dependency_cycle" => Refused,
    WorktreeExists = "worktree_exists" => Refused,
    JobKeyExists = "job_key_exists" => Refused,
    TouchesMissing = "touches_missing" => Refused,
    DraftIncomplete = "draft_incomplete" => Refused,
    OutlineMissing = "outline_missing" => Refused,
    OutlineIncomplete = "outline_incomplete" => Refused,
    NeedsUnknown = "needs_unknown" => Refused,
    NeedsUnavailable = "needs_unavailable" => Refused,
    AttemptRunning = "attempt_running" => Refused,
    NeedsRequired = "needs_required" => Refused,
    JobLocked = "job_locked" => Refused,
    ResourceUndeclared = "resource_undeclared" => Refused,
    ResourceBusy = "resource_busy" => Refused,
    JobMerged = "job_merged" => Refused,
    JobNotFailed = "job_not_failed" => Refused,
    JobNotReady = "job_not_ready" => Refused,
    NoSlot = "no_slot" => Refused,
    AttemptNotRunning = "attempt_not_running" => Refused,
    WrongWorktree = "wrong_worktree" => Refused,
    DirtyTree = "dirty_tree" => Refused,
    NoCommits = "no_commits" => Refused,
    GateTampered = "gate_tampered" => Refused,
    NotAwaitingReview = "not_awaiting_review" => Refused,
    EscalationAnswered = "escalation_answered" => Refused,
    EscalationExpired = "escalation_expired" => Refused,
    JobsNotDone = "jobs_not_done" => Refused,
    DirtyTarget = "dirty_target" => Refused,
    GateFailed = "gate_failed" => Refused,
    RebaseConflict = "rebase_conflict" => Refused,
    RebaseInProgress = "rebase_in_progress" => Refused,
    NoRebaseInProgress = "no_rebase_in_progress" => Refused,
    LeadWorktreeDirty = "lead_worktree_dirty" => Refused,
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
    DraftParked = "draft_parked" => Refused,
    Archived = "archived" => Refused,
    ItemClosed = "item_closed" => Refused,
    NotClaimable = "not_claimable" => Refused,
    NotApprovable = "not_approvable" => Refused,
    ReviewContextUnset = "review_context_unset" => Refused,
    PostNotFound = "post_not_found" => Refused,
    MergeNotClean = "merge_not_clean" => Refused,
    RequiredCheckNotPassed = "required_check_not_passed" => Refused,
    NoMergeMethod = "no_merge_method" => Refused,
    OwnerMismatch = "owner_mismatch" => Refused,
    OwnerUnset = "owner_unset" => Refused,
    SecretSessionRequired = "secret_session_required" => Refused,
    SecretRoleDenied = "secret_role_denied" => Refused,
    SecretVaultDenied = "secret_vault_denied" => Refused,
    SecretGrantRequired = "secret_grant_required" => Refused,
    SecretAttemptEnded = "secret_attempt_ended" => Refused,
    SecretExists = "secret_exists" => Refused,
    SecretNotSignedIn = "secret_not_signed_in" => Refused,
    OwnerInUse = "owner_in_use" => Refused,
    HostApprovalRequired = "host_approval_required" => Refused,
    NotAManager = "not_a_manager" => Refused,
    EngineDisabled = "engine_disabled" => Refused,
    // The tier-preset switch (commands.md §14.6): the registry refused the preset, before any write.
    PresetProviderUnconfigured = "preset_provider_unconfigured" => Refused,
    PresetModelUnoffered = "preset_model_unoffered" => Refused,

    // Exit 4: not found.
    ContextNotFound = "context_not_found" => NotFound,
    PlanNotFound = "plan_not_found" => NotFound,
    JobNotFound = "job_not_found" => NotFound,
    AttemptNotFound = "attempt_not_found" => NotFound,
    EscalationNotFound = "escalation_not_found" => NotFound,
    ReviewNotFound = "review_not_found" => NotFound,
    TopicNotFound = "topic_not_found" => NotFound,
    SessionNotFound = "session_not_found" => NotFound,
    McpUnknown = "mcp_unknown" => NotFound,
    SecretNotFound = "secret_not_found" => NotFound,
    BaseNotFound = "base_not_found" => NotFound,
    ItemNotFound = "item_not_found" => NotFound,
    EngineMissing = "engine_missing" => NotFound,
    PresetUnknown = "preset_unknown" => NotFound,

    // Exit 5: timeout.
    StoreBusy = "store_busy" => Timeout,
    LockTimeout = "lock_timeout" => Timeout,
    ToolTimeout = "tool_timeout" => Timeout,
    HostTimeout = "host_timeout" => Timeout,
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
    PrTitleRequired = "pr_title_required" => Refused,
    DescriptionRequired = "description_required" => Refused,
    JiraRequired = "jira_required" => Refused,
    DescriptionInvalid = "description_invalid" => Refused,
    AdoptNotOpen = "adopt_not_open" => Refused,
    AdoptFork = "adopt_fork" => Refused,
    AdoptCpdBranch = "adopt_cpd_branch" => Refused,
    AdoptTaken = "adopt_taken" => Refused,
    AdoptForeign = "adopt_foreign" => Refused,
    PrAdopted = "pr_adopted" => Refused,
    PrNotClosed = "pr_not_closed" => Refused,
    PrNotOpen = "pr_not_open" => Refused,
    AlreadyPosted = "already_posted" => Refused,
    DurableSession = "durable_session" => Refused,
    HostRunning = "host_running" => Refused,
    // Only an `error` Event's code (engine E18, pro-d117), never a command's failure.
    AutoMergeRefused = "auto_merge_refused" => Refused,
    PostDisabled = "post_disabled" => Refused,
    NotAuthored = "not_authored" => Refused,
    PostUnconfigured = "post_unconfigured" => Refused,
    SlackAuth = "slack_auth" => Refused,
    SlackNotConfigured = "slack_not_configured" => Refused,
    JiraAuth = "jira_auth" => Refused,
    JiraNotConfigured = "jira_not_configured" => Refused,
    FeedbackNotFound = "feedback_not_found" => Refused,
    BaseConflict = "base_conflict" => Refused,
    NeedsOtherPr = "needs_other_pr" => Refused,
    KindReserved = "kind_reserved" => Refused,
    TestJobRequired = "test_job_required" => Refused,
    ImplJobRequired = "impl_job_required" => Refused,
    ShapeViolation = "shape_violation" => Refused,
    ShapeIncomplete = "shape_incomplete" => Refused,
    TestsUnconfigured = "tests_unconfigured" => Refused,
    TestFilesMissing = "test_files_missing" => Refused,
    TestsBuildFailed = "tests_build_failed" => Refused,
    TestsNotRed = "tests_not_red" => Refused,
    TestsFailed = "tests_failed" => Refused,
    TestsNotFound = "tests_not_found" => Refused,
    TestsError = "tests_error" => Failure,
    TestsTampered = "tests_tampered" => Refused,
    NotAnImplJob = "not_an_impl_job" => Refused,
    PathNotGuarded = "path_not_guarded" => Refused,
    OpenJobLive = "open_job_live" => Refused,
    LeadBranchMoved = "lead_branch_moved" => Refused,
    HeadMoved = "head_moved" => Refused,
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

    /// `plan_not_found` (exit 4): no Plan `plan`.
    pub fn plan_not_found(plan: &crate::ids::PlanId) -> Self {
        CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {plan}.")).with("plan", plan.as_str())
    }

    /// A row the database holds in a shape the binary does not accept: it was
    /// changed outside cpd, or by a bug (`store_failed`).
    pub fn bad_row(what: impl fmt::Display) -> Self {
        CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: {what}."))
    }

    /// `file_unreadable` (exit 2): `path` cannot be read as UTF-8 text because of `cause`.
    pub fn file_unreadable(path: &std::path::Path, cause: impl fmt::Display) -> Self {
        CpdError::new(ErrorCode::FileUnreadable, format!("Cannot read {} as UTF-8 text: {cause}.", path.display()))
            .with("path", path.display().to_string())
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
            (ErrorCode::UntilPrNotIncident, 3),
            (ErrorCode::JobLocked, 3),
            (ErrorCode::ExplorePrForbidden, 3),
            (ErrorCode::ExploreNoDraft, 3),
            (ErrorCode::ExploreNeedsMixed, 3),
            (ErrorCode::ArtefactMissing, 3),
            (ErrorCode::SummaryMissing, 3),
            (ErrorCode::SummaryTooLong, 3),
            (ErrorCode::DraftIncomplete, 3),
            (ErrorCode::IllegalTransition, 3),
            (ErrorCode::DirtyTree, 3),
            (ErrorCode::GateFailed, 3),
            (ErrorCode::CasLost, 3),
            (ErrorCode::LockedJobTampered, 3),
            (ErrorCode::RecordNotFound, 3),
            (ErrorCode::SupersedesInvalid, 3),
            (ErrorCode::PlanNotFound, 4),
            (ErrorCode::TopicNotFound, 4),
            (ErrorCode::ItemNotFound, 4),
            (ErrorCode::ItemClosed, 3),
            (ErrorCode::NotClaimable, 3),
            (ErrorCode::NotApprovable, 3),
            (ErrorCode::ReviewContextUnset, 3),
            (ErrorCode::PostNotFound, 3),
            (ErrorCode::MergeNotClean, 3),
            (ErrorCode::RequiredCheckNotPassed, 3),
            (ErrorCode::NoMergeMethod, 3),
            (ErrorCode::JobNotFound, 4),
            (ErrorCode::SessionNotFound, 4),
            (ErrorCode::McpUnknown, 4),
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
            (ErrorCode::BasePrUnknown, 3),
            (ErrorCode::BasePrUnavailable, 3),
            (ErrorCode::BasePrOtherRepo, 3),
            (ErrorCode::PrOtherPlan, 3),
            (ErrorCode::PrEmpty, 3),
            (ErrorCode::PrLocked, 3),
            (ErrorCode::PrNotReady, 3),
            (ErrorCode::PrEnded, 3),
            (ErrorCode::PrClosed, 3),
            (ErrorCode::PrTitleRequired, 3),
            (ErrorCode::DescriptionRequired, 3),
            (ErrorCode::JiraRequired, 3),
            (ErrorCode::DescriptionInvalid, 3),
            (ErrorCode::AdoptNotOpen, 3),
            (ErrorCode::AdoptFork, 3),
            (ErrorCode::AdoptCpdBranch, 3),
            (ErrorCode::AdoptTaken, 3),
            (ErrorCode::AdoptForeign, 3),
            (ErrorCode::PrAdopted, 3),
            (ErrorCode::PrNotClosed, 3),
            (ErrorCode::PrNotOpen, 3),
            (ErrorCode::NoFailedRun, 3),
            (ErrorCode::AlreadyPosted, 3),
            (ErrorCode::DurableSession, 3),
            (ErrorCode::HostRunning, 3),
            (ErrorCode::AutoMergeRefused, 3),
            (ErrorCode::PostDisabled, 3),
            (ErrorCode::NotAuthored, 3),
            (ErrorCode::PostUnconfigured, 3),
            (ErrorCode::SlackAuth, 3),
            (ErrorCode::SlackNotConfigured, 3),
            (ErrorCode::SlackFailed, 1),
            (ErrorCode::SlackRefused, 1),
            (ErrorCode::SlackUnparseable, 1),
            (ErrorCode::SlackHelperFailed, 1),
            (ErrorCode::JiraFailed, 1),
            (ErrorCode::JiraRefused, 1),
            (ErrorCode::JiraHelperFailed, 1),
            (ErrorCode::JiraAuth, 3),
            (ErrorCode::JiraNotConfigured, 3),
            (ErrorCode::FeedbackNotFound, 3),
            (ErrorCode::BaseConflict, 3),
            (ErrorCode::NeedsOtherPr, 3),
            (ErrorCode::KindReserved, 3),
            (ErrorCode::TestJobRequired, 3),
            (ErrorCode::ImplJobRequired, 3),
            (ErrorCode::ShapeViolation, 3),
            (ErrorCode::ShapeIncomplete, 3),
            (ErrorCode::TestsUnconfigured, 3),
            (ErrorCode::TestFilesMissing, 3),
            (ErrorCode::ResourceUndeclared, 3),
            (ErrorCode::ResourceBusy, 3),
            (ErrorCode::TestsBuildFailed, 3),
            (ErrorCode::TestsNotRed, 3),
            (ErrorCode::TestsFailed, 3),
            (ErrorCode::TestsNotFound, 3),
            (ErrorCode::TestsError, 1),
            (ErrorCode::TestsTampered, 3),
            (ErrorCode::NotAnImplJob, 3),
            (ErrorCode::PathNotGuarded, 3),
            (ErrorCode::OpenJobLive, 3),
            (ErrorCode::LeadBranchMoved, 3),
            (ErrorCode::PrNotFound, 4),
            (ErrorCode::TitleFailed, 1),
            (ErrorCode::WorkerStalled, 1),
            (ErrorCode::PolicyInvalid, 1),
            (ErrorCode::OwnerUnconfirmed, 1),
            (ErrorCode::FallbackMissing, 1),
            (ErrorCode::RepoUnkeyed, 1),
            (ErrorCode::OwnerMismatch, 3),
            (ErrorCode::OwnerUnset, 3),
            (ErrorCode::OwnerInUse, 3),
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

    /// commands-G2: the constructors every area used to copy.
    #[test]
    fn the_shared_constructors_build_the_documented_errors() {
        let plan = crate::ids::PlanId::parse("api-schema").unwrap();
        assert_eq!(
            serde_json::to_value(CpdError::plan_not_found(&plan)).unwrap(),
            serde_json::json!({"ok": false, "error": {"code": "plan_not_found", "message": "No Plan api-schema.", "details": {"plan": "api-schema"}}})
        );
        let bad = CpdError::bad_row("Plan x has phase \"y\"");
        assert_eq!((bad.code(), bad.message()), (ErrorCode::StoreFailed, "The database holds an invalid row: Plan x has phase \"y\"."));
        assert!(bad.details().is_empty());
        let unreadable = CpdError::file_unreadable(std::path::Path::new("/tmp/brief.md"), "not UTF-8");
        assert_eq!((unreadable.code(), unreadable.message()), (ErrorCode::FileUnreadable, "Cannot read /tmp/brief.md as UTF-8 text: not UTF-8."));
        assert_eq!(unreadable.details()["path"], "/tmp/brief.md");
    }
}
