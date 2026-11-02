//! The store migration `20261103T0600-permanent-contexts` (contract §4.5; pro-d338, pro-d340, pro-d343): a store
//! at the migration before it, fixture rows written with SQL, then the migration applied by opening the store.
//! `eec-codereview` moves to `codereview` (a rename, or a merge when `codereview` exists), `host-2` merges into
//! `host`, `cpd`, `host` and `codereview` are unarchived, the Manager and Lead `cwd` of the CPD_HOME moves to the
//! user's home, and the engine index files and the digest directory follow. Fixture machine name: `box`.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;
use std::path::Path;

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
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
                                rusqlite::types::ValueRef::Text(t) => {
                                    Value::from(String::from_utf8_lossy(t).into_owned())
                                }
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
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

/// The first column of every row, as text (`NULL` for a null).
fn strings(store: &mut Store, sql: &str) -> Vec<String> {
    rows(store, sql)
        .into_iter()
        .map(|row| match &row[0] {
            Value::Null => "NULL".to_owned(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect()
}

/// The one text a query returns.
fn text(store: &mut Store, sql: &str) -> String {
    let all = strings(store, sql);
    assert_eq!(all.len(), 1, "{sql}: {all:?}");
    all.into_iter().next().unwrap()
}

fn q(value: Option<&str>) -> String {
    value.map_or("NULL".to_owned(), |v| format!("'{v}'"))
}

fn session(
    id: &str,
    role: &str,
    context: &str,
    plan: Option<&str>,
    name: &str,
    cwd: &str,
    archived: bool,
) -> String {
    let attempt = (role == "worker").then_some("j1-a1");
    let archived_at = archived.then_some("t1");
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, archived_at)
           VALUES ('{id}', '{role}', '{context}', {}, {}, '{name}', '{cwd}', 'O3', 'idle', 't', 't', {});\n",
        q(plan),
        q(attempt),
        q(archived_at)
    )
}

fn plan(id: &str, context: &str, phase: &str, lead: &str, archived: bool) -> String {
    let locked = (phase != "grounding" && phase != "plan").then_some("t");
    let done = (phase == "done").then_some("t");
    let archived_at = archived.then_some("t1");
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at, archived_at)
           VALUES ('{id}', '{context}', 'Plan {id}', '{phase}', '{lead}', 't', {}, {}, {});\n",
        q(locked),
        q(done),
        q(archived_at)
    )
}

fn record(
    context: &str,
    id: &str,
    kind: &str,
    text: &str,
    supersedes: Option<&str>,
    question: Option<&str>,
) -> String {
    let machine = id.split_once('-').unwrap().0;
    format!(
        "INSERT INTO context_records (context, id, kind, at, machine, text, supersedes, question)
           VALUES ('{context}', '{id}', '{kind}', 't', '{machine}', '{text}', {}, {});\n",
        q(supersedes),
        q(question)
    )
}

/// The store as every migration before `permanent-contexts` leaves it, empty.
fn before() -> (tempfile::TempDir, CpdHome, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-permanent-contexts"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "the permanent-contexts migration is a built-in migration"
    );
    let store = crate::store::tests::open_named(&home, &earlier).unwrap();
    (dir, home, store)
}

