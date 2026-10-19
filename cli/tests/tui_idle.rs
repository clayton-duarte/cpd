//! What the live views cost while nothing changes (commands.md §17, "Rates"; tui-F1, tui-F2, tui-F3 of
//! `docs/research/quality-tui.md`): a view re-reads its model only after an Event that concerns it, or when a pull request
//! row changed (which appends no Event), and draws only when its model, the size or a clock changed.
//!
//! Each test runs the views as `cpd tui view …` in panes of a private tmux server, on a store whose `jobs` table is
//! fat (a snapshot that scans it reads megabytes), lets them settle, and reads `/proc/<pid>/io` over a window with no
//! change at all: `rchar` is what the process read (SQLite's `pread`s included), `syscw` its `write` calls (every
//! `terminal.draw` is one, even with an empty diff). `CPD_IDLE_SECS` sets the window (default 5 s); the numbers are
//! printed (`cargo nextest run -p cpd --no-capture tui_idle`).
//!
//! The store is written all the time, as a live one is (a Worker's pane, a Session's label): a write that no view follows,
//! which also empties the page cache of the views' connections, so that a snapshot reads the file again.

mod common;

use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::{CPD, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The jobs of Plan `ship`: each row is about 4 KB, so one scan of the table is over a megabyte.
const FAT_JOBS: usize = 300;

/// The most an idle view may read in the window: its polls of the Events, about 20 KB a second. A snapshot of the fat
/// store is over a megabyte.
const READ_LIMIT: u64 = 300 * 1024;

fn env() -> TestEnv {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nconcurrency = 4\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [events]\npoll_ms = 200\n[tmux]\nsocket = \"{}\"\n",
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

/// A Plan: its PR row, its Lead (idle) and `jobs` queued Jobs of `FAT_JOBS` rows if `fat`.
fn plan(id: &str, fat: bool) -> String {
    let mut s = format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
         VALUES ('{id}', 'ctx', 'Plan {id}', 'dispatch', 'lead-{id}', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
         VALUES ('{id}', 'repo', '/repo', 'main', 'dispatch', 'cpd/{id}', '/wt', cpd_txn(), '2026-01-02T00:00:00.000Z',
                 '2026-01-02T00:00:00.000Z');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('lead-{id}', 'lead', 'ctx', '{id}', NULL, 'l-{id}', '/s/{id}.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');"
    );
    if fat {
        s += &format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {FAT_JOBS})
             INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                               fingerprint, created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
             SELECT '{id}', 'repo', 'k' || i, 'Job ' || i, 'S3', hex(randomblob(2000)), 'Why.', '[\"x\"]', '[\"y\"]', '[]',
                    'queued', 0, '{FP}', cpd_txn(), NULL, NULL, NULL, NULL, '2026-01-02T00:00:00.000Z' FROM n;"
        );
    }
    s
}

/// A Job `key` of `plan` that runs, with its Attempt `key`-a1 (a Worker with no Session yet).
fn running_job(plan: &str, key: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('{plan}', 'repo', '{key}', 'Job {key}', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', 'running', 0, '{FP}',
                 cpd_txn(), NULL, NULL, NULL, NULL, '2026-01-02T00:00:00.000Z');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, worker_session,
                               submitted_sha, submitted_at, merged_sha, started_at, ended_at)
         VALUES ('{plan}', '{key}-a1', '{key}', 1, 'S3', 'ladder', 'running', NULL, NULL, 'cpd/{plan}--{key}-a1', '/w/{key}-a1',
                 NULL, NULL, NULL, NULL, '2026-01-03T00:01:00.000Z', NULL);"
    )
}

