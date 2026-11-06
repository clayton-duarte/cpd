//! `team::gh_pr` through its one public door, `lookup_from_store`: the rows the pass wrote, the `GhPr` the rule classifies.

use rusqlite::params;

use super::team::{GhPr, lookup_from_store};
use crate::home::CpdHome;
use crate::store::Store;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, store)
}

/// One open pull request `acme/pay#n` the User did not write, with its stored `checks`, an optional stored author and the
/// rollup state of its detail (when `rollup` is `Some`).
fn seed(store: &mut Store, n: i64, checks: &str, author: Option<&str>, rollup: Option<&str>) {
    let (checks, author, rollup) = (
        checks.to_owned(),
        author.map(str::to_owned),
        rollup.map(str::to_owned),
    );
    store
        .write("seed", move |tx| {
            tx.execute(
                "INSERT INTO swr_viewer (id, login, fetched_at) VALUES (1, 'me', 't')
                 ON CONFLICT (id) DO NOTHING",
                [],
            )?;
            tx.execute(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at,
                                            changed_at, authored, review_requested, head_oid, author_login)
                 VALUES ('acme/pay', ?1, 'u', 't', 0, 'open', 'none', ?2, 'c', 't', 0, 1, 'h1', ?3)",
                params![n, checks, author],
            )?;
            if let Some(state) = rollup {
                let detail = serde_json::json!({"commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": state}}}]}});
                tx.execute(
                    "INSERT INTO swr_pr_detail (repo, number, head_oid, updated_at, hydrated_at, detail_json, detail_hash)
                     VALUES ('acme/pay', ?1, 'h1', 'u', 't', ?2, 'x')",
                    params![n, detail.to_string()],
                )?;
            }
            Ok::<(), crate::error::CpdError>(())
        })
        .unwrap();
}

fn read(store: &mut Store, n: i64) -> GhPr {
    lookup_from_store(store, &[("acme/pay".into(), n as u64)])
        .unwrap()
        .workflows
        .remove(&("acme/pay".to_owned(), n as u64))
        .unwrap()
        .unwrap()
}

#[test]
fn inbox_team_gh_pr_takes_checks_state_from_the_effective_checks_first() {
    let (_dir, mut store) = store();
    // A Friday: the raw rollup stays FAILURE, the effective checks are `passed` (the Danger is excused).
    seed(&mut store, 1, "passed", Some("ryan"), Some("FAILURE"));
    seed(&mut store, 2, "failed", Some("ryan"), Some("SUCCESS"));
    seed(&mut store, 3, "pending", Some("ryan"), Some("SUCCESS"));
    seed(&mut store, 4, "none", Some("ryan"), Some("SUCCESS"));
    let states: Vec<Option<String>> = (1..=4).map(|n| read(&mut store, n).checks_state).collect();
    assert_eq!(
        states,
        [
            Some("SUCCESS".to_owned()),
            Some("FAILURE".to_owned()),
            Some("PENDING".to_owned()),
            None
        ],
        "passed, failed, pending, none: the effective checks decide, not the detail's rollup"
    );
}

#[test]
fn inbox_team_gh_pr_uses_the_real_author_not_someone() {
    let (_dir, mut store) = store();
    seed(&mut store, 1, "passed", Some("ryan"), None);
    assert_eq!(read(&mut store, 1).author, "ryan");
}
