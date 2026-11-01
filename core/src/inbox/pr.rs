//! The authored pull request rule (contract §2.2 "The authored pull request rule", pro-d130, pro-d131): one `pr-ready`
//! condition per open authored pull request, its urgency and label by the STATUS table.

use rusqlite::Connection;

use super::model::{Condition, Item, Kind, Urgency};
use super::rules::id;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::{ContextId, PlanId, PrKey, SessionId};
use crate::pr::postable::unready;
use crate::pr::pulls::{self, PullRequest};

/// The state of an authored pull request: the word of the row's label and the colour of its urgency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Draft,
    Conflict,
    Failing,
    ChangesRequested,
    Updating,
    Fixing,
    Stacked,
    ChecksRunning,
    ReadyForReview,
    ReadyToMerge,
    InReview,
    Waiting,
}

impl Status {
    pub const ALL: [Status; 12] = [
        Status::Draft,
        Status::Conflict,
        Status::Failing,
        Status::ChangesRequested,
        Status::Updating,
        Status::Fixing,
        Status::Stacked,
        Status::ChecksRunning,
        Status::ReadyForReview,
        Status::ReadyToMerge,
        Status::InReview,
        Status::Waiting,
    ];

    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Conflict => "conflict",
            Status::Failing => "failing",
            Status::ChangesRequested => "changes requested",
            Status::Updating => "updating",
            Status::Fixing => "fixing",
            Status::Stacked => "stacked pr pending base merge",
            Status::ChecksRunning => "checks running",
            Status::ReadyForReview => "ready for user review",
            Status::ReadyToMerge => "ready to merge",
            Status::InReview => "waiting for peer review",
            Status::Waiting => "waiting",
        }
    }

    #[must_use]
    pub fn urgency(self) -> Urgency {
        match self {
            Status::Draft | Status::Stacked => Urgency::White,
            Status::Conflict | Status::Failing | Status::ChangesRequested => Urgency::Red,
            Status::Updating | Status::Fixing | Status::ChecksRunning | Status::InReview | Status::Waiting => Urgency::Cyan,
            Status::ReadyForReview | Status::ReadyToMerge => Urgency::Yellow,
        }
    }

    /// The status a `pr-ready` item's label names (Row 1, `<repo>#<n> <status>`); `None` for any other item.
    #[must_use]
    pub fn of_item(item: &Item) -> Option<Status> {
        if item.kind != Kind::PrReady {
            return None;
        }
        let (_, text) = item.label.split('\n').next()?.split_once(' ')?;
        Status::ALL.into_iter().find(|s| s.text() == text)
    }
}

/// What the table reads of a pull request besides its row.
struct Facts<'a> {
    row: &'a PullRequest,
    /// The owner's Lead works, or the owner's PR has a live `update` or `work` Job.
    handled: bool,
    owned: bool,
    posted: bool,
    /// `pulls::ready_to_merge_sql` holds.
    clean: bool,
}

/// The STATUS table, first match wins: draft; broken; checks running; ready for user review (also when stacked); ready to
/// merge; then a stacked PR is `Stacked` when approved or not posted (pro-d152: stacking blocks only ready to merge),
/// else a posted PR is waiting for peer review, else waiting. `post_enabled` is asked at most once, and only when needed.
fn classify(f: &Facts<'_>, mut post_enabled: impl FnMut() -> bool) -> Status {
    let r = f.row;
    if r.draft {
        return Status::Draft;
    }
    let conflicting = r.mergeable == "conflicting";
    if conflicting || r.checks == "failed" || r.review_decision == "changes_requested" {
        return match (f.handled, conflicting, r.checks == "failed") {
            (true, true, _) => Status::Updating,
            (true, false, _) => Status::Fixing,
            (false, true, _) => Status::Conflict,
            (false, false, true) => Status::Failing,
            (false, false, false) => Status::ChangesRequested,
        };
    }
    if r.checks == "pending" {
        return Status::ChecksRunning;
    }
    let mut enabled = None;
    let mut ask = || *enabled.get_or_insert_with(&mut post_enabled);
    let judged = f.owned && !f.posted;
    if judged && unready(r).is_empty() && ask() {
        return Status::ReadyForReview;
    }
    let approved = r.review_decision == "approved";
    if f.clean && (approved || r.review_decision == "none") && (!judged || approved || !ask()) {
        return Status::ReadyToMerge;
    }
    let stacked = matches!((&r.base_ref, &r.default_branch), (Some(b), Some(d)) if b != d);
    if stacked && (approved || !f.posted) {
        Status::Stacked
    } else if f.posted {
        Status::InReview
    } else {
        Status::Waiting
    }
}

