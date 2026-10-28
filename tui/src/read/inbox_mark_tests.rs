//! [`inbox_mark`]: what moves it and what does not.
use super::tree::inbox_mark;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;

fn sql(store: &mut Store, text: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(text)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

/// The mark moves on an open item's change and stays on any other write.
#[test]
fn the_inbox_mark_moves_on_an_open_item_and_stays_on_anything_else() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&CpdHome::resolve(Some(dir.path().into()), None).unwrap(), 5000).unwrap();
    sql(&mut store, "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('dialog', 'magenta', 's', 'one', '2026-01-01T00:00:00.000Z');
                     INSERT INTO inbox_items (kind, urgency, subject, label, opened_at, closed_at) VALUES ('dialog', 'magenta', 'old', 'old', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');");
    let mut last = inbox_mark(&mut store).unwrap();
    assert_eq!(inbox_mark(&mut store).unwrap(), last, "a read moves nothing");
    for (text, moves) in [
        ("UPDATE inbox_items SET label = 'two' WHERE subject = 's'", true),
        ("UPDATE inbox_items SET snoozed_until = '2026-02-01T00:00:00.000Z' WHERE subject = 's'", true),
        ("UPDATE inbox_items SET label = 'older' WHERE subject = 'old'", false),
        ("UPDATE sessions SET label = 'x'", false),
        ("UPDATE inbox_items SET closed_at = '2026-03-01T00:00:00.000Z' WHERE subject = 's'", true),
    ] {
        sql(&mut store, &format!("{text};"));
        let mark = inbox_mark(&mut store).unwrap();
        assert_eq!(mark != last, moves, "{text}");
        last = mark;
    }
}
