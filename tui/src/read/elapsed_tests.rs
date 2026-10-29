//! The active time of a Workflow (`PrNode.elapsed_seconds`, Context record pro-d175), read by `plan_boxes_at` at a given
//! `now_ms`: no wall clock, no sleeps.

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::store::Store;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::read;

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// A store of Context `ctx`, Lead `lead1` and Plan `pay` with the given PRs (keys), each in `dispatch`.
pub(super) fn store_with(prs: &[&str]) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let at = "'2026-01-02T00:00:00.000Z'";
    let mut sql = format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', NULL, 'lead1', '/repo', 'O3', 'idle', {at}, '2026-01-01T00:00:00.000Z');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('other', 'lead', 'ctx', NULL, 'other', '/repo', 'O3', 'idle', {at}, '2026-01-01T00:00:00.000Z');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'deploy', 'lead1', '2026-01-01T00:00:00.000Z', {at}, NULL);
         UPDATE sessions SET plan = 'pay' WHERE id = 'lead1';"
    );
    for key in prs {
        sql += &format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', '{key}', '/repo', 'main', 'dispatch', 'cpd/pay/{key}', '/wt/{key}', cpd_txn(), '2026-01-01T00:00:00.000Z', {at});"
        );
    }
    run(&mut store, &sql);
    (dir, store)
}

pub(super) fn run(store: &mut Store, sql: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

/// A Job `key` of kind `kind` of PR `pr`, queued or running.
pub(super) fn job(pr: &str, key: &str, kind: &str, status: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, created_at, kind)
         VALUES ('pay', '{pr}', '{key}', 'Job', 'S3', 'G.', 'W.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, '{FP}', cpd_txn(),
                 '2026-01-02T00:00:00.000Z', '{kind}');"
    )
}

/// A Job `key` of PR `pr` (kind `work`) with one Attempt from `from` to `to` (`None`: still running); times are `HH:MM:SS[.mmm]`.
fn worked(pr: &str, key: &str, from: &str, to: Option<&str>) -> String {
    worked_kind(pr, key, "work", from, to)
}

fn worked_kind(pr: &str, key: &str, kind: &str, from: &str, to: Option<&str>) -> String {
    let (status, outcome, cause, ended_at) = match to {
        Some(at) => ("ended", "'failed'", "'crashed'", format!("'{}'", stamp(at))),
        None => ("running", "NULL", "NULL", "NULL".to_owned()),
    };
    job(pr, key, kind, "running")
        + &format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at)
             VALUES ('pay', '{key}-a1', '{key}', 1, 'S3', 'ladder', '{status}', {outcome}, {cause}, 'cpd/pay--{key}-a1', '/w/{key}',
                     '{}', {ended_at});",
            stamp(from)
        )
}

/// `10:00:30` or `10:00:30.999` as an instant of 2026-10-18 UTC; a `HH:MM:SS` gets `.000`.
fn stamp(t: &str) -> String {
    let ms = if t.contains('.') { "" } else { ".000" };
    format!("2026-10-18T{t}{ms}Z")
}

/// A `session-state` Event of `session` at `at` moving to `to`.
fn state(session: &str, at: &str, to: &str) -> String {
    format!(
        "INSERT INTO events (txn, at, kind, context, session, text, data)
         VALUES (cpd_txn(), '{}', 'session-state', 'ctx', '{session}', 'x', '{{\"from\":\"working\",\"label\":null,\"to\":\"{to}\"}}');",
        stamp(at)
    )
}

fn now_ms(t: &str) -> i64 {
    let at = OffsetDateTime::parse(&stamp(t), &Rfc3339).unwrap();
    i64::try_from(at.unix_timestamp_nanos() / 1_000_000).unwrap()
}

/// The `elapsed_seconds` of each PR of Plan `pay` read at `now`, in key order.
fn elapsed(store: &mut Store, now: &str) -> Vec<Option<u64>> {
    let boxes = read::plan_boxes_at(store, &PlanId::parse("pay").unwrap(), now_ms(now)).unwrap();
    boxes.prs.iter().map(|p| p.elapsed_seconds).collect()
}

#[test]
fn only_a_workflow_with_an_attempt_has_an_elapsed_time() {
    let (_dir, mut store) = store_with(&["api", "web"]);
    run(&mut store, &(worked("api", "a", "10:00:00", Some("10:01:30")) + &job("web", "w", "work", "queued")));
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(90), None]); // api: 90 s; web: no Attempt
}

#[test]
fn a_finished_attempt_counts_from_its_start_to_its_end() {
    let (_dir, mut store) = store_with(&["api"]);
    run(&mut store, &worked("api", "a", "10:00:00", Some("10:01:30")));
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(90)]); // 10:00:00 to 10:01:30
}

