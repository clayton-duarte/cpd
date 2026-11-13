//! Red tests of `job approve-test-removal` (commands.md §5.9b, contract §7.5; b70): the Lead's approval for a Job to
//! remove or weaken named test cases. It unions and de-duplicates `jobs.test_removal_approved`, appends `job-edited`
//! with `fields: ["test_removal_approved"]` and the reason, writes nothing on a rerun, and is refused by a Worker, a
//! blank name and an ended Job.
//!
//! No test runs git, reads the wall clock, sleeps or spawns: the store is a temp `$CPD_HOME` fixture.

use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::ApproveTestRemovalArgs;
use crate::args::PlanArg;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{JobKey, PlanId};
use crate::output::Output;

/// `CPD_ATTEMPT` is read from the process: one of these tests at a time, and nextest gives each its process.
static LOCK: Mutex<()> = Mutex::new(());

const T: &str = "2026-01-01T00:00:00.000Z";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// Plan `pay` (phase `dispatch`) of Context `ctx`, PR `api` and Job `a` in `status`, in a temp `$CPD_HOME` with an owner
/// policy. `CPD_ATTEMPT` is restored when the fixture drops.
struct Fixture {
    _dir: tempfile::TempDir,
    env: Env,
    saved: Option<std::ffi::OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // SAFETY: `LOCK` serializes these tests, and nextest gives each a process of its own.
        unsafe {
            match &self.saved {
                Some(v) => std::env::set_var("CPD_ATTEMPT", v),
                None => std::env::remove_var("CPD_ATTEMPT"),
            }
        }
    }
}

fn fixture(status: &str) -> Fixture {
    let lock = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let owner = home.config().parent().unwrap().join("policy").join("acme");
    std::fs::create_dir_all(&owner).unwrap();
    std::fs::write(owner.join("owner.toml"), "").unwrap();
    let saved = std::env::var_os("CPD_ATTEMPT");
    // SAFETY: as in `Drop`.
    unsafe { std::env::remove_var("CPD_ATTEMPT") };
    let mut env = Env::open_home(home).unwrap();
    let merged = if status == "merged" {
        format!("'{T}'")
    } else {
        "NULL".to_owned()
    };
    let sha = if status == "merged" {
        "'abc'".to_owned()
    } else {
        "NULL".to_owned()
    };
    env.store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', '{T}', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{T}', '{T}');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'P', 'dispatch', 'l-pay', '{T}', '{T}');
                 INSERT INTO workflows (plan, key, name, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pay', 'api', 'cpd', '/r', 'main', 'dispatch', 'cpd/pay/api', '/lw', 1, '{T}', '{T}');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status,
                                   appended, fingerprint, created_txn, created_at, merged_sha)
                   VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"docs/research/x.md\"]', '[\"b\"]', '[]', 'work', '{status}',
                           0, '{FP}', cpd_txn(), '{T}', {sha});"
            ))?;
            Ok(())
        })
        .unwrap();
    let _ = merged;
    Fixture {
        _dir: dir,
        env,
        saved,
        _lock: lock,
    }
}

impl Fixture {
    /// `cpd job approve-test-removal a --test <n>… --reason <text> --plan pay` on this env.
    fn approve(&mut self, tests: &[&str], reason: &str) -> Result<Output, CpdError> {
        let args = ApproveTestRemovalArgs {
            key: JobKey::parse("a").unwrap(),
            tests: tests.iter().map(|p| (*p).to_owned()).collect(),
            reason: reason.parse().unwrap(),
            plan: PlanArg {
                plan: Some(PlanId::parse("pay").unwrap()),
            },
        };
        super::approve::approve_test_removal_in(&mut self.env, args)
    }

