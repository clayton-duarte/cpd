//! The table `pull_requests` (contract §2.2 Pull request, §4.4; D5): what the
//! PR poller last read of one of the User's pull requests. This module is the
//! only writer of the table. A row is written only when a value changes; the
//! rows append no Event of their own: `attention` says which transitions the
//! owning Plan's Lead is told of (D7).

use rusqlite::{Connection, OptionalExtension, Row};
use serde_json::{Value, json};

use super::gh::PolledPr;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{PlanId, PrKey};
use crate::store::WriteTxn;

/// One row of `pull_requests`. `state`, `review_decision`, `checks`,
/// `mergeable` and `merge_state` are the words the table's `CHECK`s allow.
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
}

const COLUMNS: &str = "id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, \
                       mergeable, merge_state";

impl PullRequest {
    fn from_row(r: &Row) -> rusqlite::Result<PullRequest> {
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
        })
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

    /// The row as the poller reports it: what `upsert` compares against.
    pub(super) fn polled(&self) -> Result<PolledPr, CpdError> {
        use super::status::{ChecksSummary, PrState, ReviewDecision};
        let bad = |what: &str, value: &str| {
            CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: pull request {} has {what} {value:?}.", self.id))
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
            created_at: self.created_at.clone(),
            merged_at: self.merged_at.clone(),
        })
    }
}

/// The row `id`, else `store_failed`: a `prs` row points to it, so it exists.
pub fn load(conn: &Connection, id: i64) -> Result<PullRequest, CpdError> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM pull_requests WHERE id = ?1"), [id], PullRequest::from_row)
        .optional()?
        .ok_or_else(|| CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: no pull request {id}.")))
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

#[allow(dead_code)] // Job `pr-poll2` calls it.
/// Every row a PR in status `open` points to, with that PR's Plan and key, in Plan and key order.
pub(super) fn owned_open(conn: &Connection) -> Result<Vec<(PlanId, PrKey, PullRequest)>, CpdError> {
    let columns: Vec<String> = COLUMNS.split(", ").map(|c| format!("pr.{c}")).collect();
    let mut stmt = conn.prepare(&format!(
        "SELECT r.plan, r.key, {} FROM prs r JOIN pull_requests pr ON pr.id = r.pull_request
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
            };
            Ok((plan, key, pull))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(plan, key, pull)| {
            let bad = |what: &str, value: &str| {
                CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: a PR has {what} {value:?}."))
            };
            Ok((PlanId::parse(&plan).map_err(|_| bad("Plan id", &plan))?, PrKey::parse(&key).map_err(|_| bad("PR key", &key))?, pull))
        })
        .collect()
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

/// Inserts the row of `polled`, or updates it, **only when a value differs**
/// (`changed_at` = the transaction's time): a poll that finds nothing new
/// writes nothing. `None` when nothing changed.
pub(super) fn upsert(tx: &WriteTxn<'_>, polled: &PolledPr) -> Result<Option<Change>, CpdError> {
    let Some(before) = find(tx, &polled.repo, polled.number)? else {
        return Ok(Some(Change { before: None, after: insert(tx, polled)? }));
    };
    if before.polled()? == *polled {
        return Ok(None);
    }
    let at = txn_time(tx)?;
    tx.execute(
        "UPDATE pull_requests SET url = ?1, title = ?2, draft = ?3, state = ?4, review_decision = ?5, checks = ?6,
                created_at = ?7, merged_at = ?8, changed_at = ?9 WHERE id = ?10",
        rusqlite::params![
            polled.url,
            polled.title,
            polled.draft,
            polled.state.as_str(),
            polled.review_decision.as_str(),
            polled.checks.as_str(),
            polled.created_at,
            polled.merged_at,
            at,
            before.id
        ],
    )?;
    Ok(Some(Change { after: load(tx, before.id)?, before: Some(before) }))
}

/// The row of `polled`'s pull request: inserted when absent, else **kept as it
/// is** (`pr open`: a poll may have stored it first, D6). Its id.
pub(super) fn ensure(tx: &WriteTxn<'_>, polled: &PolledPr) -> Result<i64, CpdError> {
    match find(tx, &polled.repo, polled.number)? {
        Some(row) => Ok(row.id),
        None => Ok(insert(tx, polled)?.id),
    }
}

fn insert(tx: &WriteTxn<'_>, polled: &PolledPr) -> Result<PullRequest, CpdError> {
    let at = txn_time(tx)?;
    tx.execute(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at,
                                    mergeable, merge_state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'unknown', 'unknown')",
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
            at
        ],
    )?;
    load(tx, tx.last_insert_rowid())
}

#[allow(dead_code)] // Job `pr-poll2` calls it.
/// Deletes the rows no PR points to whose `(repo, number)` is not in `present`
/// (D5: their pull request left the open search). Rows a PR points to stay: a
/// trigger refuses their delete. The number of rows deleted.
pub(super) fn delete_unowned_absent(tx: &WriteTxn<'_>, present: &[(String, i64)]) -> Result<usize, CpdError> {
    let mut stmt = tx.prepare(
        "SELECT id, repo, number FROM pull_requests WHERE NOT EXISTS (SELECT 1 FROM prs WHERE prs.pull_request = pull_requests.id)",
    )?;
    let unowned = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut deleted = 0;
    for (id, repo, number) in unowned {
        if !present.iter().any(|(r, n)| *r == repo && *n == number) {
            deleted += tx.execute("DELETE FROM pull_requests WHERE id = ?1", [id])?;
        }
    }
    Ok(deleted)
}

