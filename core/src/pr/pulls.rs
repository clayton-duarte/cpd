//! The table `pull_requests` (contract §2.2 Pull request, §4.4; D5): what the
//! pull request poller last read of one of the User's pull requests. This module is the
//! only writer of the table. A row is written only when a value changes; the
//! rows append no Event of their own: `attention` says which transitions the
//! owning Plan's Lead is told of (D7).

use rusqlite::{Connection, OptionalExtension, Row};
use serde_json::{Value, json};

use super::comments::{Item, Read};
use super::gh::{Listed, Membership, PolledPr};
use crate::error::CpdError;
use crate::ids::{PlanId, WorkflowKey};
use crate::policy::Checks;
use crate::store::WriteTxn;

/// One row of `pull_requests`. `state`, `review_decision`, `checks`,
/// `mergeable` and `merge_state` are the words the table's `CHECK`s allow;
/// `authored` and `review_requested` are the searches that last listed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    pub id: i64,
    pub repo: String,
    pub number: i64,
    pub url: String,
    pub title: String,
    pub draft: bool,
    pub state: String,
    pub review_decision: String,
    pub checks: String,
    pub created_at: String,
    pub merged_at: Option<String>,
    pub changed_at: String,
    pub mergeable: String,
    pub merge_state: String,
    pub authored: bool,
    pub review_requested: bool,
    /// GitHub's `updatedAt` at the last read of the feedback; `None` = never read.
    pub updated_at: Option<String>,
    /// The open feedback items at that read; `None` = never read.
    pub feedback: Option<Vec<Item>>,
    /// The base branch at the last poll; `None` = not polled since `pr-base` (unknown).
    pub base_ref: Option<String>,
    /// The repo's default branch at the last poll; `None` = unknown.
    pub default_branch: Option<String>,
}

/// The expression "ready to merge" (pro-d118) over the `pull_requests` row aliased `alias`: 1 or 0, never NULL. The
/// pull request is open, not a draft, its merge state is clean and its base is the repo's default branch; an unknown
/// (NULL) base or default branch is not ready. The one definition: every reader uses it.
pub fn ready_to_merge_sql(alias: &str) -> String {
    format!(
        "COALESCE({alias}.state = 'open' AND {alias}.draft = 0 AND {alias}.merge_state = 'clean' \
         AND {alias}.base_ref IS NOT NULL AND {alias}.base_ref = {alias}.default_branch, 0)"
    )
}

pub(crate) const COLUMNS: &str = "id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, \
                       mergeable, merge_state, authored, review_requested, updated_at, feedback, base_ref, default_branch";

/// The items a `feedback` column holds: parsed once, here.
fn feedback_of(text: Option<String>) -> rusqlite::Result<Option<Vec<Item>>> {
    text.map(|t| serde_json::from_str(&t).map_err(|e| rusqlite::Error::FromSqlConversionFailure(17, rusqlite::types::Type::Text, Box::new(e)))).transpose()
}

/// The `feedback` column of `items`: JSON.
fn feedback_text(items: &[Item]) -> String {
    serde_json::to_string(items).expect("items are plain data")
}

impl PullRequest {
    pub(crate) fn from_row(r: &Row) -> rusqlite::Result<PullRequest> {
        Ok(PullRequest {
            id: r.get(0)?,
            repo: r.get(1)?,
            number: r.get(2)?,
            url: r.get(3)?,
            title: r.get(4)?,
            draft: r.get(5)?,
            state: r.get(6)?,
            review_decision: r.get(7)?,
            checks: r.get(8)?,
            created_at: r.get(9)?,
            merged_at: r.get(10)?,
            changed_at: r.get(11)?,
            mergeable: r.get(12)?,
            merge_state: r.get(13)?,
            authored: r.get(14)?,
            review_requested: r.get(15)?,
            updated_at: r.get(16)?,
            feedback: feedback_of(r.get(17)?)?,
            base_ref: r.get(18)?,
            default_branch: r.get(19)?,
        })
    }

    /// The searches that last listed the pull request.
    pub(super) fn membership(&self) -> Membership {
        Membership { authored: self.authored, review_requested: self.review_requested }
    }

