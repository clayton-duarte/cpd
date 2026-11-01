//! Pulse's read model (`read::manager_slot`, `read::pulse`), against a store prepared through the store
//! API. Every read takes a fixed `now_ms`: no wall clock, no sleeps.

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, PlanId};
use cpd_core::store::Store;

use crate::read::{self, Bucket, Dot, Kind, PulseRow};

/// 2026-10-18T15:07:30.000Z; with 12 buckets the window is 14:10:00 .. 15:10:00, the current bucket (index 11) from 15:05:00.
const NOW: i64 = 1_792_336_050_000;
const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// A store in a temporary home, which lives as long as the returned directory.
fn open(dir: &tempfile::TempDir) -> Store {
    Store::open(
        &CpdHome::resolve(Some(dir.path().into()), None).unwrap(),
        5000,
    )
    .unwrap()
}

/// Runs `text` as the store's own write.
fn sql(store: &mut Store, text: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(text)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

fn ctx(id: &str) -> ContextId {
    ContextId::parse(id).unwrap()
}

fn context(id: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('{id}', 'T {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');"
    )
}

/// A Plan `id` in `context`, phase dispatch, with Lead Session `l-<id>` in `state` (`blocked` takes a label).
fn plan(id: &str, context: &str, created: &str, state: &str) -> String {
    let label = (state == "blocked").then_some("cpd: Workflow feedback");
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
         VALUES ('{id}', '{context}', 'Plan {id}', 'dispatch', 'l-{id}', '{created}', '2026-01-02T00:00:00.000Z');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
         VALUES ('{id}', 'repo', '/repo', 'main', 'dispatch', 'cpd/{id}', '/wt', cpd_txn(), '{created}', '{created}');
         {}",
        session(&format!("l-{id}"), "lead", context, Some(id), None, state, label)
    )
}

/// A Plan `id` already done at `done_at`.
fn plan_done(id: &str, context: &str, created: &str, done_at: &str) -> String {
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
         VALUES ('{id}', '{context}', 'Plan {id}', 'done', 'l-{id}', '{created}', '2026-01-02T00:00:00.000Z', '{done_at}');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
         VALUES ('{id}', 'repo', '/repo', 'main', 'dispatch', 'cpd/{id}', '/wt', cpd_txn(), '{created}', '{created}');
         {}",
        session(&format!("l-{id}"), "lead", context, Some(id), None, "stopped", None)
    )
}

fn session(
    id: &str,
    role: &str,
    context: &str,
    plan: Option<&str>,
    attempt: Option<&str>,
    state: &str,
    label: Option<&str>,
) -> String {
    let q = |v: Option<&str>| v.map_or("NULL".to_owned(), |v| format!("'{v}'"));
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', '{role}', '{context}', {}, {}, 'name {id}', '/s/{id}.jsonl', '/repo', 'O3', NULL, '{state}', {},
                 '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');",
        q(plan),
        q(attempt),
        q(label)
    )
}

fn job(plan: &str, key: &str, status: &str) -> String {
    let fp = if status == "draft" {
        "NULL".to_owned()
    } else {
        format!("'{FP}'")
    };
    let sha = if status == "merged" {
        "'abc123'"
    } else {
        "NULL"
    };
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, merged_sha, created_at)
         VALUES ('{plan}', 'repo', '{key}', 'Job', 'S3', 'G.', 'W.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, {fp}, cpd_txn(),
                 {sha}, '2026-01-02T00:00:00.000Z');"
    )
}

/// Attempt `<job>-a1` started at `started`; `ended` is `None` while running.
fn attempt(plan: &str, job: &str, started: &str, ended: Option<&str>) -> String {
    let (status, outcome, cause, ended_at) = match ended {
        Some(at) => ("ended", "'failed'", "'crashed'", format!("'{at}'")),
        None => ("running", "NULL", "NULL", "NULL".to_owned()),
    };
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at)
         VALUES ('{plan}', '{job}-a1', '{job}', 1, 'S3', 'ladder', '{status}', {outcome}, {cause}, 'cpd/{plan}--{job}-a1', '/w',
                 '{started}', {ended_at});"
    )
}