    /// `jobs.test_removal_approved` of Job `a`, as the JSON array it stores, or `Value::Null`.
    fn approved(&mut self) -> Value {
        let stored: Option<String> = self
            .env
            .store
            .read(|conn| {
                Ok(conn
                    .query_row(
                        "SELECT test_removal_approved FROM jobs WHERE plan = 'pay' AND key = 'a'",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?)
            })
            .unwrap()
            .flatten();
        stored.map_or(Value::Null, |text| serde_json::from_str(&text).unwrap())
    }

    /// `(text, data)` of the `job-edited` Events, in order.
    fn edited(&mut self) -> Vec<(String, Value)> {
        self.env
            .store
            .read(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT text, data FROM events WHERE kind = 'job-edited' ORDER BY id",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        serde_json::from_str(&r.get::<_, String>(1)?).unwrap(),
                    ))
                })?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    /// How many transactions and Events the store holds.
    fn writes(&mut self) -> (i64, i64) {
        self.env
            .store
            .read(|conn| {
                Ok((
                    conn.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?,
                    conn.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?,
                ))
            })
            .unwrap()
    }
}

/// The JSON of a successful output.
fn json_of(out: Output) -> Value {
    match out {
        Output::Success(map) => Value::Object(map),
        Output::Owned => panic!("job approve-test-removal prints through the envelope"),
    }
}

fn refused(result: Result<Output, CpdError>, code: ErrorCode) -> CpdError {
    match result {
        Ok(out) => panic!("expected {code:?}, got a success: {out:?}"),
        Err(e) => {
            assert_eq!(e.code(), code, "{e:?}");
            e
        }
    }
}

#[test]
fn the_lead_approves_a_test_and_the_event_records_the_reason() {
    let mut fx = fixture("running");
    let out = json_of(
        fx.approve(&["b", "m::c"], "Obsolete after the merge.")
            .unwrap(),
    );
    assert_eq!(
        out["job"],
        json!({"key": "a", "status": "running", "kind": "work"}),
        "{out}"
    );
    assert_eq!(out["approved"], json!(["b", "m::c"]));
    assert_eq!(out["fields"], json!(["test_removal_approved"]));
    assert_eq!(fx.approved(), json!(["b", "m::c"]));
    let edited = fx.edited();
    let (text, data) = edited.last().expect("a job-edited Event");
    assert!(
        text.contains("Obsolete after the merge.") && text.contains("m::c"),
        "{text}"
    );
    assert_eq!(data["fields"], json!(["test_removal_approved"]));
}

#[test]
fn a_rerun_writes_nothing_and_a_new_name_is_unioned() {
    let mut fx = fixture("queued");
    fx.approve(&["b"], "Why.").unwrap();
    let before = fx.writes();
    let again = json_of(fx.approve(&["b"], "Why.").unwrap());
    assert_eq!(again["already"], json!(true));
    assert_eq!(again["fields"], json!([]));
    assert_eq!(fx.writes(), before, "a rerun writes nothing");
    let more = json_of(fx.approve(&["b", "d"], "Why.").unwrap());
    assert_eq!(more["approved"], json!(["b", "d"]));
    assert_eq!(fx.approved(), json!(["b", "d"]));
}

#[test]
fn a_worker_is_refused_and_nothing_is_written() {
    let mut fx = fixture("running");
    let before = fx.writes();
    // SAFETY: `LOCK` serializes these tests, and nextest gives each a process of its own; `Drop` restores it.
    unsafe { std::env::set_var("CPD_ATTEMPT", "a-a1") };
    refused(fx.approve(&["b"], "Why."), ErrorCode::NotALead);
    assert_eq!(fx.writes(), before);
    assert_eq!(fx.approved(), Value::Null);
}

#[test]
fn a_blank_test_name_is_refused() {
    let mut fx = fixture("running");
    refused(fx.approve(&["  "], "Why."), ErrorCode::Usage);
    assert_eq!(fx.approved(), Value::Null);
}

#[test]
fn an_ended_job_is_refused() {
    let mut fx = fixture("merged");
    refused(fx.approve(&["b"], "Why."), ErrorCode::IllegalTransition);
}
