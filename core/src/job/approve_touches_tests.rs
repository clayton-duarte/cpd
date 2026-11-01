//! Red tests of `job approve-touches` (commands.md §5.9a, contract §2.2 "Job kinds", §7.5; hs-n641): the Lead's approval
//! for a Job to change a path outside its Spec `touches`. It unions and de-duplicates `jobs.touches_approved`, appends
//! `job-edited` with `fields: ["touches_approved"]`, writes nothing on a rerun of an approved path, and is refused by a
//! Worker and off an ended Job.
//!
//! No test runs git, reads the wall clock, sleeps or spawns: the store is a temp `$CPD_HOME` fixture.

use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::ApproveTouchesArgs;
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
                 INSERT INTO prs (plan, key, name, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pay', 'api', 'cpd', '/r', 'main', 'dispatch', 'cpd/pay/api', '/lw', 1, '{T}', '{T}');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status,
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
    /// `cpd job approve-touches a --path <p>… --reason <text> --plan pay` on this env.
    fn approve(&mut self, paths: &[&str], reason: &str) -> Result<Output, CpdError> {
        let args = ApproveTouchesArgs {
            key: JobKey::parse("a").unwrap(),
            paths: paths.iter().map(|p| (*p).to_owned()).collect(),
            reason: reason.parse().unwrap(),
            plan: PlanArg {
                plan: Some(PlanId::parse("pay").unwrap()),
            },
        };
        super::approve_touches::approve_in(&mut self.env, args)
    }

    /// `jobs.touches_approved` of Job `a`, as the JSON array it stores, or `Value::Null`.
    fn approved(&mut self) -> Value {
        let stored: Option<String> = self
            .env
            .store
            .read(|conn| {
                Ok(conn
                    .query_row(
                        "SELECT touches_approved FROM jobs WHERE plan = 'pay' AND key = 'a'",
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
        Output::Owned => {
            panic!("job approve-touches prints through the envelope, not to the terminal")
        }
    }
}

/// The refusal `code` (exit 3) of `result`.
fn refused(result: Result<Output, CpdError>, code: ErrorCode) -> CpdError {
    match result {
        Ok(out) => panic!("expected {code:?}, got a success: {out:?}"),
        Err(e) => {
            assert_eq!(e.code(), code, "{e:?}");
            assert_eq!(e.exit().code(), 3, "{code:?} is a refusal");
            e
        }
    }
}

#[test]
fn the_lead_approves_a_path_and_the_event_records_it() {
    let mut fx = fixture("queued");
    let out = json_of(
        fx.approve(
            &["cli/src/", "core/src/guard.rs"],
            "The guard needs a helper there.",
        )
        .expect("the Lead approves"),
    );
    assert_eq!(
        out["job"],
        json!({"key": "a", "status": "queued", "kind": "work"}),
        "{out}"
    );
    assert_eq!(
        out["approved"],
        json!(["cli/src/", "core/src/guard.rs"]),
        "{out}"
    );
    assert_eq!(out["fields"], json!(["touches_approved"]), "{out}");
    assert_eq!(
        fx.approved(),
        json!(["cli/src/", "core/src/guard.rs"]),
        "jobs.touches_approved is what the Job has now"
    );

    let edited = fx.edited();
    assert_eq!(edited.len(), 1, "one job-edited Event: {edited:?}");
    assert_eq!(
        edited[0].1["fields"],
        json!(["touches_approved"]),
        "the Event names the field: {}",
        edited[0].1
    );
    let text = &edited[0].0;
    assert!(
        text.contains("cli/src/") && text.contains("core/src/guard.rs"),
        "the Event names the paths: {text}"
    );
    assert!(
        text.contains("The guard needs a helper there."),
        "the Event carries the reason: {text}"
    );

    // The union of what was approved, each path once.
    let out = json_of(
        fx.approve(&["docs/y.md", "cli/src/"], "Also the doc.")
            .expect("the Lead approves more"),
    );
    assert_eq!(
        out["approved"],
        json!(["cli/src/", "core/src/guard.rs", "docs/y.md"]),
        "the approval is the union, de-duplicated"
    );
    assert_eq!(
        fx.approved(),
        json!(["cli/src/", "core/src/guard.rs", "docs/y.md"])
    );

    // A path already approved writes nothing: no transaction, no Event, and `already`.
    let writes = fx.writes();
    let out = json_of(
        fx.approve(&["core/src/guard.rs"], "Again.")
            .expect("a rerun is fine"),
    );
    assert_eq!(out["already"], json!(true), "{out}");
    assert_eq!(
        out["fields"],
        json!([]),
        "nothing changed, so no field: {out}"
    );
    assert_eq!(
        fx.writes(),
        writes,
        "a rerun of an approved path writes no transaction and no Event"
    );
    assert_eq!(fx.edited().len(), 2, "no new job-edited Event");
}

#[test]
fn a_worker_is_refused_not_a_lead() {
    let mut fx = fixture("queued");
    // cpd sets CPD_ATTEMPT in every Worker's pane (contract §6.5): the command is the Lead's alone.
    // SAFETY: `LOCK` serializes these tests, and nextest gives each a process of its own.
    unsafe { std::env::set_var("CPD_ATTEMPT", "a-a1") };
    let writes = fx.writes();
    let e = refused(
        fx.approve(&["cli/src/"], "A Worker may not allow this."),
        ErrorCode::NotALead,
    );
    assert!(
        e.message().contains("cpd ask"),
        "the refusal tells the Worker to ask: {e:?}"
    );
    assert_eq!(fx.writes(), writes, "a refusal writes nothing");
    assert_eq!(fx.approved(), Value::Null, "nothing is approved");
    assert!(fx.edited().is_empty(), "no job-edited Event");
}

#[test]
fn an_ended_job_is_refused_illegal_transition() {
    let mut fx = fixture("merged");
    let writes = fx.writes();
    refused(
        fx.approve(&["cli/src/"], "Too late."),
        ErrorCode::IllegalTransition,
    );
    assert_eq!(fx.writes(), writes, "a refusal writes nothing");
    assert_eq!(fx.approved(), Value::Null, "an ended Job approves nothing");
}
