//! The migration `20261103T0700-tier-rows-v2` (contract §4.4, §4.5; Tier model v2; pro-d204, pro-n347): `tier-syntax-g-x`
//! rebuilt `jobs` and `tier_catalog` only, so `sessions.tier` and `attempts.tier` kept `CHECK (length(tier) = 2)`,
//! `jobs.retry_tier` had no CHECK and stored effort-5 values survived (an archived Manager `O5` made `Tier::parse` fail at
//! the read boundary). The migration rebuilds the three tables with the v2 CHECK, rewrites effort 5 to effort 4 in the
//! copy, and recreates `sessions_working_pr_valid` over `workflows`. Stores here are built to the migration before it.

use super::*;
use crate::config::Tier;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::store::Store;

const MIGRATION: &str = "20261103T0700-tier-rows-v2";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TABLES: [&str; 3] = ["sessions", "attempts", "jobs"];

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
    // `host-restarts` (a later migration) adds `sessions.host_restarts` and `host_window_start`: not what these test.
    let names: Vec<String> = names
        .into_iter()
        .filter(|n| !n.starts_with("host_"))
        .collect();
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

/// The migrations before `tier-rows-v2`. Fails while the migration is missing.
fn earlier() -> Vec<Named> {
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-tier-rows-v2"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "tier-rows-v2 is a built-in migration"
    );
    earlier
}

/// The Job columns that make a full running `impl` Job (every nullable column set).
fn full_job(key: &str, retry_tier: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, workflow, review, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, retry_tier, created_at, urgent, kind, test_approved, resources, skipped_n,
                           touches_approved)
           VALUES ('pay', 'api', 0, '{key}', 'Full', 'S3', 'goal', 'ctx', '[\"a\"]', '[\"b\"]', '[\"c\"]', 'running', 1,
                   '{FP}', cpd_txn(), '{retry_tier}', 'tj', 1, 'impl', '[\"t.rs\"]', '[\"gpu\"]', 3, '[\"x/y\"]');"
    )
}

/// A draft Job with no Spec (a stub) and the given `retry_tier` (`NULL` or `'H2'`).
fn stub_job(key: &str, retry_tier: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, status, appended, created_txn, retry_tier, created_at)
           VALUES ('pay', 'api', '{key}', 'Stub', NULL, 'draft', 0, cpd_txn(), {retry_tier}, 'tj');"
    )
}

/// The rows every test starts from: one of every kind with a column the copy could lose. Tiers: Sessions `O5` (archived
/// Manager `m5`), `S3` (Lead), `H5` and `S1` (Workers); Attempts `H5`, `S1`, `G2`; `retry_tier` `S5`, NULL, `H2`.
fn seed(extra: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at,
                               created_at, archived_at, title, titled_at, asleep_at, by_user, switch_failed, runner)
           VALUES ('m5', 'manager', 'ctx', NULL, NULL, 'Old Manager', '/s/m5.jsonl', '/m', 'O5', '%3', 'idle', NULL, 'ts', 'tc',
                   'ta', 'A title', 'tt', 'tz', 1, '{{\"to\":\"S3\"}}', 'durable');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead', 'lead', 'ctx', 'pay', 'Lead', '/', 'S3', 'working', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay', 'ctx', 'Plan', 'plan', 'lead', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'api', '/r', 'main', 'waiting', 'pay/api', cpd_txn(), 't');
         UPDATE sessions SET working_pr = 'api' WHERE id = 'lead';
         {}
         {}
         {}
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, retry_tier, cancel_reason, cancel_requested_at, created_at)
           VALUES ('pay', 'api', 'cancelled', 'C', 'H2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'cancelled', 1, '{FP}', cpd_txn(),
                   'H2', 'no longer needed', 'tcr', 'tj');
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, failed_cause, retry_tier, created_at)
           VALUES ('pay', 'api', 'failed', 'F', 'GX', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'failed', 0, '{FP}', cpd_txn(),
                   'gate_failed', 'S5', 'tj');
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, merged_sha, kind, tests, test_files, created_at)
           VALUES ('pay', 'api', 'tests', 'T', 'O4', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, '{FP}', cpd_txn(),
                   'abc123', 'test', '[\"t1\"]', '[\"t.rs\"]', 'tj');
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, merged_sha, kind, summary, created_at)
           VALUES ('pay', NULL, 'explore', 'E', 'H1', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, '{FP}', cpd_txn(),
                   'def456', 'scout', 'found it', 'tj');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha, driver_pid,
                               worker_session, spec_path, submitted_sha, submitted_at, check_tail, nudged, stop_request, metrics,
                               started_at, ended_at, runner, worker_pid, gate_hook, gate_command, gate_exit, actual_files)
           VALUES ('pay', 'full-a1', 'full', 1, 'H5', 'ladder', 'ended', 'failed', 'gate_failed', 'pay/full-a1', '/wt', 'b0b0', 11,
                   'w1', '/spec.md', 's1s1', 'tsub', 'tail', 1, 'timed_out', '{{\"turns\":3}}', 'tst', 'tend', 'durable', 12,
                   'pre-merge', 'make check', 1, '[\"f.rs\"]');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
           VALUES ('pay', 'full-a2', 'full', 2, 'S1', 'retry', 'running', 'pay/full-a2', '/wt2', 'w2', 'tst2');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, merged_sha,
                               started_at, ended_at)
           VALUES ('pay', 'tests-a1', 'tests', 1, 'G2', 'ladder', 'ended', 'merged', 'merged', 'pay/api', '/wt3', 'abc123',
                   'tst3', 'tend3');
         INSERT INTO sessions (id, role, context, plan, attempt, name, forked_from, cwd, tier, state, state_at, created_at,
                               archived_at)
           VALUES ('w1', 'worker', 'ctx', 'pay', 'full-a1', 'Worker 1', NULL, '/wt', 'H5', 'stopped', 't', 't', 'ta1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, forked_from, cwd, tier, state, state_at, created_at)
           VALUES ('w2', 'worker', 'ctx', 'pay', 'full-a2', 'Worker 2', 'w1', '/wt2', 'S1', 'working', 't', 't');
         {extra}",
        full_job("full", "S5"),
        stub_job("stub-null", "NULL"),
        stub_job("stub-h2", "'H2'"),
    )
}