/// The `pr-ready` conditions: one per open authored pull request, linking its owner (the newest Plan with a `prs` row
/// that points to it). A policy that does not resolve counts as post disabled.
pub fn conditions(conn: &Connection, home: &CpdHome) -> Result<Vec<Condition>, CpdError> {
    let columns: Vec<String> = pulls::COLUMNS.split(", ").map(|c| format!("r.{c}")).collect();
    let mut stmt = conn.prepare(&format!(
        "SELECT {}, {}, o.plan, o.key, p.context, p.lead_session,
                COALESCE(s.state = 'working', 0)
                  OR EXISTS (SELECT 1 FROM jobs j WHERE j.plan = o.plan AND j.workflow = o.key
                                AND j.kind IN ('update', 'work') AND j.status IN ('queued', 'running')),
                EXISTS (SELECT 1 FROM pr_posts x WHERE x.plan = o.plan AND x.workflow = o.key)
           FROM pull_requests r
           LEFT JOIN workflows o ON o.rowid = (SELECT n.rowid FROM workflows n JOIN plans np ON np.id = n.plan
                                          WHERE n.pull_request = r.id
                                          ORDER BY np.created_at DESC, np.id DESC, n.key DESC LIMIT 1)
           LEFT JOIN plans p ON p.id = o.plan
           LEFT JOIN sessions s ON s.id = p.lead_session
          WHERE r.state = 'open' AND r.authored = 1
          ORDER BY r.id",
        columns.join(", "),
        pulls::ready_to_merge_sql("r"),
    ))?;
    let width = pulls::COLUMNS.split(", ").count();
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let pull = PullRequest::from_row(row)?;
        let clean: bool = row.get(width)?;
        let get = |i: usize| row.get::<_, Option<String>>(width + i);
        let (plan, key, context, lead) = (get(1)?, get(2)?, get(3)?, get(4)?);
        let (handled, posted): (bool, bool) = (row.get(width + 5)?, row.get(width + 6)?);
        let plan = plan.as_deref().map(|p| id("Plan id", p, PlanId::parse)).transpose()?;
        let key = key.as_deref().map(|k| id("PR key", k, PrKey::parse)).transpose()?;
        let owner = plan.as_ref().zip(key.as_ref());
        let facts = Facts { row: &pull, handled, owned: owner.is_some(), posted, clean };
        let status = classify(&facts, || {
            owner.is_some_and(|(plan, key)| crate::pr::policy_of_pr(conn, home, plan, key).is_ok_and(|p| p.pr.post.enabled))
        });
        let name = pull.repo.split_once('/').map_or(pull.repo.as_str(), |(_, name)| name);
        out.push(Condition {
            kind: Kind::PrReady,
            urgency: status.urgency(),
            subject: format!("pr:{}#{}", pull.repo, pull.number),
            label: format!("{name}#{} {}\n{}", pull.number, status.text(), pull.title),
            url: Some(pull.url.clone()),
            context: context.as_deref().map(|c| id("Context id", c, ContextId::parse)).transpose()?,
            plan,
            job: None,
            session: lead.as_deref().map(|s| id("Session id", s, SessionId::parse)).transpose()?,
            record: None,
            event: None,
            opened_at: None,
        });
    }
    Ok(out)
}

/// The `pr-closed` conditions: one red item per closed-unmerged PR (`crate::pr::closed::closed_unmerged`).
pub fn closed_conditions(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    Ok(crate::pr::closed::closed_unmerged(conn, None)?
        .into_iter()
        .map(|c| {
            let name = c.repo.split_once('/').map_or(c.repo.as_str(), |(_, name)| name);
            Condition {
                kind: Kind::PrClosed,
                urgency: Urgency::Red,
                subject: format!("pr:{}#{}", c.repo, c.number),
                label: format!("{name}#{} closed without a merge\n{}", c.number, c.title),
                url: Some(c.url.clone()),
                context: Some(c.context),
                plan: Some(c.plan),
                job: None,
                session: c.lead_session,
                record: None,
                event: None,
                opened_at: None,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_texts_are_distinct_and_the_urgencies_follow_the_table() {
        let texts: Vec<&str> = Status::ALL.iter().map(|s| s.text()).collect();
        assert_eq!(
            texts,
            ["draft", "conflict", "failing", "changes requested", "updating", "fixing", "stacked pr pending base merge", "checks running", "ready for user review", "ready to merge", "waiting for peer review", "waiting"]
        );
        assert_eq!(Status::Stacked.urgency(), Urgency::White);
        assert_eq!(Status::Failing.urgency(), Urgency::Red);
        assert_eq!(Status::Fixing.urgency(), Urgency::Cyan);
        assert_eq!(Status::ReadyForReview.urgency(), Urgency::Yellow);
    }

    fn item(kind: Kind, label: &str) -> Item {
        Item {
            id: 1,
            kind,
            urgency: Urgency::Cyan,
            subject: "pr:a/b#1".into(),
            label: label.into(),
            url: None,
            context: None,
            plan: None,
            job: None,
            session: None,
            record: None,
            event: None,
            opened_at: String::new(),
            closed_at: None,
            snoozed_until: None,
            dismissed_at: None,
            claimed_at: None,
        }
    }

    #[test]
    fn of_item_round_trips_every_status_and_refuses_the_rest() {
        for s in Status::ALL {
            assert_eq!(Status::of_item(&item(Kind::PrReady, &format!("b#1 {}", s.text()))), Some(s));
            assert_eq!(Status::of_item(&item(Kind::ReviewRequested, &format!("b#1 {}", s.text()))), None);
        }
        assert_eq!(Status::of_item(&item(Kind::PrReady, "b#1 nonsense")), None);
        assert_eq!(Status::of_item(&item(Kind::PrReady, "nospace")), None);
    }
}