    /// `{repo, number, url, title, draft, state, review_decision, checks, mergeable, merge_state, created_at, merged_at}`,
    /// as `plan list`, `plan show` and `pr poll` print it (commands.md §4.2).
    pub fn to_json(&self) -> Value {
        json!({
            "repo": self.repo, "number": self.number, "url": self.url, "title": self.title, "draft": self.draft,
            "state": self.state, "review_decision": self.review_decision, "checks": self.checks,
            "mergeable": self.mergeable, "merge_state": self.merge_state,
            "created_at": self.created_at, "merged_at": self.merged_at,
        })
    }

    /// Whether the row already holds what `listed` reports, and what `read` (the comments read of this poll, if any)
    /// found. `created_at` is not compared: it is written once, with the row, and never changes (§2.2, store-W5);
    /// `updated_at` is not either: the row's is that of its last comments read, not of the listing.
    pub(super) fn holds(&self, listed: &Listed, read: Option<&Read>) -> Result<bool, CpdError> {
        let read_held = read.is_none_or(|r| self.updated_at.as_deref() == Some(r.updated_at.as_str()) && self.feedback.as_deref() == Some(r.items.as_slice()));
        if !read_held {
            return Ok(false);
        }
        // GitHub never un-merges: a stored merge is final against any listing that is not MERGED (hs-d550).
        if self.state == "merged" && listed.pr.state != super::status::PrState::Merged {
            return Ok(true);
        }
        let same_but_created_at = PolledPr { created_at: self.created_at.clone(), updated_at: self.polled()?.updated_at, ..listed.pr.clone() };
        Ok(self.polled()? == same_but_created_at && self.membership() == listed.membership)
    }

    /// The row as the poller reports it.
    pub(super) fn polled(&self) -> Result<PolledPr, CpdError> {
        use super::status::{ChecksSummary, MergeState, Mergeable, PrState, ReviewDecision};
        let bad = |what: &str, value: &str| {
            CpdError::bad_row(format!("pull request {} has {what} {value:?}", self.id))
        };
        Ok(PolledPr {
            repo: self.repo.clone(),
            number: self.number,
            url: self.url.clone(),
            title: self.title.clone(),
            draft: self.draft,
            state: [PrState::Open, PrState::Closed, PrState::Merged]
                .into_iter()
                .find(|s| s.as_str() == self.state)
                .ok_or_else(|| bad("state", &self.state))?,
            review_decision: [ReviewDecision::None, ReviewDecision::Approved, ReviewDecision::ChangesRequested]
                .into_iter()
                .find(|s| s.as_str() == self.review_decision)
                .ok_or_else(|| bad("review_decision", &self.review_decision))?,
            checks: [ChecksSummary::None, ChecksSummary::Passed, ChecksSummary::Pending, ChecksSummary::Failed]
                .into_iter()
                .find(|s| s.as_str() == self.checks)
                .ok_or_else(|| bad("checks", &self.checks))?,
            mergeable: [Mergeable::Mergeable, Mergeable::Conflicting, Mergeable::Unknown]
                .into_iter()
                .find(|s| s.as_str() == self.mergeable)
                .ok_or_else(|| bad("mergeable", &self.mergeable))?,
            merge_state: MergeState::ALL
                .into_iter()
                .find(|s| s.as_str() == self.merge_state)
                .ok_or_else(|| bad("merge_state", &self.merge_state))?,
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone().unwrap_or_default(),
            merged_at: self.merged_at.clone(),
            base_ref: self.base_ref.clone(),
            default_branch: self.default_branch.clone(),
        })
    }
}

/// The row `id`, else `store_failed`: a `prs` row points to it, so it exists.
pub fn load(conn: &Connection, id: i64) -> Result<PullRequest, CpdError> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM pull_requests WHERE id = ?1"), [id], PullRequest::from_row)
        .optional()?
        .ok_or_else(|| CpdError::bad_row(format!("no pull request {id}")))
}

/// The row of `(repo, number)`, if any.
pub(super) fn find(conn: &Connection, repo: &str, number: i64) -> Result<Option<PullRequest>, CpdError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM pull_requests WHERE repo = ?1 AND number = ?2"),
            rusqlite::params![repo, number],
            PullRequest::from_row,
        )
        .optional()?)
}

