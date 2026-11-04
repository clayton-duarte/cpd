//! The migration `20261103T1000-agent-key-backfill` (contract §4.5 `agent-key-backfill`; pro-d345, pro-d346): a data-only
//! rename of every Manager, non-fork Lead and Worker Session to its agent key. Stores here are built to the migration
//! before it.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const MIGRATION: &str = "20261103T1000-agent-key-backfill";

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

/// The migrations before `agent-key-backfill`. Fails while the migration is missing.
fn earlier() -> Vec<Named> {
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-agent-key-backfill"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "agent-key-backfill is a built-in migration"
    );
    earlier
}

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn long(first: char, len: usize) -> String {
    format!("{first}{}", "x".repeat(len - 1))
}

/// A Context with its Plan, Lead, Workflow and Job; `n` Attempts (and their Worker Sessions) of the Job.
fn family(ctx: &str, plan: &str, job: &str, attempts: usize, manager_archived: bool) -> String {
    let archived = if manager_archived { "'ta'" } else { "NULL" };
    let mut sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('{ctx}', 'C', 't', 'm');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at,
                               created_at, archived_at, title, titled_at, asleep_at, by_user, switch_failed, runner)
           VALUES ('m-{ctx}', 'manager', '{ctx}', NULL, NULL, '{ctx}-manager', '/s/m.jsonl', '/m', 'O4', '%3', 'idle', NULL,
                   'ts', 'tc', {archived}, 'A title', 'tt', 'tz', 1, '{{\"to\":\"S3\"}}', 'durable');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, archived_at)
           VALUES ('l-{plan}', 'lead', '{ctx}', '{plan}', '{plan}', '/', 'S3', 'working', 't', 't', {archived});
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('{plan}', '{ctx}', 'Plan', 'plan', 'l-{plan}', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('{plan}', 'api', '/r', 'main', 'waiting', '{plan}/api', cpd_txn(), 't');
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, created_at)
           VALUES ('{plan}', 'api', '{job}', 'J', 'S1', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{FP}',
                   cpd_txn(), 'tj');
         INSERT INTO sessions (id, role, context, plan, name, forked_from, cwd, tier, state, state_at, created_at)
           VALUES ('f-{plan}', 'lead', '{ctx}', '{plan}', 'Lead (fork)', 'l-{plan}', '/', 'S3', 'idle', 't', 't');
"
    );
    for n in 1..=attempts {
        let status = if n < attempts {
            "'ended', 'failed', 'crashed'"
        } else {
            "'running', NULL, NULL"
        };
        let ended = if n < attempts { "'t'" } else { "NULL" };
        sql.push_str(&format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, worker_session, started_at, ended_at)
               VALUES ('{plan}', '{job}-a{n}', '{job}', {n}, 'S1', 'ladder', {status}, '{plan}/{job}-a{n}', '/wt', 'w{n}-{plan}', 't', {ended});
             INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
               VALUES ('w{n}-{plan}', 'worker', '{ctx}', '{plan}', '{job}-a{n}', 'Worker {n}', '/wt', 'S1', 'working', 't', 't');
"
        ));
    }
    sql
}

/// Two small families (the second one archived) and one with the longest ids.
fn seed() -> String {
    format!(
        "{}{}{}",
        family("cpd", "lateral-comms", "foo", 2, false),
        family("old", "plan", "bar", 1, true),
        family(&long('c', 40), &long('p', 24), &long('j', 32), 1, false)
    )
}

fn before() -> (tempfile::TempDir, CpdHome) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = crate::store::tests::open_named(&home, &earlier()).unwrap();
    write(&mut store, &seed()).unwrap_or_else(|e| panic!("fixture: {e:?}"));
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

/// `id=name` of every Session of `role`, by id.
fn names(store: &mut Store, role: &str) -> Vec<String> {
    strings(
        store,
        &format!("SELECT id || '=' || name FROM sessions WHERE role = '{role}' ORDER BY id"),
    )
}

fn expected(pairs: &[(&str, &str)]) -> Vec<String> {
    let mut v: Vec<String> = pairs.iter().map(|(i, n)| format!("{i}={n}")).collect();
    v.sort();
    v
}

