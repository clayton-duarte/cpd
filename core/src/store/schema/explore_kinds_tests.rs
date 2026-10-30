//! The migration `explore-kinds` (Plan `explore-roles`, R1, R2, R7, R8): the Job kinds `scout` and `research`, whose
//! `jobs.pr` is NULL exactly for them, `jobs.summary` (NOT NULL exactly when an explore Job is `merged`), and the
//! Attempt row of an explore Job.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn message(result: Result<(), CpdError>, what: &str) -> String {
    result.expect_err(what).details()["sqlite_message"].as_str().unwrap_or_default().to_owned()
}

const BASE: &str = "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('lead', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at)
       VALUES ('pay', 'ctx', 'Plan', 'plan', 'lead', 't');
     INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
       VALUES ('pay', 'api', '/r', 'main', 'waiting', 'pay/api', cpd_txn(), 't');";

/// A fully migrated store with Plan `pay` and its PR `api`.
fn fresh() -> (tempfile::TempDir, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    write(&mut store, BASE).unwrap();
    (dir, store)
}

/// A queued Job row: `pr` is SQL (`NULL` or `'api'`).
fn job(key: &str, kind: &str, pr: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
         VALUES ('pay', {pr}, '{key}', 'T', 'S1', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, '{kind}', '{FP}', cpd_txn(), 't');"
    )
}

#[test]
fn migration_allows_a_null_pr_only_for_explore_kinds() {
    let (_dir, mut store) = fresh();
    write(&mut store, &job("s", "scout", "NULL")).expect("a scout Job belongs to the Plan, with no PR");
    write(&mut store, &job("r", "research", "NULL")).expect("a research Job belongs to the Plan, with no PR");
    let refused = message(write(&mut store, &job("w", "work", "NULL")), "a work Job with no PR");
    assert!(refused.contains("CHECK"), "{refused}");
    let refused = message(write(&mut store, &job("s2", "scout", "'api'")), "a scout Job with a PR");
    assert!(refused.contains("CHECK"), "{refused}");
    let refused = message(write(&mut store, &job("r2", "research", "'api'")), "a research Job with a PR");
    assert!(refused.contains("CHECK"), "{refused}");
    write(&mut store, &job("w2", "work", "'api'")).expect("a work Job keeps its PR");
}

#[test]
fn migration_allows_a_summary_only_on_a_merged_explore_job() {
    let (_dir, mut store) = fresh();
    write(&mut store, &job("s", "scout", "NULL")).expect("a scout Job");
    write(&mut store, &job("w", "work", "'api'")).expect("a work Job");
    write(&mut store, "UPDATE jobs SET status = 'running';").expect("both run");

    let refused = message(write(&mut store, "UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 's';"), "merged without a summary");
    assert!(refused.contains("CHECK"), "{refused}");
    let refused = message(write(&mut store, "UPDATE jobs SET summary = 'found it' WHERE key = 's';"), "a summary while running");
    assert!(refused.contains("CHECK"), "{refused}");
    let refused = message(write(&mut store, "UPDATE jobs SET summary = 'x' WHERE key = 'w';"), "a summary on a work Job");
    assert!(refused.contains("CHECK"), "{refused}");
    let refused = message(write(&mut store, "UPDATE jobs SET status = 'merged', merged_sha = 'abc', summary = 'x' WHERE key = 'w';"), "a merged work Job with a summary");
    assert!(refused.contains("CHECK"), "{refused}");

    write(&mut store, &format!("UPDATE jobs SET status = 'merged', merged_sha = '{HASH}', summary = 'found it' WHERE key = 's';"))
        .expect("a merged explore Job holds its summary and the artefact hash");
    write(&mut store, "UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'w';").expect("a merged work Job holds no summary");
    write(&mut store, &job("r", "research", "NULL")).unwrap();
    write(&mut store, "UPDATE jobs SET status = 'running' WHERE key = 'r';").unwrap();
    write(&mut store, "UPDATE jobs SET status = 'merged', merged_sha = 'any text at all', summary = 's' WHERE key = 'r';")
        .expect("a merged explore Job may hold any merged_sha");
}

