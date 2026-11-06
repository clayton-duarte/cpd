//! The reader of GitHub state (docs/architecture/swr-github.md §6): the one function every consumer calls. It reads the raw
//! cache and applies the patches of the active mutations; it reads no clock and calls no GitHub.

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use super::outbox::Mutation;
use super::overlay;
use crate::error::CpdError;
use crate::pr::pulls::{self, PullRequest};
use crate::store::Store;

/// A pull request as a reader sees it: the raw row with the active mutations' patches applied.
pub struct EffectivePr {
    /// The raw `pull_requests` row, unchanged.
    pub row: PullRequest,
    /// The heavy part (`swr_pr_detail.detail_json`), `None` before the first hydrate.
    pub detail: Option<serde_json::Value>,
    /// Effective: `open`, `merged`, `closed`, or `merging` while a merge is queued or not yet proven.
    pub state: String,
    pub checks: String,
    pub review_decision: String,
    pub title: String,
    pub head_oid: Option<String>,
    pub feedback_open: Option<usize>,
    /// The open feedback items, the patches applied (a queued reply removes its item).
    pub feedback: Option<Vec<crate::pr::comments::Item>>,
    /// The ids of the mutations applied, in order.
    pub overlay: Vec<i64>,
}

/// The query key of a pull request.
#[must_use]
pub fn pr_key(repo: &str, number: i64) -> String {
    format!("github:pr:{repo}#{number}")
}

/// The query key of a waiting deploy run.
#[must_use]
pub fn deploy_key(repo: &str, run_id: i64) -> String {
    format!("github:deploy:{repo}#{run_id}")
}

/// The active mutations of a pull request, oldest first.
pub fn active_in(conn: &Connection, repo: &str, number: i64) -> Result<Vec<Mutation>, CpdError> {
    overlay::active_in(conn, "github", &pr_key(repo, number))
}

fn patches(active: &[Mutation]) -> Vec<(i64, Value)> {
    active
        .iter()
        .filter_map(|m| {
            let patch = serde_json::from_str::<Value>(m.patch_json.as_deref()?).ok()?;
            Some((m.id, patch))
        })
        .collect()
}

/// `row` with the patches of `active` applied, in order: the effective row a reader decides from.
pub fn apply(row: &mut PullRequest, active: &[Mutation]) {
    for (_, patch) in patches(active) {
        let text = |k: &str| patch.get(k).and_then(Value::as_str).map(str::to_owned);
        if let Some(v) = text("state") {
            row.state = v;
        }
        if let Some(v) = text("checks") {
            row.checks = v;
        }
        if let Some(v) = text("review_decision") {
            row.review_decision = v;
        }
        if let Some(v) = text("title") {
            row.title = v;
        }
        if let Some(gone) = text("feedback_without")
            && let Some(items) = row.feedback.as_mut()
        {
            items.retain(|i| i.id != gone);
        }
    }
}

/// `row` as a reader sees it now: [`apply`] over the active mutations of its key.
pub fn effective_row(conn: &Connection, row: &mut PullRequest) -> Result<(), CpdError> {
    let active = active_in(conn, &row.repo, row.number)?;
    apply(row, &active);
    Ok(())
}

/// [`effective_pr`] on a connection.
pub fn effective_in(
    conn: &Connection,
    repo: &str,
    number: i64,
) -> Result<Option<EffectivePr>, CpdError> {
    let Some(raw) = conn
        .query_row(
            &format!(
                "SELECT {} FROM pull_requests WHERE repo = ?1 AND number = ?2",
                pulls::COLUMNS
            ),
            rusqlite::params![repo, number],
            PullRequest::from_row,
        )
        .optional()?
    else {
        return Ok(None);
    };
    let head_oid: Option<String> = conn.query_row(
        "SELECT head_oid FROM pull_requests WHERE id = ?1",
        [raw.id],
        |r| r.get(0),
    )?;
    let detail: Option<String> = conn
        .query_row(
            "SELECT detail_json FROM swr_pr_detail WHERE repo = ?1 AND number = ?2",
            rusqlite::params![repo, number],
            |r| r.get(0),
        )
        .optional()?;
    let detail = detail.and_then(|d| serde_json::from_str(&d).ok());
    let active = active_in(conn, repo, number)?;
    let mut effective = raw.clone();
    apply(&mut effective, &active);
    Ok(Some(EffectivePr {
        feedback_open: effective.feedback.as_ref().map(Vec::len),
        feedback: effective.feedback.clone(),
        state: effective.state,
        checks: effective.checks,
        review_decision: effective.review_decision,
        title: effective.title,
        head_oid,
        detail,
        overlay: patches(&active).into_iter().map(|(id, _)| id).collect(),
        row: raw,
    }))
}

/// The effective state of `repo#number`: `None` when there is no raw row.
pub fn effective_pr(
    store: &mut Store,
    repo: &str,
    number: i64,
) -> Result<Option<EffectivePr>, CpdError> {
    store.read(|c| effective_in(c, repo, number))
}

/// Whether an active mutation of the deploy run `repo#run_id` has the effective deploy state `approving`.
pub fn deploy_approving(conn: &Connection, repo: &str, run_id: i64) -> Result<bool, CpdError> {
    Ok(
        overlay::active_in(conn, "github", &deploy_key(repo, run_id))?
            .iter()
            .filter_map(|m| m.patch_json.as_deref())
            .filter_map(|p| serde_json::from_str::<Value>(p).ok())
            .any(|p| p.get("deploy").and_then(Value::as_str) == Some("approving")),
    )
}

/// The effective text of `field` (`state`, `checks`, `review_decision`, `title`) of `repo#number`: `raw` with the patches of the
/// active mutations applied, in order. For a reader that holds the raw value of its own join.
pub fn effective_text(
    conn: &Connection,
    repo: &str,
    number: i64,
    field: &str,
    raw: &str,
) -> Result<String, CpdError> {
    let mut value = raw.to_owned();
    for (_, patch) in patches(&active_in(conn, repo, number)?) {
        if let Some(v) = patch.get(field).and_then(Value::as_str) {
            value = v.to_owned();
        }
    }
    Ok(value)
}
