//! The Workflow status enums (contract §7.6): what `pr poll` stores and prints for a
//! pull request's checks, review and state, and how GitHub's values map to them.

use super::gh::Check;
use crate::error::{CpdError, ErrorCode};

/// Check conclusions that count as failing. CANCELLED is neither failing nor pending (a cancelled run is no failure).
const FAILING: [&str; 5] = ["FAILURE", "TIMED_OUT", "ACTION_REQUIRED", "STARTUP_FAILURE", "ERROR"];

/// What one required name is on the head's rollup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Required {
    Missing,
    Pending,
    Failed,
    Passed,
}

/// The state of each name in `required` on `checks`, in order: the same classification of conclusions as [`ChecksSummary::of`];
/// of several runs of one name the newest decides (as [`newest_runs`]), and of several identities named alike failed wins, then pending, then passed.
pub(super) fn required_states<'a>(checks: &[Check], required: &'a [String]) -> Vec<(&'a str, Required)> {
    let newest = newest_runs(checks);
    required
        .iter()
        .map(|name| {
            let states = newest.iter().filter_map(|c| {
                let state = if c.name.is_none() && c.context.as_deref() == Some(name) {
                    let state = c.state.as_deref().unwrap_or_default().to_ascii_uppercase();
                    if failing(&state) { Required::Failed } else if matches!(state.as_str(), "PENDING" | "EXPECTED") { Required::Pending } else { Required::Passed }
                } else if c.name.as_deref() == Some(name) {
                    let conclusion = c.conclusion.clone().filter(|s| !s.is_empty()).or_else(|| c.state.clone()).unwrap_or_default().to_ascii_uppercase();
                    if failing(&conclusion) { Required::Failed } else if PENDING.contains(&conclusion.as_str()) { Required::Pending } else { Required::Passed }
                } else {
                    return None;
                };
                Some(state)
            });
            let state = states.reduce(|a, b| match (a, b) {
                (Required::Failed, _) | (_, Required::Failed) => Required::Failed,
                (Required::Pending, _) | (_, Required::Pending) => Required::Pending,
                _ => Required::Passed,
            });
            (name.as_str(), state.unwrap_or(Required::Missing))
        })
        .collect()
}

pub(super) fn failing(conclusion: &str) -> bool {
    FAILING.contains(&conclusion.to_ascii_uppercase().as_str())
}

/// Check conclusions that are not final yet (upper-cased; an empty one is pending too).
const PENDING: [&str; 5] = ["", "PENDING", "IN_PROGRESS", "QUEUED", "EXPECTED"];

/// The summary of a Workflow's current checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChecksSummary {
    None,
    Passed,
    Pending,
    Failed,
}

/// An entry's verdict word, upper-cased: a CheckRun's conclusion (else its state), a StatusContext's state.
fn verdict(c: &Check) -> String {
    if c.name.is_some() {
        c.conclusion.clone().filter(|s| !s.is_empty()).or_else(|| c.state.clone()).unwrap_or_default().to_ascii_uppercase()
    } else {
        c.state.clone().unwrap_or_default().to_ascii_uppercase()
    }
}

/// The reduction (contract §7.6 step 3b): one entry per check identity, (workflow name, `name`) for a CheckRun and `context` for a
/// StatusContext. CANCELLED and SKIPPED entries carry no verdict and are set aside while another entry of the identity exists; of
/// the others the newest wins (no time is older than any time), the worst verdict on equal times.
fn newest_runs(checks: &[Check]) -> Vec<&Check> {
    let mut groups: Vec<((Option<&str>, &str), Vec<&Check>)> = Vec::new();
    for c in checks {
        let key = match (c.name.as_deref(), c.context.as_deref()) {
            (Some(name), _) => (Some(c.workflow.as_deref().unwrap_or("")), name),
            (None, Some(context)) => (None, context),
            (None, None) => continue,
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(c),
            None => groups.push((key, vec![c])),
        }
    }
    groups
        .into_iter()
        .filter_map(|(_, g)| {
            let rank = |c: &Check| {
                let v = verdict(c);
                if failing(&v) { 2 } else if PENDING.contains(&v.as_str()) || v == "PENDING" { 1 } else { 0 }
            };
            let best = g
                .iter()
                .copied()
                .filter(|c| !matches!(verdict(c).as_str(), "CANCELLED" | "SKIPPED"))
                .max_by_key(|c| (c.started_at.as_deref(), rank(c)));
            best.or_else(|| g.first().copied())
        })
        .collect()
}

