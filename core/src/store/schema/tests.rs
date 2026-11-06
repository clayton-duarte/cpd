//! The tests of the migrations and of the schema they build (§4.4, §4.5).

use rusqlite::functions::FunctionFlags;

use super::*;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::store::{APPLICATION_ID, Store};

/// A connection with a stand-in `cpd_txn()`, which the history triggers call.
/// Fixtures only: no fsync (`synchronous = OFF`), since a
/// crash loses a throwaway database nobody reads again; what the tests claim is
/// the schema and the rows, not durability, and the fsyncs of every fixture
/// commit were the larger part of the time on a loaded disk.
fn connection(path: &std::path::Path) -> Connection {
    prepared(Connection::open(path).unwrap())
}

fn prepared(conn: Connection) -> Connection {
    conn.pragma_update(None, "synchronous", "OFF").unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, |_| Ok(1_i64))
        .unwrap();
    conn
}

/// The schema after the first `n` migrations only, with the pragmas
/// `Store::open` sets around them (§4.5 migration 4, step 1). Legacy
/// migration 7's import of the old Context stores is the runner's, not run here.
fn at_version(conn: &Connection, n: usize) {
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    conn.pragma_update(None, "legacy_alter_table", "ON")
        .unwrap();
    for legacy in &LEGACY[..n] {
        (legacy.apply)(conn).unwrap();
    }
    conn.pragma_update(None, "legacy_alter_table", "OFF")
        .unwrap();
    conn.pragma_update(None, "user_version", n as i64).unwrap();
}

/// The six Event kinds of the object before the rename (`pr-added` and its siblings, `workflow-event-kinds`):
/// `workflow-event-kinds` adds their new names and leaves these in `event_kinds` as history, since past Events
/// hold them (contract §4.5 K1, §5.2).
const RETIRED_OBJECT_KINDS: usize = 6;

#[test]
fn migrating_an_empty_database_gives_every_versions_schema_hash() {
    // One database taken through the migrations one at a time: the schema after
    // `n` of them is the same as one built from scratch to `n` (`at_version`).
    let conn = prepared(Connection::open_in_memory().unwrap());
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    for (n, expected) in SCHEMA_HASHES.iter().enumerate() {
        conn.pragma_update(None, "legacy_alter_table", "ON")
            .unwrap();
        (LEGACY[n].apply)(&conn).unwrap();
        conn.pragma_update(None, "legacy_alter_table", "OFF")
            .unwrap();
        assert_eq!(
            legacy_schema_hash(&conn).unwrap(),
            *expected,
            "version {}",
            n + 1
        );
    }
}

fn home(dir: &tempfile::TempDir) -> CpdHome {
    CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap()
}

/// A database at version `n` holding `sql`, written as cpd's txn 1.
fn database_at(home: &CpdHome, n: usize, sql: &str) {
    std::fs::create_dir_all(home.db().parent().unwrap()).unwrap();
    let conn = connection(&home.db());
    conn.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    at_version(&conn, n);
    conn.execute_batch(&format!(
        "BEGIN;
         INSERT INTO txns (id, at, command, pid, session, cwd) VALUES (1, '2026-01-01T00:00:00.000Z', 'migrate', 1, NULL, '/');
         {sql}
         COMMIT;"
    ))
    .unwrap();
}

// ---------- migration 2 ----------

const VERSION_1_PLAN: &str =
    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '2026-01-01T00:00:00.000Z', 'm1');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('l-pay', 'lead', 'ctx', 'pay', 'lead', '/repo', 'O3', 'idle', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z');
     INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session,
                        pr_number, pr_url, pr_opened_at, created_at, locked_at)
       VALUES ('pay', 'ctx', 'Pay', '/repo', 'main', 'cpd/pay', '/wt', 'pr', 'l-pay', 7, 'https://github.com/acme/pay/pull/7',
               '2026-01-05T00:00:00.000Z', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');";

#[test]
fn a_version_1_database_migrates_and_its_pr_gets_a_row_titled_like_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    database_at(&home, 1, VERSION_1_PLAN);

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    type Row = (String, i64, String, String, String, String);
    let (version, row, migrations): (i64, Row, i64) = store
        .read(|c| {
            Ok((
                c.pragma_query_value(None, "user_version", |r| r.get(0))?,
                c.query_row(
                    "SELECT r.key, r.pr_number, p.repo, p.title, p.review_decision, p.checks
                       FROM workflows r JOIN pull_requests p ON p.id = r.pull_request WHERE r.plan = 'pay'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                )?,
                c.query_row("SELECT count(*) FROM txns WHERE command = 'migrate'", [], |r| r.get(0))?,
            ))
        })
        .unwrap();
    assert_eq!(version, VERSION);
    let expected: Row = (
        "repo".into(),
        7,
        "acme/pay".into(),
        "Pay".into(),
        "none".into(),
        "none".into(),
    );
    assert_eq!(
        row, expected,
        "no title was polled: the Plan's title, and no review or checks"
    );
    assert_eq!(migrations, 2, "the old migration's row and the new one");
}

#[test]
fn pull_requests_history_lists_every_column_and_their_checks_hold() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    database_at(&home, 1, VERSION_1_PLAN);
    let mut store = Store::open(&home, 5000).unwrap();

    store
        .write("test", |tx| {
            tx.execute(
                "UPDATE pull_requests SET title = 'T', review_decision = 'approved', checks = 'passed', draft = 1, mergeable = 'conflicting', merge_state = 'dirty' WHERE number = 7",
                [],
            )?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    let after: String = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT after FROM history WHERE tbl = 'pull_requests' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    let after: Value = serde_json::from_str(&after).unwrap();
    assert_eq!(after["title"], "T");
    assert_eq!(after["review_decision"], "approved");
    assert_eq!(after["checks"], "passed");
    assert_eq!(after["draft"], 1);
    assert_eq!(
        (&after["mergeable"], &after["merge_state"]),
        (&Value::from("conflicting"), &Value::from("dirty"))
    );

    for bad in [
        "title = ''",
        "review_decision = 'maybe'",
        "checks = 'green'",
        "state = 'merged'",
        "repo = 'x'",
        "number = 0",
        "draft = 2",
        "mergeable = 'maybe'",
        "merge_state = 'ready'",
    ] {
        let sql = format!("UPDATE pull_requests SET {bad} WHERE number = 7");
        let err = store
            .write("test", |tx| {
                tx.execute(&sql, [])?;
                Ok::<(), CpdError>(())
            })
            .unwrap_err();
        assert!(
            err.to_string().contains("CHECK") || format!("{err:?}").contains("CHECK"),
            "{bad}: {err:?}"
        );
    }
}

// ---------- migration 3 ----------

/// A version 2 database with Events, as a real `cpd.db` is; the Event ids are 1, 2 and 5
/// (not contiguous), so the next one must be above 5.
fn version_2_with_events(home: &CpdHome) {
    let events: String = [1, 2, 5]
        .iter()
        .map(|id| {
            format!(
                "INSERT INTO events (id, txn, at, kind, context, plan, text, data)
                   VALUES ({id}, 1, '2026-01-01T00:00:00.000Z', 'plan-created', 'ctx', 'pay', 'event {id}', '{{}}');"
            )
        })
        .collect();
    database_at(home, 2, &format!("{VERSION_1_PLAN}\n{events}"));
}

const NEW_KINDS: [&str; 5] = [
    "context-renamed",
    "plan-renamed",
    "context-archived",
    "plan-archived",
    "session-archived",
];

#[test]
fn a_version_2_database_migrates_through_3_keeping_every_row_and_event_id() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_2_with_events(&home);

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    let (version, events, archived, triggers): (i64, Vec<(i64, String)>, i64, i64) = store
        .read(|c| {
            let mut stmt = c.prepare("SELECT id, text FROM events ORDER BY id")?;
            let events = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>>>()?;
            Ok((
                c.pragma_query_value(None, "user_version", |r| r.get(0))?,
                events,
                c.query_row(
                    "SELECT (SELECT count(*) FROM contexts WHERE id = 'ctx' AND archived_at IS NULL)
                          + (SELECT count(*) FROM plans WHERE id = 'pay' AND archived_at IS NULL)
                          + (SELECT count(*) FROM sessions WHERE id = 'l-pay' AND archived_at IS NULL)",
                    [],
                    |r| r.get(0),
                )?,
                c.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = 'events'",
                    [],
                    |r| r.get(0),
                )?,
            ))
        })
        .unwrap();
    assert_eq!(version, VERSION);
    assert_eq!(
        events,
        [
            (1, "event 1".to_owned()),
            (2, "event 2".to_owned()),
            (5, "event 5".to_owned())
        ]
    );
    assert_eq!(archived, 3, "every row kept, every archived_at null");
    assert_eq!(
        triggers, 3,
        "events_own, events_no_update, events_no_delete"
    );

    let id = store
        .write("test", |tx| {
            tx.execute(
                "INSERT INTO events (txn, at, kind, context, text, data)
                 VALUES (cpd_txn(), '2026-01-02T00:00:00.000Z', 'context-archived', 'ctx', 't', '{\"archived\":true}')",
                [],
            )?;
            Ok::<i64, CpdError>(tx.last_insert_rowid())
        })
        .unwrap();
    assert!(id > 5, "the next Event id {id} is above the old maximum");
}

#[test]
fn after_migration_3_the_sequence_never_goes_back_and_the_history_lists_archived_at() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_2_with_events(&home);
    // An old sequence above the highest id (rows that no longer exist) is kept.
    Connection::open(home.db())
        .unwrap()
        .execute(
            "UPDATE sqlite_sequence SET seq = 40 WHERE name = 'events'",
            [],
        )
        .unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let (seq, after): (i64, String) = store
        .write("test", |tx| {
            tx.execute(
                "UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'pay'",
                [],
            )?;
            Ok::<_, CpdError>((
                tx.query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name = 'events'",
                    [],
                    |r| r.get(0),
                )?,
                tx.query_row(
                    "SELECT after FROM history WHERE tbl = 'plans' ORDER BY id DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )?,
            ))
        })
        .unwrap();
    assert_eq!(seq, 40);
    let after: Value = serde_json::from_str(&after).unwrap();
    assert_eq!(after["archived_at"], "2026-02-01T00:00:00.000Z");
}

#[test]
fn events_allow_the_five_new_kinds_and_still_refuse_an_unknown_one() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_2_with_events(&home);
    let mut store = Store::open(&home, 5000).unwrap();
    let insert = |tx: &Connection, kind: &str| {
        tx.execute(
            "INSERT INTO events (txn, at, kind, context, text, data)
             VALUES (cpd_txn(), '2026-01-02T00:00:00.000Z', ?1, 'ctx', 't', '{}')",
            [kind],
        )
    };
    for kind in NEW_KINDS {
        store
            .write("test", |tx| Ok::<_, CpdError>(insert(tx, kind)?))
            .unwrap_or_else(|e| panic!("{kind}: {e:?}"));
    }
    let err = store
        .write("test", |tx| Ok::<_, CpdError>(insert(tx, "plan-exploded")?))
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("FOREIGN KEY"),
        "an Event kind is a foreign key to event_kinds: {err:?}"
    );
}

// ---------- migration 4 ----------

#[test]
fn migrated_pr_keys_follow_4_5() {
    for (repo, key) in [
        ("/src/pay", "pay"),
        ("/src/My Repo.git", "my-repo-git"),
        ("/src/2fa", "pr-2fa"),
        ("/src/-x", "pr--x"),
        ("/src/Ünïcode", "pr--n-code"),
        ("/src/web/", "web"),
        (
            "/src/a-very-long-repository-name-indeed",
            "a-very-long-repository-n",
        ),
        (
            "/src/123456789012345678901234567",
            "pr-123456789012345678901",
        ),
        ("/", "pr-"),
    ] {
        assert_eq!(migrated_workflow_key(repo).as_str(), key, "{repo}");
    }
}

const T: &str = "2026-01-01T00:00:00.000Z";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// A version 2 database with one Plan in each phase, Jobs, Dependencies,
/// Attempts, an Escalation, a Plan review, Events and Context records.
fn version_2(home: &CpdHome) {
    database_at(home, 2, &version_2_sql());
}

/// The rows of [`version_2`].
fn version_2_sql() -> String {
    format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
           ('l-gr', 'lead', 'ctx', 'gr', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
           ('l-pl', 'lead', 'ctx', 'pl', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
           ('l-di', 'lead', 'ctx', 'di', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
           ('l-pr', 'lead', 'ctx', 'pr', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
           ('l-dn', 'lead', 'ctx', 'dn', 'lead', '/src', 'O3', 'idle', '{T}', '{T}');
         INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, created_at) VALUES
           ('gr', 'ctx', 'Grounding', '/src/pay', 'main', 'cpd/gr', NULL, 'grounding', 'l-gr', '2026-01-01T00:00:00.001Z'),
           ('pl', 'ctx', 'Planning', '/src/My Repo.git', 'main', 'cpd/pl', NULL, 'plan', 'l-pl', '2026-01-01T00:00:00.002Z');
         INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, created_at, locked_at) VALUES
           ('di', 'ctx', 'Dispatch', '/src/2fa', 'main', 'cpd/di', '/src/2fa.di', 'dispatch', 'l-di', '2026-01-01T00:00:00.003Z', '2026-01-02T00:00:00.003Z');
         INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session,
                            pr_number, pr_url, pr_opened_at, pr_seen, pr_polled_at, pr_title, pr_review_decision, pr_checks,
                            all_merged_at, created_at, locked_at) VALUES
           ('pr', 'ctx', 'Open', '/src/a-very-long-repository-name-indeed', 'main', 'cpd/pr', '/src/long.pr', 'pr', 'l-pr',
            7, 'https://github.com/acme/pay/pull/7', '2026-01-03T00:00:00.004Z', '{{\"checks\":{{\"ci\":\"SUCCESS\"}},\"reviewIds\":[\"r1\"],\"commentIds\":[]}}',
            '2026-01-04T00:00:00.004Z', 'Open Workflow', 'approved', 'passed', '2026-01-03T00:00:00.004Z', '2026-01-01T00:00:00.004Z', '2026-01-02T00:00:00.004Z');
         INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session,
                            pr_number, pr_url, pr_opened_at, created_at, locked_at, done_at) VALUES
           ('dn', 'ctx', 'Done', '/src/web/', 'develop', 'cpd/dn', '/src/web.dn', 'done', 'l-dn',
            9, 'https://github.com/acme/long/pull/9', '2026-01-03T00:00:00.005Z', '2026-01-01T00:00:00.005Z', '2026-01-02T00:00:00.005Z', '2026-01-05T00:00:00.005Z');
         INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at)
           VALUES ('pl', 'r1', 1, '{FP}', 'approved', '{T}', '{T}');
         INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, created_at, merged_sha) VALUES
           ('pl', 'a', 'A', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, NULL, 1, '{T}', NULL),
           ('pl', 'b', 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, NULL, 1, '{T}', NULL),
           ('di', 'x', 'X', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, '{FP}', 1, '{T}', NULL),
           ('di', 'y', 'Y', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{FP}', 1, '{T}', NULL),
           ('pr', 'z', 'Z', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, '{FP}', 1, '{T}', 'abc'),
           ('dn', 'w', 'W', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, '{FP}', 1, '{T}', 'def');
         INSERT INTO dependencies (plan, job, needs) VALUES ('pl', 'b', 'a'), ('di', 'x', 'y');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree,
                               merged_sha, started_at, ended_at) VALUES
           ('di', 'y-a1', 'y', 1, 'S2', 'ladder', 'running', NULL, NULL, 'cpd/di--y-a1', '/src/2fa.di--y-a1', NULL, '{T}', NULL),
           ('pr', 'z-a1', 'z', 1, 'S2', 'ladder', 'ended', 'merged', 'merged', 'cpd/pr--z-a1', '/w', 'abc', '{T}', '{T}');
         INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
           VALUES ('di', 'y-a1-q1', 'y-a1', 1, 'Why?', 'open', 1, '{T}');
         INSERT INTO events (txn, at, kind, context, plan, job, text, data) VALUES
           (1, '{T}', 'plan-created', 'ctx', 'gr', NULL, 'x', '{{}}'),
           (1, '{T}', 'job-merged', 'ctx', 'pr', 'z', 'x', '{{}}'),
           (1, '{T}', 'all-merged', 'ctx', 'pr', NULL, 'x', '{{}}');
         INSERT INTO context_records (context, id, kind, at, machine, text, plan_id, plan_machine, plan_repo, plan_jira) VALUES
           ('ctx', 'm1-p1', 'plan', '{T}', 'm1', 'Grounding', 'gr', 'm1', '/src/pay', 'PAY-1');
         INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('ctx', 'm1-n1', 'note', '{T}', 'm1', 'A note');"
    )
}

/// `version_2`, migrated by `Store::open`.
fn migrated(dir: &tempfile::TempDir) -> Store {
    let home = home(dir);
    version_2(&home);
    let store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    store
}

fn rows(store: &mut Store, sql: &str) -> Vec<Value> {
    store
        .read(|c| {
            let mut stmt = c.prepare(sql)?;
            let n = stmt.column_count();
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
                        .collect::<Result<Vec<Value>>>()
                        .map(Value::from)
                })?
                .collect::<Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap()
}

fn count(store: &mut Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn a_version_2_database_migrates_through_3_4_and_5_to_6_with_one_pr_per_plan() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated(&dir);
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    let migrate = count(
        &mut store,
        "SELECT max(id) FROM txns WHERE command = 'migrate'",
    );
    assert_eq!(migrate, 2);

    let workflows = rows(
        &mut store,
        "SELECT plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                pull_request, all_merged_at, start_failed_at, created_txn, created_at, started_at, merged_at
         FROM workflows ORDER BY created_at",
    );
    let n = Value::Null;
    assert_eq!(
        workflows,
        vec![
            serde_json::json!([
                "gr",
                "pay",
                "/src/pay",
                "main",
                "waiting",
                "cpd/gr",
                n,
                n,
                n,
                n,
                n,
                n,
                n,
                migrate,
                "2026-01-01T00:00:00.001Z",
                n,
                n
            ]),
            serde_json::json!([
                "pl",
                "my-repo-git",
                "/src/My Repo.git",
                "main",
                "waiting",
                "cpd/pl",
                n,
                n,
                n,
                n,
                n,
                n,
                n,
                migrate,
                "2026-01-01T00:00:00.002Z",
                n,
                n
            ]),
            serde_json::json!([
                "di",
                "pr-2fa",
                "/src/2fa",
                "main",
                "dispatch",
                "cpd/di",
                "/src/2fa.di",
                n,
                n,
                n,
                n,
                n,
                n,
                migrate,
                "2026-01-01T00:00:00.003Z",
                "2026-01-02T00:00:00.003Z",
                n
            ]),
            serde_json::json!([
                "pr",
                "a-very-long-repository-n",
                "/src/a-very-long-repository-name-indeed",
                "main",
                "open",
                "cpd/pr",
                "/src/long.pr",
                7,
                "https://github.com/acme/pay/pull/7",
                "2026-01-03T00:00:00.004Z",
                2,
                "2026-01-03T00:00:00.004Z",
                n,
                migrate,
                "2026-01-01T00:00:00.004Z",
                "2026-01-02T00:00:00.004Z",
                n
            ]),
            serde_json::json!([
                "dn",
                "web",
                "/src/web/",
                "develop",
                "merged",
                "cpd/dn",
                "/src/web.dn",
                9,
                "https://github.com/acme/long/pull/9",
                "2026-01-03T00:00:00.005Z",
                1,
                n,
                n,
                migrate,
                "2026-01-01T00:00:00.005Z",
                "2026-01-02T00:00:00.005Z",
                "2026-01-05T00:00:00.005Z"
            ]),
        ]
    );
    // One pull request row for each Workflow with a number, in Plan and key order (migration 6).
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, repo, number, title, draft, state, review_decision, checks, created_at, merged_at, changed_at FROM pull_requests ORDER BY id"
        ),
        vec![
            serde_json::json!([
                1,
                "acme/long",
                9,
                "Done",
                0,
                "merged",
                "none",
                "none",
                "2026-01-03T00:00:00.005Z",
                "2026-01-05T00:00:00.005Z",
                "2026-01-03T00:00:00.005Z"
            ]),
            serde_json::json!([
                2,
                "acme/pay",
                7,
                "Open Workflow",
                0,
                "open",
                "approved",
                "passed",
                "2026-01-03T00:00:00.004Z",
                n,
                "2026-01-03T00:00:00.004Z"
            ]),
        ]
    );
    assert_eq!(count(&mut store, "SELECT count(*) FROM workflow_needs"), 0);

    // Every Job has its Plan's one Workflow, is no review Job, and keeps its seq.
    assert_eq!(
        rows(
            &mut store,
            "SELECT seq, plan, workflow, review, key, status FROM jobs ORDER BY seq"
        ),
        vec![
            serde_json::json!([1, "pl", "my-repo-git", 0, "a", "draft"]),
            serde_json::json!([2, "pl", "my-repo-git", 0, "b", "draft"]),
            serde_json::json!([3, "di", "pr-2fa", 0, "x", "queued"]),
            serde_json::json!([4, "di", "pr-2fa", 0, "y", "running"]),
            serde_json::json!([5, "pr", "a-very-long-repository-n", 0, "z", "merged"]),
            serde_json::json!([6, "dn", "web", 0, "w", "merged"]),
        ]
    );

    // Every other row and id is kept.
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, phase, locked_at, done_at FROM plans ORDER BY created_at"
        ),
        vec![
            serde_json::json!(["gr", "grounding", n, n]),
            serde_json::json!(["pl", "plan", n, n]),
            serde_json::json!(["di", "dispatch", "2026-01-02T00:00:00.003Z", n]),
            serde_json::json!(["pr", "deploy", "2026-01-02T00:00:00.004Z", n]),
            serde_json::json!([
                "dn",
                "done",
                "2026-01-02T00:00:00.005Z",
                "2026-01-05T00:00:00.005Z"
            ]),
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, kind, plan, job FROM events ORDER BY id"
        ),
        vec![
            serde_json::json!([1, "plan-created", "gr", n]),
            serde_json::json!([2, "job-merged", "pr", "z"]),
            serde_json::json!([3, "all-merged", "pr", n]),
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT seq, id, plan_id, plan_machine, plan_jira FROM context_records ORDER BY seq"
        ),
        vec![
            serde_json::json!([1, "m1-p1", "gr", "m1", "PAY-1"]),
            serde_json::json!([2, "m1-n1", n, n, n]),
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT plan, job, needs FROM dependencies ORDER BY plan"
        ),
        vec![
            serde_json::json!(["di", "x", "y"]),
            serde_json::json!(["pl", "b", "a"]),
        ]
    );
    assert_eq!(count(&mut store, "SELECT count(*) FROM attempts"), 2);
    assert_eq!(count(&mut store, "SELECT count(*) FROM escalations"), 1);
    assert_eq!(count(&mut store, "SELECT count(*) FROM sessions"), 5);
    assert_eq!(count(&mut store, "SELECT count(*) FROM plan_reviews"), 1);
    assert_eq!(
        count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"),
        0
    );

    // The moved and dropped columns are gone.
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM pragma_table_info('plans')"
        ),
        17
    ); // `plan-outline` added `outline`, `delete-tombstone` `deleted_at`, `codereview-followup` `pr_subject` and `changes_requested_at`, `plan-reviewed-pr` `reviewed_repo`, `reviewed_number` and `reviewed_title`
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM pragma_table_info('context_records') WHERE name = 'plan_repo'"
        ),
        0
    );

    // History: one insert per migrated Workflow (migration 4) and one per back-filled pull request
    // (migration 6) in the migration's txn; the copies write none.
    let mut expected: Vec<Value> = ["di", "dn", "gr", "pl", "pr"]
        .iter()
        .map(|plan| {
            let key = workflows.iter().find(|p| p[0] == *plan).unwrap()[1].clone();
            serde_json::json!([
                2,
                "prs",
                serde_json::json!([plan, key]).to_string(),
                "insert"
            ])
        })
        .collect();
    expected.push(serde_json::json!([2, "pull_requests", "[1]", "insert"]));
    expected.push(serde_json::json!([2, "pull_requests", "[2]", "insert"]));
    assert_eq!(
        rows(
            &mut store,
            "SELECT txn, tbl, pk, op FROM history WHERE txn = 2 ORDER BY id"
        ),
        expected
    );

    // AUTOINCREMENT goes on after the kept ids.
    let (job, event, record) = store
        .write("test", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status,
                                   appended, created_txn, created_at)
                   VALUES ('gr', 'pay', 'n', 'N', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), '{T}');
                 INSERT INTO events (txn, at, kind, context, plan, text, data) VALUES (cpd_txn(), '{T}', 'pr-added', 'ctx', 'gr', 'x', '{{}}');
                 INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('ctx', 'm1-n2', 'note', '{T}', 'm1', 'x');"
            ))?;
            Ok(tx.query_row(
                "SELECT (SELECT max(seq) FROM jobs), (SELECT max(id) FROM events), (SELECT max(seq) FROM context_records)",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!((job, event, record), (7, 4, 3));
}

/// A version 3 database as tree-menu's migration 3 leaves it (the User's
/// real case): archived rows and the Events of the archive kinds.
fn version_3(home: &CpdHome) {
    let sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on, archived_at) VALUES ('ctx', 'Ctx', '{T}', 'm1', '2026-01-06T00:00:00.000Z');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, archived_at) VALUES
           ('l-ar', 'lead', 'ctx', 'ar', 'lead', '/src', 'O3', 'stopped', '{T}', '{T}', '2026-01-06T00:00:00.001Z'),
           ('l-li', 'lead', 'ctx', 'li', 'lead', '/src', 'O3', 'idle', '{T}', '{T}', NULL);
         INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, created_at, archived_at) VALUES
           ('ar', 'ctx', 'Archived', '/src/pay', 'main', 'cpd/ar', NULL, 'plan', 'l-ar', '2026-01-01T00:00:00.001Z', '2026-01-06T00:00:00.002Z'),
           ('li', 'ctx', 'Live', '/src/web', 'main', 'cpd/li', NULL, 'grounding', 'l-li', '2026-01-01T00:00:00.002Z', NULL);
         INSERT INTO events (txn, at, kind, context, plan, session, text, data) VALUES
           (1, '{T}', 'plan-renamed', 'ctx', 'ar', NULL, 'x', '{{}}'),
           (1, '{T}', 'plan-archived', 'ctx', 'ar', NULL, 'x', '{{}}'),
           (1, '{T}', 'session-archived', 'ctx', NULL, 'l-ar', 'x', '{{}}'),
           (1, '{T}', 'context-archived', 'ctx', NULL, NULL, 'x', '{{}}');"
    );
    database_at(home, 3, &sql);
}