/// A store as the migrations before `tier-rows-v2` leave it, with the seed (and `extra`).
fn before(extra: &str) -> (tempfile::TempDir, CpdHome) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = crate::store::tests::open_named(&home, &earlier()).unwrap();
    write(&mut store, &seed(extra)).unwrap_or_else(|e| panic!("fixture: {e:?}"));
    drop(store);
    (dir, home)
}

/// The store of `home` as the migrations before `tier-rows-v2` leave it.
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

fn session(id: &str, tier: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, forked_from)
           VALUES ('{id}', 'lead', 'ctx', 'pay', 'L', '/', '{tier}', 'idle', 't', 't', 'lead');"
    )
}

fn attempt(n: usize, tier: &str) -> String {
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at)
           VALUES ('pay', 'full-a{n}', 'full', {n}, '{tier}', 'ladder', 'ended', 'failed', 'crashed', 'pay/x{n}', '/wt', 't', 't');"
    )
}

fn assert_check_refused(result: Result<(), CpdError>, what: &str) {
    let refused = format!("{result:?}");
    assert!(
        refused.contains("CHECK constraint failed"),
        "{what}: {refused}"
    );
}

#[test]
fn the_migration_is_breaking_and_sorts_after_permanent_contexts() {
    let entry = NAMED
        .iter()
        .find(|n| n.name == MIGRATION)
        .unwrap_or_else(|| panic!("{MIGRATION} is a built-in migration"));
    assert!(
        entry.compat == Compat::Breaking,
        "the migration is breaking"
    );
    assert!(
        entry.name > "20261103T0600-permanent-contexts",
        "it sorts after permanent-contexts"
    );
    let before_it = LEGACY.len() + NAMED.iter().filter(|n| n.name < MIGRATION).count();
    assert_eq!(
        entry.sql.lines().next(),
        Some(format!("-- cpd-migration: breaking after {before_it}").as_str()),
        "the header counts the migrations that sort before it"
    );
    assert!(
        entry.sql.contains("PRAGMA legacy_alter_table = ON;"),
        "as tier-syntax-g-x does"
    );
    assert!(entry.run.is_none(), "SQL only");
}

#[test]
fn pre_v2_effort_5_rows_in_sessions_attempts_and_retry_tier_become_effort_4() {
    let (_dir, mut store) = migrated();
    let sessions = strings(
        &mut store,
        "SELECT id || '=' || tier FROM sessions ORDER BY id",
    );
    assert_eq!(sessions, ["lead=S3", "m5=O4", "w1=H4", "w2=S1"]);
    let attempts = strings(
        &mut store,
        "SELECT id || '=' || tier FROM attempts ORDER BY id",
    );
    assert_eq!(attempts, ["full-a1=H4", "full-a2=S1", "tests-a1=G2"]);
    let retry = strings(
        &mut store,
        "SELECT key || '=' || coalesce(retry_tier, 'NULL') FROM jobs ORDER BY key",
    );
    assert_eq!(
        retry,
        [
            "cancelled=H2",
            "explore=NULL",
            "failed=S4",
            "full=S4",
            "stub-h2=H2",
            "stub-null=NULL",
            "tests=NULL"
        ]
    );
}