impl ChecksSummary {
    /// The one rule (contract §7.6) of the poll and `pr view`: any failure wins; a CheckRun is pending when its conclusion is not
    /// final, a StatusContext when its state is PENDING or EXPECTED; a PENDING StatusContext named in `human_gated` counts as
    /// passed only when a CheckRun exists and none is pending; no checks is `none`. A name in `required` that is missing from `checks` or
    /// not final makes it `pending` (a failure still wins).
    pub fn of(checks: &[Check], human_gated: &[String], required: &[String]) -> ChecksSummary {
        let mut runs = 0;
        let mut run_pending = false;
        let mut failed = false;
        let mut contexts: Vec<(&str, bool)> = Vec::new();
        for c in newest_runs(checks) {
            if let Some(context) = c.name.is_none().then_some(c.context.as_deref()).flatten() {
                let state = c.state.as_deref().unwrap_or_default().to_ascii_uppercase();
                failed |= failing(&state);
                contexts.push((context, matches!(state.as_str(), "PENDING" | "EXPECTED")));
            } else if c.name.is_some() {
                let conclusion = c.conclusion.clone().filter(|s| !s.is_empty()).or_else(|| c.state.clone()).unwrap_or_default().to_ascii_uppercase();
                runs += 1;
                failed |= failing(&conclusion);
                run_pending |= PENDING.contains(&conclusion.as_str());
            }
        }
        let required = required_states(checks, required);
        if failed || required.iter().any(|(_, s)| *s == Required::Failed) {
            return ChecksSummary::Failed;
        }
        if required.iter().any(|(_, s)| matches!(s, Required::Missing | Required::Pending)) {
            return ChecksSummary::Pending;
        }
        if runs + contexts.len() == 0 {
            return ChecksSummary::None;
        }
        let gated_ok = runs > 0 && !run_pending;
        let context_pending = contexts.iter().any(|(name, pending)| *pending && !(gated_ok && human_gated.iter().any(|g| g == name)));
        if run_pending || context_pending { ChecksSummary::Pending } else { ChecksSummary::Passed }
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

/// An APPROVED review's reviewer; `bot_type` is GraphQL's actor type `Bot` (`pr view` has no type: false).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Approval {
    pub login: String,
    pub bot_type: bool,
}

/// A bot reviewer: a login ending `[bot]` or one of the configured `bots`.
pub(super) fn is_bot(login: &str, bots: &[String]) -> bool {
    login.ends_with("[bot]") || bots.iter().any(|b| b == login)
}

/// The Workflow's review decision, from gh's `reviewDecision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReviewDecision {
    None,
    Approved,
    ChangesRequested,
}