#[test]
fn a_tree_menu_version_3_database_migrates_to_6_keeping_archived_at() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_3(&home);
    // The starting schema is exactly the one tree-menu's migration 3 installs.
    assert_eq!(
        legacy_schema_hash(&Connection::open(home.db()).unwrap()).unwrap(),
        SCHEMA_HASHES[2]
    );

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    let n = Value::Null;
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, phase, archived_at FROM plans ORDER BY id"
        ),
        vec![
            serde_json::json!(["ar", "plan", "2026-01-06T00:00:00.002Z"]),
            serde_json::json!(["li", "grounding", n]),
        ]
    );
    assert_eq!(
        rows(&mut store, "SELECT id, archived_at FROM contexts"),
        vec![serde_json::json!(["ctx", "2026-01-06T00:00:00.000Z"])]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT plan, key, repo, status, lead_branch FROM workflows ORDER BY plan"
        ),
        vec![
            serde_json::json!(["ar", "pay", "/src/pay", "waiting", "cpd/ar"]),
            serde_json::json!(["li", "web", "/src/web", "waiting", "cpd/li"]),
        ]
    );
    assert_eq!(
        rows(&mut store, "SELECT id, kind FROM events ORDER BY id"),
        vec![
            serde_json::json!([1, "plan-renamed"]),
            serde_json::json!([2, "plan-archived"]),
            serde_json::json!([3, "session-archived"]),
            serde_json::json!([4, "context-archived"]),
        ]
    );
    // Both sides' Event kinds are accepted, and a Plan's history lists archived_at.
    write(&mut store, "INSERT INTO events (txn, at, kind, context, text, data) VALUES
                         (cpd_txn(), 't', 'plan-archived', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'pr-started', 'ctx', 'x', '{}');
                       UPDATE plans SET archived_at = NULL WHERE id = 'ar'")
        .unwrap();
    let after = rows(
        &mut store,
        "SELECT after FROM history WHERE tbl = 'plans' ORDER BY id DESC LIMIT 1",
    );
    let plan: Value = serde_json::from_str(after[0][0].as_str().unwrap()).unwrap();
    assert!(
        plan.as_object().unwrap().contains_key("archived_at") && plan["archived_at"].is_null(),
        "{plan}"
    );
    assert_eq!(count(&mut store, "SELECT max(id) FROM events"), 6);
}

// ---------- migration 5 ----------

/// A version 4 database, as the User's real one: Plans in `dispatch`, `pr`
/// and `done`, each Workflow on its legacy Lead Branch `cpd/<plan>`; `dn` has its
/// Workflow merged, `pr` an open one.
fn version_4(home: &CpdHome) {
    let sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
           ('l-di', 'lead', 'ctx', 'di', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
           ('l-pr', 'lead', 'ctx', 'pr', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
           ('l-dn', 'lead', 'ctx', 'dn', 'lead', '/src', 'O3', 'idle', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES
           ('di', 'ctx', 'Dispatch', 'dispatch', 'l-di', '{T}', '{T}'),
           ('pr', 'ctx', 'Open', 'pr', 'l-pr', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at) VALUES
           ('dn', 'ctx', 'Done', 'done', 'l-dn', '{T}', '{T}', '2026-01-05T00:00:00.000Z');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          created_txn, created_at, started_at, merged_at) VALUES
           ('di', 'pay', '/src/pay', 'main', 'dispatch', 'cpd/di', '/src/pay.di', NULL, NULL, NULL, 1, '{T}', '{T}', NULL),
           ('pr', 'pay', '/src/pay', 'main', 'open', 'cpd/pr', '/src/pay.pr', 7, 'https://github.com/acme/pay/pull/7', '{T}', 1, '{T}', '{T}', NULL),
           ('dn', 'web', '/src/web', 'develop', 'merged', 'cpd/dn', '/src/web.dn', 9, 'https://github.com/acme/web/pull/9', '{T}', 1, '{T}', '{T}',
            '2026-01-05T00:00:00.000Z');
         INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, created_at, merged_sha) VALUES
           ('dn', 'web', 'w', 'W', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, '{FP}', 1, '{T}', 'def');"
    );
    database_at(home, 4, &sql);
}

/// `version_4`, migrated by `Store::open`.
fn migrated_4(dir: &tempfile::TempDir) -> Store {
    let home = home(dir);
    version_4(&home);
    assert_eq!(
        legacy_schema_hash(&Connection::open(home.db()).unwrap()).unwrap(),
        SCHEMA_HASHES[3]
    );
    let store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    store
}

#[test]
fn a_version_4_database_with_a_done_plan_migrates_to_5_and_6_keeping_every_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated_4(&dir);
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, phase, done_at FROM plans ORDER BY id"
        ),
        vec![
            serde_json::json!(["di", "dispatch", Value::Null]),
            serde_json::json!(["dn", "done", "2026-01-05T00:00:00.000Z"]),
            serde_json::json!(["pr", "deploy", Value::Null]),
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT plan, key, status, lead_branch FROM workflows ORDER BY plan"
        ),
        vec![
            serde_json::json!(["di", "pay", "dispatch", "cpd/di"]),
            serde_json::json!(["dn", "web", "merged", "cpd/dn"]),
            serde_json::json!(["pr", "pay", "open", "cpd/pr"]),
        ]
    );
    assert_eq!(count(&mut store, "SELECT count(*) FROM jobs"), 1);
    assert_eq!(count(&mut store, "SELECT count(*) FROM sessions"), 3);
}

#[test]
fn plans_phase_allows_deploy_and_done_back_to_dispatch_and_refuses_the_other_backward_moves() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated_4(&dir);
    for sql in [
        "UPDATE plans SET phase = 'grounding', locked_at = NULL WHERE id = 'di'",
        "UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'di'",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::IllegalTransition, "{sql}: {err:?}");
    }
    // `done_at` must be cleared by the same update (the CHECK of §4.4).
    assert!(
        write(
            &mut store,
            "UPDATE plans SET phase = 'dispatch' WHERE id = 'dn'"
        )
        .is_err()
    );
    write(
        &mut store,
        "UPDATE plans SET phase = 'dispatch', done_at = NULL WHERE id = 'dn';
                       UPDATE plans SET phase = 'dispatch', done_at = NULL WHERE id = 'pr'",
    )
    .unwrap();
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, phase, done_at FROM plans ORDER BY id"
        ),
        vec![
            serde_json::json!(["di", "dispatch", Value::Null]),
            serde_json::json!(["dn", "dispatch", Value::Null]),
            serde_json::json!(["pr", "dispatch", Value::Null]),
        ]
    );
}

fn plan_phase(store: &mut Store, id: &str) -> String {
    rows(store, &format!("SELECT phase FROM plans WHERE id = '{id}'"))[0][0]
        .as_str()
        .unwrap()
        .to_owned()
}

// pro-d433: `dispatch`, `deploy` and `done` may go back to `plan` and keep `locked_at` (pro-d429).
#[test]
fn plans_phase_allows_dispatch_deploy_and_done_back_to_plan() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated_4(&dir);
    write(
        &mut store,
        "UPDATE plans SET phase = 'plan' WHERE id = 'di'",
    )
    .expect("dispatch>plan");
    write(
        &mut store,
        "UPDATE plans SET phase = 'plan' WHERE id = 'pr'",
    )
    .expect("deploy>plan");
    write(
        &mut store,
        "UPDATE plans SET phase = 'plan', done_at = NULL WHERE id = 'dn'",
    )
    .expect("done>plan");
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, phase, locked_at IS NOT NULL, done_at FROM plans ORDER BY id"
        ),
        vec![
            serde_json::json!(["di", "plan", 1, Value::Null]),
            serde_json::json!(["dn", "plan", 1, Value::Null]),
            serde_json::json!(["pr", "plan", 1, Value::Null]),
        ]
    );
    // `plan>dispatch` still passes with `locked_at` set, and a new Plan still goes `grounding>plan`.
    write(
        &mut store,
        "UPDATE plans SET phase = 'dispatch' WHERE id = 'di'",
    )
    .expect("plan>dispatch");
    write(
        &mut store,
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('gr', 'ctx', 'G', 'grounding', 'l-di', 't');
         UPDATE plans SET phase = 'plan' WHERE id = 'gr'",
    )
    .expect("grounding>plan");
}

#[test]
fn plans_phase_still_refuses_every_other_backward_move() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated_4(&dir);
    // The new rule first: a Plan in `plan` with `locked_at` set is the starting point below.
    write(
        &mut store,
        "UPDATE plans SET phase = 'plan' WHERE id = 'di'",
    )
    .expect("dispatch>plan");
    for sql in [
        "UPDATE plans SET phase = 'grounding' WHERE id = 'di'",
        "UPDATE plans SET phase = 'grounding', locked_at = NULL WHERE id = 'di'",
        "UPDATE plans SET phase = 'grounding', locked_at = NULL WHERE id = 'pr'",
        "UPDATE plans SET phase = 'grounding', locked_at = NULL, done_at = NULL WHERE id = 'dn'",
        "UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'di'",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::IllegalTransition, "{sql}: {err:?}");
    }
    write(
        &mut store,
        "UPDATE plans SET phase = 'dispatch' WHERE id = 'di'",
    )
    .unwrap();
    let err = refused(
        &mut store,
        "UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'di'",
    );
    assert_eq!(err.code(), ErrorCode::IllegalTransition, "{err:?}");
    assert_eq!(plan_phase(&mut store, "di"), "dispatch");
}

#[test]
fn plans_check_allows_plan_with_locked_at_set() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated_4(&dir);
    write(
        &mut store,
        "UPDATE plans SET phase = 'plan' WHERE id = 'di'",
    )
    .expect("plan with locked_at set");
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM plans WHERE id = 'di' AND phase = 'plan' AND locked_at IS NOT NULL"
        ),
        1
    );
    // `plan` with `locked_at` NULL stays valid; `grounding` with it set does not.
    write(
        &mut store,
        "UPDATE plans SET locked_at = NULL WHERE id = 'di'",
    )
    .expect("plan with locked_at NULL");
    refused(
        &mut store,
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('g2', 'ctx', 'G', 'grounding', 'l-di', 't', 't')",
    );
    // `dispatch`, `deploy` and `done` require `locked_at`.
    for sql in [
        "UPDATE plans SET locked_at = NULL WHERE id = 'pr'",
        "UPDATE plans SET locked_at = NULL WHERE id = 'dn'",
        "UPDATE plans SET phase = 'dispatch' WHERE id = 'di'",
    ] {
        refused(&mut store, sql);
    }
    write(
        &mut store,
        "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'di'",
    )
    .expect("plan>dispatch with locked_at");
    refused(
        &mut store,
        "UPDATE plans SET locked_at = NULL WHERE id = 'di'",
    );
}

#[test]
fn plans_check_done_to_plan_requires_done_at_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated_4(&dir);
    // The trigger lets `done>plan` through; the CHECK (`done_at` only in `done`) refuses the row.
    let err = refused(
        &mut store,
        "UPDATE plans SET phase = 'plan' WHERE id = 'dn'",
    );
    assert_ne!(err.code(), ErrorCode::IllegalTransition, "{err:?}");
    assert_eq!(plan_phase(&mut store, "dn"), "done");
    write(
        &mut store,
        "UPDATE plans SET phase = 'plan', done_at = NULL WHERE id = 'dn'",
    )
    .expect("done>plan with done_at NULL");
    assert_eq!(plan_phase(&mut store, "dn"), "plan");
}

// pro-d433: `workflows_locked`, `workflow_needs_locked` and `workflow_needs_no_delete` key on `locked_at`, not on phase `plan`.
#[test]
fn workflow_lock_triggers_follow_locked_at_in_phase_plan() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated_4(&dir);
    write(
        &mut store,
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('di', 'web', '/src/web', 'main', 'waiting', 'cpd/di/web', cpd_txn(), 't');
         INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('di', 'web', 'pay');",
    )
    .unwrap();
    write(
        &mut store,
        "UPDATE plans SET phase = 'plan' WHERE id = 'di'",
    )
    .expect("dispatch>plan");

    // locked_at set: the after-the-lock rules hold.
    for sql in [
        "UPDATE workflows SET repo = '/src/other' WHERE plan = 'di' AND key = 'pay'",
        "UPDATE workflows SET lead_branch = 'cpd/di/x' WHERE plan = 'di' AND key = 'pay'",
        "UPDATE workflows SET key = 'other' WHERE plan = 'di' AND key = 'pay'",
        "INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('di', 'pay', 'web')",
        "DELETE FROM workflow_needs WHERE plan = 'di' AND workflow = 'web'",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{sql}: {err:?}");
    }

    // locked_at NULL: the pre-lock edits are allowed.
    write(
        &mut store,
        "UPDATE plans SET locked_at = NULL WHERE id = 'di'",
    )
    .unwrap();
    write(
        &mut store,
        "UPDATE workflows SET repo = '/src/other' WHERE plan = 'di' AND key = 'pay';
         DELETE FROM workflow_needs WHERE plan = 'di' AND workflow = 'web';
         INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('di', 'web', 'pay')",
    )
    .expect("pre-lock edits with locked_at NULL");
}

#[test]
fn prs_locked_refuses_every_lead_branch_rename_even_of_a_merged_pr() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated_4(&dir);
    for sql in [
        // another value
        "UPDATE workflows SET lead_branch = 'cpd/dn/other' WHERE plan = 'dn'",
        "UPDATE workflows SET lead_branch = 'cpd/dn/web/x' WHERE plan = 'dn'",
        "UPDATE workflows SET lead_branch = 'cpd/pr/pay' WHERE plan = 'dn'",
        // the rename together with another guarded column
        "UPDATE workflows SET lead_branch = 'cpd/dn/web', repo = '/src/other' WHERE plan = 'dn'",
        "UPDATE workflows SET lead_branch = 'cpd/dn/web', base = 'main' WHERE plan = 'dn'",
        // the other guarded columns alone
        "UPDATE workflows SET repo = '/src/other' WHERE plan = 'dn'",
        "UPDATE workflows SET base = 'main' WHERE plan = 'dn'",
        // a Workflow that is not merged
        "UPDATE workflows SET lead_branch = 'cpd/pr/pay' WHERE plan = 'pr'",
        "UPDATE workflows SET lead_branch = 'cpd/di/pay' WHERE plan = 'di'",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{sql}: {err:?}");
        assert_eq!(err.exit().code(), 3);
    }
    // `lead-branch-name` (pro-d113 A3) removed the exception: the rename itself is refused too.
    let err = refused(
        &mut store,
        "UPDATE workflows SET lead_branch = 'cpd/dn/web' WHERE plan = 'dn'",
    );
    assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{err:?}");
}

// ---------- migration 6 ----------

/// A version 5 database as plan-reopen's migration 5 leaves it: `op`'s Workflow is
/// open on its legacy Lead Branch with a polled title, `dn`'s merged, `wa`'s
/// waiting in a Plan in `dispatch`.
fn version_5(home: &CpdHome) {
    let sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
           ('l-op', 'lead', 'ctx', 'op', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
           ('l-dn', 'lead', 'ctx', 'dn', 'lead', '/src', 'O3', 'idle', '{T}', '{T}'),
           ('l-wa', 'lead', 'ctx', 'wa', 'lead', '/src', 'O3', 'idle', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES
           ('op', 'ctx', 'Open', 'pr', 'l-op', '{T}', '{T}'),
           ('wa', 'ctx', 'Waiting', 'dispatch', 'l-wa', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at) VALUES
           ('dn', 'ctx', 'Done', 'done', 'l-dn', '{T}', '{T}', '2026-01-05T00:00:00.000Z');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pr_polled_at, pr_title, pr_review_decision, pr_checks, created_txn, created_at, started_at, merged_at) VALUES
           ('op', 'pay', '/src/pay', 'main', 'open', 'cpd/op', '/src/pay.op', 7, 'https://github.com/acme/pay/pull/7',
            '2026-01-03T00:00:00.000Z', '2026-01-04T00:00:00.000Z', 'Polled title', 'approved', 'failed', 1, '{T}', '{T}', NULL),
           ('dn', 'web', '/src/web', 'develop', 'merged', 'cpd/dn', '/src/web.dn', 9, 'https://github.com/acme/web/pull/9',
            '2026-01-02T00:00:00.000Z', NULL, NULL, NULL, NULL, 1, '{T}', '{T}', '2026-01-05T00:00:00.000Z'),
           ('wa', 'web', '/src/web', 'main', 'waiting', 'cpd/wa/web', NULL, NULL, NULL, NULL,
            NULL, NULL, NULL, NULL, 1, '{T}', NULL, NULL);"
    );
    database_at(home, 5, &sql);
}

/// The triggers on `table`, `(name, sql)` by name.
fn triggers_on(conn: &Connection, table: &str) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT name, sql FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = ?1 ORDER BY name")
        .unwrap();
    stmt.query_map([table], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_>>()
        .unwrap()
}

/// `version_5`, migrated by `Store::open`, and the triggers on `prs` before.
fn migrated_5(dir: &tempfile::TempDir) -> (Store, Vec<(String, String)>) {
    let home = home(dir);
    version_5(&home);
    let conn = Connection::open(home.db()).unwrap();
    assert_eq!(legacy_schema_hash(&conn).unwrap(), SCHEMA_HASHES[4]);
    let before = triggers_on(&conn, "prs");
    drop(conn);
    let store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    (store, before)
}

#[test]
fn a_version_5_database_migrates_to_6_with_the_pull_requests_back_filled() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, _) = migrated_5(&dir);
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    let n = Value::Null;
    assert_eq!(
        rows(&mut store, "SELECT id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at
                          FROM pull_requests ORDER BY id"),
        vec![
            serde_json::json!([1, "acme/web", 9, "https://github.com/acme/web/pull/9", "Done", 0, "merged", "none", "none",
                               "2026-01-02T00:00:00.000Z", "2026-01-05T00:00:00.000Z", "2026-01-02T00:00:00.000Z"]),
            serde_json::json!([2, "acme/pay", 7, "https://github.com/acme/pay/pull/7", "Polled title", 0, "open", "approved", "failed",
                               "2026-01-03T00:00:00.000Z", n, "2026-01-03T00:00:00.000Z"]),
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT plan, key, status, lead_branch, pr_number, pull_request FROM workflows ORDER BY plan"
        ),
        vec![
            serde_json::json!(["dn", "web", "merged", "cpd/dn", 9, 1]),
            serde_json::json!(["op", "pay", "open", "cpd/op", 7, 2]),
            serde_json::json!(["wa", "web", "waiting", "cpd/wa/web", n, n]),
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT tbl, pk, op FROM history WHERE txn = 2 ORDER BY id"
        ),
        vec![
            serde_json::json!(["pull_requests", "[1]", "insert"]),
            serde_json::json!(["pull_requests", "[2]", "insert"])
        ]
    );
}

// ---------- migration 11: the pull request poll's mergeable state ----------

/// A version 10 database with two pull requests and no Workflow.
fn version_10(home: &CpdHome) {
    database_at(
        home,
        10,
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at) VALUES
           ('acme/pay', 7, 'https://github.com/acme/pay/pull/7', 'Pay', 0, 'open', 'none', 'failed', '2026-01-03T00:00:00.000Z', '2026-01-04T00:00:00.000Z'),
           ('acme/web', 9, 'https://github.com/acme/web/pull/9', 'Web', 1, 'open', 'approved', 'passed', '2026-01-03T00:00:00.000Z', '2026-01-04T00:00:00.000Z');",
    );
}

#[test]
fn a_version_10_database_migrates_to_11_with_its_rows_unknown_and_its_history_listing_both_columns()
{
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_10(&home);
    assert_eq!(
        legacy_schema_hash(&Connection::open(home.db()).unwrap()).unwrap(),
        SCHEMA_HASHES[9]
    );
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    assert_eq!(
        rows(
            &mut store,
            "SELECT number, checks, mergeable, merge_state FROM pull_requests ORDER BY id"
        ),
        vec![
            serde_json::json!([7, "failed", "unknown", "unknown"]),
            serde_json::json!([9, "passed", "unknown", "unknown"])
        ]
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM history WHERE tbl = 'pull_requests'"
        ),
        2,
        "the migration wrote no history row"
    );

    // An update, an insert and a delete: each history row lists both new columns.
    write(&mut store, "UPDATE pull_requests SET mergeable = 'conflicting', merge_state = 'dirty' WHERE number = 7").unwrap();
    write(
        &mut store,
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
         VALUES ('acme/new', 1, 'https://github.com/acme/new/pull/1', 'New', 0, 'open', 'none', 'none', 't', 't')",
    )
    .unwrap();
    write(&mut store, "DELETE FROM pull_requests WHERE number = 9").unwrap();
    let history = rows(
        &mut store,
        "SELECT op, before, after FROM history WHERE tbl = 'pull_requests' AND id > 2 ORDER BY id",
    );
    let side = |row: &Value, n: usize| -> Value {
        row[n]
            .as_str()
            .map_or(Value::Null, |text| serde_json::from_str(text).unwrap())
    };
    let columns = |v: &Value| (v["mergeable"].clone(), v["merge_state"].clone());
    let unknown = || (Value::from("unknown"), Value::from("unknown"));
    assert_eq!(history.len(), 3);
    assert_eq!(history[0][0], "update");
    assert_eq!(
        (
            columns(&side(&history[0], 1)),
            columns(&side(&history[0], 2))
        ),
        (
            unknown(),
            (Value::from("conflicting"), Value::from("dirty"))
        )
    );
    assert_eq!(
        (history[1][0].as_str(), columns(&side(&history[1], 2))),
        (Some("insert"), unknown())
    );
    assert_eq!(
        (history[2][0].as_str(), columns(&side(&history[2], 1))),
        (Some("delete"), unknown())
    );

    // `pull_requests_owned` is kept and `pull_requests` still gets no `_no_delete`.
    let triggers = rows(
        &mut store,
        "SELECT name FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = 'pull_requests' ORDER BY name",
    );
    assert_eq!(
        triggers,
        [
            "pull_requests_hist_del",
            "pull_requests_hist_ins",
            "pull_requests_hist_upd",
            "pull_requests_owned"
        ]
        .map(|n| serde_json::json!([n]))
    );
}

#[test]
fn the_checks_of_mergeable_and_merge_state_refuse_a_value_outside_their_sets() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_10(&home);
    let mut store = Store::open(&home, 5000).unwrap();
    for ok in [
        "mergeable = 'mergeable'",
        "mergeable = 'conflicting'",
        "merge_state = 'has_hooks'",
        "merge_state = 'behind'",
    ] {
        write(
            &mut store,
            &format!("UPDATE pull_requests SET {ok} WHERE number = 7"),
        )
        .unwrap();
    }
    for bad in [
        "mergeable = 'MERGEABLE'",
        "mergeable = 'dirty'",
        "mergeable = NULL",
        "merge_state = 'DIRTY'",
        "merge_state = 'conflicting'",
        "merge_state = NULL",
    ] {
        let err = refused(
            &mut store,
            &format!("UPDATE pull_requests SET {bad} WHERE number = 7"),
        );
        assert!(
            format!("{err:?}").contains("CHECK") || format!("{err:?}").contains("NOT NULL"),
            "{bad}: {err:?}"
        );
    }
}

// ---------- migration 13: the pull request poll's Inbox kinds ----------

#[test]
fn a_version_12_database_migrates_to_13_with_its_rows_authored_and_its_history_listing_both_columns()
 {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    database_at(
        &home,
        12,
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at) VALUES
           ('acme/pay', 7, 'https://github.com/acme/pay/pull/7', 'Pay', 0, 'open', 'none', 'failed', 't', 't'),
           ('acme/web', 9, 'https://github.com/acme/web/pull/9', 'Web', 1, 'open', 'approved', 'passed', 't', 't');",
    );
    assert_eq!(
        legacy_schema_hash(&Connection::open(home.db()).unwrap()).unwrap(),
        SCHEMA_HASHES[11]
    );
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    assert_eq!(
        rows(
            &mut store,
            "SELECT number, authored, review_requested FROM pull_requests ORDER BY id"
        ),
        vec![serde_json::json!([7, 1, 0]), serde_json::json!([9, 1, 0])]
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM history WHERE tbl = 'pull_requests'"
        ),
        2,
        "the migration wrote no history row"
    );

    // An update, an insert and a delete: each history row lists both new columns.
    write(
        &mut store,
        "UPDATE pull_requests SET authored = 0, review_requested = 1 WHERE number = 7",
    )
    .unwrap();
    write(
        &mut store,
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored, review_requested)
         VALUES ('acme/new', 1, 'https://github.com/acme/new/pull/1', 'New', 0, 'open', 'none', 'none', 't', 't', 0, 1)",
    )
    .unwrap();
    write(&mut store, "DELETE FROM pull_requests WHERE number = 9").unwrap();
    let history = rows(
        &mut store,
        "SELECT op, before, after FROM history WHERE tbl = 'pull_requests' AND id > 2 ORDER BY id",
    );
    let side = |row: &Value, n: usize| -> Value {
        row[n]
            .as_str()
            .map_or(Value::Null, |text| serde_json::from_str(text).unwrap())
    };
    let columns = |v: &Value| (v["authored"].clone(), v["review_requested"].clone());
    let pair = |a: i64, r: i64| (Value::from(a), Value::from(r));
    assert_eq!(history.len(), 3);
    assert_eq!(history[0][0], "update");
    assert_eq!(
        (
            columns(&side(&history[0], 1)),
            columns(&side(&history[0], 2))
        ),
        (pair(1, 0), pair(0, 1))
    );
    assert_eq!(
        (history[1][0].as_str(), columns(&side(&history[1], 2))),
        (Some("insert"), pair(0, 1))
    );
    assert_eq!(
        (history[2][0].as_str(), columns(&side(&history[2], 1))),
        (Some("delete"), pair(1, 0))
    );

    // `pull_requests_owned` is kept and `pull_requests` still gets no `_no_delete`.
    let triggers = rows(
        &mut store,
        "SELECT name FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = 'pull_requests' ORDER BY name",
    );
    assert_eq!(
        triggers,
        [
            "pull_requests_hist_del",
            "pull_requests_hist_ins",
            "pull_requests_hist_upd",
            "pull_requests_owned"
        ]
        .map(|n| serde_json::json!([n]))
    );
}

#[test]
fn a_version_12_database_keeps_its_inbox_items_and_accepts_the_four_pull_request_kinds_in_13() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    database_at(
        &home,
        12,
        "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES
           ('dialog', 'red', 'session:a', 'l1', 't1'), ('unread', 'yellow', 'plan:p', 'l2', 't2');
         INSERT INTO inbox_items (kind, urgency, subject, label, opened_at, closed_at) VALUES ('dialog', 'red', 'session:a', 'l3', 't3', 't4');",
    );
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, kind, subject, label, closed_at FROM inbox_items ORDER BY id"
        ),
        vec![
            serde_json::json!([1, "dialog", "session:a", "l1", null]),
            serde_json::json!([2, "unread", "plan:p", "l2", null]),
            serde_json::json!([3, "dialog", "session:a", "l3", "t4"]),
        ]
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM history WHERE tbl = 'inbox_items'"
        ),
        3,
        "the copy wrote no history row (the 3 are the inserts)"
    );
    // The partial unique index, the history triggers and `inbox_items_no_delete` are back.
    assert!(write(&mut store, "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('dialog', 'red', 'session:a', 'l', 't')").is_err());
    let history = |store: &mut Store| {
        count(
            store,
            "SELECT count(*) FROM history WHERE tbl = 'inbox_items'",
        )
    };
    let kept = history(&mut store);
    for kind in ["pr-failing", "pr-changes", "pr-ready", "review-requested"] {
        write(&mut store, &format!("INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('{kind}', 'red', 'pr:{kind}', 'l', 't')")).unwrap_or_else(|e| panic!("{kind}: {e:?}"));
        write(
            &mut store,
            &format!("UPDATE inbox_items SET closed_at = 't' WHERE subject = 'pr:{kind}'"),
        )
        .unwrap();
    }
    assert_eq!(
        history(&mut store) - kept,
        8,
        "an insert and an update each"
    );
    assert_eq!(
        sqlite_message(&refused(&mut store, "DELETE FROM inbox_items")),
        "append_only: inbox_items"
    );
    assert!(write(&mut store, "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('engine-down', 'red', 'engine', 'l', 't')").is_err());
}

