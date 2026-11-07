//! The migration `swr` (swr.md §3; contract §4.4): one additive migration creates every SWR table.

use super::*;
use crate::error::CpdError;
use crate::store::Store;

const MIGRATION: &str = "-swr";

/// The tables of swr.md §3 and their columns in column order.
const TABLES: &[(&str, &[&str])] = &[
    (
        "swr_queries",
        &[
            "key",
            "system",
            "status",
            "data_hash",
            "fetched_at",
            "invalidated",
            "stale_after",
            "last_error",
            "error_count",
            "inactive_at",
        ],
    ),
    (
        "swr_endpoints",
        &[
            "endpoint",
            "status",
            "fetch_status",
            "paused_until",
            "failing_since",
            "reported_at",
            "last_error",
            "error_count",
            "last_ok_at",
            "rate_remaining",
            "rate_reset_at",
        ],
    ),
    (
        "swr_mutations",
        &[
            "id",
            "system",
            "target_key",
            "op_kind",
            "vars_json",
            "patch_json",
            "status",
            "submitted_at",
            "started_at",
            "completed_at",
            "next_attempt_at",
            "attempt_count",
            "error",
            "context_json",
        ],
    ),
    ("swr_viewer", &["id", "login", "fetched_at"]),
    (
        "swr_repos",
        &[
            "repo",
            "default_branch",
            "is_archived",
            "auto_merge_allowed",
            "squash_merge_allowed",
            "merge_commit_allowed",
            "rebase_merge_allowed",
            "delete_branch_on_merge",
            "fetched_at",
            "inactive_at",
        ],
    ),
    (
        "swr_pr_detail",
        &[
            "repo",
            "number",
            "head_oid",
            "updated_at",
            "hydrated_at",
            "detail_json",
            "detail_hash",
        ],
    ),
    (
        "swr_deploys",
        &[
            "repo",
            "run_id",
            "updated_at",
            "run_json",
            "pending_json",
            "inactive_at",
        ],
    ),
    (
        "gh_issues",
        &[
            "repo",
            "number",
            "state",
            "title",
            "updated_at",
            "data_json",
            "fetched_at",
            "inactive_at",
        ],
    ),
    (
        "swr_slack_posts",
        &[
            "channel",
            "ts",
            "repo",
            "number",
            "primary_pr",
            "author_id",
            "author",
            "author_is_me",
            "reply_count",
            "latest_reply",
            "reactions_json",
            "claim_json",
            "eyes_me",
            "thread_tags_me",
            "fetched_at",
            "inactive_at",
        ],
    ),
    (
        "swr_jira_issues",
        &[
            "ticket",
            "cloud_id",
            "summary",
            "status_name",
            "status_category",
            "estimates_json",
            "synced_at",
            "inactive_at",
        ],
    ),
    (
        "swr_atlassian",
        &["id", "cloud_id", "fetched_at", "account_id"],
    ),
    (
        "swr_models",
        &[
            "provider",
            "catalog_json",
            "changes_json",
            "polled_at",
            "last_error",
            "inactive_at",
        ],
    ),
    (
        "swr_badges",
        &[
            "session_id",
            "entry_id",
            "label",
            "score",
            "text",
            "tone",
            "others_json",
            "synced_at",
            "inactive_at",
        ],
    ),
];

/// The column names of `table` in column order (empty when the table does not exist).
fn columns(store: &mut Store, table: &str) -> Vec<String> {
    store
        .read(|c| {
            let mut stmt = c.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")?;
            Ok(stmt
                .query_map([table], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?)
        })
        .unwrap()
}

const PULL_REQUEST: &str = "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
     VALUES ('acme/api', 7, 'https://x/7', 'Title 7', 0, 'open', 'none', 'passed', 't1', 't2')";

fn pull_requests(store: &mut Store) -> Vec<String> {
    store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT json_array(id, repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, merged_at)
                 FROM pull_requests ORDER BY id",
            )?;
            Ok(stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?)
        })
        .unwrap()
}

#[test]
fn swr_migration_creates_every_table_with_its_columns() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    for (table, expected) in TABLES {
        assert_eq!(&columns(&mut store, table), expected, "{table}");
    }
}

#[test]
fn swr_migration_is_additive_and_leaves_pull_requests_rows_untouched() {
    let entry = NAMED
        .iter()
        .find(|n| n.name.ends_with(MIGRATION))
        .expect("the swr migration is a built-in migration");
    assert_eq!(entry.sql.lines().next(), Some("-- cpd-migration: additive"));
    assert!(
        entry.compat == Compat::Additive,
        "the migration is additive"
    );

    let (_dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with(MIGRATION))
        .collect();
    let mut old = crate::store::tests::open_named(&home, &earlier).unwrap();
    old.write("fixture", |tx| {
        tx.execute(PULL_REQUEST, [])?;
        Ok::<(), CpdError>(())
    })
    .unwrap();
    let before = pull_requests(&mut old);
    assert_eq!(before.len(), 1);
    drop(old);

    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(pull_requests(&mut store), before, "the row is untouched");
    assert!(
        !columns(&mut store, "swr_queries").is_empty(),
        "the migration ran on the old store"
    );
}
