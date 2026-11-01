//! Test support: the statements a closure runs on a connection, and a Ledger
//! of `n` Plans with a PR, a pull request, Jobs and a review each, to count
//! them against (the read model's "a fixed number of queries", `tui/src/read`).

use std::ffi::{CStr, c_int, c_uint, c_void};
use std::sync::Mutex;

use rusqlite::{Connection, ffi};

use crate::home::CpdHome;
use crate::store::Store;

unsafe extern "C" fn on_trace(kind: c_uint, ctx: *mut c_void, stmt: *mut c_void, _sql: *mut c_void) -> c_int {
    if kind == ffi::SQLITE_TRACE_STMT as c_uint {
        // SAFETY: `ctx` is the `Mutex` of `statements`, which outlives the registration; `stmt` is a live statement.
        unsafe {
            let sql = ffi::sqlite3_expanded_sql(stmt.cast());
            if !sql.is_null() {
                (*ctx.cast::<Mutex<Vec<String>>>()).lock().unwrap().push(CStr::from_ptr(sql).to_string_lossy().into_owned());
                ffi::sqlite3_free(sql.cast());
            }
        }
    }
    0
}

/// `f`'s result and every statement it started on `conn`, with its parameters filled in.
pub(super) fn statements<T>(conn: &Connection, f: impl FnOnce() -> T) -> (T, Vec<String>) {
    let seen: Mutex<Vec<String>> = Mutex::new(Vec::new());
    // SAFETY: the callback is removed before `seen` goes out of scope, and `conn` is borrowed throughout.
    unsafe {
        ffi::sqlite3_trace_v2(conn.handle(), ffi::SQLITE_TRACE_STMT as c_uint, Some(on_trace), std::ptr::addr_of!(seen).cast_mut().cast());
    }
    let value = f();
    unsafe {
        ffi::sqlite3_trace_v2(conn.handle(), 0, None, std::ptr::null_mut());
    }
    (value, seen.into_inner().unwrap())
}

/// A Store in `dir` holding `plans` Plans (`plan-1`…), each in `phase` `plan` with one PR with a
/// pull request, two draft Jobs (the second needed by the first) and a pending review.
pub(super) fn ledger(dir: &std::path::Path, plans: usize) -> Store {
    let home = CpdHome::resolve(Some(dir.into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let mut sql = String::from("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');");
    for n in 1..=plans {
        sql.push_str(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead-{n}', 'lead', 'ctx', 'plan-{n}', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
               VALUES ('plan-{n}', 'ctx', 'Plan {n}', 'plan', 'lead-{n}', '2000-01-01T00:00:{n:02}.000Z');
             INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at,
                                        mergeable, merge_state)
               VALUES ('acme/api', {n}, 'https://github.com/acme/api/pull/{n}', 'Add api', 0, 'open', 'approved', 'passed',
                       't', NULL, 't', 'mergeable', 'clean');
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                              created_txn, created_at, started_at)
               VALUES ('plan-{n}', 'api', '/r/api', 'main', 'open', 'cpd/plan-{n}/api', '/r/api.plan-{n}', {n}, 'https://github.com/acme/api/pull/{n}',
                       't', (SELECT id FROM pull_requests WHERE number = {n}), cpd_txn(), 't', 't');
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
               VALUES ('plan-{n}', 'db', '/r/db', 'main', 'waiting', 'cpd/plan-{n}/db', cpd_txn(), 't');
             INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('plan-{n}', 'api', 'db');
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
               VALUES ('plan-{n}', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't'),
                      ('plan-{n}', 'api', 'b', 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't');
             INSERT INTO dependencies (plan, job, needs) VALUES ('plan-{n}', 'a', 'b');
             INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at)
               VALUES ('plan-{n}', 'r1', 1, '{fp}', 'pending', 't');",
            fp = "f".repeat(64)
        ));
    }
    store.write("fixture", |tx| Ok(tx.execute_batch(&sql)?)).unwrap();
    store
}
