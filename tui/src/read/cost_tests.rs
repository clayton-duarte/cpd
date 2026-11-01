//! The cost cache and total (`read::cost`) and the cost of every node of `read::tree_at` (commands.md §17,
//! contract §2.2). The cache tests count calls through closures and touch no file system; the snapshot tests write
//! their session files into their own temporary directory (the process-wide cache never crosses tests) and read at
//! a fixed `now`.

use std::cell::Cell;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;

use crate::read::cost::{CostCache, total};
use crate::read::{self, ContextNode, PlanNode, SessionNode};

const NOW: &str = "2026-10-18T15:07:30.000Z";
const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

// ---- the cache and the total: no file system

fn stamp(secs: u64, len: u64) -> Option<(SystemTime, u64)> {
    Some((UNIX_EPOCH + Duration::from_secs(secs), len))
}

fn broken() -> CpdError {
    CpdError::new(ErrorCode::SessionMalformed, "x:1: not a session entry")
}

/// One read of `path` at `stamp` whose parse gives `parsed`; returns the answer and how often `parse` ran.
fn read_once(cache: &mut CostCache, path: &str, at: Option<(SystemTime, u64)>, parsed: Result<f64, CpdError>) -> (Option<f64>, u32) {
    let (stats, parses) = (Cell::new(0u32), Cell::new(0u32));
    let got = cache.cost(
        Path::new(path),
        |_| {
            stats.set(stats.get() + 1);
            at
        },
        |_| {
            parses.set(parses.get() + 1);
            parsed
        },
    );
    assert_eq!(stats.get(), 1, "stat is called once per call");
    (got, parses.get())
}

#[test]
fn an_unchanged_file_is_parsed_once() {
    let mut cache = CostCache::new();
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Ok(0.25)), (Some(0.25), 1));
    // Same stamp: the cached cost, parse not called (it would give 9.0).
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Ok(9.0)), (Some(0.25), 0));
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Ok(9.0)), (Some(0.25), 0));
}

#[test]
fn a_changed_mtime_parses_again() {
    let mut cache = CostCache::new();
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Ok(0.25)), (Some(0.25), 1));
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(11, 100), Ok(0.5)), (Some(0.5), 1));
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(11, 100), Ok(9.0)), (Some(0.5), 0));
}

#[test]
fn a_changed_length_parses_again() {
    let mut cache = CostCache::new();
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Ok(0.25)), (Some(0.25), 1));
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 101), Ok(0.5)), (Some(0.5), 1));
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 101), Ok(9.0)), (Some(0.5), 0));
}

#[test]
fn a_failed_parse_keeps_the_last_good_cost_and_parses_again_at_the_next_read() {
    let mut cache = CostCache::new();
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Ok(0.25)), (Some(0.25), 1));
    // The file changed and cannot be parsed: the last good cost.
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(11, 120), Err(broken())), (Some(0.25), 1));
    // The failed stamp was not stored: the same stamp parses again, and now it works.
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(11, 120), Ok(0.5)), (Some(0.5), 1));
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(11, 120), Ok(9.0)), (Some(0.5), 0));
}

#[test]
fn a_file_never_parsed_well_is_none() {
    let mut cache = CostCache::new();
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Err(broken())), (None, 1));
    // Nothing was stored: it parses again at the next read.
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Err(broken())), (None, 1));
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Ok(0.25)), (Some(0.25), 1));
}

#[test]
fn a_missing_file_is_none_is_not_parsed_and_leaves_no_entry() {
    let mut cache = CostCache::new();
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Ok(0.25)), (Some(0.25), 1));
    // Gone: None, no parse.
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", None, Ok(9.0)), (None, 0));
    // It comes back with the very stamp it had: the entry was dropped, so it is parsed again.
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Ok(0.5)), (Some(0.5), 1));
}

