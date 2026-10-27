//! The table `pr_posts` (contract §4.4; PR post, pro-d64, pro-d65): the one post of a PR for review in Slack.
//! Written once by `pr post`; the only later write is `reacted_at` (PR `react`). A data row: it appends no Event.

use rusqlite::{Connection, OptionalExtension};

use crate::error::CpdError;
use crate::store::WriteTxn;

/// One row of `pr_posts`, without its key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostRecord {
    pub channel: String,
    pub ts: String,
    pub link: String,
    pub posted_at: String,
    /// The emoji name the policy had at the post; `None` = no reaction.
    pub reaction: Option<String>,
    /// When the reaction was added; `None` until then.
    pub reacted_at: Option<String>,
}

/// The post of PR `pr` of `plan`, if it has one.
pub fn find(conn: &Connection, plan: &str, pr: &str) -> Result<Option<PostRecord>, CpdError> {
    Ok(conn
        .query_row(
            "SELECT channel, ts, link, posted_at, reaction, reacted_at FROM pr_posts WHERE plan = ?1 AND pr = ?2",
            [plan, pr],
            |r| {
                Ok(PostRecord { channel: r.get(0)?, ts: r.get(1)?, link: r.get(2)?, posted_at: r.get(3)?, reaction: r.get(4)?, reacted_at: r.get(5)? })
            },
        )
        .optional()?)
}

/// Writes the post of PR `pr` of `plan`; `false` when the PR already has one (nothing is written).
pub fn insert(tx: &WriteTxn<'_>, plan: &str, pr: &str, post: &PostRecord) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "INSERT INTO pr_posts (plan, pr, channel, ts, link, posted_at, reaction, reacted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (plan, pr) DO NOTHING",
        rusqlite::params![plan, pr, post.channel, post.ts, post.link, post.posted_at, post.reaction, post.reacted_at],
    )?;
    Ok(rows == 1)
}