#[test]
fn the_checks_of_authored_and_review_requested_refuse_a_value_outside_0_and_1() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_10(&home);
    let mut store = Store::open(&home, 5000).unwrap();
    for ok in [
        "authored = 0",
        "review_requested = 1",
        "authored = 1",
        "review_requested = 0",
    ] {
        write(
            &mut store,
            &format!("UPDATE pull_requests SET {ok} WHERE number = 7"),
        )
        .unwrap();
    }
    for bad in [
        "authored = 2",
        "review_requested = -1",
        "authored = NULL",
        "review_requested = NULL",
    ] {
        let err = refused(
            &mut store,
            &format!("UPDATE pull_requests SET {bad} WHERE number = 7"),
        );
        assert!(
            format!("{err:?}").contains("CHECK") || format!("{err:?}").contains("NOT NULL"),
            "{bad}: {err:?}"
        );
    }
}

#[test]
fn migration_6_keeps_migration_5s_triggers_on_prs() {
    let dir = tempfile::tempdir().unwrap();
    let (store, before) = migrated_5(&dir);
    drop(store);
    // `prs_shape_fixed` is `job-kinds`' and `prs_head_ref_fixed` is `pr-adopt`'s: neither is one of migration 5's.
    let after: Vec<(String, String)> =
        triggers_on(&Connection::open(home(&dir).db()).unwrap(), "workflows")
            .into_iter()
            .filter(|(name, _)| {
                name != "workflows_shape_fixed"
                    && name != "workflows_head_ref_fixed"
                    && name != "workflows_hist_del"
            })
            .collect();
    // `workflow-rename` (after this migration) renames the object triggers `prs_*` to `workflows_*`; map the old names
    // to compare them.
    let renamed = |name: &str| name.replacen("prs_", "workflows_", 1);
    let names_after =
        |t: &[(String, String)]| t.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>();
    let names_before: Vec<String> = before.iter().map(|(name, _)| renamed(name)).collect();
    assert_eq!(
        names_after(&after),
        names_before,
        "the same triggers on workflows"
    );
    for ((name, old), (_, new)) in before.iter().zip(&after) {
        // The history triggers list the rebuilt table's columns; every other keeps its text.
        // `prs_status` also has `dispatch>cancelled` since `pr-cancel-started` and `open>cancelled` since `pr-cancel-open`.
        // `prs_locked` has a second exception since `stacked-prs`: a stacked or an open Workflow's `base` follows GitHub.
        // `prs_no_delete` has the delete exception since `delete-tombstone`, and `workflow-rename` retargets the text.
        let mapped = renamed(name);
        if mapped != "workflows_hist_ins"
            && mapped != "workflows_hist_upd"
            && mapped != "workflows_status"
            && mapped != "workflows_locked"
            && mapped != "workflows_no_delete"
        {
            assert_eq!(new, old, "{name}");
        }
    }
    let locked = &after
        .iter()
        .find(|(name, _)| name == "workflows_locked")
        .unwrap()
        .1;
    assert!(
        !locked.contains("OLD.status = 'merged'"),
        "`lead-branch-name` dropped migration 5's exception: {locked}"
    );
    let status = &after
        .iter()
        .find(|(name, _)| name == "workflows_status")
        .unwrap()
        .1;
    assert!(
        status.contains("'waiting>cancelled', 'dispatch>cancelled', 'open>cancelled'"),
        "workflows_status: {status}"
    );
}

#[test]
fn after_migration_6_prs_locked_refuses_the_legacy_lead_branch_rename_of_a_merged_pr() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, _) = migrated_5(&dir);
    for sql in [
        "UPDATE workflows SET lead_branch = 'cpd/op/pay' WHERE plan = 'op'",
        "UPDATE workflows SET lead_branch = 'cpd/wa' WHERE plan = 'wa'",
        "UPDATE workflows SET lead_branch = 'cpd/dn/web', base = 'main' WHERE plan = 'dn'",
        "UPDATE workflows SET repo = '/src/other' WHERE plan = 'dn'",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{sql}: {err:?}");
    }
    // `lead-branch-name` (pro-d113 A3) removed the exception: the rename itself is refused too.
    let err = refused(
        &mut store,
        "UPDATE workflows SET lead_branch = 'cpd/dn/web' WHERE plan = 'dn'",
    );
    assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{err:?}");
}

// ---------- migration 12 (the Inbox) ----------

/// A version 6 database with a Context and two Events.
fn version_6(home: &CpdHome) {
    let sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
         INSERT INTO events (txn, at, kind, context, text, data) VALUES
           (1, '{T}', 'context-created', 'ctx', 'x', '{{}}'), (1, '{T}', 'pr-merged', 'ctx', 'x', '{{}}');"
    );
    database_at(home, 6, &sql);
}

#[test]
fn a_version_6_database_migrates_to_12_with_inbox_items_and_the_four_inbox_event_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_6(&home);
    assert_eq!(
        legacy_schema_hash(&Connection::open(home.db()).unwrap()).unwrap(),
        SCHEMA_HASHES[5]
    );

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    assert_eq!(
        canonical_hash(&Connection::open(home.db()).unwrap()).unwrap(),
        SCHEMA_HASH
    );
    assert_eq!(
        rows(&mut store, "SELECT id, kind FROM events ORDER BY id"),
        vec![
            serde_json::json!([1, "context-created"]),
            serde_json::json!([2, "pr-merged"])
        ]
    );
    assert_eq!(count(&mut store, "SELECT count(*) FROM inbox_items"), 0);

    // The four kinds are accepted, an unknown one is not, and ids go on after the kept ones.
    write(&mut store, "INSERT INTO events (txn, at, kind, context, text, data) VALUES
                         (cpd_txn(), 't', 'inbox-opened', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'inbox-closed', 'ctx', 'x', '{}'),
                         (cpd_txn(), 't', 'inbox-snoozed', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'inbox-dismissed', 'ctx', 'x', '{}')")
        .unwrap();
    assert_eq!(count(&mut store, "SELECT max(id) FROM events"), 6);
    assert!(write(&mut store, "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), 't', 'inbox-other', 'x', '{}')").is_err());
    // The rebuilt table kept its triggers.
    for sql in ["UPDATE events SET text = 'y'", "DELETE FROM events"] {
        assert_eq!(
            sqlite_message(&refused(&mut store, sql)),
            "append_only: events",
            "{sql}"
        );
    }
}

#[test]
fn the_inbox_items_table_accepts_the_urgency_blue_and_refuses_others() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    let mut store = Store::open(&home, 5000).unwrap();
    let item = |urgency: &str| {
        format!(
            "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('curate-docs', '{urgency}', 'curate-docs:ctx', 'l', 't');"
        )
    };
    for urgency in ["white", "cyan", "blue", "magenta", "yellow", "red"] {
        write(&mut store, &item(urgency)).unwrap_or_else(|e| panic!("{urgency}: {e:?}"));
    }
    assert!(write(&mut store, &item("teal")).is_err());
    assert!(write(&mut store, &item("")).is_err());
}

#[test]
fn inbox_items_keep_one_open_item_per_identity_and_write_history_and_never_delete() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_6(&home);
    let mut store = Store::open(&home, 5000).unwrap();
    let item = |urgency: &str| {
        format!(
            "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('dialog', '{urgency}', 'session:a', 'l', 't');"
        )
    };
    write(&mut store, &item("red")).unwrap();
    // The same identity cannot be open twice; another urgency is another identity.
    assert!(write(&mut store, &item("red")).is_err());
    write(&mut store, &item("magenta")).unwrap();
    // Once closed, the identity is free again.
    write(
        &mut store,
        "UPDATE inbox_items SET closed_at = 't' WHERE urgency = 'red'",
    )
    .unwrap();
    write(&mut store, &item("red")).unwrap();
    assert_eq!(
        rows(
            &mut store,
            "SELECT pk, op FROM history WHERE tbl = 'inbox_items' ORDER BY id"
        ),
        vec![
            serde_json::json!(["[1]", "insert"]),
            serde_json::json!(["[2]", "insert"]),
            serde_json::json!(["[1]", "update"]),
            serde_json::json!(["[3]", "insert"]),
        ]
    );
    assert_eq!(
        sqlite_message(&refused(&mut store, "DELETE FROM inbox_items")),
        "append_only: inbox_items"
    );
    // A kind the poll adds later is refused until its migration; `engine-down` is never stored.
    assert!(write(&mut store, "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('engine-down', 'red', 'engine', 'l', 't')").is_err());
}

#[test]
fn a_migrated_database_has_the_schema_of_a_new_one() {
    let dir = tempfile::tempdir().unwrap();
    drop(migrated(&dir));
    let fresh = tempfile::tempdir().unwrap();
    let fresh_home = home(&fresh);
    drop(Store::open(&fresh_home, 5000).unwrap());
    let hash = |home: &CpdHome| canonical_hash(&Connection::open(home.db()).unwrap()).unwrap();
    assert_eq!(hash(&home(&dir)), hash(&fresh_home));
    assert_eq!(hash(&fresh_home), SCHEMA_HASH);
}

// ---------- migration 7: the old Context stores ----------

const NOTE_1: &str = r#"{"v":1,"id":"m1-n1","kind":"note","at":"2026-01-02T00:00:00.000Z","machine":"m1","by":{"session":null,"plan":null},"text":"Indexed"}"#;
const NOTE_2: &str = r#"{"v":1,"id":"m1-n2","kind":"note","at":"2026-01-03T00:00:00.000Z","machine":"m1","by":{"session":null,"plan":null},"text":"Not indexed"}"#;
const PLAN_1: &str = r#"{"v":1,"id":"m1-p1","kind":"plan","at":"2026-01-04T00:00:00.000Z","machine":"m1","by":{"session":null,"plan":null},"text":"Pay","plan":{"id":"pay","machine":"m1","repo":"/r/pay","jira":"PAY-1"}}"#;
const OTHER_1: &str = r#"{"v":1,"id":"m2-n1","kind":"note","at":"2026-01-05T00:00:00.000Z","machine":"m2","by":{"session":null,"plan":null},"text":"From m2"}"#;

/// `contexts/<id>/` as an earlier version wrote it, with its record files.
fn old_store(home: &CpdHome, id: &str, files: &[(&str, &[&str])]) {
    let dir = home.old_contexts_dir().join(id);
    std::fs::create_dir_all(dir.join("records")).unwrap();
    std::fs::write(
        dir.join("context.json"),
        format!(r#"{{"v":1,"id":"{id}","title":"Title of {id}","jira":"ABC-1","created_at":"{T}","created_on":"m1"}}"#),
    )
    .unwrap();
    for (machine, lines) in files {
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(dir.join("records").join(format!("{machine}.jsonl")), text).unwrap();
    }
}

/// A version-6 database whose index holds Context `known` with its first note only.
fn version_6_with_known_context(home: &CpdHome) {
    database_at(
        home,
        6,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('known', 'Known', '{T}', 'm1');
             UPDATE contexts SET lead_session = NULL WHERE id = 'known';
             INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('known', 'm1-n1', 'note', '{T}', 'm1', 'Indexed');"
        ),
    );
}

#[test]
fn migration_7_imports_what_the_index_lacks_and_archives_the_folders_and_a_reopen_changes_nothing()
{
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_6_with_known_context(&home);
    old_store(
        &home,
        "known",
        &[("m1", &[NOTE_1, NOTE_2, PLAN_1]), ("m2", &[OTHER_1])],
    );
    old_store(&home, "fresh", &[("m1", &[NOTE_1])]);
    std::fs::write(home.old_contexts_dir().join("README"), "not a store").unwrap();

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM pragma_table_info('contexts') WHERE name = 'sync_failing'"
        ),
        0
    );
    // The Context the index had keeps its row; the index's own record keeps its seq.
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, title, jira FROM contexts ORDER BY id"
        ),
        vec![
            serde_json::json!(["fresh", "Title of fresh", "ABC-1"]),
            serde_json::json!(["known", "Known", null])
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT context, id, seq FROM context_records ORDER BY seq"
        ),
        vec![
            serde_json::json!(["known", "m1-n1", 1]),
            serde_json::json!(["fresh", "m1-n1", 2]),
            serde_json::json!(["known", "m1-n2", 3]),
            serde_json::json!(["known", "m1-p1", 4]),
            serde_json::json!(["known", "m2-n1", 5]),
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT plan_id, plan_machine, plan_jira FROM context_records WHERE id = 'm1-p1'"
        ),
        vec![serde_json::json!(["pay", "m1", "PAY-1"])]
    );
    // `context-created` for the new Context, `context-record` for each imported record, in the migration's txn.
    assert_eq!(
        rows(
            &mut store,
            "SELECT kind, context, json_extract(data, '$.record'), txn FROM events ORDER BY id"
        ),
        vec![
            serde_json::json!(["context-created", "fresh", null, 2]),
            serde_json::json!(["context-record", "fresh", "m1-n1", 2]),
            serde_json::json!(["context-record", "known", "m1-n2", 2]),
            serde_json::json!(["context-record", "known", "m1-p1", 2]),
            serde_json::json!(["context-record", "known", "m2-n1", 2]),
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT text FROM events WHERE kind = 'context-created'"
        ),
        vec![serde_json::json!([
            "Context fresh \"Title of fresh\" exists. Next: cpd context show --context fresh"
        ])]
    );

    // The folders are archived, nothing else is touched.
    let archived = home.archived_contexts_dir();
    for id in ["known", "fresh"] {
        assert!(
            !home.old_contexts_dir().join(id).exists(),
            "{id} is still in contexts/"
        );
        assert!(
            archived.join(id).join("context.json").is_file(),
            "{id} is not archived"
        );
    }
    assert!(archived.join("known/records/m2.jsonl").is_file());
    assert!(
        home.old_contexts_dir().join("README").is_file(),
        "a file that is no store stays"
    );

    // A second open changes nothing.
    let before = (
        count(&mut store, "SELECT count(*) FROM context_records"),
        count(&mut store, "SELECT max(id) FROM events"),
        count(&mut store, "SELECT max(id) FROM txns"),
    );
    drop(store);
    let mut store = Store::open(&home, 5000).unwrap();
    let after = (
        count(&mut store, "SELECT count(*) FROM context_records"),
        count(&mut store, "SELECT max(id) FROM events"),
        count(&mut store, "SELECT max(id) FROM txns"),
    );
    assert_eq!(before, after);
    assert!(archived.join("fresh/context.json").is_file());
}

#[test]
fn the_import_of_migration_7_is_idempotent_on_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    old_store(&home, "again", &[("m1", &[NOTE_1, NOTE_2])]);
    let conn = connection(&dir.path().join("x.db"));
    at_version(&conn, 6);
    conn.execute_batch(&format!(
        "INSERT INTO txns (id, at, command, pid, session, cwd) VALUES (1, '{T}', 'migrate', 1, NULL, '/')"
    ))
    .unwrap();
    migration_7(&conn).unwrap();
    import_old_stores(&conn, &home.old_contexts_dir()).unwrap();
    let counts = |conn: &Connection| -> (i64, i64, i64) {
        let n = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        (
            n("SELECT count(*) FROM contexts"),
            n("SELECT count(*) FROM context_records"),
            n("SELECT count(*) FROM events"),
        )
    };
    assert_eq!(counts(&conn), (1, 2, 3));
    conn.execute_batch("DROP TRIGGER contexts_hist_ins; DROP TRIGGER contexts_hist_upd; DROP TRIGGER contexts_no_delete;")
        .unwrap();
    conn.execute_batch("ALTER TABLE contexts ADD COLUMN sync_failing INTEGER NOT NULL DEFAULT 0")
        .unwrap();
    import_old_stores(&conn, &home.old_contexts_dir()).unwrap();
    assert_eq!(counts(&conn), (1, 2, 3));
}

#[test]
fn an_existing_destination_is_never_overwritten_the_folder_stays_and_the_open_fails_loud() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_6_with_known_context(&home);
    old_store(&home, "blocked", &[("m1", &[NOTE_1])]);
    old_store(&home, "moves", &[("m1", &[NOTE_1])]);
    let taken = home.archived_contexts_dir().join("blocked");
    std::fs::create_dir_all(&taken).unwrap();
    std::fs::write(taken.join("mine.txt"), "keep").unwrap();

    let err = Store::open(&home, 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
    assert!(err.message().contains("blocked"), "{}", err.message());
    assert!(
        home.old_contexts_dir()
            .join("blocked/context.json")
            .is_file(),
        "the folder stays"
    );
    assert_eq!(
        std::fs::read_to_string(taken.join("mine.txt")).unwrap(),
        "keep"
    );
    assert!(
        !taken.join("context.json").exists(),
        "nothing was written over it"
    );
    assert!(
        home.archived_contexts_dir()
            .join("moves/context.json")
            .is_file(),
        "the others are moved"
    );

    // The database did migrate, so the next open works; the folder still stays.
    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM context_records WHERE context = 'blocked'"
        ),
        1
    );
    assert!(
        home.old_contexts_dir()
            .join("blocked/context.json")
            .is_file()
    );
}

#[test]
fn a_folder_that_is_not_a_valid_store_fails_the_migration_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_6_with_known_context(&home);
    old_store(&home, "good", &[("m1", &[NOTE_1])]);
    old_store(&home, "bad", &[("m1", &[NOTE_1, "{not json"])]);
    let err = Store::open(&home, 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::MigrationFailed, "{err:?}");
    assert!(
        err.message().contains("Context bad") && err.message().contains("line 2"),
        "{}",
        err.message()
    );
    let conn = Connection::open(home.db()).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 6);
    assert_eq!(legacy_schema_hash(&conn).unwrap(), SCHEMA_HASHES[5]);
    let contexts: i64 = conn
        .query_row("SELECT count(*) FROM contexts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(contexts, 1);
    assert!(
        home.old_contexts_dir().join("good/context.json").is_file(),
        "no folder is moved"
    );
    assert!(!home.archived_contexts_dir().exists());
}

#[test]
fn a_migration_that_breaks_a_foreign_key_fails_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    // An Event about a Plan that does not exist (written with foreign keys off).
    database_at(&home, 2, &format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
         INSERT INTO events (txn, at, kind, context, plan, text, data) VALUES (1, '{T}', 'plan-created', 'ctx', 'gone', 'x', '{{}}');"
    ));
    let err = Store::open(&home, 5000).err().unwrap();
    assert_eq!(err.code(), ErrorCode::MigrationFailed, "{err:?}");
    assert_eq!(
        err.details()["foreign_key_violations"],
        serde_json::json!([{"table": "events", "rowid": 1, "parent": "plans"}])
    );
    let conn = Connection::open(home.db()).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
    assert_eq!(legacy_schema_hash(&conn).unwrap(), SCHEMA_HASHES[1]);
}

// ---------- legacy migration 14: stub drafts ----------

/// A version 13 database: a Plan in `dispatch` with one Workflow and a Job of every status
/// (a review Job among them), two Dependencies and an Attempt.
fn version_13(home: &CpdHome) {
    database_at(
        home,
        13,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-pay', 'lead', 'ctx', 'pay', 'lead', '/src', 'O3', 'idle', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'l-pay', '{T}', '{T}');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', 'api', '/src/pay', 'main', 'dispatch', 'cpd/pay/api', '/src/pay.wt', 1, '{T}', '{T}');
             INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                               fingerprint, created_txn, created_at, merged_sha, failed_cause, cancel_reason, cancel_requested_at, review) VALUES
               ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, '{FP}', 1, '{T}', 'abc', NULL, NULL, NULL, 0),
               ('pay', 'api', 'b', 'B', 'S3', 'g', 'c', '[\"a\"]', '[\"b\"]', '[\"x\"]', 'running', 0, '{FP}', 1, '{T}', NULL, NULL, NULL, NULL, 0),
               ('pay', 'api', 'c', 'C', 'O3', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, '{FP}', 1, '{T}', NULL, NULL, NULL, NULL, 0),
               ('pay', 'api', 'd', 'D', 'S2', 'g', '', '[\"a\"]', '[\"b\"]', '[]', 'failed', 0, '{FP}', 1, '{T}', NULL, 'crashed', NULL, NULL, 0),
               ('pay', 'api', 'e', 'E', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 1, NULL, 1, '{T}', NULL, NULL, NULL, NULL, 0),
               ('pay', 'api', 'f', 'F', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'cancelled', 1, NULL, 1, '{T}', NULL, NULL, 'no', '{T}', 0),
               ('pay', 'api', 'r', 'R', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 1, '{FP}', 1, '{T}', 'def', NULL, NULL, NULL, 1);
             INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'c', 'b'), ('pay', 'e', 'c');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
               VALUES ('pay', 'b-a1', 'b', 1, 'S3', 'ladder', 'running', 'cpd/pay--b-a1', '/src/pay--b-a1', '{T}');"
        ),
    );
}

#[test]
fn a_version_13_database_migrates_to_14_keeping_every_job_and_seq_and_its_checks_triggers_and_history()
 {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_13(&home);
    assert_eq!(
        legacy_schema_hash(&Connection::open(home.db()).unwrap()).unwrap(),
        SCHEMA_HASHES[12]
    );
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    assert_eq!(
        count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"),
        0
    );
    // The schema is the one a fresh store gets.
    assert_eq!(
        canonical_hash(&Connection::open(home.db()).unwrap()).unwrap(),
        SCHEMA_HASH
    );

    // Every row, with its seq, its Spec and its review flag, as it was.
    let n = Value::Null;
    assert_eq!(
        rows(
            &mut store,
            "SELECT seq, key, status, tier, goal, context, touches, out_of_scope, review, fingerprint, merged_sha FROM jobs ORDER BY seq"
        ),
        vec![
            serde_json::json!([
                1, "a", "merged", "S2", "g", "c", "[\"a\"]", "[]", 0, FP, "abc"
            ]),
            serde_json::json!([
                2, "b", "running", "S3", "g", "c", "[\"a\"]", "[\"x\"]", 0, FP, n
            ]),
            serde_json::json!([3, "c", "queued", "O3", "g", "c", "[\"a\"]", "[]", 0, FP, n]),
            serde_json::json!([4, "d", "failed", "S2", "g", "", "[\"a\"]", "[]", 0, FP, n]),
            serde_json::json!([5, "e", "draft", "S2", "g", "c", "[\"a\"]", "[]", 0, n, n]),
            serde_json::json!([
                6,
                "f",
                "cancelled",
                "S2",
                "g",
                "c",
                "[\"a\"]",
                "[]",
                0,
                n,
                n
            ]),
            serde_json::json!([
                7, "r", "merged", "S2", "g", "c", "[\"a\"]", "[]", 1, FP, "def"
            ]),
        ]
    );
    assert_eq!(count(&mut store, "SELECT count(*) FROM dependencies"), 2);
    assert_eq!(count(&mut store, "SELECT count(*) FROM attempts"), 1);
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM history WHERE tbl = 'jobs'"
        ),
        7,
        "the copy wrote no history row (the 7 are the inserts)"
    );

    // A draft with only a title is accepted, with either half given too, and a stub can be cancelled.
    let stub = |key: &str, columns: &str, values: &str| {
        format!(
            "INSERT INTO jobs (plan, workflow, key, title, {columns}status, appended, created_txn, created_at)
             VALUES ('pay', 'api', '{key}', 'T', {values}'draft', 1, cpd_txn(), '{T}');"
        )
    };
    write(&mut store, &stub("s1", "", "")).unwrap();
    write(&mut store, &stub("s2", "tier, ", "'S2', ")).unwrap();
    write(
        &mut store,
        &stub(
            "s3",
            "goal, context, touches, acceptance, out_of_scope, ",
            "'g', 'c', '[\"a\"]', '[\"b\"]', '[]', ",
        ),
    )
    .unwrap();
    assert_eq!(
        count(&mut store, "SELECT max(seq) FROM jobs"),
        10,
        "AUTOINCREMENT goes on after the kept seq"
    );
    write(&mut store, &format!("UPDATE jobs SET status = 'cancelled', cancel_reason = 'no', cancel_requested_at = '{T}' WHERE key = 's1'")).unwrap();
    assert_eq!(
        rows(
            &mut store,
            "SELECT status, tier, goal FROM jobs WHERE key = 's1'"
        ),
        vec![serde_json::json!(["cancelled", n, n])]
    );

    // Anything that is not a draft or cancelled has its Tier and Spec; the Spec is all or nothing; each field keeps its own check.
    let check = |store: &mut Store, sql: &str| {
        let message = sqlite_message(&refused(store, sql));
        assert!(
            message.contains("CHECK constraint failed"),
            "{sql}: {message}"
        );
    };
    check(
        &mut store,
        &format!("UPDATE jobs SET status = 'queued', fingerprint = '{FP}' WHERE key = 's2'"),
    );
    check(
        &mut store,
        &format!("UPDATE jobs SET status = 'queued', fingerprint = '{FP}' WHERE key = 's3'"),
    );
    check(&mut store, &stub("s4", "tier, goal, ", "'S2', 'g', "));
    check(&mut store, &stub("s5", "tier, ", "'X9', "));
    check(
        &mut store,
        &stub(
            "s6",
            "goal, context, touches, acceptance, out_of_scope, ",
            "'', 'c', '[\"a\"]', '[\"b\"]', '[]', ",
        ),
    );
    check(
        &mut store,
        &stub(
            "s7",
            "goal, context, touches, acceptance, out_of_scope, ",
            "'g', 'c', '[]', '[\"b\"]', '[]', ",
        ),
    );
    check(
        &mut store,
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, status, appended, fingerprint, created_txn, created_at)
             VALUES ('pay', 'api', 's8', 'T', 'queued', 1, '{FP}', cpd_txn(), '{T}');"
        ),
    );

    // Completed, a stub is queued like any draft.
    write(&mut store, "UPDATE jobs SET tier = 'S2' WHERE key = 's3'").unwrap();
    write(
        &mut store,
        &format!("UPDATE jobs SET status = 'queued', fingerprint = '{FP}' WHERE key = 's3'"),
    )
    .unwrap();

    // The triggers of §4.4 on `jobs` are back: the Locked guard, the legal transitions, the history and no delete.
    assert_eq!(
        rows(
            &mut store,
            "SELECT name FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = 'jobs' ORDER BY name"
        ),
        // `jobs_kind_spec_ins` and `_upd` are `job-kinds`'; `jobs_hist_del` is `delete-tombstone`'s (the generator's).
        [
            "jobs_hist_del",
            "jobs_hist_ins",
            "jobs_hist_upd",
            "jobs_kind_spec_ins",
            "jobs_kind_spec_upd",
            "jobs_locked",
            "jobs_no_delete",
            "jobs_status",
            "jobs_workflow_dependencies"
        ]
        .map(|name| serde_json::json!([name]))
    );
}

// ---------- the triggers of §4.4 after migration 4 ----------

/// Runs `sql` in its own write transaction.
fn write(store: &mut Store, sql: &str) -> std::result::Result<(), CpdError> {
    store.write("test", |tx| Ok(tx.execute_batch(sql)?))
}

fn refused(store: &mut Store, sql: &str) -> CpdError {
    write(store, sql).expect_err(sql)
}

fn sqlite_message(err: &CpdError) -> String {
    err.details()["sqlite_message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// A second Workflow `web` in Plan `gr` (grounding), with a draft Job `v`, before `workflow-rename`: the object table is `prs`.
const WEB_BEFORE: &str = "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                     VALUES ('gr', 'web', '/src/web', 'main', 'waiting', 'cpd/gr/web', cpd_txn(), 't');
                   INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                     status, appended, created_txn, created_at)
                     VALUES ('gr', 'pay', 'u', 'U', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't'),
                            ('gr', 'web', 'v', 'V', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't');";

const WEB: &str = "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                     VALUES ('gr', 'web', '/src/web', 'main', 'waiting', 'cpd/gr/web', cpd_txn(), 't');
                   INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                     status, appended, created_txn, created_at)
                     VALUES ('gr', 'pay', 'u', 'U', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't'),
                            ('gr', 'web', 'v', 'V', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't');";