#[test]
fn two_files_have_their_own_entries() {
    let mut cache = CostCache::new();
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Ok(0.25)), (Some(0.25), 1));
    assert_eq!(read_once(&mut cache, "/s/b.jsonl", stamp(10, 100), Ok(1.0)), (Some(1.0), 1));
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(10, 100), Ok(9.0)), (Some(0.25), 0));
    assert_eq!(read_once(&mut cache, "/s/b.jsonl", stamp(10, 100), Ok(9.0)), (Some(1.0), 0));
    // A change of one leaves the other.
    assert_eq!(read_once(&mut cache, "/s/a.jsonl", stamp(11, 100), Ok(0.5)), (Some(0.5), 1));
    assert_eq!(read_once(&mut cache, "/s/b.jsonl", stamp(10, 100), Ok(9.0)), (Some(1.0), 0));
}

#[test]
fn a_total_of_no_data_is_none() {
    assert_eq!(total(Vec::new()), None);
    assert_eq!(total([None, None]), None);
}

#[test]
fn a_total_sums_the_parts_that_have_a_value() {
    let sum = total([Some(0.25), None, Some(1.0), Some(0.5)]).unwrap();
    assert!((sum - 1.75).abs() < 1e-9, "{sum}");
}

#[test]
fn a_total_of_data_that_sums_to_zero_is_some_zero() {
    assert_eq!(total([None, Some(0.0)]), Some(0.0));
    assert_eq!(total([Some(0.0), Some(0.0)]), Some(0.0));
}

// ---- the snapshot: `read::tree_at`

/// A store in a temporary home, which lives as long as the returned directory.
fn open(dir: &tempfile::TempDir) -> Store {
    Store::open(&CpdHome::resolve(Some(dir.path().join("home").into_os_string()), None).unwrap(), 5000).unwrap()
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

/// A temporary directory with a store (in `home/`) and room for session files.
struct Fx {
    dir: tempfile::TempDir,
    store: Store,
}

impl Fx {
    fn new() -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let store = open(&dir);
        Fx { dir, store }
    }

    /// The path of the session file `name`, which does not exist yet.
    fn path(&self, name: &str) -> String {
        self.dir.path().join(name).display().to_string()
    }

    /// Writes the session file `name` with `lines` and returns its path.
    fn file(&self, name: &str, lines: &[String]) -> String {
        let path = self.path(name);
        fs::write(&path, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        path
    }

    fn run(&mut self, text: &str) {
        sql(&mut self.store, text);
    }

    fn tree(&mut self) -> read::Tree {
        read::tree_at(&mut self.store, Some(NOW)).unwrap()
    }
}

fn header() -> String {
    r#"{"type":"session","version":3,"id":"aaa","timestamp":"2026-01-01T00:00:00.000Z","cwd":"/w"}"#.to_owned()
}

/// A fork header an hour after the entries it copies.
fn fork_header() -> String {
    r#"{"type":"session","version":3,"id":"bbb","timestamp":"2026-01-01T01:00:00.000Z","cwd":"/w","parentSession":"/s/parent.jsonl"}"#.to_owned()
}

/// An assistant message at `minute` of 2026-01-01T00 (01 for 60 and over) that cost `cost`.
fn assistant(n: u32, minute: u32, cost: f64) -> String {
    let (h, m) = (minute / 60, minute % 60);
    format!(
        r#"{{"type":"message","id":"m{n}","parentId":null,"timestamp":"2026-01-01T0{h}:{m:02}:00.000Z","message":{{"role":"assistant","content":[],"usage":{{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{{"total":{cost}}}}},"stopReason":"stop"}}}}"#
    )
}

/// A session file of `header` and assistant messages that cost `costs`.
pub(super) fn costing(costs: &[f64]) -> Vec<String> {
    let mut lines = vec![header()];
    lines.extend(costs.iter().enumerate().map(|(i, c)| assistant(i as u32, i as u32 + 1, *c)));
    lines
}

fn context(id: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('{id}', 'T {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');"
    )
}

pub(super) fn session(id: &str, role: &str, context: &str, plan: Option<&str>, attempt: Option<&str>, file: Option<&str>, extra: &str) -> String {
    let q = |v: Option<&str>| v.map_or("NULL".to_owned(), |v| format!("'{v}'"));
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', '{role}', '{context}', {}, {}, 'name {id}', {}, '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         {extra}",
        q(plan),
        q(attempt),
        q(file)
    )
}

