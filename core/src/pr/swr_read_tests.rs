//! pro-d489: a Light pass (no heavy part) carries only GitHub's coarse rollup state, `FAILURE` while any check is CANCELLED;
//! the stored same-head detail is reduced again. In process: no `gh`, no clock, no process.

use serde_json::{Value, json};

use super::gh::{Listed, Membership, PollNode, PolledPr};
use super::status::{ChecksSummary, MergeState, Mergeable, PrState, ReviewDecision};
use super::swr_read::excuse_from_detail;

fn light_node(checks: ChecksSummary) -> PollNode {
    PollNode {
        listed: Listed {
            pr: PolledPr {
                repo: "acme/pay".into(),
                number: 801,
                url: "https://github.com/acme/pay/pull/801".into(),
                title: "t".into(),
                draft: false,
                state: PrState::Open,
                review_decision: ReviewDecision::None,
                checks,
                mergeable: Mergeable::Clean,
                merge_state: MergeState::Clean,
                created_at: "2026-10-30T00:00:00Z".into(),
                updated_at: "2026-10-30T00:00:00Z".into(),
                merged_at: None,
                base_ref: Some("main".into()),
                default_branch: Some("main".into()),
                open_threads: vec![],
                archived: false,
                head_oid: Some("a1".into()),
            },
            membership: Membership::default(),
        },
        head_oid: "a1".into(),
        threads_complete: true,
        auto_merge_requested: false,
        author_login: None,
        reviews: json!([]),
        labels: json!([]),
        heavy: None,
    }
}

fn detail(runs: &[(&str, &str)]) -> Value {
    let nodes: Vec<Value> = runs
        .iter()
        .map(|(name, conclusion)| {
            json!({"__typename": "CheckRun", "name": name, "conclusion": conclusion,
                   "startedAt": "2026-10-30T01:00:00Z"})
        })
        .collect();
    json!({"commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": nodes}}}}]}})
}

#[test]
fn light_pass_cancelled_check_is_not_failed() {
    let mut node = light_node(ChecksSummary::Failed);
    let d = detail(&[("build", "SUCCESS"), ("deploy", "CANCELLED")]);
    excuse_from_detail(&mut node, &d, &[]);
    assert_ne!(node.listed.pr.checks, ChecksSummary::Failed);
}

#[test]
fn light_pass_truly_failed_check_stays_failed() {
    let mut node = light_node(ChecksSummary::Failed);
    let d = detail(&[("build", "SUCCESS"), ("test", "FAILURE")]);
    excuse_from_detail(&mut node, &d, &[]);
    assert_eq!(node.listed.pr.checks, ChecksSummary::Failed);
}

#[test]
fn passed_checks_normalize_unstable_to_clean() {
    let mut node = light_node(ChecksSummary::Failed);
    node.listed.pr.merge_state = MergeState::Unstable;
    let d = detail(&[("gate", "SUCCESS"), ("e2e", "CANCELLED")]);
    excuse_from_detail(&mut node, &d, &[]);
    assert_eq!(node.listed.pr.checks, ChecksSummary::Passed);
    assert_eq!(node.listed.pr.merge_state, MergeState::Clean);
}

#[test]
fn failed_check_keeps_unstable() {
    let mut node = light_node(ChecksSummary::Failed);
    node.listed.pr.merge_state = MergeState::Unstable;
    let d = detail(&[("gate", "FAILURE"), ("e2e", "CANCELLED")]);
    excuse_from_detail(&mut node, &d, &[]);
    assert_eq!(node.listed.pr.merge_state, MergeState::Unstable);
}

#[test]
fn passed_checks_keep_behind() {
    let mut node = light_node(ChecksSummary::Failed);
    node.listed.pr.merge_state = MergeState::Behind;
    let d = detail(&[("gate", "SUCCESS"), ("e2e", "CANCELLED")]);
    excuse_from_detail(&mut node, &d, &[]);
    assert_eq!(node.listed.pr.merge_state, MergeState::Behind);
}

#[test]
fn incomplete_metadata_never_clears_the_merge_gate_and_unchanged_reads_backfill() {
    use super::swr_read::{review_metadata_held, write_header, write_review_metadata};
    use crate::home::CpdHome;
    use crate::store::Store;
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let mut node = light_node(ChecksSummary::Passed);
    node.threads_complete = false;
    node.reviews = json!({"nodes": [{"author": {"login": "bot"}}]});
    store.write("test", |tx| write_header(tx, &node)).unwrap();
    assert!(
        store
            .read(|c| super::pulls::unresolved_threads(c, "acme/pay", 801))
            .is_err()
    );
    node.threads_complete = true;
    node.listed.pr.open_threads = vec!["T1".into()];
    store
        .write("test", |tx| write_review_metadata(tx, &node))
        .unwrap();
    assert_eq!(
        store
            .read(|c| super::pulls::unresolved_threads(c, "acme/pay", 801))
            .unwrap(),
        1
    );
    node.threads_complete = false;
    node.listed.pr.open_threads.clear();
    store
        .write("test", |tx| write_review_metadata(tx, &node))
        .unwrap();
    assert!(
        store
            .read(|c| super::pulls::unresolved_threads(c, "acme/pay", 801))
            .is_err()
    );
    let threads: String = store
        .read(|c| Ok(c.query_row("SELECT open_threads FROM pull_requests", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(threads, "[\"T1\"]");
    node.threads_complete = true;
    store
        .write("old cache", |tx| {
            tx.execute(
                "UPDATE pull_requests SET reviewers = NULL, rereview_head = NULL",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(!store.read(|c| review_metadata_held(c, &node)).unwrap());
    store
        .write("backfill", |tx| write_review_metadata(tx, &node))
        .unwrap();
    assert!(store.read(|c| review_metadata_held(c, &node)).unwrap());
    assert_eq!(
        store
            .read(|c| super::pulls::unresolved_threads(c, "acme/pay", 801))
            .unwrap(),
        0
    );
    let head: String = store
        .read(|c| Ok(c.query_row("SELECT rereview_head FROM pull_requests", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(head, "a1");
    for invalid in ["not json", "{}", "null"] {
        store
            .write("invalid metadata", |tx| {
                tx.execute(
                    "UPDATE pull_requests SET open_threads = ?1, review_threads_complete = 1",
                    [invalid],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(
            store
                .read(|c| super::pulls::unresolved_threads(c, "acme/pay", 801))
                .is_err()
        );
        let ready = store
            .read(|c| {
                Ok(c.query_row(
                    &format!(
                        "SELECT {} FROM pull_requests r",
                        super::pulls::no_open_thread_sql("r")
                    ),
                    [],
                    |r| r.get::<_, bool>(0),
                )?)
            })
            .unwrap();
        assert!(!ready, "invalid thread metadata cannot clear readiness");
    }
}