#[test]
fn prs_status_refuses_a_transition_outside_3_8() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated(&dir);
    for sql in [
        "UPDATE workflows SET status = 'open', lead_worktree = '/w', started_at = 't', pr_number = 1, pr_url = 'u', pr_opened_at = 't' WHERE plan = 'gr'",
        "UPDATE workflows SET status = 'waiting', lead_worktree = NULL, started_at = NULL WHERE plan = 'di'",
        "UPDATE workflows SET status = 'cancelled' WHERE plan = 'dn'",
        "UPDATE workflows SET status = 'open', merged_at = NULL WHERE plan = 'dn'",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::IllegalTransition, "{sql}: {err:?}");
    }
    write(&mut store, "UPDATE workflows SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE plan = 'gr';
                       UPDATE workflows SET status = 'cancelled' WHERE plan = 'pl';
                       UPDATE workflows SET status = 'cancelled', lead_worktree = NULL, started_at = NULL WHERE plan = 'gr';
                       INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                         VALUES ('acme/di', 11, 'u', 'D', 0, 'open', 'none', 'none', 't', 't');
                       UPDATE workflows SET status = 'open', pr_number = 11, pr_url = 'u', pr_opened_at = 't',
                              pull_request = (SELECT id FROM pull_requests WHERE number = 11) WHERE plan = 'di';
                       UPDATE workflows SET status = 'merged', merged_at = 't' WHERE plan = 'pr';
                       UPDATE workflows SET status = 'cancelled', lead_worktree = NULL, started_at = NULL, pr_number = NULL, pr_url = NULL,
                              pr_opened_at = NULL, pull_request = NULL WHERE plan = 'di'")
        .unwrap();
    let err = refused(
        &mut store,
        "UPDATE workflows SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE plan = 'di'",
    );
    assert_eq!(err.code(), ErrorCode::IllegalTransition, "{err:?}");
}

#[test]
fn prs_locked_refuses_a_change_of_identity_repo_or_base_after_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated(&dir);
    for sql in [
        "UPDATE workflows SET repo = '/src/other' WHERE plan = 'di'",
        "UPDATE workflows SET base = 'develop' WHERE plan = 'dn'",
        "UPDATE workflows SET key = 'other' WHERE plan = 'dn'",
        "UPDATE workflows SET lead_branch = 'cpd/di/pr-2fa' WHERE plan = 'di'",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{sql}: {err:?}");
        assert_eq!(err.exit().code(), 3);
    }
    // Before the lock they may change; after it, the other columns may.
    write(
        &mut store,
        "UPDATE workflows SET repo = '/src/other', base = 'develop' WHERE plan IN ('gr', 'pl');
                       UPDATE workflows SET all_merged_at = 't' WHERE plan = 'pr'",
    )
    .unwrap();
}

#[test]
fn pr_needs_locked_refuses_a_need_after_the_lock_unless_the_pr_is_new() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated(&dir);
    let add = |key: &str| {
        format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
               VALUES ('di', '{key}', '/src/web', 'main', 'waiting', 'cpd/di/{key}', cpd_txn(), 't');"
        )
    };
    write(&mut store, &add("old")).unwrap();
    let err = refused(
        &mut store,
        "INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('di', 'old', 'pr-2fa')",
    );
    assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{err:?}");
    // `workflow add` in dispatch: the Workflow and its needs in one transaction.
    write(
        &mut store,
        &format!(
            "{} INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('di', 'new', 'pr-2fa');",
            add("new")
        ),
    )
    .unwrap();
    // Before the lock, any time.
    write(&mut store, WEB).unwrap();
    write(
        &mut store,
        "INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('gr', 'web', 'pay')",
    )
    .unwrap();
}

#[test]
fn pr_needs_no_delete_refuses_after_the_lock_and_history_records_a_delete() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated(&dir);
    write(
        &mut store,
        &format!(
            "{WEB} INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('gr', 'web', 'pay');"
        ),
    )
    .unwrap();
    write(&mut store, "DELETE FROM workflow_needs WHERE plan = 'gr'").unwrap();
    assert_eq!(
        rows(
            &mut store,
            "SELECT pk, op, before, after FROM history WHERE tbl = 'workflow_needs' ORDER BY id"
        ),
        vec![
            serde_json::json!([
                r#"["gr","web","pay"]"#,
                "insert",
                Value::Null,
                r#"{"plan":"gr","workflow":"web","needs":"pay"}"#
            ]),
            serde_json::json!([
                r#"["gr","web","pay"]"#,
                "delete",
                r#"{"plan":"gr","workflow":"web","needs":"pay"}"#,
                Value::Null
            ]),
        ]
    );
    write(
        &mut store,
        "INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('gr', 'web', 'pay');
                       UPDATE plans SET phase = 'plan' WHERE id = 'gr';
                       UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'gr'",
    )
    .unwrap();
    let err = refused(&mut store, "DELETE FROM workflow_needs WHERE plan = 'gr'");
    assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{err:?}");
}

#[test]
fn dependencies_same_pr_refuses_a_dependency_across_prs() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated(&dir);
    write(&mut store, WEB).unwrap();
    let err = refused(
        &mut store,
        "INSERT INTO dependencies (plan, job, needs) VALUES ('gr', 'v', 'u')",
    );
    assert_eq!(err.code(), ErrorCode::NeedsOtherWorkflow, "{err:?}");
    assert_eq!(err.exit().code(), 3);
    write(
        &mut store,
        "UPDATE jobs SET workflow = 'web' WHERE key = 'u' AND plan = 'gr';
                       INSERT INTO dependencies (plan, job, needs) VALUES ('gr', 'v', 'u')",
    )
    .unwrap();
}

#[test]
fn jobs_pr_dependencies_refuses_moving_a_job_with_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated(&dir);
    write(&mut store, WEB).unwrap();
    write(
        &mut store,
        "UPDATE jobs SET workflow = 'web' WHERE key = 'u' AND plan = 'gr';
                       INSERT INTO dependencies (plan, job, needs) VALUES ('gr', 'v', 'u')",
    )
    .unwrap();
    for sql in [
        "UPDATE jobs SET workflow = 'pay' WHERE key = 'u' AND plan = 'gr'",
        "UPDATE jobs SET workflow = 'pay' WHERE key = 'v' AND plan = 'gr'",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::NeedsOtherWorkflow, "{sql}: {err:?}");
    }
    // `job edit --workflow --needs`: delete the Dependencies, move, insert again.
    write(
        &mut store,
        "DELETE FROM dependencies WHERE plan = 'gr';
                       UPDATE jobs SET workflow = 'pay' WHERE plan = 'gr';
                       INSERT INTO dependencies (plan, job, needs) VALUES ('gr', 'v', 'u')",
    )
    .unwrap();
}

#[test]
fn jobs_locked_refuses_changing_a_locked_jobs_pr_or_review() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated(&dir);
    write(&mut store, "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                         VALUES ('pr', 'web', '/src/web', 'main', 'waiting', 'cpd/pr/web', cpd_txn(), 't')")
        .unwrap();
    for sql in [
        "UPDATE jobs SET workflow = 'web' WHERE plan = 'pr' AND key = 'z'",
        "UPDATE jobs SET review = 1 WHERE plan = 'di' AND key = 'x'",
        "UPDATE jobs SET title = 'New' WHERE plan = 'di' AND key = 'x'",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::JobLocked, "{sql}: {err:?}");
    }
    write(
        &mut store,
        "UPDATE jobs SET review = 1, title = 'New' WHERE plan = 'pl' AND key = 'a'",
    )
    .unwrap();
}

#[test]
fn plans_phase_and_jobs_status_refuse_illegal_transitions() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated(&dir);
    for sql in [
        "UPDATE plans SET phase = 'deploy', locked_at = 't' WHERE id = 'pl'",
        "UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = 'di' AND key = 'x'",
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(err.code(), ErrorCode::IllegalTransition, "{sql}: {err:?}");
    }
}

#[test]
fn events_and_context_records_stay_append_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated(&dir);
    for (sql, message) in [
        ("UPDATE events SET text = 'y'", "append_only: events"),
        ("DELETE FROM events", "append_only: events"),
        (
            "UPDATE context_records SET text = 'y'",
            "append_only: context_records",
        ),
        (
            "DELETE FROM context_records",
            "append_only: context_records",
        ),
        ("DELETE FROM plans", "append_only: plans"),
        ("DELETE FROM workflows", "append_only: workflows"),
        ("DELETE FROM jobs", "append_only: jobs"),
        (
            "INSERT INTO events (txn, at, kind, context, text, data) VALUES (1, 't', 'pr-added', 'ctx', 'x', '{}')",
            "events.txn must be cpd_txn()",
        ),
    ] {
        let err = refused(&mut store, sql);
        assert_eq!(sqlite_message(&err), message, "{sql}");
    }
    // The six new Event kinds are accepted.
    write(&mut store, "INSERT INTO events (txn, at, kind, context, text, data) VALUES
                         (cpd_txn(), 't', 'pr-added', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'pr-edited', 'ctx', 'x', '{}'),
                         (cpd_txn(), 't', 'pr-cancelled', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'pr-started', 'ctx', 'x', '{}'),
                         (cpd_txn(), 't', 'pr-opened', 'ctx', 'x', '{}'), (cpd_txn(), 't', 'pr-merged', 'ctx', 'x', '{}')")
        .unwrap();
}

#[test]
fn the_rebuilt_tables_history_triggers_list_their_new_columns() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = migrated(&dir);
    write(
        &mut store,
        "UPDATE jobs SET review = 1 WHERE plan = 'pl' AND key = 'a';
                       UPDATE plans SET title = 'G2' WHERE id = 'gr'",
    )
    .unwrap();
    let after = rows(
        &mut store,
        "SELECT tbl, after FROM history WHERE op = 'update' AND tbl IN ('jobs', 'plans') ORDER BY id",
    );
    let job: Value = serde_json::from_str(after[0][1].as_str().unwrap()).unwrap();
    assert_eq!(job["review"], 1, "{job}");
    // An update row holds the changed columns only (§4.4 "History row shape"): `pr` is listed in the trigger.
    let upd = rows(
        &mut store,
        "SELECT sql FROM sqlite_schema WHERE name = 'jobs_hist_upd'",
    );
    assert!(
        upd[0][0]
            .as_str()
            .unwrap()
            .contains("WHERE OLD.workflow IS NOT NEW.workflow"),
        "{upd:?}"
    );
    let plan: Value = serde_json::from_str(after[1][1].as_str().unwrap()).unwrap();
    assert!(
        plan.get("repo").is_none() && plan["title"] == "G2",
        "{plan}"
    );
}

// ---------- named migrations: the conversion (§4.5 C1–C6) ----------

/// The database files of the legacy stores of versions 1 to `upto` (index
/// `n - 1`), each with a row in every table, as the integer-era binary left it:
/// `version_2`'s rows (without the columns migration 2 adds, for version 1),
/// taken through the legacy migrations to `n`, and a tier and a need. Built in
/// one pass: the store of version `n` is the store of `n - 1` after one more
/// migration, so the migrations run once each instead of `n` times (the extra
/// rows are added to a copy, after the migration, as before).
fn legacy_stores(upto: usize) -> Vec<Vec<u8>> {
    let tier = format!(
        "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0', 'S', 'm', '[]', '{T}');"
    );
    let dir = tempfile::tempdir().unwrap();
    let scratch = home(&dir);
    let sql = version_2_sql();
    let v1 = sql
        .replace(
            "pr_seen, pr_polled_at, pr_title, pr_review_decision, pr_checks,",
            "pr_seen, pr_polled_at,",
        )
        .replace("'Open Workflow', 'approved', 'passed', ", "");
    assert_ne!(v1, sql);
    database_at(&scratch, 1, &format!("{v1}{tier}"));
    let mut stores = vec![std::fs::read(scratch.db()).unwrap()];

    let chain_dir = tempfile::tempdir_in(dir.path()).unwrap();
    let chain = home(&chain_dir);
    database_at(&chain, 2, &format!("{sql}{tier}"));
    stores.push(std::fs::read(chain.db()).unwrap());
    let conn = connection(&chain.db());
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    for n in 3..=upto {
        conn.pragma_update(None, "legacy_alter_table", "ON")
            .unwrap();
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        (LEGACY[n - 1].apply)(&conn).unwrap();
        conn.execute_batch("COMMIT").unwrap();
        conn.pragma_update(None, "legacy_alter_table", "OFF")
            .unwrap();
        conn.pragma_update(None, "user_version", n as i64).unwrap();
        // The chain is at rest: no open transaction and so no journal file, so the file is the database.
        let mut bytes = std::fs::read(chain.db()).unwrap();
        if n >= 4 {
            std::fs::write(scratch.db(), &bytes).unwrap();
            let copy = connection(&scratch.db());
            copy.pragma_update(None, "foreign_keys", "ON").unwrap();
            copy.execute_batch(&format!("BEGIN; {WEB_BEFORE} INSERT INTO pr_needs (plan, pr, needs) VALUES ('gr', 'web', 'pay'); COMMIT;")).unwrap();
            if n >= 12 {
                copy.execute_batch(&format!(
                    "BEGIN; INSERT INTO inbox_items (kind, urgency, subject, plan, context, label, opened_at) VALUES ('dialog', 'red', 'plan:gr', 'gr', 'ctx', 'Answer', '{T}'); COMMIT;"
                ))
                .unwrap();
            }
            if n >= 14 {
                // A stub draft: a title and no Tier or Spec (0014-register).
                copy.execute_batch(&format!(
                    "BEGIN; INSERT INTO jobs (plan, pr, key, title, status, appended, created_txn, created_at) VALUES ('gr', 'web', 'stub', 'Stub', 'draft', 1, 1, '{T}'); COMMIT;"
                ))
                .unwrap();
            }
            drop(copy);
            bytes = std::fs::read(scratch.db()).unwrap();
        }
        stores.push(bytes);
    }
    stores.truncate(upto);
    stores
}

/// `store` (one of [`legacy_stores`]) as `home`'s database.
fn write_store(home: &CpdHome, store: &[u8]) {
    std::fs::create_dir_all(home.db().parent().unwrap()).unwrap();
    std::fs::write(home.db(), store).unwrap();
}

/// Every row of every table, as JSON text, by table, in rowid order.
fn dump(conn: &Connection) -> std::collections::BTreeMap<String, Vec<String>> {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let columns: Vec<String> = table_columns(conn, &table)
                .unwrap()
                .into_iter()
                .map(|(c, _)| format!("\"{c}\""))
                .collect();
            let without_rowid: bool = conn
                .query_row(
                    "SELECT wr FROM pragma_table_list WHERE name = ?1",
                    [&table],
                    |r| r.get(0),
                )
                .unwrap();
            let order = if without_rowid {
                columns.join(", ")
            } else {
                "rowid".to_owned()
            };
            let sql = format!(
                "SELECT json_array({}) FROM \"{table}\" ORDER BY {order}",
                columns.join(", ")
            );
            let rows = conn
                .prepare(&sql)
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}

fn rowids(conn: &Connection, table: &str) -> Vec<i64> {
    conn.prepare(&format!("SELECT rowid FROM \"{table}\" ORDER BY rowid"))
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_>>()
        .unwrap()
}

fn fresh_canonical_hash() -> String {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    drop(Store::open(&home, 5000).unwrap());
    canonical_hash(&Connection::open(home.db()).unwrap()).unwrap()
}

/// The legacy store of version `n` converts keeping every row and id, records
/// its migrations, converts twice to the same store, and ends with the schema
/// of a fresh store. One test per version (below): each takes a fraction of
/// what all of them take, and they run side by side.
fn converts_keeping_every_row_and_id_at(n: usize) {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    write_store(&home, &legacy_stores(n).pop().unwrap());
    let before = Connection::open(home.db()).unwrap();
    let rows_before = dump(&before);
    let ids_before: Vec<(String, Vec<i64>)> = rows_before
        .keys()
        .map(|t| (t.clone(), rowids(&before, t)))
        .collect();
    let empty: Vec<&String> = ids_before
        .iter()
        .filter(|(_, ids)| ids.is_empty())
        .map(|(t, _)| t)
        .collect();
    assert!(
        empty.is_empty(),
        "version {n}: the fixture has no row in {empty:?}"
    );
    drop(before);

    let store = Store::open(&home, 5000).unwrap_or_else(|e| panic!("version {n}: {e:?}"));
    assert!(
        store.schema_mismatch().is_none(),
        "version {n}: {:?}",
        store.schema_mismatch()
    );
    drop(store);

    let conn = Connection::open(home.db()).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, VERSION, "version {n}");
    let violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0, "version {n}: foreign_key_check");
    for (table, ids) in &ids_before {
        // `workflow-rename` renames the object tables: the post-open store speaks the new names.
        let table = match table.as_str() {
            "prs" => "workflows",
            "pr_needs" => "workflow_needs",
            other => other,
        };
        let after = rowids(&conn, table);
        if table == "tier_catalog" {
            // Migration `20261030T1100-tier-catalog-source` rebuilds tier_catalog EMPTY: it is a cache, recomputed by the next
            // `cpd tiers`, so it is the one table conversion does not keep.
            assert!(
                after.is_empty(),
                "version {n}: tier_catalog is empty after conversion (a cache, rebuilt by tier-catalog-source)"
            );
            continue;
        }
        assert!(
            ids.iter().all(|id| after.contains(id)),
            "version {n}: {table} lost a row id"
        );
    }
    let recorded: Vec<(String, String, i64, i64)> = conn
        .prepare("SELECT name, compat, seq, converted FROM schema_migrations ORDER BY seq")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_>>()
        .unwrap();
    let legacy = LEGACY.iter().enumerate().map(|(i, l)| {
        (
            l.name.to_owned(),
            l.compat.as_str().to_owned(),
            i as i64 + 1,
            1,
        )
    });
    let named = NAMED.iter().enumerate().map(|(i, m)| {
        (
            m.name.to_owned(),
            m.compat.as_str().to_owned(),
            (LEGACY.len() + i) as i64 + 1,
            0,
        )
    });
    let expected: Vec<(String, String, i64, i64)> = legacy.chain(named).collect();
    assert_eq!(
        recorded, expected,
        "version {n}: one converted row per legacy migration, then the named ones as not converted"
    );
    // The pinned hash is a fresh store's (`a_fresh_store_has_the_pinned_canonical_hash`).
    assert_eq!(
        canonical_hash(&conn).unwrap(),
        SCHEMA_HASH,
        "version {n}: the canonical hash of a fresh store"
    );

    if n == LEGACY.len() {
        // Nothing but the runner's own rows changes in a store that is already at the last legacy version.
        let mut rows_after = dump(&conn);
        let mut rows_before = rows_before;
        // `workflow-rename` renames the object tables: the pre-rename dump's keys follow, so the rows compare.
        let workflows = rows_before.remove("prs").unwrap();
        let workflow_needs = rows_before.remove("pr_needs").unwrap();
        rows_before.insert("workflows".into(), workflows);
        rows_before.insert("workflow_needs".into(), workflow_needs);
        assert_eq!(
            rows_after.remove("schema_migrations").map(|r| r.len()),
            Some(LEGACY.len() + NAMED.len())
        );
        // `kind-tables`' own tables: every Event kind that exists, the record kinds, the tables with history.
        // `workflow-event-kinds` adds the six kinds of the object beside the six it retired, which stay as history.
        assert_eq!(
            rows_after.remove("event_kinds").map(|r| r.len()),
            Some(crate::events::Kind::ALL.len() + RETIRED_OBJECT_KINDS)
        );
        assert_eq!(rows_after.remove("record_kinds").map(|r| r.len()), Some(6));
        assert_eq!(
            rows_after.remove("slack_reactions").map(|r| r.len()),
            Some(0)
        ); // `slack-reactions`: a new empty table
        assert_eq!(rows_after.remove("jira_transitions"), Some(Vec::new())); // `jira-transitions`: a new empty table
        assert_eq!(
            rows_after.remove("history_tables").map(|r| r.len()),
            Some(27)
        ); // 17, `board_posts`, `record_retirements`, `record_curations`, `incident`, `incident_plans`, `pr_posts`, `job_pools`, `job_addenda`, `context_reads` and `record_topics`
        let (txns_before, txns_after) = (
            rows_before.remove("txns").unwrap(),
            rows_after.remove("txns").unwrap(),
        );
        assert_eq!(
            txns_after[..txns_before.len()],
            txns_before[..],
            "the migrating transaction only adds a txns row"
        );
        assert_eq!(txns_after.len(), txns_before.len() + 1);
        // `job-urgent` added the column `urgent`, last: every Job reads 0, the default.
        for row in rows_before.get_mut("jobs").unwrap() {
            *row = format!("{},0]", row.strip_suffix(']').unwrap());
        }
        // `policy-keys` added `contexts.owner` and `prs.name`, last, and filled them (the fixture's
        // pull requests are `acme/…`), writing an `update` history row per filled row in the migrating transaction.
        // (`delete-tombstone`'s `deleted_at` comes after `owner`, so the column is found by its position.)
        let owner = table_columns(&conn, "contexts")
            .unwrap()
            .iter()
            .position(|(c, _)| c == "owner")
            .unwrap();
        assert_eq!(
            rows_after["contexts"]
                .iter()
                .map(|r| serde_json::from_str::<Vec<Value>>(r).unwrap().remove(owner))
                .collect::<Vec<_>>(),
            [Value::from("acme")]
        );
        // The filled column is taken out by its position: a later additive migration (`pr-title`) adds a column after it.
        for (table, column) in [("contexts", "owner"), ("workflows", "name")] {
            let at = table_columns(&conn, table)
                .unwrap()
                .iter()
                .position(|(c, _)| c == column)
                .unwrap();
            for row in rows_after.get_mut(table).unwrap() {
                let mut values: Vec<Value> = serde_json::from_str(row).unwrap();
                values.remove(at);
                *row = Value::from(values).to_string();
            }
        }
        assert_eq!(rows_after.remove("board_posts"), Some(Vec::new()));
        assert_eq!(rows_after.remove("pr_posts"), Some(Vec::new()));
        // `job-tenancy` made `job_pools`, and it is empty.
        assert_eq!(rows_after.remove("job_pools"), Some(Vec::new()));
        // `20261103T0400-job-addenda` made `job_addenda`, and it is empty.
        assert_eq!(rows_after.remove("job_addenda"), Some(Vec::new()));
        assert_eq!(rows_after.remove("context_reads"), Some(Vec::new())); // `context-reads`: a new empty table
        assert_eq!(rows_after.remove("record_topics"), Some(Vec::new())); // `record-topics`: a new empty table
        // `swr` made the SWR data tables, and they are empty.
        for table in [
            "swr_queries",
            "swr_endpoints",
            "swr_mutations",
            "swr_viewer",
            "swr_repos",
            "swr_pr_detail",
            "swr_deploys",
            "gh_issues",
            "swr_slack_posts",
            "swr_jira_issues",
            "swr_atlassian",
            "swr_models",
            "swr_badges",
        ] {
            assert_eq!(rows_after.remove(table), Some(Vec::new()), "{table}");
        }
        // `curation` made two tables, and they are empty.
        assert_eq!(rows_after.remove("record_retirements"), Some(Vec::new()));
        assert_eq!(rows_after.remove("record_curations"), Some(Vec::new()));
        // `incident` made a table, and it is empty.
        assert_eq!(rows_after.remove("incident"), Some(Vec::new()));
        // `incident-plans` made another, and it is empty too.
        assert_eq!(rows_after.remove("incident_plans"), Some(Vec::new()));
        // `tier-catalog-source` rebuilds tier_catalog EMPTY (a cache, recomputed by the next `cpd tiers`): the one table conversion drops.
        assert_eq!(rows_after.remove("tier_catalog"), Some(Vec::new()));
        assert!(
            rows_before
                .remove("tier_catalog")
                .is_some_and(|r| !r.is_empty()),
            "the legacy tier_catalog row was seeded"
        );
        let (history_before, history_after) = (
            rows_before.remove("history").unwrap(),
            rows_after.remove("history").unwrap(),
        );
        assert_eq!(history_after[..history_before.len()], history_before[..]);
        assert!(
            history_after[history_before.len()..]
                .iter()
                .all(|r| r.contains("\"update\"")),
            "{history_after:?}"
        );
        // The columns the other additive named migrations add come last and are null in every converted row.
        for (table, rows) in rows_after.iter_mut() {
            let width = rows_before
                .get(table)
                .and_then(|r| r.first())
                .map(|r| serde_json::from_str::<Vec<Value>>(r).unwrap().len());
            for row in rows.iter_mut() {
                let mut values: Vec<Value> = serde_json::from_str(row).unwrap();
                let width = width.unwrap_or(values.len());
                if values.len() == width {
                    continue;
                }
                // `job-kinds` added two NOT NULL columns with a default: `jobs.kind` `work` and `prs.shape` `free`; `session-by-user` one, `sessions.by_user` 0. `session-runner` one, `sessions.runner` 'pi'.
                let default = |v: &Value| {
                    v.is_null()
                        || (table == "jobs" && v == "work")
                        || (table == "workflows" && v == "free")
                        || (table == "sessions" && v == 0)
                        || (table == "pull_requests" && v == 0)
                        || (table == "sessions" && v == "pi")
                        || (table == "attempts" && v == "pi")
                };
                assert!(
                    values[width..].iter().all(default),
                    "{table}: an added column is set: {row}"
                );
                values.truncate(width);
                *row = Value::from(values).to_string();
            }
        }
        // `agent-key-backfill` rewrites the name of every Lead that is not a fork (all of them here) to `<context>:<plan>`.
        for row in rows_before.get_mut("sessions").unwrap() {
            let mut values: Vec<Value> = serde_json::from_str(row).unwrap();
            values[5] = Value::from(format!(
                "{}:{}",
                values[2].as_str().unwrap(),
                values[3].as_str().unwrap()
            ));
            *row = Value::from(values).to_string();
        }
        assert_eq!(rows_after, rows_before);
    }

    // Converting twice is a no-op.
    let snapshot = dump(&conn);
    drop(conn);
    drop(Store::open(&home, 5000).unwrap());
    assert_eq!(
        dump(&Connection::open(home.db()).unwrap()),
        snapshot,
        "version {n}: the second open writes nothing"
    );
}

/// Pins the last-version branch, where the `history_tables` count and the `context_reads` rows are asserted.
#[test]
fn converts_keeping_every_row_and_id() {
    converts_keeping_every_row_and_id_at(LEGACY.len());
}

macro_rules! conversion_tests {
    ($($name:ident: $n:literal,)*) => {
        $(
            #[test]
            fn $name() {
                converts_keeping_every_row_and_id_at($n);
            }
        )*
        // A version added to `LEGACY` without its test does not compile.
        const _: () = assert!(LEGACY.len() == [$($n),*].len());
    };
}

conversion_tests! {
    a_version_1_legacy_store_converts_keeping_every_row_and_id: 1,
    a_version_2_legacy_store_converts_keeping_every_row_and_id: 2,
    a_version_3_legacy_store_converts_keeping_every_row_and_id: 3,
    a_version_4_legacy_store_converts_keeping_every_row_and_id: 4,
    a_version_5_legacy_store_converts_keeping_every_row_and_id: 5,
    a_version_6_legacy_store_converts_keeping_every_row_and_id: 6,
    a_version_7_legacy_store_converts_keeping_every_row_and_id: 7,
    a_version_8_legacy_store_converts_keeping_every_row_and_id: 8,
    a_version_9_legacy_store_converts_keeping_every_row_and_id: 9,
    a_version_10_legacy_store_converts_keeping_every_row_and_id: 10,
    a_version_11_legacy_store_converts_keeping_every_row_and_id: 11,
    a_version_12_legacy_store_converts_keeping_every_row_and_id: 12,
    a_version_13_legacy_store_converts_keeping_every_row_and_id: 13,
    a_version_14_legacy_store_converts_keeping_every_row_and_id: 14,
}

