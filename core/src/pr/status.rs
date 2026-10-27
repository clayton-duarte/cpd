//! The PR status enums (contract §7.6): what `pr poll` stores and prints for a
//! pull request's checks, review and state, and how GitHub's values map to them.

use std::collections::BTreeMap;

use crate::error::{CpdError, ErrorCode};

/// Check conclusions that count as failing. CANCELLED is neither failing nor pending (a cancelled run is no failure).
const FAILING: [&str; 5] = ["FAILURE", "TIMED_OUT", "ACTION_REQUIRED", "STARTUP_FAILURE", "ERROR"];

pub(super) fn failing(conclusion: &str) -> bool {
    FAILING.contains(&conclusion.to_ascii_uppercase().as_str())
}

/// Check conclusions that are not final yet (upper-cased; an empty one is pending too).
const PENDING: [&str; 5] = ["", "PENDING", "IN_PROGRESS", "QUEUED", "EXPECTED"];

/// The summary of a PR's current checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChecksSummary {
    None,
    Passed,
    Pending,
    Failed,
}

impl ChecksSummary {
    /// Any failure wins over pending, pending over passed; no checks is `none`.
    pub fn of(checks: &BTreeMap<String, String>) -> ChecksSummary {
        let upper: Vec<String> = checks.values().map(|c| c.to_ascii_uppercase()).collect();
        if upper.is_empty() {
            ChecksSummary::None
        } else if upper.iter().any(|c| failing(c)) {
            ChecksSummary::Failed
        } else if upper.iter().any(|c| PENDING.contains(&c.as_str())) {
            ChecksSummary::Pending
        } else {
            ChecksSummary::Passed
        }
    }

    /// From GraphQL's `statusCheckRollup.state` (`StatusState`); `None` when the
    /// commit or its rollup is null. `FAILURE` and `ERROR` are `failed` (it can
    /// show while other checks still run: accepted); an unknown state is `none`.
    pub fn of_rollup(state: Option<&str>) -> ChecksSummary {
        match state {
            Some("SUCCESS") => ChecksSummary::Passed,
            Some("FAILURE" | "ERROR") => ChecksSummary::Failed,
            Some("PENDING" | "EXPECTED") => ChecksSummary::Pending,
            _ => ChecksSummary::None,
        }
    }

    /// As stored and printed.
    pub fn as_str(self) -> &'static str {
        match self {
            ChecksSummary::None => "none",
            ChecksSummary::Passed => "passed",
            ChecksSummary::Pending => "pending",
            ChecksSummary::Failed => "failed",
        }
    }
}

/// The PR's review decision, from gh's `reviewDecision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReviewDecision {
    None,
    Approved,
    ChangesRequested,
}

impl ReviewDecision {
    /// `APPROVED` and `CHANGES_REQUESTED` are themselves; anything else (`REVIEW_REQUIRED`, empty, absent) is `none`.
    pub fn of(gh: Option<&str>) -> ReviewDecision {
        match gh {
            Some("APPROVED") => ReviewDecision::Approved,
            Some("CHANGES_REQUESTED") => ReviewDecision::ChangesRequested,
            _ => ReviewDecision::None,
        }
    }

    /// As stored and printed.
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewDecision::None => "none",
            ReviewDecision::Approved => "approved",
            ReviewDecision::ChangesRequested => "changes_requested",
        }
    }
}

/// The PR's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PrState {
    Open,
    Closed,
    Merged,
}

impl PrState {
    /// From GraphQL's `PullRequestState`; any other value is `gh_failed`.
    pub fn of_graphql(state: &str) -> Result<PrState, CpdError> {
        match state {
            "OPEN" => Ok(PrState::Open),
            "CLOSED" => Ok(PrState::Closed),
            "MERGED" => Ok(PrState::Merged),
            other => Err(CpdError::new(ErrorCode::GhFailed, format!("gh printed the unknown pull request state {other:?}."))
                .with("state", other)),
        }
    }

    /// As stored and printed.
    pub fn as_str(self) -> &'static str {
        match self {
            PrState::Open => "open",
            PrState::Closed => "closed",
            PrState::Merged => "merged",
        }
    }
}

/// GitHub's `PullRequest.mergeable`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mergeable {
    Mergeable,
    Conflicting,
    Unknown,
}

impl Mergeable {
    /// From GraphQL's `MergeableState`; any other value is `gh_failed`.
    pub fn of_graphql(state: &str) -> Result<Mergeable, CpdError> {
        match state {
            "MERGEABLE" => Ok(Mergeable::Mergeable),
            "CONFLICTING" => Ok(Mergeable::Conflicting),
            "UNKNOWN" => Ok(Mergeable::Unknown),
            other => Err(CpdError::new(ErrorCode::GhFailed, format!("gh printed the unknown mergeable state {other:?}."))
                .with("mergeable", other)),
        }
    }

    /// As stored and printed.
    pub fn as_str(self) -> &'static str {
        match self {
            Mergeable::Mergeable => "mergeable",
            Mergeable::Conflicting => "conflicting",
            Mergeable::Unknown => "unknown",
        }
    }
}

