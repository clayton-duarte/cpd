//! The data migration `20261104T1601-finish-legacy-review-plans` (pro-d383): a `codereview` Plan stuck in `grounding`
//! with nothing live or open becomes `done`, so the Lead can be archived. Stores here are built to the migration before it.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const MIGRATION: &str = "20261104T1601-finish-legacy-review-plans";
const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

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

/// The migrations before `finish-legacy-review-plans`. Fails while the migration is missing.
fn earlier() -> Vec<Named> {
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-finish-legacy-review-plans"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "finish-legacy-review-plans is a built-in migration"
    );
    earlier
}

const SEED: &str = "INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('codereview', 'Code review', 't', 'm');
     INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('cpd', 'Cpd', 't', 'm');";

/// A Lead Session `l-<id>` in `state` and a Plan `<id>` of `ctx` in `grounding`, locked at `locked` (SQL).
fn plan(id: &str, ctx: &str, state: &str, locked: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-{id}', 'lead', '{ctx}', '{id}', 'L {id}', '/', 'S3', '{state}', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('{id}', '{ctx}', 'Review', 'grounding', 'l-{id}', 't', {locked});"
    )
}

/// A workflow `api` of Plan `id` and a Job in `status`, reached through the legal steps.
fn job(id: &str, status: &str) -> String {
    let mut sql = format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('{id}', 'api', '/r', 'main', 'waiting', '{id}/api', cpd_txn(), 't');
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
           VALUES ('{id}', 'api', 'j', 'T', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, 'work', NULL, cpd_txn(), 't');"
    );
    let steps: &[&str] = match status {
        "draft" => &[],
        "queued" => &["queued"],
        "running" => &["queued", "running"],
        "failed" => &["queued", "running", "failed"],
        _ => &["queued", "running", "merged"],
    };
    for s in steps {
        let fp = if *s == "queued" {
            format!(", fingerprint = '{FP}'")
        } else {
            String::new()
        };
        let fp = if *s == "merged" {
            format!("{fp}, merged_sha = 'abc'")
        } else {
            fp
        };
        let fp = if *s == "failed" {
            format!("{fp}, failed_cause = 'x'")
        } else {
            fp
        };
        sql.push_str(&format!(
            "UPDATE jobs SET status = '{s}'{fp} WHERE plan = '{id}' AND key = 'j';"
        ));
    }
    sql
}

fn before(extra: &str) -> (tempfile::TempDir, CpdHome) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = crate::store::tests::open_named(&home, &earlier()).unwrap();
    write(&mut store, &format!("{SEED}{extra}")).unwrap_or_else(|e| panic!("fixture: {e:?}"));
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

fn phase(store: &mut Store, id: &str) -> String {
    store
        .read(|c: &Connection| {
            Ok(c.query_row("SELECT phase FROM plans WHERE id = ?1", [id], |r| r.get(0))?)
        })
        .unwrap()
}