/// The fixture of the Job: `eec-codereview` (live, owner missionlane, a Manager, two Plans, a running Job with
/// its Attempt and Worker, records with links, curations, retirements, inbox items, a board post, Events), an
/// archived `host` (13 records), a live `host-2` (7 records and a decision), an archived `cpd`, an archived
/// `old`, a live `other`; and, with `codereview`, an archived `codereview` that has its own live Manager,
/// an archived Plan and records `box-n1..n4`, `box-d1`, `box-q1`. `root` is the cwd of the sessions that are
/// "in the CPD_HOME".
fn fixture(store: &mut Store, root: &str, codereview: bool, host_owner: &str) {
    let mut sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on, archived_at, owner) VALUES
           ('eec-codereview', 'eec-codereview', 't', 'box', NULL, 'missionlane'),
           ('host', 'Host', 't', 'box', 't1', '{host_owner}'),
           ('host-2', 'Host Two', 't', 'box', NULL, 'missionlane'),
           ('cpd', 'Cpd', 't', 'box', 't1', 'clayton-duarte'),
           ('old', 'Old', 't', 'box', 't1', 'clayton-duarte'),
           ('other', 'Other', 't', 'box', NULL, 'clayton-duarte');\n"
    );
    // eec-codereview: sessions, plans, a running job
    sql += &session(
        "ecm",
        "manager",
        "eec-codereview",
        None,
        "eec-codereview-manager",
        root,
        false,
    );
    sql += &session(
        "ecl",
        "lead",
        "eec-codereview",
        Some("ecp"),
        "eec-codereview",
        root,
        false,
    );
    sql += &session(
        "ecl2",
        "lead",
        "eec-codereview",
        Some("ecq"),
        "Lead eec-codereview",
        "/work",
        false,
    );
    sql += &session(
        "ecw",
        "worker",
        "eec-codereview",
        Some("ecp"),
        "eec-codereview:ecp:w",
        root,
        false,
    );
    sql += &plan("ecp", "eec-codereview", "dispatch", "ecl", false);
    sql += &plan("ecq", "eec-codereview", "grounding", "ecl2", false);
    sql += "UPDATE sessions SET file = '/t/lead.jsonl' WHERE id = 'ecl';
        INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
          VALUES ('ecp', 'wf', '/r', 'main', 'waiting', 'ecp/wf', cpd_txn(), 't');
        INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status,
                          appended, fingerprint, created_txn, created_at)
          VALUES ('ecp', 'wf', 'j1', 'J', 'S2', 'g', 'eec-codereview', '[\"x\"]', '[\"y\"]', '[]', 'running', 0,
                  hex(zeroblob(32)), cpd_txn(), 't');
        INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status,
                          appended, cancel_reason, cancel_requested_at, created_txn, created_at)
          VALUES ('ecp', 'wf', 'j2', 'J2', 'S2', 'g', 'prose about eec-codereview', '[\"x\"]', '[\"y\"]', '[]',
                  'cancelled', 0, 'gone', 't', cpd_txn(), 't');
        INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
          VALUES ('ecp', 'j1-a1', 'j1', 1, 'S2', 'ladder', 'running', 'ecp/wf-j1', '/w', 'ecw', 't');\n";
    // eec-codereview: records and links
    sql += &record("eec-codereview", "box-n1", "note", "e note 1", None, None);
    sql += &record("eec-codereview", "box-n2", "note", "e note 2", None, None);
    sql += &record(
        "eec-codereview",
        "box-n3",
        "note",
        "e note 3",
        Some("box-n1"),
        None,
    );
    sql += &record(
        "eec-codereview",
        "box-d1",
        "decision",
        "e decision 1",
        None,
        None,
    );
    sql += &record(
        "eec-codereview",
        "box-q1",
        "question",
        "e question 1",
        None,
        None,
    );
    sql += &record(
        "eec-codereview",
        "box-a1",
        "answer",
        "e answer 1",
        None,
        Some("box-q1"),
    );
    sql += "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at) VALUES
          ('eec-codereview', 'box-n1', 'superseded', 'box-n3', 'pending', 't'),
          ('eec-codereview', 'box-n2', 'duplicate', 'box-d1', 'pending', 't'),
          ('eec-codereview', 'box-n3', 'documented', 'docs/box-n1.md', 'pending', 't');
        INSERT INTO record_curations (context, record, topic, short, curated_at) VALUES
          ('eec-codereview', 'box-q1', 'topic', 'short', 't'),
          ('eec-codereview', 'box-a1', 'topic', 'short', 't');
        INSERT INTO inbox_items (kind, urgency, subject, context, record, label, opened_at) VALUES
          ('curate-retire', 'blue', 's1', 'eec-codereview', 'box-n1', 'l', 't');
        INSERT INTO inbox_items (kind, urgency, subject, context, plan, job, label, opened_at) VALUES
          ('job-stuck', 'red', 's2', 'eec-codereview', 'ecp', 'j1', 'l', 't');
        INSERT INTO board_posts (at, context, from_session, to_session, text)
          VALUES ('t', 'eec-codereview', 'ecm', 'ecl', 'hello eec-codereview');
        INSERT INTO events (txn, at, kind, context, text, data) VALUES
          (cpd_txn(), 't', 'context-record', 'eec-codereview', 'note box-n1 recorded in Context eec-codereview', '{\"record\":\"box-n1\"}');
        INSERT INTO events (txn, at, kind, context, plan, session, text, data) VALUES
          (cpd_txn(), 't', 'context-record', 'eec-codereview', 'ecp', 'ecl', 'plan ecp of eec-codereview', '{\"context\":\"eec-codereview\"}');\n";
    // host: archived Manager, a done archived Plan with its archived Lead, 13 notes
    sql += &session("hm", "manager", "host", None, "host-manager", "/work", true);
    sql += &session("hl", "lead", "host", Some("hp"), "Lead", "/work", true);
    sql += &plan("hp", "host", "done", "hl", true);
    for n in 1..=13 {
        sql += &record(
            "host",
            &format!("box-n{n}"),
            "note",
            &format!("host note {n}"),
            None,
            None,
        );
    }
    // host-2: live Manager, a Plan with Leads of every name shape, 7 notes (seq order n1..n5, n7, n6) and a decision
    sql += &session(
        "h2m",
        "manager",
        "host-2",
        None,
        "host-2-manager",
        "/elsewhere",
        false,
    );
    sql += &session(
        "h2l",
        "lead",
        "host-2",
        Some("h2p"),
        "host-2",
        "/work",
        false,
    );
    sql += &session(
        "h2f",
        "lead",
        "host-2",
        Some("h2p"),
        "Manager host-2",
        "/work",
        false,
    );
    sql += &session(
        "h2x",
        "lead",
        "host-2",
        Some("h2p"),
        "host-2:h2p",
        "/work",
        false,
    );
    sql += &session(
        "h2y",
        "lead",
        "host-2",
        Some("h2p"),
        "Reviewer",
        "/work",
        false,
    );
    sql += &session(
        "h2z",
        "lead",
        "host-2",
        Some("h2p"),
        "host-2x",
        "/work",
        false,
    );
    sql += "UPDATE sessions SET forked_from = 'h2l' WHERE id IN ('h2f', 'h2x', 'h2y', 'h2z');\n";
    sql += &plan("h2p", "host-2", "dispatch", "h2l", false);
    for n in [1, 2, 3, 4, 5, 7, 6] {
        let supersedes = (n == 2).then_some("box-n1");
        sql += &record(
            "host-2",
            &format!("box-n{n}"),
            "note",
            &format!("h2 note {n}"),
            supersedes,
            None,
        );
    }
    sql += &record("host-2", "box-d1", "decision", "h2 decision 1", None, None);
    // cpd and old: archived, with archived Manager, Plan and Lead
    sql += &session("cpm", "manager", "cpd", None, "cpd-manager", "/work", true);
    sql += &session("cpl", "lead", "cpd", Some("cpp"), "Lead", "/work", true);
    sql += &plan("cpp", "cpd", "done", "cpl", true);
    sql += &session("oldm", "manager", "old", None, "old-manager", "/work", true);
    // other: outside the three, a Manager and a Lead in the CPD_HOME
    sql += &session("om", "manager", "other", None, "other-manager", root, false);
    sql += &session("ol", "lead", "other", Some("op"), "Lead", root, false);
    sql += &plan("op", "other", "dispatch", "ol", false);
    sql += "UPDATE sessions SET file = '/t/other.jsonl' WHERE id = 'ol';\n";
    if codereview {
        sql += "INSERT INTO contexts (id, title, created_at, created_on, archived_at, owner)
                  VALUES ('codereview', 'Codereview', 't', 'box', 't1', 'clayton-duarte');\n";
        sql += &session(
            "crm",
            "manager",
            "codereview",
            None,
            "codereview-manager",
            "/work",
            false,
        );
        sql += &session(
            "crl",
            "lead",
            "codereview",
            Some("crp"),
            "Lead",
            "/work",
            true,
        );
        sql += &plan("crp", "codereview", "done", "crl", true);
        for n in 1..=4 {
            sql += &record(
                "codereview",
                &format!("box-n{n}"),
                "note",
                &format!("cr note {n}"),
                None,
                None,
            );
        }
        sql += &record(
            "codereview",
            "box-d1",
            "decision",
            "cr decision 1",
            None,
            None,
        );
        sql += &record(
            "codereview",
            "box-q1",
            "question",
            "cr question 1",
            None,
            None,
        );
    }
    write(store, &sql).unwrap();
}

