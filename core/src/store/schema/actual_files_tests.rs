//! The migration `actual-files`: the nullable `attempts.actual_files`, a JSON array.

use super::*;
use crate::error::CpdError;
use crate::store::Store;

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

/// A fully migrated store with Plan `pay`, its PR `api` and a running Job `j`: the rows an Attempt needs.
fn fresh() -> (tempfile::TempDir, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    write(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
               VALUES ('pay', 'ctx', 'Plan', 'plan', 'lead', 't');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
               VALUES ('pay', 'api', '/r', 'main', 'waiting', 'pay/api', cpd_txn(), 't');
             INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
               VALUES ('pay', 'api', 'j', 'T', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, 'work', NULL, cpd_txn(), 't');
             UPDATE jobs SET status = 'queued', fingerprint = '{FP}' WHERE key = 'j';
             UPDATE jobs SET status = 'running' WHERE key = 'j';"
        ),
    )
    .unwrap();
    (dir, store)
}

fn attempt(n: u32, columns: &str, values: &str) -> String {
    // One Attempt per Job may be active (`attempts_one_active`): the first runs, the later ones are ended.
    let (status, ended) = if n == 1 { ("running", "") } else { ("ended", ", 'cancelled', 'cancel_requested', 't'") };
    let ended_columns = if n == 1 { "" } else { ", outcome, cause, ended_at" };
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at{ended_columns}{columns})
         VALUES ('pay', 'j-a{n}', 'j', {n}, 'S2', 'ladder', '{status}', 'pay/api', '/w', 't'{ended}{values});"
    )
}

#[test]
fn actual_files_is_additive_and_old_rows_read_null() {
    let (_dir, mut store) = fresh();
    write(&mut store, &attempt(1, "", "")).expect("an Attempt that does not name the column");
    let read = |store: &mut Store, id: &str| -> Option<String> {
        store.read(|c| Ok(c.query_row("SELECT actual_files FROM attempts WHERE id = ?1", [id], |r| r.get(0))?)).unwrap()
    };
    assert_eq!(read(&mut store, "j-a1"), None);
    for (n, v) in [(2, "[]"), (3, r#"["a.rs","docs/"]"#)] {
        write(&mut store, &attempt(n, ", actual_files", &format!(", '{v}'"))).expect(v);
        assert_eq!(read(&mut store, &format!("j-a{n}")).as_deref(), Some(v));
    }
    assert!(NAMED.iter().any(|m| m.name.ends_with("-actual-files")), "the migration actual-files is declared");
}

#[test]
fn actual_files_must_be_a_json_array() {
    let (_dir, mut store) = fresh();
    write(&mut store, &attempt(1, ", actual_files", ", '[]'")).expect("`[]` is an array");
    for (n, bad) in [(2, "{}"), (3, "x"), (4, ""), (5, "1"), (6, "null")] {
        let failed = write(&mut store, &attempt(n, ", actual_files", &format!(", '{bad}'")))
            .expect_err(bad)
            .details()["sqlite_message"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert!(failed.contains("CHECK"), "{bad:?}: {failed}");
    }
}