#[test]
fn every_other_row_and_column_is_kept_byte_for_byte() {
    let (_dir, home) = before("");
    let mut old = old_store(&home);
    let order = [
        ("sessions", "id"),
        ("attempts", "plan, id"),
        ("jobs", "seq"),
    ];
    let snapshot: Vec<_> = order
        .iter()
        .map(|(t, by)| {
            let sql = select_all(&mut old, t, by);
            table(&mut old, &sql)
        })
        .collect();
    let history = count(&mut old, "SELECT count(*) FROM history");
    drop(old);

    let mut store = migrated_in(&home);
    let effort_4 = |value: &mut Value| {
        if let Some(t) = value
            .as_str()
            .filter(|t| t.as_bytes().get(1) == Some(&b'5'))
        {
            *value = Value::from(format!("{}4", &t[..1]));
        }
    };
    for ((t, by), (cols, mut rows)) in order.iter().zip(snapshot) {
        let tier_columns: &[&str] = if *t == "jobs" {
            &["retry_tier"]
        } else {
            &["tier"]
        };
        for row in &mut rows {
            for (name, value) in cols.iter().zip(row.iter_mut()) {
                if tier_columns.contains(&name.as_str()) {
                    effort_4(value);
                }
            }
        }
        if *t == "sessions" {
            // The later migration `agent-key-backfill` rewrites the name of a Manager, a Lead that is not a fork and a
            // Worker to its key (every row here is one of them).
            let at = |c: &str| cols.iter().position(|n| n == c).unwrap();
            for row in &mut rows {
                let text = |c: &str| row[at(c)].as_str().unwrap_or_default().to_owned();
                let (context, plan) = (text("context"), text("plan"));
                let key = match text("role").as_str() {
                    "manager" => context,
                    "lead" => format!("{context}:{plan}"),
                    _ => {
                        let attempt = text("attempt");
                        format!("{context}:{plan}:{}", attempt.rsplit_once("-a").unwrap().0)
                    }
                };
                row[at("name")] = Value::from(key);
            }
        }
        let sql = select_all(&mut store, t, by);
        let (new_cols, new_rows) = table(&mut store, &sql);
        assert_eq!(new_cols, cols, "{t}: the same columns, in the same order");
        assert_eq!(new_rows, rows, "{t}: every row, every column");
    }
    assert_eq!(
        count(&mut store, "SELECT count(*) FROM history"),
        // the one `workflow-reopened` row of `event_kinds` (`workflow-reopen`), the one `guard` row of `event_kinds`
        // (`guard-event`) and the `context_reads` and `record_topics` rows of `history_tables` (`context-reads`,
        // `record-topics`); `refusal-alert-kind` rebuilds `inbox_items` and writes no history row
        history + 4,
        "the copy writes no history row"
    );
}

#[test]
fn sessions_and_attempts_accept_g_and_x_and_refuse_effort_5_and_bad_tiers() {
    let (_dir, mut store) = migrated();
    for (i, tier) in ["GX", "HX", "G2", "O4"].into_iter().enumerate() {
        write(&mut store, &session(&format!("ok{i}"), tier))
            .unwrap_or_else(|e| panic!("session {tier}: {e:?}"));
        write(&mut store, &attempt(10 + i, tier))
            .unwrap_or_else(|e| panic!("attempt {tier}: {e:?}"));
    }
    for (i, tier) in ["O5", "s2", "S22", "S", "Z1"].into_iter().enumerate() {
        assert_check_refused(
            write(&mut store, &session(&format!("bad{i}"), tier)),
            &format!("session {tier}"),
        );
        assert_check_refused(
            write(&mut store, &attempt(20 + i, tier)),
            &format!("attempt {tier}"),
        );
    }
    assert_eq!(count(&mut store, "SELECT count(*) FROM sessions"), 4 + 4);
    assert_eq!(count(&mut store, "SELECT count(*) FROM attempts"), 3 + 4);
}