/// `(locked_at, done_at)` of a Plan.
fn stamps(store: &mut Store, id: &str) -> (Option<String>, Option<String>) {
    store
        .read(|c: &Connection| {
            Ok(c.query_row(
                "SELECT locked_at, done_at FROM plans WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

#[test]
fn legacy_codereview_grounding_plan_becomes_done() {
    let (_dir, home) = before(&plan("r1", "codereview", "idle", "NULL"));
    let mut store = migrated_in(&home);
    assert_eq!(phase(&mut store, "r1"), "done");
    let (locked, done) = stamps(&mut store, "r1");
    assert!(locked.is_some(), "locked_at is set");
    assert_eq!(
        locked, done,
        "locked_at and done_at are the migration's time"
    );
    // E14's codereview branch (archivable()): the Plan is finished and its Lead idle or stopped.
    let finished = count(
        &mut store,
        "SELECT count(*) FROM plans p JOIN sessions s ON s.id = p.lead_session
          WHERE p.id = 'r1' AND p.context = 'codereview' AND p.archived_at IS NULL AND p.deleted_at IS NULL
            AND s.state IN ('idle', 'stopped') AND p.phase = 'done'
            AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = p.id AND j.status IN ('draft', 'queued', 'running', 'failed'))
            AND NOT EXISTS (SELECT 1 FROM inbox_items i WHERE i.session = s.id AND i.closed_at IS NULL AND i.dismissed_at IS NULL)",
    );
    assert_eq!(finished, 1, "the Plan is archivable");
}

#[test]
fn an_existing_locked_at_is_kept() {
    // A Plan in grounding with `locked_at` set breaks a CHECK, which a legacy row may predate: seed it past the check.
    let (_dir, home) = before(&format!(
        "PRAGMA ignore_check_constraints = ON;{}PRAGMA ignore_check_constraints = OFF;",
        plan("r1", "codereview", "idle", "'t0'")
    ));
    let mut store = migrated_in(&home);
    assert_eq!(phase(&mut store, "r1"), "done");
    let (locked, done) = stamps(&mut store, "r1");
    assert_eq!(locked.as_deref(), Some("t0"));
    assert!(done.is_some(), "done_at is set");
}

#[test]
fn a_plan_with_a_live_job_stays_in_grounding() {
    let statuses = ["draft", "queued", "running", "failed", "merged"];
    let mut sql = String::new();
    for s in statuses {
        let id = format!("r-{s}");
        sql.push_str(&plan(&id, "codereview", "idle", "NULL"));
        sql.push_str(&job(&id, s));
    }
    let (_dir, home) = before(&sql);
    let mut store = migrated_in(&home);
    for s in statuses {
        let want = if s == "merged" { "done" } else { "grounding" };
        assert_eq!(phase(&mut store, &format!("r-{s}")), want, "Job {s}");
    }
}

#[test]
fn a_plan_with_an_open_inbox_item_stays_in_grounding() {
    let item = |id: &str, extra: &str| {
        format!(
            "INSERT INTO inbox_items (kind, urgency, subject, session, label, opened_at{}) VALUES ('dialog', 'blue', 's-{id}', 'l-{id}', 'l', 't'{});",
            if extra.is_empty() {
                ""
            } else {
                ", dismissed_at"
            },
            if extra.is_empty() { "" } else { ", 't2'" }
        )
    };
    let sql = format!(
        "{}{}{}{}{}",
        plan("r-open", "codereview", "idle", "NULL"),
        item("r-open", ""),
        plan("r-dismissed", "codereview", "idle", "NULL"),
        item("r-dismissed", "x"),
        ""
    );
    let (_dir, home) = before(&sql);
    let mut store = migrated_in(&home);
    assert_eq!(phase(&mut store, "r-open"), "grounding");
    assert_eq!(phase(&mut store, "r-dismissed"), "done");
}

#[test]
fn other_plans_are_left_alone() {
    let sql = format!(
        "{}{}{}{}{}UPDATE plans SET deleted_at = 'td' WHERE id = 'r-deleted';
         UPDATE plans SET phase = 'done', locked_at = 'tl', done_at = 'td' WHERE id = 'r-done';
         UPDATE plans SET phase = 'plan' WHERE id = 'r-dispatch';
         UPDATE plans SET phase = 'dispatch', locked_at = 'tl' WHERE id = 'r-dispatch';",
        plan("c-pay", "cpd", "idle", "NULL"),
        plan("r-legacy", "codereview", "idle", "NULL"),
        plan("r-deleted", "codereview", "idle", "NULL"),
        plan("r-done", "codereview", "idle", "NULL"),
        plan("r-dispatch", "codereview", "idle", "NULL"),
    );
    let (_dir, home) = before(&sql);
    let dump = |store: &mut Store| -> String {
        store
            .read(|c: &Connection| {
                Ok(c.query_row(
                    "SELECT json_group_array(json_object('id', id, 'phase', phase, 'locked_at', locked_at, 'done_at', done_at, 'deleted_at', deleted_at, 'archived_at', archived_at)) FROM (SELECT id, phase, locked_at, done_at, deleted_at, archived_at FROM plans WHERE id <> 'r-legacy' ORDER BY id)",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    };
    let mut old = crate::store::tests::open_named(&home, &earlier()).unwrap();
    let want = dump(&mut old);
    drop(old);
    let mut store = migrated_in(&home);
    assert_eq!(phase(&mut store, "r-legacy"), "done", "the migration ran");
    assert_eq!(dump(&mut store), want);
}

#[test]
fn a_stopped_lead_plan_is_finished_too() {
    let (_dir, home) = before(&plan("r1", "codereview", "stopped", "NULL"));
    let mut store = migrated_in(&home);
    assert_eq!(phase(&mut store, "r1"), "done");
}