#[test]
fn migration_keeps_every_existing_job_row_and_dependency() {
    let idx = NAMED.iter().position(|m| m.name.ends_with("-explore-kinds")).expect("the migration explore-kinds is declared");
    let (_dir, home) = crate::store::tests::tmp_home();
    let before: CpdHome = home.clone();
    let mut old = crate::store::tests::open_named(&before, &NAMED[..idx]).unwrap();
    write(&mut old, BASE).unwrap();
    write(
        &mut old,
        &format!(
            "{} {} {}
             INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'b', 'a');
             UPDATE jobs SET status = 'running' WHERE key = 'a';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
               VALUES ('pay', 'a-a1', 'a', 1, 'S1', 'ladder', 'running', 'pay/api', '/w', 't');",
            job("a", "work", "'api'"),
            job("b", "docs", "'api'"),
            job("c", "verify", "'api'")
        ),
    )
    .unwrap();
    let snapshot = |s: &mut Store| -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
        s.read(|c| {
            let col = |sql: &str| -> rusqlite::Result<Vec<String>> {
                c.prepare(sql)?.query_map([], |r| r.get::<_, String>(0))?.collect()
            };
            Ok((
                col("SELECT plan || '/' || key || '/' || kind || '/' || ifnull(pr, '') || '/' || status || '/' || ifnull(fingerprint, '') FROM jobs ORDER BY seq")?,
                col("SELECT plan || '/' || id || '/' || job || '/' || status FROM attempts ORDER BY id")?,
                col("SELECT plan || '/' || job || '/' || needs FROM dependencies ORDER BY 1")?,
                col("SELECT type || ':' || name FROM sqlite_schema WHERE tbl_name IN ('jobs', 'attempts', 'dependencies') AND name NOT LIKE 'sqlite_%' AND type IN ('index', 'trigger') AND name NOT LIKE 'history_%' ORDER BY 1")?,
            ))
        })
        .unwrap()
    };
    let (jobs, attempts, deps, objects) = snapshot(&mut old);
    assert_eq!((jobs.len(), attempts.len(), deps.len()), (3, 1, 1));
    drop(old);

    let mut new = Store::open(&home, 5000).unwrap();
    let (jobs2, attempts2, deps2, objects2) = snapshot(&mut new);
    assert_eq!((jobs, attempts, deps), (jobs2, attempts2, deps2), "every row survives");
    for o in &objects {
        assert!(objects2.contains(o), "{o} survives the rebuild");
    }
    let violations: i64 = new.read(|c| Ok(c.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?)).unwrap();
    assert_eq!(violations, 0, "every foreign key holds");
    let fks: i64 = new.read(|c| Ok(c.query_row("SELECT count(*) FROM pragma_foreign_key_list('jobs')", [], |r| r.get(0))?)).unwrap();
    assert!(fks >= 1, "jobs keeps its foreign keys");
    let has_summary: i64 = new.read(|c| Ok(c.query_row("SELECT count(*) FROM pragma_table_info('jobs') WHERE name = 'summary'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(has_summary, 1, "the migration ran: jobs.summary exists");
}

#[test]
fn explore_attempt_row_satisfies_the_attempt_checks() {
    let (_dir, mut store) = fresh();
    write(&mut store, &job("s", "scout", "NULL")).expect("a scout Job");
    write(&mut store, "UPDATE jobs SET status = 'running' WHERE key = 's';").unwrap();
    write(
        &mut store,
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
         VALUES ('pay', 's-a1', 's', 1, 'H2', 'ladder', 'starting', 'cpd/pay--s-a1', '/home/x/run/pay/explore/', NULL, 't');",
    )
    .expect("an explore Attempt: no base_sha, the explore directory as worktree");
    write(&mut store, "UPDATE attempts SET status = 'running' WHERE id = 's-a1';").expect("starting to running");
    write(&mut store, &format!("UPDATE attempts SET status = 'merging', submitted_sha = '{HASH}', submitted_at = 't' WHERE id = 's-a1';"))
        .expect("running to merging with the artefact hash");
    write(
        &mut store,
        &format!(
            "UPDATE attempts SET status = 'ended', outcome = 'merged', cause = 'merged', merged_sha = '{HASH}', ended_at = 't' WHERE id = 's-a1';
             UPDATE jobs SET status = 'merged', merged_sha = '{HASH}', summary = 'found it' WHERE key = 's';"
        ),
    )
    .expect("ended merged with a 64-hex artefact hash");
    let (branch, sha): (String, String) = store
        .read(|c| Ok(c.query_row("SELECT branch, merged_sha FROM attempts", [], |r| Ok((r.get(0)?, r.get(1)?)))?))
        .unwrap();
    assert_eq!((branch.as_str(), sha.as_str()), ("cpd/pay--s-a1", HASH));
}
