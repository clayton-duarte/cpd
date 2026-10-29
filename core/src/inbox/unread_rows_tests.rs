//! Row 2 of an `unread` item: the Plan's title, else the Session's title, and its in-place update (pro-d177, pro-d176).

use super::model::{Condition, Kind, reconcile};
use super::rules::conditions_with;
use crate::home::CpdHome;
use crate::store::Store;
use time::OffsetDateTime;

const T: &str = "2026-01-01T00:00:00.000Z";
const SNOOZE: &str = "2026-01-02T00:00:00.000Z";

struct Fixture {
    _dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
}

/// A Context `ctx` with its Manager `mgr` (untitled) and a Plan `pay` titled `Pay` with Lead `l-pay`; all `working`.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let mut f = Fixture { _dir: dir, home, store };
    f.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
           VALUES ('mgr', 'manager', 'ctx', 'Manager', '/', 'O3', 'working', '{T}', '{T}');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'l-pay', '{T}', '{T}', NULL);"
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

    /// A side fork: a Session of role lead, with `plan` or none, named `n`, with `title` or none.
    fn fork(&mut self, id: &str, plan: Option<&str>, title: Option<&str>) {
        let plan = plan.map_or("NULL".to_owned(), |p| format!("'{p}'"));
        let title = title.map_or("NULL".to_owned(), |t| format!("'{t}'"));
        self.sql(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, title, cwd, tier, state, state_at, created_at)
             VALUES ('{id}', 'lead', 'ctx', {plan}, 'n', {title}, '/', 'O3', 'working', '{T}', '{T}');"
        ));
    }

    /// The Session ends a turn `idle` as a reply to the User.
    fn reply(&mut self, session: &str) {
        self.sql(&format!(
            "UPDATE sessions SET state = 'idle', state_label = NULL WHERE id = '{session}';
             INSERT INTO events (txn, at, kind, context, session, text, data)
             VALUES (cpd_txn(), '{T}', 'session-state', 'ctx', '{session}', 'x',
               '{{\"from\":\"working\",\"label\":null,\"to\":\"idle\",\"reply\":true}}');"
        ));
    }

    fn unread(&mut self) -> Vec<Condition> {
        let now = OffsetDateTime::parse("2026-01-01T00:00:00Z", &time::format_description::well_known::Rfc3339).unwrap();
        let all = conditions_with(&mut self.store, &self.home, now, 20, 1, None).unwrap();
        all.into_iter().filter(|c| c.kind == Kind::Unread).collect()
    }

    fn label(&mut self, session: &str) -> String {
        let subject = format!("session:{session}");
        let found = self.unread();
        found.iter().find(|c| c.subject == subject).unwrap_or_else(|| panic!("{found:?}")).label.clone()
    }

    fn count(&mut self, sql: &str) -> i64 {
        self.store.read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }
}

#[test]
fn an_unread_lead_has_its_plans_title_on_row_two() {
    let mut f = fixture();
    f.reply("l-pay");
    assert_eq!(f.label("l-pay"), "pay replied\nPay");
    f.sql("UPDATE plans SET title = 'Add retries to payments' WHERE id = 'pay'");
    assert_eq!(f.label("l-pay"), "pay replied\nAdd retries to payments");
}

#[test]
fn an_unread_fork_of_a_plan_shows_the_plans_title_and_never_its_own() {
    let mut f = fixture();
    f.fork("fork-1234", Some("pay"), Some("Fork idea"));
    f.reply("fork-1234");
    assert_eq!(f.label("fork-1234"), "pay·fork replied\nPay");
}

#[test]
fn an_unread_manager_shows_its_session_title_and_none_while_untitled() {
    let mut f = fixture();
    f.reply("mgr");
    assert_eq!(f.label("mgr"), "ctx replied");
    f.sql("UPDATE sessions SET title = 'Payments work' WHERE id = 'mgr'");
    assert_eq!(f.label("mgr"), "ctx replied\nPayments work");
}

#[test]
fn an_unread_loose_fork_shows_its_session_title_and_never_its_name() {
    let mut f = fixture();
    f.fork("loose-fork-id", None, Some("Side idea"));
    f.reply("loose-fork-id");
    assert_eq!(f.label("loose-fork-id"), "loose-fo replied\nSide idea");
    f.sql("UPDATE sessions SET title = NULL WHERE id = 'loose-fork-id'");
    assert_eq!(f.label("loose-fork-id"), "loose-fo replied");
}

#[test]
fn an_open_unread_item_follows_its_title_in_place_and_keeps_its_snooze() {
    let mut f = fixture();
    f.reply("mgr");
    let conditions = f.unread();
    let opened = reconcile(&mut f.store, &conditions, T).unwrap();
    assert_eq!(opened.opened.len(), 1);
    let id = opened.opened[0];
    let label = |f: &mut Fixture| -> String {
        f.store.read(|c| Ok(c.query_row("SELECT label FROM inbox_items WHERE id = ?1 AND closed_at IS NULL", [id], |r| r.get(0))?)).unwrap()
    };
    assert_eq!(label(&mut f), "ctx replied");
    f.sql(&format!("UPDATE inbox_items SET snoozed_until = '{SNOOZE}' WHERE id = {id}"));
    let events = "SELECT count(*) FROM events WHERE kind IN ('inbox-opened', 'inbox-closed')";
    let before = f.count(events);
    let snooze = format!("SELECT count(*) FROM inbox_items WHERE id = {id} AND snoozed_until = '{SNOOZE}'");

    for title in ["Payments work", "Billing work"] {
        f.sql(&format!("UPDATE sessions SET title = '{title}' WHERE id = 'mgr'"));
        let conditions = f.unread();
        let changes = reconcile(&mut f.store, &conditions, T).unwrap();
        assert!(changes.is_empty(), "{changes:?}");
        assert_eq!(label(&mut f), format!("ctx replied\n{title}"));
        assert_eq!(f.count(&snooze), 1);
        assert_eq!(f.count(events), before);
        assert_eq!(f.count("SELECT count(*) FROM inbox_items WHERE closed_at IS NULL"), 1);
    }
}
