//! The migration `20261103T1100-workflow-reopen` (pro-d362 A1, A3, A6): a `cancelled` Workflow may keep its pull request identity and come back
//! `cancelled>open`, and its `done` Plan goes back `done>deploy`. Every other transition and CHECK stays as it was, and no row changes.
//! Stores here are built to the migration before it.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const MIGRATION: &str = "20261103T1100-workflow-reopen";

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

/// Every row of the tables the migration must leave alone, the columns named, as one text.
fn dump(store: &mut Store) -> String {
    let mut out = String::new();
    for t in [
        "contexts",
        "sessions",
        "plans",
        "pull_requests",
        "workflows",
        "events",
    ] {
        let names: Vec<String> = store
            .read(|c: &Connection| {
                let mut stmt = c.prepare(&format!(
                    "SELECT name FROM pragma_table_info('{t}') ORDER BY cid"
                ))?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
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

/// The migrations before `workflow-reopen`. Fails while the migration is missing.
fn earlier() -> Vec<Named> {
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-workflow-reopen"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "workflow-reopen is a built-in migration"
    );
    earlier
}

/// A Context `cpd` and one of `codereview`, a Lead each, a Plan in `deploy` (`pay`), one `done` (`fin`), a `codereview` one in `grounding` (`rev`), and a pull
/// request `api#7`. Workflows: `open-wf` `open` on it, `merged-wf` `merged`, `cancel-wf` `cancelled` with no pull request (the legacy shape).
const SEED: &str = "INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('codereview', 'Code review', 't', 'm');
     INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('cpd', 'Cpd', 't', 'm');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('l1', 'lead', 'cpd', 'pay', 'L1', '/', 'S3', 'idle', 't', 't');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('l2', 'lead', 'codereview', 'rev', 'L2', '/', 'S3', 'idle', 't', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
       VALUES ('pay', 'cpd', 'Pay', 'deploy', 'l1', 't', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
       VALUES ('fin', 'cpd', 'Fin', 'done', 'l1', 't', 't', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at)
       VALUES ('rev', 'codereview', 'Rev', 'grounding', 'l2', 't');
     INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
       VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'T', 0, 'closed', 'none', 'none', 't', 't');
     INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at)
       VALUES ('pay', 'open-wf', '/r/api', 'main', 'open', 'cpd/pay/open-wf', '/w/open', 7, 'https://github.com/acme/api/pull/7', 't',
               (SELECT id FROM pull_requests WHERE number = 7), cpd_txn(), 't', 't');
     INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at, merged_at)
       VALUES ('pay', 'merged-wf', '/r/api', 'main', 'merged', 'cpd/pay/merged-wf', '/w/merged', 8, 'https://github.com/acme/api/pull/8', 't',
               (SELECT id FROM pull_requests WHERE number = 7), cpd_txn(), 't', 't', 't');
     INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
       VALUES ('pay', 'cancel-wf', '/r/api', 'main', 'cancelled', 'cpd/pay/cancel-wf', cpd_txn(), 't');";

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

fn assert_illegal(result: Result<(), CpdError>, what: &str) {
    let text = format!("{result:?}");
    assert!(text.contains("illegal_transition"), "{what}: {text}");
}

fn assert_check_refused(result: Result<(), CpdError>, what: &str) {
    let text = format!("{result:?}");
    assert!(text.contains("CHECK constraint failed"), "{what}: {text}");
}

fn status(store: &mut Store, key: &str) -> String {
    store
        .read(|c: &Connection| {
            Ok(
                c.query_row("SELECT status FROM workflows WHERE key = ?1", [key], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
}

fn phase(store: &mut Store, id: &str) -> String {
    store
        .read(|c: &Connection| {
            Ok(c.query_row("SELECT phase FROM plans WHERE id = ?1", [id], |r| r.get(0))?)
        })
        .unwrap()
}

/// The `UPDATE` that reopens `key`: status `open` with the Lead worktree, start time and pull request identity back.
fn reopen(key: &str) -> String {
    format!(
        "UPDATE workflows SET status = 'open', lead_worktree = '/w/x', started_at = 't2', pr_number = 7,
           pr_url = 'https://github.com/acme/api/pull/7', pr_opened_at = 't',
           pull_request = (SELECT id FROM pull_requests WHERE number = 7) WHERE key = '{key}'"
    )
}

#[test]
fn workflow_reopen_migration_exists_and_is_breaking() {
    let (index, migration) = NAMED
        .iter()
        .enumerate()
        .find(|(_, n)| n.name == MIGRATION)
        .expect("workflow-reopen is a built-in migration");
    assert_eq!(migration.compat, Compat::Breaking);
    let after = LEGACY.len() + index;
    assert_eq!(after, 102);
    assert_eq!(
        migration.sql.lines().next().unwrap(),
        format!("-- cpd-migration: breaking after {after}")
    );
}

#[test]
fn workflow_reopen_allows_cancelled_to_open_only() {
    let (_dir, home) = before();
    let mut old = crate::store::tests::open_named(&home, &earlier()).unwrap();
    assert_illegal(
        write(&mut old, &reopen("cancel-wf")),
        "cancelled>open before the migration",
    );
    drop(old);

    let mut store = migrated_in(&home);
    write(&mut store, &reopen("cancel-wf")).expect("cancelled>open");
    assert_eq!(status(&mut store, "cancel-wf"), "open");
    assert_illegal(
        write(
            &mut store,
            "UPDATE workflows SET status = 'open', merged_at = NULL WHERE key = 'merged-wf'",
        ),
        "merged>open",
    );
    assert_eq!(status(&mut store, "merged-wf"), "merged");
    write(
        &mut store,
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'cancel-2', '/r/api', 'main', 'cancelled', 'cpd/pay/cancel-2', cpd_txn(), 't');",
    )
    .unwrap();
    assert_illegal(
        write(
            &mut store,
            "UPDATE workflows SET status = 'waiting' WHERE key = 'cancel-2'",
        ),
        "cancelled>waiting",
    );
    assert_eq!(status(&mut store, "cancel-2"), "cancelled");
}

#[test]
fn workflow_reopen_allows_done_to_deploy_and_keeps_the_other_phases() {
    let (_dir, home) = before();
    let mut old = crate::store::tests::open_named(&home, &earlier()).unwrap();
    assert_illegal(
        write(
            &mut old,
            "UPDATE plans SET phase = 'deploy', done_at = NULL WHERE id = 'fin'",
        ),
        "done>deploy before the migration",
    );
    drop(old);

    let mut store = migrated_in(&home);
    write(
        &mut store,
        "UPDATE plans SET phase = 'deploy', done_at = NULL WHERE id = 'fin'",
    )
    .expect("done>deploy with done_at NULL");
    assert_eq!(phase(&mut store, "fin"), "deploy");
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM plans WHERE id = 'fin' AND done_at IS NULL"
        ),
        1
    );

    // `grounding>done` stays for Context codereview only.
    write(
        &mut store,
        "UPDATE plans SET phase = 'done', locked_at = 't2', done_at = 't2' WHERE id = 'rev'",
    )
    .expect("codereview grounding>done");
    write(
        &mut store,
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('g-cpd', 'cpd', 'G', 'grounding', 'l1', 't');",
    )
    .unwrap();
    assert_illegal(
        write(
            &mut store,
            "UPDATE plans SET phase = 'done', locked_at = 't2', done_at = 't2' WHERE id = 'g-cpd'",
        ),
        "grounding>done of Context cpd",
    );

    // `done>plan` still aborts (a done Plan that is not `fin` any more: `rev`).
    assert_illegal(
        write(
            &mut store,
            "UPDATE plans SET phase = 'plan', done_at = NULL WHERE id = 'rev'",
        ),
        "done>plan",
    );
    assert_eq!(phase(&mut store, "rev"), "done");
}

#[test]
fn workflow_reopen_cancelled_row_may_keep_its_pull_request_but_not_its_worktree() {
    let (_dir, home) = before();
    let mut store = migrated_in(&home);
    let insert = |key: &str, wt: &str, started: &str, number: &str, url: &str, pr: &str| {
        format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at)
               VALUES ('pay', '{key}', '/r/api', 'main', '{}', 'cpd/pay/{key}', {wt}, {started}, {number}, {url}, {}, {pr}, cpd_txn(), 't')",
            if key.starts_with("m-") { "merged" } else if key.starts_with("o-") { "open" } else { "cancelled" },
            if number == "NULL" { "NULL" } else { "'t'" },
        )
    };
    let url = "'https://github.com/acme/api/pull/7'";
    let pr = "(SELECT id FROM pull_requests WHERE number = 7)";
    write(&mut store, &insert("c-kept", "NULL", "NULL", "7", url, pr))
        .expect("cancelled keeping its pull request identity");
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM workflows WHERE key = 'c-kept' AND pr_number = 7 AND pr_url IS NOT NULL AND pr_opened_at IS NOT NULL AND pull_request IS NOT NULL"
        ),
        1
    );
    assert_check_refused(
        write(&mut store, &insert("c-wt", "'/w/x'", "NULL", "7", url, pr)),
        "cancelled with a Lead worktree",
    );
    assert_check_refused(
        write(&mut store, &insert("c-st", "NULL", "'t'", "7", url, pr)),
        "cancelled with a start time",
    );
    // pr_number is set iff pr_url and pr_opened_at are.
    assert_check_refused(
        write(
            &mut store,
            &insert("c-nourl", "NULL", "NULL", "7", "NULL", pr),
        ),
        "pr_number without pr_url",
    );
    assert_check_refused(
        write(
            &mut store,
            &insert("c-nonum", "NULL", "NULL", "NULL", url, "NULL"),
        ),
        "pr_url without pr_number",
    );
    // `open` and `merged` still need pr_number and pull_request.
    assert_check_refused(
        write(
            &mut store,
            &insert("o-nopr", "'/w/y'", "'t'", "7", url, "NULL"),
        ),
        "open without pull_request",
    );
    assert_check_refused(
        write(
            &mut store,
            &insert("o-nonum", "'/w/y'", "'t'", "NULL", "NULL", pr),
        ),
        "open without pr_number",
    );
    assert_check_refused(
        write(
            &mut store,
            &insert("m-nopr", "'/w/y'", "'t'", "7", url, "NULL"),
        ),
        "merged without pull_request",
    );
}

#[test]
fn workflow_reopen_leaves_rows_unchanged_and_adds_the_event_kind() {
    let (_dir, home) = before();
    let mut old = crate::store::tests::open_named(&home, &earlier()).unwrap();
    // The later migrations rewrite nothing of the tables dumped here but the names of the two Leads (agent-key-backfill, already in `earlier`).
    let want = dump(&mut old);
    assert_eq!(
        count(
            &mut old,
            "SELECT count(*) FROM event_kinds WHERE name = 'workflow-reopened'"
        ),
        0
    );
    drop(old);
    let mut store = migrated_in(&home);
    assert_eq!(dump(&mut store), want);
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM event_kinds WHERE name = 'workflow-reopened'"
        ),
        1
    );
}