fn event(
    at: &str,
    kind: &str,
    context: &str,
    plan: Option<&str>,
    session: Option<&str>,
    data: &str,
) -> String {
    let q = |v: Option<&str>| v.map_or("NULL".to_owned(), |v| format!("'{v}'"));
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '{at}', '{kind}', '{context}', {}, {}, 'fixture', '{data}');",
        q(plan),
        q(session)
    )
}

/// A plain Event of the Plan at `at` on 2026-10-18, `hhmmss` like `15:04:59.999`.
fn ev(hhmmss: &str, kind: &str, plan: &str) -> String {
    event(
        &format!("2026-10-18T{hhmmss}Z"),
        kind,
        "ctx",
        Some(plan),
        None,
        "{}",
    )
}

fn moved(at: &str, context: &str, session: &str, place: &str) -> String {
    event(
        at,
        "session-moved",
        context,
        None,
        Some(session),
        &format!(r#"{{"pane":"%1","window":"@1","place":"{place}"}}"#),
    )
}

/// `ctx` with Plans `p1` (first) and `p2`, and Context `other` with `o1`.
fn base() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(&dir);
    let mut s = context("ctx") + &context("other");
    s += &plan("p1", "ctx", "2026-01-02T00:00:00.000Z", "idle");
    s += &plan("p2", "ctx", "2026-01-02T00:00:01.000Z", "idle");
    s += &plan("o1", "other", "2026-01-02T00:00:00.000Z", "idle");
    s += &ev("15:06:00.000", "job-merged", "o1").replace("'ctx'", "'other'");
    sql(&mut store, &s);
    (dir, store)
}

fn read(store: &mut Store, buckets: usize) -> read::Pulse {
    read::pulse(store, &ctx("ctx"), NOW, buckets).unwrap()
}

fn row<'a>(p: &'a read::Pulse, plan: &str) -> &'a PulseRow {
    p.rows
        .iter()
        .find(|r| r.plan == PlanId::parse(plan).unwrap())
        .unwrap()
}

fn counts(r: &PulseRow) -> Vec<u32> {
    r.buckets.iter().map(|b| b.count).collect()
}

// ---------- manager_slot ----------

