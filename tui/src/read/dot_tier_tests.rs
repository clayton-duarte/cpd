//! pro-d401: an open unread Session's dot is cyan (the unread rule's urgency), a white item's dot stays white.

use super::dot::{Dot, urgency};
use cpd_core::home::CpdHome;
use cpd_core::ids::SessionId;
use cpd_core::inbox::model::{Item, Kind, Urgency};
use cpd_core::inbox::rules::conditions_with;
use cpd_core::session::State;
use cpd_core::store::Store;
use time::macros::datetime;

const T: &str = "2026-01-01T00:00:00.000Z";

fn item(kind: Kind, urgency: Urgency, session: &str) -> Item {
    Item {
        id: 1,
        kind,
        urgency,
        subject: "s".into(),
        label: "l".into(),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: Some(SessionId::parse(session).unwrap()),
        record: None,
        event: None,
        opened_at: T.into(),
        closed_at: None,
        snoozed_until: None,
        dismissed_at: None,
        claimed_at: None,
    }
}

/// The urgency the engine's unread rule gives an idle Lead whose latest turn replied to the User.
fn unread_urgency() -> Urgency {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{T}', '{T}');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'l-pay', '{T}', '{T}', NULL);
                 INSERT INTO events (txn, at, kind, context, plan, job, session, text, data)
                   VALUES (cpd_txn(), '{T}', 'session-state', 'ctx', NULL, NULL, 'l-pay', 'x',
                           '{{\"from\":\"working\",\"label\":null,\"to\":\"idle\",\"reply\":true}}');"
            ))?;
            Ok(())
        })
        .unwrap();
    let all = conditions_with(
        &mut store,
        &home,
        datetime!(2026-01-02 00:00 UTC),
        20,
        1,
        None,
    )
    .unwrap();
    let unread: Vec<_> = all.iter().filter(|c| c.kind == Kind::Unread).collect();
    assert_eq!(unread.len(), 1, "{all:?}");
    unread[0].urgency
}

#[test]
fn an_open_unread_session_gets_a_cyan_dot() {
    let session = SessionId::parse("l-pay").unwrap();
    let dot_of = |u: Urgency| {
        let items = [item(Kind::Unread, u, "l-pay")];
        Dot::of(State::Idle, urgency(&items, &session, None), false, None)
    };
    assert_eq!(dot_of(unread_urgency()), Dot::Cyan);
    assert_eq!(dot_of(Urgency::White), Dot::White);
}