/// A store at the prefix with the fixture, still open.
fn old_open(codereview: bool, host_owner: &str) -> (tempfile::TempDir, CpdHome, Store) {
    let (dir, home, mut store) = before();
    let root = home.root().to_str().unwrap().to_owned();
    fixture(&mut store, &root, codereview, host_owner);
    (dir, home, store)
}

/// A store at the prefix with the fixture, closed.
fn old_store(codereview: bool) -> (tempfile::TempDir, CpdHome) {
    let (dir, home, store) = old_open(codereview, "clayton-duarte");
    drop(store);
    (dir, home)
}

/// The new binary opens the old store and applies the migration.
fn migrated(home: &CpdHome) -> Store {
    let mut store = Store::open(home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert!(
        rows(&mut store, "PRAGMA foreign_key_check").is_empty(),
        "no dangling reference"
    );
    store
}

/// Every table and column that holds a Context id (scout-migrate §1), the Context row aside.
const CONTEXT_COLUMNS: [(&str, &str); 9] = [
    ("events", "context"),
    ("context_records", "context"),
    ("plans", "context"),
    ("jobs", "context"),
    ("sessions", "context"),
    ("inbox_items", "context"),
    ("board_posts", "context"),
    ("record_retirements", "context"),
    ("record_curations", "context"),
];

fn per_context(store: &mut Store, context: &str) -> Vec<i64> {
    CONTEXT_COLUMNS
        .iter()
        .map(|(t, c)| {
            count(
                store,
                &format!("SELECT count(*) FROM {t} WHERE {c} = '{context}'"),
            )
        })
        .collect()
}

fn history_rows(store: &mut Store) -> Vec<Value> {
    rows(
        store,
        "SELECT id, tbl, pk, op, before, after FROM history ORDER BY id",
    )
}

#[test]
fn eec_codereview_moves_to_codereview_in_every_table() {
    let (_dir, home, mut pre) = old_open(false, "clayton-duarte");
    let before = per_context(&mut pre, "eec-codereview");
    drop(pre);
    assert!(
        before.iter().all(|n| *n > 0),
        "the fixture fills every table: {before:?}"
    );

    let mut store = migrated(&home);
    assert_eq!(per_context(&mut store, "codereview"), before);
    assert_eq!(per_context(&mut store, "eec-codereview"), vec![0; 9]);
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, title, owner, archived_at, deleted_at FROM contexts WHERE id IN ('codereview', 'eec-codereview')"
        ),
        vec![json!([
            "codereview",
            "codereview",
            "missionlane",
            null,
            null
        ])]
    );
    // no collision: the record ids and links stay, the Manager stays live and keeps its name
    assert_eq!(
        strings(
            &mut store,
            "SELECT id FROM context_records WHERE context = 'codereview' ORDER BY seq"
        ),
        ["box-n1", "box-n2", "box-n3", "box-d1", "box-q1", "box-a1"]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, name, archived_at FROM sessions WHERE role = 'manager' AND context = 'codereview'"
        ),
        vec![json!(["ecm", "eec-codereview-manager", null])]
    );
    // a Job's prose that mentions the id is not a Context id
    assert_eq!(
        text(&mut store, "SELECT context FROM jobs WHERE key = 'j2'"),
        "prose about eec-codereview"
    );
}