#[test]
fn jobs_retry_tier_has_the_tier_v2_check() {
    let (_dir, mut store) = migrated();
    let set = |tier: &str| format!("UPDATE jobs SET retry_tier = {tier} WHERE key = 'stub-null'");
    for tier in ["'G2'", "'HX'", "NULL"] {
        write(&mut store, &set(tier)).unwrap_or_else(|e| panic!("{tier}: {e:?}"));
    }
    for tier in ["'S5'", "'Z1'", "'s2'"] {
        assert_check_refused(write(&mut store, &set(tier)), tier);
    }
    assert_eq!(
        strings(
            &mut store,
            "SELECT coalesce(retry_tier, 'NULL') FROM jobs WHERE key = 'stub-null'"
        ),
        ["NULL"]
    );
}

#[test]
fn a_stored_tier_outside_the_syntax_fails_the_migration() {
    let (_dir, home) = before(&session("bad", "Z9"));
    let refused = Store::open(&home, 5000).err().expect("the migration fails");
    assert!(
        format!("{refused:?}").contains("CHECK constraint failed"),
        "{refused:?}"
    );
    let mut old = old_store(&home);
    assert_eq!(
        strings(&mut old, "SELECT tier FROM sessions WHERE id = 'bad'"),
        ["Z9"],
        "the store is as it was"
    );
    assert_eq!(
        strings(&mut old, "SELECT tier FROM sessions WHERE id = 'm5'"),
        ["O5"],
        "no row was rewritten"
    );
    assert_eq!(
        count(
            &mut old,
            &format!("SELECT count(*) FROM schema_migrations WHERE name = '{MIGRATION}'")
        ),
        0,
        "the migration is not recorded"
    );
}

#[test]
fn sessions_working_pr_valid_names_workflows_and_still_works() {
    let (_dir, mut store) = migrated();
    let sql = strings(
        &mut store,
        "SELECT sql FROM sqlite_master WHERE name = 'sessions_working_pr_valid'",
    );
    assert_eq!(sql.len(), 1);
    assert!(sql[0].contains("workflows"), "{}", sql[0]);
    assert!(!sql[0].contains("prs"), "{}", sql[0]);
    write(
        &mut store,
        "UPDATE sessions SET working_pr = NULL WHERE id = 'lead'",
    )
    .unwrap();
    write(
        &mut store,
        "UPDATE sessions SET working_pr = 'api' WHERE id = 'lead'",
    )
    .expect("a Workflow of the Plan");
    assert_eq!(
        strings(
            &mut store,
            "SELECT working_pr FROM sessions WHERE id = 'lead'"
        ),
        ["api"]
    );
    let err = write(
        &mut store,
        "UPDATE sessions SET working_pr = 'nope' WHERE id = 'lead'",
    )
    .expect_err("no such Workflow");
    assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
    assert_eq!(
        err.details()["sqlite_message"].as_str().unwrap_or_default(),
        "working_pr_invalid"
    );
}

#[test]
fn the_rebuilt_tables_keep_their_indexes_and_triggers() {
    let sql = "SELECT type || ' ' || name || ' ON ' || tbl_name FROM sqlite_master
               WHERE type IN ('index', 'trigger') AND name NOT LIKE 'sqlite_autoindex%'
                 AND tbl_name IN ('sessions', 'attempts', 'jobs') ORDER BY 1";
    let (_dir, home) = before("");
    let mut old = old_store(&home);
    let mut names = strings(&mut old, sql);
    assert!(
        names.len() > 20,
        "the live schema has indexes and triggers on the three tables: {names:?}"
    );
    drop(old);
    let mut store = migrated_in(&home);
    // the migration `20261103T0900-agent-key-schema` adds this index (one Lead per Plan)
    names.push("index sessions_one_lead ON sessions".to_owned());
    names.sort();
    assert_eq!(strings(&mut store, sql), names);
    for t in TABLES {
        let strict: i64 = count(
            &mut store,
            &format!("SELECT strict FROM pragma_table_list('{t}')"),
        );
        assert_eq!(strict, 1, "{t} is STRICT");
    }
}

#[test]
fn the_tree_reader_parses_every_tier_the_migrated_store_holds() {
    let (_dir, mut store) = migrated();
    let tiers = strings(
        &mut store,
        "SELECT tier FROM sessions UNION ALL SELECT tier FROM attempts
         UNION ALL SELECT retry_tier FROM jobs WHERE retry_tier IS NOT NULL",
    );
    assert_eq!(tiers.len(), 4 + 3 + 4, "{tiers:?}");
    for tier in tiers {
        Tier::parse(&tier).unwrap_or_else(|e| panic!("{tier}: {e:?}"));
    }
}
