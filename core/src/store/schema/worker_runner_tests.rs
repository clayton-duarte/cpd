//! The migration `worker-runner` (contract §4.4, §4.5): `attempts.runner` (`pi` or `durable`, default `pi`) and the
//! nullable `attempts.worker_pid`.

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
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at{columns})
         VALUES ('pay', 'j-a{n}', 'j', {n}, 'S2', 'ladder', 'running', 'pay/api', '/w', 't'{values});"
    )
}

#[test]
fn attempts_gain_runner_defaulting_to_pi_and_a_null_worker_pid() {
    let (_dir, mut store) = fresh();
    write(&mut store, &attempt(1, "", "")).expect("an Attempt that names neither column");
    let (runner, pid): (String, Option<i64>) =
        store.read(|c| Ok(c.query_row("SELECT runner, worker_pid FROM attempts", [], |r| Ok((r.get(0)?, r.get(1)?)))?)).unwrap();
    assert_eq!((runner.as_str(), pid), ("pi", None));

    write(&mut store, "UPDATE attempts SET runner = 'durable', worker_pid = 4242;").expect("a durable Worker with its pid");
    let (runner, pid): (String, Option<i64>) =
        store.read(|c| Ok(c.query_row("SELECT runner, worker_pid FROM attempts", [], |r| Ok((r.get(0)?, r.get(1)?)))?)).unwrap();
    assert_eq!((runner.as_str(), pid), ("durable", Some(4242)));
    assert!(NAMED.iter().any(|m| m.name.ends_with("-worker-runner")), "the migration worker-runner is declared");
}

#[test]
fn attempts_runner_refuses_a_value_that_is_neither_pi_nor_durable() {
    let (_dir, mut store) = fresh();
    write(&mut store, &attempt(1, ", runner", ", 'durable'")).expect("`durable` is a runner");
    for (n, bad) in [(2, "tmux"), (3, "PI"), (4, "")] {
        let failed = write(&mut store, &attempt(n, ", runner", &format!(", '{bad}'")))
            .expect_err(bad)
            .details()["sqlite_message"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert!(failed.contains("CHECK"), "{bad:?}: {failed}");
    }
}