#[test]
fn a_fresh_store_has_the_pinned_canonical_hash() {
    assert_eq!(fresh_canonical_hash(), SCHEMA_HASH);
}

#[test]
fn a_fresh_store_records_its_legacy_migrations_as_not_converted() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    drop(Store::open(&home, 5000).unwrap());
    let conn = Connection::open(home.db()).unwrap();
    let converted: Vec<i64> = conn
        .prepare("SELECT converted FROM schema_migrations ORDER BY seq")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_>>()
        .unwrap();
    let all = LEGACY.len() + NAMED.len();
    assert_eq!(converted, vec![0; all]);
    let txns: i64 = conn
        .query_row(
            "SELECT count(*) FROM schema_migrations s JOIN txns t ON t.id = s.txn",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        txns, all as i64,
        "every row names the migrating transaction"
    );
}

#[test]
fn a_tampered_legacy_store_is_refused_and_left_as_it_was() {
    let stores = legacy_stores(LEGACY.len());
    for n in [1, 3, LEGACY.len()] {
        for tamper in [
            "DROP TRIGGER contexts_no_delete",
            "CREATE INDEX stray ON contexts(title)",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let home = home(&dir);
            write_store(&home, &stores[n - 1]);
            let conn = connection(&home.db());
            conn.execute_batch(tamper).unwrap();
            let before = (dump(&conn), legacy_schema_hash(&conn).unwrap());
            drop(conn);

            let err = Store::open(&home, 5000)
                .err()
                .unwrap_or_else(|| panic!("version {n}, {tamper}: opened"));
            assert_eq!(
                err.code(),
                ErrorCode::StoreTampered,
                "version {n}, {tamper}: {err:?}"
            );
            assert_eq!(err.details()["version"], n);

            let conn = connection(&home.db());
            let version: i64 = conn
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(version, n as i64);
            assert_eq!(
                (dump(&conn), legacy_schema_hash(&conn).unwrap()),
                before,
                "version {n}, {tamper}: changed"
            );
        }
    }
}

// ---------- the canonical form (§4.3) ----------

#[test]
fn normalizing_deletes_comments_and_collapses_whitespace_outside_quoted_runs() {
    assert_eq!(
        normalize("  a  /* x */ b -- c\n d 'x  --y' [a  b] \"q\"\"  r\" `s  t`  "),
        "a b d 'x  --y' [a  b] \"q\"\"  r\" `s  t`"
    );
    assert_eq!(normalize("a/**/b"), "a b");
    assert_eq!(normalize(""), "");
}

#[test]
fn a_table_is_split_into_sorted_columns_ordered_constraints_and_options() {
    let parts = table_parts(
        "CREATE TABLE \"t\" (\n  \"Zed\" TEXT, -- a, comment (with parens\n  b TEXT DEFAULT 'x,y' CHECK (b IN ('a', 'b')),\n  \
         [a c] INTEGER,\n  \"check\" TEXT,\n  CONSTRAINT k UNIQUE (b, \"Zed\"),\n  check (length(b) > 0)\n) STRICT, WITHOUT ROWID",
    )
    .unwrap();
    let keys: Vec<&str> = parts.columns.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["a c", "b", "check", "zed"]);
    assert_eq!(
        parts.columns[1].1,
        "b TEXT DEFAULT 'x,y' CHECK (b IN ('a', 'b'))"
    );
    assert_eq!(
        parts.constraints,
        ["CONSTRAINT k UNIQUE (b, \"Zed\")", "check (length(b) > 0)"]
    );
    assert_eq!(parts.options, "STRICT, WITHOUT ROWID");
}

#[test]
fn add_column_in_either_order_gives_one_canonical_table() {
    let canonical = |statements: &[&str]| {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT, CHECK (a <> 'x')) STRICT;",
        )
        .unwrap();
        for statement in statements {
            conn.execute_batch(statement).unwrap();
        }
        canonical_entries(&conn).unwrap()
    };
    let (b, c) = (
        "ALTER TABLE t ADD COLUMN b TEXT",
        "ALTER TABLE t ADD COLUMN c TEXT NOT NULL DEFAULT ''",
    );
    assert_eq!(canonical(&[b, c]), canonical(&[c, b]));
    assert_ne!(canonical(&[b]), canonical(&[c]));
}

#[test]
fn a_changed_generated_trigger_or_object_is_a_schema_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    // One fresh store, copied for each case: its file is the whole database once it is closed.
    let template_dir = tempfile::tempdir_in(dir.path()).unwrap();
    let template = home(&template_dir);
    drop(Store::open(&template, 5000).unwrap());
    let fresh = std::fs::read(template.db()).unwrap();
    for (tamper, trigger) in [
        ("DROP TRIGGER plans_no_delete", Some("plans_no_delete")),
        (
            "DROP TRIGGER plans_hist_upd; CREATE TRIGGER plans_hist_upd AFTER UPDATE ON plans BEGIN SELECT 1; END",
            Some("plans_hist_upd"),
        ),
        ("DROP TRIGGER plans_phase", None),
        ("CREATE INDEX stray ON contexts (title)", None),
        (
            "CREATE TRIGGER stray AFTER INSERT ON contexts BEGIN SELECT 1; END",
            None,
        ),
    ] {
        let home = CpdHome::resolve(
            Some(dir.path().join(format!("{}", tamper.len())).into()),
            None,
        )
        .unwrap();
        write_store(&home, &fresh);
        connection(&home.db()).execute_batch(tamper).unwrap();
        let store = Store::open(&home, 5000).unwrap();
        let mismatch = store
            .schema_mismatch()
            .unwrap_or_else(|| panic!("{tamper}: no mismatch"));
        assert_eq!(mismatch.object.as_deref(), trigger, "{tamper}");
        let err = {
            let mut store = store;
            store.write("test", |_| Ok(())).unwrap_err()
        };
        assert_eq!(err.code(), ErrorCode::StoreTampered, "{tamper}");
    }
}

// ---------- named migration `history-changed` (§4.4 "History row shape", sql-F2) ----------

/// A named store as the binaries before `history-changed` left it: every
/// earlier named migration applied, `_hist_upd` with the whole row, and one
/// such `update` row in `history`.
fn store_before_history_changed(home: &CpdHome) -> Vec<Named> {
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-history-changed"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "history-changed is a built-in migration"
    );
    drop(crate::store::tests::open_named(home, &earlier).unwrap());
    let conn = connection(&home.db());
    for (table, no_delete) in history_tables(&conn).unwrap() {
        let (name, sql) = history_triggers(&conn, &table, no_delete, UpdateRow::Whole, false)
            .unwrap()
            .remove(1);
        conn.execute_batch(&format!("DROP TRIGGER {name}; {sql};"))
            .unwrap();
    }
    conn.execute_batch(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         UPDATE contexts SET title = 'Ctx 2' WHERE id = 'ctx';",
    )
    .unwrap();
    earlier
}

#[test]
fn history_changed_keeps_the_whole_update_rows_and_writes_the_changed_columns_from_then_on() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let earlier = store_before_history_changed(&home);
    let history = |conn: &Connection| -> Vec<(String, Option<String>, Option<String>)> {
        let mut stmt = conn
            .prepare("SELECT op, before, after FROM history WHERE tbl = 'contexts' ORDER BY id")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_>>()
            .unwrap()
    };
    let old = history(&connection(&home.db()));
    let whole: Value = serde_json::from_str(old[1].2.as_deref().unwrap()).unwrap();
    assert_eq!(
        (
            old[1].0.as_str(),
            whole["title"].clone(),
            whole["created_on"].clone()
        ),
        ("update", json!("Ctx 2"), json!("m"))
    );

    // This binary applies `history-changed`: the schema check passes and no row is rewritten.
    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(store.schema_mismatch(), None);
    assert!(
        crate::store::tests::applied(&mut store)
            .iter()
            .any(|(name, _, _)| name.ends_with("-history-changed"))
    );
    assert_eq!(history(&connection(&home.db())), old);
    // The tamper report reads both shapes alike (§4.3): nothing to report.
    assert_eq!(store.tamper().unwrap(), Vec::<Value>::new());

    // A new update holds the key and the changed column; applied to the whole row it replays.
    write(
        &mut store,
        "UPDATE contexts SET title = 'Ctx 3' WHERE id = 'ctx'",
    )
    .unwrap();
    let new = history(&connection(&home.db()));
    assert_eq!(new.len(), 3);
    let (before, after): (Value, Value) = (
        serde_json::from_str(new[2].1.as_deref().unwrap()).unwrap(),
        serde_json::from_str(new[2].2.as_deref().unwrap()).unwrap(),
    );
    assert_eq!(
        (before, after.clone()),
        (
            json!({"id": "ctx", "title": "Ctx 2"}),
            json!({"id": "ctx", "title": "Ctx 3"})
        )
    );
    let mut replayed = whole.as_object().unwrap().clone();
    replayed.extend(after.as_object().unwrap().clone());
    // `policy-keys`, which sorts after `history-changed`, added `contexts.owner`: the old whole row predates it,
    // and this Context has no pull request to fill it from.
    replayed.insert("owner".into(), Value::Null);
    // and `delete-tombstone` added `contexts.deleted_at`.
    replayed.insert("deleted_at".into(), Value::Null);
    let conn = connection(&home.db());
    let columns = table_columns(&conn, "contexts").unwrap();
    let pairs: Vec<String> = columns.iter().map(|(c, _)| format!("'{c}', {c}")).collect();
    let row: String = conn
        .query_row(
            &format!("SELECT json_object({}) FROM contexts", pairs.join(", ")),
            [],
            |r| r.get(0),
        )
        .unwrap();
    let row: Value = serde_json::from_str(&row).unwrap();
    assert_eq!(Value::Object(replayed), row);
    assert_eq!(store.tamper().unwrap(), Vec::<Value>::new());

    // `jobs.urgent` (the additive `job-urgent`, which sorts before it) is in the regenerated
    // trigger: an update of `urgent` alone writes the key and `urgent` (O3: re-checked against it).
    write(
        &mut store,
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pl', 'ctx', 'Plan', 'grounding', 'lead', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pl', 'api', '/r/api', 'main', 'waiting', 'cpd/pl/api', cpd_txn(), 't');
         INSERT INTO jobs (plan, workflow, key, title, status, appended, created_txn, created_at)
           VALUES ('pl', 'api', 'j', 'J', 'draft', 1, cpd_txn(), 't');",
    )
    .unwrap();
    write(&mut store, "UPDATE jobs SET urgent = 1 WHERE key = 'j'").unwrap();
    let (before, after): (String, String) = connection(&home.db())
        .query_row("SELECT before, after FROM history WHERE tbl = 'jobs' AND op = 'update' ORDER BY id DESC LIMIT 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    let seq: i64 = connection(&home.db())
        .query_row("SELECT seq FROM jobs WHERE key = 'j'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        (
            serde_json::from_str::<Value>(&before).unwrap(),
            serde_json::from_str::<Value>(&after).unwrap()
        ),
        (
            json!({"seq": seq, "urgent": 0}),
            json!({"seq": seq, "urgent": 1})
        )
    );
    assert_eq!(store.tamper().unwrap(), Vec::<Value>::new());
    drop(store);

    // A binary without it cannot open the store: the migration is breaking (§4.5 A1).
    let err = crate::store::tests::open_named(&home, &earlier)
        .err()
        .unwrap();
    assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
}

// ---------- named migration `waiting-state` (§3.6, hs-d303) ----------

/// Every column of every Session, as JSON, by id.
/// The rows as the later migration `agent-key-backfill` leaves them: the name of a Manager, a Lead that is not a fork
/// and a Worker is its key (`<context>`, `<context>:<plan>`, `<context>:<plan>:<job>`); a fork keeps its name.
fn with_agent_keys(rows: Vec<Value>) -> Vec<Value> {
    rows.into_iter()
        .map(|mut row| {
            let text = |row: &Value, k: &str| row[k].as_str().map(str::to_owned);
            if row["forked_from"].is_null() {
                let context = text(&row, "context").unwrap();
                let key = match text(&row, "role").unwrap().as_str() {
                    "manager" => context,
                    "lead" => format!("{context}:{}", text(&row, "plan").unwrap()),
                    _ => {
                        let attempt = text(&row, "attempt").unwrap();
                        let job = attempt.rsplit_once("-a").unwrap().0;
                        format!("{context}:{}:{job}", text(&row, "plan").unwrap())
                    }
                };
                row["name"] = Value::from(key);
            }
            row
        })
        .collect()
}

fn session_rows(conn: &Connection) -> Vec<Value> {
    let columns = table_columns(conn, "sessions").unwrap();
    let pairs: Vec<String> = columns.iter().map(|(c, _)| format!("'{c}', {c}")).collect();
    let mut stmt = conn
        .prepare(&format!(
            "SELECT json_object({}) FROM sessions ORDER BY id",
            pairs.join(", ")
        ))
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| serde_json::from_str(&r.unwrap()).unwrap())
        .collect()
}

#[test]
fn waiting_state_keeps_every_session_and_lets_waiting_and_blocked_say_why() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-waiting-state"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "waiting-state is a built-in migration"
    );
    drop(crate::store::tests::open_named(&home, &earlier).unwrap());

    // A Manager, a Plan's Lead, a fork, a Worker and a stopped Lead with no pane, in every state this schema has, each
    // with a pane or an archive time here and there, so that no column is null in all of them.
    let conn = connection(&home.db());
    conn.execute_batch(
        "BEGIN;
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pl', 'ctx', 'Plan', 'grounding', 'lead', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pl', 'api', '/r/api', 'main', 'waiting', 'cpd/pl/api', 1, 't');
         INSERT INTO jobs (plan, pr, key, title, status, appended, created_txn, created_at)
           VALUES ('pl', 'api', 'j', 'J', 'draft', 1, 1, 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pl', 'j-a1', 'j', 1, 'S2', 'ladder', 'running', 'cpd/pl--j-a1', '/w', 't');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane, state, state_label, state_at, created_at, archived_at)
           VALUES ('mgr',    'manager', 'ctx', NULL, NULL,   'Manager', '/f/m', NULL,   '/', 'O3', '%1',  'idle',    NULL,      't1', 't', NULL),
                  ('lead',   'lead',    'ctx', 'pl', NULL,   'Lead',    '/f/l', NULL,   '/', 'O3', '%2',  'blocked', 'dialog',  't2', 't', NULL),
                  ('fork',   'lead',    'ctx', 'pl', NULL,   'Fork',    NULL,   'lead', '/', 'O3', NULL,  'working', NULL,      't3', 't', NULL),
                  ('worker', 'worker',  'ctx', 'pl', 'j-a1', 'Worker',  NULL,   NULL,   '/', 'S2', '%3',  'blocked', 'φ: question to the Lead', 't4', 't', NULL),
                  ('old',    'lead',    'ctx', 'pl', NULL,   'Old',     '/f/o', 'lead',   '/', 'O3', NULL,  'stopped', NULL,      't5', 't', 'a');
         COMMIT;",
    )
    .unwrap();
    // `agent-key-backfill`, a later migration, rewrites the names of the rows that are not forks to their keys.
    let before = with_agent_keys(session_rows(&conn));
    let history: i64 = conn
        .query_row(
            "SELECT count(*) FROM history WHERE tbl = 'sessions'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!((before.len(), history), (5, 5));
    drop(conn);

    // This binary rebuilds `sessions`: every row and column as it was, the schema check passes, and the copy writes no history.
    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(store.schema_mismatch(), None);
    assert!(
        crate::store::tests::applied(&mut store)
            .iter()
            .any(|(name, _, _)| name.ends_with("-waiting-state"))
    );
    let conn = connection(&home.db());
    // `asleep_at` is a later migration's column (`session-asleep`), null on every row; so is `by_user` (`session-by-user`), 0, `switch_failed` (`switch-failed`), null, and `working_pr` (`session-working-pr`), null, and `runner` (`session-runner`), 'pi'.
    let after: Vec<Value> = session_rows(&conn)
        .into_iter()
        .map(|mut row| {
            assert_eq!(
                row.as_object_mut().unwrap().remove("asleep_at"),
                Some(Value::Null)
            );
            assert_eq!(
                row.as_object_mut().unwrap().remove("by_user"),
                Some(Value::from(0))
            );
            assert_eq!(
                row.as_object_mut().unwrap().remove("switch_failed"),
                Some(Value::Null)
            );
            assert_eq!(
                row.as_object_mut().unwrap().remove("working_pr"),
                Some(Value::Null)
            );
            assert_eq!(
                row.as_object_mut().unwrap().remove("runner"),
                Some(Value::from("pi"))
            );
            // `host-restarts` (a later migration) adds these two columns.
            assert_eq!(
                row.as_object_mut().unwrap().remove("host_restarts"),
                Some(Value::from(0))
            );
            assert_eq!(
                row.as_object_mut().unwrap().remove("host_window_start"),
                Some(Value::Null)
            );
            row
        })
        .collect();
    assert_eq!(after, before);
    assert_eq!(
        conn.query_row::<i64, _, _>(
            "SELECT count(*) FROM history WHERE tbl = 'sessions'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        history
    );
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get(0))
            .unwrap(),
        0
    );
    let objects = names(
        &conn,
        "SELECT name FROM sqlite_schema WHERE tbl_name = 'sessions' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    );
    for expected in [
        "sessions_plan",
        "sessions_one_manager",
        "sessions_live",
        "sessions_hist_ins",
        "sessions_hist_upd",
        "sessions_no_delete",
    ] {
        assert!(objects.contains(expected), "{expected} in {objects:?}");
    }
    drop(conn);

    // `waiting` is a state, and says why, as `blocked` does; no other state has a label.
    write(&mut store, "UPDATE sessions SET state = 'waiting', state_label = 'φ: Worker question' WHERE id = 'lead'").unwrap();
    write(&mut store, "UPDATE sessions SET state = 'waiting', state_label = 'φ: question to the Lead' WHERE id = 'worker'").unwrap();
    for bad in [
        "UPDATE sessions SET state = 'waiting', state_label = NULL WHERE id = 'fork'",
        "UPDATE sessions SET state = 'blocked', state_label = NULL WHERE id = 'fork'",
        "UPDATE sessions SET state = 'working', state_label = 'x' WHERE id = 'fork'",
        "UPDATE sessions SET state = 'asleep', state_label = NULL WHERE id = 'fork'",
    ] {
        let message = sqlite_message(&refused(&mut store, bad));
        assert!(
            message.starts_with("CHECK constraint failed"),
            "{bad}: {message}"
        );
    }
    assert_eq!(store.tamper().unwrap(), Vec::<Value>::new());
    drop(store);

    // A binary without it cannot open the store: the migration is breaking (§4.5 A1).
    let err = crate::store::tests::open_named(&home, &earlier)
        .err()
        .unwrap();
    assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
}

// ---------- named migration `session-title-length` (hs-d543) ----------

#[test]
fn session_title_length_keeps_every_session_and_title_and_raises_the_limit_to_200() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-session-title-length"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "session-title-length is a built-in migration"
    );
    drop(crate::store::tests::open_named(&home, &earlier).unwrap());

    // A Manager with a title of 60 characters, a Lead with a short one and a Lead with none, one asleep.
    let sixty = "t".repeat(60);
    let conn = connection(&home.db());
    conn.execute_batch(&format!(
        "BEGIN;
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pl', 'ctx', 'Plan', 'grounding', 'lead', 't');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane, state, state_label, state_at, created_at, archived_at, title, titled_at, asleep_at)
           VALUES ('mgr',  'manager', 'ctx', NULL, NULL, 'Manager', '/f/m', NULL, '/', 'O3', '%1', 'idle',    NULL, 't1', 't', NULL, '{sixty}', 'u1', NULL),
                  ('lead', 'lead',    'ctx', 'pl', NULL, 'Lead',    '/f/l', NULL, '/', 'O3', '%2', 'blocked', 'dialog', 't2', 't', NULL, 'Short', 'u2', 'a'),
                  ('bare', 'lead',    'ctx', 'pl', NULL, 'Bare',    NULL,   'lead', '/', 'O3', NULL, 'stopped', NULL, 't3', 't', 'a', NULL, NULL, NULL);
         COMMIT;"
    ))
    .unwrap();
    // `agent-key-backfill`, a later migration, rewrites the names of the rows that are not forks to their keys.
    let before = with_agent_keys(session_rows(&conn));
    let history: i64 = conn
        .query_row(
            "SELECT count(*) FROM history WHERE tbl = 'sessions'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!((before.len(), history), (3, 3));
    drop(conn);

    // The rebuild keeps every row and column and writes no history; the schema check passes and the triggers are back.
    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(store.schema_mismatch(), None);
    assert!(
        crate::store::tests::applied(&mut store)
            .iter()
            .any(|(name, _, _)| name.ends_with("-session-title-length"))
    );
    let conn = connection(&home.db());
    // `by_user` is a later migration's column (`session-by-user`), 0 on every row; `switch_failed` (`switch-failed`), null; `working_pr` (`session-working-pr`), null; `runner` (`session-runner`), 'pi'.
    let after: Vec<Value> = session_rows(&conn)
        .into_iter()
        .map(|mut row| {
            assert_eq!(
                row.as_object_mut().unwrap().remove("by_user"),
                Some(Value::from(0))
            );
            assert_eq!(
                row.as_object_mut().unwrap().remove("switch_failed"),
                Some(Value::Null)
            );
            assert_eq!(
                row.as_object_mut().unwrap().remove("working_pr"),
                Some(Value::Null)
            );
            assert_eq!(
                row.as_object_mut().unwrap().remove("runner"),
                Some(Value::from("pi"))
            );
            // `host-restarts` (a later migration) adds these two columns.
            assert_eq!(
                row.as_object_mut().unwrap().remove("host_restarts"),
                Some(Value::from(0))
            );
            assert_eq!(
                row.as_object_mut().unwrap().remove("host_window_start"),
                Some(Value::Null)
            );
            row
        })
        .collect();
    assert_eq!(after, before);
    assert_eq!(
        conn.query_row::<i64, _, _>(
            "SELECT count(*) FROM history WHERE tbl = 'sessions'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        history
    );
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get(0))
            .unwrap(),
        0
    );
    let objects = names(
        &conn,
        "SELECT name FROM sqlite_schema WHERE tbl_name = 'sessions' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    );
    for expected in [
        "sessions_plan",
        "sessions_one_manager",
        "sessions_live",
        "sessions_hist_ins",
        "sessions_hist_upd",
        "sessions_no_delete",
    ] {
        assert!(objects.contains(expected), "{expected} in {objects:?}");
    }
    drop(conn);

    // A title is 1–200 characters now; 201 and the empty one are refused.
    write(
        &mut store,
        &format!(
            "UPDATE sessions SET title = '{}' WHERE id = 'lead'",
            "t".repeat(200)
        ),
    )
    .unwrap();
    for bad in [
        format!(
            "UPDATE sessions SET title = '{}' WHERE id = 'lead'",
            "t".repeat(201)
        ),
        "UPDATE sessions SET title = '' WHERE id = 'lead'".to_owned(),
    ] {
        let message = sqlite_message(&refused(&mut store, &bad));
        assert!(
            message.starts_with("CHECK constraint failed"),
            "{bad}: {message}"
        );
    }
    assert_eq!(store.tamper().unwrap(), Vec::<Value>::new());
    drop(store);

    // A binary without it cannot open the store: the migration is breaking (§4.5 A1).
    let err = crate::store::tests::open_named(&home, &earlier)
        .err()
        .unwrap();
    assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
}

// ---------- migration 6: pull_requests and prs.pull_request, on a version 5 database with polled Workflows ----------

fn names(conn: &Connection, sql: &str) -> std::collections::BTreeSet<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

/// A database at version 5, holding
/// - `op1`: its Workflow open, `pr_title` and the review and the checks polled;
/// - `op2`: its Workflow open, never polled (`pr_title` null);
/// - `mg`: its Workflow merged;
/// - `wt`: its Workflow waiting.
fn version_5_four_prs(home: &CpdHome) {
    let workflow = |plan: &str, key: &str, status: &str, more: &str, values: &str| {
        let (worktree, started) = if status == "waiting" {
            ("NULL", "NULL")
        } else {
            ("'/wt'", "'2026-01-02T00:00:00.000Z'")
        };
        format!(
            "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('{plan}', 'ctx', 'Plan {plan}', '{phase}', 'l-{plan}', 't', {locked}, {done});
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-{plan}', 'lead', 'ctx', '{plan}', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at {more})
               VALUES ('{plan}', '{key}', '/r/{key}', 'main', '{status}', 'cpd/{plan}/{key}', {worktree}, {started}, 1, 't' {values});",
            phase = match status {
                "waiting" => "plan",
                "merged" => "done",
                _ => "pr",
            },
            locked = if status == "waiting" { "NULL" } else { "'2026-01-02T00:00:00.000Z'" },
            done = if status == "merged" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" },
        )
    };
    let sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         {}{}{}{}",
        workflow(
            "op1",
            "api",
            "open",
            ", pr_number, pr_url, pr_opened_at, pr_polled_at, pr_title, pr_review_decision, pr_checks",
            ", 7, 'https://github.com/acme/api/pull/7', '2026-01-05T00:00:00.000Z', '2026-01-06T00:00:00.000Z', 'Add api', 'approved', 'passed'",
        ),
        workflow(
            "op2",
            "web",
            "open",
            ", pr_number, pr_url, pr_opened_at",
            ", 8, 'https://github.com/acme/web/pull/8', '2026-01-05T00:00:01.000Z'",
        ),
        workflow(
            "mg",
            "docs",
            "merged",
            ", pr_number, pr_url, pr_opened_at, merged_at",
            ", 9, 'https://github.com/acme/docs/pull/9', '2026-01-05T00:00:02.000Z', '2026-01-08T00:00:00.000Z'",
        ),
        workflow("wt", "cli", "waiting", "", ""),
    );
    database_at(home, 5, &sql);
}

/// A `prs` row: plan, status, pull_request, pr_number, merged_at.
type PrsRow = (String, String, Option<i64>, Option<i64>, Option<String>);

