//! Readiness to post a PR for review (pro-d64): the one definition, for `pr post` and the poll. Pure: no I/O, no policy.

use super::pulls::PullRequest;

/// A reason a pull request is not ready to be posted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unready {
    NotOpen,
    Draft,
    Conflicting,
    ChangesRequested,
    ChecksFailed,
    ChecksPending,
    CommentsUnread,
    BotComments,
}

impl Unready {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Unready::NotOpen => "not_open",
            Unready::Draft => "draft",
            Unready::Conflicting => "conflicting",
            Unready::ChangesRequested => "changes_requested",
            Unready::ChecksFailed => "checks_failed",
            Unready::ChecksPending => "checks_pending",
            Unready::CommentsUnread => "comments_unread",
            Unready::BotComments => "bot_comments",
        }
    }
}

/// Every reason `row` is not ready, in the order of [`Unready`]; empty = ready. CI is green only when `checks` is
/// `passed`; `mergeable` "unknown" is no reason; open human comments are none.
pub(crate) fn unready(row: &PullRequest) -> Vec<Unready> {
    let mut why = Vec::new();
    if row.state != "open" {
        why.push(Unready::NotOpen);
    }
    if row.draft {
        why.push(Unready::Draft);
    }
    if row.mergeable == "conflicting" {
        why.push(Unready::Conflicting);
    }
    if row.review_decision == "changes_requested" {
        why.push(Unready::ChangesRequested);
    }
    if row.checks == "failed" {
        why.push(Unready::ChecksFailed);
    }
    if matches!(row.checks.as_str(), "pending" | "none") {
        why.push(Unready::ChecksPending);
    }
    match &row.feedback {
        None => why.push(Unready::CommentsUnread),
        Some(items) if items.iter().any(|i| i.bot) => why.push(Unready::BotComments),
        Some(_) => {}
    }
    why
}

#[cfg(test)]
mod tests {
    use super::super::comments::{Item, Kind};
    use super::*;

    fn item(bot: bool) -> Item {
        Item {
            id: "IC_1".into(),
            kind: Kind::Comment,
            last: "IC_1".into(),
            author: "someone".into(),
            bot,
            path: None,
            outdated: false,
            url: "https://github.com/a/b/pull/1#IC_1".into(),
            excerpt: "x".into(),
        }
    }

    fn ready() -> PullRequest {
        PullRequest {
            id: 1,
            repo: "a/b".into(),
            number: 1,
            url: "https://github.com/a/b/pull/1".into(),
            title: "t".into(),
            draft: false,
            state: "open".into(),
            review_decision: "none".into(),
            checks: "passed".into(),
            created_at: "c".into(),
            merged_at: None,
            changed_at: "c".into(),
            mergeable: "mergeable".into(),
            merge_state: "clean".into(),
            authored: true,
            review_requested: false,
            updated_at: None,
            feedback: Some(vec![]),
        }
    }

    fn only(change: impl FnOnce(&mut PullRequest), reason: Unready) {
        let mut row = ready();
        change(&mut row);
        assert_eq!(unready(&row), [reason]);
    }

    #[test]
    fn a_ready_row_has_no_reason() {
        assert_eq!(unready(&ready()), []);
    }

    #[test]
    fn not_open() {
        only(|r| r.state = "merged".into(), Unready::NotOpen);
        only(|r| r.state = "closed".into(), Unready::NotOpen);
    }

    #[test]
    fn draft() {
        only(|r| r.draft = true, Unready::Draft);
    }

    #[test]
    fn conflicting_but_not_unknown() {
        only(|r| r.mergeable = "conflicting".into(), Unready::Conflicting);
        let mut row = ready();
        row.mergeable = "unknown".into();
        assert_eq!(unready(&row), []);
    }

    #[test]
    fn changes_requested() {
        only(|r| r.review_decision = "changes_requested".into(), Unready::ChangesRequested);
    }

    #[test]
    fn checks_failed() {
        only(|r| r.checks = "failed".into(), Unready::ChecksFailed);
    }

    #[test]
    fn checks_pending_or_none() {
        only(|r| r.checks = "pending".into(), Unready::ChecksPending);
        only(|r| r.checks = "none".into(), Unready::ChecksPending);
    }

    #[test]
    fn comments_unread() {
        only(|r| r.feedback = None, Unready::CommentsUnread);
    }

    #[test]
    fn bot_comments_but_not_human_ones() {
        only(|r| r.feedback = Some(vec![item(false), item(true)]), Unready::BotComments);
        let mut row = ready();
        row.feedback = Some(vec![item(false)]);
        assert_eq!(unready(&row), []);
    }

    #[test]
    fn several_reasons_come_in_order() {
        let mut row = ready();
        row.state = "closed".into();
        row.draft = true;
        row.mergeable = "conflicting".into();
        row.review_decision = "changes_requested".into();
        row.checks = "failed".into();
        row.feedback = Some(vec![item(true)]);
        let names: Vec<&str> = unready(&row).into_iter().map(Unready::as_str).collect();
        assert_eq!(names, ["not_open", "draft", "conflicting", "changes_requested", "checks_failed", "bot_comments"]);
    }
}