/// GitHub's `PullRequest.mergeStateStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MergeState {
    Behind,
    Blocked,
    Clean,
    Dirty,
    Draft,
    HasHooks,
    Unknown,
    Unstable,
}

impl MergeState {
    /// Every value, in the order of the table's `CHECK`.
    pub const ALL: [MergeState; 8] = [
        MergeState::Behind,
        MergeState::Blocked,
        MergeState::Clean,
        MergeState::Dirty,
        MergeState::Draft,
        MergeState::HasHooks,
        MergeState::Unknown,
        MergeState::Unstable,
    ];

    /// From GraphQL's `MergeStateStatus`; any other value is `gh_failed`.
    pub fn of_graphql(state: &str) -> Result<MergeState, CpdError> {
        MergeState::ALL.into_iter().find(|s| s.as_str().to_ascii_uppercase() == state).ok_or_else(|| {
            CpdError::new(ErrorCode::GhFailed, format!("gh printed the unknown merge state status {state:?}."))
                .with("merge_state", state)
        })
    }

    /// As stored and printed.
    pub fn as_str(self) -> &'static str {
        match self {
            MergeState::Behind => "behind",
            MergeState::Blocked => "blocked",
            MergeState::Clean => "clean",
            MergeState::Dirty => "dirty",
            MergeState::Draft => "draft",
            MergeState::HasHooks => "has_hooks",
            MergeState::Unknown => "unknown",
            MergeState::Unstable => "unstable",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rollup_maps_to_checks() {
        for (graphql, checks) in [
            (Some("SUCCESS"), ChecksSummary::Passed),
            (Some("FAILURE"), ChecksSummary::Failed),
            (Some("ERROR"), ChecksSummary::Failed),
            (Some("PENDING"), ChecksSummary::Pending),
            (Some("EXPECTED"), ChecksSummary::Pending),
            (None, ChecksSummary::None),
        ] {
            assert_eq!(ChecksSummary::of_rollup(graphql), checks, "{graphql:?}");
        }
    }

    #[test]
    fn only_approved_and_changes_requested_are_decisions() {
        assert_eq!(ReviewDecision::of(Some("APPROVED")), ReviewDecision::Approved);
        assert_eq!(ReviewDecision::of(Some("CHANGES_REQUESTED")), ReviewDecision::ChangesRequested);
        for other in [Some("REVIEW_REQUIRED"), Some(""), None] {
            assert_eq!(ReviewDecision::of(other), ReviewDecision::None, "{other:?}");
        }
    }

    #[test]
    fn the_state_maps_and_an_unknown_one_is_gh_failed() {
        for (graphql, state, text) in
            [("OPEN", PrState::Open, "open"), ("CLOSED", PrState::Closed, "closed"), ("MERGED", PrState::Merged, "merged")]
        {
            assert_eq!(PrState::of_graphql(graphql).unwrap(), state);
            assert_eq!(state.as_str(), text);
        }
        assert_eq!(PrState::of_graphql("DRAFT").unwrap_err().code(), ErrorCode::GhFailed);
    }

    #[test]
    fn mergeable_maps_and_an_unknown_value_is_gh_failed() {
        for (graphql, mergeable, text) in [
            ("MERGEABLE", Mergeable::Mergeable, "mergeable"),
            ("CONFLICTING", Mergeable::Conflicting, "conflicting"),
            ("UNKNOWN", Mergeable::Unknown, "unknown"),
        ] {
            assert_eq!(Mergeable::of_graphql(graphql).unwrap(), mergeable);
            assert_eq!(mergeable.as_str(), text);
        }
        for bad in ["", "mergeable", "DIRTY"] {
            assert_eq!(Mergeable::of_graphql(bad).unwrap_err().code(), ErrorCode::GhFailed, "{bad:?}");
        }
    }

    #[test]
    fn the_merge_state_maps_and_an_unknown_value_is_gh_failed() {
        for graphql in ["BEHIND", "BLOCKED", "CLEAN", "DIRTY", "DRAFT", "HAS_HOOKS", "UNKNOWN", "UNSTABLE"] {
            let state = MergeState::of_graphql(graphql).unwrap();
            assert_eq!(state.as_str(), graphql.to_ascii_lowercase());
        }
        for bad in ["", "dirty", "MERGEABLE", "QUEUED"] {
            assert_eq!(MergeState::of_graphql(bad).unwrap_err().code(), ErrorCode::GhFailed, "{bad:?}");
        }
    }

    #[test]
    fn the_names_are_as_stored() {
        let checks = [ChecksSummary::None, ChecksSummary::Passed, ChecksSummary::Pending, ChecksSummary::Failed];
        assert_eq!(checks.map(ChecksSummary::as_str), ["none", "passed", "pending", "failed"]);
        let reviews = [ReviewDecision::None, ReviewDecision::Approved, ReviewDecision::ChangesRequested];
        assert_eq!(reviews.map(ReviewDecision::as_str), ["none", "approved", "changes_requested"]);
    }
}
