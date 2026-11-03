//! Red tests for the Session resolver's key order (commands.md §1.3) and the `SessionRef` key syntax. They call
//! `resolve_as` with an explicit caller: none reads `CPD_SESSION`.

use super::resolve::resolve_as;
use crate::args::SessionRef;
use crate::error::ErrorCode;
use crate::home::CpdHome;
use crate::ids::SessionId;
use crate::store::Store;

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
}

/// Contexts `cpd` and `other`; Plans `plan-x` (Lead `lx`, Context `cpd`) and `plan-o` (Lead `lo`, Context `other`); the Job
/// `job-y` of `plan-x`; the Leads `lx` (`cpd:plan-x`) and `lo` (`other:plan-o`). Each test adds the other Sessions.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let fingerprint = "f".repeat(64);
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('cpd', 'Cpd', 't', 'm1');
                 INSERT INTO contexts (id, title, created_at, created_on) VALUES ('other', 'Other', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lx', 'lead', 'cpd', 'plan-x', 'cpd:plan-x', '/', 'O3', 'working', 't', 't');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lo', 'lead', 'other', 'plan-o', 'other:plan-o', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('plan-x', 'cpd', 'X', 'dispatch', 'lx', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('plan-o', 'other', 'O', 'dispatch', 'lo', 't', 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('plan-x', 'api', '/r', 'main', 'waiting', 'cpd/plan-x', cpd_txn(), 't');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('plan-x', 'api', 'job-y', 'Y', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fingerprint}', cpd_txn(), 't');"
            ))?;
            Ok(())
        })
        .unwrap();
    Fixture { _dir: dir, store }
}

impl Fixture {
    fn run(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    fn manager(&mut self, id: &str, context: &str, name: &str, archived: Option<&str>) {
        let archived = archived.map_or("NULL".to_string(), |t| format!("'{t}'"));
        self.run(&format!(
            "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at, archived_at)
               VALUES ('{id}', 'manager', '{context}', '{name}', '/', 'O3', 'working', 't', 't', {archived});"
        ));
    }

    /// A Lead row with the given `forked_from`, `archived_at` and `created_at`.
    fn lead_row(
        &mut self,
        id: &str,
        name: &str,
        fork_of: Option<&str>,
        archived: Option<&str>,
        created: &str,
    ) {
        let fork = fork_of.map_or("NULL".to_string(), |s| format!("'{s}'"));
        let archived = archived.map_or("NULL".to_string(), |t| format!("'{t}'"));
        self.run(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, forked_from, cwd, tier, state, state_at, created_at, archived_at)
               VALUES ('{id}', 'lead', 'cpd', 'plan-x', '{name}', {fork}, '/', 'O3', 'stopped', 't', '{created}', {archived});"
        ));
    }

    /// An ended Attempt `job-y-a<number>` of `plan-x` and its Worker Session `id` named `name`.
    fn worker(&mut self, id: &str, number: u32, name: &str) {
        self.run(&format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, ended_at, outcome, cause)
               VALUES ('plan-x', 'job-y-a{number}', 'job-y', {number}, 'S2', 'ladder', 'ended', 'cpd/plan-x--job-y-a{number}', '/w', 't', 't', 'cancelled', 'cancel_requested');
             INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
               VALUES ('{id}', 'worker', 'cpd', 'plan-x', 'job-y-a{number}', '{name}', '/', 'S2', 'stopped', 't', 't');"
        ));
    }

    fn resolve(&mut self, value: &str, caller: Option<&str>) -> Result<SessionId, ErrorCode> {
        let reference: SessionRef = value.parse().unwrap();
        let caller = caller.map(|c| SessionId::parse(c).unwrap());
        self.store
            .read(|conn| resolve_as(conn, &reference, caller.as_ref()))
            .map_err(|e| e.code())
    }

    fn found(&mut self, value: &str, caller: Option<&str>) -> String {
        self.resolve(value, caller)
            .unwrap_or_else(|code| panic!("{value} should resolve, got {code:?}"))
            .to_string()
    }
}

// ---- syntax ------------------------------------------------------------------------------------------------------------

#[test]
fn session_ref_parses_a_two_segment_key() {
    let parsed: SessionRef = "cpd:plan-x".parse().unwrap();
    assert_eq!(parsed, SessionRef::Key("cpd:plan-x".into()));
    assert_eq!(parsed.to_string(), "cpd:plan-x");
}

#[test]
fn session_ref_parses_a_three_segment_key() {
    let parsed: SessionRef = "cpd:plan-x:job-y".parse().unwrap();
    assert_eq!(parsed, SessionRef::Key("cpd:plan-x:job-y".into()));
    assert_eq!(parsed.to_string(), "cpd:plan-x:job-y");
}

#[test]
fn session_ref_refuses_a_malformed_key() {
    // A well-formed key is accepted, so a refusal below is for the key's own sake.
    assert!("cpd:plan-x".parse::<SessionRef>().is_ok());
    for bad in [
        "ab:cd:ef:gh",
        "cpd:",
        ":plan-x",
        "cpd::job-y",
        "Cpd:plan-x",
        "cpd:Plan",
    ] {
        let err = bad.parse::<SessionRef>().expect_err(bad);
        assert_eq!(err.code(), ErrorCode::InvalidSessionId, "{bad}");
    }
}

