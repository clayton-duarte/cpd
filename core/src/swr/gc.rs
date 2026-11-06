//! Eviction (swr.md §4): rows inactive past gcTime (`swr.gc_days`) with no observer.

use std::time::Duration;

use rusqlite::params;
use time::OffsetDateTime;

use crate::clock::timestamp;
use crate::error::CpdError;
use crate::store::Store;

/// Tables whose rows carry `inactive_at` and have no observer rule.
const INACTIVE_TABLES: [&str; 7] = [
    "swr_repos",
    "swr_deploys",
    "gh_issues",
    "swr_slack_posts",
    "swr_jira_issues",
    "swr_models",
    "swr_badges",
];

/// The key `github:pr:<repo>#<number>` of a `swr_queries` row, as SQL over a table alias `{t}`.
const PR_KEY: &str = "'github:pr:' || {t}.repo || '#' || {t}.{n}";

/// Delete what is inactive past `ttl` and unobserved; `done`/`failed`/`superseded` mutations older than
/// `ttl`. A query row is observed by an active mutation on the key, an open or waiting Workflow whose
/// pull request is the key, or an open Inbox item whose `subject` is the key. A `pull_requests` row of a
/// deleted `github:pr:` key goes too, unless a Workflow still owns it (`pr_posts` is append-only and stays).
/// Returns the number of rows deleted.
pub fn sweep(store: &mut Store, now: OffsetDateTime, ttl: Duration) -> Result<usize, CpdError> {
    let cutoff = timestamp(now - ttl);
    let workflow_key = PR_KEY.replace("{t}", "w").replace("{n}", "pr_number");
    store.write("swr gc", |tx| {
        let mut total = 0;
        // Pull requests first, while the keys still exist.
        total += tx.execute(
            "DELETE FROM pull_requests WHERE 'github:pr:' || repo || '#' || number IN (
               SELECT key FROM swr_queries q WHERE q.inactive_at < ?1
                 AND NOT EXISTS (SELECT 1 FROM swr_mutations m WHERE m.target_key = q.key
                   AND m.status IN ('pending', 'in_flight', 'revalidating'))
                 AND NOT EXISTS (SELECT 1 FROM inbox_items i WHERE i.subject = q.key AND i.closed_at IS NULL))
             AND NOT EXISTS (SELECT 1 FROM workflows w WHERE w.pull_request = pull_requests.id)",
            params![cutoff],
        )?;
        total += tx.execute(
            &format!(
                "DELETE FROM swr_queries WHERE inactive_at < ?1
                 AND NOT EXISTS (SELECT 1 FROM swr_mutations m WHERE m.target_key = swr_queries.key
                   AND m.status IN ('pending', 'in_flight', 'revalidating'))
                 AND NOT EXISTS (SELECT 1 FROM workflows w WHERE w.status IN ('waiting', 'dispatch', 'open')
                   AND {workflow_key} = swr_queries.key)
                 AND NOT EXISTS (SELECT 1 FROM inbox_items i WHERE i.subject = swr_queries.key AND i.closed_at IS NULL)"
            ),
            params![cutoff],
        )?;
        for table in INACTIVE_TABLES {
            total += tx.execute(
                &format!("DELETE FROM {table} WHERE inactive_at < ?1"),
                params![cutoff],
            )?;
        }
        total += tx.execute(
            "DELETE FROM swr_mutations WHERE status IN ('done', 'failed', 'superseded')
             AND coalesce(completed_at, submitted_at) < ?1",
            params![cutoff],
        )?;
        Ok::<usize, CpdError>(total)
    })
}
