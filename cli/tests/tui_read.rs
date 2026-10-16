//! The TUI's read model (commands.md §17), against a store prepared through the
//! store API (`Store::write`, as the other areas' tests do, because Plans, Jobs
//! and Attempts belong to their areas) and through the built `cpd` commands.

mod common;

use std::time::{Duration, Instant};

use common::TestEnv;
use cpd_core::config::Tier;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, EscalationId, JobKey, PlanId, SessionId};
use cpd_core::plan::{Phase, jobs, plan_fingerprint};
use cpd_core::session::State;
use cpd_core::store::Store;
use cpd_tui::read::{
    self, AttemptStatus, Cause, Checks, Engine, EscalationStatus, JobStatus, Origin, Outcome, Place, PrState, ReviewStatus,
};
use rusqlite::Connection;

// ---------- fixtures ----------

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn sql(store: &mut Store, text: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(text)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

fn plan(s: &str) -> PlanId {
    PlanId::parse(s).unwrap()
}

fn attempt(s: &str) -> AttemptId {
    AttemptId::parse(s).unwrap()
}

fn session(s: &str) -> SessionId {
    SessionId::parse(s).unwrap()
}

fn key(s: &str) -> JobKey {
    JobKey::parse(s).unwrap()
}

fn tier(s: &str) -> Tier {
    Tier::parse(s).unwrap()
}

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn context(id: &str, grounded: Option<&str>) -> String {
    let (lead, seq) = grounded.map_or(("NULL".to_owned(), "NULL"), |g| (format!("'{g}'"), "0"));
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on, lead_session, grounded_seq)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1', {lead}, {seq});"
    )
}

/// `phase` decides the columns the schema ties to it; `pr` is `(number, pr_seen)`.
fn plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str, pr: Option<(i64, &str)>) -> String {
    let late = !matches!(phase, "grounding" | "plan");
    let worktree = if late { "'/wt'" } else { "NULL" };
    let locked = if late { "'2026-01-02T00:00:00.000Z'" } else { "NULL" };
    let done = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
    let (number, url, opened, seen, polled) = match pr {
        Some((n, seen)) => (
            n.to_string(),
            format!("'https://example.test/pull/{n}'"),
            "'2026-01-05T00:00:00.000Z'".to_owned(),
            seen.to_owned(),
            "'2026-01-06T00:00:00.000Z'".to_owned(),
        ),
        None => (
            "NULL".into(),
            "NULL".into(),
            "NULL".into(),
            r#"{"checks":{},"reviewIds":[],"commentIds":[]}"#.to_owned(),
            "NULL".into(),
        ),
    };
    format!(
        "INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, pr_number,
                            pr_url, pr_opened_at, pr_seen, pr_polled_at, created_at, locked_at, done_at)
         VALUES ('{id}', '{context}', 'Plan {id}', '/repo', 'main', 'cpd/{id}', {worktree}, '{phase}', '{lead}',
                 {number}, {url}, {opened}, '{seen}', {polled}, '{created}', {locked}, {done});"
    )
}

fn session_row(id: &str, role: &str, context: &str, plan: &str, attempt: Option<&str>, state: &str, label: Option<&str>, created: &str) -> String {
    let plan = if plan.is_empty() { "NULL".to_owned() } else { format!("'{plan}'") };
    let attempt = attempt.map_or("NULL".to_owned(), |a| format!("'{a}'"));
    let label = label.map_or("NULL".to_owned(), |l| format!("'{l}'"));
    let tier = if role == "lead" { "O3" } else { "S3" };
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', '{role}', '{context}', {plan}, {attempt}, 'name of {id}', '/sessions/{id}.jsonl', '/repo', '{tier}',
                 NULL, '{state}', {label}, '2026-01-03T00:00:00.000Z', '{created}');"
    )
}

fn job_row(plan: &str, key: &str, status: &str, tier: &str, needs: &[&str]) -> String {
    let fingerprint = if status == "draft" { "NULL".to_owned() } else { format!("'{FP}'") };
    let merged = if status == "merged" { "'abc123'" } else { "NULL" };
    let failed = if status == "failed" { "'crashed'" } else { "NULL" };
    let (reason, requested) =
        if status == "cancelled" { ("'not needed'", "'2026-01-04T00:00:00.000Z'") } else { ("NULL", "NULL") };
    let mut out = format!(
        "INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('{plan}', '{key}', 'Job {key}', '{tier}', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, {fingerprint},
                 cpd_txn(), {reason}, {requested}, {failed}, {merged}, '2026-01-02T00:00:00.000Z');"
    );
    for n in needs {
        out += &format!("INSERT INTO dependencies (plan, job, needs) VALUES ('{plan}', '{key}', '{n}');");
    }
    out
}

