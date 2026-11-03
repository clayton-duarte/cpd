//! The migration `20261103T0900-agent-key-schema` (contract §4.4 `sessions`, §4.5; pro-d345, pro-d346): a Session name is
//! an agent key, up to 98 characters (`<context>:<plan>:<job>`); one non-fork Lead per Plan; a non-fork Lead has a Plan.
//! Stores here are built to the migration before it.

use super::*;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::store::Store;

const MIGRATION: &str = "20261103T0900-agent-key-schema";

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

/// The column names and the rows of a query, each value a JSON value (NULL as `null`).
fn table(store: &mut Store, sql: &str) -> (Vec<String>, Vec<Vec<Value>>) {
    store
        .read(|c: &Connection| {
            let mut stmt = c.prepare(sql)?;
            let names: Vec<String> = stmt
                .column_names()
                .iter()
                .map(|n| (*n).to_owned())
                .collect();
            let n = names.len();
            let rows = stmt
                .query_map([], |r| {
                    (0..n)
                        .map(|i| {
                            Ok(match r.get_ref(i)? {
                                rusqlite::types::ValueRef::Null => Value::Null,
                                rusqlite::types::ValueRef::Integer(v) => Value::from(v),
                                rusqlite::types::ValueRef::Text(t) => {
                                    Value::from(String::from_utf8_lossy(t).into_owned())
                                }
                                other => panic!("{other:?}"),
                            })
                        })
                        .collect::<rusqlite::Result<Vec<Value>>>()
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((names, rows))
        })
        .unwrap()
}

/// `SELECT <every column of t, in table order> FROM t ORDER BY <by>`: the columns are named, never `*`.
fn select_all(store: &mut Store, t: &str, by: &str) -> String {
    let names = strings(
        store,
        &format!("SELECT name FROM pragma_table_info('{t}') ORDER BY cid"),
    );
    format!("SELECT {} FROM {t} ORDER BY {by}", names.join(", "))
}

/// The first column of every row, as text (`NULL` for a null).
fn strings(store: &mut Store, sql: &str) -> Vec<String> {
    table(store, sql)
        .1
        .into_iter()
        .map(|row| match &row[0] {
            Value::Null => "NULL".to_owned(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect()
}

fn count(store: &mut Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

/// The migrations before `agent-key-schema`. Fails while the migration is missing.
fn earlier() -> Vec<Named> {
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-agent-key-schema"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "agent-key-schema is a built-in migration"
    );
    earlier
}

/// A Manager, a Lead, a fork Lead and a Worker (with its Attempt, Job and Plan rows); every nullable column of the
/// Manager set.
const SEED: &str = "
INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at,
                      created_at, archived_at, title, titled_at, asleep_at, by_user, switch_failed, runner)
  VALUES ('m1', 'manager', 'ctx', NULL, NULL, 'ctx', '/s/m1.jsonl', '/m', 'O4', '%3', 'idle', NULL, 'ts', 'tc',
          NULL, 'A title', 'tt', 'tz', 1, '{\"to\":\"S3\"}', 'durable');
INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
  VALUES ('lead', 'lead', 'ctx', 'pay', 'Lead', '/', 'S3', 'working', 't', 't');
INSERT INTO plans (id, context, title, phase, lead_session, created_at)
  VALUES ('pay', 'ctx', 'Plan', 'plan', 'lead', 't');
INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
  VALUES ('pay', 'api', '/r', 'main', 'waiting', 'pay/api', cpd_txn(), 't');
UPDATE sessions SET working_pr = 'api' WHERE id = 'lead';
INSERT INTO sessions (id, role, context, plan, name, forked_from, cwd, tier, state, state_at, created_at)
  VALUES ('flead', 'lead', 'ctx', 'pay', 'Lead (fork)', 'lead', '/', 'S3', 'idle', 't', 't');
INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                  fingerprint, created_txn, created_at)
  VALUES ('pay', 'api', 'full', 'Full', 'S1', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0,
          'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', cpd_txn(), 'tj');
INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
  VALUES ('pay', 'full-a1', 'full', 1, 'S1', 'ladder', 'running', 'pay/full-a1', '/wt', 'w1', 'tst');
INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
  VALUES ('w1', 'worker', 'ctx', 'pay', 'full-a1', 'ctx:pay:full', '/wt', 'S1', 'working', 't', 't');
";

/// A store as the migrations before `agent-key-schema` leave it, with the seed (and `extra`).
fn before(extra: &str) -> (tempfile::TempDir, CpdHome) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = crate::store::tests::open_named(&home, &earlier()).unwrap();
    write(&mut store, &format!("{SEED}{extra}")).unwrap_or_else(|e| panic!("fixture: {e:?}"));
    drop(store);
    (dir, home)
}

fn old_store(home: &CpdHome) -> Store {
    crate::store::tests::open_named(home, &earlier()).unwrap()
}

fn migrated_in(home: &CpdHome) -> Store {
    let mut store = Store::open(home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(
        strings(
            &mut store,
            &format!("SELECT name FROM schema_migrations WHERE name = '{MIGRATION}'")
        ),
        [MIGRATION],
        "the migration is applied"
    );
    store
}

fn migrated() -> (tempfile::TempDir, Store) {
    let (dir, home) = before("");
    let store = migrated_in(&home);
    (dir, store)
}

fn lead(id: &str, name: &str, plan: &str, forked_from: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, name, forked_from, cwd, tier, state, state_at, created_at)
           VALUES ('{id}', 'lead', 'ctx', {plan}, '{name}', {forked_from}, '/', 'S3', 'idle', 't', 't');"
    )
}

fn refused(result: Result<(), CpdError>, needle: &str, what: &str) {
    let text = format!("{result:?}");
    assert!(text.contains(needle), "{what}: {text}");
}

fn sessions_snapshot(store: &mut Store) -> (Vec<String>, Vec<Vec<Value>>) {
    let sql = select_all(store, "sessions", "rowid");
    table(store, &sql)
}

#[test]
fn agent_key_schema_is_a_breaking_migration_after_100() {
    let entry = NAMED
        .iter()
        .find(|n| n.name == MIGRATION)
        .unwrap_or_else(|| panic!("{MIGRATION} is a built-in migration"));
    assert_eq!(
        entry.sql.lines().next(),
        Some("-- cpd-migration: breaking after 100")
    );
    assert!(
        entry.compat == Compat::Breaking,
        "the migration is breaking"
    );
    assert_eq!(
        NAMED.last().map(|n| n.name),
        Some(MIGRATION),
        "it is the last migration"
    );
    let (_dir, home) = before("");
    drop(migrated_in(&home));
    let err = crate::store::tests::open_named(&home, &earlier())
        .err()
        .expect("an older binary refuses the store");
    assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
}

#[test]
fn agent_key_schema_keeps_every_session_row_and_column() {
    let (_dir, home) = before("");
    let mut old = old_store(&home);
    let snapshot = sessions_snapshot(&mut old);
    let ids = strings(&mut old, "SELECT id FROM sessions ORDER BY rowid");
    assert_eq!(ids, ["m1", "lead", "flead", "w1"]);
    drop(old);
    let mut store = migrated_in(&home);
    assert_eq!(sessions_snapshot(&mut store), snapshot);
    assert_eq!(
        strings(&mut store, "SELECT id FROM sessions ORDER BY rowid"),
        ids
    );
}

#[test]
fn agent_key_schema_recreates_the_indexes_and_working_pr_triggers() {
    let (_dir, home) = before("");
    let mut old = old_store(&home);
    let triggers = "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'sessions' ORDER BY name";
    let old_triggers = strings(&mut old, triggers);
    drop(old);
    let mut store = migrated_in(&home);
    assert_eq!(strings(&mut store, triggers), old_triggers);
    let names = strings(
        &mut store,
        "SELECT name FROM sqlite_master WHERE tbl_name = 'sessions' AND type IN ('index', 'trigger') ORDER BY name",
    );
    for want in [
        "sessions_plan",
        "sessions_one_manager",
        "sessions_live",
        "sessions_one_lead",
        "sessions_working_pr_new",
        "sessions_working_pr_valid",
        "sessions_working_pr_clear",
    ] {
        assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
    }
    assert!(
        old_triggers.iter().any(|n| n.contains("_hist_")),
        "the history triggers are generated: {old_triggers:?}"
    );
    let history = count(&mut store, "SELECT count(*) FROM history");
    write(&mut store, &lead("l2", "L2", "'pay'", "'lead'")).unwrap();
    assert!(count(&mut store, "SELECT count(*) FROM history") > history);
    refused(
        write(
            &mut store,
            "UPDATE sessions SET working_pr = 'nope' WHERE id = 'lead'",
        ),
        "working_pr_invalid",
        "the working_pr trigger works",
    );
}

#[test]
fn agent_key_schema_accepts_a_name_of_98_characters_and_refuses_99_and_empty() {
    let (_dir, mut store) = migrated();
    let key = |n: usize| "a".repeat(n);
    let named = |id: &str, name: &str| {
        format!(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
               VALUES ('{id}', 'worker', 'ctx', 'pay', 'full-a1', '{name}', '/', 'S1', 'idle', 't', 't');"
        )
    };
    write(&mut store, &named("w98", &key(98))).unwrap();
    for (id, name) in [("w99", key(99)), ("w0", String::new())] {
        refused(
            write(&mut store, &named(id, &name)),
            "CHECK constraint failed",
            "name length",
        );
    }
    let widest = format!("{}:{}:{}", "c".repeat(40), "p".repeat(24), "j".repeat(32));
    assert_eq!(widest.len(), 98);
    write(&mut store, &named("wkey", &widest)).unwrap();
}

#[test]
fn agent_key_schema_refuses_a_second_non_fork_lead_on_a_plan() {
    let (_dir, mut store) = migrated();
    refused(
        write(&mut store, &lead("l2", "L2", "'pay'", "NULL")),
        "UNIQUE constraint failed",
        "second Lead",
    );
    write(
        &mut store,
        "UPDATE sessions SET archived_at = 'x' WHERE id = 'lead'",
    )
    .unwrap();
    refused(
        write(&mut store, &lead("l2", "L2", "'pay'", "NULL")),
        "UNIQUE constraint failed",
        "second Lead after archiving the first",
    );
}

#[test]
fn agent_key_schema_lets_a_fork_share_the_plan_of_a_lead() {
    let (_dir, mut store) = migrated();
    write(
        &mut store,
        &lead("f2", "Lead (fork) (fork)", "'pay'", "'lead'"),
    )
    .unwrap();
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM sessions WHERE role = 'lead' AND plan = 'pay'"
        ),
        3
    );
}