fn snapshot(store: &mut Store) -> Vec<Value> {
    let mut all = Vec::new();
    for table in [
        "contexts",
        "events",
        "context_records",
        "plans",
        "jobs",
        "sessions",
        "inbox_items",
        "board_posts",
        "record_retirements",
        "record_curations",
        "history",
    ] {
        let names = rows(
            store,
            &format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid"),
        );
        let columns: Vec<&str> = names.iter().map(|n| n[0].as_str().unwrap()).collect();
        let sql = format!("SELECT {} FROM {table}", columns.join(", "));
        all.push(Value::from(rows(store, &sql)));
    }
    all
}

#[test]
fn the_move_keeps_every_record_every_event_and_the_append_only_history() {
    let (_dir, home, mut pre) = old_open(false, "clayton-duarte");
    let tables = [
        "contexts",
        "events",
        "context_records",
        "plans",
        "jobs",
        "sessions",
        "inbox_items",
        "board_posts",
        "record_retirements",
        "record_curations",
        "workflows",
        "attempts",
    ];
    let counts: Vec<i64> = tables
        .iter()
        .map(|t| count(&mut pre, &format!("SELECT count(*) FROM {t}")))
        .collect();
    let events = rows(&mut pre, "SELECT id, text, data FROM events ORDER BY id");
    let history = history_rows(&mut pre);
    drop(pre);

    let mut store = migrated(&home);
    // the move happened ...
    assert_eq!(per_context(&mut store, "eec-codereview"), vec![0; 9]);
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM events WHERE context = 'codereview'"
        ),
        2
    );
    // ... and lost no row
    let after: Vec<i64> = tables
        .iter()
        .map(|t| count(&mut store, &format!("SELECT count(*) FROM {t}")))
        .collect();
    assert_eq!(after, counts);
    // Event text and data keep the old id
    assert_eq!(
        rows(&mut store, "SELECT id, text, data FROM events ORDER BY id"),
        events
    );
    // history rows are never rewritten: the old ones are all there, unchanged, with the old id in them
    let now = history_rows(&mut store);
    assert_eq!(now[..history.len()], history[..]);
    assert!(
        count(
            &mut store,
            "SELECT count(*) FROM history WHERE tbl = 'contexts' AND (pk LIKE '%eec-codereview%' OR after LIKE '%eec-codereview%')"
        ) > 0
    );
}