/// An Attempt `<job>-a<number>`; `status` `ended` takes `outcome`.
fn attempt_row(plan: &str, job: &str, number: u32, tier: &str, status: &str, ended: Option<(&str, &str)>, worker: Option<&str>) -> String {
    let id = format!("{job}-a{number}");
    let (outcome, cause, merged, ended_at) = match ended {
        Some((outcome, cause)) => (
            format!("'{outcome}'"),
            format!("'{cause}'"),
            if outcome == "merged" { "'abc123'" } else { "NULL" }.to_owned(),
            "'2026-01-03T01:00:00.000Z'".to_owned(),
        ),
        None => ("NULL".into(), "NULL".into(), "NULL".into(), "NULL".into()),
    };
    let (sha, at) = if status == "merging" { ("'def456'", "'2026-01-03T00:30:00.000Z'") } else { ("NULL", "NULL") };
    let worker = worker.map_or("NULL".to_owned(), |w| format!("'{w}'"));
    let origin = if number == 1 { "ladder" } else { "retry" };
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, worker_session,
                               submitted_sha, submitted_at, merged_sha, started_at, ended_at)
         VALUES ('{plan}', '{id}', '{job}', {number}, '{tier}', '{origin}', '{status}', {outcome}, {cause}, 'cpd/{plan}--{id}',
                 '/w/{id}', {worker}, {sha}, {at}, {merged}, '2026-01-03T00:{number:02}:00.000Z', {ended_at});"
    )
}

fn escalation_row(plan: &str, attempt: &str, number: u32, status: &str, asked: &str) -> String {
    let (answer, answered, expired) = match status {
        "answered" => ("'Use v2.'", "'2026-01-03T02:00:00.000Z'", "NULL"),
        "expired" => ("NULL", "NULL", "'2026-01-03T02:00:00.000Z'"),
        _ => ("NULL", "NULL", "NULL"),
    };
    format!(
        "INSERT INTO escalations (plan, id, attempt, number, question, status, answer, asker_pid, asked_at, answered_at, expired_at)
         VALUES ('{plan}', '{attempt}-q{number}', '{attempt}', {number}, 'Question {attempt}-q{number}?', '{status}', {answer}, 1,
                 '{asked}', {answered}, {expired});"
    )
}

fn event(kind: &str, context: Option<&str>, plan: Option<&str>, session: Option<&str>, data: &str) -> String {
    let q = |v: Option<&str>| v.map_or("NULL".to_owned(), |v| format!("'{v}'"));
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', '{kind}', {}, {}, {}, 'fixture {kind}', '{data}');",
        q(context),
        q(plan),
        q(session)
    )
}