#[test]
fn agent_key_backfill_is_a_breaking_migration_after_101() {
    let entry = NAMED
        .iter()
        .find(|n| n.name == MIGRATION)
        .unwrap_or_else(|| panic!("{MIGRATION} is a built-in migration"));
    assert_eq!(
        entry.sql.lines().next(),
        Some("-- cpd-migration: breaking after 101")
    );
    assert!(
        entry.compat == Compat::Breaking,
        "the migration is breaking"
    );
    assert_eq!(
        NAMED.iter().rev().nth(5).map(|n| n.name),
        Some(MIGRATION),
        "only `workflow-reopen`, `context-reads`, `context-supersedes-list`, `drop-question-answer` and `workflow-base-retarget` come after it"
    );
    assert_eq!(
        NAMED.iter().rev().nth(4).map(|n| n.name),
        Some("20261103T1100-workflow-reopen")
    );
    assert_eq!(
        NAMED.iter().rev().nth(3).map(|n| n.name),
        Some("20261103T1200-context-reads")
    );
    assert_eq!(
        NAMED.iter().rev().nth(2).map(|n| n.name),
        Some("20261103T2150-context-supersedes-list")
    );
    assert_eq!(
        NAMED.iter().rev().nth(1).map(|n| n.name),
        Some("20261103T2200-drop-question-answer")
    );
    assert_eq!(
        NAMED.iter().next_back().map(|n| n.name),
        Some("20261104T0900-workflow-base-retarget")
    );
}

#[test]
fn agent_key_backfill_names_every_manager_by_its_context_archived_too() {
    let (_dir, home) = before();
    assert_eq!(
        names(&mut old_store(&home), "manager")[0],
        "m-cpd=cpd-manager"
    );
    let mut store = migrated_in(&home);
    let long_ctx = long('c', 40);
    let mut want = expected(&[("m-cpd", "cpd"), ("m-old", "old")]);
    want.push(format!("m-{long_ctx}={long_ctx}"));
    want.sort();
    assert_eq!(names(&mut store, "manager"), want);
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM sessions WHERE role = 'manager' AND archived_at IS NOT NULL"
        ),
        1,
        "the archived Manager is still archived"
    );
}

#[test]
fn agent_key_backfill_names_every_non_fork_lead_context_colon_plan() {
    let (_dir, home) = before();
    let mut store = migrated_in(&home);
    let (c, p) = (long('c', 40), long('p', 24));
    let got = strings(
        &mut store,
        "SELECT id || '=' || name FROM sessions WHERE role = 'lead' AND forked_from IS NULL ORDER BY id",
    );
    let mut want = expected(&[
        ("l-lateral-comms", "cpd:lateral-comms"),
        ("l-plan", "old:plan"),
    ]);
    want.push(format!("l-{p}={c}:{p}"));
    want.sort();
    assert_eq!(got, want);
    assert_eq!(format!("{c}:{p}").len(), 65);
}

#[test]
fn agent_key_backfill_names_every_worker_context_plan_job_for_every_attempt() {
    let (_dir, home) = before();
    let mut store = migrated_in(&home);
    let (c, p, j) = (long('c', 40), long('p', 24), long('j', 32));
    let mut want = expected(&[
        ("w1-lateral-comms", "cpd:lateral-comms:foo"),
        ("w2-lateral-comms", "cpd:lateral-comms:foo"),
        ("w1-plan", "old:plan:bar"),
    ]);
    want.push(format!("w1-{p}={c}:{p}:{j}"));
    want.sort();
    assert_eq!(names(&mut store, "worker"), want);
    assert_eq!(format!("{c}:{p}:{j}").len(), 98);
}

#[test]
fn agent_key_backfill_leaves_a_fork_name_alone() {
    let (_dir, home) = before();
    let mut store = migrated_in(&home);
    let got = strings(
        &mut store,
        "SELECT name FROM sessions WHERE forked_from IS NOT NULL ORDER BY id",
    );
    assert_eq!(got, ["Lead (fork)"; 3]);
}

#[test]
fn agent_key_backfill_changes_only_the_name_column() {
    let (_dir, home) = before();
    let mut old = old_store(&home);
    let sql = select_all(&mut old, "sessions", "rowid");
    let (cols, old_rows) = table(&mut old, &sql);
    let tables = ["contexts", "plans", "workflows", "jobs", "attempts"];
    let mut old_other = Vec::new();
    for t in tables {
        let sql = select_all(&mut old, t, "rowid");
        old_other.push(table(&mut old, &sql));
    }
    drop(old);
    let mut store = migrated_in(&home);
    let sql = select_all(&mut store, "sessions", "rowid");
    let (new_cols, new_rows) = table(&mut store, &sql);
    assert_eq!(cols, new_cols);
    assert_eq!(old_rows.len(), new_rows.len(), "the row count");
    let name = cols.iter().position(|c| c == "name").unwrap();
    let mut renamed = 0;
    for (o, n) in old_rows.iter().zip(&new_rows) {
        for (i, (a, b)) in o.iter().zip(n).enumerate() {
            if i != name {
                assert_eq!(a, b, "column {} of {}", cols[i], o[0]);
            }
        }
        renamed += usize::from(o[name] != n[name]);
    }
    assert_eq!(renamed, new_rows.len() - 3, "every row but the forks");
    for (t, before) in tables.iter().zip(old_other) {
        let sql = select_all(&mut store, t, "rowid");
        assert_eq!(table(&mut store, &sql), before, "{t} is untouched");
    }
}
