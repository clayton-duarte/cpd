//! The migration `20261031T1000-deploy-kind` (contract §4.5): `inbox_items.kind` holds `deploy`.

use super::*;
use crate::store::Store;

#[test]
fn inbox_items_accepts_the_deploy_kind() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at)
                 VALUES ('deploy', 'yellow', 'deploy:acme/pay#1', 'acme/pay deploy production: Release', 't1')",
                [],
            )?;
            Ok::<(), crate::error::CpdError>(())
        })
        .expect("inbox_items accepts the kind `deploy`");
    let kind: String = store.read(|c| Ok(c.query_row("SELECT kind FROM inbox_items", [], |r| r.get(0))?)).unwrap();
    assert_eq!(kind, "deploy");
    assert!(NAMED.iter().any(|m| m.name.ends_with("20261031T1000-deploy-kind")), "the migration deploy-kind is declared");
}
