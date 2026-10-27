//! The Slack reaction queue `slack_reactions` (contract §4.4; commands.md §21.9; pro-d107, pro-d110).
//! STUB: every body is wrong on purpose (the red tests of Job `queue-tests`); the implementation Job replaces them.

use rusqlite::Connection;

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
pub fn queue(_tx: &WriteTxn<'_>, _channel: &str, _ts: &str, _emoji: &str) -> Result<bool, CpdError> {
    Ok(false)
}

/// Queues the reaction of PR `pr` of Plan `plan`'s post when its `pr_posts.reaction` is not NULL; `true` when newly queued.
pub fn queue_merge(_tx: &WriteTxn<'_>, _plan: &str, _pr: &str) -> Result<bool, CpdError> {
    Ok(false)
}

/// The oldest row with `reacted_at` NULL whose id is not in `skip`.
pub fn next_due(_conn: &Connection, _skip: &[i64]) -> Result<Option<Due>, CpdError> {
    Ok(None)
}

/// Sets the row's `reacted_at` to the transaction's time where it is still NULL; `false` when nothing changed.
pub fn mark_reacted(_tx: &WriteTxn<'_>, _id: i64) -> Result<bool, CpdError> {
    Ok(false)
}