/// Every row a Workflow in status `open` points to, with that Workflow's Plan and key, in Plan and key order.
pub(super) fn owned_open(conn: &Connection) -> Result<Vec<(PlanId, WorkflowKey, PullRequest)>, CpdError> {
    let columns: Vec<String> = COLUMNS.split(", ").map(|c| format!("pr.{c}")).collect();
    let mut stmt = conn.prepare(&format!(
        "SELECT r.plan, r.key, {} FROM workflows r JOIN pull_requests pr ON pr.id = r.pull_request
          WHERE r.status = 'open' ORDER BY r.plan, r.key",
        columns.join(", ")
    ))?;
    let rows = stmt
        .query_map([], |r| {
            let (plan, key): (String, String) = (r.get(0)?, r.get(1)?);
            let pull = PullRequest {
                id: r.get(2)?,
                repo: r.get(3)?,
                number: r.get(4)?,
                url: r.get(5)?,
                title: r.get(6)?,
                draft: r.get(7)?,
                state: r.get(8)?,
                review_decision: r.get(9)?,
                checks: r.get(10)?,
                created_at: r.get(11)?,
                merged_at: r.get(12)?,
                changed_at: r.get(13)?,
                mergeable: r.get(14)?,
                merge_state: r.get(15)?,
                authored: r.get(16)?,
                review_requested: r.get(17)?,
                updated_at: r.get(18)?,
                feedback: feedback_of(r.get(19)?)?,
                base_ref: r.get(20)?,
                default_branch: r.get(21)?,
            };
            Ok((plan, key, pull))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(plan, key, pull)| {
            let bad = |what: &str, value: &str| {
                CpdError::bad_row(format!("a pull request has {what} {value:?}"))
            };
            Ok((PlanId::parse(&plan).map_err(|_| bad("Plan id", &plan))?, WorkflowKey::parse(&key).map_err(|_| bad("Workflow key", &key))?, pull))
        })
        .collect()
}

/// Every row stored `open`, owned or not, in id order.
pub(super) fn stored_open(conn: &Connection) -> Result<Vec<PullRequest>, CpdError> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM pull_requests WHERE state = 'open' ORDER BY id"))?;
    Ok(stmt.query_map([], PullRequest::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// What `upsert` changed: the row before (none when it was inserted) and after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Change {
    pub before: Option<PullRequest>,
    pub after: PullRequest,
}

/// The time of the transaction (`txns.at`).
pub(super) fn txn_time(tx: &WriteTxn<'_>) -> Result<String, CpdError> {
    Ok(tx.query_row("SELECT at FROM txns WHERE id = cpd_txn()", [], |r| r.get(0))?)
}

/// Inserts the row of `listed`, or updates it, **only when a value differs**
/// (`changed_at` = the transaction's time; the searches that listed it are values):
/// a poll that finds nothing new writes nothing. `None` when nothing changed. `created_at`
/// is the one value never updated: GitHub's, as first written.
pub(super) fn upsert(tx: &WriteTxn<'_>, listed: &Listed, read: Option<&Read>) -> Result<Option<Change>, CpdError> {
    let Listed { pr: polled, membership } = listed;
    let Some(before) = find(tx, &polled.repo, polled.number)? else {
        return Ok(Some(Change { before: None, after: insert(tx, polled, *membership, read)? }));
    };
    if before.holds(listed, read)? {
        return Ok(None);
    }
    let at = txn_time(tx)?;
    let updated_at = read.map(|r| r.updated_at.clone()).or_else(|| before.updated_at.clone());
    let feedback = read.map(|r| feedback_text(&r.items)).or_else(|| before.feedback.as_deref().map(feedback_text));
    tx.execute(
        "UPDATE pull_requests SET url = ?1, title = ?2, draft = ?3, state = ?4, review_decision = ?5, checks = ?6,
                merged_at = ?7, changed_at = ?8, mergeable = ?9, merge_state = ?10, authored = ?11,
                review_requested = ?12, updated_at = ?13, feedback = ?14, base_ref = ?15, default_branch = ?16 WHERE id = ?17",
        rusqlite::params![
            polled.url,
            polled.title,
            polled.draft,
            polled.state.as_str(),
            polled.review_decision.as_str(),
            polled.checks.as_str(),
            polled.merged_at,
            at,
            polled.mergeable.as_str(),
            polled.merge_state.as_str(),
            membership.authored,
            membership.review_requested,
            updated_at,
            feedback,
            polled.base_ref,
            polled.default_branch,
            before.id
        ],
    )?;
    Ok(Some(Change { after: load(tx, before.id)?, before: Some(before) }))
}

/// The row of `polled`'s pull request: inserted when absent (the User's own: `authored`
/// 1, `review_requested` 0), else **kept as it is** (`pr open`: a poll may have stored it
/// first, D6). Its id.
pub(super) fn ensure(tx: &WriteTxn<'_>, polled: &PolledPr) -> Result<i64, CpdError> {
    match find(tx, &polled.repo, polled.number)? {
        Some(row) => Ok(row.id),
        None => Ok(insert(tx, polled, Membership { authored: true, review_requested: false }, None)?.id),
    }
}

fn insert(tx: &WriteTxn<'_>, polled: &PolledPr, membership: Membership, read: Option<&Read>) -> Result<PullRequest, CpdError> {
    let at = txn_time(tx)?;
    tx.execute(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at,
                                    mergeable, merge_state, authored, review_requested, updated_at, feedback, base_ref, default_branch)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        rusqlite::params![
            polled.repo,
            polled.number,
            polled.url,
            polled.title,
            polled.draft,
            polled.state.as_str(),
            polled.review_decision.as_str(),
            polled.checks.as_str(),
            polled.created_at,
            polled.merged_at,
            at,
            polled.mergeable.as_str(),
            polled.merge_state.as_str(),
            membership.authored,
            membership.review_requested,
            read.map(|r| r.updated_at.as_str()),
            read.map(|r| feedback_text(&r.items)),
            polled.base_ref,
            polled.default_branch
        ],
    )?;
    load(tx, tx.last_insert_rowid())
}

/// Why the Plan's Lead is told of a change (D7), in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reason {
    ChecksFailed,
    ChangesRequested,
    Comments,
    Closed,
    ReadyToPost,
    Approved,
}

