//! The table `job_addenda` and the Event kind `job-addendum` (contract §3.4 "Addenda", §4.4, §4.5; migration
//! `20261103T0400-job-addenda`; decision pro-d294): append-only, one row per `cpd job addendum`, with history.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn message(result: Result<(), CpdError>, what: &str) -> String {
    result.expect_err(what).details()["sqlite_message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn count(store: &mut Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get::<_, i64>(0))?))
        .unwrap()
}

/// Every row of `table` as one text each, in rowid order.
fn dump(store: &mut Store, table: &str) -> Vec<String> {
    store
        .read(|c| {
            let columns: Vec<String> = table_columns(c, table)?
                .into_iter()
                .map(|(name, _)| format!("\"{name}\""))
                .collect();
            let mut s = c.prepare(&format!(
                "SELECT json_array({}) FROM \"{table}\" ORDER BY rowid",
                columns.join(", ")
            ))?;
            let rows = s
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap()
}

/// Plan `pay` (locked), its PR `r`, a Job `api` running its Attempt `api-a1` with the Worker Session `w`, and a `queued` Job `idle`.
fn world() -> String {
    format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at) VALUES
           ('l', 'lead', 'ctx', 'pay', NULL, 'L', '/', 'O3', 'idle', 't', 't'),
           ('w', 'worker', 'ctx', 'pay', 'api-a1', 'W', '/', 'S2', 'working', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'l', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay/r', cpd_txn(), 't');
         INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
           VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{FP}', cpd_txn(), 't'),
                  ('pay', 'r', 'idle', 'Idle', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, '{FP}', cpd_txn(), 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
           VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 'w', 't');"
    )
}

fn row(job: &str, number: i64, attempt: &str, session: &str, text: &str) -> String {
    format!(
        "INSERT INTO job_addenda (plan, job, number, attempt, session, text, created_at) VALUES ('pay', '{job}', {number}, {attempt}, {session}, '{text}', 't');"
    )
}

fn store_with_world() -> (tempfile::TempDir, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    // The fully migrated store holds the renamed tables: `world()` is written as the pre-rename store held it
    // and the names follow `workflow-rename`.
    write(
        &mut store,
        &world()
            .replace("INSERT INTO prs (", "INSERT INTO workflows (")
            .replace(
                "INSERT INTO jobs (plan, pr,",
                "INSERT INTO jobs (plan, workflow,",
            ),
    )
    .unwrap();
    (dir, store)
}

fn before_job_addenda() -> (tempfile::TempDir, CpdHome) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-job-addenda"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "20261103T0400-job-addenda is a built-in migration"
    );
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    write(&mut store, &world()).unwrap();
    drop(store);
    (dir, home)
}

#[test]
fn a_database_before_job_addenda_migrates_to_an_empty_table_and_accepts_the_new_event_kind() {
    let (_dir, home) = before_job_addenda();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-job-addenda"))
        .collect();
    let mut old = crate::store::tests::open_named(&home, &earlier).unwrap();
    let (jobs, attempts, events, sessions) = (
        dump(&mut old, "jobs"),
        dump(&mut old, "attempts"),
        dump(&mut old, "events"),
        dump(&mut old, "sessions"),
    );
    // The later migration `agent-key-backfill` rewrites the names of the Lead and the Worker to their keys.
    let sessions: Vec<String> = sessions
        .iter()
        .map(|row| {
            row.replace("\"L\"", "\"ctx:pay\"")
                .replace("\"W\"", "\"ctx:pay:api\"")
        })
        .collect();
    let kinds_before = count(&mut old, "SELECT count(*) FROM event_kinds");
    drop(old);

    // The migration itself adds exactly its kind: the count at its own prefix, before the later migrations add
    // theirs.
    let at = NAMED
        .iter()
        .position(|n| n.name.ends_with("-job-addenda"))
        .expect("the job-addenda migration is declared");
    let mut prefix = crate::store::tests::open_named(&home, &NAMED[..=at]).unwrap();
    assert_eq!(
        count(&mut prefix, "SELECT count(*) FROM event_kinds"),
        kinds_before + 1
    );
    assert_eq!(
        count(
            &mut prefix,
            "SELECT count(*) FROM event_kinds WHERE name = 'job-addendum'"
        ),
        1
    );
    drop(prefix);

    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'job_addenda'"
        ),
        1
    );
    assert_eq!(count(&mut store, "SELECT count(*) FROM job_addenda"), 0);
    write(&mut store, "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), 't', 'job-addendum', 'x', '{\"text\":\"t\"}');")
        .expect("an Event of kind job-addendum is insertable");
    assert_eq!(dump(&mut store, "jobs"), jobs);
    assert_eq!(dump(&mut store, "attempts"), attempts);
    assert_eq!(dump(&mut store, "sessions"), sessions);
    assert_eq!(
        dump(&mut store, "events")[..events.len()],
        events[..],
        "every earlier Event is unchanged"
    );
}

