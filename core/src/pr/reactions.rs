//! The Slack reaction queue `slack_reactions` (contract §4.4; commands.md §21.9; pro-d107, pro-d110): any step appends,
//! the engine drains oldest first. A data row: it appends no Event and is never deleted.

use rusqlite::{Connection, OptionalExtension};

use crate::error::CpdError;
use crate::store::WriteTxn;

/// A queued reaction that is due.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    pub id: i64,
    pub channel: String,
    pub ts: String,
    pub emoji: String,
}

/// Queues `emoji` on the post (`channel`, `ts`); `true` when a row was newly queued, `false` when it exists.
pub fn queue(tx: &WriteTxn<'_>, channel: &str, ts: &str, emoji: &str) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "INSERT INTO slack_reactions (channel, ts, emoji, queued_at) VALUES (?1, ?2, ?3, (SELECT at FROM txns WHERE id = cpd_txn()))
         ON CONFLICT (channel, ts, emoji) DO NOTHING",
        [channel, ts, emoji],
    )?;
    Ok(rows == 1)
}

/// Queues the reaction of PR `pr` of Plan `plan`'s post when its `pr_posts.reaction` is not NULL; `true` when newly queued.
pub fn queue_merge(tx: &WriteTxn<'_>, plan: &str, pr: &str) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "INSERT INTO slack_reactions (channel, ts, emoji, queued_at)
         SELECT channel, ts, reaction, (SELECT at FROM txns WHERE id = cpd_txn()) FROM pr_posts
          WHERE plan = ?1 AND pr = ?2 AND reaction IS NOT NULL
         ON CONFLICT (channel, ts, emoji) DO NOTHING",
        [plan, pr],
    )?;
    Ok(rows == 1)
}

/// The oldest row with `reacted_at` NULL whose id is not in `skip`.
pub fn next_due(conn: &Connection, skip: &[i64]) -> Result<Option<Due>, CpdError> {
    let skip = serde_json::to_string(skip).expect("integers serialize");
    Ok(conn
        .query_row(
            "SELECT id, channel, ts, emoji FROM slack_reactions
             WHERE reacted_at IS NULL AND id NOT IN (SELECT value FROM json_each(?1)) ORDER BY id LIMIT 1",
            [skip],
            |r| Ok(Due { id: r.get(0)?, channel: r.get(1)?, ts: r.get(2)?, emoji: r.get(3)? }),
        )
        .optional()?)
}

/// Sets the row's `reacted_at` to the transaction's time where it is still NULL; `false` when nothing changed.
pub fn mark_reacted(tx: &WriteTxn<'_>, id: i64) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "UPDATE slack_reactions SET reacted_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1 AND reacted_at IS NULL",
        [id],
    )?;
    Ok(rows == 1)
}