/// What `attention` needs of the post (pro-d64): the owner's `pr.post.enabled` and whether the Workflow has a `pr_posts` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PostFacts {
    pub enabled: bool,
    pub posted: bool,
}

/// What `attention` needs of the checks: whether the Workflow carries one of the owner's `pr.excused_labels` (pro-d216).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Excused(pub bool);

impl Reason {
    /// As `pr-feedback`'s `reasons` lists it.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Reason::ChecksFailed => "checks_failed",
            Reason::ChangesRequested => "changes_requested",
            Reason::Comments => "comments",
            Reason::Closed => "closed",
            Reason::ReadyToPost => "ready_to_post",
            Reason::Approved => "approved",
        }
    }
}

/// D7: the reasons `after` needs attention that `before` did not: checks
/// became `failed`, the review became `changes_requested` (each from anything
/// else), the state became `closed` from `open`, or the feedback has `new` open items (`comments::new_items`). `before` none is a row first
/// seen: it counts as no checks, no review and `open`. A conflict is no reason any more (hs-d340): it queues an `update` Job
/// (`poll::update_for_conflict`). Checks are a reason only when the Workflow's `pr.feedback.checks` is `any` (§8.3, hs-d292).
pub(super) fn attention(before: Option<&PullRequest>, after: &PullRequest, checks_policy: Checks, new: &[&Item], post: PostFacts, excused: Excused) -> Vec<Reason> {
    let (checks, review, state) = match before {
        Some(b) => (b.checks.as_str(), b.review_decision.as_str(), b.state.as_str()),
        None => ("none", "none", "open"),
    };
    let mut reasons = Vec::new();
    if checks_policy == Checks::Any && !excused.0 && after.checks == "failed" && checks != "failed" {
        reasons.push(Reason::ChecksFailed);
    }
    if after.review_decision == "changes_requested" && review != "changes_requested" {
        reasons.push(Reason::ChangesRequested);
    }
    if !new.is_empty() {
        reasons.push(Reason::Comments);
    }
    if after.state == "closed" && state == "open" {
        reasons.push(Reason::Closed);
    }
    // The post's reasons (pro-d64): only for the User's own pull requests of an owner that enabled posting.
    if post.enabled && after.authored {
        // Ready to post (pro-d225): a transition, told once when the Workflow turns ready; `before` none was not ready.
        let was_ready = before.is_some_and(|b| super::postable::unready(b).is_empty());
        if !post.posted && !was_ready && super::postable::unready(after).is_empty() {
            reasons.push(Reason::ReadyToPost);
        }
        if post.posted && after.state == "open" && after.review_decision == "approved" && review != "approved" {
            reasons.push(Reason::Approved);
        }
    }
    reasons
}