/// Context `ctx`; Plan `ship` (fat, its Lead in the chat slot) and Plan `other`, which has the running Job `o1` for the
/// worker view. Nothing is working, so nothing spins.
fn fixture() -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'Title', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         {} {} {}
         INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', 'session-moved', 'ctx', 'ship', 'lead-ship', 'Session moved.',
                 '{{\"pane\":\"%1\",\"window\":\"@1\",\"place\":\"chat\"}}');",
        plan("ship", true),
        plan("other", false),
        running_job("other", "o1"),
    )
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// Starts `cpd tui view <view…>` in its own session of the private server; returns the pane and its process.
fn start(env: &TestEnv, name: &str, view: &[&str]) -> (String, u32) {
    let mut args = vec!["new-session", "-d", "-s", name, "-x", "120", "-y", "30", "-P", "-F", "#{pane_id} #{pane_pid}", CPD, "tui", "view"];
    args.extend(view);
    let out = tmux_out(env, &args);
    let (pane, pid) = out.trim().split_once(' ').unwrap();
    (pane.to_owned(), pid.parse().unwrap())
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// What `/proc/<pid>/io` and `/proc/<pid>/stat` say the process has done so far.
#[derive(Clone, Copy, Debug)]
struct Usage {
    rchar: u64,
    syscw: u64,
    /// utime + stime, in clock ticks.
    cpu: u64,
}

fn usage(pid: u32) -> Usage {
    let io = fs::read_to_string(format!("/proc/{pid}/io")).unwrap();
    let field = |name: &str| -> u64 {
        io.lines().find_map(|l| l.strip_prefix(name)?.trim().parse().ok()).unwrap_or_else(|| panic!("no {name} in {io}"))
    };
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let after_name: Vec<&str> = stat[stat.rfind(')').unwrap() + 2..].split(' ').collect();
    // fields 14 and 15 of the line are the 12th and 13th after the `)` and the state
    let cpu = after_name[11].parse::<u64>().unwrap() + after_name[12].parse::<u64>().unwrap();
    Usage { rchar: field("rchar:"), syscw: field("syscw:"), cpu }
}

/// The views `(name, args)` this file runs: one pane each.
const VIEWS: [(&str, &[&str]); 6] = [
    ("tree", &["tree"]),
    ("plan", &["plan"]),
    ("widgets", &["widgets"]),
    ("statusbar", &["statusbar"]),
    ("logs", &["logs"]),
    ("worker", &["worker", "o1-a1", "--plan", "other"]),
];

/// How long the views are watched with no change.
fn window() -> Duration {
    Duration::from_secs(std::env::var("CPD_IDLE_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(5))
}

/// Runs `views`, waits for each to draw and settle, and returns what each did over the window with no change.
fn idle(env: &TestEnv, views: &[&str]) -> Vec<(String, Usage)> {
    let window = window();
    let started: Vec<(String, String, u32)> = VIEWS
        .iter()
        .filter(|(name, _)| views.contains(name))
        .map(|(name, args)| {
            let (pane, pid) = start(env, name, args);
            (name.to_string(), pane, pid)
        })
        .collect();
    for (name, pane, _) in &started {
        wait_until(&format!("the {name} view to draw"), || {
            !tmux_out(env, &["capture-pane", "-p", "-t", pane]).trim().is_empty()
        });
    }
    let stop = AtomicBool::new(false);
    let (before, after) = std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut store = Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap();
            for n in 0.. {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                store
                    .write("noise", |tx| {
                        tx.execute("UPDATE sessions SET tmux_pane = ?1 WHERE id = 'lead-other'", [format!("%{n}")])?;
                        Ok::<(), CpdError>(())
                    })
                    .unwrap();
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        std::thread::sleep(Duration::from_secs(3)); // the first reads, and what follows them
        let before: Vec<Usage> = started.iter().map(|(.., pid)| usage(*pid)).collect();
        std::thread::sleep(window);
        let after: Vec<Usage> = started.iter().map(|(.., pid)| usage(*pid)).collect();
        stop.store(true, Ordering::Relaxed);
        (before, after)
    });
    started
        .into_iter()
        .zip(before.into_iter().zip(after))
        .map(|((name, ..), (b, a))| {
            let used = Usage { rchar: a.rchar - b.rchar, syscw: a.syscw - b.syscw, cpu: a.cpu - b.cpu };
            eprintln!("idle {name}: {window:?}: read {} B, {} writes, {} ticks of CPU", used.rchar, used.syscw, used.cpu);
            (name, used)
        })
        .collect()
}

/// tui-F1: the widgets and statusbar views re-read the whole Dispatch model every 2 s (1.9 MB each on the live store).
#[test]
fn idle_dispatch_views_read_next_to_nothing() {
    let env = env();
    sql(&env, &fixture());
    for (name, used) in idle(&env, &["widgets", "statusbar"]) {
        assert!(used.rchar < READ_LIMIT, "{name} read {} B while nothing changed", used.rchar);
    }
}

/// tui-F3: the tree takes a 2.4 MB snapshot every 2 s, and the plan view one of its Plan, with no Event.
#[test]
fn idle_tree_and_plan_read_next_to_nothing() {
    let env = env();
    sql(&env, &fixture());
    for (name, used) in idle(&env, &["tree", "plan"]) {
        assert!(used.rchar < READ_LIMIT, "{name} read {} B while nothing changed", used.rchar);
    }
}

/// tui-F2: every `terminal.draw` writes to the terminal, even with an empty diff. The widgets view draws when the
/// kernel updates the load average (every 5 s, and the value changed), which is a change: once per 5 s at most.
#[test]
fn idle_views_write_nothing_to_the_terminal() {
    let env = env();
    sql(&env, &fixture());
    for (name, used) in idle(&env, &["tree", "plan", "widgets", "statusbar", "logs", "worker"]) {
        let allowed = if name == "widgets" { 2 + window().as_secs() / 5 } else { 0 };
        assert!(used.syscw <= allowed, "{name} wrote {} times while nothing changed", used.syscw);
    }
}

/// What F2's fix must keep: a spinner on screen turns (4 frames a second), the tree's and the plan view's.
#[test]
fn a_spinner_on_screen_keeps_turning() {
    let env = env();
    sql(&env, &fixture());
    sql(&env, "UPDATE sessions SET state = 'working' WHERE id = 'lead-ship'; UPDATE jobs SET status = 'running' WHERE plan = 'ship' AND key = 'k1';");
    for (name, used) in idle(&env, &["tree", "plan"]) {
        assert!(used.syscw >= 10, "{name} wrote {} times in the window: the spinner stopped", used.syscw);
    }
}