#[test]
fn the_move_carries_a_running_job_past_the_jobs_locked_trigger() {
    let (_dir, home) = old_store(false);
    let mut store = migrated(&home);
    assert_eq!(
        rows(
            &mut store,
            "SELECT status, context FROM jobs WHERE key = 'j1'"
        ),
        vec![json!(["running", "codereview"])]
    );
    assert_eq!(
        rows(&mut store, "SELECT id, job, status FROM attempts"),
        vec![json!(["j1-a1", "j1", "running"])]
    );
    // the guard is back: a running Job's locked fields still refuse a change
    let refused = write(&mut store, "UPDATE jobs SET context = 'x' WHERE key = 'j1'").unwrap_err();
    let message = refused.details()["sqlite_message"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(message.contains("job_locked"), "{refused:?}");
}

#[test]
fn moving_into_an_existing_codereview_merges_instead_of_aborting() {
    let (_dir, home) = old_store(true);
    let mut store = migrated(&home);
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, title, owner, archived_at, deleted_at IS NOT NULL FROM contexts WHERE id IN ('codereview', 'eec-codereview') ORDER BY id"
        ),
        vec![
            json!(["codereview", "Codereview", "clayton-duarte", null, 0]),
            json!(["eec-codereview", "eec-codereview", "missionlane", null, 1]),
        ]
    );
    // nothing is left under the tombstone, everything is under `codereview`
    assert_eq!(per_context(&mut store, "eec-codereview"), vec![0; 9]);
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM context_records WHERE context = 'codereview'"
        ),
        12
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM context_records WHERE context = 'codereview' AND text LIKE 'e %'"
        ),
        6
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM context_records WHERE context = 'codereview' AND text LIKE 'cr %'"
        ),
        6
    );
    assert_eq!(
        strings(
            &mut store,
            "SELECT id FROM plans WHERE context = 'codereview' ORDER BY id"
        ),
        ["crp", "ecp", "ecq"]
    );
    // the Manager seat: the incoming Manager is live, the old live one is archived (at the transaction time)
    assert_eq!(
        strings(
            &mut store,
            "SELECT id FROM sessions WHERE role = 'manager' AND context = 'codereview' AND archived_at IS NULL"
        ),
        ["ecm"]
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM sessions WHERE id = 'crm' AND archived_at IS NOT NULL AND context = 'codereview'"
        ),
        1
    );
}