/// `owner/name` and the number of `https://github.com/<owner>/<name>/pull/<n>`
/// (what `gh pr create` prints); `None` for any other URL.
pub(super) fn parse_url(url: &str) -> Option<(String, i64)> {
    let (repo, number) = url.strip_prefix("https://github.com/")?.split_once("/pull/")?;
    let (owner, name) = repo.split_once('/')?;
    let number = number.parse::<i64>().ok().filter(|n| *n >= 1)?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/')).then(|| (repo.to_owned(), number))
}

#[cfg(test)]
mod tests {
    use super::super::status::{ChecksSummary, MergeState, Mergeable, PrState, ReviewDecision};
    use super::*;
    use crate::policy::Checks;

    const NOT: Excused = Excused(false);
    const OFF: PostFacts = PostFacts { enabled: false, posted: false };

    /// `super::attention` with `pr.feedback.checks` `any`, cpd's default.
    fn attention(before: Option<&PullRequest>, after: &PullRequest) -> Vec<Reason> {
        super::attention(before, after, Checks::Any, &[], OFF, NOT)
    }
    use crate::home::CpdHome;
    use crate::store::Store;

    fn row(state: &str, review: &str, checks: &str) -> PullRequest {
        PullRequest {
            id: 1,
            repo: "acme/pay".into(),
            number: 7,
            url: "https://github.com/acme/pay/pull/7".into(),
            title: "T".into(),
            draft: false,
            state: state.into(),
            review_decision: review.into(),
            checks: checks.into(),
            created_at: "t".into(),
            merged_at: None,
            changed_at: "t".into(),
            mergeable: "unknown".into(),
            merge_state: "unknown".into(),
            authored: true,
            review_requested: false,
            updated_at: None,
            feedback: None,
            base_ref: None,
            default_branch: None,
        }
    }

    #[test]
    fn attention_is_checks_failing_changes_requested_and_closing_each_from_anything_else() {
        let open = row("open", "none", "none");
        assert_eq!(attention(Some(&open), &row("open", "none", "failed")), [Reason::ChecksFailed]);
        assert_eq!(attention(Some(&row("open", "none", "pending")), &row("open", "none", "failed")), [Reason::ChecksFailed]);
        assert_eq!(attention(Some(&open), &row("open", "changes_requested", "none")), [Reason::ChangesRequested]);
        assert_eq!(attention(Some(&row("open", "approved", "none")), &row("open", "changes_requested", "none")), [Reason::ChangesRequested]);
        assert_eq!(attention(Some(&open), &row("closed", "none", "none")), [Reason::Closed]);
    }

    #[test]
    fn attention_ignores_failing_checks_when_the_policy_is_none_and_keeps_every_other_reason() {
        let open = row("open", "none", "none");
        assert_eq!(super::attention(Some(&open), &row("open", "none", "failed"), Checks::None, &[], OFF, NOT), []);
        assert_eq!(super::attention(Some(&open), &row("open", "none", "failed"), Checks::Any, &[], OFF, NOT), [Reason::ChecksFailed]);
        let all = super::attention(Some(&row("open", "none", "pending")), &row("closed", "changes_requested", "failed"), Checks::None, &[], OFF, NOT);
        assert_eq!(all, [Reason::ChangesRequested, Reason::Closed]);
    }

    #[test]
    fn an_excused_pr_raises_no_checks_failed_and_every_other_reason_stays() {
        let open = row("open", "none", "none");
        assert_eq!(super::attention(Some(&open), &row("open", "none", "failed"), Checks::Any, &[], OFF, Excused(true)), []);
        let rest = super::attention(Some(&open), &row("closed", "changes_requested", "failed"), Checks::Any, &[], OFF, Excused(true));
        assert_eq!(rest, [Reason::ChangesRequested, Reason::Closed]);
    }

