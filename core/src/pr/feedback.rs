//! What `pr poll` compares: `pr_seen` (contract §2.2), the new feedback in a
//! poll, and the text of the `pr-feedback` Event (§5.2). Pure: no I/O.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::gh::{InlineComment, View};
use super::status::failing;
// poll.rs (not this Job's to touch) still imports these from here; Job `pr-poll2` rewrites it.
pub(super) use super::status::{ChecksSummary, ReviewDecision};
use crate::error::{CpdError, ErrorCode};

/// Longest body an Event carries, in characters.
const BODY_MAX: usize = 2000;
/// How many changes the Event's `text` lists; `data` always carries all of them.
const LISTED_MAX: usize = 20;
/// Longest excerpt of a body in the Event's `text`.
const EXCERPT_MAX: usize = 300;

/// `plans.pr_seen`: what `pr poll` has already reported.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Seen {
    pub checks: BTreeMap<String, String>,
    #[serde(rename = "reviewIds")]
    pub review_ids: Vec<String>,
    #[serde(rename = "commentIds")]
    pub comment_ids: Vec<String>,
    /// Set when the PR's closure without a merge was reported; removed when it is open again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
}

impl Seen {
    pub fn parse(text: &str) -> Result<Seen, CpdError> {
        serde_json::from_str(text).map_err(|e| {
            CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid pr_seen: {e}."))
                .with("pr_seen", text)
        })
    }

    pub fn to_text(&self) -> String {
        serde_json::to_string(self).expect("pr_seen always serialises")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct CheckChange {
    pub name: String,
    pub conclusion: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct ReviewChange {
    pub id: String,
    pub author: Option<String>,
    pub state: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct CommentChange {
    pub id: String,
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub body: String,
}

/// What is new in a poll.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct Changes {
    pub checks: Vec<CheckChange>,
    pub reviews: Vec<ReviewChange>,
    pub comments: Vec<CommentChange>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.checks.is_empty() && self.reviews.is_empty() && self.comments.is_empty()
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("changes always serialise")
    }
}

fn body(text: &Option<String>) -> String {
    text.as_deref().unwrap_or("").chars().take(BODY_MAX).collect()
}

fn union(old: &[String], new: impl Iterator<Item = String>) -> Vec<String> {
    let mut all = old.to_vec();
    for id in new {
        if !all.contains(&id) {
            all.push(id);
        }
    }
    all
}

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

/// What is new in `view` and `inline` relative to `seen`, and the `pr_seen` to
/// store: a check is new when it fails now and did not before; a review or
/// comment is new when its id was not seen. `closed` is carried over as it was.
pub(super) fn diff(seen: &Seen, view: &View, inline: &[InlineComment]) -> (Changes, Seen) {
    let checks = current_checks(view);
    let mut changes = Changes::default();
    for (name, conclusion) in &checks {
        let was_failing = seen.checks.get(name).is_some_and(|was| failing(was));
        if failing(conclusion) && !was_failing {
            changes.checks.push(CheckChange { name: name.clone(), conclusion: conclusion.clone() });
        }
    }
    for r in view.reviews.iter().filter(|r| !seen.review_ids.contains(&r.id.0)) {
        changes.reviews.push(ReviewChange {
            id: r.id.0.clone(),
            author: r.author.as_ref().map(|a| a.login.clone()),
            state: r.state.clone(),
            body: body(&r.body),
        });
    }
    for c in view.comments.iter().filter(|c| !seen.comment_ids.contains(&c.id.0)) {
        changes.comments.push(CommentChange {
            id: c.id.0.clone(),
            author: c.author.as_ref().map(|a| a.login.clone()),
            path: None,
            body: body(&c.body),
        });
    }
    for c in inline.iter().filter(|c| !seen.comment_ids.contains(&c.id.0)) {
        changes.comments.push(CommentChange {
            id: c.id.0.clone(),
            author: c.user.as_ref().map(|u| u.login.clone()),
            path: c.path.clone(),
            body: body(&c.body),
        });
    }
    let next = Seen {
        checks,
        review_ids: union(&seen.review_ids, view.reviews.iter().map(|r| r.id.0.clone())),
        comment_ids: union(
            &seen.comment_ids,
            view.comments.iter().map(|c| c.id.0.clone()).chain(inline.iter().map(|c| c.id.0.clone())),
        ),
        closed: seen.closed,
    };
    (changes, next)
}

fn excerpt(text: &str) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > EXCERPT_MAX {
        format!("{}…", one.chars().take(EXCERPT_MAX).collect::<String>())
    } else {
        one
    }
}

/// The Event's `text`, written for the Lead (§5.1): one line per change, then
/// the commands it may run.
pub(super) fn text(plan: &str, key: &str, number: i64, changes: &Changes, closed: bool) -> String {
    let mut lines = Vec::new();
    if closed {
        lines.push(format!("PR {key} (#{number}) of Plan {plan} was closed without being merged."));
    }
    if !changes.is_empty() {
        lines.push(format!("PR {key} (#{number}) of Plan {plan} has new feedback:"));
    }
    let who = |a: &Option<String>| a.clone().unwrap_or_else(|| "unknown".to_owned());
    let mut items = Vec::new();
    items.extend(changes.checks.iter().map(|c| format!("check {}: {}", c.name, c.conclusion)));
    items.extend(changes.reviews.iter().map(|r| format!("review {} by {} ({}): {}", r.id, who(&r.author), r.state, excerpt(&r.body))));
    items.extend(changes.comments.iter().map(|c| {
        let at = c.path.as_deref().unwrap_or("(conversation)");
        format!("comment {} by {} at {at}: {}", c.id, who(&c.author), excerpt(&c.body))
    }));
    let more = items.len().saturating_sub(LISTED_MAX);
    lines.extend(items.into_iter().take(LISTED_MAX).map(|i| format!("- {i}")));
    if more > 0 {
        lines.push(format!("- …and {more} more (see data)"));
    }
    lines.push(format!(
        "Turn what needs work into Jobs: cpd job add <key> --title <text> --tier <T> --spec <file> --pr {key} --plan {plan}; then cpd pr rebase --pr {key} --plan {plan} once they are Merged."
    ));
    lines.join("\n")
}

/// The Event's `data` (§5.2).
pub(super) fn data(key: &str, changes: &Changes, state: &str) -> Value {
    let mut data = changes.to_json();
    data["pr"] = json!(key);
    data["state"] = json!(state);
    data
}

#[cfg(test)]
mod tests {
    use super::super::gh::{Check, Comment, Id, Login, Review};
    use super::super::status::ChecksSummary;
    use super::*;

