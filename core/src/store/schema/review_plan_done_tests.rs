//! The migration `20261103T0800-review-plan-done` (contract §3.1, §4.5; pro-n346, pro-d6): `review submit` finishes the
//! `codereview` Plan from `grounding`, so the `plans_phase` trigger allows `grounding>done` for Context `codereview` only.
//! Every other transition stays as it was, and no row changes. Stores here are built to the migration before it.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const MIGRATION: &str = "20261103T0800-review-plan-done";

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn count(store: &mut Store, sql: &str) -> i64 {
    store
        .read(|c: &Connection| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

/// Every row of `table` (and Events) as one JSON text, the columns named: what the migration must leave alone.
fn dump(store: &mut Store) -> String {
    let mut out = String::new();
    for t in ["contexts", "sessions", "plans", "events"] {
        let names: Vec<String> = store
            .read(|c: &Connection| {
                let mut stmt = c.prepare(&format!(
                    "SELECT name FROM pragma_table_info('{t}') ORDER BY cid"
                ))?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                // `codereview-followup` (a later migration) adds two `plans` columns: not what this migration touches.
                Ok(rows
                    .into_iter()
                    .filter(|n| n != "pr_subject" && n != "changes_requested_at")
                    .collect())
            })
            .unwrap();
        let pairs: Vec<String> = names.iter().map(|n| format!("'{n}', {n}")).collect();
        let sql = format!(
            "SELECT coalesce(json_group_array(json_object({})), '[]') FROM {t}",
            pairs.join(", ")
        );
        let text: String = store
            .read(|c: &Connection| Ok(c.query_row(&sql, [], |r| r.get(0))?))
            .unwrap();
        out.push_str(&format!("{t}: {text}\n"));
    }
    out
}

/// The migrations before `review-plan-done`. Fails while the migration is missing.
fn earlier() -> Vec<Named> {
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-review-plan-done"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "review-plan-done is a built-in migration"
    );
    earlier
}

/// Two Contexts, a Lead Session each, and a Plan of each in `grounding`: `r-pay-812` of `codereview`, `c-pay` of `cpd`.
const SEED: &str = "INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('codereview', 'Code review', 't', 'm');
     INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('cpd', 'Cpd', 't', 'm');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('l1', 'lead', 'codereview', 'r-pay-812', 'L1', '/', 'S3', 'idle', 't', 't');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('l2', 'lead', 'cpd', 'c-pay', 'L2', '/', 'S3', 'idle', 't', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at)
       VALUES ('r-pay-812', 'codereview', 'Review', 'grounding', 'l1', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at)
       VALUES ('c-pay', 'cpd', 'Pay', 'grounding', 'l2', 't');
     -- An open Inbox item of the codereview Lead keeps `finish-legacy-review-plans` (a later migration) from finishing `r-pay-812`.
     INSERT INTO inbox_items (kind, urgency, subject, session, label, opened_at) VALUES ('dialog', 'white', 'open dialog', 'l1', 'Open', 't');";

fn before() -> (tempfile::TempDir, CpdHome) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = crate::store::tests::open_named(&home, &earlier()).unwrap();
    write(&mut store, SEED).unwrap_or_else(|e| panic!("fixture: {e:?}"));
    drop(store);
    (dir, home)
}

fn migrated_in(home: &CpdHome) -> Store {
    let mut store = Store::open(home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(
        count(
            &mut store,
            &format!("SELECT count(*) FROM schema_migrations WHERE name = '{MIGRATION}'")
        ),
        1,
        "the migration is applied"
    );
    store
}

const FINISH: &str =
    "UPDATE plans SET phase = 'done', locked_at = 't2', done_at = 't2' WHERE id = ";

fn assert_illegal(result: Result<(), CpdError>, what: &str) {
    let text = format!("{result:?}");
    assert!(text.contains("illegal_transition"), "{what}: {text}");
}

fn phase(store: &mut Store, id: &str) -> String {
    store
        .read(|c: &Connection| {
            Ok(c.query_row("SELECT phase FROM plans WHERE id = ?1", [id], |r| r.get(0))?)
        })
        .unwrap()
}

#[test]
fn review_plan_done_migration_allows_grounding_to_done_for_codereview_only() {
    let (_dir, home) = before();
    let mut old = crate::store::tests::open_named(&home, &earlier()).unwrap();
    assert_illegal(
        write(&mut old, &format!("{FINISH}'r-pay-812'")),
        "before the migration",
    );
    drop(old);

    let mut store = migrated_in(&home);
    write(&mut store, &format!("{FINISH}'r-pay-812'")).expect("codereview grounding>done");
    assert_eq!(phase(&mut store, "r-pay-812"), "done");
    assert_illegal(
        write(&mut store, &format!("{FINISH}'c-pay'")),
        "a Plan of Context cpd",
    );
    assert_eq!(phase(&mut store, "c-pay"), "grounding");
}

#[test]
fn review_plan_done_migration_keeps_every_other_transition_as_before() {
    let (_dir, home) = before();
    let mut store = migrated_in(&home);
    let allowed = [
        ("grounding", "plan"),
        ("plan", "dispatch"),
        ("dispatch", "deploy"),
        ("deploy", "done"),
        ("deploy", "dispatch"),
        ("done", "dispatch"),
    ];
    let refused = [
        ("grounding", "dispatch"),
        ("plan", "done"),
        ("grounding", "deploy"),
    ];
    for (n, (from, to)) in allowed.iter().chain(refused.iter()).enumerate() {
        for ctx in ["codereview", "cpd"] {
            let id = format!("t{n}-{ctx}");
            let locked = if matches!(*from, "grounding" | "plan") {
                "NULL"
            } else {
                "'t'"
            };
            let done = if *from == "done" { "'t'" } else { "NULL" };
            write(
                &mut store,
                &format!(
                    "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
                       VALUES ('{id}', '{ctx}', 'T', '{from}', 'l1', 't', {locked}, {done});"
                ),
            )
            .unwrap_or_else(|e| panic!("fixture {from}: {e:?}"));
            let set = match *to {
                "done" => "phase = 'done', locked_at = 't2', done_at = 't2'".to_owned(),
                "dispatch" if *from == "done" => "phase = 'dispatch', done_at = NULL".to_owned(),
                "dispatch" | "deploy" if *from != "deploy" && *from != "dispatch" => {
                    format!("phase = '{to}', locked_at = 't2'")
                }
                to => format!("phase = '{to}'"),
            };
            let result = write(
                &mut store,
                &format!("UPDATE plans SET {set} WHERE id = '{id}'"),
            );
            if allowed.contains(&(*from, *to)) {
                result.unwrap_or_else(|e| panic!("{ctx} {from}>{to} is allowed: {e:?}"));
            } else {
                assert_illegal(result, &format!("{ctx} {from}>{to}"));
            }
        }
    }
}

#[test]
fn the_migration_changes_no_row() {
    let (_dir, home) = before();
    let mut old = crate::store::tests::open_named(&home, &earlier()).unwrap();
    // The later migration `agent-key-backfill` rewrites the names of the two Leads to their keys.
    let want = dump(&mut old)
        .replace("\"name\":\"L1\"", "\"name\":\"codereview:r-pay-812\"")
        .replace("\"name\":\"L2\"", "\"name\":\"cpd:c-pay\"");
    drop(old);
    let mut store = migrated_in(&home);
    assert_eq!(dump(&mut store), want);
}
