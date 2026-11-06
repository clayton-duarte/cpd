//! What `pr poll` tells a Plan's Lead (contract §5.2 `pr-feedback`, D7): the
//! text and the data of the Event, from the reasons `pulls::attention` found.
//! Pure: no I/O.

use super::pulls::{PullRequest, Reason};
use crate::events::NewEvent;

fn phrase(reason: Reason) -> &'static str {
    match reason {
        Reason::ChecksFailed => "checks failed",
        Reason::ChangesRequested => "changes requested",
        Reason::Comments => "comments",
        Reason::Closed => "closed without a merge",
        Reason::ReadyToPost => "ready to post for review",
        Reason::Approved => "approved",
    }
}

/// The Event's `text`, written for the Lead (§5.1). `comments` is the number of new and of open feedback items: it is
/// stated when `comments` is a reason.
pub(super) fn text(
    plan: &str,
    key: &str,
    number: i64,
    reasons: &[Reason],
    comments: (usize, usize),
) -> String {
    let why: Vec<&str> = reasons.iter().map(|r| phrase(*r)).collect();
    let counts = if reasons.contains(&Reason::Comments) {
        format!(
            " {} new and {} open review comments.",
            comments.0, comments.1
        )
    } else {
        String::new()
    };
    format!(
        "Workflow {key} (#{number}) of Plan {plan} needs attention: {}.{counts} Read the details with cpd pr view --plan {plan} --workflow {key}.",
        why.join(", ")
    )
}

/// The Event (§5.2 `pr-feedback`). Its `conflict` field is always false: a conflict is no reason since hs-d340 (the field stays, the
/// Event's data shape being fixed by a table `core/src/events` and its tests own).
pub(super) fn event(text: String, key: &str, pull: &PullRequest, reasons: &[Reason]) -> NewEvent {
    let reasons: Vec<&str> = reasons.iter().map(|r| r.as_str()).collect();
    NewEvent::pr_feedback(
        text,
        key,
        pull.number,
        &pull.url,
        &pull.state,
        &pull.checks,
        &pull.review_decision,
        &pull.mergeable,
        &pull.merge_state,
        false,
        &reasons,
    )
}

#[cfg(test)]
mod tests {
    use super::super::gh::Check;
    use super::super::status::ChecksSummary;
    use super::*;
    use serde_json::{Value, json};

    fn view(checks: Vec<(&str, &str)>) -> Vec<Check> {
        checks
            .into_iter()
            .map(|(name, conclusion)| Check {
                name: Some(name.into()),
                context: None,
                conclusion: Some(conclusion.into()),
                state: None,
                url: None,
                workflow: None,
                started_at: None,
            })
            .collect()
    }

    #[test]
    fn checks_are_summarised_failed_over_pending_over_passed() {
        let summary = |checks: Vec<(&str, &str)>| {
            ChecksSummary::of(&view(checks), &[], &[])
        };
        assert_eq!(summary(vec![]), ChecksSummary::None);
        assert_eq!(summary(vec![("a", "success")]), ChecksSummary::Passed);
        assert_eq!(
            summary(vec![("a", "SUCCESS"), ("b", "")]),
            ChecksSummary::Pending
        );
        assert_eq!(
            summary(vec![
                ("a", "SKIPPED"),
                ("b", "queued"),
                ("c", "startup_failure")
            ]),
            ChecksSummary::Failed
        );
        assert_eq!(
            summary(vec![("a", "CANCELLED"), ("b", "SUCCESS")]),
            ChecksSummary::Passed
        );
        assert_eq!(
            summary(vec![("a", "CANCELLED"), ("b", "")]),
            ChecksSummary::Pending
        );
    }

    #[test]
    fn the_text_names_every_reason_and_the_command() {
        let all = [
            Reason::ChecksFailed,
            Reason::ChangesRequested,
            Reason::Closed,
        ];
        assert_eq!(
            text("pay", "api", 7, &all, (0, 0)),
            "Workflow api (#7) of Plan pay needs attention: checks failed, changes requested, closed without a merge. \
             Read the details with cpd pr view --plan pay --workflow api."
        );
        assert!(
            text("pay", "api", 7, &all[..1], (0, 0))
                .contains("needs attention: checks failed. Read")
        );
        assert!(
            text("pay", "api", 7, &all[2..], (0, 0))
                .contains("needs attention: closed without a merge. Read")
        );
    }

    /// The `data` of the Event.
    fn data(key: &str, pull: &PullRequest, reasons: &[Reason]) -> Value {
        event("text".into(), key, pull, reasons).data().clone()
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
            authored: true,
            review_requested: false,
            updated_at: None,
            feedback: None,
            base_ref: None,
            default_branch: None,
            archived: false,
        }
    }

    #[test]
    fn the_data_carries_the_mergeable_state_and_a_conflict_flag_that_is_always_false() {
        let url = "https://github.com/acme/pay/pull/7";
        assert_eq!(
            data(
                "api",
                &pull("conflicting", "dirty"),
                &[Reason::ChecksFailed]
            ),
            json!({"workflow": "api", "number": 7, "url": url, "state": "open", "checks": "failed", "review_decision": "none",
                   "mergeable": "conflicting", "merge_state": "dirty", "conflict": false, "reasons": ["checks_failed"]})
        );
        assert_eq!(
            data("api", &pull("unknown", "unknown"), &[Reason::Closed])["conflict"],
            json!(false)
        );
    }
}
