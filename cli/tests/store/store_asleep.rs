//! Migration of `sessions.asleep_at` and the `session-asleep` Event kind (contract §4.5; hs-d515, hs-d520).

use crate::home::{HasCpdHome, Home};

use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};

#[test]
fn migration_adds_asleep_at_and_the_event_kind() {
    let env = Home::empty();
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();

    let columns: Vec<String> = store
        .read(|c| {
            let mut stmt = c.prepare("SELECT name FROM pragma_table_info('sessions')")?;
            Ok(stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?)
        })
        .unwrap();
    assert!(
        columns.contains(&"asleep_at".to_owned()),
        "sessions.asleep_at: {columns:?}"
    );

    let kind: Option<String> = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT name FROM event_kinds WHERE name = 'session-asleep'",
                [],
                |r| r.get(0),
            )
            .ok())
        })
        .unwrap();
    assert_eq!(kind.as_deref(), Some("session-asleep"));

    let insert = |tx: &WriteTxn<'_>| {
        tx.execute(
            "INSERT INTO events (txn, at, kind, context, text, data) VALUES (cpd_txn(), 't', 'session-asleep', 'ctx', 'x', '{}')",
            [],
        )
    };
    store
        .write("test", |tx| {
            tx.execute("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1')", [])?;
            Ok(insert(tx)?)
        })
        .expect("the events table accepts session-asleep");
}
