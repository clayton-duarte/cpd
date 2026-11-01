//! The tree view draws the cost of a Plan and of its Context from real session files (decision pro-d179): a prepared
//! store whose Lead and Manager Sessions name session files that the test writes, the view on the fakes of the `Fx`.

use crate::common;

use std::path::Path;

use common::tree::{Tree, engine, fx};
use common::{TestEnv, sql};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn context(id: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');"
    )
}

/// `pr`: the Plan has a PR (the schema ties phases `deploy` and `done` to one).
fn plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str, pr: bool) -> String {
    let late = !matches!(phase, "grounding" | "plan");
    let worktree = if late { "'/wt'" } else { "NULL" };
    let locked = if late { "'2026-01-02T00:00:00.000Z'" } else { "NULL" };
    let done = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
    let (row, number, url, opened, pull_request) = if pr {
        let state = if phase == "done" { "merged" } else { "open" };
        let merged = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
        (
            format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/{id}', 7, 'https://github.com/acme/{id}/pull/7', 'PR of {id}', 0, '{state}', 'none', 'none',
                         '2026-01-05T00:00:00.000Z', {merged}, '2026-01-06T00:00:00.000Z');"
            ),
            "7",
            format!("'https://github.com/acme/{id}/pull/7'"),
            "'2026-01-05T00:00:00.000Z'",
            format!("(SELECT id FROM pull_requests WHERE repo = 'acme/{id}')"),
        )
    } else {
        (String::new(), "NULL", "NULL".to_owned(), "NULL", "NULL".to_owned())
    };
    let status = match phase {
        "grounding" | "plan" => "waiting",
        "dispatch" => "dispatch",
        "deploy" => "open",
        _ => "merged",
    };
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
         VALUES ('{id}', '{context}', 'Plan {id}', '{phase}', '{lead}', '{created}', {locked}, {done});
         {row}
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pull_request, created_txn, created_at, started_at, merged_at)
         VALUES ('{id}', 'repo', '/repo/{id}', 'main', '{status}', 'cpd/{id}', {worktree}, {number}, {url}, {opened},
                 {pull_request}, cpd_txn(), '{created}', {locked}, {done});"
    )
}

/// A Session row with `file` (`None`: no session file) of `role`.
fn session(id: &str, role: &str, context: &str, plan: &str, file: Option<&Path>) -> String {
    let plan = if plan.is_empty() { "NULL".to_owned() } else { format!("'{plan}'") };
    let file = file.map_or("NULL".to_owned(), |f| format!("'{}'", f.display()));
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', '{role}', '{context}', {plan}, NULL, '{id}', {file}, '/repo', 'O3',
                 NULL, 'idle', NULL, '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');"
    )
}

const HEADER: &str = r#"{"type":"session","version":3,"id":"s","timestamp":"2026-01-02T00:00:00.000Z","cwd":"/repo"}"#;

fn usage(cost: f64) -> String {
    format!(r#"{{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":2,"cost":{{"total":{cost}}}}}"#)
}

fn message(cost: f64) -> String {
    format!(
        r#"{{"type":"message","id":"m","timestamp":"2026-01-02T00:00:01.000Z","message":{{"role":"assistant","stopReason":"stop","usage":{}}}}}"#,
        usage(cost)
    )
}

fn compaction(cost: f64) -> String {
    format!(r#"{{"type":"compaction","id":"c","timestamp":"2026-01-02T00:00:02.000Z","summary":"s","usage":{}}}"#, usage(cost))
}

/// Writes a session file of `lines` after the header into the Fx's home.
fn write(env: &TestEnv, name: &str, lines: &[String]) -> std::path::PathBuf {
    let path = env.home().join(name);
    std::fs::write(&path, format!("{HEADER}\n{}\n", lines.join("\n"))).unwrap();
    path
}

/// The drawn row that shows `text`.
fn row(screen: &[String], text: &str) -> String {
    screen.iter().find(|r| r.contains(text)).unwrap_or_else(|| panic!("no row with {text:?}: {screen:?}")).trim_end().to_owned()
}

#[test]
fn the_tree_draws_the_cost_of_a_plan_and_of_its_context_from_the_session_files() {
    let fx = fx();
    let _engine = engine(&fx);
    let lead = write(&fx.env, "lead.jsonl", &[message(0.10), message(0.20), compaction(0.05)]);
    let manager = write(&fx.env, "manager.jsonl", &[message(0.60)]);
    let mut s = context("alpha");
    s += &plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:10.000Z", false);
    s += &plan_row("bare", "alpha", "plan", "l-bare", "2026-01-02T00:00:09.000Z", false);
    s += &session("l-ship", "lead", "alpha", "ship", Some(&lead));
    s += &session("l-bare", "lead", "alpha", "bare", None);
    s += &session("m-alpha", "manager", "alpha", "", Some(&manager));
    sql(&mut open(&fx.env), &s);
    let mut tree = Tree::start(&fx, 60, 14);
    let screen = tree.screen();
    assert!(screen.iter().all(|r| !r.contains('$')), "the default: no cost on any row: {screen:?}");
    tree.keys(&["$"]);
    let screen = tree.screen();
    assert!(row(&screen, "alpha").contains("alpha $0.950"), "the Context: Plan + Manager: {screen:?}");
    assert!(row(&screen, " ship").contains("ship $0.350"), "the Plan: Lead messages + compaction: {screen:?}");
    assert!(!row(&screen, " bare").contains('$'), "a Plan with no file draws none: {screen:?}");
}

#[test]
fn the_tree_draws_no_cost_until_the_dollar_key() {
    let fx = fx();
    let _engine = engine(&fx);
    let lead = write(&fx.env, "lead.jsonl", &[message(0.10), message(0.20), compaction(0.05)]);
    let manager = write(&fx.env, "manager.jsonl", &[message(0.60)]);
    let mut s = context("alpha");
    s += &plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:10.000Z", false);
    s += &session("l-ship", "lead", "alpha", "ship", Some(&lead));
    s += &session("m-alpha", "manager", "alpha", "", Some(&manager));
    sql(&mut open(&fx.env), &s);
    let mut tree = Tree::start(&fx, 60, 14);
    let none = |screen: &[String]| screen.iter().all(|r| !r.contains('$'));
    let screen = tree.screen();
    assert!(none(&screen), "by default: {screen:?}");
    tree.keys(&["$"]);
    let screen = tree.screen();
    assert!(row(&screen, "alpha").contains("alpha $0.950") && row(&screen, " ship").contains("ship $0.350"), "after `$`: {screen:?}");
    tree.keys(&["$"]);
    let screen = tree.screen();
    assert!(none(&screen), "after a second `$`: {screen:?}");
}

#[test]
fn a_plan_with_no_session_file_draws_no_cost() {
    let fx = fx();
    let _engine = engine(&fx);
    let mut s = context("alpha");
    s += &plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:10.000Z", false);
    s += &session("l-ship", "lead", "alpha", "ship", None);
    s += &session("m-alpha", "manager", "alpha", "", None);
    sql(&mut open(&fx.env), &s);
    let tree = Tree::start(&fx, 60, 14);
    let screen = tree.screen();
    assert!(screen.iter().all(|r| !r.contains('$') && !r.contains('¢')), "no cost anywhere: {screen:?}");
}
