//! The migration `20261102T1600-drafts-parked-kind` (contract §4.5): `inbox_items.kind` holds `drafts-parked`.

use super::*;
use crate::store::Store;

#[test]
fn inbox_items_accepts_the_drafts_parked_kind() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at)
                 VALUES ('drafts-parked', 'magenta', 'drafts-parked:pay', 'pay: 2 parked drafts', 't1')",
                [],
            )?;
            Ok::<(), crate::error::CpdError>(())
        })
        .expect("inbox_items accepts the kind `drafts-parked`");
    let kind: String = store.read(|c| Ok(c.query_row("SELECT kind FROM inbox_items", [], |r| r.get(0))?)).unwrap();
    assert_eq!(kind, "drafts-parked");
    assert!(NAMED.iter().any(|m| m.name.ends_with("20261102T1600-drafts-parked-kind")), "the migration drafts-parked-kind is declared");
}