#[test]
fn the_manager_slot_is_the_newest_chat_move_when_a_manager_made_it() {
    let (_dir, mut store) = base();
    assert_eq!(
        read::manager_slot(&mut store).unwrap(),
        None,
        "nobody was ever in the chat slot"
    );

    let mut s = session("m-ctx", "manager", "ctx", None, None, "idle", None);
    s += &moved("2026-10-18T15:00:00.000Z", "ctx", "l-p1", "chat");
    s += &moved("2026-10-18T15:01:00.000Z", "ctx", "m-ctx", "chat");
    sql(&mut store, &s);
    let slot = read::manager_slot(&mut store).unwrap().unwrap();
    assert_eq!(
        (slot.session.as_str(), slot.context.as_str()),
        ("m-ctx", "ctx")
    );
    let newest: i64 = store
        .read(|c| Ok(c.query_row("SELECT max(id) FROM events", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(slot.moved_event, newest);

    // A parked move of another Session does not take the slot; a Lead's chat move does.
    sql(
        &mut store,
        &moved("2026-10-18T15:02:00.000Z", "ctx", "l-p2", "parked"),
    );
    assert!(read::manager_slot(&mut store).unwrap().is_some());
    sql(
        &mut store,
        &moved("2026-10-18T15:03:00.000Z", "ctx", "l-p2", "chat"),
    );
    assert_eq!(
        read::manager_slot(&mut store).unwrap(),
        None,
        "a Lead moved to chat after the Manager"
    );
}

#[test]
fn a_lead_in_the_slot_alone_is_no_manager() {
    let (_dir, mut store) = base();
    sql(
        &mut store,
        &moved("2026-10-18T15:00:00.000Z", "ctx", "l-p1", "chat"),
    );
    assert_eq!(read::manager_slot(&mut store).unwrap(), None);
}

// ---------- rows ----------

#[test]
fn the_pulse_rows_are_in_tree_order_by_lead_dot_tier_then_newest_first() {
    let (_dir, mut store) = base();
    // `p0` is the oldest Plan and its Lead has an open magenta Inbox item; `t1` and `t2` share one created_at.
    let mut s = plan("p0", "ctx", "2026-01-01T00:00:00.000Z", "idle");
    s += &plan("p3", "ctx", "2026-01-02T00:00:05.000Z", "idle");
    s += &plan("t1", "ctx", "2026-01-02T00:00:03.000Z", "idle");
    s += &plan("t2", "ctx", "2026-01-02T00:00:03.000Z", "idle");
    s += &plan("arch", "ctx", "2026-01-02T00:00:02.000Z", "idle");
    s += &plan_done(
        "old",
        "ctx",
        "2026-01-02T00:00:03.500Z",
        "2026-10-18T14:09:59.999Z",
    );
    s += &plan_done(
        "new",
        "ctx",
        "2026-01-02T00:00:04.000Z",
        "2026-10-18T14:10:00.000Z",
    );
    s += "INSERT INTO inbox_items (kind, urgency, subject, context, plan, session, label, opened_at)
          VALUES ('dialog', 'magenta', 'session:l-p0', 'ctx', 'p0', 'l-p0', 'p0 asks you', '2026-10-18T15:00:00.000Z');";
    // The newest Event names the oldest-created tied Plan: Events never reorder.
    s += &ev("15:06:00.000", "job-merged", "p1");
    s += &ev("15:01:00.000", "job-merged", "p3");
    sql(&mut store, &s);
    sql(
        &mut store,
        "UPDATE plans SET archived_at = '2026-10-18T15:00:00.000Z' WHERE id = 'arch';",
    );
    let p = read(&mut store, 12);
    let ids: Vec<&str> = p.rows.iter().map(|r| r.plan.as_str()).collect();
    assert_eq!(ids, ["p0", "p3", "new", "t1", "t2", "p2", "p1"]);
    assert_eq!(p.context, ctx("ctx"));
    assert_eq!(row(&p, "p1").lead.as_str(), "l-p1");
    assert_eq!(row(&p, "p1").dot, Dot::Idle);
    assert_eq!(row(&p, "p0").dot, Dot::Magenta);
    assert!(row(&p, "p1").buckets.len() == 12);
}

// ---------- buckets ----------

#[test]
fn every_activity_kind_counts_and_the_others_do_not() {
    let (_dir, mut store) = base();
    let counted = [
        "plan-phase",
        "plan-review",
        "plan-done",
        "job-added",
        "job-edited",
        "job-merged",
        "job-failed",
        "job-retried",
        "job-cancel-requested",
        "job-cancelled",
        "attempt-started",
        "attempt-status",
        "attempt-retry",
        "attempt-nudge",
        "all-merged",
        "pr-feedback",
        "workflow-added",
        "workflow-edited",
        "workflow-cancelled",
        "workflow-started",
        "pr-opened",
        "pr-merged",
        "escalation",
        "escalation-answered",
        "escalation-expired",
        "error",
    ];
    let ignored = [
        "session-moved",
        "session-registered",
        "session-renamed",
        "session-archived",
        "session-message",
        "context-created",
        "context-record",
        "context-grounded",
        "context-renamed",
        "context-owner",
        "context-archived",
        "plan-created",
        "plan-renamed",
        "plan-archived",
        "watch-started",
        "watch-stopped",
    ];
    let mut s = String::new();
    for kind in counted {
        s += &ev("15:06:00.000", kind, "p1");
    }
    for kind in ignored {
        s += &ev("15:06:00.000", kind, "p2");
    }
    sql(&mut store, &s);
    let p = read(&mut store, 12);
    assert_eq!(row(&p, "p1").buckets[11].count, counted.len() as u32);
    assert_eq!(counts(row(&p, "p2")), vec![0; 12]);
}

#[test]
fn only_a_leads_turns_count_and_so_does_a_side_fork() {
    let (_dir, mut store) = base();
    let mut s = job("p1", "a", "running")
        + &attempt(
            "p1",
            "a",
            "2026-10-18T13:00:00.000Z",
            Some("2026-10-18T13:01:00.000Z"),
        );
    s += &session("f-p1", "lead", "ctx", Some("p1"), None, "idle", None);
    s += &session(
        "w-1",
        "worker",
        "ctx",
        Some("p1"),
        Some("a-a1"),
        "idle",
        None,
    );
    sql(&mut store, &s);
    let turn = |at: &str, session: &str, to: &str| {
        event(
            &format!("2026-10-18T{at}Z"),
            "session-state",
            "ctx",
            Some("p1"),
            Some(session),
            &format!(r#"{{"from":"idle","to":"{to}"}}"#),
        )
    };
    let mut s = turn("15:06:00.000", "l-p1", "working");
    s += &turn("15:06:01.000", "f-p1", "working");
    s += &turn("15:06:02.000", "l-p1", "idle"); // not a turn
    s += &turn("15:06:03.000", "w-1", "working"); // a worker's
    sql(&mut store, &s);
    assert_eq!(row(&read(&mut store, 12), "p1").buckets[11].count, 2);
}

#[test]
fn buckets_are_wall_clock_five_minute_marks() {
    let (_dir, mut store) = base();
    let mut s = String::new();
    s += &ev("15:04:59.999", "job-added", "p1"); // bucket 10: 15:00 .. 15:05
    s += &ev("15:05:00.000", "job-added", "p1"); // bucket 11
    s += &ev("15:09:59.999", "job-added", "p1"); // bucket 11
    s += &ev("14:10:00.000", "job-added", "p1"); // bucket 0, the window's first instant
    s += &ev("14:09:59.999", "job-added", "p1"); // before the window
    s += &ev("15:10:00.000", "job-added", "p1"); // after the current bucket
    sql(&mut store, &s);
    let p = read(&mut store, 12);
    assert_eq!(counts(row(&p, "p1")), [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2]);
    assert_eq!(row(&p, "p1").merges, 0);
    // A longer window sees the earlier Event.
    let p = read(&mut store, 13);
    assert_eq!(counts(row(&p, "p1"))[0], 1);
}

#[test]
fn merges_set_their_flags_and_are_counted_in_the_window() {
    let (_dir, mut store) = base();
    let mut s = String::new();
    s += &ev("15:01:00.000", "job-merged", "p1");
    s += &ev("15:02:00.000", "job-merged", "p1");
    s += &ev("15:06:00.000", "pr-merged", "p1");
    s += &ev("14:00:00.000", "job-merged", "p1"); // before the window
    sql(&mut store, &s);
    let p = read(&mut store, 12);
    let r = row(&p, "p1");
    assert_eq!(r.merges, 3);
    assert_eq!(
        r.buckets[10],
        Bucket {
            count: 2,
            kind: Some(Kind::Merged),
            job_merged: true,
            pr_merged: false,
            worker_ran: false
        }
    );
    assert_eq!(
        r.buckets[11],
        Bucket {
            count: 1,
            kind: Some(Kind::Merged),
            job_merged: false,
            pr_merged: true,
            worker_ran: false
        }
    );
}

#[test]
fn an_attempt_marks_every_bucket_it_overlaps_through_now_while_not_ended() {
    let (_dir, mut store) = base();
    let mut s = job("p1", "a", "running") + &job("p1", "b", "merged") + &job("p2", "c", "running");
    s += &attempt("p1", "a", "2026-10-18T14:22:00.000Z", None); // from bucket 2 through the current one
    s += &attempt(
        "p1",
        "b",
        "2026-10-18T13:00:00.000Z",
        Some("2026-10-18T14:10:00.000Z"),
    ); // ends on the first instant
    s += &attempt(
        "p2",
        "c",
        "2026-10-18T14:31:00.000Z",
        Some("2026-10-18T14:34:00.000Z"),
    ); // inside bucket 4
    sql(&mut store, &s);
    let p = read(&mut store, 12);
    let ran =
        |plan: &str| -> Vec<bool> { row(&p, plan).buckets.iter().map(|b| b.worker_ran).collect() };
    assert_eq!(
        ran("p1"),
        [
            true, false, true, true, true, true, true, true, true, true, true, true
        ]
    );
    assert_eq!(
        ran("p2"),
        [
            false, false, false, false, true, false, false, false, false, false, false, false
        ]
    );
}

// ---------- max, merges, age ----------

#[test]
fn max_is_the_largest_bucket_over_all_rows_and_age_counts_whole_minutes() {
    let (_dir, mut store) = base();
    let mut s = String::new();
    for _ in 0..3 {
        s += &ev("15:06:00.000", "job-added", "p1");
    }
    s += &ev("15:06:00.000", "job-added", "p2");
    s += &ev("15:06:00.000", "job-added", "p2");
    sql(&mut store, &s);
    let p = read(&mut store, 12);
    assert_eq!(p.max, 3);
    assert_eq!(row(&p, "p1").age_minutes, Some(1), "15:06:00 to 15:07:30");

    sql(
        &mut store,
        &ev("12:00:00.000", "job-added", "p2")
            .replace("2026-10-18T12:00:00.000Z", "2026-10-17T12:07:30.001Z"),
    );
    let q = read::pulse(&mut store, &ctx("ctx"), NOW, 12).unwrap();
    assert_eq!(
        row(&q, "p2").age_minutes,
        Some(1),
        "the latest, not the oldest"
    );
}

#[test]
fn age_reads_an_event_older_than_the_window_zero_while_an_attempt_runs_and_none_without_activity() {
    let (_dir, mut store) = base();
    assert_eq!(read(&mut store, 12).max, 0);
    assert_eq!(row(&read(&mut store, 12), "p1").age_minutes, None);

    sql(&mut store, &ev("12:07:30.001", "job-added", "p1"));
    assert_eq!(row(&read(&mut store, 12), "p1").age_minutes, Some(179));

    sql(
        &mut store,
        &(job("p1", "a", "running") + &attempt("p1", "a", "2026-10-18T14:00:00.000Z", None)),
    );
    let p = read(&mut store, 12);
    assert_eq!(row(&p, "p1").age_minutes, Some(0));
    assert_eq!(row(&p, "p2").age_minutes, None);
}

// ---------- quiet ----------

#[test]
fn quiet_needs_work_no_attempt_no_recent_activity_and_a_lead_not_waiting_on_the_user() {
    let (_dir, mut store) = base();
    let quiet = |store: &mut Store, plan: &str| row(&read(store, 12), plan).quiet;

    assert!(!quiet(&mut store, "p1"), "no Jobs");
    sql(&mut store, &job("p1", "d", "draft"));
    assert!(!quiet(&mut store, "p1"), "drafts are not work");
    sql(&mut store, &job("p1", "q", "queued"));
    assert!(quiet(&mut store, "p1"), "queued work, no activity ever");

    sql(&mut store, &ev("14:37:30.000", "job-added", "p1")); // exactly 30 min old
    assert!(quiet(&mut store, "p1"));
    sql(&mut store, &ev("14:37:30.001", "job-added", "p1")); // 29 min 59.999 s
    assert!(!quiet(&mut store, "p1"), "recent activity");

    // Another Plan, still without activity: the Lead waiting on the User, then an Attempt not ended.
    sql(&mut store, &job("p2", "q", "queued"));
    assert!(quiet(&mut store, "p2"));
    // The Lead waits on the User: blocked, with the open `dialog` item the engine's E8 gives it (the dots come from
    // the Inbox, commands.md §17).
    sql(
        &mut store,
        "UPDATE sessions SET state = 'blocked', state_label = 'cpd: Workflow feedback' WHERE id = 'l-p2';
         INSERT INTO inbox_items (kind, urgency, subject, label, context, plan, session, opened_at)
           VALUES ('dialog', 'red', 'session:l-p2', 'p2 asks you', 'ctx', 'p2', 'l-p2', '2026-10-18T15:00:00.000Z');",
    );
    let p = read(&mut store, 12);
    assert!(row(&p, "p2").dot.needs_you());
    assert!(!row(&p, "p2").quiet, "waiting on the User");
    sql(
        &mut store,
        "UPDATE sessions SET state = 'idle', state_label = NULL WHERE id = 'l-p2';
         UPDATE inbox_items SET closed_at = '2026-10-18T15:01:00.000Z' WHERE subject = 'session:l-p2';",
    );
    assert!(quiet(&mut store, "p2"));
    sql(
        &mut store,
        &attempt("p2", "q", "2026-10-18T12:00:00.000Z", None),
    );
    assert!(!quiet(&mut store, "p2"), "an Attempt is not ended");
}

// ---------- the transaction ----------

#[test]
fn the_cursor_is_the_highest_event_id_of_the_read() {
    let (_dir, mut store) = base();
    sql(&mut store, &ev("15:06:00.000", "job-added", "p1"));
    let newest: i64 = store
        .read(|c| Ok(c.query_row("SELECT max(id) FROM events", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(read(&mut store, 12).events_cursor, newest);
}
