//! The table `pr_posts` (contract §4.4; Workflow post, pro-d64, pro-d65): the one post of a Workflow for review in Slack.
//! Written once by `pr post`; no later write (the reaction is queued in `slack_reactions`).
//! `reaction` only records that a reaction was wanted at post time (NULL = none); it is never the emoji used. A data row: it appends no Event.

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
    /// Whether a reaction was wanted at post time (`None` = none): the emoji name the policy had then, never the emoji that will be
    /// used (the engine resolves that from the Workflow's policy when it places the reaction).
    pub reaction: Option<String>,
    /// When the reaction was added; `None` until then.
    pub reacted_at: Option<String>,
    /// The auto-merge request: `none`, `pending`, `enabled` or `refused` (contract §4.4).
    pub auto_merge: String,
    /// When the request became `enabled` or `refused`; `None` before.
    pub auto_merge_at: Option<String>,
}

/// The post of Workflow `pr` of `plan`, if it has one.
pub fn find(conn: &Connection, plan: &str, workflow: &str) -> Result<Option<PostRecord>, CpdError> {
    Ok(conn
        .query_row(
            "SELECT channel, ts, link, posted_at, reaction, reacted_at, auto_merge, auto_merge_at FROM pr_posts WHERE plan = ?1 AND workflow = ?2",
            [plan, workflow],
            |r| {
                Ok(PostRecord { channel: r.get(0)?, ts: r.get(1)?, link: r.get(2)?, posted_at: r.get(3)?, reaction: r.get(4)?, reacted_at: r.get(5)?, auto_merge: r.get(6)?, auto_merge_at: r.get(7)? })
            },
        )
        .optional()?)
}

/// Writes the post of Workflow `pr` of `plan`, its auto-merge request stored as `pending`; `false` when the Workflow already has one (nothing is written).
pub fn insert(tx: &WriteTxn<'_>, plan: &str, workflow: &str, post: &PostRecord) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, reaction, reacted_at, auto_merge) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending')
         ON CONFLICT (plan, workflow) DO NOTHING",
        rusqlite::params![plan, workflow, post.channel, post.ts, post.link, post.posted_at, post.reaction, post.reacted_at],
    )?;
    Ok(rows == 1)
}

/// Sets the auto-merge request of a `pending` post to `state` (`enabled`, `refused` or `none`); `enabled` and `refused` set
/// `auto_merge_at` to the transaction's time. `false` when the post is not `pending` (nothing changed).
pub fn set_auto_merge(tx: &WriteTxn<'_>, plan: &str, workflow: &str, state: &str) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "UPDATE pr_posts SET auto_merge = ?3,
           auto_merge_at = IIF(?3 IN ('enabled', 'refused'), (SELECT at FROM txns WHERE id = cpd_txn()), NULL)
         WHERE plan = ?1 AND workflow = ?2 AND auto_merge = 'pending'",
        [plan, workflow, state],
    )?;
    Ok(rows == 1)
}

/// A post whose auto-merge request waits (`pending`) for a Workflow that can take it now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub plan: String,
    pub workflow: String,
    /// The GitHub repo, `owner/name`.
    pub repo: String,
    pub number: i64,
}

/// The oldest-posted `pending` request whose Workflow is `open`, whose Plan is not archived and not held by incident mode, and
/// which is not stacked (its `base` is no `prs.lead_branch`; the rule of `auto_merge::stacked`), if any.
pub fn next_pending(conn: &Connection) -> Result<Option<Pending>, CpdError> {
    Ok(conn
        .query_row(
            "SELECT p.plan, p.workflow, q.repo, q.number
             FROM pr_posts p JOIN workflows r ON r.plan = p.plan AND r.key = p.workflow
               JOIN plans l ON l.id = p.plan JOIN pull_requests q ON q.id = r.pull_request
             WHERE p.auto_merge = 'pending' AND r.status = 'open' AND l.archived_at IS NULL
               AND (NOT EXISTS (SELECT 1 FROM incident) OR EXISTS (SELECT 1 FROM incident_plans i WHERE i.plan = p.plan))
               AND NOT EXISTS (SELECT 1 FROM workflows b WHERE b.lead_branch = r.base)
             ORDER BY p.posted_at, p.plan, p.pr LIMIT 1",
            [],
            |r| Ok(Pending { plan: r.get(0)?, workflow: r.get(1)?, repo: r.get(2)?, number: r.get(3)? }),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::CpdHome;
    use crate::store::Store;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        store
            .write("fixture", |tx| {
                tx.execute_batch(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                       VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                       VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');",
                )?;
                Ok(())
            })
            .unwrap();
        (dir, store)
    }

    /// A Workflow `key` with `status`, merged at `merged_at`, and a post with `reaction`.
    fn add(store: &mut Store, key: &str, status: &str, merged_at: &str, reaction: Option<&str>) {
        store
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                     VALUES ('a/b', (SELECT count(*) + 1 FROM pull_requests), 'u', 't', 0, 'open', 'none', 'none', 't', 't')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, pr_number, pr_url, pr_opened_at, pull_request, merged_at)
                     VALUES ('pay', ?1, '/r', 'main', ?2, 'cpd/pay/' || ?1, IIF(?2 IN ('waiting', 'cancelled'), NULL, '/r/lead'), cpd_txn(), 't', IIF(?2 IN ('waiting', 'cancelled'), NULL, 't'), 7, 'u', 't', IIF(?2 IN ('open', 'merged'), (SELECT max(id) FROM pull_requests), NULL), NULLIF(?3, ''))",
                    rusqlite::params![key, status, merged_at],
                )?;
                let post = PostRecord {
                    channel: "C0POLICY1".into(),
                    ts: format!("1793040197.{key:0>6}"),
                    link: "l".into(),
                    posted_at: "t".into(),
                    reaction: reaction.map(str::to_owned),
                    reacted_at: None,
                    auto_merge: String::new(),
                    auto_merge_at: None,
                };
                insert(tx, "pay", key, &post)?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn a_post_starts_pending_and_set_auto_merge_settles_it_once() {
        let (_dir, mut store) = store();
        add(&mut store, "a", "open", "", None);
        let post = store.read(|c| find(c, "pay", "a")).unwrap().unwrap();
        assert_eq!((post.auto_merge.as_str(), post.auto_merge_at), ("pending", None));
        assert!(store.write("set", |tx| set_auto_merge(tx, "pay", "a", "enabled")).unwrap());
        let post = store.read(|c| find(c, "pay", "a")).unwrap().unwrap();
        assert_eq!(post.auto_merge, "enabled");
        assert!(post.auto_merge_at.is_some());
        assert!(!store.write("set", |tx| set_auto_merge(tx, "pay", "a", "refused")).unwrap());
    }
}