    fn view(checks: Vec<(&str, &str)>, reviews: Vec<&str>, comments: Vec<&str>) -> View {
        View {
            state: "OPEN".into(),
            merged_at: None,
            title: "T".into(),
            review_decision: None,
            checks: Some(
                checks
                    .into_iter()
                    .map(|(name, conclusion)| Check {
                        name: Some(name.into()),
                        context: None,
                        conclusion: Some(conclusion.into()),
                        state: None,
                    })
                    .collect(),
            ),
            reviews: reviews
                .into_iter()
                .map(|id| Review { id: Id(id.into()), author: Some(Login { login: "ann".into() }), state: "COMMENTED".into(), body: Some("x".into()) })
                .collect(),
            comments: comments.into_iter().map(|id| Comment { id: Id(id.into()), author: None, body: Some("y".into()) }).collect(),
        }
    }

    #[test]
    fn a_failing_check_is_reported_once() {
        let v = view(vec![("ci", "FAILURE"), ("lint", "SUCCESS")], vec![], vec![]);
        let (changes, seen) = diff(&Seen::default(), &v, &[]);
        assert_eq!(changes.checks, vec![CheckChange { name: "ci".into(), conclusion: "FAILURE".into() }]);
        let (again, _) = diff(&seen, &v, &[]);
        assert!(again.is_empty());
    }

    #[test]
    fn a_check_that_fails_again_after_passing_is_reported_again() {
        let (_, passed) = diff(&Seen::default(), &view(vec![("ci", "SUCCESS")], vec![], vec![]), &[]);
        let (changes, _) = diff(&passed, &view(vec![("ci", "FAILURE")], vec![], vec![]), &[]);
        assert_eq!(changes.checks.len(), 1);
    }

    #[test]
    fn reviews_and_comments_are_reported_by_id() {
        let (changes, seen) = diff(&Seen::default(), &view(vec![], vec!["R1"], vec!["C1"]), &[]);
        assert_eq!((changes.reviews.len(), changes.comments.len()), (1, 1));
        let (changes, seen) = diff(&seen, &view(vec![], vec!["R1", "R2"], vec!["C1"]), &[]);
        assert_eq!((changes.reviews.len(), changes.comments.len()), (1, 0));
        assert_eq!(seen.review_ids, ["R1", "R2"]);
    }

    #[test]
    fn checks_are_summarised_failed_over_pending_over_passed() {
        let summary = |checks: Vec<(&str, &str)>| ChecksSummary::of(&current_checks(&view(checks, vec![], vec![])));
        assert_eq!(summary(vec![]), ChecksSummary::None);
        assert_eq!(summary(vec![("a", "success")]), ChecksSummary::Passed);
        assert_eq!(summary(vec![("a", "SUCCESS"), ("b", "")]), ChecksSummary::Pending);
        assert_eq!(summary(vec![("a", "SKIPPED"), ("b", "queued"), ("c", "startup_failure")]), ChecksSummary::Failed);
    }

    #[test]
    fn closed_is_kept_and_unknown_keys_are_refused() {
        let seen = Seen { closed: Some(true), ..Seen::default() };
        assert_eq!(diff(&seen, &view(vec![], vec![], vec![]), &[]).1.closed, Some(true));
        assert_eq!(Seen::parse(&seen.to_text()).unwrap(), seen);
        assert!(Seen::parse(r#"{"checks":{},"reviewIds":[],"commentIds":[],"x":1}"#).is_err());
        assert_eq!(Seen::parse(r#"{"checks":{},"reviewIds":[],"commentIds":[]}"#).unwrap(), Seen::default());
    }

    #[test]
    fn the_text_ends_with_the_commands_and_caps_the_list() {
        let many: Vec<String> = (0..25).map(|i| format!("C{i}")).collect();
        let (changes, _) = diff(&Seen::default(), &view(vec![], vec![], many.iter().map(String::as_str).collect()), &[]);
        let text = text("pay", "api", 7, &changes, false);
        assert!(text.contains("…and 5 more"), "{text}");
        assert!(text.lines().last().unwrap().contains("cpd job add"));
    }
}