/// A Plan `id` in `context` with its one Workflow and its Lead Session `l-<id>` (file `file`).
fn plan(id: &str, context: &str, file: Option<&str>) -> String {
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
         VALUES ('{id}', '{context}', 'Plan {id}', 'dispatch', 'l-{id}', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
         VALUES ('{id}', 'repo', '/repo', 'main', 'dispatch', 'cpd/{id}', '/wt', cpd_txn(), '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         {}",
        session(&format!("l-{id}"), "lead", context, Some(id), None, file, "")
    )
}

/// A side fork of the Plan's Lead, `f-<id>`, with the file `file`.
fn fork_of(id: &str, context: &str, file: &str) -> String {
    session(
        &format!("f-{id}"),
        "lead",
        context,
        Some(id),
        None,
        Some(file),
        &format!("UPDATE sessions SET forked_from = 'l-{id}', created_at = '2026-01-04T00:00:00.000Z' WHERE id = 'f-{id}';"),
    )
}

/// A Manager Session `id` of `context`; archived when `archived`.
fn manager(id: &str, context: &str, file: Option<&str>, archived: bool) -> String {
    let extra = if archived { format!("UPDATE sessions SET archived_at = '2026-01-05T00:00:00.000Z' WHERE id = '{id}';") } else { String::new() };
    session(id, "manager", context, None, None, file, &extra)
}

fn job(plan: &str, key: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, created_at)
         VALUES ('{plan}', 'repo', '{key}', 'Job', 'S3', 'G.', 'W.', '[\"x\"]', '[\"y\"]', '[]', 'queued', 0, '{FP}', cpd_txn(),
                 '2026-01-02T00:00:00.000Z');"
    )
}

/// Attempt `<job>-a1` of the Plan's Job `job`, with Worker Session `w-<job>` (file `file`). `metrics` is the stored
/// JSON of an ended Attempt, `None` for a live one (a running Attempt with NULL `metrics`).
fn attempt(plan: &str, context: &str, job_key: &str, metrics: Option<&str>, file: Option<&str>) -> String {
    let id = format!("{job_key}-a1");
    let (status, outcome, cause, ended, metrics) = match metrics {
        Some(m) => ("ended", "'failed'", "'crashed'", "'2026-01-02T02:00:00.000Z'", format!("'{m}'")),
        None => ("running", "NULL", "NULL", "NULL", "NULL".to_owned()),
    };
    format!(
        "{}
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at,
                               worker_session, metrics)
         VALUES ('{plan}', '{id}', '{job_key}', 1, 'S3', 'ladder', '{status}', {outcome}, {cause}, 'cpd/{plan}--{id}', '/w',
                 '2026-01-02T01:00:00.000Z', {ended}, 'w-{job_key}', {metrics});
         {}",
        job(plan, job_key),
        session(&format!("w-{job_key}"), "worker", context, Some(plan), Some(&id), file, "")
    )
}

fn ctx<'a>(t: &'a read::Tree, id: &str) -> &'a ContextNode {
    t.contexts.iter().find(|c| c.id.as_str() == id).unwrap()
}

fn pl<'a>(t: &'a read::Tree, context: &str, id: &str) -> &'a PlanNode {
    ctx(t, context).plans.iter().find(|p| p.head.id.as_str() == id).unwrap()
}

pub(super) fn near(got: Option<f64>, want: f64) {
    let got = got.unwrap_or_else(|| panic!("no cost, wanted {want}"));
    assert!((got - want).abs() < 1e-9, "cost {got}, wanted {want}");
}

fn lead_of<'a>(t: &'a read::Tree, context: &str, plan: &str) -> &'a SessionNode {
    &pl(t, context, plan).sessions[0]
}

#[test]
fn a_lead_session_has_its_own_cost() {
    let mut fx = Fx::new();
    let file = fx.file("lead.jsonl", &costing(&[0.25, 0.5]));
    fx.run(&(context("ctx") + &plan("p1", "ctx", Some(&file))));
    let t = fx.tree();
    near(lead_of(&t, "ctx", "p1").cost_usd, 0.75);
}

