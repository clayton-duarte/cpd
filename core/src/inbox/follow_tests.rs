//! Reconcile of `pr-ready` and `dialog` items: a label or link change inside a colour updates the open row in place (no Event);
//! a colour change closes and opens (pro-d130, pro-d131, pro-d150).

use super::model::{Condition, Filter, Item, Kind, State, Urgency, list, reconcile};
use crate::home::CpdHome;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::store::Store;

const T1: &str = "2026-01-01T00:00:00.000Z";
const T2: &str = "2026-01-01T00:05:00.000Z";
const SUBJECT: &str = "pr:acme/shop#4";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
                   VALUES ('s1', 'lead', 'ctx', 'One', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('l-pay', 'lead', 'ctx', NULL, 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
                   VALUES ('pay', 'ctx', 'Pay', 'grounding', 'l-pay', 't', NULL, NULL);",
            )?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

fn pr(urgency: Urgency, label: &str) -> Condition {
    Condition {
        kind: Kind::PrReady,
        urgency,
        subject: SUBJECT.to_owned(),
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

fn count(store: &mut Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

fn events(store: &mut Store, kind: &str) -> i64 {
    count(
        store,
        &format!("SELECT count(*) FROM events WHERE kind = '{kind}'"),
    )
}

fn all_events(store: &mut Store) -> i64 {
    count(store, "SELECT count(*) FROM events")
}

fn items(store: &mut Store, state: State) -> Vec<Item> {
    store
        .read(|c| {
            list(
                c,
                &Filter {
                    context: None,
                    state,
                },
            )
        })
        .unwrap()
}

fn visible(store: &mut Store) -> Vec<Item> {
    items(store, State::Visible { now: T2.into() })
}

#[test]
fn a_pr_ready_item_whose_label_changes_inside_its_colour_is_updated_in_place() {
    let (_d, mut store) = store();
    let first = reconcile(
        &mut store,
        &[pr(Urgency::Cyan, "shop#4 checks running")],
        T1,
    )
    .unwrap();
    let id = first.opened[0];
    let before = all_events(&mut store);
    let second = reconcile(&mut store, &[pr(Urgency::Cyan, "shop#4 in review")], T2).unwrap();
    assert!(second.opened.is_empty() && second.closed.is_empty());
    let open = items(&mut store, State::Open);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, id);
    assert_eq!(open[0].opened_at, T1);
    assert_eq!(open[0].closed_at, None);
    assert_eq!(open[0].label, "shop#4 in review");
    assert_eq!(all_events(&mut store), before);
    assert_eq!(events(&mut store, "inbox-opened"), 1);
    assert_eq!(events(&mut store, "inbox-closed"), 0);
}

#[test]
fn an_in_place_update_keeps_a_snooze_and_a_dismissal() {
    for column in ["snoozed_until", "dismissed_at"] {
        let (_d, mut store) = store();
        let id = reconcile(
            &mut store,
            &[pr(Urgency::Cyan, "shop#4 checks running")],
            T1,
        )
        .unwrap()
        .opened[0];
        let later = "2026-02-01T00:00:00.000Z";
        store
            .write("hide", |tx| {
                tx.execute(
                    &format!("UPDATE inbox_items SET {column} = ?2 WHERE id = ?1"),
                    rusqlite::params![id, later],
                )?;
                Ok(())
            })
            .unwrap();
        reconcile(&mut store, &[pr(Urgency::Cyan, "shop#4 in review")], T2).unwrap();
        let open = items(&mut store, State::Open);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].label, "shop#4 in review");
        assert_eq!(open[0].snoozed_until.is_some(), column == "snoozed_until");
        assert_eq!(open[0].dismissed_at.is_some(), column == "dismissed_at");
        assert!(visible(&mut store).is_empty());
    }
}

#[test]
fn a_pr_ready_item_follows_its_links_in_place() {
    let (_d, mut store) = store();
    let id = reconcile(&mut store, &[pr(Urgency::Cyan, "shop#4 in review")], T1)
        .unwrap()
        .opened[0];
    let before = all_events(&mut store);
    let linked = Condition {
        url: Some("https://github.com/acme/shop/pull/4".into()),
        context: Some(ContextId::parse("ctx").unwrap()),
        plan: Some(PlanId::parse("pay").unwrap()),
        session: Some(SessionId::parse("l-pay").unwrap()),
        ..pr(Urgency::Cyan, "shop#4 in review")
    };
    let changes = reconcile(&mut store, &[linked], T2).unwrap();
    assert!(changes.opened.is_empty() && changes.closed.is_empty());
    let open = items(&mut store, State::Open);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, id);
    assert_eq!(
        open[0].url.as_deref(),
        Some("https://github.com/acme/shop/pull/4")
    );
    assert_eq!(open[0].context.as_ref().map(|c| c.as_str()), Some("ctx"));
    assert_eq!(open[0].plan.as_ref().map(|p| p.as_str()), Some("pay"));
    assert_eq!(open[0].session.as_ref().map(|s| s.as_str()), Some("l-pay"));
    assert_eq!(all_events(&mut store), before);
    assert_eq!(items(&mut store, State::Closed { limit: 10 }).len(), 0);
}

