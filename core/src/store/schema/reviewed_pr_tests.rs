//! The migration `plan-reviewed-pr` (pro-d412): three nullable `plans` columns, every row kept.

use super::*;
use crate::store::Store;

const PLAN: &str = "INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('p1', 'c1', 'P', 'grounding', 'l1', 't');";

fn run(store: &mut Store, sql: &str) -> Result<(), crate::error::CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok(())
    })
}

#[test]
fn plan_reviewed_pr_migration_keeps_every_plan_with_null_columns() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-plan-reviewed-pr"))
        .collect();
    assert!(earlier.len() < NAMED.len());
    let mut old = crate::store::tests::open_named(&home, &earlier).unwrap();
    run(
        &mut old,
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('c1', 'C', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l1', 'lead', 'c1', 'p1', 'p1', '/r', 'O2', 'stopped', 't', 't');",
    )
    .unwrap();
    run(&mut old, PLAN).unwrap();
    drop(old);
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none());
    let nulls: i64 = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM plans WHERE reviewed_repo IS NULL AND reviewed_number IS NULL AND reviewed_title IS NULL",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(nulls, 1);
    run(
        &mut store,
        "UPDATE plans SET reviewed_repo = 'acme/pay', reviewed_number = 812, reviewed_title = 'T' WHERE id = 'p1'",
    )
    .unwrap();
    assert!(run(&mut store, "UPDATE plans SET reviewed_number = 0").is_err());
}