/// Why the Plan's Lead is told of a change (D7), in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reason {
    ChecksFailed,
    ChangesRequested,
    Closed,
}

impl Reason {
    /// As `pr-feedback`'s `reasons` lists it.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Reason::ChecksFailed => "checks_failed",
            Reason::ChangesRequested => "changes_requested",
            Reason::Closed => "closed",
        }
    }
}

/// D7: the reasons `after` needs attention that `before` did not: checks
/// became `failed`, the review became `changes_requested` (each from anything
/// else), the state became `closed` from `open`. `before` none is a row first
/// seen: it counts as no checks, no review and `open`.
pub(super) fn attention(before: Option<&PullRequest>, after: &PullRequest) -> Vec<Reason> {
    let (checks, review, state) = match before {
        Some(b) => (b.checks.as_str(), b.review_decision.as_str(), b.state.as_str()),
        None => ("none", "none", "open"),
    };
    let mut reasons = Vec::new();
    if after.checks == "failed" && checks != "failed" {
        reasons.push(Reason::ChecksFailed);
    }
    if after.review_decision == "changes_requested" && review != "changes_requested" {
        reasons.push(Reason::ChangesRequested);
    }
    if after.state == "closed" && state == "open" {
        reasons.push(Reason::Closed);
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
    use super::super::status::{ChecksSummary, PrState, ReviewDecision};
    use super::*;
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
    fn attention_lists_every_reason_in_order() {
        let all = attention(Some(&row("open", "none", "pending")), &row("closed", "changes_requested", "failed"));
        assert_eq!(all, [Reason::ChecksFailed, Reason::ChangesRequested, Reason::Closed]);
        assert_eq!(all.iter().map(|r| r.as_str()).collect::<Vec<_>>(), ["checks_failed", "changes_requested", "closed"]);
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
            repo: "acme/pay".into(),
            number,
            url: format!("https://github.com/acme/pay/pull/{number}"),
            title: "T".into(),
            draft: false,
            state: PrState::Open,
            review_decision: ReviewDecision::None,
            checks,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            merged_at: None,
        }
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
        let first = store.write("test", |tx| upsert(tx, &polled(7, ChecksSummary::Pending))).unwrap().unwrap();
        assert_eq!(first.before, None);
        assert_eq!((first.after.checks.as_str(), history_rows(&mut store)), ("pending", 1));
        assert_eq!((first.after.mergeable.as_str(), first.after.merge_state.as_str()), ("unknown", "unknown"));
        let json = first.after.to_json();
        assert_eq!((&json["mergeable"], &json["merge_state"]), (&json!("unknown"), &json!("unknown")));

        // The same values: no change, and nothing written (no history row, `changed_at` kept).
        let again = store.write("test", |tx| upsert(tx, &polled(7, ChecksSummary::Pending))).unwrap();
        assert_eq!(again, None);
        assert_eq!(history_rows(&mut store), 1);
        let kept = store.read(|c| load(c, first.after.id)).unwrap();
        assert_eq!(kept, first.after);

        let changed = store.write("test", |tx| upsert(tx, &polled(7, ChecksSummary::Failed))).unwrap().unwrap();
        assert_eq!(changed.before.as_ref(), Some(&first.after));
        assert_eq!((changed.after.checks.as_str(), changed.after.id), ("failed", first.after.id));
        assert_eq!(history_rows(&mut store), 2);
        assert_eq!(attention(changed.before.as_ref(), &changed.after), [Reason::ChecksFailed]);
    }

    #[test]
    fn ensure_keeps_a_row_the_poll_stored_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(&dir);
        let id = store.write("test", |tx| upsert(tx, &polled(7, ChecksSummary::Failed))).unwrap().unwrap().after.id;
        let kept = store.write("test", |tx| ensure(tx, &polled(7, ChecksSummary::None))).unwrap();
        assert_eq!(kept, id);
        assert_eq!(store.read(|c| load(c, id)).unwrap().checks, "failed");
        let new = store.write("test", |tx| ensure(tx, &polled(8, ChecksSummary::None))).unwrap();
        assert_ne!(new, id);
    }

    #[test]
    fn rows_that_left_the_search_are_deleted_when_no_pr_points_to_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(&dir);
        for n in [7, 8, 9] {
            store.write("test", |tx| upsert(tx, &polled(n, ChecksSummary::None))).unwrap();
        }
        let present = [("acme/pay".to_owned(), 8)];
        assert_eq!(store.write("test", |tx| delete_unowned_absent(tx, &present)).unwrap(), 2);
        assert_eq!(store.write("test", |tx| delete_unowned_absent(tx, &present)).unwrap(), 0);
        let left: Vec<i64> = store.read(|c| Ok(c.prepare("SELECT number FROM pull_requests")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)).unwrap();
        assert_eq!(left, [8]);
    }
}