#[test]
fn a_version_5_database_with_polled_prs_migrates_to_6_with_the_back_filled_rows_and_the_link() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_5_four_prs(&home);
    let store = Store::open(&home, 5000).unwrap();
    // Its schema is exactly that of a fresh database at version 6.
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    drop(store);
    let conn = Connection::open(home.db()).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, VERSION);
    let violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);

    // One row per Workflow with a number, in Plan and key order; the title is the polled one, else the Plan's.
    let row = |r: &rusqlite::Row| -> rusqlite::Result<Value> {
        Ok(serde_json::json!([
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, Option<String>>(10)?,
            r.get::<_, String>(11)?
        ]))
    };
    let rows: Vec<Value> = conn
        .prepare("SELECT id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at FROM pull_requests ORDER BY id")
        .unwrap()
        .query_map([], row)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        rows,
        vec![
            serde_json::json!([
                1,
                "acme/docs",
                9,
                "https://github.com/acme/docs/pull/9",
                "Plan mg",
                0,
                "merged",
                "none",
                "none",
                "2026-01-05T00:00:02.000Z",
                "2026-01-08T00:00:00.000Z",
                "2026-01-05T00:00:02.000Z"
            ]),
            serde_json::json!([
                2,
                "acme/api",
                7,
                "https://github.com/acme/api/pull/7",
                "Add api",
                0,
                "open",
                "approved",
                "passed",
                "2026-01-05T00:00:00.000Z",
                null,
                "2026-01-05T00:00:00.000Z"
            ]),
            serde_json::json!([
                3,
                "acme/web",
                8,
                "https://github.com/acme/web/pull/8",
                "Plan op2",
                0,
                "open",
                "none",
                "none",
                "2026-01-05T00:00:01.000Z",
                null,
                "2026-01-05T00:00:01.000Z"
            ]),
        ]
    );

    // `pull_request` is set exactly for the open and the merged Workflow; every other column is kept.
    let workflows: Vec<PrsRow> = conn
        .prepare(
            "SELECT plan, status, pull_request, pr_number, merged_at FROM workflows ORDER BY plan",
        )
        .unwrap()
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let some = |s: &str| Some(s.to_owned());
    assert_eq!(
        workflows,
        vec![
            (
                "mg".into(),
                "merged".into(),
                Some(1),
                Some(9),
                some("2026-01-08T00:00:00.000Z")
            ),
            ("op1".into(), "open".into(), Some(2), Some(7), None),
            ("op2".into(), "open".into(), Some(3), Some(8), None),
            ("wt".into(), "waiting".into(), None, None, None),
        ]
    );
    let columns = names(&conn, "SELECT name FROM pragma_table_info('workflows')");
    for gone in [
        "pr_seen",
        "pr_polled_at",
        "pr_title",
        "pr_review_decision",
        "pr_checks",
    ] {
        assert!(!columns.contains(gone), "{gone}");
    }
    for kept in ["pr_number", "pr_url", "pr_opened_at", "pull_request"] {
        assert!(columns.contains(kept), "{kept}");
    }

    // The history triggers of both tables exist; the back-fill wrote an `insert` row for each pull request.
    let triggers = names(
        &conn,
        "SELECT name FROM sqlite_schema WHERE type = 'trigger' AND tbl_name IN ('pull_requests', 'workflows')",
    );
    for trigger in [
        "pull_requests_hist_ins",
        "pull_requests_hist_upd",
        "pull_requests_hist_del",
        "pull_requests_owned",
        "workflows_hist_ins",
        "workflows_hist_upd",
    ] {
        assert!(triggers.contains(trigger), "{trigger}: {triggers:?}");
    }
    assert!(!triggers.contains("pull_requests_no_delete"));
    let inserted: Vec<String> = conn
        .prepare("SELECT pk FROM history WHERE tbl = 'pull_requests' AND op = 'insert' ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(inserted, ["[1]", "[2]", "[3]"]);
}

#[test]
fn an_owned_pull_request_cannot_be_deleted_and_an_unowned_one_can_with_a_history_row() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_5_four_prs(&home);
    let mut store = Store::open(&home, 5000).unwrap();
    // Owned by the open Workflow of `op1`.
    let err = refused(&mut store, "DELETE FROM pull_requests WHERE number = 7");
    assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
    assert!(sqlite_message(&err).starts_with("append_only"), "{err:?}");
    // Owned by the merged Workflow of `mg`.
    assert!(
        sqlite_message(&refused(
            &mut store,
            "DELETE FROM pull_requests WHERE number = 9"
        ))
        .starts_with("append_only")
    );
    // One no Workflow points to: a pull request of the User's that no Plan owns.
    write(
        &mut store,
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
         VALUES ('acme/other', 3, 'https://github.com/acme/other/pull/3', 'Other', 1, 'open', 'none', 'pending', 't', 't')",
    )
    .unwrap();
    let id: i64 = store
        .read(|c| {
            Ok(
                c.query_row("SELECT id FROM pull_requests WHERE number = 3", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    write(&mut store, "DELETE FROM pull_requests WHERE number = 3").unwrap();
    let (op, pk, before, after): (String, String, String, Option<String>) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT op, pk, before, after FROM history WHERE tbl = 'pull_requests' ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        (op.as_str(), pk, after),
        ("delete", format!("[{id}]"), None)
    );
    let before: Value = serde_json::from_str(&before).unwrap();
    assert_eq!(
        (
            &before["repo"],
            &before["number"],
            &before["draft"],
            &before["checks"]
        ),
        (
            &serde_json::json!("acme/other"),
            &serde_json::json!(3),
            &serde_json::json!(1),
            &serde_json::json!("pending")
        )
    );
    let left: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM pull_requests", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(left, 3);
}

// ---------- migration 8 ----------

/// A version-7 database with a Plan in each phase and `plan-phase` Events that say `pr`.
fn version_7_with_a_plan_in_pr(home: &CpdHome) {
    let plan = |id: &str, phase: &str, locked: &str, done: &str| {
        format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-{id}', 'lead', 'ctx', '{id}', 'lead', '/repo', 'O3', 'idle', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('{id}', 'ctx', 'Plan {id}', '{phase}', 'l-{id}', '{T}', {locked}, {done});"
        )
    };
    let event = |plan: &str, from: &str, to: &str| {
        format!(
            "INSERT INTO events (txn, at, kind, context, plan, text, data) VALUES
               (1, '{T}', 'plan-phase', 'ctx', '{plan}', 'Plan {plan} is in phase pr. Next: cpd pr poll',
                '{{\"from\":\"{from}\",\"to\":\"{to}\",\"prs\":[{{\"key\":\"pr\"}}]}}');"
        )
    };
    let locked = format!("'{T}'");
    database_at(
        home,
        7,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
             {} {} {} {} {}
             {} {} {} {}",
            plan("gr", "grounding", "NULL", "NULL"),
            plan("pl", "plan", "NULL", "NULL"),
            plan("di", "dispatch", &locked, "NULL"),
            plan("pr", "pr", &locked, "NULL"),
            plan("dn", "done", &locked, &locked),
            event("di", "plan", "dispatch"),
            event("pr", "dispatch", "pr"),
            event("pr", "pr", "dispatch"),
            event("dn", "pr", "done"),
        ),
    );
}

#[test]
fn a_version_7_database_migrates_the_phase_pr_to_deploy_everywhere_and_keeps_the_event_text() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_7_with_a_plan_in_pr(&home);
    let events_before = {
        let conn = connection(&home.db());
        conn.prepare("SELECT id, kind, plan, text FROM events ORDER BY id")
            .unwrap()
            .query_map([], |r| {
                Ok(serde_json::json!([
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?
                ]))
            })
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap()
    };

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, phase FROM plans ORDER BY created_at, id"
        ),
        vec![
            serde_json::json!(["di", "dispatch"]),
            serde_json::json!(["dn", "done"]),
            serde_json::json!(["gr", "grounding"]),
            serde_json::json!(["pl", "plan"]),
            serde_json::json!(["pr", "deploy"]),
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT plan, json_extract(data, '$.from'), json_extract(data, '$.to') FROM events ORDER BY id"
        ),
        vec![
            serde_json::json!(["di", "plan", "dispatch"]),
            serde_json::json!(["pr", "dispatch", "deploy"]),
            serde_json::json!(["pr", "deploy", "dispatch"]),
            serde_json::json!(["dn", "deploy", "done"]),
        ]
    );
    // Only `from` and `to` change: the rest of `data` and the text stay.
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM events WHERE json_extract(data, '$.prs[0].key') = 'pr'"
        ),
        4
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, kind, plan, text FROM events ORDER BY id"
        ),
        events_before
    );
    assert_eq!(
        count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"),
        0
    );
    // The guards are back; the new phase graph is in force.
    assert_eq!(
        sqlite_message(&refused(&mut store, "UPDATE events SET text = 'x'")),
        "append_only: events"
    );
    assert_eq!(
        sqlite_message(&refused(&mut store, "DELETE FROM events")),
        "append_only: events"
    );
    write(
        &mut store,
        "UPDATE plans SET phase = 'done', done_at = '2026-01-09T00:00:00.000Z' WHERE id = 'pr'",
    )
    .unwrap();
    assert_eq!(
        sqlite_message(&refused(
            &mut store,
            "UPDATE plans SET phase = 'pr' WHERE id = 'di'"
        )),
        "illegal_transition"
    );
    assert_eq!(
        sqlite_message(&refused(
            &mut store,
            "UPDATE plans SET phase = 'deploy' WHERE id = 'gr'"
        )),
        "illegal_transition"
    );
}

// ---------- migration 9 ----------

#[test]
fn a_version_8_database_migrates_to_9_keeping_every_event_and_accepts_session_message() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    let events: String = [
        "context-created", "context-record", "context-grounded", "plan-created", "plan-phase", "plan-review", "plan-done",
        "job-added", "job-edited", "job-merged", "job-failed", "job-retried", "job-cancel-requested", "job-cancelled",
        "attempt-started", "attempt-status", "attempt-retry", "attempt-nudge", "all-merged", "pr-feedback", "pr-added",
        "pr-edited", "pr-cancelled", "pr-started", "pr-opened", "pr-merged", "escalation", "escalation-answered",
        "escalation-expired", "session-registered", "session-state", "session-moved", "session-renamed", "context-renamed",
        "plan-renamed", "context-archived", "plan-archived", "session-archived", "watch-started", "watch-stopped", "error",
    ]
    .iter()
    .map(|kind| format!("INSERT INTO events (txn, at, kind, text, data) VALUES (1, '{T}', '{kind}', 'text of {kind}', '{{\"k\":1}}');"))
    .collect();
    database_at(&home, 8, &events);
    let before = {
        let conn = connection(&home.db());
        conn.prepare("SELECT id, txn, at, kind, context, plan, job, attempt, session, text, data FROM events ORDER BY id")
            .unwrap()
            .query_map([], |r| (0..11).map(|i| r.get::<_, rusqlite::types::Value>(i).map(|v| format!("{v:?}"))).collect::<Result<Vec<_>>>())
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(before.len(), 41);

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    let after = rows(
        &mut store,
        "SELECT id, txn, at, kind, context, plan, job, attempt, session, text, data FROM events ORDER BY id",
    );
    assert_eq!(after.len(), before.len());
    for (i, row) in after.iter().enumerate() {
        assert_eq!(
            (row[0].as_i64(), row[1].as_i64()),
            (Some(i as i64 + 1), Some(1))
        );
        assert_eq!(row[9], format!("text of {}", row[3].as_str().unwrap()));
    }
    assert_eq!(
        (after[0][3].as_str(), after[40][3].as_str()),
        (Some("context-created"), Some("error"))
    );
    assert_eq!(
        count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"),
        0
    );
    // The sequence never goes back, and the new kind is accepted; an unknown one is not.
    write(&mut store, "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), 't', 'session-message', 'x', '{\"from\":null,\"text\":\"hi\"}')")
        .unwrap();
    assert_eq!(count(&mut store, "SELECT max(id) FROM events"), 42);
    assert!(sqlite_message(&refused(&mut store, "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), 't', 'session-mail', 'x', '{}')"))
        .contains("FOREIGN KEY constraint failed"), "after kind-tables a kind is a foreign key");
    // The indexes and guards are back.
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM sqlite_schema WHERE tbl_name = 'events' AND type = 'index' AND name LIKE 'events_%'"
        ),
        3
    );
    assert_eq!(
        sqlite_message(&refused(&mut store, "UPDATE events SET text = 'x'")),
        "append_only: events"
    );
    assert_eq!(
        sqlite_message(&refused(&mut store, "DELETE FROM events")),
        "append_only: events"
    );
    assert_eq!(
        sqlite_message(&refused(
            &mut store,
            "INSERT INTO events (txn, at, kind, text, data) VALUES (1, 't', 'error', 'x', '{}')"
        )),
        "events.txn must be cpd_txn()"
    );
}

// ---------- migration 10 ----------

/// A version-9 database: Context `ctx` with a grounded Lead (a Lead with no Plan),
/// a Plan with its Lead, a second Context with no grounded Lead, records of every
/// kind, and Events of kind `context-grounded`.
fn version_9_with_a_grounded_lead(home: &CpdHome) {
    database_at(
        home,
        9,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1'), ('two', 'Two', '{T}', 'm1');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, tmux_pane, forked_from)
               VALUES ('l-g', 'lead', 'ctx', NULL, 'grounded', '/r', 'O3', 'idle', '{T}', '2026-01-01T00:00:01.000Z', '%3', 'l-a'),
                      ('l-a', 'lead', 'ctx', 'aa', 'lead a', '/r', 'O3', 'idle', '{T}', '{T}', NULL, NULL);
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
               VALUES ('aa', 'ctx', 'Plan A', 'grounding', 'l-a', '{T}');
             UPDATE contexts SET lead_session = 'l-g', grounded_seq = 2 WHERE id = 'ctx';
             INSERT INTO context_records (context, id, kind, at, machine, text, supersedes, question, plan_id, plan_machine, plan_jira) VALUES
               ('ctx', 'm1-d1', 'decision', '{T}', 'm1', 'D', NULL, NULL, NULL, NULL, NULL),
               ('ctx', 'm1-n1', 'note', '{T}', 'm1', 'N', NULL, NULL, NULL, NULL, NULL),
               ('ctx', 'm1-p1', 'plan', '{T}', 'm1', 'Plan A', NULL, NULL, 'aa', 'm1', 'ABC-1');
             INSERT INTO events (txn, at, kind, context, session, text, data) VALUES
               (1, '{T}', 'context-grounded', 'ctx', 'l-g', 'Context ctx has a grounded Lead.', '{{\"seq\":2,\"forked_from\":\"l-a\"}}');"
        ),
    );
}

#[test]
fn a_version_9_database_migrates_to_10_archiving_the_grounded_lead_and_dropping_its_columns() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    version_9_with_a_grounded_lead(&home);
    let records = "SELECT seq, context, id, kind, at, machine, text, supersedes, question, plan_id, plan_machine, plan_jira FROM context_records ORDER BY seq";
    let events = "SELECT id, txn, kind, context, plan, session, text, data FROM events WHERE kind <> 'session-archived' ORDER BY id";
    let dump = |sql: &str| -> Vec<Vec<String>> {
        let conn = connection(&home.db());
        let mut stmt = conn.prepare(sql).unwrap();
        let n = stmt.column_count();
        stmt.query_map([], |r| {
            (0..n)
                .map(|i| {
                    r.get::<_, rusqlite::types::Value>(i)
                        .map(|v| format!("{v:?}"))
                })
                .collect()
        })
        .unwrap()
        .collect::<Result<_>>()
        .unwrap()
    };
    let (records_before, events_before) = (dump(records), dump(events));

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(count(&mut store, "PRAGMA user_version"), VERSION);
    drop(store);
    assert_eq!(dump(records), records_before);
    assert_eq!(dump(events), events_before);
    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(
        count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"),
        0
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM pragma_table_info('contexts') WHERE name IN ('lead_session', 'grounded_seq')"
        ),
        0
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, title, archived_at FROM contexts ORDER BY id"
        )
        .len(),
        2
    );

    // The grounded Lead is archived, once, with its Event; the Plan's Lead is not.
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, role, plan, archived_at IS NOT NULL, tmux_pane FROM sessions ORDER BY id"
        ),
        vec![
            serde_json::json!(["l-a", "lead", "aa", 0, Value::Null]),
            serde_json::json!(["l-g", "lead", Value::Null, 1, "%3"])
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT kind, context, plan, session, text, data FROM events WHERE kind = 'session-archived'"
        ),
        vec![serde_json::json!([
            "session-archived",
            "ctx",
            Value::Null,
            "l-g",
            "Session grounded archived.",
            r#"{"archived":true}"#
        ])]
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM history WHERE tbl = 'sessions' AND op = 'update'"
        ),
        1
    );

    // A `brief` record needs its Plan reference; positions continue after the old ones.
    let brief = |plan: &str| {
        format!(
            "INSERT INTO context_records (context, id, kind, at, machine, text, plan_id, plan_machine) VALUES ('ctx', 'm1-b1', 'brief', 't', 'm1', 'B', {plan})"
        )
    };
    assert!(
        sqlite_message(&refused(&mut store, &brief("NULL, NULL")))
            .contains("CHECK constraint failed")
    );
    write(&mut store, &brief("'aa', 'm1'")).unwrap();
    assert_eq!(
        count(
            &mut store,
            "SELECT seq FROM context_records WHERE id = 'm1-b1'"
        ),
        4
    );
    assert_eq!(
        sqlite_message(&refused(
            &mut store,
            "UPDATE context_records SET text = 'x'"
        )),
        "append_only: context_records"
    );

    // One live Manager per Context, none with a Plan, and a second after the first is archived.
    let manager = |id: &str, context: &str, plan: &str| {
        format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('{id}', 'manager', '{context}', {plan}, 'm', '/', 'O4', 'stopped', 't', 't')"
        )
    };
    write(&mut store, &manager("m-1", "ctx", "NULL")).unwrap();
    let second = refused(&mut store, &manager("m-2", "ctx", "NULL"));
    assert!(
        sqlite_message(&second).contains("UNIQUE constraint failed"),
        "{second:?}"
    );
    assert!(
        sqlite_message(&refused(&mut store, &manager("m-3", "two", "'aa'")))
            .contains("CHECK constraint failed")
    );
    write(&mut store, &manager("m-4", "two", "NULL")).unwrap();
    write(
        &mut store,
        "UPDATE sessions SET archived_at = 't' WHERE id = 'm-1'",
    )
    .unwrap();
    write(&mut store, &manager("m-2", "ctx", "NULL")).unwrap();
    assert_eq!(
        sqlite_message(&refused(&mut store, "DELETE FROM sessions")),
        "append_only: sessions"
    );
    // A Worker still needs an Attempt, a Lead none.
    assert!(sqlite_message(&refused(
        &mut store,
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('w', 'worker', 'ctx', 'aa', 'w', '/', 'S3', 'stopped', 't', 't')"
    ))
    .contains("CHECK constraint failed"));
}

#[test]
fn a_version_9_database_with_no_grounded_lead_migrates_to_10_with_no_new_event() {
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    database_at(
        &home,
        9,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');"
        ),
    );
    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(count(&mut store, "SELECT count(*) FROM events"), 0);
    assert!(store.schema_mismatch().is_none());
}

// ---------- named migration `stacked-prs` (§4.5; §2.2 "The base Workflow", §3.8 "Stacked Workflows") ----------

/// A locked Plan `pl` of every kind of Workflow, and a Plan `other` in grounding with one; the Workflow `b` needs `c`.
/// `b` and `e` are stacked on `a` (their `base` is its Lead Branch); `ms` and `xs` are too, but `merged` and `cancelled`.
const STACK: &str = "
  INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
  INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
    VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'idle', 't', 't'),
           ('lead2', 'lead', 'ctx', 'other', 'Lead', '/', 'O3', 'idle', 't', 't');
  INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
    VALUES ('pl', 'ctx', 'Plan', 'dispatch', 'lead', 't', 't');
  INSERT INTO plans (id, context, title, phase, lead_session, created_at)
    VALUES ('other', 'ctx', 'Other', 'grounding', 'lead2', 't');
  INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
    VALUES ('acme/r', 7, 'u7', 'T', 0, 'open', 'none', 'none', 't', NULL, 't'),
           ('acme/r', 8, 'u8', 'T', 0, 'merged', 'none', 'none', 't', 't', 't'),
           ('acme/r', 9, 'u9', 'T', 0, 'merged', 'none', 'none', 't', 't', 't');
  INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at) VALUES
    ('pl', 'a',  '/r', 'main',     'waiting',  'cpd/pl/a',  NULL,   NULL, 1, 't'),
    ('pl', 'b',  '/r', 'cpd/pl/a', 'waiting',  'cpd/pl/b',  NULL,   NULL, 1, 't'),
    ('pl', 'c',  '/r', 'main',     'waiting',  'cpd/pl/c',  NULL,   NULL, 1, 't'),
    ('pl', 'd',  '/r', 'main',     'dispatch', 'cpd/pl/d',  '/w/d', 't',  1, 't'),
    ('pl', 'e',  '/r', 'cpd/pl/a', 'dispatch', 'cpd/pl/e',  '/w/e', 't',  1, 't'),
    ('pl', 'xs', '/r', 'cpd/pl/a', 'cancelled', 'cpd/pl/xs', NULL,  NULL, 1, 't'),
    ('pl', 'x',  '/r', 'main',     'cancelled', 'cpd/pl/x',  NULL,  NULL, 1, 't'),
    ('other', 'o', '/r2', 'main',   'waiting',  'cpd/other/o', NULL, NULL, 1, 't');
  INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, pr_number, pr_url, pr_opened_at,
                   pull_request, merged_at, created_txn, created_at) VALUES
    ('pl', 'op', '/r', 'main',     'open',   'cpd/pl/op', '/w/op', 't', 7, 'u7', 't', 1, NULL, 1, 't'),
    ('pl', 'm',  '/r', 'main',     'merged', 'cpd/pl/m',  '/w/m',  't', 8, 'u8', 't', 2, 't',  1, 't'),
    ('pl', 'ms', '/r', 'cpd/pl/a', 'merged', 'cpd/pl/ms', '/w/ms', 't', 9, 'u9', 't', 3, 't',  1, 't');
";

/// A named store as the binaries before `stacked-prs` left it, holding [`STACK`] and one need.
fn store_before_stacked_prs(home: &CpdHome) -> Vec<Named> {
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-stacked-prs"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "stacked-prs is a built-in migration"
    );
    drop(crate::store::tests::open_named(home, &earlier).unwrap());
    let conn = connection(&home.db());
    // The Plans and their Leads point at each other: the fixture is written with foreign keys off.
    conn.execute_batch(&format!("PRAGMA foreign_keys = OFF; {STACK} INSERT INTO pr_needs (plan, pr, needs) VALUES ('pl', 'b', 'c');")).unwrap();
    earlier
}

#[test]
fn stacked_prs_is_breaking_after_the_names_before_it() {
    let (index, migration) = NAMED
        .iter()
        .enumerate()
        .find(|(_, n)| n.name.ends_with("-stacked-prs"))
        .unwrap();
    assert_eq!(migration.compat, Compat::Breaking);
    let after = LEGACY.len() + index;
    assert_eq!(
        migration.sql.lines().next().unwrap(),
        format!("-- cpd-migration: breaking after {after}")
    );
}

#[test]
fn stacked_prs_keeps_every_pr_and_need() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let earlier = store_before_stacked_prs(&home);
    let before = dump(&connection(&home.db()));

    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(store.schema_mismatch(), None);
    assert!(
        crate::store::tests::applied(&mut store)
            .iter()
            .any(|(name, _, _)| name.ends_with("-stacked-prs"))
    );
    let violations: i64 = connection(&home.db())
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);

    // Every `pr_needs` row is kept; every `prs` row is as it was.
    assert_eq!(
        rows(
            &mut store,
            "SELECT plan, workflow, needs FROM workflow_needs"
        ),
        vec![serde_json::json!(["pl", "b", "c"])]
    );
    let after = dump(&connection(&home.db()));
    // `job-kinds` added `prs.shape`, `free` in every row, and `pr-adopt` `prs.head_ref` after it, null in every row, `pr-description` `prs.description` after them, null in every row.
    let without_shape: Vec<String> = after["workflows"]
        .iter()
        .map(|row| {
            let mut cells: Vec<serde_json::Value> = serde_json::from_str(row).unwrap();
            assert_eq!(cells.pop(), Some(serde_json::Value::Null));
            assert_eq!(cells.pop(), Some(serde_json::Value::Null));
            assert_eq!(cells.pop(), Some(serde_json::Value::Null));
            assert_eq!(cells.pop(), Some(serde_json::json!("free")));
            serde_json::Value::from(cells).to_string()
        })
        .collect();
    assert_eq!(without_shape, before["prs"]);
    assert_eq!(after["workflow_needs"].len(), 1);

    // `local-needs`: a need names a Workflow of its own Plan; one naming no Workflow is refused.
    write(
        &mut store,
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('other', 'o2', '/r2', 'main', 'waiting', 'cpd/other/o2', cpd_txn(), 't');
         INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('other', 'o2', 'o')",
    )
    .unwrap();
    let err = refused(
        &mut store,
        "INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('other', 'o2', 'nope')",
    );
    assert!(format!("{err:?}").contains("FOREIGN KEY"), "{err:?}");
    // A Workflow does not need itself.
    let err = refused(
        &mut store,
        "INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('other', 'o', 'o')",
    );
    assert!(format!("{err:?}").contains("CHECK"), "{err:?}");
    assert_eq!(
        rows(
            &mut store,
            "SELECT pk, after FROM history WHERE tbl = 'workflow_needs' ORDER BY id DESC LIMIT 1"
        ),
        vec![serde_json::json!([
            r#"["other","o2","o"]"#,
            r#"{"plan":"other","workflow":"o2","needs":"o"}"#
        ])]
    );

    // pr_needs_locked: after the lock only a Workflow's own insert may add needs; the Plan `pl` is locked.
    let err = refused(
        &mut store,
        "INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pl', 'a', 'c')",
    );
    assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{err:?}");
    // pr_needs_no_delete: and its history row for a delete holds the key.
    let err = refused(&mut store, "DELETE FROM workflow_needs WHERE plan = 'pl'");
    assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{err:?}");
    write(
        &mut store,
        "DELETE FROM workflow_needs WHERE plan = 'other'",
    )
    .unwrap();
    assert_eq!(
        rows(
            &mut store,
            "SELECT pk, op, before FROM history WHERE tbl = 'workflow_needs' ORDER BY id DESC LIMIT 1"
        ),
        vec![serde_json::json!([
            r#"["other","o2","o"]"#,
            "delete",
            r#"{"plan":"other","workflow":"o2","needs":"o"}"#
        ])]
    );
    drop(store);

    // A binary without it cannot open the store: the migration is breaking (§4.5 A1).
    let err = crate::store::tests::open_named(&home, &earlier)
        .err()
        .unwrap();
    assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
}

#[test]
fn stacked_prs_lets_base_follow_github_on_an_open_or_stacked_pr_only() {
    let (_dir, home) = crate::store::tests::tmp_home();
    store_before_stacked_prs(&home);
    // Before it, `base` is Locked for every status after the lock.
    for key in ["a", "b", "c", "d", "e", "x", "xs", "op", "m", "ms"] {
        let err = connection(&home.db())
            .execute_batch(&format!(
                "UPDATE prs SET base = 'develop' WHERE plan = 'pl' AND key = '{key}'"
            ))
            .unwrap_err();
        assert!(err.to_string().contains("pr_locked"), "{key}: {err}");
    }

    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(store.schema_mismatch(), None);
    // After it, a `waiting`, `dispatch` or `open` Workflow's `base` follows (`workflow-base-retarget` dropped the lead-branch condition); `pl` is locked.
    for key in ["x", "m"] {
        let err = refused(
            &mut store,
            &format!("UPDATE workflows SET base = 'develop' WHERE plan = 'pl' AND key = '{key}'"),
        );
        assert_eq!(err.code(), ErrorCode::WorkflowLocked, "{key}: {err:?}");
    }
    for key in ["xs", "ms"] {
        let err = refused(
            &mut store,
            &format!("UPDATE workflows SET base = 'main' WHERE plan = 'pl' AND key = '{key}'"),
        );
        assert_eq!(
            err.code(),
            ErrorCode::WorkflowLocked,
            "a {key} Workflow is not followed: {err:?}"
        );
    }
    // Nothing but `base` moves: not the identity, the repo or the Lead Branch, nor with the base.
    for set in [
        "key = 'z'",
        "repo = '/x'",
        "lead_branch = 'cpd/pl/z'",
        "plan = 'other'",
        "base = 'main', repo = '/x'",
        "base = 'main', lead_branch = 'cpd/pl/z'",
    ] {
        for key in ["b", "e", "op"] {
            let err = refused(
                &mut store,
                &format!("UPDATE workflows SET {set} WHERE plan = 'pl' AND key = '{key}'"),
            );
            assert_eq!(
                err.code(),
                ErrorCode::WorkflowLocked,
                "{set} on {key}: {err:?}"
            );
        }
    }
    write(
        &mut store,
        "UPDATE workflows SET base = 'main' WHERE plan = 'pl' AND key IN ('b', 'e');
         UPDATE workflows SET base = 'develop' WHERE plan = 'pl' AND key IN ('a', 'c', 'd', 'op')",
    )
    .unwrap();
    assert_eq!(
        rows(
            &mut store,
            "SELECT key, base FROM workflows WHERE plan = 'pl' AND key IN ('b', 'e', 'op') ORDER BY key"
        ),
        vec![
            serde_json::json!(["b", "main"]),
            serde_json::json!(["e", "main"]),
            serde_json::json!(["op", "develop"])
        ]
    );
    // `b` is no longer stacked, but a waiting Workflow's base still follows (`workflow-base-retarget`).
    // Before the lock any Workflow may change, as before.
    write(
        &mut store,
        "UPDATE workflows SET base = 'develop' WHERE plan = 'other'",
    )
    .unwrap();
}