#[test]
fn an_unchanged_pr_ready_condition_writes_nothing() {
    let (_d, mut store) = store();
    let c = pr(Urgency::Cyan, "shop#4 in review");
    reconcile(&mut store, std::slice::from_ref(&c), T1).unwrap();
    let txns = count(&mut store, "SELECT count(*) FROM txns");
    let changes = reconcile(&mut store, &[c], T2).unwrap();
    assert!(changes.opened.is_empty() && changes.closed.is_empty());
    assert_eq!(count(&mut store, "SELECT count(*) FROM txns"), txns);
}

#[test]
fn a_colour_change_closes_the_pr_ready_item_and_opens_a_new_one_a_dismissal_stays_with_the_colour()
{
    let (_d, mut store) = store();
    let first = reconcile(&mut store, &[pr(Urgency::Red, "shop#4 failing")], T1)
        .unwrap()
        .opened[0];
    store
        .write("dismiss", |tx| {
            tx.execute(
                "UPDATE inbox_items SET dismissed_at = ?2 WHERE id = ?1",
                rusqlite::params![first, T1],
            )?;
            Ok(())
        })
        .unwrap();
    let changes = reconcile(&mut store, &[pr(Urgency::Cyan, "shop#4 in review")], T2).unwrap();
    assert_eq!(changes.closed, vec![first]);
    assert_eq!(changes.opened.len(), 1);
    assert_ne!(changes.opened[0], first);
    assert_eq!(events(&mut store, "inbox-closed"), 1);
    assert_eq!(events(&mut store, "inbox-opened"), 2);
    let shown = visible(&mut store);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].id, changes.opened[0]);
}

#[test]
fn a_dialog_item_whose_label_changes_is_updated_in_place() {
    let dialog = |urgency: Urgency, label: &str| Condition {
        kind: Kind::Dialog,
        ..pr(urgency, label)
    };
    let (_d, mut store) = store();
    let id = reconcile(&mut store, &[dialog(Urgency::Red, "pay asks you")], T1)
        .unwrap()
        .opened[0];
    let before = all_events(&mut store);
    let changes = reconcile(
        &mut store,
        &[dialog(Urgency::Red, "pay verdict: Approve")],
        T2,
    )
    .unwrap();
    assert!(changes.opened.is_empty() && changes.closed.is_empty());
    let open = items(&mut store, State::Open);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, id);
    assert_eq!(open[0].label, "pay verdict: Approve");
    assert_eq!(open[0].opened_at, T1);
    assert_eq!(all_events(&mut store), before);
    assert_eq!(items(&mut store, State::Closed { limit: 10 }).len(), 0);

    // a snooze and a dismissal set in between stay
    for column in ["snoozed_until", "dismissed_at"] {
        let (_d, mut store) = store_with_dialog(&dialog);
        let id = open_id(&mut store);
        store
            .write("hide", |tx| {
                tx.execute(
                    &format!("UPDATE inbox_items SET {column} = ?2 WHERE id = ?1"),
                    rusqlite::params![id, "2026-02-01T00:00:00.000Z"],
                )?;
                Ok(())
            })
            .unwrap();
        reconcile(
            &mut store,
            &[dialog(Urgency::Red, "pay verdict: Approve")],
            T2,
        )
        .unwrap();
        let open = items(&mut store, State::Open);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].label, "pay verdict: Approve", "{column}");
        assert_eq!(open[0].snoozed_until.is_some(), column == "snoozed_until");
        assert_eq!(open[0].dismissed_at.is_some(), column == "dismissed_at");
    }

    // the same label again writes nothing
    let txns = count(&mut store, "SELECT count(*) FROM txns");
    reconcile(
        &mut store,
        &[dialog(Urgency::Red, "pay verdict: Approve")],
        T2,
    )
    .unwrap();
    assert_eq!(count(&mut store, "SELECT count(*) FROM txns"), txns);

    // an urgency change still closes the item and opens a new one
    let changes = reconcile(
        &mut store,
        &[dialog(Urgency::Magenta, "pay verdict: Approve")],
        T2,
    )
    .unwrap();
    assert_eq!(changes.closed, vec![id]);
    assert_eq!(changes.opened.len(), 1);
    assert_ne!(changes.opened[0], id);
}

fn store_with_dialog(dialog: &dyn Fn(Urgency, &str) -> Condition) -> (tempfile::TempDir, Store) {
    let (d, mut store) = store();
    reconcile(&mut store, &[dialog(Urgency::Red, "pay asks you")], T1).unwrap();
    (d, store)
}

fn open_id(store: &mut Store) -> i64 {
    items(store, State::Open)[0].id
}