#[test]
fn a_running_attempt_counts_up_to_the_given_now() {
    let (_dir, mut store) = store_with(&["api"]);
    run(&mut store, &worked("api", "a", "10:00:00", None));
    assert_eq!(elapsed(&mut store, "10:02:00"), vec![Some(120)]); // 10:00:00 to 10:02:00
    assert_eq!(elapsed(&mut store, "10:05:00"), vec![Some(300)]); // 10:00:00 to 10:05:00
}

#[test]
fn sequential_attempts_add_up_and_the_gap_between_them_is_not_counted() {
    let (_dir, mut store) = store_with(&["api"]);
    run(&mut store, &(worked("api", "a", "10:00:00", Some("10:01:00")) + &worked("api", "b", "10:05:00", Some("10:05:30"))));
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(90)]); // 60 s + 30 s
}

#[test]
fn parallel_and_overlapping_attempts_count_once() {
    let (_dir, mut store) = store_with(&["api"]);
    run(
        &mut store,
        &(worked("api", "a", "10:00:00", Some("10:01:40"))
            + &worked("api", "b", "10:00:50", Some("10:02:30"))
            + &worked("api", "c", "10:01:00", Some("10:01:10"))),
    );
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(150)]); // the union 10:00:00 to 10:02:30
}

#[test]
fn the_leads_blocked_time_is_subtracted_from_the_attempts() {
    let (_dir, mut store) = store_with(&["api"]);
    run(&mut store, &(worked("api", "a", "10:00:00", Some("10:02:00")) + &state("lead1", "10:00:30", "blocked") + &state("lead1", "10:00:50", "working")));
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(100)]); // 120 s minus 20 s blocked
    run(&mut store, &(state("lead1", "10:01:50", "blocked") + &state("lead1", "10:03:00", "working")));
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(90)]); // minus 10 s more: the interval is clipped to the Attempt's end
    run(&mut store, &(state("lead1", "09:00:00", "blocked") + &state("lead1", "09:30:00", "working")));
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(90)]); // before the Attempt: nothing changes
    // a label change while blocked is ONE interval; waiting, idle and working subtract nothing
    run(
        &mut store,
        &(state("lead1", "10:00:10", "waiting")
            + &state("lead1", "10:00:12", "idle")
            + &state("lead1", "10:00:14", "working")
            + &state("lead1", "10:01:00", "blocked")
            + &state("lead1", "10:01:05", "blocked")
            + &state("lead1", "10:01:08", "working")),
    );
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(82)]); // 90 s minus the one blocked 10:01:00 to 10:01:08 (8 s)
}

#[test]
fn a_lead_still_blocked_is_subtracted_up_to_now() {
    let (_dir, mut store) = store_with(&["api"]);
    run(&mut store, &(worked("api", "a", "10:00:00", None) + &state("lead1", "10:01:00", "blocked")));
    assert_eq!(elapsed(&mut store, "10:03:00"), vec![Some(60)]); // 180 s minus 120 s blocked
}

#[test]
fn attempts_of_a_follow_up_job_after_the_pull_request_opens_add_to_the_total() {
    let (_dir, mut store) = store_with(&["api"]);
    run(
        &mut store,
        &(worked_kind("api", "w", "work", "10:00:00", Some("10:01:00"))
            + &worked_kind("api", "o", "open", "10:01:10", Some("10:01:40"))
            + &worked_kind("api", "u", "update", "10:20:00", Some("10:20:20"))),
    );
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(110)]); // 60 s + 30 s + 20 s
}

#[test]
fn only_the_workflows_own_attempts_and_the_leads_own_blocked_state_count() {
    let (_dir, mut store) = store_with(&["api", "web"]);
    run(&mut store, &(worked("api", "a", "10:00:00", Some("10:01:00")) + &worked("web", "w", "10:00:30", Some("10:01:30"))));
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(60), Some(60)]); // each its own 60 s
    run(&mut store, &(state("other", "10:00:10", "blocked") + &state("other", "10:00:20", "working")));
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(60), Some(60)]); // another Session's blocked time subtracts nothing
    run(&mut store, &(state("lead1", "10:00:40", "blocked") + &state("lead1", "10:00:50", "working")));
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(50), Some(50)]); // the Lead's 10 s is inside both
}

#[test]
fn the_elapsed_time_is_whole_seconds_rounded_down() {
    let (_dir, mut store) = store_with(&["api"]);
    run(&mut store, &worked("api", "a", "10:00:00.000", Some("10:01:30.999")));
    assert_eq!(elapsed(&mut store, "11:00:00"), vec![Some(90)]); // 90.999 s
}