#[test]
fn host_2_merges_into_the_archived_host_and_host_is_unarchived() {
    let (_dir, home) = old_store(false);
    let mut store = migrated(&home);
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, archived_at, deleted_at IS NOT NULL FROM contexts WHERE id IN ('host', 'host-2') ORDER BY id"
        ),
        vec![json!(["host", null, 0]), json!(["host-2", null, 1])]
    );
    assert_eq!(per_context(&mut store, "host-2"), vec![0; 9]);
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM context_records WHERE context = 'host'"
        ),
        13 + 7 + 1
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM context_records WHERE context = 'host' AND text LIKE 'host note %'"
        ),
        13
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM context_records WHERE context = 'host' AND text LIKE 'h2 %'"
        ),
        8
    );
    assert_eq!(
        strings(
            &mut store,
            "SELECT id FROM plans WHERE context = 'host' ORDER BY id"
        ),
        ["h2p", "hp"]
    );
}

#[test]
fn the_merged_host_keeps_host_2s_manager_live_and_archives_the_old_one() {
    let (_dir, home) = old_store(false);
    let mut store = migrated(&home);
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, name FROM sessions WHERE role = 'manager' AND context = 'host' AND archived_at IS NULL"
        ),
        vec![json!(["h2m", "host-manager"])]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, name, archived_at FROM sessions WHERE id = 'hm'"
        ),
        vec![json!(["hm", "host-manager", "t1"])]
    );
    assert_eq!(
        strings(
            &mut store,
            "SELECT context FROM sessions WHERE id IN ('hm', 'h2m') ORDER BY id"
        ),
        ["host", "host"]
    );
}

#[test]
fn the_merged_host_is_owned_by_clayton_duarte_and_keeps_its_title() {
    // whoever owned `host` before, and with `codereview` keeping its own owner
    let (_dir, home, store) = old_open(true, "missionlane");
    drop(store);
    let mut store = migrated(&home);
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, title, owner FROM contexts WHERE id IN ('host', 'codereview') ORDER BY id"
        ),
        vec![
            json!(["codereview", "Codereview", "clayton-duarte"]),
            json!(["host", "Host", "clayton-duarte"]),
        ]
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM contexts WHERE id = 'host-2' AND deleted_at IS NOT NULL"
        ),
        1
    );
}

#[test]
fn a_colliding_record_id_gets_the_next_free_id_in_seq_order_and_only_it() {
    let (_dir, home) = old_store(false);
    let mut store = migrated(&home);
    // host-2's notes were written n1..n5, n7, n6: they take n14..n20 in that order
    for (text, id) in [
        ("h2 note 1", "box-n14"),
        ("h2 note 2", "box-n15"),
        ("h2 note 3", "box-n16"),
        ("h2 note 4", "box-n17"),
        ("h2 note 5", "box-n18"),
        ("h2 note 7", "box-n19"),
        ("h2 note 6", "box-n20"),
        // no collision: the id stays
        ("h2 decision 1", "box-d1"),
    ] {
        assert_eq!(text_of(&mut store, "host", text), id, "{text}");
    }
    // host's own ids and their order are untouched
    assert_eq!(
        strings(
            &mut store,
            "SELECT id FROM context_records WHERE context = 'host' AND text LIKE 'host note %' ORDER BY seq"
        ),
        (1..=13).map(|n| format!("box-n{n}")).collect::<Vec<_>>()
    );
    // host-2's link followed its target (n2 superseded n1)
    assert_eq!(
        text(
            &mut store,
            "SELECT supersedes FROM context_records WHERE context = 'host' AND text = 'h2 note 2'"
        ),
        "box-n14"
    );
}

fn text_of(store: &mut Store, context: &str, body: &str) -> String {
    text(
        store,
        &format!("SELECT id FROM context_records WHERE context = '{context}' AND text = '{body}'"),
    )
}

