//! The store migration `workflow-rename` (Plan `glossary-rename`, Context hs-d805; rename map rows
//! `ren-red-store`/`ren-impl-migration`; research `docs/research/glossary-rename.md` §3.4 and §3.11): an old
//! store whose object tables are `prs` and `pr_needs` migrates with every row and its append-only history kept,
//! the object columns renamed, the hand-written triggers and the head-ref index recreated under the new names,
//! `jobs.cancel_reason` rewritten (R14a) and the schema hash re-pinned.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

/// Every row of `history` for one `tbl`, as JSON arrays.
fn history(store: &mut Store, table: &str) -> Vec<Value> {
    rows(store, &format!("SELECT id, tbl, pk, op, before, after FROM history WHERE tbl = '{table}' ORDER BY id"))
}

/// The rows of an arbitrary query, each row a JSON array.
fn rows(store: &mut Store, sql: &str) -> Vec<Value> {
    store
        .read(|c: &Connection| {
            let mut stmt = c.prepare(sql)?;
            let n = stmt.column_count();
            let rows = stmt
                .query_map([], |r| {
                    (0..n)
                        .map(|i| {
                            Ok(match r.get_ref(i)? {
                                rusqlite::types::ValueRef::Null => Value::Null,
                                rusqlite::types::ValueRef::Integer(v) => Value::from(v),
                                rusqlite::types::ValueRef::Text(t) => Value::from(String::from_utf8_lossy(t).into_owned()),
                                other => panic!("{other:?}"),
                            })
                        })
                        .collect::<rusqlite::Result<Vec<Value>>>()
                        .map(Value::from)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap()
}

fn count(store: &mut Store, sql: &str) -> i64 {
    store.read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

/// The column names of `table` in column order.
fn columns(store: &mut Store, table: &str) -> Vec<String> {
    store
        .read(|c| {
            let mut stmt = c.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")?;
            Ok(stmt.query_map([table], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
        })
        .unwrap()
}

/// The names of the store's tables and triggers.
fn named(store: &mut Store, kind: &str) -> Vec<String> {
    store
        .read(|c| {
            let mut stmt = c.prepare("SELECT name FROM sqlite_schema WHERE type = ?1 AND name NOT LIKE 'sqlite_%'")?;
            Ok(stmt.query_map([kind], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
        })
        .unwrap()
}

/// An old store as every migration before `workflow-rename` leaves it: the object tables are `prs` and
/// `pr_needs`, a cancelled object and its `cancel_reason`. Fails while the migration is missing.
fn before_rename() -> (tempfile::TempDir, CpdHome, [Vec<Value>; 2]) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED.iter().copied().take_while(|n| !n.name.ends_with("-workflow-rename")).collect();
    assert!(earlier.len() < NAMED.len(), "the workflow-rename migration is a built-in migration");
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    write(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
               VALUES ('pay', 'ctx', 'Plan', 'plan', 'lead', 't');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
               VALUES ('pay', 'api', '/r', 'main', 'cancelled', 'pay/api', cpd_txn(), 't'),
                      ('pay', 'web', '/r', 'main', 'waiting', 'pay/web', cpd_txn(), 't');
             INSERT INTO pr_needs (plan, pr, needs) VALUES ('pay', 'web', 'api');
             INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status,
                               appended, fingerprint, cancel_reason, cancel_requested_at, kind, created_txn, created_at)
               VALUES ('pay', 'api', 'j', 'J', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'cancelled', 1, NULL,
                       'pr cancelled', 't', 'work', cpd_txn(), 't');"
        ),
    )
    .unwrap();
    let before = [history(&mut store, "prs"), history(&mut store, "pr_needs")];
    assert!(!before[0].is_empty() && !before[1].is_empty(), "the fixture wrote the old tables' history");
    drop(store);
    (dir, home, before)
}

#[test]
fn old_store_migrates_with_history() {
    let (_dir, home, before) = before_rename();

    // The new binary opens the old store and applies the migration.
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());

    // The object tables are renamed, and no old name is left.
    let tables = named(&mut store, "table");
    assert!(tables.contains(&"workflows".to_owned()) && tables.contains(&"workflow_needs".to_owned()), "{tables:?}");
    assert!(!tables.contains(&"prs".to_owned()) && !tables.contains(&"pr_needs".to_owned()), "{tables:?}");

    // Every row survives, with its values.
    assert_eq!(
        rows(&mut store, "SELECT plan, key, repo, base, status, lead_branch FROM workflows ORDER BY key"),
        vec![
            json!(["pay", "api", "/r", "main", "cancelled", "pay/api"]),
            json!(["pay", "web", "/r", "main", "waiting", "pay/web"]),
        ]
    );
    assert_eq!(rows(&mut store, "SELECT plan, pr, needs FROM workflow_needs"), vec![json!(["pay", "web", "api"])]);

    // The object columns are renamed, and the old ones are gone.
    assert!(columns(&mut store, "workflow_needs").contains(&"workflow".to_owned()));
    assert!(!columns(&mut store, "workflow_needs").contains(&"pr".to_owned()));
    assert!(columns(&mut store, "jobs").contains(&"workflow".to_owned()));
    assert!(!columns(&mut store, "jobs").contains(&"pr".to_owned()));
    assert!(columns(&mut store, "pr_posts").contains(&"workflow".to_owned()) && !columns(&mut store, "pr_posts").contains(&"pr".to_owned()));
    assert!(columns(&mut store, "slack_reactions").contains(&"workflow".to_owned()) && !columns(&mut store, "slack_reactions").contains(&"pr".to_owned()));
    assert!(columns(&mut store, "incident").contains(&"until_workflow".to_owned()) && !columns(&mut store, "incident").contains(&"until_pr".to_owned()));

    // A Job keeps its row and its new `workflow` value; `cancel_reason` is rewritten (R14a).
    assert_eq!(
        rows(&mut store, "SELECT plan, workflow, key, status, cancel_reason FROM jobs"),
        vec![json!(["pay", "api", "j", "cancelled", "workflow cancelled"])]
    );

    // The hand-written triggers are recreated under the new names; the old ones are gone.
    let triggers = named(&mut store, "trigger");
    for name in [
        "workflows_locked",
        "workflows_status",
        "workflows_shape_fixed",
        "workflows_head_ref_fixed",
        "workflow_needs_locked",
        "workflow_needs_no_delete",
        "jobs_workflow_dependencies",
        "dependencies_same_workflow",
        "workflows_hist_ins",
        "workflows_hist_upd",
        "workflows_no_delete",
        "workflow_needs_hist_ins",
        "workflow_needs_hist_upd",
    ] {
        assert!(triggers.contains(&name.to_owned()), "missing trigger {name}: {triggers:?}");
    }
    for old in ["prs_locked", "prs_status", "prs_shape_fixed", "prs_head_ref_fixed", "pr_needs_locked", "pr_needs_no_delete", "jobs_pr_dependencies", "dependencies_same_pr"] {
        assert!(!triggers.contains(&old.to_owned()), "the old trigger {old} is gone");
    }
    let indexes = named(&mut store, "index");
    assert!(indexes.contains(&"workflows_head_ref_live".to_owned()) && !indexes.contains(&"prs_head_ref_live".to_owned()), "{indexes:?}");

    // The triggers still work: the renamed lock trigger refuses a Lead Branch move after the lock.
    let moved = write(&mut store, "UPDATE workflows SET lead_branch = 'other/api' WHERE key = 'api'").unwrap_err();
    let message = moved.details()["sqlite_message"].as_str().unwrap_or_default().to_owned();
    assert!(message.contains("locked"), "the lock trigger fires: {moved:?}");

    // `history_tables` rows are renamed, so the reconcile regenerates the new triggers.
    assert_eq!(count(&mut store, "SELECT count(*) FROM history_tables WHERE name IN ('workflows', 'workflow_needs')"), 2);
    assert_eq!(count(&mut store, "SELECT count(*) FROM history_tables WHERE name IN ('prs', 'pr_needs')"), 0);

    // The append-only history is kept, its `tbl` values included; the rename writes none under the new names.
    assert_eq!(history(&mut store, "prs"), before[0], "every `prs` history row survives");
    assert_eq!(history(&mut store, "pr_needs"), before[1], "every `pr_needs` history row survives");
    assert_eq!(count(&mut store, "SELECT count(*) FROM history WHERE tbl IN ('workflows', 'workflow_needs')"), 0);

    // Every foreign key holds and the schema hash is re-pinned (§4.3).
    assert_eq!(count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"), 0);
    let pinned = store.read(|c| Ok(canonical_hash(c)?)).unwrap();
    assert_eq!(pinned, SCHEMA_HASH, "the schema hash is re-pinned");
}