#[test]
fn job_addenda_refuses_an_update_a_repeated_number_a_blank_text_and_an_attempt_without_a_session() {
    let (_dir, mut store) = store_with_world();
    let bad = |sql: String, what: &str, store: &mut Store| {
        let text = message(write(store, &sql), what);
        assert!(
            text.contains("CHECK") || text.contains("UNIQUE") || text.contains("FOREIGN KEY"),
            "{what}: {text}"
        );
    };
    bad(
        row("api", 0, "'api-a1'", "'w'", "t"),
        "number 0",
        &mut store,
    );
    bad(
        row("api", 1, "'api-a1'", "'w'", ""),
        "a blank text",
        &mut store,
    );
    bad(
        row("api", 1, "'api-a1'", "'w'", &"x".repeat(20001)),
        "a text of 20001 characters",
        &mut store,
    );
    bad(
        row("api", 1, "'api-a1'", "NULL", "t"),
        "an attempt without a session",
        &mut store,
    );
    bad(
        row("api", 1, "NULL", "'w'", "t"),
        "a session without an attempt",
        &mut store,
    );
    assert_eq!(count(&mut store, "SELECT count(*) FROM job_addenda"), 0);

    write(
        &mut store,
        &row("api", 1, "'api-a1'", "'w'", &"x".repeat(20000)),
    )
    .expect("both set, 20000 characters");
    write(&mut store, &row("idle", 1, "NULL", "NULL", "y")).expect("both NULL");
    assert_eq!(count(&mut store, "SELECT count(*) FROM job_addenda"), 2);
    let text = message(
        write(&mut store, &row("api", 1, "'api-a1'", "'w'", "again")),
        "the same plan, job and number",
    );
    assert!(
        text.contains("UNIQUE") || text.contains("PRIMARY KEY"),
        "{text}"
    );

    let text = message(
        write(
            &mut store,
            "UPDATE job_addenda SET text = 'z' WHERE job = 'idle'",
        ),
        "an update",
    );
    assert_eq!(text, "append_only: job_addenda");
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM job_addenda WHERE text = 'y'"
        ),
        1
    );
}

#[test]
fn job_addenda_has_history_triggers_and_refuses_a_delete() {
    let (_dir, mut store) = store_with_world();
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM history_tables WHERE name = 'job_addenda'"
        ),
        1
    );
    write(&mut store, &row("idle", 1, "NULL", "NULL", "y")).unwrap();
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM history WHERE tbl = 'job_addenda' AND op = 'insert'"
        ),
        1
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'job_addenda' AND name LIKE '%hist%'"
        ),
        3,
        "an insert, an update and a delete history trigger"
    );
    let err = write(&mut store, "DELETE FROM job_addenda WHERE job = 'idle'")
        .expect_err("a delete of a live Plan's row");
    assert!(
        err.details()["sqlite_message"].as_str().is_some(),
        "{err:?}"
    );
    assert_eq!(count(&mut store, "SELECT count(*) FROM job_addenda"), 1);
}