#[test]
fn a_renumbered_record_is_re_pointed_in_every_column_that_holds_a_record_id() {
    let (_dir, home) = old_store(true);
    let mut store = migrated(&home);
    // codereview has n1..n4, d1, q1: eec's n1, n2, n3, d1, q1 collide and take n5, n6, n7, d2, q2; a1 stays
    for (body, id) in [
        ("e note 1", "box-n5"),
        ("e note 2", "box-n6"),
        ("e note 3", "box-n7"),
        ("e decision 1", "box-d2"),
        ("e question 1", "box-q2"),
        ("e answer 1", "box-a1"),
        ("cr note 1", "box-n1"),
        ("cr question 1", "box-q1"),
    ] {
        assert_eq!(text_of(&mut store, "codereview", body), id, "{body}");
    }
    // context_records.supersedes and .question
    assert_eq!(
        text(
            &mut store,
            "SELECT supersedes FROM context_records WHERE context = 'codereview' AND text = 'e note 3'"
        ),
        "box-n5"
    );
    assert_eq!(
        text(
            &mut store,
            "SELECT question FROM context_records WHERE context = 'codereview' AND text = 'e answer 1'"
        ),
        "box-q2"
    );
    // record_retirements.record and .target (a `documented` target is not a record id)
    assert_eq!(
        rows(
            &mut store,
            "SELECT record, reason, target FROM record_retirements WHERE context = 'codereview' ORDER BY record"
        ),
        vec![
            json!(["box-n5", "superseded", "box-n7"]),
            json!(["box-n6", "duplicate", "box-d2"]),
            json!(["box-n7", "documented", "docs/box-n1.md"]),
        ]
    );
    // record_curations.record
    assert_eq!(
        strings(
            &mut store,
            "SELECT record FROM record_curations WHERE context = 'codereview' ORDER BY record"
        ),
        ["box-a1", "box-q2"]
    );
    // inbox_items.record
    assert_eq!(
        rows(
            &mut store,
            "SELECT subject, record FROM inbox_items WHERE context = 'codereview' ORDER BY subject"
        ),
        vec![json!(["s1", "box-n5"]), json!(["s2", null])]
    );
    // Event data is not rewritten
    assert_eq!(
        text(
            &mut store,
            "SELECT data FROM events WHERE text LIKE 'note box-n1 recorded%'"
        ),
        "{\"record\":\"box-n1\"}"
    );
}

#[test]
fn cpd_and_codereview_are_unarchived_but_their_plans_and_sessions_are_not() {
    let (_dir, home) = old_store(true);
    let mut store = migrated(&home);
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, archived_at FROM contexts WHERE id IN ('cpd', 'host', 'codereview', 'old', 'other') ORDER BY id"
        ),
        vec![
            json!(["codereview", null]),
            json!(["cpd", null]),
            json!(["host", null]),
            json!(["old", "t1"]),
            json!(["other", null]),
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, archived_at FROM plans WHERE id IN ('cpp', 'crp', 'hp') ORDER BY id"
        ),
        vec![
            json!(["cpp", "t1"]),
            json!(["crp", "t1"]),
            json!(["hp", "t1"])
        ]
    );
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, archived_at FROM sessions WHERE id IN ('cpm', 'cpl', 'crl', 'hm', 'hl') ORDER BY id"
        ),
        vec![
            json!(["cpl", "t1"]),
            json!(["cpm", "t1"]),
            json!(["crl", "t1"]),
            json!(["hl", "t1"]),
            json!(["hm", "t1"]),
        ]
    );
}

#[test]
fn a_moved_sessions_name_follows_the_new_context_id() {
    let (_dir, home) = old_store(true);
    let mut store = migrated(&home);
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, context, name FROM sessions WHERE id IN
               ('ecm', 'ecl', 'ecl2', 'ecw', 'h2m', 'h2l', 'h2f', 'h2x', 'h2y', 'h2z', 'om') ORDER BY id"
        ),
        vec![
            json!(["ecl", "codereview", "codereview"]),
            json!(["ecl2", "codereview", "Lead eec-codereview"]),
            json!(["ecm", "codereview", "codereview-manager"]),
            json!(["ecw", "codereview", "codereview:ecp:w"]),
            json!(["h2f", "host", "Manager host"]),
            json!(["h2l", "host", "host"]),
            json!(["h2m", "host", "host-manager"]),
            json!(["h2x", "host", "host:h2p"]),
            json!(["h2y", "host", "Reviewer"]),
            json!(["h2z", "host", "host-2x"]),
            json!(["om", "other", "other-manager"]),
        ]
    );
}

