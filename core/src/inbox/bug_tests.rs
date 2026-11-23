//! Unit tests for bug Inbox conditions and reconcile (Job `inbox-tests`, Plan `bug-inbox`).
//!
//! Age alone does not open a bug item; explicit escalation still does.

use super::model::{Kind, Urgency, reconcile};
use crate::home::CpdHome;
use crate::store::Store;

const T0: &str = "2026-01-01T00:00:00.000Z";
// 300 seconds (5 minutes) after T0: inside the 900s protocol window
const T_INSIDE: &str = "2026-01-01T00:05:00.000Z";
// 900 seconds (15 minutes) after T0: at the protocol window bound
const T_BOUND: &str = "2026-01-01T00:15:00.000Z";
// 1000 seconds after T0: past the protocol window bound
const T_PAST: &str = "2026-01-01T00:16:40.000Z";

struct Fixture {
    _dir: tempfile::TempDir,
    _home: CpdHome,
    store: Store,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let mut f = Fixture {
        _dir: dir,
        _home: home,
        store,
    };
    f.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T0}', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{T0}', '{T0}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'l-pay', '{T0}', '{T0}');"
    ));
    f
}

impl Fixture {
    fn sql(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    fn report_bug(&mut self, id: &str, reported_at: &str) {
        self.sql(&format!(
            "INSERT INTO bug_records (id, plan, title, symptom, status, reported_at)
             VALUES ('{id}', 'pay', 'Crash on checkout', 'Crash on checkout', 'open', '{reported_at}');"
        ));
    }

    fn mitigate_bug(&mut self, id: &str, mitigated_at: &str) {
        self.sql(&format!(
            "UPDATE bug_records SET mitigation = 'Fixed', mitigated_by = 'l-pay', mitigated_at = '{mitigated_at}'
             WHERE id = '{id}';"
        ));
    }

    fn close_bug(&mut self, id: &str, closed_at: &str) {
        self.sql(&format!(
            "UPDATE bug_records SET status = 'closed', closed_at = '{closed_at}', closed_by = 'l-pay',
                    root_fix = 'fix pr #1', regression_test = 'bug_test'
             WHERE id = '{id}';"
        ));
    }

    fn reconcile(&mut self, now: &str) {
        reconcile(&mut self.store, &[], now).unwrap();
    }

    fn escalate_bug(&mut self, id: &str) {
        self.sql(&format!(
            "UPDATE bug_records SET escalated_at = '{T_INSIDE}', escalated_by = 'l-pay',
                    escalation_reason = 'Need the owner decision' WHERE id = '{id}'"
        ));
    }

    fn bug_items(&mut self) -> Vec<super::model::Item> {
        self.store
            .read(super::model::open_items)
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == Kind::Bug)
            .collect()
    }
}

/// (1) Report then mitigate inside the window -> no Inbox item opened.
#[test]
fn report_mitigated_inside_window_opens_no_item() {
    let mut f = fixture();
    f.report_bug("b1", T0);

    // Reconcile inside the 900 s window (at T0 + 300 s): should NOT open an Inbox item.
    f.reconcile(T_INSIDE);
    let items = f.bug_items();
    assert!(
        items.is_empty(),
        "bug inside protocol window should not open an Inbox item, found: {items:?}"
    );

    // Mitigate inside the window.
    f.mitigate_bug("b1", T_INSIDE);
    f.reconcile(T_INSIDE);
    assert!(
        f.bug_items().is_empty(),
        "mitigated bug should not have open Inbox items"
    );
}

#[test]
fn an_unmitigated_bug_never_opens_an_item_on_age_alone() {
    let mut f = fixture();
    f.report_bug("b1", T0);
    for now in [T_INSIDE, T_BOUND, T_PAST] {
        f.reconcile(now);
        assert!(f.bug_items().is_empty(), "{now}");
    }
    f.sql(&format!(
        "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at)
         VALUES ('bug', 'yellow', 'bug:b1', 'legacy age-only row', '{T_BOUND}')"
    ));
    f.reconcile(T_PAST);
    assert!(f.bug_items().is_empty(), "legacy age-only row closes");
    let (status, mitigation): (String, Option<String>) = f
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT status, mitigation FROM bug_records WHERE id = 'b1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!((status.as_str(), mitigation), ("open", None));
}

/// (3) Mitigating an open item closes it.
#[test]
fn mitigating_closes_open_item() {
    let mut f = fixture();
    f.report_bug("b1", T0);

    f.escalate_bug("b1");
    f.reconcile(T_PAST);
    assert_eq!(f.bug_items().len(), 1, "escalated item must be open");

    // Mitigating at T_PAST closes the open item at next reconcile.
    f.mitigate_bug("b1", T_PAST);
    f.reconcile(T_PAST);
    let items = f.bug_items();
    assert!(
        items.is_empty(),
        "mitigating bug must close open item, found: {items:?}"
    );
}

/// (4) Closing the bug closes any open item.
#[test]
fn closing_closes_open_item() {
    let mut f = fixture();
    f.report_bug("b1", T0);

    f.escalate_bug("b1");
    f.reconcile(T_PAST);
    assert_eq!(f.bug_items().len(), 1, "escalated item must be open");

    // Closing at T_PAST closes the open item at next reconcile.
    f.close_bug("b1", T_PAST);
    f.reconcile(T_PAST);
    let items = f.bug_items();
    assert!(
        items.is_empty(),
        "closing bug must close open item, found: {items:?}"
    );
}

#[test]
fn an_explicit_bug_escalation_opens_only_that_bug_inside_the_window() {
    let mut f = fixture();
    f.report_bug("b1", T0);
    f.report_bug("b2", T0);
    f.escalate_bug("b1");
    f.reconcile(T_INSIDE);
    let items = f.bug_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].subject, "bug:b1");
    assert_eq!(items[0].urgency, Urgency::Yellow);
    assert_eq!(items[0].plan.as_ref().map(|p| p.as_str()), Some("pay"));
    f.mitigate_bug("b1", T_INSIDE);
    f.reconcile(T_INSIDE);
    assert!(f.bug_items().is_empty());
}
