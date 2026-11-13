//! Unit tests for bug Inbox conditions and reconcile (Job `inbox-tests`, Plan `bug-inbox`).
//!
//! Verifies that bug reports open an Inbox item only when unmitigated past the protocol window
//! (900 s) or explicitly requested, with urgency Yellow, and close on mitigation or close
//! (pro-d578, contract §2.2 "Inbox item", "Bug record").

use super::model::{Kind, Urgency, reconcile};
use crate::home::CpdHome;
use crate::store::Store;

const T0: &str = "2026-01-01T00:00:00.000Z";
// 300 seconds (5 minutes) after T0: inside the 900s protocol window
const T_INSIDE: &str = "2026-01-01T00:05:00.000Z";
// 900 seconds (15 minutes) after T0: at the protocol window bound
const _T_BOUND: &str = "2026-01-01T00:15:00.000Z";
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

/// (2) Report, no mitigation past the window (now >= reported_at + 900s) -> exactly one Inbox item opened with Urgency::Yellow.
#[test]
fn report_unmitigated_past_window_opens_yellow_item() {
    let mut f = fixture();
    f.report_bug("b1", T0);

    // Past the window (T_PAST = T0 + 1000s >= 900s): should open exactly one Yellow item.
    f.reconcile(T_PAST);
    let items = f.bug_items();
    assert_eq!(
        items.len(),
        1,
        "past the window, exactly one bug Inbox item must open: {items:?}"
    );
    let item = &items[0];
    assert_eq!(
        item.urgency,
        Urgency::Yellow,
        "bug Inbox item must open with Yellow urgency, not Red"
    );
    assert_eq!(item.subject, "bug:b1");
    assert_eq!(item.plan.as_ref().map(|p| p.as_str()), Some("pay"));
}

/// (3) Mitigating an open item closes it.
#[test]
fn mitigating_closes_open_item() {
    let mut f = fixture();
    f.report_bug("b1", T0);

    // Past the window: item opens.
    f.reconcile(T_PAST);
    assert_eq!(f.bug_items().len(), 1, "bug item must be open past window");

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

    // Past the window: item opens.
    f.reconcile(T_PAST);
    assert_eq!(f.bug_items().len(), 1, "bug item must be open past window");

    // Closing at T_PAST closes the open item at next reconcile.
    f.close_bug("b1", T_PAST);
    f.reconcile(T_PAST);
    let items = f.bug_items();
    assert!(
        items.is_empty(),
        "closing bug must close open item, found: {items:?}"
    );
}
