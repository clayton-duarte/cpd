//! The bot re-review comment (pro-d588, b71): after a Lead pushes, the engine asks each bot that reviewed the PR to
//! re-review, through the `swr_mutations` outbox.

use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::CpdError;
use crate::store::Store;
use crate::swr::source::github_write::{enqueue, target_of};

/// One push of the Lead to a PR that a bot reviewed.
pub struct Push<'a> {
    pub plan: &'a str,
    pub workflow: &'a str,
    pub repo: &'a str,
    pub number: i64,
    pub head_sha: &'a str,
    /// Logins that actually reviewed the PR.
    pub reviewers: &'a [String],
    /// `pr.rereview_bots`.
    pub configured: &'a [String],
}

/// The comment asking `bot` to re-review.
#[must_use]
pub fn comment_text(bot: &str) -> String {
    format!(
        "@{bot} review specifically whether your previous findings are addressed in the latest commits. Identify which are fixed, partially fixed, or still outstanding, and explain anything remaining."
    )
}

/// Queues one `comment` mutation per bot that reviewed and is configured, once per (plan, workflow, head_sha).
/// Returns the bots asked by this call.
pub fn post(
    store: &mut Store,
    now: OffsetDateTime,
    push: &Push<'_>,
) -> Result<Vec<String>, CpdError> {
    let at = now.format(&Rfc3339).expect("a UTC time formats");
    let wanted: Vec<&String> = push
        .configured
        .iter()
        .filter(|bot| push.reviewers.contains(bot))
        .collect();
    // Claims each (plan, workflow, head_sha, bot) once; only the bots newly claimed are asked.
    let fresh: Vec<String> = store.write("pr rereview", |tx| {
        let mut fresh = Vec::new();
        for bot in wanted {
            let rows = tx.execute(
                "INSERT INTO pr_rereview_asks (plan, workflow, head_sha, bot, asked_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT DO NOTHING",
                rusqlite::params![push.plan, push.workflow, push.head_sha, bot, at],
            )?;
            if rows == 1 {
                fresh.push(bot.clone());
            }
        }
        Ok(fresh)
    })?;
    let target = target_of(push.repo, push.number);
    for bot in &fresh {
        enqueue(
            store,
            now,
            &target,
            "comment",
            // `head_sha` keeps the dedupe of the outbox from merging the asks of two pushes.
            json!({"body": comment_text(bot), "head_sha": push.head_sha}),
            None,
        )?;
    }
    Ok(fresh)
}

/// The re-review pass of a poll: for each open Workflow whose open pull request has a known head and open feedback from a
/// bot in `configured`, asks that bot once per head. Reads only the store, so a poll that changed nothing posts nothing.
pub fn pass(
    store: &mut Store,
    now: OffsetDateTime,
    configured: &[String],
) -> Result<Vec<String>, CpdError> {
    if configured.is_empty() {
        return Ok(Vec::new());
    }
    type Row = (String, String, String, i64, String, String);
    let rows: Vec<Row> = store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT w.plan, w.key, r.repo, r.number, r.head_oid, r.feedback FROM pull_requests r
             JOIN workflows w ON w.pull_request = r.id AND w.status = 'open'
             JOIN plans p ON p.id = w.plan AND p.archived_at IS NULL
             WHERE r.state = 'open' AND r.head_oid IS NOT NULL AND r.feedback IS NOT NULL",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    let mut asked = Vec::new();
    for (plan, workflow, repo, number, head_sha, feedback) in rows {
        let items: Vec<super::comments::Item> =
            serde_json::from_str(&feedback).map_err(CpdError::bad_row)?;
        let reviewers: Vec<String> = items.into_iter().map(|i| i.author).collect();
        asked.extend(post(
            store,
            now,
            &Push {
                plan: &plan,
                workflow: &workflow,
                repo: &repo,
                number,
                head_sha: &head_sha,
                reviewers: &reviewers,
                configured,
            },
        )?);
    }
    Ok(asked)
}
