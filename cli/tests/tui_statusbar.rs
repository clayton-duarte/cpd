//! The `statusbar` view (commands.md §13.2, §17), the bottom row of the cockpit, on a private tmux server: the two
//! buttons at the left and `workers N/M` at the right edge, from the Attempts holding a slot and config `concurrency`.
//!
//! The store is prepared through the store API (`Store::write`), as `tui_plan.rs` does; the view runs as
//! `cpd2 tui view statusbar` in a pane, and its screen is read with `tmux capture-pane`.

mod common;

use std::fs;
use std::time::{Duration, Instant};

use common::{CPD2, TestEnv};
use cpd2::error::CpdError;
use cpd2::home::CpdHome;
use cpd2::store::Store;

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// An env whose config has `concurrency = 4` and the private tmux server.
fn env() -> TestEnv {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nconcurrency = 4\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [events]\npoll_ms = 50\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env
}

fn sql(env: &TestEnv, text: &str) {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000)
        .unwrap()
        .write("fixture", |tx| {
            tx.execute_batch(text)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

/// One Plan `ship` with one Job per entry of `attempts`, each with one Attempt in that status.
fn plan_with_attempts(attempts: &[&str]) -> String {
    let mut s = format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on, lead_session, grounded_seq)
         VALUES ('ctx', 'Title', NULL, '2026-01-01T00:00:00.000Z', 'm1', 'g-ctx', 0);
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('g-ctx', 'lead', 'ctx', NULL, NULL, 'g', '/s/g.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '2026-01-01T00:00:00.000Z');
         INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, pr_number,
                            pr_url, pr_opened_at, pr_seen, pr_polled_at, created_at, locked_at)
         VALUES ('ship', 'ctx', 'Plan ship', '/repo', 'main', 'cpd/ship', '/wt', 'dispatch', 'lead-ship', NULL, NULL, NULL,
                 '{{\"checks\":{{}},\"reviewIds\":[],\"commentIds\":[]}}', NULL, '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('lead-ship', 'lead', 'ctx', 'ship', NULL, 'l', '/s/l.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');"
    );
    for (i, status) in attempts.iter().enumerate() {
        s += &attempt_row(i + 1, status);
    }
    s
}

/// Job `j<n>` with its Attempt `j<n>-a1`; an `ended` one holds no slot.
fn attempt_row(n: usize, status: &str) -> String {
    let (outcome, cause, ended) =
        if status == "ended" { ("'failed'", "'crashed'", "'2026-01-03T01:00:00.000Z'") } else { ("NULL", "NULL", "NULL") };
    format!(
        "INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('ship', 'j{n}', 'Job {n}', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', 'running', 0, '{FP}',
                 cpd_txn(), NULL, NULL, NULL, NULL, '2026-01-02T00:00:00.000Z');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, worker_session,
                               submitted_sha, submitted_at, merged_sha, started_at, ended_at)
         VALUES ('ship', 'j{n}-a1', 'j{n}', 1, 'S3', 'ladder', '{status}', {outcome}, {cause}, 'cpd/ship--j{n}-a1', '/w/j{n}-a1',
                 NULL, NULL, NULL, NULL, '2026-01-03T00:{n:02}:00.000Z', {ended});"
    )
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// Waits until `done` holds, at most 30 s.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Starts `cpd2 tui view statusbar` alone in a one-row window `width` columns wide; returns its pane.
fn start_view(env: &TestEnv, width: u16) -> String {
    tmux_out(
        env,
        &["new-session", "-d", "-s", "view", "-x", &width.to_string(), "-y", "1", "-P", "-F", "#{pane_id}", CPD2, "tui", "view", "statusbar"],
    )
    .trim()
    .to_owned()
}

/// Waits until the row ends with `tail`, and returns it.
fn row_ending(env: &TestEnv, pane: &str, tail: &str) -> String {
    let mut row = String::new();
    wait_until(tail, || {
        row = tmux_out(env, &["capture-pane", "-p", "-t", pane]).lines().next().unwrap_or("").trim_end().to_owned();
        row.ends_with(tail)
    });
    row
}

#[test]
fn the_row_has_the_two_buttons_left_and_the_workers_at_the_right_edge() {
    let env = env();
    sql(&env, &plan_with_attempts(&["ended", "running", "starting"]));
    let pane = start_view(&env, 80);
    let row = row_ending(&env, &pane, "workers 2/4");
    assert!(row.starts_with("⧉   ? legend "), "{row:?}");
    assert_eq!(row.chars().count(), 80, "the workers end at the last column: {row:?}");
}

#[test]
fn the_workers_shorten_on_a_narrow_row_and_follow_the_slots() {
    let env = env();
    sql(&env, &plan_with_attempts(&[]));
    // the indicator gets the whole row, as in the reference: 10 columns hold `0/4` (3) but not `workers 0/4` (11)
    let pane = start_view(&env, 10);
    let row = row_ending(&env, &pane, "0/4");
    assert!(row.starts_with('⧉'), "{row:?}");
    assert!(!row.contains("workers"), "{row:?}");
    // an Attempt takes a slot: the view re-reads when the store's Events move, or within 2 s
    sql(&env, &attempt_row(1, "running"));
    row_ending(&env, &pane, "1/4");
}

#[test]
fn an_unknown_view_is_usage_and_the_statusbar_needs_a_terminal() {
    let env = env();
    let run = env.cpd2(&["tui", "view", "nope"]);
    assert_eq!((run.exit, run.error()["code"].as_str()), (2, Some("usage")), "{run:?}");
    let run = env.cpd2(&["tui", "view", "statusbar"]);
    assert_eq!((run.exit, run.error()["code"].as_str()), (3, Some("not_a_terminal")), "{run:?}");
}
