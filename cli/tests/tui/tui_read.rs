//! The TUI's read model (commands.md §17), against a store prepared through the
//! store API (`Store::write`, as the other areas' tests do, because Plans, Jobs
//! and Attempts belong to their areas), in this process (`Fx`: no process is started; the one test that runs `cpd`
//! does it through `run_with` on `Fx::with_ports`, tmux being the fake route).

use crate::common;

use std::fs::{self, File};
use std::sync::Mutex;

use common::fx::Fx;
use common::{TestEnv, sql};
use cpd_core::attempt::ledger::{Cause, Stored};
use cpd_core::config::Tier;
use cpd_core::error::ErrorCode;
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, ContextId, EscalationId, JobKey, PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::plan::{Phase, plan_fingerprint};
use cpd_core::session::State;
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};
use cpd_tui::read::{Work, 
    self, AttemptStatus, Checks, Dot, Engine, JobStatus, Origin, Outcome, Place, PlanNode, PrState,
    ReviewStatus,
};
use rusqlite::Connection;

// ---------- fixtures ----------

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
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

fn context(id: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');"
    )
}

/// `phase` decides the columns the schema ties to it; `pr` is `(number, pull_requests.state, pull_requests.checks)`.
fn plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str, pr: Option<(i64, &str, &str)>) -> String {
    let late = !matches!(phase, "grounding" | "plan");
    let worktree = if late { "'/wt'" } else { "NULL" };
    let locked = if late { "'2026-01-02T00:00:00.000Z'" } else { "NULL" };
    let done = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
    let status = match phase {
        "grounding" | "plan" => "waiting",
        "dispatch" => "dispatch",
        "deploy" => "open",
        _ => "merged",
    };
    let (row, number, url, opened, pull_request) = match pr {
        Some((n, state, checks)) => {
            let merged = if state == "merged" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
            (
                format!(
                    "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                     VALUES ('acme/repo', {n}, 'https://github.com/acme/repo/pull/{n}', 'PR {n}', 0, '{state}', 'none', '{checks}',
                             '2026-01-05T00:00:00.000Z', {merged}, '2026-01-06T00:00:00.000Z');"
                ),
                n.to_string(),
                format!("'https://github.com/acme/repo/pull/{n}'"),
                "'2026-01-05T00:00:00.000Z'",
                format!("(SELECT id FROM pull_requests WHERE number = {n})"),
            )
        }
        None => (String::new(), "NULL".into(), "NULL".into(), "NULL", "NULL".into()),
    };
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
         VALUES ('{id}', '{context}', 'Plan {id}', '{phase}', '{lead}', '{created}', {locked}, {done});
         {row}
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                          created_txn, created_at, started_at, merged_at)
         VALUES ('{id}', 'repo', '/repo', 'main', '{status}', 'cpd/{id}', {worktree}, {number}, {url}, {opened}, {pull_request},
                 cpd_txn(), '{created}', {locked}, {done});"
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
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('{plan}', 'repo', '{key}', 'Job {key}', '{tier}', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, {fingerprint},
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
/// - `alpha` (also a Lead with no Plan, `g-alpha`, as migration 0010 leaves a grounded Lead): `ship` (dispatch, Lead `l-ship`, side fork `f-ship`) with one Job in
///   every status; `sketch` (phase plan, drafts only).
/// - `beta`: `rel` (phase pr, PR 7 with a failing check), `shut` (pr, closed unmerged), `old` (done, merged PR).
/// - `gamma`: nothing.
///
/// The chat slot holds `l-ship`; `l-rel` was there before and is parked.
fn populate(store: &mut Store) {
    let mut s = String::new();
    s += &context("alpha");
    s += &context("beta");
    s += &context("gamma");
    s += &plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:00.000Z", None);
    s += &plan_row("sketch", "alpha", "plan", "l-sketch", "2026-01-02T00:00:01.000Z", None);
    s += &plan_row(
        "rel",
        "beta",
        "deploy",
        "l-rel",
        "2026-01-02T00:00:02.000Z",
        Some((7, "open", "failed")),
    );
    s += &plan_row(
        "shut",
        "beta",
        "deploy",
        "l-shut",
        "2026-01-02T00:00:03.000Z",
        Some((8, "closed", "none")),
    );
    s += &plan_row(
        "old",
        "beta",
        "done",
        "l-old",
        "2026-01-01T12:00:00.000Z",
        Some((3, "merged", "passed")),
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
        r#"{"pr":"repo","number":7,"url":"https://github.com/acme/repo/pull/7","state":"open","checks":"failed","review_decision":"none","reasons":["checks_failed"]}"#,
    );
    s += &event("watch-started", None, None, None, &format!(r#"{{"pid":{}}}"#, std::process::id()));
    sql(store, &s);

    let fingerprint = store.read(|c| Ok(plan_fingerprint(c, &plan("ship"))?)).unwrap();
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

/// Holds `locks/watch.lock` as an engine does, until dropped.
fn engine(env: &TestEnv) -> Flock<File> {
    let path = env.cpd_home().join("locks/watch.lock");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    Flock::lock(File::create(path).unwrap(), FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap()
}

/// The fixture store in an `Fx`: no process (`Fx::new` runs no reaper).
fn fixture() -> (Fx, Store) {
    let fx = Fx::new();
    let mut store = open(&fx.env);
    populate(&mut store);
    (fx, store)
}

// ---------- the tree: context list, session list ----------

#[test]
fn the_tree_is_contexts_then_plans_then_lead_sessions_with_their_states() {
    let (fx, mut store) = fixture();
    let _engine = engine(&fx.env);
    let tree = read::tree(&mut store).unwrap();

    let ids: Vec<&str> = tree.contexts.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["alpha", "beta", "gamma"]);
    let [alpha, beta, gamma] = &tree.contexts[..] else { panic!() };

    assert_eq!((alpha.title.as_str(), alpha.records, alpha.open_questions), ("Title of alpha", 4, 1));
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
        "the Lead first, then the side fork; the Workers and the Lead with no Plan are not in the tree's Plans"
    );
    assert!(ship.sessions[0].is_active() && !ship.sessions[1].is_active());
    let forks: Vec<(&str, &str)> = alpha.forks().iter().map(|(p, s)| (p.head.id.as_str(), s.id.as_str())).collect();
    assert_eq!(forks, [("ship", "f-ship")], "a side fork of a Plan's Lead is a fork of the Context, listed after its Plans (B1)");
    assert!(beta.forks().is_empty() && gamma.forks().is_empty(), "a Lead is no fork");
    assert_eq!(ship.lead().unwrap().id.as_str(), "l-ship");
    assert_eq!(ship.sessions[0].name, "name of l-ship");

    let rel = &beta.plans[1];
    assert_eq!(beta.plans.iter().map(|p| p.head.id.as_str()).collect::<Vec<_>>(), ["old", "rel", "shut"]);
    let lead = &rel.sessions[0];
    assert_eq!((lead.state, lead.state_label.as_deref(), lead.place), (State::Blocked, Some("cpd: PR feedback"), Place::Parked));
    assert!(tree.inbox.is_empty(), "no item, no filled dot: the Sessions' states and Escalations no longer say who needs the User");
    assert!(!rel.sessions[0].needs_you() && !beta.dot.needs_you() && !alpha.dot.needs_you() && !gamma.dot.needs_you());

    assert_eq!(tree.events_cursor, last_event(&mut store));
}

#[test]
fn archived_objects_are_in_the_tree_with_their_archived_at() {
    let (_fx, mut store) = fixture();
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

/// A condition of the Inbox (§2.2): `session` and `plan` are what it is linked to.
fn condition(kind: Kind, urgency: Urgency, session: Option<&str>, plan: Option<&str>, context: &str, subject: &str) -> Condition {
    Condition {
        kind,
        urgency,
        subject: subject.to_owned(),
        label: format!("{subject} label"),
        url: None,
        context: Some(ContextId::parse(context).unwrap()),
        plan: plan.map(self::plan),
        job: None,
        session: session.map(self::session),
        record: None,
        event: None,
        opened_at: None,
    }
}

/// The Inbox is exactly `conditions` (engine step E8), opened at `at`.
fn inbox(store: &mut Store, conditions: &[Condition], at: &str) {
    reconcile(store, conditions, at).unwrap();
}

const T0: &str = "2026-01-04T00:00:00.000Z";

/// The dots of the tree (§17): a Session's filled dot comes from its Inbox items only, and a working Session shows the spinner whatever its items
/// (own state first, hs-d183).
///
/// - `hot`: `red` (a dialog, red), `yel` (the Lead has a `pr-ready` item of its Plan; its fork `f-yel` is working and shows
///   no item of the Plan), `mag` (a magenta dialog), `whi` (`unread`), `blk` (blocked, no item), `wai` (waiting, no item), `wrk` (working, with items: still the spinner), `idl` (idle, with an
///   `unread` and a red `dialog`: the most urgent), `stp`;
/// - `calm`: a working and an idle Lead; `dead`: a stopped one; `ice`: no Plan, no Session.
#[test]
fn a_plan_carries_its_stuck_jobs_and_whether_its_lead_asks_the_user() {
    let (_fx, mut store) = fixture();
    let needs = |store: &mut Store, p: &str| read::plan_boxes(store, &plan(p)).unwrap().needs_user;
    assert_eq!(
        needs(&mut store, "ship"),
        read::NeedsUser { lead_working: true, lead_work: Some(Work::Planning), worked_on: vec![key("e")], ..Default::default() }, // a draft Job: Planning; an open Escalation
        "no item: an agent handles everything"
    );

    let stuck = |plan: &str, job: &str| Condition {
        job: Some(key(job)),
        ..condition(Kind::JobStuck, Urgency::Red, Some(&format!("l-{plan}")), Some(plan), "alpha", &format!("job:{plan}/{job}"))
    };
    inbox(
        &mut store,
        &[
            stuck("ship", "e"),
            stuck("sketch", "s1"),
            condition(Kind::Dialog, Urgency::Red, Some("f-ship"), Some("ship"), "alpha", "session:f-ship"),
            condition(Kind::Dialog, Urgency::Red, Some("l-sketch"), Some("sketch"), "alpha", "session:l-sketch"),
        ],
        T0,
    );
    assert_eq!(
        needs(&mut store, "ship"),
        read::NeedsUser { stuck_jobs: vec![key("e")], lead_dialog: false, lead_working: true, lead_work: Some(Work::Planning), worked_on: vec![key("e")] },
        "a fork's dialog is not the Lead's"
    );
    assert_eq!(
        needs(&mut store, "sketch"),
        read::NeedsUser { stuck_jobs: vec![key("s1")], lead_dialog: true, lead_working: false, lead_work: Some(Work::Planning), worked_on: vec![] }
    );
    assert_eq!(
        needs(&mut store, "rel"),
        read::NeedsUser { lead_work: Some(Work::Review), ..Default::default() },
        "another Plan's items are not its own"
    );

    // a hidden item needs nobody: the item is snoozed
    sql(&mut store, "UPDATE inbox_items SET snoozed_until = '2999-01-01T00:00:00.000Z' WHERE subject = 'session:l-sketch';");
    assert_eq!(
        needs(&mut store, "sketch"),
        read::NeedsUser { stuck_jobs: vec![key("s1")], lead_dialog: false, lead_working: false, lead_work: Some(Work::Planning), worked_on: vec![] }
    );
}

#[test]
fn a_session_shows_the_colour_of_its_most_urgent_item_and_with_no_item_never_a_filled_dot() {
    let (_fx, mut store) = fixture();
    let mut s = String::new();
    for c in ["hot", "calm", "dead", "ice"] {
        s += &context(c);
    }
    let at = |n: u32| format!("2026-01-02T00:00:{n:02}.000Z");
    let plans = [
        ("red", "hot", "dispatch", "blocked"),
        ("yel", "hot", "deploy", "idle"),
        ("mag", "hot", "plan", "blocked"),
        ("whi", "hot", "dispatch", "idle"),
        ("blk", "hot", "dispatch", "blocked"),
        ("wai", "hot", "dispatch", "waiting"),
        ("wrk", "hot", "dispatch", "working"),
        ("idl", "hot", "dispatch", "idle"),
        ("stp", "hot", "dispatch", "stopped"),
        ("cw", "calm", "dispatch", "working"),
        ("ci", "calm", "dispatch", "idle"),
        ("ds", "dead", "dispatch", "stopped"),
    ];
    for (n, (plan, context, phase, state)) in plans.into_iter().enumerate() {
        let pr = (plan == "yel").then_some((20, "open", "passed"));
        s += &plan_row(plan, context, phase, &format!("l-{plan}"), &at(n as u32), pr);
        let label = matches!(state, "blocked" | "waiting").then_some("cpd: asking");
        s += &session_row(&format!("l-{plan}"), "lead", context, plan, None, state, label, &at(n as u32));
    }
    s += &session_row("f-yel", "lead", "hot", "yel", None, "working", None, "2026-01-02T00:01:00.000Z");
    sql(&mut store, &s);

    let dots = |store: &mut Store| -> Vec<(String, Dot)> {
        let tree = read::tree(store).unwrap();
        tree.contexts.iter().flat_map(|c| &c.plans).flat_map(|p| &p.sessions).map(|n| (n.id.as_str().to_owned(), n.dot)).collect()
    };
    let dot = |all: &[(String, Dot)], id: &str| all.iter().find(|(i, _)| i == id).unwrap().1;

    // No item: no filled dot, whatever the state (a blocked Session is idle until the engine opens its dialog).
    let all = dots(&mut store);
    assert!(all.iter().all(|(_, d)| !d.needs_you()), "no item, never a filled dot: {all:?}");
    assert_eq!(
        (dot(&all, "l-red"), dot(&all, "l-wrk"), dot(&all, "l-idl"), dot(&all, "l-stp")),
        (Dot::Idle, Dot::Working(Some(Work::Dispatch)), Dot::Idle, Dot::Stopped)
    );
    // A Lead waiting for an agent (hs-d303) shows the waiting dot: not the idle glyph, and never red.
    assert_eq!(dot(&all, "l-wai"), Dot::Waiting);

    inbox(
        &mut store,
        &[
            condition(Kind::Dialog, Urgency::Red, Some("l-red"), Some("red"), "hot", "session:l-red"),
            condition(Kind::PrReady, Urgency::Yellow, None, Some("yel"), "hot", "pr:acme/repo#20"),
            condition(Kind::Dialog, Urgency::Magenta, Some("l-mag"), Some("mag"), "hot", "session:l-mag"),
            condition(Kind::Unread, Urgency::White, Some("l-whi"), Some("whi"), "hot", "session:l-whi"),
            // an `unread` and a red `dialog` of one idle Session: the most urgent wins
            condition(Kind::Unread, Urgency::White, Some("l-idl"), Some("idl"), "hot", "session:l-idl"),
            condition(Kind::Dialog, Urgency::Red, Some("l-idl"), Some("idl"), "hot", "session:l-idl"),
            // the same two items of a working Session: its own state first, the spinner
            condition(Kind::Unread, Urgency::White, Some("l-wrk"), Some("wrk"), "hot", "session:l-wrk"),
            condition(Kind::Dialog, Urgency::Red, Some("l-wrk"), Some("wrk"), "hot", "session:l-wrk"),
        ],
        T0,
    );
    let all = dots(&mut store);
    for (id, want) in [
        ("l-red", Dot::Red),
        ("l-yel", Dot::Yellow), // the Plan's own Lead shows the Plan's pull request item
        ("f-yel", Dot::Working(None)), // a fork shows no item of the Plan
        ("l-mag", Dot::Magenta),
        ("l-whi", Dot::White),
        ("l-wrk", Dot::Working(Some(Work::Dispatch))), // working shows the spinner, not its red dialog
        ("l-blk", Dot::Idle),
        ("l-wai", Dot::Waiting), // waiting with no item: its own calm dot, not idle and not red
        ("l-idl", Dot::Red),
        ("l-stp", Dot::Stopped),
        ("l-ship", Dot::Fixing), // working, and its Plan has a failed Job (`e`): the red loader (hs-d581)
        ("l-rel", Dot::Idle), // blocked in deploy, with a failing pull request, but no item
        ("l-old", Dot::Stopped),
    ] {
        assert_eq!(dot(&all, id), want, "{id}");
    }

    // An archived Session keeps its glyph and takes no item.
    sql(&mut store, "UPDATE sessions SET archived_at = '2026-02-03T00:00:00.000Z' WHERE id = 'l-red';");
    assert_eq!(dot(&dots(&mut store), "l-red"), Dot::Idle);
}

#[test]
fn a_working_lead_of_a_plan_with_a_failed_job_shows_the_red_loader() {
    let (_fx, mut store) = fixture();
    let at = |n: u32| format!("2026-01-02T00:01:{n:02}.000Z");
    let mut s = context("fix");
    // (plan, Lead state, Jobs: (key, status))
    let failed = [("f", "failed")];
    let rest = [("m", "merged"), ("r", "running"), ("q", "queued"), ("c", "cancelled"), ("d", "draft")];
    let plans: [(&str, &str, &[(&str, &str)]); 10] = [
        ("fx", "working", &failed),
        ("fxr", "working", &failed),
        ("fxw", "working", &failed),
        ("nf", "working", &rest),
        ("nfr", "working", &rest),
        ("idl", "idle", &failed),
        ("wai", "waiting", &failed),
        ("stp", "stopped", &failed),
        ("fk", "idle", &failed),
        ("arc", "working", &failed),
    ];
    for (n, (plan, state, jobs)) in plans.into_iter().enumerate() {
        s += &plan_row(plan, "fix", "dispatch", &format!("l-{plan}"), &at(n as u32), None);
        let label = (state == "waiting").then_some("cpd: asking");
        s += &session_row(&format!("l-{plan}"), "lead", "fix", plan, None, state, label, &at(n as u32));
        for (key, status) in jobs {
            s += &job_row(plan, key, status, "S3", &[]);
        }
    }
    // a working fork of a Plan with a failed Job: not the Plan's Lead
    s += &session_row("f-fk", "lead", "fix", "fk", None, "working", None, "2026-01-02T00:02:00.000Z");
    sql(&mut store, &s);

    let dot = |store: &mut Store, id: &str| -> Dot {
        let tree = read::tree(store).unwrap();
        let sessions = tree.contexts.iter().filter(|c| c.id.as_str() == "fix").flat_map(|c| &c.plans).flat_map(|p| &p.sessions);
        sessions.into_iter().find(|n| n.id.as_str() == id).unwrap().dot
    };

    // (i) a working Lead, a failed Job: the red loader
    assert_eq!(dot(&mut store, "l-fx"), Dot::Fixing);
    assert!(!Dot::Fixing.needs_you(), "the loader is no needs-you dot");
    // (iv) no failed Job, a draft Job: the magenta loader (Planning)
    assert_eq!(dot(&mut store, "l-nf"), Dot::Working(Some(Work::Planning)));
    // (v) idle, waiting, stopped: their usual dot
    assert_eq!((dot(&mut store, "l-idl"), dot(&mut store, "l-wai"), dot(&mut store, "l-stp")), (Dot::Idle, Dot::Waiting, Dot::Stopped));
    // (vi) a working fork is not the Plan's Lead
    assert_eq!(dot(&mut store, "f-fk"), Dot::Working(None));

    inbox(
        &mut store,
        &[
            condition(Kind::Dialog, Urgency::Red, Some("l-fxr"), Some("fxr"), "fix", "session:l-fxr"),
            condition(Kind::Unread, Urgency::White, Some("l-fxw"), Some("fxw"), "fix", "session:l-fxw"),
            condition(Kind::Dialog, Urgency::Red, Some("l-nfr"), Some("nfr"), "fix", "session:l-nfr"),
        ],
        T0,
    );
    // (ii) a red item of its own keeps the static red dot; (iii) a white one does not
    assert_eq!(dot(&mut store, "l-fxr"), Dot::Red);
    assert_eq!(dot(&mut store, "l-fxw"), Dot::Fixing);
    // (iv) no failed Job: own state first, whatever the items (hs-d183)
    assert_eq!(dot(&mut store, "l-nfr"), Dot::Working(Some(Work::Planning)));
    assert_eq!(dot(&mut store, "l-fx"), Dot::Fixing);

    // (viii) an archived Lead keeps its glyph
    sql(&mut store, "UPDATE sessions SET archived_at = '2026-02-03T00:00:00.000Z' WHERE id = 'l-arc';");
    assert_eq!(dot(&mut store, "l-arc"), Dot::Working(None));

    // (vii) the failed Job is queued again: the cyan loader
    sql(&mut store, "UPDATE jobs SET status = 'queued', failed_cause = NULL WHERE plan = 'fx';");
    assert_eq!(dot(&mut store, "l-fx"), Dot::Working(Some(Work::Dispatch)));
}

#[test]
fn a_plans_items_fill_its_lead_and_not_its_forks() {
    let (_fx, mut store) = fixture();
    // The Lead is idle: a working Session shows the spinner whatever its items (hs-d183).
    sql(&mut store, "UPDATE sessions SET state = 'idle' WHERE id = 'l-ship';");
    inbox(
        &mut store,
        &[
            condition(Kind::PlanReview, Urgency::Magenta, None, Some("ship"), "alpha", "review:ship/r2"),
            condition(Kind::JobStuck, Urgency::Red, None, Some("ship"), "alpha", "job:ship/e"),
            condition(Kind::Dialog, Urgency::Red, Some("f-ship"), Some("ship"), "alpha", "session:f-ship"),
            condition(Kind::PrFailing, Urgency::Red, None, Some("rel"), "beta", "pr:acme/repo#7"),
        ],
        T0,
    );
    let tree = read::tree(&mut store).unwrap();
    fn dots(plan: &PlanNode) -> Vec<(&str, Dot)> {
        plan.sessions.iter().map(|s| (s.id.as_str(), s.dot)).collect()
    }
    assert_eq!(dots(&tree.contexts[0].plans[0]), [("l-ship", Dot::Red), ("f-ship", Dot::Red)], "the Lead: job-stuck; the fork: its own dialog");
    assert_eq!(dots(&tree.contexts[0].plans[1]), [("l-sketch", Dot::Idle)], "another Plan of the Context: nothing");
    assert_eq!(dots(&tree.contexts[1].plans[1]), [("l-rel", Dot::Idle)], "pr-failing of its Plan: its Lead fixes it, no dot (hs-d302)");
    assert_eq!(dots(&tree.contexts[1].plans[0]), [("l-old", Dot::Stopped)]);

    inbox(&mut store, &[condition(Kind::PlanReview, Urgency::Magenta, None, Some("ship"), "alpha", "review:ship/r2")], T0);
    let tree = read::tree(&mut store).unwrap();
    assert_eq!(dots(&tree.contexts[0].plans[0]), [("l-ship", Dot::Magenta), ("f-ship", Dot::Idle)], "closed items fill no dot");
}

/// A Context row is its live Manager's dot (hs-d215, hs-d217): a filled dot only from the Manager's own items, else the
/// glyph of its state; `Stopped` with no live Manager.
#[test]
fn a_context_row_fills_only_from_its_managers_items() {
    let (_fx, mut store) = fixture();
    // The Lead is idle, so that its own row shows the items it is given below.
    sql(&mut store, "UPDATE sessions SET state = 'idle' WHERE id = 'l-ship';");
    sql(&mut store, &session_row("m-alpha", "manager", "alpha", "", None, "idle", None, "2026-01-02T00:00:05.000Z"));
    let rows = |store: &mut Store| -> Vec<(String, Dot)> {
        read::tree(store).unwrap().contexts.iter().map(|c| (c.id.as_str().to_owned(), c.dot)).collect()
    };
    assert_eq!(rows(&mut store), [("alpha".into(), Dot::Idle), ("beta".into(), Dot::Stopped), ("gamma".into(), Dot::Stopped)], "no item: the Manager's glyph, `Stopped` with no Manager");

    // Red items of other Sessions, of its Plans and of the Context itself do not fill the row.
    let others = [
        condition(Kind::Dialog, Urgency::Red, Some("l-ship"), Some("ship"), "alpha", "session:l-ship"),
        condition(Kind::Dialog, Urgency::Red, Some("f-ship"), Some("ship"), "alpha", "session:f-ship"),
        condition(Kind::JobStuck, Urgency::Red, None, Some("ship"), "alpha", "job:ship/e"),
        condition(Kind::Question, Urgency::Magenta, None, None, "alpha", "record:alpha/m1-q2"),
        condition(Kind::EngineError, Urgency::Red, None, None, "alpha", "error:E8//x"),
        condition(Kind::Dialog, Urgency::Red, Some("l-rel"), Some("rel"), "beta", "session:l-rel"),
    ];
    inbox(&mut store, &others, T0);
    assert_eq!(rows(&mut store), [("alpha".into(), Dot::Idle), ("beta".into(), Dot::Stopped), ("gamma".into(), Dot::Stopped)]);
    let tree = read::tree(&mut store).unwrap();
    assert_eq!(tree.contexts[0].plans[0].sessions[0].dot, Dot::Red, "while the Session's own row is red");

    // The Manager's own items fill it, in their colour.
    let mut with_manager = others.to_vec();
    with_manager.push(condition(Kind::Unread, Urgency::White, Some("m-alpha"), None, "alpha", "session:m-alpha"));
    inbox(&mut store, &with_manager, T0);
    assert_eq!(rows(&mut store)[0], ("alpha".into(), Dot::White));
    with_manager.push(condition(Kind::Dialog, Urgency::Magenta, Some("m-alpha"), None, "alpha", "session:m-alpha"));
    inbox(&mut store, &with_manager, T0);
    assert_eq!(rows(&mut store)[0], ("alpha".into(), Dot::Magenta), "its most urgent");

    // An archived Context takes no item: `context archive` archives its Manager too (commands.md §3.8), so it has no live one.
    sql(
        &mut store,
        "UPDATE contexts SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'alpha';
         UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'm-alpha';",
    );
    assert_eq!(rows(&mut store)[0], ("alpha".into(), Dot::Stopped));
}

/// An archived Context carries its archived Manager in `archived_managers`; a live Context's list is empty while its
/// dot still comes from its live Manager (commands.md §17; hs-d630, hs-d633).
#[test]
fn the_tree_reads_an_archived_manager_of_an_archived_context() {
    let (_fx, mut store) = fixture();
    // `alpha` is live and gets a live Manager; `zeta` is an archived Context with its archived Manager.
    sql(&mut store, &session_row("m-alpha", "manager", "alpha", "", None, "idle", None, "2026-01-02T00:00:05.000Z"));
    sql(&mut store, &context("zeta"));
    sql(
        &mut store,
        &format!(
            "{}\n             UPDATE contexts SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'zeta';\n             UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'm-zeta';",
            session_row("m-zeta", "manager", "zeta", "", None, "idle", None, "2026-01-02T00:00:06.000Z")
        ),
    );

    let tree = read::tree(&mut store).unwrap();
    let by = |id: &str| tree.contexts.iter().find(|c| c.id.as_str() == id).unwrap();
    let alpha = by("alpha");
    assert_eq!(alpha.dot, Dot::Idle, "a live Context's dot is still its live Manager's");
    assert!(alpha.archived_managers.is_empty(), "a live Context has no archived Manager");
    let zeta = by("zeta");
    assert!(zeta.is_archived());
    assert_eq!(
        zeta.archived_managers.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["m-zeta"],
        "the archived Context's archived Manager"
    );
}

#[test]
fn a_snoozed_or_dismissed_item_fills_no_dot_and_is_not_in_the_snapshot() {
    let (fx, mut store) = fixture();
    let _engine = engine(&fx.env);
    // The Lead is idle: a working Session shows the spinner whatever its items (hs-d183).
    sql(&mut store, "UPDATE sessions SET state = 'idle' WHERE id = 'l-ship';");
    sql(&mut store, &session_row("m-alpha", "manager", "alpha", "", None, "idle", None, "2026-01-02T00:00:05.000Z"));
    inbox(
        &mut store,
        &[
            condition(Kind::Dialog, Urgency::Red, Some("l-ship"), Some("ship"), "alpha", "session:l-ship"),
            condition(Kind::Dialog, Urgency::Red, Some("f-ship"), Some("ship"), "alpha", "session:f-ship"),
            condition(Kind::Unread, Urgency::White, Some("m-alpha"), None, "alpha", "session:m-alpha"),
        ],
        T0,
    );
    let tree = read::tree(&mut store).unwrap();
    assert_eq!(tree.inbox.len(), 3);
    assert_eq!((tree.contexts[0].plans[0].sessions[0].dot, tree.contexts[0].dot), (Dot::Red, Dot::White));

    // Snoozed into the far future, dismissed: hidden at once.
    sql(
        &mut store,
        "UPDATE inbox_items SET snoozed_until = '2999-01-01T00:00:00.000Z' WHERE subject = 'session:l-ship';
         UPDATE inbox_items SET dismissed_at = '2026-01-04T00:01:00.000Z' WHERE subject = 'session:m-alpha';",
    );
    let tree = read::tree(&mut store).unwrap();
    assert_eq!(tree.inbox.iter().map(|i| i.subject.as_str()).collect::<Vec<_>>(), ["session:f-ship"]);
    let ship = &tree.contexts[0].plans[0];
    assert_eq!((ship.sessions[0].dot, ship.sessions[1].dot, tree.contexts[0].dot), (Dot::Idle, Dot::Red, Dot::Idle));

    // A snooze that has ended by the clock shows the item again, with no Event.
    sql(&mut store, "UPDATE inbox_items SET snoozed_until = '2026-01-04T00:00:00.000Z' WHERE subject = 'session:l-ship';");
    let tree = read::tree(&mut store).unwrap();
    assert_eq!(tree.contexts[0].plans[0].sessions[0].dot, Dot::Red);
    assert_eq!(tree.inbox.len(), 2);
}

#[test]
fn the_snapshot_carries_the_visible_open_items_in_the_inbox_order() {
    let (fx, mut store) = fixture();
    let _engine = engine(&fx.env);
    assert!(read::tree(&mut store).unwrap().inbox.is_empty());
    let mut white = condition(Kind::Unread, Urgency::White, Some("l-sketch"), Some("sketch"), "alpha", "session:l-sketch");
    white.url = Some("https://example.com/w".into());
    inbox(&mut store, &[white], "2026-01-04T00:00:00.000Z");
    inbox(
        &mut store,
        &[
            condition(Kind::Unread, Urgency::White, Some("l-sketch"), Some("sketch"), "alpha", "session:l-sketch"),
            condition(Kind::Question, Urgency::Magenta, None, None, "alpha", "record:alpha/m1-q2"),
            condition(Kind::JobStuck, Urgency::Red, None, Some("ship"), "alpha", "job:ship/e"),
            condition(Kind::PrReady, Urgency::Yellow, None, Some("rel"), "beta", "pr:acme/repo#7"),
        ],
        "2026-01-04T00:00:05.000Z",
    );
    let tree = read::tree(&mut store).unwrap();
    let got: Vec<(Kind, Urgency, &str)> = tree.inbox.iter().map(|i| (i.kind, i.urgency, i.opened_at.as_str())).collect();
    assert_eq!(
        got,
        [
            (Kind::JobStuck, Urgency::Red, "2026-01-04T00:00:05.000Z"),
            (Kind::PrReady, Urgency::Yellow, "2026-01-04T00:00:05.000Z"),
            (Kind::Question, Urgency::Magenta, "2026-01-04T00:00:05.000Z"),
            (Kind::Unread, Urgency::White, "2026-01-04T00:00:00.000Z"),
        ],
        "red, yellow, magenta, white"
    );
    let unread = tree.inbox.last().unwrap();
    assert_eq!((unread.label.as_str(), unread.url.as_deref(), unread.session.as_ref().map(SessionId::as_str)), ("session:l-sketch label", Some("https://example.com/w"), Some("l-sketch")));
    assert_eq!(tree.events_cursor, last_event(&mut store), "the same transaction as the tree");
}

#[test]
fn the_tree_counts_jobs_by_status_with_merging_apart_from_running() {
    let (_fx, mut store) = fixture();
    let tree = read::tree(&mut store).unwrap();
    let ship = &tree.contexts[0].plans[0];
    let c = ship.head.counts;
    assert_eq!((c.draft, c.queued, c.running, c.merging, c.failed, c.merged, c.cancelled), (1, 1, 1, 1, 1, 1, 1));
    assert_eq!(c.total(), 7);
    assert_eq!(c.counters(), [2, 2, 1, 1], "planned (drafts and queued), active (running and merging), failed, done; cancelled is not shown");
    let sketch = &tree.contexts[0].plans[1];
    assert_eq!(sketch.head.counts.counters(), [2, 0, 0, 0], "the planned role is the drafts plus queued");
    assert_eq!((ship.head.open_escalations, ship.head.review_pending, sketch.head.review_pending), (1, true, false));
}

/// An `attempt-status` Event of a check span: `check` is "start" or "end".
fn span(job: &str, attempt: &str, check: &str) -> String {
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
         VALUES (cpd_txn(), '2026-01-03T00:10:00.000Z', 'attempt-status', 'alpha', 'ship', '{job}', '{attempt}', 'fixture span',
                 '{{\"from\":\"running\",\"to\":\"running\",\"check\":\"{check}\",\"what\":\"checks\"}}');"
    )
}

fn ship_counts(store: &mut Store) -> (usize, usize) {
    let c = read::tree(store).unwrap().contexts[0].plans[0].head.counts;
    (c.running, c.merging)
}

#[test]
fn a_running_attempt_in_an_open_check_span_is_counted_with_the_merging_jobs() {
    let (_fx, mut store) = fixture();
    assert_eq!(ship_counts(&mut store), (1, 1));
    sql(&mut store, &span("c", "c-a2", "start"));
    assert_eq!(ship_counts(&mut store), (0, 2), "job c is landing, apart from running");
}

#[test]
fn the_latest_check_event_of_an_attempt_decides_whether_its_span_is_open() {
    let (_fx, mut store) = fixture();
    sql(&mut store, &span("c", "c-a2", "start"));
    sql(&mut store, &span("c", "c-a2", "end"));
    assert_eq!(ship_counts(&mut store), (1, 1), "start, end: running again");
    sql(&mut store, &span("c", "c-a2", "start"));
    assert_eq!(ship_counts(&mut store), (0, 2), "start, end, start: checking");
}

#[test]
fn an_open_span_of_an_ended_or_another_attempt_changes_no_count() {
    let (_fx, mut store) = fixture();
    sql(&mut store, &span("c", "c-a1", "start")); // ended
    sql(&mut store, &span("r1", "r1-a1", "start")); // another Plan's Job: not ship's count
    sql(&mut store, &span("zz", "zz-a9", "start")); // no such Attempt
    assert_eq!(ship_counts(&mut store), (1, 1));
    sql(&mut store, &span("c", "c-a2", "start"));
    sql(&mut store, &span("c", "c-a1", "end")); // another Attempt's end does not close c-a2's span
    assert_eq!(ship_counts(&mut store), (0, 2));
}

#[test]
fn every_read_shows_an_attempt_in_an_open_check_span_as_checking() {
    let (_fx, mut store) = fixture();
    sql(&mut store, &span("c", "c-a2", "start"));
    sql(&mut store, &span("c", "c-a1", "start")); // the ended Attempt stays ended

    let flow = read::flow(&mut store, &plan("ship")).unwrap();
    let status = |job: &str| flow.jobs.iter().find(|j| j.key == key(job)).unwrap().active_attempt().map(|a| a.status);
    assert_eq!((status("c"), status("b")), (Some(AttemptStatus::Checking), Some(AttemptStatus::Merging)));
    let c1 = flow.jobs.iter().find(|j| j.key == key("c")).unwrap().attempts.iter().find(|a| a.number == 1).unwrap();
    assert_eq!(c1.status, AttemptStatus::Ended);

    let (slots, _) = read::slots(&mut store, 3).unwrap();
    let held = |id: &str| slots.held.iter().find(|h| h.attempt == attempt(id)).unwrap().status;
    assert_eq!((held("c-a2"), held("b-a1")), (AttemptStatus::Checking, AttemptStatus::Merging));

    sql(&mut store, &span("c", "c-a2", "end"));
}

#[test]
fn a_command_that_changes_a_session_shows_in_the_next_snapshot() {
    let fx = Fx::with_ports();
    let mut store = open(&fx.env);
    populate(&mut store);
    fx.env.write_tmux_config("");
    let before = read::tree(&mut store).unwrap();
    fx.cpd(&["session", "state", "blocked", "--label", "cpd: asking", "--session", "f-ship"]).ok();
    fx.cpd(&["session", "rename", "renamed fork", "--session", "f-ship"]).ok();
    let after = read::tree(&mut store).unwrap();
    let fork = &after.contexts[0].plans[0].sessions[1];
    assert_eq!((fork.state, fork.state_label.as_deref(), fork.name.as_str()), (State::Blocked, Some("cpd: asking"), "renamed fork"));
    assert!(after.events_cursor > before.events_cursor);
}

// ---------- the plan view: plan show, the chat slot ----------

#[test]
fn a_plan_is_its_jobs_dependencies_attempts_and_tiers() {
    let (_fx, mut store) = fixture();
    let flow = read::flow(&mut store, &plan("ship")).unwrap();

    assert_eq!((flow.head.id.as_str(), flow.head.context.as_str(), flow.head.phase), ("ship", "alpha", Phase::Dispatch));
    assert!(flow.head.pr.is_none());
    let shape: Vec<(&str, JobStatus, String, Vec<&str>, usize)> = flow
        .jobs
        .iter()
        .map(|j| (j.key.as_str(), j.status, j.tier.expect("a Tier").to_string(), j.needs.iter().map(|n| n.as_str()).collect(), j.attempts.len()))
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
    assert_eq!((first.status, first.outcome, first.cause), (AttemptStatus::Ended, Some(Outcome::Failed), Some(Stored::Failed(Cause::TimedOut))));
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
fn a_stub_draft_without_tier_or_spec_is_a_job_without_a_tier_and_does_not_fail_the_plan() {
    let (_fx, mut store) = fixture();
    sql(
        &mut store,
        "INSERT INTO jobs (plan, pr, key, title, status, appended, created_txn, created_at)
         VALUES ('ship', 'repo', 'stub', 'Planned', 'draft', 1, cpd_txn(), '2026-01-05T00:00:00.000Z');",
    );
    let flow = read::flow(&mut store, &plan("ship")).unwrap();
    let stub = flow.jobs.iter().find(|j| j.key == key("stub")).unwrap();
    assert_eq!((stub.status, stub.tier, stub.title.as_str()), (JobStatus::Draft, None, "Planned"));
    assert_eq!(flow.jobs.iter().find(|j| j.key == key("g")).unwrap().tier, Some(tier("S3")));
}

#[test]
fn a_plan_carries_its_open_escalations_and_its_latest_review() {
    let (_fx, mut store) = fixture();
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
fn a_superseded_latest_review_reads_and_is_not_pending() {
    let (_fx, mut store) = fixture();
    sql(&mut store, "UPDATE plan_reviews SET status = 'superseded', decided_at = '2026-01-02T02:00:00.000Z' WHERE plan = 'ship' AND id = 'r2'");
    let review = read::flow(&mut store, &plan("ship")).unwrap().review.unwrap();
    assert_eq!((review.id.as_str(), review.number, review.status.as_str()), ("r2", 2, "superseded"));
    assert_eq!(review.decided_at.as_deref(), Some("2026-01-02T02:00:00.000Z"));
}

#[test]
fn a_plan_carries_its_pr_and_what_the_poll_saw() {
    let (_fx, mut store) = fixture();
    let rel = read::flow(&mut store, &plan("rel")).unwrap().head;
    let pr = rel.pr.unwrap();
    assert_eq!((pr.number, pr.url.as_str(), pr.state), (7, "https://github.com/acme/repo/pull/7", PrState::Open));
    assert_eq!(pr.opened_at, "2026-01-05T00:00:00.000Z");
    assert_eq!(pr.checks, Checks::Failed);

    let mut state = |p: &str| read::flow(&mut store, &plan(p)).unwrap().head.pr.map(|pr| (pr.state, pr.checks));
    assert_eq!(state("shut"), Some((PrState::Closed, Checks::None)));
    assert_eq!(state("old"), Some((PrState::Merged, Checks::Passed)));
    assert_eq!(state("ship"), None);
}

#[test]
fn a_pull_request_is_conflicting_only_when_the_poll_saw_a_conflict() {
    let (_fx, mut store) = fixture();
    let pull = |store: &mut Store, p: &str| read::plan_boxes(store, &plan(p)).unwrap().prs[0].pull.clone().unwrap();
    assert!(!pull(&mut store, "rel").conflicting, "a fresh row reads `unknown`");
    for (mergeable, want) in [("conflicting", true), ("mergeable", false), ("unknown", false)] {
        sql(&mut store, &format!("UPDATE pull_requests SET mergeable = '{mergeable}' WHERE number = 7"));
        assert_eq!(pull(&mut store, "rel").conflicting, want, "{mergeable}");
    }
}

#[test]
fn an_open_pull_request_is_clean_only_when_its_merge_state_is_clean() {
    let (_fx, mut store) = fixture();
    let clean = |store: &mut Store| read::plan_boxes(store, &plan("rel")).unwrap().prs[0].pull.as_ref().unwrap().clean;
    assert!(!clean(&mut store), "a fresh row reads `unknown`");
    sql(&mut store, "UPDATE pull_requests SET base_ref = 'main', default_branch = 'main' WHERE number = 7");
    for (merge_state, want) in [("clean", true), ("blocked", false), ("unstable", false), ("unknown", false)] {
        sql(&mut store, &format!("UPDATE pull_requests SET merge_state = '{merge_state}' WHERE number = 7"));
        assert_eq!(clean(&mut store), want, "{merge_state}");
    }
}

/// A Plan as `plan new` leaves it: in grounding, with its Lead and no `prs` row (hs-n54).
fn add_plan_without_pr(store: &mut Store) {
    let lead = session_row("l-fresh", "lead", "gamma", "fresh", None, "idle", None, "2026-01-02T00:00:09.000Z");
    sql(
        store,
        &format!(
            "INSERT INTO plans (id, context, title, phase, lead_session, created_at)
             VALUES ('fresh', 'gamma', 'Plan fresh', 'grounding', 'l-fresh', '2026-01-02T00:00:09.000Z');
             {lead}"
        ),
    );
}

#[test]
fn a_plan_with_no_pr_is_in_the_tree_the_widgets_and_the_flow() {
    let (_fx, mut store) = fixture();
    add_plan_without_pr(&mut store);

    let tree = read::tree(&mut store).unwrap();
    let gamma = tree.contexts.iter().find(|c| c.id.as_str() == "gamma").unwrap();
    let [fresh] = &gamma.plans[..] else { panic!("gamma has the one Plan") };
    assert_eq!((fresh.head.id.as_str(), fresh.head.phase, fresh.head.pr.is_none()), ("fresh", Phase::Grounding, true));
    assert_eq!(fresh.sessions.iter().map(|s| (s.id.as_str(), s.plan_lead)).collect::<Vec<_>>(), [("l-fresh", true)]);

    let w = read::widgets(&mut store, 3, 0).unwrap();
    assert!(w.plans.iter().any(|p| p.head.id.as_str() == "fresh" && p.head.pr.is_none()));

    let head = read::flow(&mut store, &plan("fresh")).unwrap().head;
    assert_eq!((head.id.as_str(), head.phase, head.pr.is_none()), ("fresh", Phase::Grounding, true));
}

#[test]
fn an_unknown_plan_is_plan_not_found() {
    let (_fx, mut store) = fixture();
    let e = read::flow(&mut store, &plan("nope")).unwrap_err();
    assert_eq!(e.code(), ErrorCode::PlanNotFound);
}

#[test]
fn the_chat_slot_is_the_plan_lead_whose_latest_move_is_chat() {
    let (_fx, mut store) = fixture();
    let slot = read::chat_slot(&mut store).unwrap().unwrap();
    assert_eq!((slot.session.as_str(), slot.plan.as_str(), slot.name.as_str()), ("l-ship", "ship", "name of l-ship"));
    let id = slot.moved_event;

    // A Lead with no Plan is never in the slot; parking leaves the placeholder.
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
    let (_fx, mut store) = fixture();
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
fn the_widgets_list_every_plan_with_its_pr() {
    let (_fx, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let ids: Vec<&str> = w.plans.iter().map(|p| p.head.id.as_str()).collect();
    assert_eq!(ids, ["old", "ship", "sketch", "rel", "shut"], "oldest first");

    let rel = w.plans.iter().find(|p| p.head.id.as_str() == "rel").unwrap();
    assert_eq!(rel.head.pr.as_ref().unwrap().number, 7);
    let ship = w.plans.iter().find(|p| p.head.id.as_str() == "ship").unwrap();
    assert_eq!((ship.head.phase, ship.head.counts.merged, ship.head.pr.is_none()), (Phase::Dispatch, 1, true));
}

#[test]
fn a_pull_request_is_worked_only_while_its_owners_lead_is_working() {
    let (_fx, mut store) = fixture();
    // `acme/work#70` is no Plan's: nobody works it.
    sql(
        &mut store,
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
         VALUES ('acme/work', 70, 'https://github.com/acme/work/pull/70', 'Mine', 0, 'open', 'none', 'none',
                 '2026-01-07T00:00:00.000Z', NULL, '2026-01-07T00:00:00.000Z');",
    );
    let worked = |store: &mut Store| read::plan_boxes(store, &plan("rel")).unwrap().prs[0].lead_dot.is_some();
    for state in ["blocked", "idle", "waiting", "stopped"] {
        sql(&mut store, &format!(
                "UPDATE sessions SET state = '{state}', state_label = CASE WHEN '{state}' IN ('waiting', 'blocked') THEN 'why' END WHERE id = 'l-rel'"
            ));
        assert!(!worked(&mut store), "{state}");
    }
    sql(&mut store, "UPDATE sessions SET state = 'working', state_label = NULL WHERE id = 'l-rel'");
    assert!(!worked(&mut store), "working, naming no Workflow: no dot");
    let pr_key: String = read::plan_boxes(&mut store, &plan("rel")).unwrap().prs[0].key.to_string();
    sql(&mut store, &format!("UPDATE sessions SET working_pr = '{pr_key}' WHERE id = 'l-rel'"));
    assert!(worked(&mut store), "the Lead of acme/repo#7 works on it");
}

#[test]
fn the_widgets_list_every_open_escalation_of_the_machine() {
    let (_fx, mut store) = fixture();
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
    let (_fx, mut store) = fixture();
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

/// tui-F1: the StatusBar reads the slots, not the whole model.
#[test]
fn the_slots_read_alone_are_the_widgets_own() {
    let (_fx, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    assert_eq!(read::slots(&mut store, 3).unwrap(), (w.slots, w.events_cursor));
}

/// tui-F3: a pull request row changes with no Event (`pr poll` writes the checks, the review, `mergeable`), so the follow
/// tells a view that the rows moved; the first poll says so, since a change since the snapshot cannot be told from none.
#[test]
fn the_follow_tells_when_a_pull_request_row_changed_though_no_event_came() {
    let (_fx, mut store) = fixture();
    let cursor = read::slots(&mut store, 3).unwrap().1;
    let mut follow = read::Follow::new(&mut store, &[], cursor).unwrap();
    let changes = |store: &mut Store, follow: &mut read::Follow| {
        let changes = follow.poll_changes(store).unwrap();
        (changes.events.len(), changes.pulls, changes.any())
    };
    assert_eq!(changes(&mut store, &mut follow), (0, true, true), "the first poll");
    assert_eq!(changes(&mut store, &mut follow), (0, false, false), "nothing came");

    sql(&mut store, "UPDATE pull_requests SET checks = 'failed', changed_at = '2026-03-01T00:00:00.000Z' WHERE number = 7");
    assert_eq!(changes(&mut store, &mut follow), (0, true, true), "a row changed, with no Event");
    assert_eq!(changes(&mut store, &mut follow), (0, false, false));

    sql(&mut store, &event("watch-started", None, None, None, r#"{"pid":1}"#));
    assert_eq!(changes(&mut store, &mut follow), (1, false, true), "an Event, no row");

    sql(
        &mut store,
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
         VALUES ('acme/repo', 99, 'https://github.com/acme/repo/pull/99', 'New', 0, 'open', 'none', 'none',
                 '2026-03-02T00:00:00.000Z', '2026-03-02T00:00:00.000Z')",
    );
    assert_eq!(changes(&mut store, &mut follow), (0, true, true), "a row came");
}

/// A pid no process has: above every platform's pid limit (macOS 99999, Linux at most 4194304).
const DEAD_PID: u32 = 999_999_999;

#[test]
fn the_engine_is_never_running_stopped_or_crashed() {
    let fx = Fx::new();
    let mut store = open(&fx.env);
    assert_eq!(read::widgets(&mut store, 3, 0).unwrap().engine, Engine::Never);

    sql(&mut store, &event("watch-started", None, None, None, &format!(r#"{{"pid":{}}}"#, std::process::id())));
    let Engine::Running { pid, since } = read::widgets(&mut store, 3, 0).unwrap().engine else { panic!() };
    assert_eq!((pid, since.as_str()), (std::process::id(), "2026-01-03T00:00:00.000Z"));

    sql(&mut store, &event("watch-stopped", None, None, None, &format!(r#"{{"pid":{}}}"#, std::process::id())));
    assert_eq!(read::widgets(&mut store, 3, 0).unwrap().engine, Engine::Stopped { at: "2026-01-03T00:00:00.000Z".into() });

    let dead = DEAD_PID;
    sql(&mut store, &event("watch-started", None, None, None, &format!(r#"{{"pid":{dead}}}"#)));
    let Engine::Crashed { pid, .. } = read::widgets(&mut store, 3, 0).unwrap().engine else { panic!() };
    assert_eq!(pid, dead, "a watch-started whose process is gone and that no watch-stopped follows");
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
}

#[test]
fn no_read_writes() {
    let (fx, mut store) = fixture();
    // A second connection sees `data_version` change when any other connection commits.
    let watcher = Connection::open(fx.env.cpd_home().join("cpd.db")).unwrap();
    let (version, counts) = (data_version(&watcher), rows(&watcher));
    for _ in 0..3 {
        every_read(&mut store);
    }
    assert_eq!(data_version(&watcher), version, "a read committed something");
    assert_eq!(rows(&watcher), counts);
}

/// Contexts × Plans × Jobs, Attempts, Sessions and Events, enough for a long-lived machine.
fn populate_many(store: &mut Store) {
    let mut s = String::new();
    for c in 0..10 {
        s += &context(&format!("ctx{c}"));
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

/// Collects the SQL text of every statement the connection starts, through `sqlite3_trace_v2` (`rusqlite::ffi`,
/// no feature beyond `bundled` and `functions`).
unsafe extern "C" fn record_statement(
    _mask: std::ffi::c_uint,
    log: *mut std::ffi::c_void,
    _stmt: *mut std::ffi::c_void,
    sql: *mut std::ffi::c_void,
) -> std::ffi::c_int {
    // SAFETY: `log` is the leaked `Mutex` of `statements_run`; for `SQLITE_TRACE_STMT` the fourth argument is the statement's SQL text.
    let (log, sql) = unsafe { (&*log.cast::<Mutex<Vec<String>>>(), std::ffi::CStr::from_ptr(sql.cast())) };
    log.lock().unwrap().push(sql.to_string_lossy().into_owned());
    0
}

fn trace(store: &mut Store, mask: std::ffi::c_uint, log: &'static Mutex<Vec<String>>) {
    let ctx = std::ptr::from_ref(log).cast_mut().cast();
    // SAFETY: `store`'s connection is open for the call; `ctx` is leaked, so it outlives any statement.
    let rc = store.read(|c| Ok(unsafe { rusqlite::ffi::sqlite3_trace_v2(c.handle(), mask, Some(record_statement), ctx) })).unwrap();
    assert_eq!(rc, rusqlite::ffi::SQLITE_OK);
}

/// The statements `f` runs on the store's connection, in order, `BEGIN` and `COMMIT` of each read included.
fn statements_run(store: &mut Store, f: impl FnOnce(&mut Store)) -> Vec<String> {
    let log: &'static Mutex<Vec<String>> = Box::leak(Box::default());
    trace(store, rusqlite::ffi::SQLITE_TRACE_STMT, log);
    log.lock().unwrap().clear();
    f(store);
    let run = std::mem::take(&mut *log.lock().unwrap());
    trace(store, 0, log);
    run
}

/// §17: each read runs a fixed number of queries whatever the size of the Ledger. Counts the statements of each
/// read on the small fixture and on the large store (a wall-clock bound measures the machine's load as much as the work).
#[test]
fn every_read_runs_the_same_statements_on_a_large_store_as_on_a_small_one() {
    let (_fx, mut store) = fixture();
    // The Plans and the Attempt of the small fixture, then of the large store: same reads, ten times the rows.
    let reads: [(&str, &dyn Fn(&mut Store, bool)); 4] = [
        ("tree", &|s, _| drop(read::tree(s).unwrap())),
        ("flow", &|s, large| drop(read::flow(s, &plan(if large { "p3x3" } else { "ship" })).unwrap())),
        ("chat_slot", &|s, _| drop(read::chat_slot(s).unwrap())),
        ("widgets", &|s, _| drop(read::widgets(s, 3, 20).unwrap())),
    ];
    let run_all = |store: &mut Store, large: bool| -> Vec<Vec<String>> {
        reads
            .iter()
            .map(|(_, read)| statements_run(store, |s| read(s, large)))
            .collect()
    };
    let small = run_all(&mut store, false);

    populate_many(&mut store);
    assert!(last_event(&mut store) > 1600);
    let large = run_all(&mut store, true);

    for (((name, _), small), large) in reads.iter().zip(&small).zip(&large) {
        eprintln!("{name}: {} statements", small.len());
        assert!(small.len() > 2, "{name} ran no query: {small:#?}");
        assert_eq!(small.len(), large.len(), "{name} runs a number of statements that grows with the store:\nsmall: {small:#?}\nlarge: {large:#?}");
    }
}

#[test]
fn with_a_live_manager_the_snapshot_has_no_engine_down_though_the_engine_is_down() {
    let (_fx, mut store) = fixture();
    sql(&mut store, &session_row("m-alpha", "manager", "alpha", "", None, "idle", None, "2026-01-02T00:00:05.000Z"));
    let tree = read::tree(&mut store).unwrap();
    assert!(!tree.engine_running, "the lock is still free");
    assert!(tree.inbox.is_empty(), "a Manager handles it: {:?}", tree.inbox);
    sql(&mut store, "UPDATE sessions SET archived_at = '2026-01-04T00:00:00.000Z' WHERE id = 'm-alpha'");
    let tree = read::tree(&mut store).unwrap();
    assert_eq!(tree.inbox.iter().map(|i| i.kind).collect::<Vec<_>>(), [Kind::EngineDown], "an archived Manager is not live");
}

#[test]
fn with_no_engine_the_snapshot_leads_with_engine_down_and_while_a_process_holds_the_lock_it_does_not() {
    let (fx, mut store) = fixture();
    let tree = read::tree(&mut store).unwrap();
    assert!(!tree.engine_running);
    let [down] = &tree.inbox[..] else { panic!("only engine-down: {:?}", tree.inbox) };
    assert_eq!((down.kind, down.urgency, down.label.as_str()), (Kind::EngineDown, Urgency::Red, "engine is down"));
    assert_eq!((down.context.as_ref(), down.session.as_ref(), down.event, down.url.as_deref()), (None, None, None, None));
    assert!(!tree.contexts.iter().any(|c| c.dot.needs_you()), "it is no Session's or Context's item");

    // It comes before the stored items, which keep their order.
    inbox(&mut store, &[condition(Kind::Unread, Urgency::White, Some("l-sketch"), Some("sketch"), "alpha", "session:l-sketch")], T0);
    let tree = read::tree(&mut store).unwrap();
    assert_eq!(tree.inbox.iter().map(|i| i.kind).collect::<Vec<_>>(), [Kind::EngineDown, Kind::Unread]);

    let engine = engine(&fx.env);
    let tree = read::tree(&mut store).unwrap();
    assert!(tree.engine_running);
    assert_eq!(tree.inbox.iter().map(|i| i.kind).collect::<Vec<_>>(), [Kind::Unread]);
    drop(engine);
    assert!(!read::tree(&mut store).unwrap().engine_running);
}

#[test]
fn the_flow_gives_a_stacked_pr_its_base_pr_as_a_need_and_as_its_blocker_until_it_opens() {
    let (_fx, mut store) = fixture();
    // `stk` is stacked on `zeta` (its base is `zeta`'s Lead Branch) and stores a need on `mid`, which has merged
    sql(
        &mut store,
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
         VALUES ('acme/mid', 91, 'https://github.com/acme/mid/pull/91', 'Mid', 0, 'merged', 'none', 'none',
                 '2026-01-05T00:00:00.000Z', '2026-01-06T00:00:00.000Z', '2026-01-06T00:00:00.000Z'),
                ('acme/zeta', 92, 'https://github.com/acme/zeta/pull/92', 'Zeta', 0, 'open', 'none', 'none',
                 '2026-01-05T00:00:00.000Z', NULL, '2026-01-06T00:00:00.000Z');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, pr_number, pull_request, pr_url, pr_opened_at, merged_at, created_txn, created_at)
         VALUES ('ship', 'mid', '/r/mid', 'main', 'merged', 'cpd/ship/mid', '/wt/mid', '2026-01-02T00:00:00.000Z', 91, (SELECT id FROM pull_requests WHERE number = 91), 'https://github.com/acme/mid/pull/91', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z', cpd_txn(), '2026-01-02T00:00:00.000Z');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
         VALUES ('ship', 'zeta', '/r/zeta', 'main', 'dispatch', 'cpd/ship/zeta', '/wt/zeta', '2026-01-02T00:00:00.000Z', cpd_txn(), '2026-01-02T00:00:00.000Z');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('ship', 'stk', '/r/stk', 'cpd/ship/zeta', 'waiting', 'cpd/ship/stk', cpd_txn(), '2026-01-02T00:00:00.000Z');
         INSERT INTO pr_needs (plan, pr, needs) VALUES ('ship', 'stk', 'mid');",
    );
    let stk = |store: &mut Store| {
        let node = read::plan_boxes(store, &plan("ship")).unwrap().prs.into_iter().find(|p| p.key.as_str() == "stk").unwrap();
        (node.needs.iter().map(|n| n.as_str().to_string()).collect::<Vec<_>>(), node.waiting_on)
    };
    let (needs, waiting_on) = stk(&mut store);
    assert_eq!(needs, ["mid", "zeta"], "the base PR is a need, sorted with the stored ones");
    assert_eq!(waiting_on, ["zeta"], "the base PR has started and not opened: it blocks");
    // a base PR that is waiting blocks: `stk2` is stacked on `wb`, which has not started
    sql(
        &mut store,
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('ship', 'wb', '/r/wb', 'main', 'waiting', 'cpd/ship/wb', cpd_txn(), '2026-01-02T00:00:00.000Z');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('ship', 'stk2', '/r/wb', 'cpd/ship/wb', 'waiting', 'cpd/ship/stk2', cpd_txn(), '2026-01-02T00:00:00.000Z');",
    );
    let node = read::plan_boxes(&mut store, &plan("ship")).unwrap().prs.into_iter().find(|p| p.key.as_str() == "stk2").unwrap();
    assert_eq!(node.waiting_on, ["wb"], "a base PR that is waiting blocks");
    sql(&mut store, "UPDATE prs SET status = 'open', pr_number = 92, pull_request = (SELECT id FROM pull_requests WHERE number = 92), pr_url = 'https://github.com/acme/zeta/pull/92', pr_opened_at = '2026-01-02T00:00:00.000Z', started_at = '2026-01-02T00:00:00.000Z', lead_worktree = '/wt/zeta' WHERE plan = 'ship' AND key = 'zeta'");
    let (needs, waiting_on) = stk(&mut store);
    assert_eq!(needs, ["mid", "zeta"], "an open base PR is still listed");
    assert!(waiting_on.is_empty(), "an open base PR blocks nothing: {waiting_on:?}");
}