    #[test]
    fn attention_lists_every_reason_in_order() {
        let all = attention(Some(&row("open", "none", "pending")), &row("closed", "changes_requested", "failed"));
        assert_eq!(all, [Reason::ChecksFailed, Reason::ChangesRequested, Reason::Closed]);
        assert_eq!(all.iter().map(|r| r.as_str()).collect::<Vec<_>>(), ["checks_failed", "changes_requested", "closed"]);
        let order = [Reason::ChecksFailed, Reason::ChangesRequested, Reason::Comments, Reason::Closed, Reason::ReadyToPost, Reason::Approved];
        assert_eq!(
            order.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
            ["checks_failed", "changes_requested", "comments", "closed", "ready_to_post", "approved"]
        );
    }

    const ON: PostFacts = PostFacts { enabled: true, posted: false };
    const POSTED: PostFacts = PostFacts { enabled: true, posted: true };

    fn ready() -> PullRequest {
        let mut r = row("open", "none", "passed");
        r.feedback = Some(vec![]);
        r
    }

    fn post_reasons(before: Option<&PullRequest>, after: &PullRequest, post: PostFacts) -> Vec<Reason> {
        super::attention(before, after, Checks::Any, &[], post, NOT)
    }

    fn bot_item() -> Item {
        Item { id: "1".into(), kind: super::super::comments::Kind::Thread, last: "t".into(), author: "bot".into(), bot: true, path: None, outdated: false, url: "u".into(), excerpt: "e".into() }
    }

    #[test]
    fn no_posting_state_or_authorship_raises_a_reason_for_a_ready_pr() {
        let mut other = ready();
        other.authored = false;
        let mut bot = ready();
        bot.feedback = Some(vec![bot_item()]);
        assert_eq!(post_reasons(None, &ready(), ON), [Reason::ReadyToPost]);
        for post in [OFF, ON, POSTED] {
            if post != ON {
                assert!(post_reasons(None, &ready(), post).is_empty());
            }
            assert!(post_reasons(None, &other, post).is_empty());
            assert!(post_reasons(None, &bot, post).is_empty());
        }
    }

    #[test]
    fn ready_to_post_fires_from_none_and_after_not_ready_but_not_while_ready() {
        assert_eq!(post_reasons(None, &ready(), ON), [Reason::ReadyToPost]);
        let mut pending = ready();
        pending.checks = "pending".into();
        assert_eq!(post_reasons(Some(&pending), &ready(), ON), [Reason::ReadyToPost]);
        let mut unread = ready();
        unread.feedback = None;
        assert_eq!(post_reasons(Some(&unread), &ready(), ON), [Reason::ReadyToPost]);
        let mut later = ready();
        later.title = "other".into();
        assert!(post_reasons(Some(&ready()), &later, ON).is_empty(), "no repeat while ready");
    }

    #[test]
    fn ready_to_post_needs_the_policy_an_unposted_authored_ready_pr() {
        assert!(post_reasons(None, &ready(), OFF).is_empty(), "not enabled");
        assert!(post_reasons(None, &ready(), POSTED).is_empty(), "posted");
        let mut other = ready();
        other.authored = false;
        assert!(post_reasons(None, &other, ON).is_empty(), "not authored");
        let mut bot = ready();
        bot.feedback = Some(vec![bot_item()]);
        assert!(post_reasons(None, &bot, ON).is_empty(), "a bot item is open");
        let mut closed = ready();
        closed.state = "closed".into();
        assert_eq!(post_reasons(None, &closed, ON), [Reason::Closed], "a closed Workflow is not ready");
    }

    #[test]
    fn approved_fires_once_for_a_posted_open_pr() {
        let mut approved = ready();
        approved.review_decision = "approved".into();
        assert_eq!(post_reasons(None, &approved, POSTED), [Reason::Approved]);
        let mut changes = ready();
        changes.review_decision = "changes_requested".into();
        assert_eq!(post_reasons(Some(&changes), &approved, POSTED), [Reason::Approved]);
        assert!(post_reasons(Some(&approved), &approved, POSTED).is_empty(), "no repeat");
        assert_eq!(post_reasons(None, &approved, ON), [Reason::ReadyToPost], "unposted: ready, no approved");
        assert!(post_reasons(None, &approved, OFF).is_empty(), "disabled");
        let mut closed = approved.clone();
        closed.state = "closed".into();
        assert_eq!(post_reasons(None, &closed, POSTED), [Reason::Closed], "closed Workflow");
    }

    fn conflicting(mut row: PullRequest) -> PullRequest {
        row.mergeable = "conflicting".into();
        row
    }

