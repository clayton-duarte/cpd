//! The Slack reaction queue `slack_reactions` (contract §4.4; commands.md §21.9; pro-d107, pro-d110): any step appends,
//! the engine drains oldest first. A data row: it appends no Event and is never deleted.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::home::CpdHome;
use crate::ids::{OwnerLogin, PlanId, RepoName, WorkflowKey};
use crate::policy::{policy_dir, repo_path};
use crate::store::WriteTxn;

/// A queued reaction that is due.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    pub id: i64,
    pub channel: String,
    pub ts: String,
    pub emoji: String,
    /// The Workflow whose merge this reaction is; `None` for any other reaction.
    pub merge: Option<MergeOf>,
}

/// The Workflow whose merge a queued reaction is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeOf {
    pub plan: String,
    pub workflow: String,
}

/// The emoji the Workflow's policy names (`""` when none) and the policy file that sets, or should set, `pr.post.merged_reaction`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyEmoji {
    pub emoji: String,
    pub file: PathBuf,
}

fn due(r: &rusqlite::Row<'_>) -> rusqlite::Result<Due> {
    let merge = match (
        r.get::<_, Option<String>>(4)?,
        r.get::<_, Option<String>>(5)?,
    ) {
        (Some(plan), Some(workflow)) => Some(MergeOf { plan, workflow }),
        _ => None,
    };
    Ok(Due {
        id: r.get(0)?,
        channel: r.get(1)?,
        ts: r.get(2)?,
        emoji: r.get(3)?,
        merge,
    })
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

/// Queues the reaction of Workflow `pr` of Plan `plan`'s post when its `pr_posts.reaction` is not NULL; `true` when newly queued.
pub fn queue_merge(tx: &WriteTxn<'_>, plan: &str, workflow: &str) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "INSERT INTO slack_reactions (channel, ts, emoji, queued_at, plan, workflow)
         SELECT channel, ts, reaction, (SELECT at FROM txns WHERE id = cpd_txn()), plan, workflow FROM pr_posts
          WHERE plan = ?1 AND workflow = ?2 AND reaction IS NOT NULL
         ON CONFLICT (channel, ts, emoji) DO NOTHING",
        [plan, workflow],
    )?;
    Ok(rows == 1)
}

/// The oldest row neither reacted nor refused whose id is not in `skip`.
pub fn next_due(conn: &Connection, skip: &[i64]) -> Result<Option<Due>, CpdError> {
    let skip = serde_json::to_string(skip).expect("integers serialize");
    Ok(conn
        .query_row(
            "SELECT id, channel, ts, emoji, plan, workflow FROM slack_reactions
             WHERE reacted_at IS NULL AND refused_at IS NULL AND id NOT IN (SELECT value FROM json_each(?1)) ORDER BY id LIMIT 1",
            [skip],
            due,
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

/// The refused merge rows (`plan` and `pr` set), oldest first; `emoji` is the refused emoji.
pub fn parked(conn: &Connection) -> Result<Vec<Due>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, channel, ts, emoji, plan, workflow FROM slack_reactions WHERE refused_at IS NOT NULL AND plan IS NOT NULL ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], due)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Sets the emoji of a row that is neither reacted nor refused; `false` when nothing changed.
pub fn set_emoji(tx: &WriteTxn<'_>, id: i64, emoji: &str) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "UPDATE slack_reactions SET emoji = ?2 WHERE id = ?1 AND reacted_at IS NULL AND refused_at IS NULL",
        rusqlite::params![id, emoji],
    )?;
    Ok(rows == 1)
}

/// Reopens a refused row: sets its emoji and clears `refused_at` and `refusal`; `false` when it was not refused.
pub fn reopen(tx: &WriteTxn<'_>, id: i64, emoji: &str) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "UPDATE slack_reactions SET emoji = ?2, refused_at = NULL, refusal = NULL WHERE id = ?1 AND refused_at IS NOT NULL",
        rusqlite::params![id, emoji],
    )?;
    Ok(rows == 1)
}

/// Parks a row that is neither reacted nor refused (`refused_at` = the transaction's time, `refusal` = `reason`) and appends the
/// one `error` Event `slack_refused` in the same transaction (`source` is its source); `false`, nothing written, when the row was
/// already parked or reacted. `file` is the policy file to fix, for a merge reaction.
pub fn refuse(
    tx: &WriteTxn<'_>,
    source: &str,
    id: i64,
    reason: &str,
    file: Option<&Path>,
) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "UPDATE slack_reactions SET refused_at = (SELECT at FROM txns WHERE id = cpd_txn()), refusal = ?2
          WHERE id = ?1 AND reacted_at IS NULL AND refused_at IS NULL",
        rusqlite::params![id, reason],
    )?;
    if rows != 1 {
        return Ok(false);
    }
    let row = tx.query_row(
        "SELECT id, channel, ts, emoji, plan, workflow FROM slack_reactions WHERE id = ?1",
        [id],
        due,
    )?;
    let emoji = &row.emoji;
    let mut details =
        json!({"id": id, "emoji": emoji, "reason": reason, "channel": row.channel, "ts": row.ts});
    let what = if reason == "policy_empty" {
        "The policy names no emoji (pr.post.merged_reaction is empty)".to_owned()
    } else {
        format!("Slack refused the reaction `{emoji}`: {reason}")
    };
    let text = match (&row.merge, file) {
        (Some(merge), Some(file)) => {
            details["plan"] = json!(merge.plan);
            details["pr"] = json!(merge.workflow);
            details["file"] = json!(file.display().to_string());
            format!(
                "The merge reaction of Workflow {} of Plan {} on Slack message {} of {} was not placed. {what}. Set pr.post.merged_reaction in {} to an emoji of the workspace; the engine tries again when the policy names another emoji.",
                merge.workflow,
                merge.plan,
                row.ts,
                row.channel,
                file.display()
            )
        }
        _ => format!(
            "The reaction `{emoji}` on Slack message {} of {} was not placed. {what}. The engine does not try this reaction again.",
            row.ts, row.channel
        ),
    };
    let mut event = NewEvent::error(
        text.clone(),
        source,
        ErrorCode::SlackRefused.as_str(),
        &text,
        details,
    );
    if let Some(merge) = &row.merge {
        event = event.plan(PlanId::parse(&merge.plan)?);
    }
    append(tx, event)?;
    Ok(true)
}