#[test]
fn a_session_with_no_file_or_a_missing_file_has_no_cost() {
    let mut fx = Fx::new();
    let missing = fx.path("never-written.jsonl");
    let present = fx.file("present.jsonl", &costing(&[0.25]));
    fx.run(&(context("ctx") + &plan("p1", "ctx", None) + &plan("p2", "ctx", Some(&missing)) + &plan("p3", "ctx", Some(&present))));
    let t = fx.tree();
    // A Session with a file does have one: the others' None is "no data", not a cost of zero.
    near(lead_of(&t, "ctx", "p3").cost_usd, 0.25);
    assert_eq!(lead_of(&t, "ctx", "p1").cost_usd, None);
    assert_eq!(lead_of(&t, "ctx", "p2").cost_usd, None);
}

#[test]
fn a_fork_session_s_cost_leaves_out_what_it_inherited() {
    let mut fx = Fx::new();
    let lead = fx.file("lead.jsonl", &costing(&[0.5, 0.25]));
    // The fork's header, the lead's entries as pi copies them (earlier timestamps), then its own 1.0.
    let mut lines = vec![fork_header()];
    lines.extend(costing(&[0.5, 0.25]).into_iter().skip(1));
    lines.push(assistant(9, 70, 1.0));
    let fork = fx.file("fork.jsonl", &lines);
    fx.run(&(context("ctx") + &plan("p1", "ctx", Some(&lead)) + &fork_of("p1", "ctx", &fork)));
    let t = fx.tree();
    let sessions = &pl(&t, "ctx", "p1").sessions;
    assert_eq!(sessions.len(), 2);
    near(sessions[0].cost_usd, 0.75);
    near(sessions[1].cost_usd, 1.0);
}