#[test]
fn agent_key_schema_refuses_a_non_fork_lead_without_a_plan() {
    let (_dir, mut store) = migrated();
    refused(
        write(&mut store, &lead("l2", "L2", "NULL", "NULL")),
        "CHECK constraint failed",
        "Lead without a Plan",
    );
}

#[test]
fn agent_key_schema_lets_a_fork_have_no_plan_and_a_manager_never_has_one() {
    let (_dir, mut store) = migrated();
    write(&mut store, &lead("f2", "F", "NULL", "'lead'")).unwrap();
    let manager = |id: &str, ctx: &str, plan: &str| {
        format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('{id}', 'manager', '{ctx}', {plan}, '{ctx}', '/', 'O4', 'idle', 't', 't');"
        )
    };
    write(
        &mut store,
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('c2', 'C2', 't', 'm');
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('c3', 'C3', 't', 'm');",
    )
    .unwrap();
    refused(
        write(&mut store, &manager("mp", "c2", "'pay'")),
        "CHECK constraint failed",
        "Manager with a Plan",
    );
    write(&mut store, &manager("mn", "c3", "NULL")).unwrap();
}

/// Opening `home` with the new binary is refused and nothing is applied.
fn assert_open_refused_and_unchanged(home: &CpdHome) {
    let mut old = old_store(home);
    let applied = strings(&mut old, "SELECT name FROM schema_migrations ORDER BY name");
    let rows = sessions_snapshot(&mut old);
    drop(old);
    let err = Store::open(home, 5000).err().expect("the store is refused");
    assert_eq!(err.code(), ErrorCode::MigrationFailed, "{err:?}");
    let mut after = old_store(home);
    assert_eq!(
        strings(
            &mut after,
            "SELECT name FROM schema_migrations ORDER BY name"
        ),
        applied
    );
    assert_eq!(sessions_snapshot(&mut after), rows);
}

#[test]
fn agent_key_schema_refuses_a_store_with_two_non_fork_leads_on_a_plan() {
    let (_dir, home) = before(&lead("l2", "L2", "'pay'", "NULL"));
    assert_open_refused_and_unchanged(&home);
}

#[test]
fn agent_key_schema_refuses_a_store_with_a_non_fork_lead_without_a_plan() {
    let (_dir, home) = before(&lead("l2", "L2", "NULL", "NULL"));
    assert_open_refused_and_unchanged(&home);
}
