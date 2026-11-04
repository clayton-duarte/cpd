//! The reset after a push (pro-d384): `pulls::reset_after_push` makes the linked row `pending`/`unknown`/`unknown`, so a
//! red -> push -> red raises `checks_failed` again and the Inbox says `checks running` right after a push.

use time::macros::datetime;

use super::gh::{Listed, Membership, PolledPr};
use super::pulls::{self, Excused, PostFacts, Reason};
use super::status::{ChecksSummary, MergeState, Mergeable, PrState, ReviewDecision};
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::{PlanId, WorkflowKey};
use crate::inbox::model::Kind;
use crate::inbox::rules::conditions_with;
use crate::policy::Checks;
use crate::store::{Store, WriteTxn};

const T: &str = "2026-01-01T00:00:00.000Z";

fn store(dir: &tempfile::TempDir) -> (CpdHome, Store) {
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (home, store)
}

fn polled(checks: ChecksSummary) -> PolledPr {
    PolledPr {
        mergeable: Mergeable::Conflicting,
        merge_state: MergeState::Dirty,
        repo: "acme/shop".into(),
        number: 7,
        url: "https://github.com/acme/shop/pull/7".into(),
        title: "T".into(),
        draft: false,
        state: PrState::Open,
        review_decision: ReviewDecision::None,
        checks,
        created_at: T.into(),
        updated_at: T.into(),
        merged_at: None,
        base_ref: Some("main".into()),
        default_branch: Some("main".into()),
        open_threads: vec![],
    }
}

fn poll(tx: &WriteTxn<'_>, checks: ChecksSummary) -> Option<pulls::Change> {
    pulls::upsert(
        tx,
        &Listed {
            pr: polled(checks),
            membership: Membership {
                authored: true,
                review_requested: false,
            },
        },
        None,
    )
    .unwrap()
}

fn reasons(change: &pulls::Change) -> Vec<Reason> {
    pulls::attention(
        change.before.as_ref(),
        &change.after,
        Checks::Any,
        &[],
        PostFacts {
            enabled: false,
            posted: false,
        },
        Excused(false),
    )
}

/// Context `ctx`, Plan `pay` (its Lead idle) and an open Workflow `p7` of it, linked to the pull request `acme/shop#7`.
fn workflow(tx: &WriteTxn<'_>) -> Result<(), CpdError> {
    tx.execute_batch(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'deploy', 'l-pay', '{T}', '{T}');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pull_request, pr_number, pr_url,
                                pr_opened_at, created_txn, created_at, started_at)
           VALUES ('pay', 'p7', '/r', 'main', 'open', 'cpd/pay/p7', '/lw',
                   (SELECT id FROM pull_requests WHERE number = 7), 7, 'https://github.com/acme/shop/pull/7', '{T}', 1, '{T}', '{T}');"
    ))?;
    Ok(())
}

fn reset(store: &mut Store) {
    store
        .write("test", |tx| {
            pulls::reset_after_push(
                tx,
                &PlanId::parse("pay").unwrap(),
                &WorkflowKey::parse("p7").unwrap(),
            )
        })
        .unwrap();
}

fn stored(store: &mut Store) -> (String, String, String) {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT checks, mergeable, merge_state FROM pull_requests WHERE number = 7",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap()
}

#[test]
fn red_push_red_again_raises_second_checks_failed_feedback() {
    let dir = tempfile::tempdir().unwrap();
    let (_home, mut store) = store(&dir);
    store
        .write("test", |tx| {
            poll(tx, ChecksSummary::Pending);
            let red = poll(tx, ChecksSummary::Failed).unwrap();
            assert_eq!(reasons(&red), [Reason::ChecksFailed], "the first red");
            workflow(tx)
        })
        .unwrap();

    reset(&mut store);
    assert_eq!(
        stored(&mut store),
        ("pending".into(), "unknown".into(), "unknown".into())
    );

    // No poll saw `pending` in between: the second red is still a transition.
    let again = store
        .write("test", |tx| Ok(poll(tx, ChecksSummary::Failed)))
        .unwrap()
        .expect("the second red changes the row");
    assert_eq!(
        again.before.as_ref().map(|b| b.checks.as_str()),
        Some("pending")
    );
    assert_eq!(reasons(&again), [Reason::ChecksFailed], "the second red");
}

#[test]
fn inbox_shows_checks_running_right_after_a_push() {
    let dir = tempfile::tempdir().unwrap();
    let (home, mut store) = store(&dir);
    store
        .write("test", |tx| {
            poll(tx, ChecksSummary::Failed);
            workflow(tx)
        })
        .unwrap();
    let label = |store: &mut Store| {
        let all =
            conditions_with(store, &home, datetime!(2026-01-02 00:00 UTC), 20, 1, None).unwrap();
        let row: Vec<_> = all
            .into_iter()
            .filter(|c| c.kind == Kind::PrReady && c.subject == "pr:acme/shop#7")
            .collect();
        assert_eq!(row.len(), 1, "{row:?}");
        row[0].label.split('\n').next().unwrap().to_owned()
    };
    assert_eq!(label(&mut store), "shop#7 conflict", "before the push");

    reset(&mut store);

    assert_eq!(label(&mut store), "shop#7 checks running");
}