/// The emoji Workflow `pr` of `plan` names for its merge (`pr.post.merged_reaction`, `""` when none), resolved with the Workflow's repo and
/// owner policy, and the file to edit: the repo's file when it sets the key, else the owner's `owner.toml`.
/// A policy that does not resolve is its error, unchanged.
pub fn policy_emoji(
    conn: &Connection,
    home: &CpdHome,
    plan: &str,
    workflow: &str,
) -> Result<PolicyEmoji, CpdError> {
    let plan_id = PlanId::parse(plan)?;
    let key = WorkflowKey::parse(workflow)?;
    let emoji = super::policy_of_workflow(conn, home, &plan_id, &key)?
        .pr
        .post
        .merged_reaction;
    let (owner, name): (String, Option<String>) = conn.query_row(
        "SELECT c.owner, r.name FROM workflows r JOIN plans p ON p.id = r.plan JOIN contexts c ON c.id = p.context WHERE r.plan = ?1 AND r.key = ?2",
        [plan, workflow],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let owner = OwnerLogin::parse(&owner)?;
    let owner_file = policy_dir(home).join(owner.as_str()).join("owner.toml");
    let file = match name {
        Some(name) => {
            let repo = repo_path(home, &owner, &RepoName::parse(&name)?);
            if repo_sets_reaction(&repo)? {
                repo
            } else {
                owner_file
            }
        }
        None => owner_file,
    };
    Ok(PolicyEmoji { emoji, file })
}

/// Whether the repo policy file exists and sets `pr.post.merged_reaction` (it parsed when the policy resolved).
fn repo_sets_reaction(path: &Path) -> Result<bool, CpdError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(CpdError::new(
                ErrorCode::Internal,
                format!("cannot read {}: {e}", path.display()),
            ));
        }
    };
    let value: toml::Table = text.parse().map_err(|e| {
        CpdError::new(
            ErrorCode::Internal,
            format!("{} is not TOML: {e}", path.display()),
        )
    })?;
    Ok(value
        .get("pr")
        .and_then(|v| v.get("post"))
        .and_then(|v| v.get("merged_reaction"))
        .is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
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
                       VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                     INSERT INTO slack_reactions (channel, ts, emoji, queued_at, plan, workflow) VALUES ('C0POST', '1.1', 'git-merge', 'q', 'pay', 'api');
                     INSERT INTO slack_reactions (channel, ts, emoji, queued_at) VALUES ('C0POST', '1.1', 'eyes', 'q');",
                )?;
                Ok(())
            })
            .unwrap();
        (dir, store)
    }

    #[test]
    fn a_refused_row_is_parked_once_with_one_event_and_reopens_with_another_emoji() {
        let (_dir, mut store) = store();
        let file = Path::new("/h/policy/acme/owner.toml");
        assert!(
            store
                .write("t", |tx| refuse(tx, "watch", 1, "invalid_name", Some(file)))
                .unwrap()
        );
        assert!(
            !store
                .write("t", |tx| refuse(tx, "watch", 1, "invalid_name", Some(file)))
                .unwrap(),
            "already parked: nothing written"
        );
        let (events, plan): (i64, Option<String>) = store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT count(*), max(plan) FROM events WHERE kind = 'error'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!((events, plan.as_deref()), (1, Some("pay")));
        assert_eq!(
            store.read(|c| next_due(c, &[])).unwrap().map(|d| d.id),
            Some(2),
            "a parked row is never due"
        );
        let parked_rows = store.read(parked).unwrap();
        assert_eq!(parked_rows.len(), 1);
        assert_eq!(
            parked_rows[0].merge,
            Some(MergeOf {
                plan: "pay".into(),
                workflow: "api".into()
            })
        );
        assert!(
            !store.write("t", |tx| set_emoji(tx, 1, "x")).unwrap(),
            "a refused row keeps its emoji"
        );
        assert!(store.write("t", |tx| reopen(tx, 1, "git-merged")).unwrap());
        assert_eq!(
            store
                .read(|c| next_due(c, &[]))
                .unwrap()
                .map(|d| (d.id, d.emoji)),
            Some((1, "git-merged".to_owned()))
        );
        assert!(store.write("t", |tx| set_emoji(tx, 1, "y")).unwrap());
    }
}
