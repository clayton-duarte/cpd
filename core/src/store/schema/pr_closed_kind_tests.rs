//! The migration `pr-closed-kind` (contract §4.5): `inbox_items.kind` holds `pr-closed`.

use super::*;
use crate::store::Store;

#[test]
fn the_inbox_kind_check_accepts_pr_closed() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at)
                 VALUES ('pr-closed', 'red', 'pr:acme/api#7', 'api#7 closed without a merge' || char(10) || 'Title 7', 't1')",
                [],
            )?;
            Ok::<(), crate::error::CpdError>(())
        })
        .expect("inbox_items accepts the kind `pr-closed`");
    let kind: String = store.read(|c| Ok(c.query_row("SELECT kind FROM inbox_items", [], |r| r.get(0))?)).unwrap();
    assert_eq!(kind, "pr-closed");
    assert!(NAMED.iter().any(|m| m.name.ends_with("-pr-closed-kind")), "the migration pr-closed-kind is declared");
}