// ---- steps 1 and 2 -----------------------------------------------------------------------------------------------------

#[test]
fn resolve_as_exact_id_beats_a_session_named_like_it() {
    let mut f = fixture();
    f.manager("m1", "cpd", "other-thing", None);
    f.manager("m2", "other", "m1", None);
    assert_eq!(f.found("m1", None), "m1");
}

#[test]
fn resolve_as_finds_a_manager_by_its_context_key() {
    let mut f = fixture();
    f.manager("m1", "cpd", "cpd", None);
    assert_eq!(f.found("cpd", None), "m1");
}

#[test]
fn resolve_as_finds_a_lead_by_its_plan_key() {
    let mut f = fixture();
    assert_eq!(f.found("cpd:plan-x", None), "lx");
}

#[test]
fn resolve_as_finds_the_latest_attempt_of_a_worker_key() {
    let mut f = fixture();
    f.worker("w2", 2, "cpd:plan-x:job-y");
    f.worker("w10", 10, "cpd:plan-x:job-y");
    f.worker("w1", 1, "cpd:plan-x:job-y");
    assert_eq!(f.found("cpd:plan-x:job-y", None), "w10");
}

#[test]
fn resolve_as_prefers_a_live_session_to_an_archived_one() {
    let mut f = fixture();
    f.manager("old", "cpd", "shared", Some("2026-03-01"));
    f.manager("live", "cpd", "shared", None);
    assert_eq!(f.found("shared", None), "live");
}

#[test]
fn resolve_as_prefers_the_non_fork_session_of_a_name() {
    let mut f = fixture();
    f.lead_row("forked", "cpd:plan-x", Some("lx"), None, "2027-01-01");
    assert_eq!(f.found("cpd:plan-x", None), "lx");
}

// ---- step 3 ------------------------------------------------------------------------------------------------------------

#[test]
fn resolve_as_takes_the_manager_alias_when_no_session_has_that_name() {
    let mut f = fixture();
    f.manager("m1", "cpd", "cpd", None);
    assert_eq!(f.found("cpd-manager", None), "m1");
}

#[test]
fn resolve_as_prefers_a_session_named_like_the_alias() {
    let mut f = fixture();
    f.manager("m1", "cpd", "cpd", None);
    f.manager("named", "other", "cpd-manager", None);
    assert_eq!(f.found("cpd-manager", None), "named");
}

// ---- steps 4 and 5 -----------------------------------------------------------------------------------------------------

#[test]
fn resolve_as_reads_a_bare_plan_id_in_the_caller_context() {
    let mut f = fixture();
    f.manager("m1", "cpd", "cpd", None);
    // A decoy named as the relative key of the other Plan: step 4 must not read it (the Plan is not of the caller's Context).
    f.manager("decoy", "other", "cpd:plan-o", None);
    assert_eq!(f.found("plan-x", Some("m1")), "lx");
    assert_eq!(f.found("plan-o", Some("m1")), "lo");
}

#[test]
fn resolve_as_reads_a_bare_job_key_in_the_caller_plan() {
    let mut f = fixture();
    f.worker("w1", 1, "cpd:plan-x:job-y");
    assert_eq!(f.found("job-y", Some("lx")), "w1");
}

#[test]
fn resolve_as_skips_the_relative_forms_without_a_caller() {
    let mut f = fixture();
    f.worker("w1", 1, "cpd:plan-x:job-y");
    // `job-y` is no legacy form: without a caller nothing matches.
    assert_eq!(f.resolve("job-y", None), Err(ErrorCode::SessionNotFound));
    // `plan-x` is a legacy Plan id: its Lead, by step 6.
    assert_eq!(f.found("plan-x", None), "lx");
}

// ---- step 6 ------------------------------------------------------------------------------------------------------------

#[test]
fn resolve_as_keeps_the_legacy_plan_prefix_and_scoped_forms() {
    let mut f = fixture();
    f.manager("mgr-cccc1", "cpd", "cpd", None);
    f.worker("work-bbbb1", 1, "cpd:plan-x:job-y");
    f.worker("work-bbbb2", 2, "cpd:plan-x:job-y");
    assert_eq!(f.found("plan-x", None), "lx");
    assert_eq!(f.found("plan-x.work-bbbb2", None), "work-bbbb2");
    assert_eq!(f.found("mgr-c", None), "mgr-cccc1");
    assert_eq!(
        f.resolve("work-bbbb", None),
        Err(ErrorCode::SessionAmbiguous)
    );
}

#[test]
fn resolve_as_refuses_an_unknown_key_as_session_not_found() {
    let mut f = fixture();
    assert_eq!(
        f.resolve("cpd:plan-x:nope", None),
        Err(ErrorCode::SessionNotFound)
    );
}
