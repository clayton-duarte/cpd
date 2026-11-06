//! The review half of the two-row label (pro-d171, assumption A3): the `review-requested` label carries the cached GitHub title
//! on Row 2, `plan-review` its review title.

use serde_json::json;
use time::OffsetDateTime;
use time::macros::datetime;

use super::model::{Condition, Kind};
use super::rules::{Team, conditions_with};
use super::team::{GhPr, GhState, Lookup};
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);
const T: &str = "2026-01-01T00:00:00.000Z";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn store() -> (tempfile::TempDir, CpdHome, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, home, store)
}

fn sql(store: &mut Store, statement: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(statement)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

/// A Context `ctx` with Manager `mgr` and Plan `pay` (phase `plan`) led by `l-pay`.
fn fixture() -> (tempfile::TempDir, CpdHome, Store) {
    let (dir, home, mut store) = store();
    sql(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
             INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
               VALUES ('mgr', 'manager', 'ctx', 'Manager', '/', 'O3', 'working', '{T}', '{T}');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('pay', 'ctx', 'Pay', 'plan', 'l-pay', '{T}', NULL, NULL);"
        ),
    );
    (dir, home, store)
}

fn of(store: &mut Store, home: &CpdHome, kind: Kind) -> Vec<Condition> {
    conditions_with(store, home, NOW, 20, 1, None)
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == kind)
        .collect()
}

#[test]
fn a_review_requested_label_has_the_pull_request_title_on_row_two() {
    let (_dir, home, mut store) = store();
    let ts = NOW.unix_timestamp() as f64 - 3600.0;
    let post = json!({"repo": "acme/pay", "number": 812, "ts": ts, "ts_raw": "1.1", "author": "Ryan Case", "primary": true,
                      "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false});
    crate::inbox::team::seed_cache(&mut store, "C02JU6C2Z1D", &[post]);
    let lookup = Lookup {
        me: "clayton-duarte".to_owned(),
        workflows: [(
            ("acme/pay".to_owned(), 812),
            Some(GhPr {
                title: "Add retries".to_owned(),
                draft: false,
                state: GhState::Open,
                author: "ryan".to_owned(),
                decision: None,
                reviews: Vec::new(),
                head_oid: None,
                checks_state: None,
            }),
        )]
        .into(),
    };
    crate::inbox::swr_fixture::lookup(&mut store, &lookup).unwrap();
    let bots = ["github-actions".to_owned()];
    let team = Team {
        bot_reviewers: &bots,
    };
    let all = conditions_with(&mut store, &home, NOW, 20, 1, Some(team)).unwrap();
    let reviews: Vec<Condition> = all
        .into_iter()
        .filter(|c| c.kind == Kind::ReviewRequested)
        .collect();
    assert_eq!(reviews.len(), 1, "{reviews:?}");
    assert_eq!(reviews[0].label, "review pay#812\nAdd retries");
}

#[test]
fn a_plan_review_label_has_the_review_title_on_row_two_and_none_without_a_title() {
    let insert = "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at)";
    let (_dir, home, mut store) = fixture();
    sql(
        &mut store,
        &format!("{insert} VALUES ('pay', 'r1', 1, '{FP}', 'pending', '{T}', NULL);"),
    );
    sql(
        &mut store,
        "UPDATE plan_reviews SET title = 'Add retries' WHERE id = 'r1'",
    );
    let titled = of(&mut store, &home, Kind::PlanReview);
    assert_eq!(
        titled.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
        ["pay plan review\nAdd retries"]
    );

    let (_dir, home, mut store) = fixture();
    sql(
        &mut store,
        &format!("{insert} VALUES ('pay', 'r1', 1, '{FP}', 'pending', '{T}', NULL);"),
    );
    let untitled = of(&mut store, &home, Kind::PlanReview);
    assert_eq!(
        untitled
            .iter()
            .map(|c| c.label.as_str())
            .collect::<Vec<_>>(),
        ["pay plan review"]
    );
}