#[test]
fn manager_and_lead_rows_with_the_cpd_home_cwd_move_to_the_users_home() {
    // read, never set: no test changes the process environment
    #[allow(deprecated)]
    let user = std::env::home_dir().expect("the test process has a HOME");
    let user = user
        .canonicalize()
        .unwrap_or(user)
        .to_string_lossy()
        .into_owned();
    let (_dir, home) = old_store(false);
    let root = home.root().to_str().unwrap().to_owned();
    let mut store = migrated(&home);
    assert_eq!(
        rows(
            &mut store,
            "SELECT id, cwd, file FROM sessions WHERE id IN ('ecm', 'ecl', 'ecl2', 'ecw', 'h2m', 'om', 'ol') ORDER BY id"
        ),
        vec![
            // a Lead in a moved Context, with its transcript
            json!(["ecl", user, "/t/lead.jsonl"]),
            json!(["ecl2", "/work", null]),
            // a Manager in a moved Context
            json!(["ecm", user, null]),
            // a Worker in the CPD_HOME stays
            json!(["ecw", root, null]),
            // a Manager with another cwd stays
            json!(["h2m", "/elsewhere", null]),
            // a Manager and a Lead of a Context outside the three move too
            json!(["ol", user, "/t/other.jsonl"]),
            json!(["om", user, null]),
        ]
    );
}

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "x").unwrap();
}

#[test]
fn the_engine_index_files_are_removed_and_the_digest_directory_follows_the_id() {
    let engines = |home: &CpdHome| home.root().join("engines/holographic");
    let digests = |home: &CpdHome| home.root().join("run/context-digests");
    let seed = |home: &CpdHome| {
        for name in ["eec-codereview", "codereview", "host", "host-2", "other"] {
            touch(&engines(home).join(format!("{name}.db")));
        }
        touch(&digests(home).join("eec-codereview/batch.md"));
    };

    // `codereview` absent: the digest directory is renamed
    let (_dir, home) = old_store(false);
    seed(&home);
    drop(migrated(&home));
    for gone in ["eec-codereview", "codereview", "host", "host-2"] {
        assert!(
            !engines(&home).join(format!("{gone}.db")).exists(),
            "{gone}.db"
        );
    }
    assert!(engines(&home).join("other.db").exists());
    assert!(digests(&home).join("codereview/batch.md").exists());
    assert!(!digests(&home).join("eec-codereview").exists());

    // `codereview` has a digest directory: it is not overwritten, and the old one stays
    let (_dir, home) = old_store(true);
    seed(&home);
    touch(&digests(&home).join("codereview/own.md"));
    drop(migrated(&home));
    for gone in ["eec-codereview", "codereview", "host", "host-2"] {
        assert!(
            !engines(&home).join(format!("{gone}.db")).exists(),
            "{gone}.db"
        );
    }
    assert!(digests(&home).join("codereview/own.md").exists());
    assert!(!digests(&home).join("codereview/batch.md").exists());
    assert!(digests(&home).join("eec-codereview/batch.md").exists());
}

// ---------- regression: these pass against the stub, they are not in the Job's tests ----------

#[test]
fn the_blocking_triggers_come_back_with_their_exact_sql() {
    let (_dir, home, mut pre) = old_open(true, "clayton-duarte");
    let sql = "SELECT name, sql FROM sqlite_master WHERE type = 'trigger' AND name IN ('events_no_update', 'context_records_immutable', 'jobs_locked') ORDER BY name";
    let triggers = rows(&mut pre, sql);
    for name in [
        "events_no_update",
        "context_records_immutable",
        "jobs_locked",
    ] {
        assert!(triggers.iter().any(|t| t[0] == name), "{name}");
    }
    drop(pre);
    let mut store = migrated(&home);
    assert_eq!(rows(&mut store, sql), triggers);
}

#[test]
fn a_store_with_none_of_these_contexts_is_unchanged_and_gets_no_context() {
    let (_dir, home, mut pre) = before();
    let history = history_rows(&mut pre);
    drop(pre);
    let mut store = migrated(&home);
    assert_eq!(count(&mut store, "SELECT count(*) FROM contexts"), 0);
    assert_eq!(count(&mut store, "SELECT count(*) FROM events"), 0);
    assert_eq!(count(&mut store, "SELECT count(*) FROM sessions"), 0);
    assert_eq!(history_rows(&mut store), history);
}

#[test]
fn a_second_open_changes_nothing() {
    let (_dir, home) = old_store(true);
    let mut store = migrated(&home);
    let data = snapshot(&mut store);
    let applied = crate::store::tests::applied(&mut store);
    drop(store);
    let mut again = migrated(&home);
    assert_eq!(crate::store::tests::applied(&mut again), applied);
    assert_eq!(snapshot(&mut again), data);
}
