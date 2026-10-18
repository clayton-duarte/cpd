//! What `pr poll` tells a Plan's Lead (contract §5.2 `pr-feedback`, D7): the
//! text and the data of the Event, from the reasons `pulls::attention` found.
//! Pure: no I/O.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::gh::View;
use super::pulls::{PullRequest, Reason};

/// The PR's current checks: name (a CheckRun's `name`, a StatusContext's `context`) → conclusion (else state).
pub(super) fn current_checks(view: &View) -> BTreeMap<String, String> {
    let mut checks = BTreeMap::new();
    for c in view.checks.iter().flatten() {
        let Some(name) = c.name.clone().or_else(|| c.context.clone()) else { continue };
        let conclusion = c.conclusion.clone().filter(|s| !s.is_empty()).or_else(|| c.state.clone()).unwrap_or_default();
        checks.insert(name, conclusion);
    }
    checks
}

fn phrase(reason: Reason) -> &'static str {
    match reason {
        Reason::ChecksFailed => "checks failed",
        Reason::ChangesRequested => "changes requested",
        Reason::Closed => "closed without a merge",
        Reason::Conflict => "merge conflict",
    }
}

/// The Event's `text`, written for the Lead (§5.1).
pub(super) fn text(plan: &str, key: &str, number: i64, reasons: &[Reason]) -> String {
    let why: Vec<&str> = reasons.iter().map(|r| phrase(*r)).collect();
    format!(
        "PR {key} (#{number}) of Plan {plan} needs attention: {}. Read the details with cpd pr view --plan {plan} --pr {key}.",
        why.join(", ")
    )
}

/// The Event's `data` (§5.2): `{pr, number, url, state, checks, review_decision, mergeable, merge_state, conflict, reasons}`;
/// `conflict` is true exactly when `conflict` is in `reasons`.
pub(super) fn data(key: &str, pull: &PullRequest, reasons: &[Reason]) -> Value {
    json!({
        "pr": key, "number": pull.number, "url": pull.url, "state": pull.state, "checks": pull.checks,
        "review_decision": pull.review_decision, "mergeable": pull.mergeable, "merge_state": pull.merge_state,
        "conflict": reasons.contains(&Reason::Conflict), "reasons": reasons.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::gh::Check;
    use super::super::status::ChecksSummary;
    use super::*;

    fn view(checks: Vec<(&str, &str)>) -> View {
        View {
            state: "OPEN".into(),
            title: "T".into(),
            url: "u".into(),
            review_decision: None,
            checks: Some(
                checks
                    .into_iter()
                    .map(|(name, conclusion)| Check {
                        name: Some(name.into()),
                        context: None,
                        conclusion: Some(conclusion.into()),
                        state: None,
                        url: None,
                    })
                    .collect(),
            ),
            reviews: Vec::new(),
            comments: Vec::new(),
        }
    }

    #[test]
    fn checks_are_summarised_failed_over_pending_over_passed() {
        let summary = |checks: Vec<(&str, &str)>| ChecksSummary::of(&current_checks(&view(checks)));
        assert_eq!(summary(vec![]), ChecksSummary::None);
        assert_eq!(summary(vec![("a", "success")]), ChecksSummary::Passed);
        assert_eq!(summary(vec![("a", "SUCCESS"), ("b", "")]), ChecksSummary::Pending);
        assert_eq!(summary(vec![("a", "SKIPPED"), ("b", "queued"), ("c", "startup_failure")]), ChecksSummary::Failed);
    }

    #[test]
    fn the_text_names_every_reason_and_the_command() {
        let all = [Reason::ChecksFailed, Reason::ChangesRequested, Reason::Closed, Reason::Conflict];
        assert_eq!(
            text("pay", "api", 7, &all),
            "PR api (#7) of Plan pay needs attention: checks failed, changes requested, closed without a merge, merge conflict. \
             Read the details with cpd pr view --plan pay --pr api."
        );
        assert!(text("pay", "api", 7, &all[..1]).contains("needs attention: checks failed. Read"));
        assert!(text("pay", "api", 7, &all[3..]).contains("needs attention: merge conflict. Read"));
    }

    fn pull(mergeable: &str, merge_state: &str) -> PullRequest {
        PullRequest {
            id: 1,
            repo: "acme/pay".into(),
            number: 7,
            url: "https://github.com/acme/pay/pull/7".into(),
            title: "T".into(),
            draft: false,
            state: "open".into(),
            review_decision: "none".into(),
            checks: "failed".into(),
            created_at: "t".into(),
            merged_at: None,
            changed_at: "t".into(),
            mergeable: mergeable.into(),
            merge_state: merge_state.into(),
        }
    }

    #[test]
    fn the_data_carries_the_mergeable_state_and_a_conflict_flag_true_exactly_for_the_reason() {
        let url = "https://github.com/acme/pay/pull/7";
        assert_eq!(
            data("api", &pull("conflicting", "dirty"), &[Reason::ChecksFailed, Reason::Conflict]),
            json!({"pr": "api", "number": 7, "url": url, "state": "open", "checks": "failed", "review_decision": "none",
                   "mergeable": "conflicting", "merge_state": "dirty", "conflict": true, "reasons": ["checks_failed", "conflict"]})
        );
        // Still conflicting, but told for another reason: the flag follows the reasons.
        let still = data("api", &pull("conflicting", "dirty"), &[Reason::ChecksFailed]);
        assert_eq!((&still["conflict"], &still["mergeable"]), (&json!(false), &json!("conflicting")));
        assert_eq!(data("api", &pull("unknown", "unknown"), &[Reason::Closed])["conflict"], json!(false));
    }
}