    #[test]
    fn a_conflict_is_no_reason_to_tell_the_lead() {
        let open = || row("open", "none", "none");
        assert!(attention(Some(&open()), &conflicting(open())).is_empty());
        assert!(attention(None, &conflicting(open())).is_empty(), "a row first seen conflicting");
    }

    #[test]
    fn attention_is_empty_for_what_is_already_so_or_improves() {
        let bad = row("open", "changes_requested", "failed");
        assert!(attention(Some(&bad), &bad).is_empty());
        assert!(attention(Some(&bad), &row("open", "approved", "passed")).is_empty());
        assert!(attention(Some(&row("closed", "none", "none")), &row("closed", "none", "none")).is_empty());
        // Reopened, then closed again: the second closing is a change from `open`.
        assert_eq!(attention(Some(&row("open", "none", "none")), &row("closed", "none", "none")), [Reason::Closed]);
        assert!(attention(Some(&row("closed", "none", "none")), &row("open", "none", "none")).is_empty());
        assert!(attention(Some(&row("open", "none", "none")), &row("merged", "none", "none")).is_empty());
    }

    #[test]
    fn the_url_gives_the_repo_and_the_number() {
        assert_eq!(parse_url("https://github.com/acme/pay/pull/812"), Some(("acme/pay".to_owned(), 812)));
        for bad in ["", "https://example.com/acme/pay/pull/1", "https://github.com/acme/pay/pull/x", "https://github.com/acme/pull/1", "https://github.com/a/b/c/pull/1", "https://github.com/acme/pay/pull/0"] {
            assert_eq!(parse_url(bad), None, "{bad}");
        }
    }

    fn polled(number: i64, checks: ChecksSummary) -> PolledPr {
        PolledPr {
            mergeable: Mergeable::Unknown,
            merge_state: MergeState::Unknown,
            repo: "acme/pay".into(),
            number,
            url: format!("https://github.com/acme/pay/pull/{number}"),
            title: "T".into(),
            draft: false,
            state: PrState::Open,
            review_decision: ReviewDecision::None,
            checks,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-02T00:00:00Z".into(),
            merged_at: None,
            base_ref: None,
            default_branch: None,
        }
    }

    const AUTHORED: Membership = Membership { authored: true, review_requested: false };

    /// `upsert` of a pull request the User wrote.
    fn up(tx: &WriteTxn<'_>, polled: &PolledPr) -> Result<Option<Change>, CpdError> {
        upsert(tx, &Listed { pr: polled.clone(), membership: AUTHORED }, None)
    }

    fn store(dir: &tempfile::TempDir) -> Store {
        let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
        Store::open(&home, 5000).unwrap()
    }

    fn history_rows(store: &mut Store) -> i64 {
        store.read(|c| Ok(c.query_row("SELECT count(*) FROM history WHERE tbl = 'pull_requests'", [], |r| r.get(0))?)).unwrap()
    }

    #[test]
    fn upsert_writes_only_when_a_value_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(&dir);
        let first = store.write("test", |tx| up(tx, &polled(7, ChecksSummary::Pending))).unwrap().unwrap();
        assert_eq!(first.before, None);
        assert_eq!((first.after.checks.as_str(), history_rows(&mut store)), ("pending", 1));
        assert_eq!((first.after.mergeable.as_str(), first.after.merge_state.as_str()), ("unknown", "unknown"));
        let json = first.after.to_json();
        assert_eq!((&json["mergeable"], &json["merge_state"]), (&json!("unknown"), &json!("unknown")));

        // The same values: no change, and nothing written (no history row, `changed_at` kept).
        let again = store.write("test", |tx| up(tx, &polled(7, ChecksSummary::Pending))).unwrap();
        assert_eq!(again, None);
        assert_eq!(history_rows(&mut store), 1);
        let kept = store.read(|c| load(c, first.after.id)).unwrap();
        assert_eq!(kept, first.after);