impl ReviewDecision {
    /// The one rule (contract §7.6, pro-d158) of the poll and `pr view`: `CHANGES_REQUESTED` is itself; `APPROVED` is `approved`
    /// only when one of the `approvals` is a human's; anything else (`REVIEW_REQUIRED`, empty, absent) is `none`.
    pub fn of_reviews(gh: Option<&str>, approvals: &[Approval], bots: &[String]) -> ReviewDecision {
        match gh {
            Some("APPROVED") if approvals.iter().any(|a| !a.bot_type && !is_bot(&a.login, bots)) => ReviewDecision::Approved,
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

/// The Workflow's state.
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
#[path = "status_required_tests.rs"]
mod status_required_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn run(workflow: Option<&str>, name: &str, conclusion: &str, at: Option<&str>) -> Check {
        Check {
            name: Some(name.into()),
            context: None,
            conclusion: Some(conclusion.into()),
            state: None,
            url: None,
            workflow: workflow.map(Into::into),
            started_at: at.map(Into::into),
        }
    }

    fn status(context: &str, state: &str, at: Option<&str>) -> Check {
        Check { name: None, context: Some(context.into()), conclusion: None, state: Some(state.into()), url: None, workflow: None, started_at: at.map(Into::into) }
    }

    fn summary(checks: &[Check]) -> ChecksSummary {
        ChecksSummary::of(checks, &[], &[])
    }

    const T1: &str = "2026-10-30T01:00:00Z";
    const T2: &str = "2026-10-30T02:00:00Z";

    #[test]
    fn the_newest_run_of_a_check_decides() {
        assert_eq!(summary(&[run(Some("w"), "a", "FAILURE", Some(T1)), run(Some("w"), "a", "SUCCESS", Some(T2))]), ChecksSummary::Passed);
        assert_eq!(summary(&[run(Some("w"), "a", "SUCCESS", Some(T2)), run(Some("w"), "a", "FAILURE", Some(T1))]), ChecksSummary::Passed);
        assert_eq!(summary(&[run(Some("w"), "a", "SUCCESS", Some(T1)), run(Some("w"), "a", "FAILURE", Some(T2))]), ChecksSummary::Failed);
    }

    #[test]
    fn required_check_follows_its_newest_run() {
        let req = vec!["gate".to_owned()];
        let of = |checks: &[Check]| ChecksSummary::of(checks, &[], &req);
        assert_eq!(of(&[run(Some("w"), "gate", "FAILURE", Some(T1)), run(Some("w"), "gate", "SUCCESS", Some(T2))]), ChecksSummary::Passed);
        assert_eq!(of(&[run(Some("w"), "gate", "SUCCESS", Some(T1)), run(Some("w"), "gate", "FAILURE", Some(T2))]), ChecksSummary::Failed);
        assert_eq!(of(&[run(Some("w"), "gate", "SUCCESS", Some(T1)), run(Some("w"), "gate", "IN_PROGRESS", Some(T2))]), ChecksSummary::Pending);
    }

    #[test]
    fn a_newer_pending_run_supersedes_an_older_failure() {
        assert_eq!(summary(&[run(Some("w"), "a", "FAILURE", Some(T1)), run(Some("w"), "a", "IN_PROGRESS", Some(T2))]), ChecksSummary::Pending);
    }

    #[test]
    fn the_workflow_is_part_of_the_identity() {
        assert_eq!(summary(&[run(Some("w1"), "a", "FAILURE", Some(T1)), run(Some("w2"), "a", "SUCCESS", Some(T2))]), ChecksSummary::Failed);
        assert_eq!(summary(&[run(None, "a", "FAILURE", Some(T1)), run(Some("w"), "a", "SUCCESS", Some(T2))]), ChecksSummary::Failed);
        assert_eq!(summary(&[run(None, "a", "FAILURE", Some(T1)), run(None, "a", "SUCCESS", Some(T2))]), ChecksSummary::Passed);
    }

    #[test]
    fn cancelled_and_skipped_runs_supersede_nothing() {
        for set_aside in ["CANCELLED", "SKIPPED"] {
            assert_eq!(summary(&[run(None, "a", "FAILURE", Some(T1)), run(None, "a", set_aside, Some(T2))]), ChecksSummary::Failed, "{set_aside}");
        }
        assert_eq!(summary(&[run(None, "a", "CANCELLED", Some(T2)), run(None, "a", "SUCCESS", Some(T1))]), ChecksSummary::Passed);
    }

    #[test]
    fn only_set_aside_runs_make_a_check_neither_failing_nor_pending() {
        assert_eq!(summary(&[run(None, "a", "CANCELLED", Some(T1)), run(None, "a", "CANCELLED", Some(T2))]), ChecksSummary::Passed);
    }

    #[test]
    fn an_entry_with_no_time_is_older_than_any_with_one() {
        assert_eq!(summary(&[run(None, "a", "FAILURE", None), run(None, "a", "SUCCESS", Some(T1))]), ChecksSummary::Passed);
        assert_eq!(summary(&[run(None, "a", "SUCCESS", None), run(None, "a", "FAILURE", Some(T1))]), ChecksSummary::Failed);
    }

    #[test]
    fn on_equal_times_the_worst_verdict_wins() {
        assert_eq!(summary(&[run(None, "a", "SUCCESS", Some(T1)), run(None, "a", "FAILURE", Some(T1))]), ChecksSummary::Failed);
        assert_eq!(summary(&[run(None, "a", "SUCCESS", Some(T1)), run(None, "a", "IN_PROGRESS", Some(T1))]), ChecksSummary::Pending);
        assert_eq!(summary(&[run(None, "a", "FAILURE", Some(T1)), run(None, "a", "IN_PROGRESS", Some(T1))]), ChecksSummary::Failed);
    }

    #[test]
    fn a_status_context_is_decided_by_its_newest_state() {
        assert_eq!(summary(&[status("d", "FAILURE", Some(T1)), status("d", "SUCCESS", Some(T2))]), ChecksSummary::Passed);
        assert_eq!(summary(&[status("d", "SUCCESS", Some(T1)), status("d", "FAILURE", Some(T2))]), ChecksSummary::Failed);
    }

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

    fn approval(login: &str, bot_type: bool) -> Approval {
        Approval { login: login.to_owned(), bot_type }
    }

    fn of(gh: Option<&str>, approvals: &[Approval], bots: &[&str]) -> ReviewDecision {
        let bots: Vec<String> = bots.iter().map(|b| (*b).to_owned()).collect();
        ReviewDecision::of_reviews(gh, approvals, &bots)
    }

    #[test]
    fn approved_needs_a_human_approval_and_changes_requested_stands() {
        let human = [approval("alice", false)];
        assert_eq!(of(Some("APPROVED"), &human, &[]), ReviewDecision::Approved);
        assert_eq!(of(Some("APPROVED"), &[approval("bot", true)], &[]), ReviewDecision::None);
        assert_eq!(of(Some("APPROVED"), &[], &[]), ReviewDecision::None);
        assert_eq!(of(Some("APPROVED"), &[approval("bot", true), approval("alice", false)], &[]), ReviewDecision::Approved);
        assert_eq!(of(Some("CHANGES_REQUESTED"), &[approval("bot", true)], &[]), ReviewDecision::ChangesRequested);
        for other in [Some("REVIEW_REQUIRED"), Some(""), None] {
            assert_eq!(of(other, &human, &[]), ReviewDecision::None, "{other:?}");
        }
    }

    #[test]
    fn a_bot_is_known_by_its_type_its_suffix_or_the_list_and_a_configured_list_replaces_the_default() {
        assert_eq!(of(Some("APPROVED"), &[approval("x", true)], &[]), ReviewDecision::None);
        assert_eq!(of(Some("APPROVED"), &[approval("x[bot]", false)], &[]), ReviewDecision::None);
        assert_eq!(of(Some("APPROVED"), &[approval("lane", false)], &["lane"]), ReviewDecision::None);
        assert_eq!(of(Some("APPROVED"), &[approval("lane", false)], &["other"]), ReviewDecision::Approved);
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