// ---------- named migration `event-pr-base-changed` (§4.5, §5.2) ----------

#[test]
fn event_pr_base_changed_is_additive_and_adds_exactly_its_kind() {
    let migration = NAMED
        .iter()
        .find(|n| n.name.ends_with("-event-pr-base-changed"))
        .unwrap();
    assert_eq!(migration.compat, Compat::Additive);
    assert_eq!(
        migration.sql.lines().next().unwrap(),
        "-- cpd-migration: additive"
    );

    let (_dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-event-pr-base-changed"))
        .collect();
    drop(crate::store::tests::open_named(&home, &earlier).unwrap());
    let before: i64 = connection(&home.db())
        .query_row("SELECT count(*) FROM event_kinds", [], |r| r.get(0))
        .unwrap();

    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(store.schema_mismatch(), None);
    assert_eq!(
        rows(
            &mut store,
            "SELECT name FROM event_kinds WHERE name = 'pr-base-changed'"
        ),
        vec![serde_json::json!(["pr-base-changed"])]
    );
    // this migration's one kind, the two of an earlier migration, the one of `event-session-retiered`, the one of `plan-outline`, the one of `session-asleep`, the two of `delete-tombstone`, the three of `curation`, the one of `loop-guard`, the two of `incident` the one of `incident-plans-event` and the one of `event-mcp-attached`, the one of `inbox-claim`, the one of `pr-description`, the one of `event-session-model-changed`, the one of `attempt-ended-kind`, the one of `attempt-aborted-kind`, the one of `job-blocked-kind`, the one of `job-addenda`, the one of `workflow-reopen`, and the six of `workflow-event-kinds`: the later migrations that add Event kinds
    assert_eq!(
        rows(&mut store, "SELECT count(*) FROM event_kinds"),
        vec![serde_json::json!([before
            + 1
            + 2
            + 1
            + 1
            + 1
            + 2
            + 3
            + 1
            + 2
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + RETIRED_OBJECT_KINDS as i64])]
    );
}

// ---------- named migration session-titles ----------

#[test]
fn a_store_before_session_titles_migrates_keeping_every_session_with_no_title_and_accepts_session_titled()
 {
    let titles = NAMED
        .iter()
        .position(|n| n.name.ends_with("-session-titles"))
        .expect("the migration session-titles exists");
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    let mut before = crate::store::tests::open_named(&home, &NAMED[..titles]).unwrap();
    write(
        &mut before,
        &"INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at) VALUES
           ('m', 'manager', 'ctx', NULL, NULL, 'M', '/', 'O4', 'idle', 't', 't'),
           ('l', 'lead', 'ctx', 'pay', NULL, 'L', '/', 'O3', 'idle', 't', 't'),
           ('w', 'worker', 'ctx', 'pay', 'api-a1', 'W', '/', 'S2', 'working', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 'l', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay', cpd_txn(), 't');
         INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
           VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', cpd_txn(), 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
           VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 'w', 't');"
            .replace("{fp}", &"a".repeat(64)),
    )
    .unwrap();
    // `name` is left out: the later migration `agent-key-backfill` rewrites it (every row is a Manager, a Lead or a Worker).
    let columns = "rowid, id, role, context, plan, attempt, cwd, tier, state";
    let sessions = rows(
        &mut before,
        &format!("SELECT {columns} FROM sessions ORDER BY id"),
    );
    assert!(sqlite_message(&refused(&mut before, "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), 't', 'session-titled', 'x', '{}')"))
        .contains("FOREIGN KEY constraint failed"));
    drop(before);

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(
        rows(
            &mut store,
            &format!("SELECT {columns} FROM sessions ORDER BY id")
        ),
        sessions
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM sessions WHERE title IS NOT NULL OR titled_at IS NOT NULL"
        ),
        0
    );
    assert_eq!(
        count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"),
        0
    );
    write(&mut store, "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), 't', 'session-titled', 'x', '{\"title\":\"T\"}')").unwrap();
    // The title is 1–200 characters (`session-title-length` widened it from 60, hs-d543), and the history lists both new columns.
    write(
        &mut store,
        &format!(
            "UPDATE sessions SET title = '{}', titled_at = 't' WHERE id = 'l'",
            "é".repeat(60)
        ),
    )
    .unwrap();
    for bad in [String::new(), "x".repeat(201)] {
        let error = refused(
            &mut store,
            &format!("UPDATE sessions SET title = '{bad}' WHERE id = 'm'"),
        );
        assert!(
            sqlite_message(&error).contains("CHECK constraint failed"),
            "{bad:?}: {error:?}"
        );
    }
    assert_eq!(
        rows(
            &mut store,
            "SELECT json_extract(after, '$.title') IS NOT NULL, json_extract(after, '$.titled_at') FROM history WHERE tbl = 'sessions' AND op = 'update'"
        ),
        vec![serde_json::json!([1, "t"])]
    );
    let migrated = store.read(|c| Ok(canonical_hash(c)?)).unwrap();
    assert_eq!(
        migrated,
        fresh_canonical_hash(),
        "the schema of a fresh store with the migration"
    );
}

// ---------- named migration `job-kinds` (§4.5; §2.2 "Job kinds", hs-d341, hs-d497) ----------

/// A locked Plan `pl` with one `waiting` Workflow `a` and a Job of every status, as a store before `job-kinds` holds them.
const KINDS_FIXTURE: &str = "
  INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
  INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
    VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'idle', 't', 't');
  INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
    VALUES ('pl', 'ctx', 'Plan', 'dispatch', 'lead', 't', 't');
  INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
    VALUES ('pl', 'a', '/r', 'main', 'waiting', 'cpd/pl/a', cpd_txn(), 't');
  INSERT INTO jobs (plan, pr, key, title, status, appended, created_txn, created_at)
    VALUES ('pl', 'a', 'stub', 'Stub', 'draft', 1, cpd_txn(), 't');
  INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
    VALUES ('pl', 'a', 'draft', 'D', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, NULL, cpd_txn(), 't'),
           ('pl', 'a', 'queued', 'Q', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', cpd_txn(), 't'),
           ('pl', 'a', 'running', 'R', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', cpd_txn(), 't'),
           ('pl', 'a', 'merged', 'M', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', cpd_txn(), 't'),
           ('pl', 'a', 'failed', 'F', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', cpd_txn(), 't'),
           ('pl', 'a', 'gone', 'G', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', cpd_txn(), 't');
  UPDATE jobs SET status = 'running' WHERE key IN ('running', 'merged', 'failed');
  UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'merged';
  UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'failed';
  UPDATE jobs SET status = 'cancelled', cancel_reason = 'no', cancel_requested_at = 't' WHERE key = 'gone';
";

/// A named store as the binaries before `job-kinds` left it, holding [`KINDS_FIXTURE`].
fn store_before_job_kinds(home: &CpdHome) -> Vec<Named> {
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-job-kinds"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "job-kinds is a built-in migration"
    );
    drop(crate::store::tests::open_named(home, &earlier).unwrap());
    connection(&home.db())
        .execute_batch(&format!("PRAGMA foreign_keys = OFF; {KINDS_FIXTURE}"))
        .unwrap();
    earlier
}

#[test]
fn job_kinds_is_breaking_after_the_names_before_it() {
    let (index, migration) = NAMED
        .iter()
        .enumerate()
        .find(|(_, n)| n.name.ends_with("-job-kinds"))
        .unwrap();
    assert_eq!(migration.compat, Compat::Breaking);
    let after = LEGACY.len() + index;
    assert_eq!(
        migration.sql.lines().next().unwrap(),
        format!("-- cpd-migration: breaking after {after}")
    );
}

#[test]
fn job_kinds_keeps_every_job_and_pr_as_work_and_free_and_matches_a_fresh_store() {
    let (_dir, home) = crate::store::tests::tmp_home();
    store_before_job_kinds(&home);
    let before = dump(&connection(&home.db()));

    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(store.schema_mismatch(), None);
    assert!(
        crate::store::tests::applied(&mut store)
            .iter()
            .any(|(name, _, _)| name.ends_with("-job-kinds"))
    );
    assert_eq!(
        count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"),
        0
    );

    // Every Job is `work` with the other columns null (`resources` of `job-resources` `skipped_n` of `job-tenancy` and `summary` of `explore-kinds` too) and every Workflow is `free`: the rows are as they were, plus those.
    let after = dump(&connection(&home.db()));
    let strip = |rows: &[String], added: usize, default: &str| -> Vec<String> {
        rows.iter()
            .map(|row| {
                let mut cells: Vec<serde_json::Value> = serde_json::from_str(row).unwrap();
                let tail = cells.split_off(cells.len() - added);
                assert_eq!(tail[0], serde_json::json!(default), "{row}");
                assert!(tail[1..].iter().all(serde_json::Value::is_null), "{row}");
                serde_json::Value::from(cells).to_string()
            })
            .collect()
    };
    assert_eq!(strip(&after["jobs"], 8, "work"), before["jobs"]);
    // `prs` gained `shape` (`free`), then `pr-adopt` `head_ref` and `pr-description` `description`, both null.
    assert_eq!(strip(&after["workflows"], 4, "free"), before["prs"]);
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM jobs WHERE status = 'merged'"
        ),
        1
    );

    let migrated = store.read(|c| Ok(canonical_hash(c)?)).unwrap();
    assert_eq!(
        migrated,
        fresh_canonical_hash(),
        "the schema of a fresh store with the migration"
    );
}

/// The store migrated to `job-kinds` from [`store_before_job_kinds`].
fn store_with_job_kinds() -> (tempfile::TempDir, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    store_before_job_kinds(&home);
    let store = Store::open(&home, 5000).unwrap();
    (dir, store)
}

#[test]
fn the_shape_of_a_pr_is_one_of_four_and_never_changes() {
    let (_dir, mut store) = store_with_job_kinds();
    assert_eq!(
        rows(&mut store, "SELECT shape FROM workflows"),
        vec![serde_json::json!(["free"])],
        "the Workflow that existed before the migration"
    );
    for shape in ["tdd", "docs", "config"] {
        write(
            &mut store,
            &format!(
                "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, shape, created_txn, created_at)
                 VALUES ('pl', 's-{shape}', '/r', 'main', 'waiting', 'cpd/pl/s-{shape}', '{shape}', cpd_txn(), 't')"
            ),
        )
        .unwrap();
    }
    let bad = refused(
        &mut store,
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, shape, created_txn, created_at)
         VALUES ('pl', 'bad', '/r', 'main', 'waiting', 'cpd/pl/bad', 'other', cpd_txn(), 't')",
    );
    assert!(
        sqlite_message(&bad).contains("CHECK constraint failed"),
        "{bad:?}"
    );
    // The shape is the Workflow's identity, in any phase: `prs_shape_fixed` refuses every update of it, a same value included.
    for shape in ["docs", "tdd", "free"] {
        assert_eq!(
            sqlite_message(&refused(
                &mut store,
                &format!("UPDATE workflows SET shape = '{shape}' WHERE key = 's-tdd'")
            )),
            "workflow_locked"
        );
    }
    // The Workflow's other columns still change where they could.
    write(
        &mut store,
        "UPDATE workflows SET title = 'New' WHERE key = 's-tdd'",
    )
    .unwrap();
}

/// A Job `key` of `kind` in Workflow `a`, as a draft with the Spec and the given extra columns and values.
fn job_of_kind(key: &str, kind: &str, columns: &str, values: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind{columns}, created_txn, created_at)
         VALUES ('pl', 'a', '{key}', 'T', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, '{kind}'{values}, cpd_txn(), 't')"
    )
}

#[test]
fn a_jobs_kind_is_one_of_six_and_its_spec_fields_agree_with_it() {
    let (_dir, mut store) = store_with_job_kinds();
    let spec_invalid = |store: &mut Store, sql: &str| {
        assert_eq!(
            sqlite_message(&refused(store, sql)),
            "spec_invalid",
            "{sql}"
        )
    };
    let tests = ", tests, test_files";
    let named = ", '[\"t::one\"]', '[\"tests/t.rs\"]'";
    for kind in ["work", "docs", "impl", "open", "update"] {
        write(&mut store, &job_of_kind(&format!("k-{kind}"), kind, "", "")).unwrap();
    }
    write(&mut store, &job_of_kind("k-test", "test", tests, named)).unwrap();
    assert_eq!(
        count(&mut store, "SELECT count(DISTINCT kind) FROM jobs"),
        6
    );
    let bad = refused(&mut store, &job_of_kind("k-bad", "other", "", ""));
    assert!(
        sqlite_message(&bad).contains("CHECK constraint failed"),
        "{bad:?}"
    );

    // `tests` and `test_files` belong to a `test` Job, and both or neither are set.
    spec_invalid(&mut store, &job_of_kind("k-1", "impl", tests, named));
    spec_invalid(&mut store, &job_of_kind("k-2", "work", tests, named));
    spec_invalid(
        &mut store,
        &job_of_kind("k-3", "test", ", tests", ", '[\"t::one\"]'"),
    );
    spec_invalid(
        &mut store,
        &job_of_kind("k-4", "test", ", test_files", ", '[\"tests/t.rs\"]'"),
    );
    // A `test` Job that is a stub draft has neither; one that is queued has both.
    write(
        &mut store,
        "INSERT INTO jobs (plan, workflow, key, title, kind, status, appended, created_txn, created_at)
         VALUES ('pl', 'a', 'k-stub', 'Stub', 'test', 'draft', 1, cpd_txn(), 't')",
    )
    .unwrap();
    spec_invalid(
        &mut store,
        &format!("INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
                  VALUES ('pl', 'a', 'k-5', 'T', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'test', '{FP}', cpd_txn(), 't')"),
    );
    write(
        &mut store,
        "UPDATE jobs SET kind = 'work' WHERE key = 'k-stub'",
    )
    .unwrap();
    // The same rule on update: the review flag is for `work`, the approvals for `impl`, the named tests for `test`.
    spec_invalid(
        &mut store,
        "UPDATE jobs SET review = 1 WHERE key = 'k-docs'",
    );
    write(
        &mut store,
        "UPDATE jobs SET review = 1 WHERE key = 'k-work'",
    )
    .unwrap();
    spec_invalid(
        &mut store,
        "UPDATE jobs SET kind = 'docs' WHERE key = 'k-work'",
    );
    spec_invalid(
        &mut store,
        "UPDATE jobs SET test_approved = '[\"a\"]' WHERE key = 'k-work'",
    );
    write(
        &mut store,
        "UPDATE jobs SET test_approved = '[\"tests/t.rs\"]' WHERE key = 'k-impl'",
    )
    .unwrap();
    spec_invalid(
        &mut store,
        "UPDATE jobs SET kind = 'work' WHERE key = 'k-impl'",
    );
    spec_invalid(
        &mut store,
        "UPDATE jobs SET kind = 'impl' WHERE key = 'k-test'",
    );
    spec_invalid(
        &mut store,
        "UPDATE jobs SET tests = NULL WHERE key = 'k-test'",
    );
    // Each column keeps its own check: a JSON array, of at least one name for `tests` and `test_files`.
    for column in ["tests", "test_files"] {
        for bad in ["'[]'", "'{}'", "'nope'"] {
            let error = refused(
                &mut store,
                &format!("UPDATE jobs SET {column} = {bad} WHERE key = 'k-test'"),
            );
            assert!(
                sqlite_message(&error).contains("CHECK constraint failed"),
                "{column} = {bad}: {error:?}"
            );
        }
    }
    let error = refused(
        &mut store,
        "UPDATE jobs SET test_approved = '\"a\"' WHERE key = 'k-impl'",
    );
    assert!(
        sqlite_message(&error).contains("CHECK constraint failed"),
        "{error:?}"
    );
    write(
        &mut store,
        "UPDATE jobs SET test_approved = '[]' WHERE key = 'k-impl'",
    )
    .unwrap();
}

#[test]
fn kind_tests_and_test_files_are_locked_with_the_job_and_test_approved_is_not() {
    let (_dir, mut store) = store_with_job_kinds();
    write(&mut store, &job_of_kind("impl-draft", "impl", "", "")).unwrap();
    // A draft is editable in all three; once it is not a draft they are Locked, as the Spec is.
    write(
        &mut store,
        "UPDATE jobs SET kind = 'docs' WHERE key = 'draft'",
    )
    .unwrap();
    write(&mut store, "UPDATE jobs SET kind = 'test', tests = '[\"a\"]', test_files = '[\"b\"]' WHERE key = 'draft'").unwrap();
    write(
        &mut store,
        "UPDATE jobs SET kind = 'work', tests = NULL, test_files = NULL WHERE key = 'draft'",
    )
    .unwrap();
    for sql in [
        "UPDATE jobs SET kind = 'docs' WHERE key = 'queued'",
        "UPDATE jobs SET kind = 'impl' WHERE key = 'running'",
        "UPDATE jobs SET kind = 'work' WHERE key = 'merged'",
        "UPDATE jobs SET kind = 'work' WHERE key = 'failed'",
    ] {
        assert_eq!(
            sqlite_message(&refused(&mut store, sql)),
            "job_locked",
            "{sql}"
        );
    }
    write(
        &mut store,
        &job_of_kind(
            "test-draft",
            "test",
            ", tests, test_files",
            ", '[\"t::one\"]', '[\"tests/t.rs\"]'",
        ),
    )
    .unwrap();
    for key in ["impl-draft", "test-draft"] {
        write(
            &mut store,
            &format!("UPDATE jobs SET status = 'queued', fingerprint = '{FP}' WHERE key = '{key}'"),
        )
        .unwrap();
    }
    // `tests` and `test_files` are Spec columns: they change while the queued Job has no Attempt, and are Locked once it has one.
    write(
        &mut store,
        "UPDATE jobs SET tests = '[\"a\"]' WHERE key = 'test-draft'",
    )
    .unwrap();
    write(
        &mut store,
        "UPDATE jobs SET test_files = '[\"a\"]' WHERE key = 'test-draft'",
    )
    .unwrap();
    write(
        &mut store,
        "UPDATE jobs SET status = 'running' WHERE key = 'test-draft';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pl', 'test-draft-a1', 'test-draft', 1, 'S2', 'ladder', 'running', 'cpd/pl--test-draft-a1', '/w', 't');
         UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'timed_out', ended_at = 't' WHERE id = 'test-draft-a1';
         UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'test-draft';
         UPDATE jobs SET status = 'queued', failed_cause = NULL WHERE key = 'test-draft';",
    )
    .unwrap();
    assert_eq!(
        sqlite_message(&refused(
            &mut store,
            "UPDATE jobs SET tests = '[\"b\"]' WHERE key = 'test-draft'"
        )),
        "job_locked"
    );
    assert_eq!(
        sqlite_message(&refused(
            &mut store,
            "UPDATE jobs SET test_files = '[\"b\"]' WHERE key = 'test-draft'"
        )),
        "job_locked"
    );
    // `test_approved` changes after the lock: that is what `job approve-tests` does.
    write(
        &mut store,
        "UPDATE jobs SET test_approved = '[\"tests/t.rs\"]' WHERE key = 'impl-draft'",
    )
    .unwrap();
    assert_eq!(
        rows(
            &mut store,
            "SELECT test_approved FROM jobs WHERE key = 'impl-draft'"
        ),
        vec![serde_json::json!(["[\"tests/t.rs\"]"])]
    );
}

#[test]
fn the_history_of_jobs_and_prs_lists_the_new_columns() {
    let (_dir, mut store) = store_with_job_kinds();
    write(
        &mut store,
        &job_of_kind(
            "h",
            "test",
            ", tests, test_files",
            ", '[\"t::one\"]', '[\"tests/t.rs\"]'",
        ),
    )
    .unwrap();
    write(
        &mut store,
        "UPDATE jobs SET test_files = '[\"tests/u.rs\"]' WHERE key = 'h'",
    )
    .unwrap();
    // The insert lists every column; an update the columns it changed (`history-changed`).
    assert_eq!(
        rows(
            &mut store,
            "SELECT json_extract(after, '$.kind'), json_extract(after, '$.tests'), json_extract(after, '$.test_files') FROM history WHERE tbl = 'jobs' AND op = 'insert' AND json_extract(after, '$.key') = 'h'"
        ),
        vec![serde_json::json!([
            "test",
            "[\"t::one\"]",
            "[\"tests/t.rs\"]"
        ])]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT json_extract(after, '$.test_files') FROM history WHERE tbl = 'jobs' AND op = 'update' AND json_extract(after, '$.test_files') IS NOT NULL"
        ),
        vec![serde_json::json!(["[\"tests/u.rs\"]"])]
    );
    write(
        &mut store,
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, shape, created_txn, created_at)
         VALUES ('pl', 'b', '/r', 'main', 'waiting', 'cpd/pl/b', 'tdd', cpd_txn(), 't')",
    )
    .unwrap();
    assert_eq!(
        rows(
            &mut store,
            "SELECT json_extract(after, '$.shape') FROM history WHERE tbl = 'workflows' AND op = 'insert' AND json_extract(after, '$.key') = 'b'"
        ),
        vec![serde_json::json!(["tdd"])]
    );
}

// ---------- named migration `plan-outline` (§4.4, §4.5; hs-d502) ----------

#[test]
fn plan_outline_is_additive_and_adds_the_nullable_column_and_its_event_kind() {
    let migration = NAMED
        .iter()
        .find(|n| n.name.ends_with("-plan-outline"))
        .expect("the migration plan-outline exists");
    assert_eq!(migration.compat, Compat::Additive);
    assert_eq!(
        migration.sql.lines().next().unwrap(),
        "-- cpd-migration: additive"
    );

    let (_dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-plan-outline"))
        .collect();
    let mut before = crate::store::tests::open_named(&home, &earlier).unwrap();
    write(
        &mut before,
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l', 'lead', 'ctx', 'pay', 'L', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 'l', 't');",
    )
    .unwrap();
    assert!(sqlite_message(&refused(&mut before, "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), 't', 'plan-outlined', 'x', '{}')"))
        .contains("FOREIGN KEY constraint failed"));
    drop(before);

    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(store.schema_mismatch(), None);
    // The Plan is kept, with a null outline, and its history lists the new column.
    assert_eq!(
        rows(&mut store, "SELECT id, outline FROM plans"),
        vec![serde_json::json!(["pay", null])]
    );
    write(
        &mut store,
        "UPDATE plans SET outline = '{\"goal\":\"G\"}' WHERE id = 'pay'",
    )
    .unwrap();
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM history WHERE tbl = 'plans' AND json_extract(after, '$.outline') IS NOT NULL"
        ),
        1
    );
    // The outline is a JSON object, or null.
    for bad in ["not json", "[]", "\"text\""] {
        let error = refused(
            &mut store,
            &format!("UPDATE plans SET outline = '{bad}' WHERE id = 'pay'"),
        );
        assert!(
            sqlite_message(&error).contains("CHECK constraint failed"),
            "{bad}: {error:?}"
        );
    }
    write(&mut store, "INSERT INTO events (txn, at, kind, context, plan, text, data) VALUES (cpd_txn(), 't', 'plan-outlined', 'ctx', 'pay', 'x', '{\"replaced\":false}')").unwrap();
    assert_eq!(
        count(&mut store, "SELECT count(*) FROM pragma_foreign_key_check"),
        0
    );
    let migrated = store.read(|c| Ok(canonical_hash(c)?)).unwrap();
    assert_eq!(
        migrated,
        fresh_canonical_hash(),
        "the schema of a fresh store with the migration"
    );
}

// ---------- named migration delete-tombstone ----------

#[test]
fn delete_tombstone_keeps_every_row_and_lets_only_the_rows_of_a_deleted_plan_or_context_go() {
    let at = NAMED
        .iter()
        .position(|n| n.name.ends_with("-delete-tombstone"))
        .expect("the migration delete-tombstone exists");
    let dir = tempfile::tempdir().unwrap();
    let home = home(&dir);
    let mut before = crate::store::tests::open_named(&home, &NAMED[..at]).unwrap();
    write(
        &mut before,
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1'), ('other', 'Other', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at) VALUES
           ('m', 'manager', 'ctx', NULL, NULL, 'M', '/', 'O4', 'idle', 't', 't'),
           ('l', 'lead', 'ctx', 'pay', NULL, 'L', '/', 'O3', 'idle', 't', 't'),
           ('lo', 'lead', 'other', 'live', NULL, 'LO', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
           ('pay', 'ctx', 'Pay', 'grounding', 'l', 't'), ('live', 'other', 'Live', 'grounding', 'lo', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay', cpd_txn(), 't'), ('live', 'r', '/r', 'main', 'waiting', 'cpd/live', cpd_txn(), 't');
         INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
           VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't'),
                  ('live', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't');
         INSERT INTO events (txn, at, kind, context, plan, text, data) VALUES
           (cpd_txn(), 't', 'plan-created', 'ctx', 'pay', 'x', '{}'), (cpd_txn(), 't', 'plan-created', 'other', 'live', 'x', '{}'),
           (cpd_txn(), 't', 'context-created', 'ctx', NULL, 'x', '{}');",
    )
    .unwrap();
    let rows_before = rows(
        &mut before,
        "SELECT id, title FROM contexts UNION ALL SELECT id, title FROM plans",
    );
    drop(before);

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, title FROM contexts UNION ALL SELECT id, title FROM plans"
        ),
        rows_before
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM contexts WHERE deleted_at IS NOT NULL"
        ) + count(
            &mut store,
            "SELECT count(*) FROM plans WHERE deleted_at IS NOT NULL"
        ),
        0
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM event_kinds WHERE name IN ('plan-deleted', 'context-deleted')"
        ),
        2
    );

    // A live Plan's rows stay undeletable, with the error each had.
    for (sql, message) in [
        ("DELETE FROM jobs WHERE plan = 'pay'", "append_only: jobs"),
        (
            "DELETE FROM workflows WHERE plan = 'pay'",
            "append_only: workflows",
        ),
        (
            "DELETE FROM events WHERE plan = 'pay'",
            "append_only: events",
        ),
        (
            "DELETE FROM events WHERE context = 'ctx' AND plan IS NULL",
            "append_only: events",
        ),
        (
            "DELETE FROM sessions WHERE id = 'm'",
            "append_only: sessions",
        ),
        ("DELETE FROM plans WHERE id = 'pay'", "append_only: plans"),
        (
            "DELETE FROM contexts WHERE id = 'ctx'",
            "append_only: contexts",
        ),
        ("DELETE FROM txns", "append_only: txns"),
        ("DELETE FROM history", "append_only: history"),
    ] {
        assert_eq!(sqlite_message(&refused(&mut store, sql)), message, "{sql}");
    }

    // The tombstone is written once; the rows of the tombstoned Plan go, the others' not.
    write(
        &mut store,
        "UPDATE plans SET deleted_at = 'd1' WHERE id = 'pay'",
    )
    .unwrap();
    assert_eq!(
        sqlite_message(&refused(
            &mut store,
            "UPDATE plans SET deleted_at = NULL WHERE id = 'pay'"
        )),
        "append_only: plans.deleted_at"
    );
    assert_eq!(
        sqlite_message(&refused(
            &mut store,
            "UPDATE plans SET deleted_at = 'd2' WHERE id = 'pay'"
        )),
        "append_only: plans.deleted_at"
    );
    write(
        &mut store,
        "UPDATE plans SET title = 'Pay 2' WHERE id = 'pay'",
    )
    .unwrap();
    assert_eq!(
        sqlite_message(&refused(&mut store, "DELETE FROM jobs WHERE plan = 'live'")),
        "append_only: jobs"
    );
    assert_eq!(
        sqlite_message(&refused(&mut store, "DELETE FROM plans WHERE id = 'pay'")),
        "append_only: plans"
    );
    write(&mut store, "DELETE FROM events WHERE plan = 'pay'; DELETE FROM jobs WHERE plan = 'pay'; DELETE FROM workflows WHERE plan = 'pay';").unwrap();
    assert_eq!(
        rows(
            &mut store,
            "SELECT tbl, op, json_extract(before, '$.key') FROM history WHERE op = 'delete' AND tbl <> 'history_tables' ORDER BY id"
        ),
        [
            serde_json::json!(["jobs", "delete", "api"]),
            serde_json::json!(["workflows", "delete", "r"])
        ]
    );

    // A Context's own rows go by its tombstone alone.
    write(
        &mut store,
        "UPDATE contexts SET deleted_at = 'd3' WHERE id = 'ctx'",
    )
    .unwrap();
    assert_eq!(
        sqlite_message(&refused(
            &mut store,
            "UPDATE contexts SET deleted_at = NULL WHERE id = 'ctx'"
        )),
        "append_only: contexts.deleted_at"
    );
    write(&mut store, "DELETE FROM events WHERE context = 'ctx' AND plan IS NULL; DELETE FROM sessions WHERE id = 'm';").unwrap();
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM history WHERE tbl = 'sessions' AND op = 'delete'"
        ),
        1
    );
    assert_eq!(
        sqlite_message(&refused(
            &mut store,
            "DELETE FROM events WHERE context = 'other'"
        )),
        "append_only: events"
    );
    assert_eq!(store.tamper().unwrap(), Vec::<Value>::new());
}

