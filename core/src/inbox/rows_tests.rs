//! The two-row label of an Inbox item (pro-d171): the stored label is `<row1>\n<row2>`, Events say it on one line.

use serde_json::json;
use time::OffsetDateTime;
use time::macros::datetime;

use super::model::{Condition, Item, Kind, Urgency, one_line, reconcile};
use super::pr::Status;
use super::rules::conditions_with;
use crate::home::CpdHome;
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);
const T: &str = "2026-01-01T00:00:00.000Z";

fn store() -> (tempfile::TempDir, CpdHome, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, home, store)
}

fn sql(store: &mut Store, sql: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok(())
        })
        .unwrap();
}

fn conditions(store: &mut Store, home: &CpdHome, kind: Kind) -> Vec<Condition> {
    conditions_with(store, home, NOW, 20, 1, None)
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == kind)
        .collect()
}

fn item(kind: Kind, label: &str) -> Item {
    Item {
        id: 1,
        kind,
        urgency: Urgency::White,
        subject: "subject:1".to_owned(),
        label: label.to_owned(),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: T.to_owned(),
        closed_at: None,
        snoozed_until: None,
        dismissed_at: None,
        claimed_at: None,
    }
}

fn condition(label: &str) -> Condition {
    Condition {
        kind: Kind::PrReady,
        urgency: Urgency::White,
        subject: "pr:acme/shop#3".to_owned(),
        label: label.to_owned(),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
    }
}

#[test]
fn a_pr_ready_label_is_the_status_row_and_the_pull_request_title_on_row_two() {
    let (_d, home, mut store) = store();
    sql(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', '{T}', 'm1');
             INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                        mergeable, authored, review_requested, merge_state, feedback, base_ref, default_branch)
             VALUES ('acme/shop', 3, 'https://github.com/acme/shop/pull/3', 'Add retries', 1, 'open', 'none', 'passed', '{T}', '{T}',
                     'mergeable', 1, 0, 'draft', '[]', 'main', 'main');"
        ),
    );
    let found: Vec<Condition> = conditions(&mut store, &home, Kind::PrReady)
        .into_iter()
        .filter(|c| c.subject == "pr:acme/shop#3")
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].label, "shop#3 draft\nAdd retries");
}

#[test]
fn status_of_item_reads_only_the_first_row_of_a_two_row_pr_ready_label() {
    assert_eq!(
        Status::of_item(&item(
            Kind::PrReady,
            "shop#3 ready to merge\nready to merge soon"
        )),
        Some(Status::ReadyToMerge)
    );
    assert_eq!(
        Status::of_item(&item(Kind::PrReady, "shop#3 draft\nfix the thing")),
        Some(Status::Draft)
    );
}

#[test]
fn a_deploy_label_is_the_repo_and_environment_with_the_run_title_on_row_two() {
    let (_d, home, mut store) = store();
    let path = home.run_dir().join("deploy-runs.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let run = json!({"repo": "acme/pay", "workflow": "main.yml", "run_id": 7, "environment": "production", "title": "Release 1.2",
                     "url": "https://github.com/acme/pay/actions/runs/7", "created_at": "2026-10-27T13:05:00Z"});
    std::fs::write(
        &path,
        json!({"version": 1, "polled_at": "2026-10-27T14:59:55.000Z", "runs": [run]}).to_string(),
    )
    .unwrap();
    let found = conditions(&mut store, &home, Kind::Deploy);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].label, "acme/pay deploy production\nRelease 1.2");
}

#[test]
fn a_verdict_dialog_label_is_who_verdict_with_the_option_on_row_two() {
    let (_d, home, mut store) = store();
    sql(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
             INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at, state_label)
               VALUES ('mgr', 'manager', 'ctx', 'Manager', '/', 'O3', 'blocked', '{T}', '{T}', 'Which repo?');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, state_label)
               VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'blocked', '{T}', '{T}', 'verdict: Approve with comments');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('pay', 'ctx', 'Pay', 'deploy', 'l-pay', '{T}', '{T}');
             INSERT INTO events (txn, at, kind, context, session, text, data)
             VALUES (cpd_txn(), '{T}', 'session-state', 'ctx', 'mgr', 'x', '{{\"from\":\"working\",\"label\":null,\"to\":\"blocked\"}}');
             INSERT INTO events (txn, at, kind, context, session, text, data)
             VALUES (cpd_txn(), '{T}', 'session-state', 'ctx', 'l-pay', 'x', '{{\"from\":\"working\",\"label\":null,\"to\":\"blocked\"}}');"
        ),
    );
    let found = conditions(&mut store, &home, Kind::Dialog);
    let label_of = |session: &str| {
        found
            .iter()
            .find(|c| c.session.as_ref().is_some_and(|s| s.as_str() == session))
            .map(|c| c.label.clone())
    };
    assert_eq!(
        label_of("l-pay").as_deref(),
        Some("pay verdict\nApprove with comments"),
        "{found:?}"
    );
    assert_eq!(
        label_of("mgr").as_deref(),
        Some("ctx asks you"),
        "{found:?}"
    );
}

#[test]
fn one_line_joins_the_two_rows_with_a_colon_and_drops_an_empty_row_two() {
    assert_eq!(one_line("a\nb"), "a: b");
    assert_eq!(one_line("a\n"), "a");
    assert_eq!(one_line("a"), "a");
}

#[test]
fn the_events_of_a_two_row_item_say_one_line_and_the_opened_data_keeps_the_label_whole() {
    let (_d, _home, mut store) = store();
    let opened = reconcile(&mut store, &[condition("shop#3 draft\nAdd retries")], T).unwrap();
    let id = opened.opened[0];
    reconcile(&mut store, &[], "2026-01-01T00:05:00.000Z").unwrap();
    let read = |store: &mut Store, kind: &str| -> (String, String) {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT text, data FROM events WHERE kind = ?1",
                    [kind],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap()
    };
    let (text, data) = read(&mut store, "inbox-opened");
    assert_eq!(
        text,
        format!("Inbox item {id} opened: shop#3 draft: Add retries.")
    );
    let data: serde_json::Value = serde_json::from_str(&data).unwrap();
    assert_eq!(data["label"], "shop#3 draft\nAdd retries");
    let (text, _) = read(&mut store, "inbox-closed");
    assert_eq!(
        text,
        format!("Inbox item {id} closed: shop#3 draft: Add retries.")
    );
}