#[test]
fn a_plan_costs_its_lead_its_forks_and_its_attempts() {
    let mut fx = Fx::new();
    let lead = fx.file("lead.jsonl", &costing(&[0.5]));
    let mut lines = vec![fork_header()];
    lines.push(assistant(9, 70, 0.25));
    let fork = fx.file("fork.jsonl", &lines);
    let w1 = fx.file("w1.jsonl", &costing(&[1.0]));
    let w2 = fx.file("w2.jsonl", &costing(&[0.25]));
    fx.run(
        &(context("ctx")
            + &plan("p1", "ctx", Some(&lead))
            + &fork_of("p1", "ctx", &fork)
            + &attempt("p1", "ctx", "j1", Some(r#"{"cost_usd":1.0}"#), Some(&w1))
            + &attempt("p1", "ctx", "j2", None, Some(&w2))),
    );
    let t = fx.tree();
    // 0.5 + 0.25 + 1.0 (stored) + 0.25 (live, from its file)
    near(pl(&t, "ctx", "p1").cost_usd, 2.0);
}

#[test]
fn an_ended_attempt_costs_its_stored_metrics_not_its_session_file() {
    let mut fx = Fx::new();
    let w = fx.file("w.jsonl", &costing(&[1.0])); // says 1.0; the stored metrics say 0.5
    fx.run(&(context("ctx") + &plan("p1", "ctx", None) + &attempt("p1", "ctx", "j1", Some(r#"{"cost_usd":0.5}"#), Some(&w))));
    let t = fx.tree();
    near(pl(&t, "ctx", "p1").cost_usd, 0.5);
}

#[test]
fn a_live_attempt_costs_its_worker_sessions_file() {
    let mut fx = Fx::new();
    let w = fx.file("w.jsonl", &costing(&[0.25, 0.25]));
    fx.run(&(context("ctx") + &plan("p1", "ctx", None) + &attempt("p1", "ctx", "j1", None, Some(&w))));
    let t = fx.tree();
    near(pl(&t, "ctx", "p1").cost_usd, 0.5);
}

#[test]
fn an_attempt_whose_metrics_have_no_cost_has_no_data() {
    let mut fx = Fx::new();
    // The session file says 1.0, but a written `metrics` is the only source: no data, not the file's 1.0.
    let w = fx.file("w.jsonl", &costing(&[1.0]));
    fx.run(
        &(context("ctx")
            + &plan("p1", "ctx", None)
            + &plan("p2", "ctx", None)
            + &attempt("p1", "ctx", "j1", Some(r#"{"turns":3}"#), Some(&w))
            + &attempt("p2", "ctx", "j2", Some(r#"{"cost_usd":0.25}"#), None)),
    );
    let t = fx.tree();
    assert_eq!(pl(&t, "ctx", "p1").cost_usd, None);
    near(pl(&t, "ctx", "p2").cost_usd, 0.25);
}

#[test]
fn a_context_costs_its_plans_and_its_managers() {
    let mut fx = Fx::new();
    let l1 = fx.file("l1.jsonl", &costing(&[0.5]));
    let l2 = fx.file("l2.jsonl", &costing(&[0.25]));
    let m_live = fx.file("m1.jsonl", &costing(&[1.0]));
    let m_old = fx.file("m0.jsonl", &costing(&[0.25]));
    fx.run(
        &(context("ctx")
            + &context("other")
            + &plan("p1", "ctx", Some(&l1))
            + &plan("p2", "ctx", Some(&l2))
            + &manager("m-old", "ctx", Some(&m_old), true)
            + &manager("m-live", "ctx", Some(&m_live), false)),
    );
    let t = fx.tree();
    let c = ctx(&t, "ctx");
    near(c.cost_usd, 2.0); // 0.5 + 0.25 + 1.0 + 0.25
    assert_eq!(c.archived_managers.len(), 1);
    near(c.archived_managers[0].cost_usd, 0.25);
    near(pl(&t, "ctx", "p1").cost_usd, 0.5);
    near(pl(&t, "ctx", "p2").cost_usd, 0.25);
    assert_eq!(ctx(&t, "other").cost_usd, None);
}

#[test]
fn an_archived_plan_still_counts_in_its_context() {
    let mut fx = Fx::new();
    let l1 = fx.file("l1.jsonl", &costing(&[0.5]));
    let l2 = fx.file("l2.jsonl", &costing(&[0.25]));
    fx.run(
        &(context("ctx")
            + &plan("p1", "ctx", Some(&l1))
            + &plan("p2", "ctx", Some(&l2))
            + "UPDATE plans SET archived_at = '2026-01-06T00:00:00.000Z' WHERE id = 'p2';"),
    );
    let t = fx.tree();
    assert!(pl(&t, "ctx", "p2").head.archived_at.is_some());
    near(pl(&t, "ctx", "p2").cost_usd, 0.25);
    near(ctx(&t, "ctx").cost_usd, 0.75);
}

#[test]
fn a_context_with_no_data_has_no_cost() {
    let mut fx = Fx::new();
    let missing = fx.path("never-written.jsonl");
    let busy = fx.file("busy.jsonl", &costing(&[0.25]));
    fx.run(
        &(context("ctx")
            + &context("busy")
            + &plan("b1", "busy", Some(&busy))
            + &plan("p1", "ctx", None)
            + &attempt("p1", "ctx", "j1", Some(r#"{"turns":1}"#), None)
            + &attempt("p1", "ctx", "j2", None, Some(&missing))
            + &manager("m1", "ctx", None, false)),
    );
    let t = fx.tree();
    assert_eq!(pl(&t, "ctx", "p1").cost_usd, None);
    assert_eq!(ctx(&t, "ctx").cost_usd, None);
    // Not "no data anywhere": the other Context has some.
    near(ctx(&t, "busy").cost_usd, 0.25);
}

#[test]
fn a_changed_session_file_shows_its_new_cost_at_the_next_snapshot() {
    let mut fx = Fx::new();
    let file = fx.file("lead.jsonl", &costing(&[0.25]));
    fx.run(&(context("ctx") + &plan("p1", "ctx", Some(&file))));
    near(lead_of(&fx.tree(), "ctx", "p1").cost_usd, 0.25);
    // Another length (and so another stamp, whatever the file system's clock): the new cost.
    fx.file("lead.jsonl", &costing(&[0.25, 1.0, 0.5]));
    let t = fx.tree();
    near(lead_of(&t, "ctx", "p1").cost_usd, 1.75);
    near(pl(&t, "ctx", "p1").cost_usd, 1.75);
    near(ctx(&t, "ctx").cost_usd, 1.75);
}