// ---------- named migration `review-superseded` ----------

/// A store as every migration before `review-superseded` leaves it (the test ends here with that name: it fails
/// while the migration is missing).
fn before_review_superseded() -> (tempfile::TempDir, CpdHome, Vec<Named>) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-review-superseded"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "review-superseded is a built-in migration"
    );
    drop(crate::store::tests::open_named(&home, &earlier).unwrap());
    (dir, home, earlier)
}

const REVIEW_FP: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A Plan `id` in `phase` with its Lead (no Workflow): the SQL of one fixture.
fn review_plan(id: &str, phase: &str) -> String {
    let (locked, done) = match phase {
        "grounding" | "plan" => ("NULL", "NULL"),
        "done" => ("'2026-01-02T00:00:00.000Z'", "'2026-01-09T00:00:00.000Z'"),
        _ => ("'2026-01-02T00:00:00.000Z'", "NULL"),
    };
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('{id}', 'ctx', 'Plan {id}', '{phase}', 'l-{id}', 't', {locked}, {done});
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-{id}', 'lead', 'ctx', '{id}', 'Lead', '/', 'O3', 'idle', 't', 't');"
    )
}

/// One review of `plan`: `number`, `status`, a feedback and a title always set, `decided_at` unless pending.
fn review_row(plan: &str, number: u32, status: &str) -> String {
    let decided = if status == "pending" {
        "NULL"
    } else {
        "'2026-01-03T00:00:00.000Z'"
    };
    format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, feedback, requested_at, decided_at, title)
           VALUES ('{plan}', 'r{number}', {number}, '{REVIEW_FP}', '{status}', 'feedback of {plan} r{number}', '2026-01-02T00:00:00.000Z', {decided}, 'Title {number}');"
    )
}

fn review_rows(conn: &Connection) -> Vec<Value> {
    let columns = table_columns(conn, "plan_reviews").unwrap();
    let pairs: Vec<String> = columns.iter().map(|(c, _)| format!("'{c}', {c}")).collect();
    let mut stmt = conn
        .prepare(&format!(
            "SELECT json_object({}) FROM plan_reviews ORDER BY plan, number",
            pairs.join(", ")
        ))
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| serde_json::from_str(&r.unwrap()).unwrap())
        .collect()
}

fn review_status(
    conn: &Connection,
    plan: &str,
    id: &str,
) -> (String, Option<String>, Option<String>) {
    conn.query_row(
        "SELECT status, decided_at, feedback FROM plan_reviews WHERE plan = ?1 AND id = ?2",
        [plan, id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

#[test]
fn review_superseded_keeps_every_review_and_accepts_the_status() {
    let (_dir, home, earlier) = before_review_superseded();
    let conn = connection(&home.db());
    conn.execute_batch(&format!(
        "BEGIN;
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         {}
         {} {} {}
         COMMIT;",
        review_plan("pl", "plan"),
        review_row("pl", 1, "rejected"),
        review_row("pl", 2, "approved"),
        review_row("pl", 3, "pending"),
    ))
    .unwrap();
    let before = review_rows(&conn);
    let objects = "SELECT name || ' ' || coalesce(sql, '') FROM sqlite_schema WHERE tbl_name = 'plan_reviews' AND name NOT LIKE 'sqlite_%'";
    let objects_before = names(&conn, objects);
    let history = |c: &Connection| {
        c.query_row::<i64, _, _>(
            "SELECT count(*) FROM history WHERE tbl = 'plan_reviews'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    let history_before = history(&conn);
    assert_eq!((before.len(), history_before), (3, 3));
    drop(conn);

    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(store.schema_mismatch(), None);
    assert!(
        crate::store::tests::applied(&mut store)
            .iter()
            .any(|(name, _, _)| name.ends_with("-review-superseded"))
    );
    let conn = connection(&home.db());
    assert_eq!(review_rows(&conn), before, "every row, column for column");
    assert_eq!(history(&conn), history_before, "the copy writes no history");
    let after = names(&conn, objects);
    for expected in [
        "plan_reviews_hist_ins",
        "plan_reviews_hist_upd",
        "plan_reviews_no_delete",
    ] {
        assert!(
            after.iter().any(|o| o.starts_with(expected)),
            "{expected} in {after:?}"
        );
    }
    // Everything but the CHECK is as it was: the same indexes and triggers.
    let named = |set: &std::collections::BTreeSet<String>| -> Vec<String> {
        set.iter()
            .map(|o| o.split(' ').next().unwrap().to_owned())
            .collect()
    };
    assert_eq!(named(&after), named(&objects_before));
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get(0))
            .unwrap(),
        0
    );
    drop(conn);

    // `superseded` is decided: it needs `decided_at`; `pending` still has none.
    let message = sqlite_message(&refused(
        &mut store,
        "UPDATE plan_reviews SET status = 'superseded' WHERE plan = 'pl' AND id = 'r3'",
    ));
    assert!(message.starts_with("CHECK constraint failed"), "{message}");
    let message = sqlite_message(&refused(
        &mut store,
        "UPDATE plan_reviews SET status = 'pending' WHERE plan = 'pl' AND id = 'r1'",
    ));
    assert!(message.starts_with("CHECK constraint failed"), "{message}");
    write(&mut store, "UPDATE plan_reviews SET status = 'superseded', decided_at = '2026-01-04T00:00:00.000Z' WHERE plan = 'pl' AND id = 'r3'").unwrap();
    assert_eq!(store.tamper().unwrap(), Vec::<Value>::new());
    drop(store);

    // A binary without it cannot open the store: the migration is breaking (§4.5 A1).
    let err = crate::store::tests::open_named(&home, &earlier)
        .err()
        .unwrap();
    assert_eq!(err.code(), ErrorCode::StoreTooNew, "{err:?}");
}

#[test]
fn review_superseded_closes_the_pending_reviews_that_are_not_the_latest_or_of_a_done_plan() {
    let (_dir, home, _earlier) = before_review_superseded();
    let conn = connection(&home.db());
    conn.execute_batch(&format!(
        "BEGIN;
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         {} {} {} {}
         {} {}
         {} {}
         {}
         {}
         INSERT INTO events (txn, at, kind, context, plan, text, data) VALUES (1, 't', 'plan-created', 'ctx', 'pa', 'x', '{{}}');
         COMMIT;",
        review_plan("pa", "plan"),
        review_plan("pb", "plan"),
        review_plan("pc", "done"),
        review_plan("pd", "plan"),
        review_row("pa", 1, "pending"),
        review_row("pa", 2, "approved"),
        review_row("pb", 1, "pending"),
        review_row("pb", 2, "pending"),
        review_row("pc", 1, "pending"),
        review_row("pd", 1, "pending"),
    ))
    .unwrap();
    let before = review_rows(&conn);
    let events: i64 = conn
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .unwrap();
    drop(conn);

    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(store.schema_mismatch(), None);
    let conn = connection(&home.db());
    for (plan, id) in [("pa", "r1"), ("pb", "r1"), ("pc", "r1")] {
        let (status, decided_at, feedback) = review_status(&conn, plan, id);
        assert_eq!(status, "superseded", "{plan} {id}");
        assert!(decided_at.is_some(), "{plan} {id}: decided_at");
        assert_eq!(
            feedback.as_deref(),
            Some(format!("feedback of {plan} {id}").as_str()),
            "{plan} {id}: feedback is kept"
        );
    }
    for (plan, id, status) in [
        ("pa", "r2", "approved"),
        ("pb", "r2", "pending"),
        ("pd", "r1", "pending"),
    ] {
        let row = before
            .iter()
            .find(|r| r["plan"] == plan && r["id"] == id)
            .unwrap();
        assert_eq!(row["status"], status);
        assert_eq!(
            review_rows(&conn)
                .iter()
                .find(|r| r["plan"] == plan && r["id"] == id),
            Some(row),
            "{plan} {id} is as it was"
        );
    }
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT count(*) FROM events", [], |r| r.get(0))
            .unwrap(),
        events,
        "no event is appended"
    );
    drop(conn);
    assert_eq!(store.tamper().unwrap(), Vec::<Value>::new());
}

#[test]
fn review_superseded_refuses_a_plan_done_while_a_review_is_pending() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    write(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
             {} {} {}
             {}",
            review_plan("pa", "deploy"),
            review_plan("pb", "deploy"),
            review_plan("pc", "done"),
            review_row("pa", 1, "pending"),
        ),
    )
    .unwrap();
    let done = "UPDATE plans SET phase = 'done', done_at = 't' WHERE id = ";

    // A pending review of the Plan forbids `done`.
    let message = sqlite_message(&refused(&mut store, &format!("{done}'pa'")));
    assert!(message.contains("review_pending"), "{message}");
    // Decided, then superseded: it goes through.
    write(&mut store, "UPDATE plan_reviews SET status = 'approved', decided_at = 't' WHERE plan = 'pa' AND id = 'r1'").unwrap();
    write(&mut store, &format!("{done}'pa'")).unwrap();
    write(
        &mut store,
        &format!(
            "{} {}
             UPDATE plan_reviews SET status = 'superseded', decided_at = 't' WHERE plan = 'pb' AND id = 'r1';
             {done}'pb';",
            review_row("pb", 1, "pending"),
            review_row("pb", 2, "approved")
        ),
    )
    .unwrap();

    // A Plan that is done already is not blocked by a pending review it holds, nor is another phase.
    write(&mut store, &review_row("pc", 1, "pending")).unwrap();
    write(
        &mut store,
        "UPDATE plans SET phase = 'done' WHERE id = 'pc'",
    )
    .unwrap();
    write(
        &mut store,
        "UPDATE plans SET phase = 'dispatch', done_at = NULL WHERE id = 'pc'",
    )
    .unwrap();
    assert_eq!(store.tamper().unwrap(), Vec::<Value>::new());
}

// ---------- resource tenancy (pro-d106; migration `job-tenancy`; contract §3.4 "Tenancy", §4.4 `job_pools` and `job_tenants`) ----------

const TENANCY_FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PORT: &str = "repo:acme/pay:e2e-port";

/// A fully migrated store with Plan `pl` (locked), its Workflow `a` and one `queued` Job `api` with no Attempt.
fn tenancy_store() -> (tempfile::TempDir, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    write(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('pl', 'ctx', 'Plan', 'dispatch', 'lead', 't', 't');
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
               VALUES ('pl', 'a', '/r', 'main', 'waiting', 'cpd/pl/a', cpd_txn(), 't');
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, resources, fingerprint, created_txn, created_at)
               VALUES ('pl', 'a', 'api', 'T', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, '[\"e2e-port\"]', NULL, cpd_txn(), 't');
             UPDATE jobs SET status = 'queued', fingerprint = '{TENANCY_FP}' WHERE key = 'api';"
        ),
    )
    .unwrap();
    (dir, store)
}

/// The Job's first Attempt, `running`, and its `job_pools` row, in ONE transaction, as the starter writes them.
const FIRST_ATTEMPT: &str = "UPDATE jobs SET status = 'running' WHERE key = 'api';
    INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
      VALUES ('pl', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pl--api-a1', '/w', 't');
    INSERT INTO job_pools (plan, job, pool) VALUES ('pl', 'api', 'repo:acme/pay:e2e-port');";

const END_FAILED: &str = "UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'timed_out', ended_at = 't' WHERE id = 'api-a1';";

/// The pools `api` is a tenant of, sorted.
fn tenancy_of_api(store: &mut Store) -> Vec<String> {
    rows(
        store,
        "SELECT pool FROM job_tenants WHERE plan = 'pl' AND job = 'api' ORDER BY pool",
    )
    .into_iter()
    .map(|r| r[0].as_str().unwrap().to_owned())
    .collect()
}

fn both() -> Vec<String> {
    vec![PORT.to_owned(), "workers".to_owned()]
}

#[test]
fn tenancy_job_pools_row_is_written_in_the_transaction_of_the_first_attempt() {
    let (_dir, mut store) = tenancy_store();
    // A row with no Attempt of the Job, or in a later transaction than the first Attempt's, is refused: a Job is a tenant only
    // from its first Attempt, and the rows commit with it.
    let alone =
        "INSERT INTO job_pools (plan, job, pool) VALUES ('pl', 'api', 'repo:acme/pay:e2e-port')";
    assert!(
        write(&mut store, alone).is_err(),
        "a job_pools row with no Attempt is refused"
    );
    assert_eq!(count(&mut store, "SELECT count(*) FROM job_pools"), 0);

    write(&mut store, FIRST_ATTEMPT).unwrap();
    assert_eq!(
        rows(&mut store, "SELECT plan, job, pool FROM job_pools"),
        [serde_json::json!(["pl", "api", PORT])]
    );

    // After the ladder retry (a second Attempt) no row is written: the Job holds its pools already.
    write(&mut store, &format!("{END_FAILED} UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'api';")).unwrap();
    write(
        &mut store,
        "UPDATE jobs SET status = 'queued', failed_cause = NULL WHERE key = 'api';
         UPDATE jobs SET status = 'running' WHERE key = 'api';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pl', 'api-a2', 'api', 2, 'S2', 'retry', 'running', 'cpd/pl--api-a2', '/w', 't');",
    )
    .unwrap();
    assert!(
        write(
            &mut store,
            "INSERT INTO job_pools (plan, job, pool) VALUES ('pl', 'api', 'host:other')"
        )
        .is_err(),
        "no row after the first Attempt"
    );
    // Never updated or deleted.
    assert!(
        write(
            &mut store,
            "UPDATE job_pools SET pool = 'host:x' WHERE job = 'api'"
        )
        .is_err(),
        "append only: no update"
    );
    assert!(
        write(&mut store, "DELETE FROM job_pools WHERE job = 'api'").is_err(),
        "append only: no delete"
    );
    assert_eq!(count(&mut store, "SELECT count(*) FROM job_pools"), 1);
    // The pool text names its level: `[a-z]*:*`.
    assert!(
        write(
            &mut store,
            "INSERT INTO job_pools (plan, job, pool) VALUES ('pl', 'api', 'nocolon')"
        )
        .is_err()
    );
}

#[test]
fn tenancy_view_lists_a_job_from_its_first_attempt_until_merged_or_cancelled() {
    let (_dir, mut store) = tenancy_store();
    assert_eq!(
        tenancy_of_api(&mut store),
        Vec::<String>::new(),
        "a queued Job with no Attempt is no tenant"
    );

    write(&mut store, FIRST_ATTEMPT).unwrap();
    assert_eq!(
        tenancy_of_api(&mut store),
        both(),
        "`workers` (no row) and the pool of its row, from the first Attempt"
    );

    // Another Job that never had an Attempt, and one with no `job_pools` row (a Job started before the migration): `workers` only.
    write(
        &mut store,
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pl', 'a', 'old', 'T', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, NULL, cpd_txn(), 't');
             UPDATE jobs SET status = 'queued', fingerprint = '{TENANCY_FP}' WHERE key = 'old';
             UPDATE jobs SET status = 'running' WHERE key = 'old';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
               VALUES ('pl', 'old-a1', 'old', 1, 'S2', 'ladder', 'running', 'cpd/pl--old-a1', '/w', 't');"
        ),
    )
    .unwrap();
    assert_eq!(
        rows(
            &mut store,
            "SELECT job, pool FROM job_tenants ORDER BY job, pool"
        ),
        [
            serde_json::json!(["api", PORT]),
            serde_json::json!(["api", "workers"]),
            serde_json::json!(["old", "workers"])
        ]
    );
}

#[test]
fn tenancy_survives_a_failed_job_and_a_ladder_retry() {
    let (_dir, mut store) = tenancy_store();
    write(&mut store, FIRST_ATTEMPT).unwrap();

    // The Attempt ends `failed` and the Job is `failed`: it keeps ALL its resources, `workers` included.
    write(&mut store, &format!("{END_FAILED} UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'api';")).unwrap();
    assert_eq!(
        tenancy_of_api(&mut store),
        both(),
        "a failed Job keeps its pools and its worker"
    );

    // `queued` again for the next Attempt (a ladder retry, `job retry`): the same.
    write(
        &mut store,
        "UPDATE jobs SET status = 'queued', failed_cause = NULL WHERE key = 'api';",
    )
    .unwrap();
    assert_eq!(
        tenancy_of_api(&mut store),
        both(),
        "a retried queued Job keeps its pools and its worker"
    );

    // Ended `running` Job -> `queued` after a ladder step, with its Attempt ended too.
    write(
        &mut store,
        "UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'api';",
    )
    .unwrap();
    assert_eq!(tenancy_of_api(&mut store), both());
}

#[test]
fn tenancy_ends_at_once_when_a_job_is_merged() {
    let (_dir, mut store) = tenancy_store();
    write(&mut store, FIRST_ATTEMPT).unwrap();
    assert_eq!(tenancy_of_api(&mut store), both());

    write(
        &mut store,
        "UPDATE attempts SET status = 'ended', outcome = 'merged', cause = 'merged', merged_sha = 'abc', ended_at = 't' WHERE id = 'api-a1';
         UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';",
    )
    .unwrap();

    assert_eq!(
        tenancy_of_api(&mut store),
        Vec::<String>::new(),
        "free the moment the Job is merged, no timer"
    );
}

#[test]
fn tenancy_ends_at_once_when_a_job_is_cancelled_with_no_live_attempt() {
    let (_dir, mut store) = tenancy_store();
    write(&mut store, FIRST_ATTEMPT).unwrap();
    write(&mut store, &format!("{END_FAILED} UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'api';")).unwrap();
    assert_eq!(tenancy_of_api(&mut store), both(), "failed: held");

    write(&mut store, "UPDATE jobs SET status = 'cancelled', failed_cause = NULL, cancel_reason = 'no longer needed', cancel_requested_at = 't' WHERE key = 'api';").unwrap();

    assert_eq!(
        tenancy_of_api(&mut store),
        Vec::<String>::new(),
        "cancelled with its Attempt ended: free at once"
    );
}

#[test]
fn tenancy_of_a_cancelled_job_with_a_live_attempt_ends_with_the_attempt() {
    let (_dir, mut store) = tenancy_store();
    write(&mut store, FIRST_ATTEMPT).unwrap();

    write(&mut store, "UPDATE jobs SET status = 'cancelled', cancel_reason = 'no longer needed', cancel_requested_at = 't' WHERE key = 'api';").unwrap();
    assert_eq!(
        tenancy_of_api(&mut store),
        both(),
        "a cancelled Job whose Attempt is live holds until the Attempt ends"
    );

    write(
        &mut store,
        "UPDATE attempts SET status = 'ended', outcome = 'cancelled', cause = 'cancel_requested', ended_at = 't' WHERE id = 'api-a1';",
    )
    .unwrap();
    assert_eq!(
        tenancy_of_api(&mut store),
        Vec::<String>::new(),
        "the Attempt ended: free"
    );
}

#[test]
fn tenancy_workers_tenants_are_the_jobs_with_an_attempt_not_merged_or_cancelled() {
    let (_dir, mut store) = tenancy_store();
    // `workers` has no row of its own: its tenants are the Jobs with an Attempt, each counted once however many Attempts it had.
    let workers = |store: &mut Store| {
        count(
            store,
            "SELECT count(*) FROM job_tenants WHERE pool = 'workers'",
        )
    };
    assert_eq!(workers(&mut store), 0, "a Job with no Attempt is no tenant");

    write(&mut store, FIRST_ATTEMPT).unwrap();
    assert_eq!(workers(&mut store), 1);
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM job_pools WHERE pool = 'workers'"
        ),
        0,
        "workers has no job_pools row"
    );

    write(&mut store, &format!("{END_FAILED} UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'api';")).unwrap();
    write(
        &mut store,
        "UPDATE jobs SET status = 'queued', failed_cause = NULL WHERE key = 'api'; UPDATE jobs SET status = 'running' WHERE key = 'api';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pl', 'api-a2', 'api', 2, 'S2', 'retry', 'running', 'cpd/pl--api-a2', '/w', 't');",
    )
    .unwrap();
    assert_eq!(workers(&mut store), 1, "two Attempts, one slot");

    write(
        &mut store,
        "UPDATE attempts SET status = 'ended', outcome = 'merged', cause = 'merged', merged_sha = 'abc', ended_at = 't' WHERE id = 'api-a2';
         UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';",
    )
    .unwrap();
    assert_eq!(workers(&mut store), 0, "merged: no tenant");
}

// ---------- the Slack reaction queue (pro-d107; migration `slack-reactions`; contract §4.4 `slack_reactions`, §4.5) ----------

/// A store as every migration before `slack-reactions` leaves it, holding four Workflows of Plan `pay` and their posts:
/// `a` merged, reaction, reacted; `b` merged, reaction, not reacted; `c` open, reaction; `d` merged, no reaction.
fn before_slack_reactions() -> (tempfile::TempDir, crate::home::CpdHome, Vec<Named>) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-slack-reactions"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "slack-reactions is a built-in migration"
    );
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    let workflow = |key: &str,
                    number: u32,
                    state: &str,
                    merged_at: &str,
                    reaction: &str,
                    reacted_at: &str| {
        format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/pay', {number}, 'https://github.com/acme/pay/pull/{number}', 'Workflow {key}', 0, '{state}', 'none', 'none', 't', {merged_at}, 't');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at, merged_at)
               VALUES ('pay', '{key}', '/nowhere', 'main', '{state}', 'cpd/pay/{key}', '/nowhere/lead', {number}, 'https://github.com/acme/pay/pull/{number}', 't',
                       (SELECT id FROM pull_requests WHERE number = {number}), cpd_txn(), 't', 't', {merged_at});
             INSERT INTO pr_posts (plan, pr, channel, ts, link, posted_at, reaction, reacted_at)
               VALUES ('pay', '{key}', 'C0POST', '1793040000.00000{number}', 'https://acme.slack.com/p{number}', 't', {reaction}, {reacted_at});"
        )
    };
    let sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('pay-lead', 'lead', 'ctx', 'pay', 'Lead pay', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'deploy', 'pay-lead', 't', 't');
         {}{}{}{}",
        workflow("a", 1, "merged", "'2026-10-01T10:00:00.000Z'", "'git-merge'", "'2026-10-01T10:05:00.000Z'"),
        workflow("b", 2, "merged", "'2026-10-02T10:00:00.000Z'", "'git-merge'", "NULL"),
        workflow("c", 3, "open", "NULL", "'git-merge'", "NULL"),
        workflow("d", 4, "merged", "'2026-10-04T10:00:00.000Z'", "NULL", "NULL"),
    );
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    drop(store);
    (dir, home, earlier)
}

fn queue_rows(store: &mut Store) -> Vec<String> {
    store
        .read(|c| {
            let mut stmt = c.prepare("SELECT json_object('channel', channel, 'ts', ts, 'emoji', emoji, 'queued_at', queued_at, 'reacted_at', reacted_at) FROM slack_reactions ORDER BY id")?;
            Ok(stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

#[test]
fn a_database_with_merged_posts_migrates_to_slack_reactions_copying_them() {
    let (_dir, home, _earlier) = before_slack_reactions();
    // Only up to `slack-reactions`: `swr-slack` moves the open rows to the outbox and empties the queue.
    let upto: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-slack-reaction-refusal"))
        .collect();
    let mut store = crate::store::tests::open_named(&home, &upto).unwrap();
    let rows: Vec<Value> = queue_rows(&mut store)
        .iter()
        .map(|r| serde_json::from_str(r).unwrap())
        .collect();
    assert_eq!(
        rows,
        [
            serde_json::json!({"channel": "C0POST", "ts": "1793040000.000001", "emoji": "git-merge", "queued_at": "2026-10-01T10:00:00.000Z", "reacted_at": "2026-10-01T10:05:00.000Z"}),
            serde_json::json!({"channel": "C0POST", "ts": "1793040000.000002", "emoji": "git-merge", "queued_at": "2026-10-02T10:00:00.000Z", "reacted_at": null}),
        ],
        "only the merged posts with a reaction are copied: queued_at is the Workflow's merged_at, reacted_at the post's"
    );
}

#[test]
fn slack_reactions_refuses_a_second_row_for_one_channel_ts_and_emoji() {
    let (_dir, home, _earlier) = before_slack_reactions();
    let mut store = Store::open(&home, 5000).unwrap();
    let insert = |emoji: &str, ts: &str| {
        format!(
            "INSERT INTO slack_reactions (channel, ts, emoji, queued_at) VALUES ('C0POST', '{ts}', '{emoji}', 'q');"
        )
    };
    store
        .write("fixture", |tx| {
            Ok::<(), CpdError>(tx.execute_batch(&insert("eyes", "1793040000.000001"))?)
        })
        .unwrap();
    let refused = store.write("fixture", |tx| {
        Ok::<(), CpdError>(tx.execute_batch(&insert("eyes", "1793040000.000001"))?)
    });
    assert!(refused.is_err(), "the same (channel, ts, emoji) twice");
    // Another emoji, or another post, is another row.
    store
        .write("fixture", |tx| {
            Ok::<(), CpdError>(tx.execute_batch(&insert("white_check_mark", "1793040000.000001"))?)
        })
        .unwrap();
    store
        .write("fixture", |tx| {
            Ok::<(), CpdError>(tx.execute_batch(&insert("eyes", "1793040000.000009"))?)
        })
        .unwrap();
}