fn moved(context: &str, plan: Option<&str>, session: &str, place: &str) -> String {
    event("session-moved", Some(context), plan, Some(session), &format!(r#"{{"pane":"%1","window":"@1","place":"{place}"}}"#))
}

/// Three Contexts, the Plans below, Jobs in every status, Attempts, a PR, Escalations and Sessions.
///
/// - `alpha` (grounded Lead `g-alpha`): `ship` (dispatch, Lead `l-ship`, side fork `f-ship`) with one Job in
///   every status; `sketch` (phase plan, drafts only).
/// - `beta`: `rel` (phase pr, PR 7 with a failing check), `shut` (pr, closed unmerged), `old` (done, merged PR).
/// - `gamma`: nothing.
///
/// The chat slot holds `l-ship`; `l-rel` was there before and is parked.
fn populate(store: &mut Store) {
    let mut s = String::new();
    s += &context("alpha", Some("g-alpha"));
    s += &context("beta", None);
    s += &context("gamma", None);
    s += &plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:00.000Z", None);
    s += &plan_row("sketch", "alpha", "plan", "l-sketch", "2026-01-02T00:00:01.000Z", None);
    s += &plan_row(
        "rel",
        "beta",
        "pr",
        "l-rel",
        "2026-01-02T00:00:02.000Z",
        Some((7, r#"{"checks":{"ci":"FAILURE","lint":"SUCCESS"},"reviewIds":["r1"],"commentIds":["c1","c2"]}"#)),
    );
    s += &plan_row(
        "shut",
        "beta",
        "pr",
        "l-shut",
        "2026-01-02T00:00:03.000Z",
        Some((8, r#"{"checks":{},"reviewIds":[],"commentIds":[],"closed":true}"#)),
    );
    s += &plan_row(
        "old",
        "beta",
        "done",
        "l-old",
        "2026-01-01T12:00:00.000Z",
        Some((3, r#"{"checks":{"ci":"SUCCESS"},"reviewIds":[],"commentIds":[]}"#)),
    );

    s += &session_row("g-alpha", "lead", "alpha", "", None, "idle", None, "2026-01-02T00:00:00.000Z");
    s += &session_row("l-ship", "lead", "alpha", "ship", None, "working", None, "2026-01-02T00:00:00.000Z");
    s += &session_row("f-ship", "lead", "alpha", "ship", None, "idle", None, "2026-01-02T00:00:09.000Z");
    s += &session_row("l-sketch", "lead", "alpha", "sketch", None, "idle", None, "2026-01-02T00:00:01.000Z");
    s += &session_row("l-rel", "lead", "beta", "rel", None, "blocked", Some("cpd: PR feedback"), "2026-01-02T00:00:02.000Z");
    s += &session_row("l-shut", "lead", "beta", "shut", None, "idle", None, "2026-01-02T00:00:03.000Z");
    s += &session_row("l-old", "lead", "beta", "old", None, "stopped", None, "2026-01-01T12:00:00.000Z");

    // ship: every Job status.
    s += &job_row("ship", "a", "merged", "O3", &[]);
    s += &job_row("ship", "b", "running", "S3", &["a"]); // its Attempt is merging
    s += &job_row("ship", "c", "running", "S2", &["a"]); // second Attempt running
    s += &job_row("ship", "d", "queued", "S3", &["b", "c"]);
    s += &job_row("ship", "e", "failed", "H1", &[]);
    s += &job_row("ship", "f", "cancelled", "S3", &[]);
    s += &job_row("ship", "g", "draft", "S3", &["d"]);
    s += &session_row("w-c2", "worker", "alpha", "ship", Some("c-a2"), "working", None, "2026-01-03T00:02:00.000Z");
    s += &session_row("w-b1", "worker", "alpha", "ship", Some("b-a1"), "idle", None, "2026-01-03T00:01:00.000Z");
    s += &attempt_row("ship", "a", 1, "O3", "ended", Some(("merged", "merged")), None);
    s += &attempt_row("ship", "b", 1, "S3", "merging", None, Some("w-b1"));
    s += &attempt_row("ship", "c", 1, "S2", "ended", Some(("failed", "timed_out")), None);
    s += &attempt_row("ship", "c", 2, "S3", "running", None, Some("w-c2"));
    s += &attempt_row("ship", "e", 1, "H1", "ended", Some(("failed", "crashed")), None);
    s += &escalation_row("ship", "a-a1", 1, "answered", "2026-01-03T00:10:00.000Z");
    s += &escalation_row("ship", "e-a1", 1, "expired", "2026-01-03T00:11:00.000Z");
    s += &escalation_row("ship", "c-a2", 1, "open", "2026-01-03T00:20:00.000Z");

    // sketch: drafts.
    s += &job_row("sketch", "s1", "draft", "S3", &[]);
    s += &job_row("sketch", "s2", "draft", "S3", &["s1"]);

    // rel: a Job still running, with an Attempt asking.
    s += &job_row("rel", "r1", "running", "S3", &[]);
    s += &session_row("w-r1", "worker", "beta", "rel", Some("r1-a1"), "blocked", Some("cpd: question to the Lead"), "2026-01-03T00:03:00.000Z");
    s += &attempt_row("rel", "r1", 1, "S3", "running", None, Some("w-r1"));
    s += &escalation_row("rel", "r1-a1", 1, "open", "2026-01-03T00:05:00.000Z");
    s += &job_row("rel", "r0", "merged", "S3", &[]);

    // Context records: a note, an answered and an unanswered question.
    for (id, kind, extra) in [("m1-n1", "note", "NULL"), ("m1-q1", "question", "NULL"), ("m1-q2", "question", "NULL"), ("m1-a1", "answer", "'m1-q1'")] {
        s += &format!(
            "INSERT INTO context_records (context, id, kind, at, machine, text, question)
             VALUES ('alpha', '{id}', '{kind}', '2026-01-02T00:00:00.000Z', 'm1', 'text of {id}', {extra});"
        );
    }

    // Events: the chat slot, the PR's feedback, the engine.
    s += &moved("beta", Some("rel"), "l-rel", "chat");
    s += &moved("beta", Some("rel"), "l-rel", "parked");
    s += &moved("alpha", Some("ship"), "l-ship", "chat");
    s += &moved("alpha", None, "g-alpha", "work");
    s += &event(
        "pr-feedback",
        Some("beta"),
        Some("rel"),
        None,
        r#"{"checks":[{"name":"ci","conclusion":"FAILURE"}],"reviews":[{"id":"r1","author":"x","state":"COMMENTED","body":""}],"comments":[],"state":"OPEN"}"#,
    );
    s += &event("watch-started", None, None, None, &format!(r#"{{"pid":{}}}"#, std::process::id()));
    sql(store, &s);

    let fingerprint = store.read(|c| Ok(plan_fingerprint(&jobs::load(c, &plan("ship"))?))).unwrap();
    sql(
        store,
        &format!(
            "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, feedback, requested_at, decided_at)
             VALUES ('ship', 'r1', 1, '{FP}', 'rejected', 'Split job d.', '2026-01-02T00:00:00.000Z', '2026-01-02T00:30:00.000Z');
             INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at)
             VALUES ('ship', 'r2', 2, '{fingerprint}', 'pending', '2026-01-02T01:00:00.000Z');"
        ),
    );
}

fn fixture() -> (TestEnv, Store) {
    let env = TestEnv::new();
    let mut store = open(&env);
    populate(&mut store);
    (env, store)
}

// ---------- the tree: context list, session list ----------

#[test]
fn the_tree_is_contexts_then_plans_then_lead_sessions_with_their_states() {
    let (_env, mut store) = fixture();
    let tree = read::tree(&mut store).unwrap();

    let ids: Vec<&str> = tree.contexts.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["alpha", "beta", "gamma"]);
    let [alpha, beta, gamma] = &tree.contexts[..] else { panic!() };

    assert_eq!((alpha.title.as_str(), alpha.records, alpha.open_questions, alpha.sync_failing), ("Title of alpha", 4, 1, false));
    let grounded = alpha.grounded_lead.as_ref().unwrap();
    assert_eq!((grounded.id.as_str(), grounded.plan.as_ref(), grounded.grounded_lead, grounded.place), ("g-alpha", None, true, Place::Work));
    assert!(beta.grounded_lead.is_none() && gamma.grounded_lead.is_none());
    assert!(gamma.plans.is_empty());

    let plans: Vec<&str> = alpha.plans.iter().map(|p| p.head.id.as_str()).collect();
    assert_eq!(plans, ["ship", "sketch"], "oldest first");
    let ship = &alpha.plans[0];
    assert_eq!(ship.head.phase, Phase::Dispatch);
    let sessions: Vec<(&str, State, Place, bool)> =
        ship.sessions.iter().map(|s| (s.id.as_str(), s.state, s.place, s.plan_lead)).collect();
    assert_eq!(
        sessions,
        [("l-ship", State::Working, Place::Chat, true), ("f-ship", State::Idle, Place::None, false)],
        "the Lead first, then the side fork; the Workers and the grounded Lead are not in the tree's Plans"
    );
    assert!(ship.sessions[0].is_active() && !ship.sessions[1].is_active());
    assert_eq!(ship.lead().unwrap().id.as_str(), "l-ship");
    assert_eq!(ship.sessions[0].name, "name of l-ship");

    let rel = &beta.plans[1];
    assert_eq!(beta.plans.iter().map(|p| p.head.id.as_str()).collect::<Vec<_>>(), ["old", "rel", "shut"]);
    let lead = &rel.sessions[0];
    assert_eq!((lead.state, lead.state_label.as_deref(), lead.place), (State::Blocked, Some("cpd: PR feedback"), Place::Parked));
    assert!(rel.needs_you() && beta.needs_you() && alpha.needs_you(), "a blocked Lead and open Escalations need the User");
    assert!(!gamma.needs_you());
    assert!(!beta.plans[2].needs_you(), "shut: nothing open");

    assert_eq!(tree.events_cursor, last_event(&mut store));
}

#[test]
fn archived_objects_are_in_the_tree_with_their_archived_at() {
    let (_env, mut store) = fixture();
    let before = read::tree(&mut store).unwrap();
    assert!(
        before.contexts.iter().all(|c| c.archived_at.is_none() && c.plans.iter().all(|p| p.head.archived_at.is_none() && p.sessions.iter().all(|s| s.archived_at.is_none()))),
        "nothing is archived by default"
    );
    sql(
        &mut store,
        "UPDATE contexts SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'gamma';
         UPDATE plans SET archived_at = '2026-02-02T00:00:00.000Z' WHERE id = 'sketch';
         UPDATE sessions SET archived_at = '2026-02-03T00:00:00.000Z' WHERE id = 'f-ship';",
    );
    let tree = read::tree(&mut store).unwrap();
    assert_eq!(tree.contexts.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["alpha", "beta", "gamma"], "archived ones are included");
    let [alpha, _, gamma] = &tree.contexts[..] else { panic!() };
    assert_eq!((gamma.archived_at.as_deref(), gamma.is_archived(), alpha.is_archived()), (Some("2026-02-01T00:00:00.000Z"), true, false));
    let (ship, sketch) = (&alpha.plans[0], &alpha.plans[1]);
    assert_eq!((sketch.head.archived_at.as_deref(), sketch.head.is_archived(), ship.head.is_archived()), (Some("2026-02-02T00:00:00.000Z"), true, false));
    assert_eq!(sketch.sessions.len(), 1, "an archived Plan keeps its Lead");
    let fork = &ship.sessions[1];
    assert_eq!((fork.id.as_str(), fork.archived_at.as_deref(), fork.is_archived()), ("f-ship", Some("2026-02-03T00:00:00.000Z"), true));
    assert!(!ship.sessions[0].is_archived());
}

fn settled(session: &str) -> String {
    event("session-state", Some("alpha"), Some("sketch"), Some(session), r#"{"from":"working","to":"idle","label":null}"#)
}

/// The ● of a Session, from its state and its Events (cockpit parity, the tree's needs-you rule).
#[test]
fn a_session_needs_you_when_blocked_or_idle_with_a_turn_the_user_has_not_seen() {
    let (_env, mut store) = fixture();
    let mut s = String::new();
    let mut fork = |id: &str, state: &str, events: &[String]| {
        let label = (state == "blocked").then_some("cpd: asking");
        s += &session_row(id, "lead", "alpha", "sketch", None, state, label, "2026-01-02T00:01:00.000Z");
        for e in events {
            s += e;
        }
    };
    let chat = |id: &str| moved("alpha", Some("sketch"), id, "chat");
    let parked = |id: &str| moved("alpha", Some("sketch"), id, "parked");
    fork("x-blocked", "blocked", &[]);
    fork("x-working", "working", &[settled("x-working")]);
    fork("x-quiet", "idle", &[]);
    fork("x-unseen", "idle", &[settled("x-unseen")]);
    fork("x-seen", "idle", &[settled("x-seen"), chat("x-seen"), parked("x-seen")]);
    fork("x-slot", "idle", &[chat("x-slot"), settled("x-slot")]);
    fork("x-again", "idle", &[settled("x-again"), chat("x-again"), settled("x-again"), parked("x-again")]);
    sql(&mut store, &s);

    let tree = read::tree(&mut store).unwrap();
    let sketch = &tree.contexts[0].plans[1];
    let got: Vec<(&str, bool)> =
        sketch.sessions.iter().filter(|n| n.id.as_str().starts_with("x-")).map(|n| (n.id.as_str(), n.needs_you)).collect();
    assert_eq!(
        got,
        [
            ("x-again", true),
            ("x-blocked", true),
            ("x-quiet", false),
            ("x-seen", false),
            ("x-slot", false),
            ("x-unseen", true),
            ("x-working", false),
        ],
        "by id: idle again after being seen; blocked; idle with no turn; idle and seen; in the slot; idle and unseen; working"
    );
    assert!(sketch.needs_you() && tree.contexts[0].needs_you());
}

#[test]
fn the_tree_counts_jobs_by_status_with_merging_apart_from_running() {
    let (_env, mut store) = fixture();
    let tree = read::tree(&mut store).unwrap();
    let ship = &tree.contexts[0].plans[0];
    let c = ship.head.counts;
    assert_eq!((c.draft, c.queued, c.running, c.merging, c.failed, c.merged, c.cancelled), (1, 1, 1, 1, 1, 1, 1));
    assert_eq!(c.total(), 7);
    assert_eq!(c.counters(ship.head.phase), [1, 1, 1, 1, 1], "queued, running, merging, failed, merged; cancelled is not shown");
    let sketch = &tree.contexts[0].plans[1];
    assert_eq!(sketch.head.counts.counters(sketch.head.phase), [2, 0, 0, 0, 0], "in phase plan the first counter is the drafts");
    assert_eq!((ship.head.open_escalations, ship.head.review_pending, sketch.head.review_pending), (1, true, false));
}

#[test]
fn a_command_that_changes_a_session_shows_in_the_next_snapshot() {
    let (env, mut store) = fixture();
    std::fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    let before = read::tree(&mut store).unwrap();
    env.cpd(&["session", "state", "blocked", "--label", "cpd: asking", "--session", "f-ship"]).ok();
    env.cpd(&["session", "rename", "renamed fork", "--session", "f-ship"]).ok();
    let after = read::tree(&mut store).unwrap();
    let fork = &after.contexts[0].plans[0].sessions[1];
    assert_eq!((fork.state, fork.state_label.as_deref(), fork.name.as_str()), (State::Blocked, Some("cpd: asking"), "renamed fork"));
    assert!(after.events_cursor > before.events_cursor);
}

// ---------- the plan view: plan show, the chat slot ----------

#[test]
fn a_plan_is_its_jobs_dependencies_attempts_and_tiers() {
    let (_env, mut store) = fixture();
    let flow = read::flow(&mut store, &plan("ship")).unwrap();

    assert_eq!((flow.head.id.as_str(), flow.head.context.as_str(), flow.head.phase), ("ship", "alpha", Phase::Dispatch));
    assert_eq!((flow.head.lead_branch.as_str(), flow.head.base.as_str(), flow.head.pr.is_none()), ("cpd/ship", "main", true));
    let shape: Vec<(&str, JobStatus, String, Vec<&str>, usize)> = flow
        .jobs
        .iter()
        .map(|j| (j.key.as_str(), j.status, j.tier.to_string(), j.needs.iter().map(|n| n.as_str()).collect(), j.attempts.len()))
        .collect();
    assert_eq!(
        shape,
        [
            ("a", JobStatus::Merged, "O3".into(), vec![], 1),
            ("b", JobStatus::Running, "S3".into(), vec!["a"], 1),
            ("c", JobStatus::Running, "S2".into(), vec!["a"], 2),
            ("d", JobStatus::Queued, "S3".into(), vec!["b", "c"], 0),
            ("e", JobStatus::Failed, "H1".into(), vec![], 1),
            ("f", JobStatus::Cancelled, "S3".into(), vec![], 0),
            ("g", JobStatus::Draft, "S3".into(), vec!["d"], 0),
        ]
    );
    let by = |k: &str| flow.jobs.iter().find(|j| j.key == key(k)).unwrap();
    assert_eq!(by("a").merged_sha.as_deref(), Some("abc123"));
    assert_eq!(by("e").failed_cause.as_deref(), Some("crashed"));
    assert_eq!(by("f").cancel_reason.as_deref(), Some("not needed"));
    assert!(by("d").current_attempt().is_none());

    let c = by("c");
    let (first, second) = (&c.attempts[0], &c.attempts[1]);
    assert_eq!((first.id.clone(), first.number, first.tier, first.origin), (attempt("c-a1"), 1, tier("S2"), Origin::Ladder));
    assert_eq!((first.status, first.outcome, first.cause), (AttemptStatus::Ended, Some(Outcome::Failed), Some(Cause::TimedOut)));
    assert!(first.ended_at.is_some() && first.worker.is_none());
    assert_eq!((second.number, second.tier, second.origin, second.status), (2, tier("S3"), Origin::Retry, AttemptStatus::Running));
    let worker = second.worker.as_ref().unwrap();
    assert_eq!((worker.session.as_str(), worker.state), ("w-c2", State::Working));
    assert_eq!(c.active_attempt().unwrap().id, attempt("c-a2"));
    assert_eq!(by("e").current_attempt().unwrap().id, attempt("e-a1"), "no active Attempt: the last one");
    assert_eq!(by("b").attempts[0].status, AttemptStatus::Merging);
    assert_eq!(by("b").attempts[0].worker.as_ref().unwrap().state, State::Idle);

    assert_eq!(flow.events_cursor, last_event(&mut store));
}

#[test]
fn a_plan_carries_its_open_escalations_and_its_latest_review() {
    let (_env, mut store) = fixture();
    let flow = read::flow(&mut store, &plan("ship")).unwrap();
    let asked: Vec<(EscalationId, AttemptId, &str)> =
        flow.escalations.iter().map(|e| (e.id.clone(), e.attempt.clone(), e.question.as_str())).collect();
    assert_eq!(asked, [(EscalationId::parse("c-a2-q1").unwrap(), attempt("c-a2"), "Question c-a2-q1?")], "answered and expired ones are not open");
    assert_eq!(flow.escalations[0].asked_at, "2026-01-03T00:20:00.000Z");

    let review = flow.review.unwrap();
    assert_eq!((review.id.as_str(), review.number, review.status, review.current), ("r2", 2, ReviewStatus::Pending, true));
    assert!(review.decided_at.is_none());

    // A draft Job edited since: the fingerprint is no longer the review's.
    sql(&mut store, "UPDATE jobs SET title = 'Changed' WHERE plan = 'ship' AND key = 'g'");
    let review = read::flow(&mut store, &plan("ship")).unwrap().review.unwrap();
    assert!(!review.current);

    let flow = read::flow(&mut store, &plan("sketch")).unwrap();
    assert!(flow.review.is_none() && flow.escalations.is_empty());
}

#[test]
fn a_plan_carries_its_pr_and_what_the_poll_saw() {
    let (_env, mut store) = fixture();
    let rel = read::flow(&mut store, &plan("rel")).unwrap().head;
    let pr = rel.pr.unwrap();
    assert_eq!((pr.number, pr.url.as_str(), pr.state), (7, "https://example.test/pull/7", PrState::Open));
    assert_eq!(pr.opened_at, "2026-01-05T00:00:00.000Z");
    assert_eq!(pr.polled_at.as_deref(), Some("2026-01-06T00:00:00.000Z"));
    assert_eq!((pr.checks.len(), pr.checks["ci"].as_str(), pr.reviews, pr.comments), (2, "FAILURE", 1, 2));
    assert_eq!(pr.checks_summary(), Checks::Failed);

    let mut state = |p: &str| read::flow(&mut store, &plan(p)).unwrap().head.pr.map(|pr| (pr.state, pr.checks_summary()));
    assert_eq!(state("shut"), Some((PrState::Closed, Checks::None)));
    assert_eq!(state("old"), Some((PrState::Merged, Checks::Passed)));
    assert_eq!(state("ship"), None);
}

#[test]
fn an_unknown_plan_is_plan_not_found() {
    let (_env, mut store) = fixture();
    let e = read::flow(&mut store, &plan("nope")).unwrap_err();
    assert_eq!(e.code(), ErrorCode::PlanNotFound);
}

#[test]
fn the_chat_slot_is_the_plan_lead_whose_latest_move_is_chat() {
    let (_env, mut store) = fixture();
    let slot = read::chat_slot(&mut store).unwrap().unwrap();
    assert_eq!((slot.session.as_str(), slot.plan.as_str(), slot.name.as_str()), ("l-ship", "ship", "name of l-ship"));
    let id = slot.moved_event;

    // The grounded Lead is never in the slot; parking leaves the placeholder.
    sql(&mut store, &moved("alpha", None, "g-alpha", "chat"));
    assert_eq!(read::chat_slot(&mut store).unwrap().unwrap().session, session("l-ship"));
    sql(&mut store, &moved("alpha", Some("ship"), "l-ship", "parked"));
    assert!(read::chat_slot(&mut store).unwrap().is_none());
    sql(&mut store, &moved("beta", Some("rel"), "l-rel", "chat"));
    let slot = read::chat_slot(&mut store).unwrap().unwrap();
    assert_eq!(slot.session, session("l-rel"));
    assert!(slot.moved_event > id);
}

// ---------- the Dispatch column: plan list, slots, the engine, Escalations, Events ----------

fn last_event(store: &mut Store) -> i64 {
    store.read(|c| Ok(c.query_row("SELECT max(id) FROM events", [], |r| r.get(0))?)).unwrap()
}

#[test]
fn the_slots_are_the_attempts_not_ended_against_the_limit() {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    assert_eq!(w.slots.limit, 3);
    assert_eq!(w.slots.used(), 3);
    let held: Vec<(&str, String, AttemptStatus, String)> =
        w.slots.held.iter().map(|h| (h.plan.as_str(), h.attempt.to_string(), h.status, h.tier.to_string())).collect();
    assert_eq!(
        held,
        [
            ("rel", "r1-a1".to_owned(), AttemptStatus::Running, "S3".to_owned()),
            ("ship", "b-a1".to_owned(), AttemptStatus::Merging, "S3".to_owned()),
            ("ship", "c-a2".to_owned(), AttemptStatus::Running, "S3".to_owned()),
        ],
        "oldest first; ended Attempts hold no slot"
    );
    assert_eq!(w.slots.held[2].started_at, "2026-01-03T00:02:00.000Z");
}

#[test]
fn the_widgets_list_every_plan_with_its_pr_and_feedback() {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let ids: Vec<&str> = w.plans.iter().map(|p| p.head.id.as_str()).collect();
    assert_eq!(ids, ["old", "ship", "sketch", "rel", "shut"], "oldest first");

    let rel = w.plans.iter().find(|p| p.head.id.as_str() == "rel").unwrap();
    assert_eq!(rel.head.pr.as_ref().unwrap().number, 7);
    let feedback = rel.feedback.as_ref().unwrap();
    assert_eq!((feedback.state.as_str(), feedback.failing_checks.as_slice(), feedback.reviews, feedback.comments), ("OPEN", &["ci".to_owned()][..], 1, 0));
    assert!(w.plans.iter().filter(|p| p.head.id.as_str() != "rel").all(|p| p.feedback.is_none()));
    let ship = w.plans.iter().find(|p| p.head.id.as_str() == "ship").unwrap();
    assert_eq!((ship.head.phase, ship.head.counts.merged, ship.head.pr.is_none()), (Phase::Dispatch, 1, true));
}

#[test]
fn the_widgets_list_every_open_escalation_of_the_machine() {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let open: Vec<(&str, &str, &str, String, String)> = w
        .escalations
        .iter()
        .map(|e| (e.context.as_str(), e.plan.as_str(), e.job.as_str(), e.escalation.id.to_string(), e.escalation.attempt.to_string()))
        .collect();
    assert_eq!(
        open,
        [
            ("beta", "rel", "r1", "r1-a1-q1".to_owned(), "r1-a1".to_owned()),
            ("alpha", "ship", "c", "c-a2-q1".to_owned(), "c-a2".to_owned()),
        ],
        "oldest asked first"
    );
    assert_eq!(w.escalations[0].escalation.question, "Question r1-a1-q1?");
}

#[test]
fn the_widgets_return_the_latest_events_oldest_first_and_the_cursor() {
    let (_env, mut store) = fixture();
    let last = last_event(&mut store);
    let w = read::widgets(&mut store, 3, 3).unwrap();
    let ids: Vec<i64> = w.events.iter().map(|e| e.id).collect();
    assert_eq!(ids, [last - 2, last - 1, last]);
    assert_eq!(w.events.last().unwrap().kind, "watch-started");
    assert_eq!(w.events[1].kind, "pr-feedback");
    assert_eq!(w.events_cursor, last);
    assert_eq!(read::widgets(&mut store, 3, 0).unwrap().events, []);
    assert!(read::widgets(&mut store, 3, 1000).unwrap().events.len() as i64 <= last);
}

fn spawn_and_reap() -> u32 {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

#[test]
fn the_engine_is_never_running_stopped_or_crashed() {
    let env = TestEnv::new();
    let mut store = open(&env);
    assert_eq!(read::widgets(&mut store, 3, 0).unwrap().engine, Engine::Never);

    sql(&mut store, &event("watch-started", None, None, None, &format!(r#"{{"pid":{}}}"#, std::process::id())));
    let Engine::Running { pid, since } = read::widgets(&mut store, 3, 0).unwrap().engine else { panic!() };
    assert_eq!((pid, since.as_str()), (std::process::id(), "2026-01-03T00:00:00.000Z"));

    sql(&mut store, &event("watch-stopped", None, None, None, &format!(r#"{{"pid":{}}}"#, std::process::id())));
    assert_eq!(read::widgets(&mut store, 3, 0).unwrap().engine, Engine::Stopped { at: "2026-01-03T00:00:00.000Z".into() });

    let dead = spawn_and_reap();
    sql(&mut store, &event("watch-started", None, None, None, &format!(r#"{{"pid":{dead}}}"#)));
    let Engine::Crashed { pid, .. } = read::widgets(&mut store, 3, 0).unwrap().engine else { panic!() };
    assert_eq!(pid, dead, "a watch-started whose process is gone and that no watch-stopped follows");
}

// ---------- the worker view: attempt show ----------

#[test]
fn a_worker_is_its_attempt_session_and_escalations() {
    let (_env, mut store) = fixture();
    let w = read::worker(&mut store, &plan("ship"), &attempt("c-a2")).unwrap();
    assert_eq!((w.job_title.as_str(), w.job_status), ("Job c", JobStatus::Running));
    let a = &w.attempt;
    assert_eq!((a.id.clone(), a.tier, a.origin, a.status, a.outcome, a.cause), (attempt("c-a2"), tier("S3"), Origin::Retry, AttemptStatus::Running, None, None));
    assert_eq!((a.branch.as_str(), a.worktree.as_str(), a.nudged, a.metrics.is_none()), ("cpd/ship--c-a2", "/w/c-a2", false, true));
    let session = w.session.as_ref().unwrap();
    assert_eq!((session.id.as_str(), session.state, session.file.as_deref()), ("w-c2", State::Working, Some("/sessions/w-c2.jsonl")));
    assert_eq!(w.worker_ref().unwrap().session, session.id);
    let asked: Vec<(String, EscalationStatus)> = w.escalations.iter().map(|e| (e.id.to_string(), e.status)).collect();
    assert_eq!(asked, [("c-a2-q1".to_owned(), EscalationStatus::Open)]);
    assert_eq!(w.events_cursor, last_event(&mut store));

    let ended = read::worker(&mut store, &plan("ship"), &attempt("a-a1")).unwrap();
    assert_eq!((ended.attempt.status, ended.attempt.outcome, ended.attempt.cause), (AttemptStatus::Ended, Some(Outcome::Merged), Some(Cause::Merged)));
    assert_eq!((ended.attempt.merged_sha.as_deref(), ended.session.is_none()), (Some("abc123"), true));
    assert_eq!((ended.escalations[0].status, ended.escalations[0].answer.as_deref()), (EscalationStatus::Answered, Some("Use v2.")));
    let merging = read::worker(&mut store, &plan("ship"), &attempt("b-a1")).unwrap();
    assert_eq!((merging.attempt.submitted_sha.as_deref(), merging.attempt.status), (Some("def456"), AttemptStatus::Merging));
    let expired = read::worker(&mut store, &plan("ship"), &attempt("e-a1")).unwrap();
    assert_eq!(expired.escalations[0].status, EscalationStatus::Expired);
    assert!(expired.escalations[0].expired_at.is_some());
}

#[test]
fn an_unknown_plan_or_attempt_is_not_found() {
    let (_env, mut store) = fixture();
    assert_eq!(read::worker(&mut store, &plan("nope"), &attempt("c-a2")).unwrap_err().code(), ErrorCode::PlanNotFound);
    assert_eq!(read::worker(&mut store, &plan("ship"), &attempt("c-a9")).unwrap_err().code(), ErrorCode::AttemptNotFound);
}

// ---------- read-only, cheap ----------

fn data_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA data_version", [], |r| r.get(0)).unwrap()
}

fn rows(conn: &Connection) -> Vec<i64> {
    ["txns", "history", "events", "plans", "jobs", "attempts", "sessions", "escalations"]
        .iter()
        .map(|t| conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).unwrap())
        .collect()
}

fn every_read(store: &mut Store) {
    read::tree(store).unwrap();
    read::flow(store, &plan("ship")).unwrap();
    read::chat_slot(store).unwrap();
    read::widgets(store, 3, 20).unwrap();
    read::worker(store, &plan("ship"), &attempt("c-a2")).unwrap();
}

#[test]
fn no_read_writes() {
    let (env, mut store) = fixture();
    // A second connection sees `data_version` change when any other connection commits.
    let watcher = Connection::open(env.cpd_home().join("cpd.db")).unwrap();
    let (version, counts) = (data_version(&watcher), rows(&watcher));
    for _ in 0..3 {
        every_read(&mut store);
    }
    assert_eq!(data_version(&watcher), version, "a read committed something");
    assert_eq!(rows(&watcher), counts);
}

#[test]
fn the_read_model_spawns_nothing_and_has_no_write_statement() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tui/src/read");
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        // The module docs and comments may name these; code may not.
        let code: String = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for forbidden in ["std::process", "process::Command", "Command::new", "crate::host", "crate::git", ".write(", "INSERT ", "UPDATE ", "DELETE ", "execute"] {
            assert!(!code.contains(forbidden), "{} contains {forbidden:?}", path.display());
        }
        seen += 1;
    }
    assert!(seen >= 5);
}

/// Contexts × Plans × Jobs, Attempts, Sessions and Events, enough for a long-lived machine.
fn populate_many(store: &mut Store) {
    let mut s = String::new();
    for c in 0..10 {
        s += &context(&format!("ctx{c}"), None);
        for p in 0..8 {
            let id = format!("p{c}x{p}");
            let lead = format!("lead-{id}");
            s += &plan_row(&id, &format!("ctx{c}"), "dispatch", &lead, &format!("2026-01-02T00:{c:02}:{p:02}.000Z"), None);
            s += &session_row(&lead, "lead", &format!("ctx{c}"), &id, None, "idle", None, "2026-01-02T00:00:00.000Z");
            for j in 0..12 {
                let status = ["merged", "merged", "running", "queued", "failed", "draft"][j % 6];
                let previous = format!("j{}", j.saturating_sub(1));
                let needs: &[&str] = if j > 0 { &[previous.as_str()] } else { &[] };
                s += &job_row(&id, &format!("j{j}"), status, "S3", needs);
                if status == "running" {
                    s += &session_row(&format!("w-{id}-{j}"), "worker", &format!("ctx{c}"), &id, Some(&format!("j{j}-a1")), "working", None, "2026-01-03T00:00:00.000Z");
                    s += &attempt_row(&id, &format!("j{j}"), 1, "S3", "running", None, Some(&format!("w-{id}-{j}")));
                }
            }
            for _ in 0..20 {
                s += &event("plan-phase", Some(&format!("ctx{c}")), Some(&id), None, r#"{"from":"plan","to":"dispatch"}"#);
            }
        }
    }
    sql(store, &s);
}

#[test]
fn every_read_takes_a_few_milliseconds_on_a_large_store() {
    let (_env, mut store) = fixture();
    populate_many(&mut store);
    let events: i64 = last_event(&mut store);
    assert!(events > 1600);
    // Warm up, then take the best of several runs: a bound on the work, not on a noisy neighbour.
    every_read(&mut store);
    let best = |name: &str, mut f: Box<dyn FnMut()>| {
        let fastest = (0..7)
            .map(|_| {
                let start = Instant::now();
                f();
                start.elapsed()
            })
            .min()
            .unwrap();
        assert!(fastest < Duration::from_millis(20), "{name} took {fastest:?}");
        eprintln!("{name}: {fastest:?}");
    };
    let store = std::cell::RefCell::new(store);
    best("tree", Box::new(|| drop(read::tree(&mut store.borrow_mut()).unwrap())));
    best("flow", Box::new(|| drop(read::flow(&mut store.borrow_mut(), &plan("p3x3")).unwrap())));
    best("chat_slot", Box::new(|| drop(read::chat_slot(&mut store.borrow_mut()).unwrap())));
    best("widgets", Box::new(|| drop(read::widgets(&mut store.borrow_mut(), 3, 20).unwrap())));
    best("worker", Box::new(|| drop(read::worker(&mut store.borrow_mut(), &plan("p3x3"), &attempt("j2-a1")).unwrap())));
}
