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

/// A post whose reaction is due: its PR merged, the policy named an emoji and `reacted_at` is still NULL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unreacted {
    pub plan: String,
    pub pr: String,
    pub channel: String,
    pub ts: String,
    pub reaction: String,
}

/// The post that waits longest for its reaction (oldest merge first), if any.
pub fn next_unreacted(conn: &Connection) -> Result<Option<Unreacted>, CpdError> {
    Ok(conn
        .query_row(
            "SELECT p.plan, p.pr, p.channel, p.ts, p.reaction FROM pr_posts p JOIN prs r ON r.plan = p.plan AND r.key = p.pr
             WHERE r.status = 'merged' AND p.reaction IS NOT NULL AND p.reacted_at IS NULL
             ORDER BY r.merged_at, p.plan, p.pr LIMIT 1",
            [],
            |r| Ok(Unreacted { plan: r.get(0)?, pr: r.get(1)?, channel: r.get(2)?, ts: r.get(3)?, reaction: r.get(4)? }),
        )
        .optional()?)
}

/// Sets `reacted_at` to the transaction's time where it is still NULL; `false` when nothing changed.
pub fn mark_reacted(tx: &WriteTxn<'_>, plan: &str, pr: &str) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "UPDATE pr_posts SET reacted_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE plan = ?1 AND pr = ?2 AND reacted_at IS NULL",
        [plan, pr],
    )?;
    Ok(rows == 1)
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

    /// A PR `key` with `status`, merged at `merged_at`, and a post with `reaction`.
    fn add(store: &mut Store, key: &str, status: &str, merged_at: &str, reaction: Option<&str>) {
        store
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                     VALUES ('a/b', (SELECT count(*) + 1 FROM pull_requests), 'u', 't', 0, 'open', 'none', 'none', 't', 't')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, pr_number, pr_url, pr_opened_at, pull_request, merged_at)
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
                };
                insert(tx, "pay", key, &post)?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn the_oldest_merged_post_with_a_reaction_is_next() {
        let (_dir, mut store) = store();
        add(&mut store, "open", "open", "", Some("git-merge"));
        add(&mut store, "mute", "merged", "2026-01-01", None);
        add(&mut store, "late", "merged", "2026-03-01", Some("git-merge"));
        add(&mut store, "early", "merged", "2026-02-01", Some("git-merge"));
        let next = store.read(next_unreacted).unwrap().unwrap();
        assert_eq!((next.pr.as_str(), next.reaction.as_str(), next.channel.as_str()), ("early", "git-merge", "C0POLICY1"));
    }

    #[test]
    fn nothing_is_next_without_a_merged_post_with_a_reaction() {
        let (_dir, mut store) = store();
        add(&mut store, "open", "open", "", Some("git-merge"));
        add(&mut store, "mute", "merged", "2026-01-01", None);
        assert_eq!(store.read(next_unreacted).unwrap(), None);
    }

    #[test]
    fn a_marked_post_is_not_next_and_marking_twice_changes_nothing() {
        let (_dir, mut store) = store();
        add(&mut store, "a", "merged", "2026-01-01", Some("git-merge"));
        assert!(store.write("mark", |tx| mark_reacted(tx, "pay", "a")).unwrap());
        assert_eq!(store.read(next_unreacted).unwrap(), None);
        assert!(store.read(|c| find(c, "pay", "a")).unwrap().unwrap().reacted_at.is_some());
        assert!(!store.write("mark", |tx| mark_reacted(tx, "pay", "a")).unwrap());
    }
}