        let changed = store.write("test", |tx| up(tx, &polled(7, ChecksSummary::Failed))).unwrap().unwrap();
        assert_eq!(changed.before.as_ref(), Some(&first.after));
        assert_eq!((changed.after.checks.as_str(), changed.after.id), ("failed", first.after.id));
        assert_eq!(history_rows(&mut store), 2);
        assert_eq!(attention(changed.before.as_ref(), &changed.after), [Reason::ChecksFailed]);
    }

    #[test]
    fn a_change_of_mergeable_or_merge_state_alone_is_a_change_of_the_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(&dir);
        let first = store.write("test", |tx| up(tx, &polled(7, ChecksSummary::None))).unwrap().unwrap();
        let with = |mergeable, merge_state| PolledPr { mergeable, merge_state, ..polled(7, ChecksSummary::None) };

        let conflict = store
            .write("test", |tx| up(tx, &with(Mergeable::Conflicting, MergeState::Unknown)))
            .unwrap()
            .unwrap();
        assert_eq!((conflict.after.mergeable.as_str(), conflict.after.merge_state.as_str()), ("conflicting", "unknown"));
        assert_eq!(attention(conflict.before.as_ref(), &conflict.after), []);
        assert_eq!(history_rows(&mut store), 2);

        let dirty = store.write("test", |tx| up(tx, &with(Mergeable::Conflicting, MergeState::Dirty))).unwrap().unwrap();
        assert_eq!((dirty.after.mergeable.as_str(), dirty.after.merge_state.as_str()), ("conflicting", "dirty"));
        assert_eq!(attention(dirty.before.as_ref(), &dirty.after), []);
        assert_eq!(history_rows(&mut store), 3);

        let again = store.write("test", |tx| up(tx, &with(Mergeable::Conflicting, MergeState::Dirty))).unwrap();
        assert_eq!(again, None);
        assert_eq!(history_rows(&mut store), 3);
        assert_eq!(store.read(|c| load(c, first.after.id)).unwrap(), dirty.after);
    }

    #[test]
    fn a_change_of_the_searches_that_list_it_alone_is_a_change_of_the_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(&dir);
        let both = Membership { authored: true, review_requested: true };
        let requested = Membership { authored: false, review_requested: true };
        let put = |store: &mut Store, membership| {
            store.write("test", |tx| upsert(tx, &Listed { pr: polled(7, ChecksSummary::None), membership }, None)).unwrap()
        };

        let first = put(&mut store, requested).unwrap();
        assert_eq!(first.before, None);
        assert_eq!((first.after.authored, first.after.review_requested), (false, true));
        assert_eq!(put(&mut store, requested), None, "the same searches: nothing written");
        assert_eq!(history_rows(&mut store), 1);

        let both_now = put(&mut store, both).unwrap();
        assert_eq!((both_now.after.authored, both_now.after.review_requested), (true, true));
        assert_eq!(history_rows(&mut store), 2);
        assert_eq!(attention(both_now.before.as_ref(), &both_now.after), [], "no Reason: the flags are no feedback");

        let gone = put(&mut store, AUTHORED).unwrap();
        assert_eq!((gone.after.authored, gone.after.review_requested), (true, false));
        assert_eq!(history_rows(&mut store), 3);
    }

    #[test]
    fn ensure_inserts_the_users_own_pull_request_as_authored() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(&dir);
        let id = store.write("test", |tx| ensure(tx, &polled(7, ChecksSummary::None))).unwrap();
        let row = store.read(|c| load(c, id)).unwrap();
        assert_eq!((row.authored, row.review_requested), (true, false));
    }

    #[test]
    fn ensure_keeps_a_row_the_poll_stored_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(&dir);
        let id = store.write("test", |tx| up(tx, &polled(7, ChecksSummary::Failed))).unwrap().unwrap().after.id;
        let kept = store.write("test", |tx| ensure(tx, &polled(7, ChecksSummary::None))).unwrap();
        assert_eq!(kept, id);
        assert_eq!(store.read(|c| load(c, id)).unwrap().checks, "failed");
        let new = store.write("test", |tx| ensure(tx, &polled(8, ChecksSummary::None))).unwrap();
        assert_ne!(new, id);
    }

    #[test]
    fn stored_open_lists_only_the_open_rows() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(&dir);
        for n in [7, 8] {
            store.write("test", |tx| up(tx, &polled(n, ChecksSummary::None))).unwrap();
        }
        let closed = PolledPr { state: PrState::Closed, ..polled(9, ChecksSummary::None) };
        store.write("test", |tx| upsert(tx, &Listed { pr: closed, membership: Membership::default() }, None)).unwrap();
        let open: Vec<i64> = store.read(stored_open).unwrap().iter().map(|r| r.number).collect();
        assert_eq!(open, [7, 8]);
    }
}
