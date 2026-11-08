//! The persisted containment flag and the readiness predicate (contract §2.2 "Ready to merge", Plan
//! `merge-uptodate`): a pull request whose base is the default branch and whose head does not contain the current
//! default-branch head is not ready to merge; a stacked pull request is not subject to the containment condition and
//! its readiness is unchanged. Raw SQL over the store: no `gh`, no git.

use crate::home::CpdHome;
use crate::pr::pulls::ready_to_merge_sql;
use crate::store::Store;

/// A migrated store in `dir`, as `pulls`'s own tests open one.
fn store(dir: &tempfile::TempDir) -> Store {
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    Store::open(&home, 5000).unwrap()
}

/// Inserts one open pull request row: `number`, its `base_ref` and `default_branch`, and the persisted containment
/// flag `base_contained` (`None` = NULL, the new `pull_requests` column).
fn insert(store: &mut Store, number: i64, base: &str, default: &str, contained: Option<i64>) {
    store
        .write("test", |tx| {
            tx.execute(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks,
                                            created_at, changed_at, mergeable, merge_state, authored, review_requested,
                                            base_ref, default_branch, archived, base_contained)
                 VALUES ('acme/pay', ?1, 'https://github.com/acme/pay/pull/' || ?1, 'T', 0, 'open', 'none', 'passed',
                         'c', 'c', 'mergeable', 'clean', 1, 0, ?2, ?3, 0, ?4)",
                rusqlite::params![number, base, default, contained],
            )?;
            Ok(())
        })
        .unwrap();
}

/// Whether the readiness predicate holds of the row `number`: `ready_to_merge_sql`, the one definition every reader
/// uses.
fn ready(store: &mut Store, number: i64) -> bool {
    store
        .read(|c| {
            Ok(c.query_row(
                &format!(
                    "SELECT {} FROM pull_requests r WHERE r.number = ?1",
                    ready_to_merge_sql("r")
                ),
                [number],
                |r| r.get::<_, i64>(0),
            )? != 0)
        })
        .unwrap()
}

#[test]
fn a_behind_pr_is_not_ready_to_merge() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store(&dir);
    // A default-branch pull request whose head does not contain the current default-branch head: not ready.
    insert(&mut store, 801, "main", "main", Some(0));
    assert!(
        !ready(&mut store, 801),
        "a pull request whose head is behind main is not ready to merge"
    );
    // The flag is honoured, not merely stored: a head that contains main is ready.
    insert(&mut store, 802, "main", "main", Some(1));
    assert!(
        ready(&mut store, 802),
        "a pull request whose head contains main is ready to merge"
    );
}

#[test]
fn a_stacked_pr_is_not_checked_against_main() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store(&dir);
    // A stacked pull request: its base is another Workflow's Lead Branch, not the default; the poll leaves the flag NULL.
    insert(&mut store, 801, "feature", "main", None);
    assert!(
        !ready(&mut store, 801),
        "a stacked pull request's readiness is unchanged: not ready, its base is not the default branch"
    );
    // The containment condition does not apply to it: even a set flag leaves a stacked pull request not ready.
    insert(&mut store, 802, "feature", "main", Some(1));
    assert!(
        !ready(&mut store, 802),
        "a stacked pull request is not subject to the containment condition"
    );
}
