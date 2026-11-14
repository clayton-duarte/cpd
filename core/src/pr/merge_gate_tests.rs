//! An unresolved review thread keeps a pull request out of merge-ready and out of `pr merge-green` (pro-d588).
//! The stored `feedback` holds the open items; `[]` = all addressed (pro-d69). Raw SQL over the store and an in-process
//! rig: `gh` is a counting fake, no process, no network.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::home::CpdHome;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::ports::Ports;
use crate::pr::pulls::ready_to_merge_sql;
use crate::store::Store;

use clap::Parser;

use super::merge::MergeArgs;

#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    args: MergeArgs,
}

const THREAD: &str = r#"[{"id":"T1","kind":"thread","last":"C1","author":"codex","bot":true,"path":"core/src/swr/source/github.rs","outdated":false,"url":"https://github.com/acme/pay/pull/801#discussion_r1","excerpt":"P2"}]"#;
const OUTDATED_THREAD: &str = r#"[{"id":"T1","kind":"thread","last":"C1","author":"codex","bot":true,"path":"a.rs","outdated":true,"url":"https://github.com/acme/pay/pull/801#discussion_r1","excerpt":"P2"}]"#;

/// Inserts the otherwise-ready pull request `number` with the stored `feedback`.
fn insert(store: &mut Store, number: i64, feedback: &str) {
    store
        .write("test", |tx| {
            tx.execute(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks,
                                            created_at, changed_at, mergeable, merge_state, authored, review_requested,
                                            base_ref, default_branch, archived, base_contained, head_oid, feedback, open_threads, review_threads_complete)
                 VALUES ('acme/pay', ?1, 'https://github.com/acme/pay/pull/' || ?1, 'T', 0, 'open', 'none', 'passed',
                         'c', 'c', 'mergeable', 'clean', 1, 0, 'main', 'main', 0, 1, 'abc', ?2, (SELECT json_group_array(json_extract(value, '$.id')) FROM json_each(?2) WHERE json_extract(value, '$.kind') = 'thread'), 1)",
                rusqlite::params![number, feedback],
            )?;
            Ok(())
        })
        .unwrap();
}

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
fn open_thread_blocks_merge_ready() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    insert(&mut store, 801, THREAD);
    insert(&mut store, 802, OUTDATED_THREAD);
    assert!(
        !ready(&mut store, 801),
        "a clean pull request with an open thread is not merge-ready"
    );
    assert!(
        !ready(&mut store, 802),
        "an outdated thread that is still open still blocks"
    );
}

#[test]
fn resolved_threads_do_not_block_merge_ready() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    insert(&mut store, 801, "[]");
    assert!(
        ready(&mut store, 801),
        "a complete read with no open threads is merge-ready"
    );
    // Guard against a vacuous pass: the same row with an open thread must differ.
    insert(&mut store, 802, THREAD);
    assert_ne!(
        ready(&mut store, 801),
        ready(&mut store, 802),
        "an open thread is what separates the two rows"
    );
}

/// Counts every `gh` call; any call fails.
struct CountingGh(Arc<AtomicUsize>);

impl Handler for CountingGh {
    fn handle(&self, call: &Call) -> Reply {
        self.0.fetch_add(1, Ordering::SeqCst);
        Reply::exit(9, "", &format!("gh call: {}\n", call.args.join(" ")))
    }
}

#[test]
fn open_thread_refuses_merge_green() {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    rig.sql(&format!(
        "UPDATE pull_requests SET mergeable = 'mergeable', merge_state = 'clean', base_ref = 'main',
                default_branch = 'main', head_oid = 'abc', checks = 'passed', feedback = '{}' WHERE number = 801;",
        THREAD
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let _ = (*rig.fakes.runner)
        .clone()
        .route("gh", Arc::new(CountingGh(calls.clone())));
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    let args = Cli::try_parse_from(["merge-green", "acme/pay#801"])
        .unwrap()
        .args;
    let err = Ports::scope(&rig.fakes.ports, || {
        super::merge_green::merge_green(&rig.fakes.ports, args)
    })
    .expect_err("an open thread refuses merge-green");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "refused before any gh call: {}",
        err.message()
    );
}

#[test]
fn unread_review_threads_never_clear_merge_readiness() {
    let dir = tempfile::tempdir().unwrap();
    let home = crate::home::CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let mut store = crate::store::Store::open(&home, 5000).unwrap();
    insert(&mut store, 99, "[]");
    store
        .write("unread", |tx| {
            tx.execute(
                "UPDATE pull_requests SET open_threads = NULL, feedback = NULL WHERE number = 99",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(!ready(&mut store, 99));
    let error = store
        .read(|conn| super::pulls::unresolved_threads(conn, "acme/pay", 99))
        .unwrap_err();
    assert_eq!(error.code(), crate::error::ErrorCode::ChecksNotGreen);
    store
        .write("read comments", |tx| {
            tx.execute(
                "UPDATE pull_requests SET review_threads_complete = 1, open_threads = '[]' WHERE number = 99",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(ready(&mut store, 99));
    assert_eq!(
        store
            .read(|conn| super::pulls::unresolved_threads(conn, "acme/pay", 99))
            .unwrap(),
        0
    );
}
