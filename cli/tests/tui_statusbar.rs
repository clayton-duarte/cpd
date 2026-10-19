//! The `statusbar` view (commands.md §13.2, §17), the bottom row of the cockpit, on a private tmux server: the three
//! glyph buttons at the left, and at the right edge `inbox <n>` (from the Inbox's visible items) and `workers N/M` (from the Attempts holding a slot and config `concurrency`).
//!
//! The store is prepared through the store API (`Store::write`), as `tui_plan.rs` does; the view runs as
//! `cpd tui view statusbar` in a pane, and its screen is read with `tmux capture-pane`.

mod common;

use std::fs;

use common::{CPD, TestEnv, wait_until};

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

/// One Plan `ship` with one Job per entry of `attempts`, each with one Attempt in that status.
fn plan_with_attempts(attempts: &[&str]) -> String {
    let mut s = format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('ctx', 'Title', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('g-ctx', 'lead', 'ctx', NULL, NULL, 'g', '/s/g.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '2026-01-01T00:00:00.000Z');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
         VALUES ('ship', 'ctx', 'Plan ship', 'dispatch', 'lead-ship', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
         VALUES ('ship', 'repo', '/repo', 'main', 'dispatch', 'cpd/ship', '/wt', cpd_txn(), '2026-01-02T00:00:00.000Z',
                 '2026-01-02T00:00:00.000Z');
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
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('ship', 'repo', 'j{n}', 'Job {n}', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', 'running', 0, '{FP}',
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

/// Starts `cpd tui view statusbar` alone in a one-row window `width` columns wide; returns its pane.
fn start_view(env: &TestEnv, width: u16) -> String {
    tmux_out(
        env,
        &["new-session", "-d", "-s", "view", "-x", &width.to_string(), "-y", "1", "-P", "-F", "#{pane_id}", "-e", common::VIEW_REFRESH, CPD, "tui", "view", "statusbar"],
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
fn the_row_has_the_three_buttons_left_and_the_workers_at_the_right_edge() {
    let env = env();
    env.sql(&plan_with_attempts(&["ended", "running", "starting"]));
    let pane = start_view(&env, 80);
    let row = row_ending(&env, &pane, "workers 2/4");
    assert!(row.starts_with("⧉ ? ▤"), "{row:?}");
    assert_eq!(row.chars().count(), 80, "the workers end at the last column: {row:?}");
}

#[test]
fn the_workers_follow_the_slots() {
    let env = env();
    env.sql(&plan_with_attempts(&[]));
    // the shortening to `0/4` on a row under 11 columns is `the_workers_indicator_is_given_the_full_width` (unit
    // test); this one uses a row wide enough for the long form
    let pane = start_view(&env, 40);
    row_ending(&env, &pane, "workers 0/4");
    // an Attempt takes a slot: the view re-reads when the store's Events move, or within its refresh (2 s, here `VIEW_REFRESH`)
    env.sql(&attempt_row(1, "running"));
    row_ending(&env, &pane, "workers 1/4");
}

#[test]
fn an_unknown_view_is_usage_and_the_statusbar_needs_a_terminal() {
    let env = env();
    let run = env.cpd(&["tui", "view", "nope"]);
    assert_eq!((run.exit, run.error()["code"].as_str()), (2, Some("usage")), "{run:?}");
    let run = env.cpd(&["tui", "view", "statusbar"]);
    assert_eq!((run.exit, run.error()["code"].as_str()), (3, Some("not_a_terminal")), "{run:?}");
}

/// An open Inbox item of `urgency`; its `subject` and label are `name`.
fn item(urgency: &str, name: &str) -> String {
    format!(
        "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at)
         VALUES ('engine-error', '{urgency}', '{name}', '{name}', '2026-01-02T00:00:00.000Z');"
    )
}

/// Waits until the row ends with `tail` and returns the screen's first row with its escape codes (`capture-pane -e`).
fn styled_row_ending(env: &TestEnv, pane: &str, tail: &str) -> String {
    row_ending(env, pane, tail);
    tmux_out(env, &["capture-pane", "-p", "-e", "-t", pane]).lines().next().unwrap_or("").to_owned()
}

#[test]
fn the_inbox_counts_the_visible_open_items_in_the_colour_of_the_most_urgent() {
    let env = env();
    env.sql(&plan_with_attempts(&[]));
    let pane = start_view(&env, 60);
    // nothing open: a dim zero, with no colour (SGR 2 is dim; tmux writes colours 1 and 3 as `38;5;1`, `38;5;3`)
    let row = styled_row_ending(&env, &pane, "inbox 0 workers 0/4");
    assert!(row.contains("\x1b[2minbox 0\x1b[0m"), "{row:?}");

    // red, yellow and white are open; a snoozed, a dismissed and a closed red one are not counted
    env.sql(
        &format!(
            "{}{}{}{}{}{}
             UPDATE inbox_items SET snoozed_until = '2999-01-01T00:00:00.000Z' WHERE subject = 'snoozed';
             UPDATE inbox_items SET dismissed_at = '2026-01-03T00:00:00.000Z' WHERE subject = 'dismissed';
             UPDATE inbox_items SET closed_at = '2026-01-03T00:00:00.000Z' WHERE subject = 'closed';",
            item("red", "red"),
            item("yellow", "yellow"),
            item("white", "white"),
            item("red", "snoozed"),
            item("red", "dismissed"),
            item("red", "closed"),
        ),
    );
    let row = styled_row_ending(&env, &pane, "inbox 3 workers 0/4");
    assert!(row.contains("\x1b[38;5;1minbox 3"), "red: {row:?}");

    // the red one is dismissed: the most urgent left is yellow
    env.sql("UPDATE inbox_items SET dismissed_at = '2026-01-04T00:00:00.000Z' WHERE subject = 'red';");
    let row = styled_row_ending(&env, &pane, "inbox 2 workers 0/4");
    assert!(row.contains("\x1b[38;5;3minbox 2"), "yellow: {row:?}");
}
