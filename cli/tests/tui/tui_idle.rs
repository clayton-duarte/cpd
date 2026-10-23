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

use crate::common;

use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{CPD, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The jobs of Plan `ship`: each row is about 4 KB, so one scan of the table is over a megabyte.
const FAT_JOBS: usize = 300;

/// The most an idle view may read in the window: its polls of the Events, about 20 KB a second. A snapshot of the fat
/// store is over a megabyte.
const READ_LIMIT: u64 = 300 * 1024;

/// An env whose engine runs: its lock is held until the test process ends, so the Inbox has no `engine-down` item, whose
/// age would be a clock on the tree's screen.
fn env() -> TestEnv {
    let env = TestEnv::new();
    let lock = env.cpd_home().join("locks/watch.lock");
    fs::create_dir_all(lock.parent().unwrap()).unwrap();
    let lock = Flock::lock(fs::File::create(lock).unwrap(), FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    std::mem::forget(lock);
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

/// What the process has done so far: `/proc/<pid>/io` and `/proc/<pid>/stat` on Linux; `ps` on macOS, which has no /proc
/// and no counter of bytes read or `write` calls (decision pro-d3), so only the CPU time.
#[derive(Clone, Copy, Debug)]
struct Usage {
    /// Linux only.
    rchar: Option<u64>,
    /// Linux only.
    syscw: Option<u64>,
    /// utime + stime, in milliseconds (Linux: clock ticks of 10 ms; macOS: `ps -o time=`, 10 ms resolution).
    cpu: u64,
}

#[cfg(not(target_os = "macos"))]
fn usage(pid: u32) -> Usage {
    let io = fs::read_to_string(format!("/proc/{pid}/io")).unwrap();
    let field = |name: &str| -> u64 {
        io.lines().find_map(|l| l.strip_prefix(name)?.trim().parse().ok()).unwrap_or_else(|| panic!("no {name} in {io}"))
    };
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let after_name: Vec<&str> = stat[stat.rfind(')').unwrap() + 2..].split(' ').collect();
    // fields 14 and 15 of the line are the 12th and 13th after the `)` and the state
    let ticks = after_name[11].parse::<u64>().unwrap() + after_name[12].parse::<u64>().unwrap();
    Usage { rchar: Some(field("rchar:")), syscw: Some(field("syscw:")), cpu: ticks * 10 }
}

#[cfg(target_os = "macos")]
fn usage(pid: u32) -> Usage {
    let out = std::process::Command::new("ps").args(["-o", "time=", "-p", &pid.to_string()]).output().unwrap();
    assert!(out.status.success(), "ps -p {pid}: {}", String::from_utf8_lossy(&out.stderr));
    Usage { rchar: None, syscw: None, cpu: parse_ps_time(String::from_utf8(out.stdout).unwrap().trim()) }
}

/// `ps -o time=`: `M:SS.cc`, or `H:MM:SS` (or `D-H:MM:SS`), in milliseconds.
#[cfg(target_os = "macos")]
fn parse_ps_time(text: &str) -> u64 {
    let (days, rest) = text.split_once('-').map_or((0, text), |(d, r)| (d.parse::<u64>().unwrap(), r));
    let parts: Vec<&str> = rest.split(':').collect();
    let secs = |s: &str| (s.parse::<f64>().unwrap_or_else(|_| panic!("ps time {text:?}")) * 1000.0).round() as u64;
    let whole = |s: &str| s.parse::<u64>().unwrap_or_else(|_| panic!("ps time {text:?}")) * 1000;
    let ms = match parts[..] {
        [m, s] => whole(m) * 60 + secs(s),
        [h, m, s] => (whole(h) * 60 + whole(m)) * 60 + secs(s),
        _ => panic!("ps time {text:?}"),
    };
    ms + days * 86_400_000
}

/// What a view did over the window with no change.
struct Idle {
    used: Usage,
    /// How many different screens the pane showed over the window (macOS only: the spinner's own observable).
    screens: usize,
}

/// The most CPU an idle view may use on macOS, of the window (decision pro-d3: the tests assert CPU time there).
/// guess: Linux has no CPU budget in this file; 20 % is far over an idle view's draw of a spinner at 4 frames a second.
#[cfg(target_os = "macos")]
const CPU_SHARE_PERCENT: u64 = 20;

/// The views `(name, args)` this file runs: one pane each.
const VIEWS: [(&str, &[&str]); 5] = [
    ("tree", &["tree"]),
    ("plan", &["plan"]),
    ("widgets", &["widgets"]),
    ("logs", &["logs"]),
    ("worker", &["worker", "o1-a1", "--plan", "other"]),
];

/// How long the views are watched with no change.
fn window() -> Duration {
    Duration::from_secs(std::env::var("CPD_IDLE_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(5))
}

/// Runs `views`, waits for each to draw and settle, and returns what each did over the window with no change.
fn idle(env: &TestEnv, views: &[&str]) -> Vec<(String, Idle)> {
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
    // a panic below (a pid `usage` cannot read) ends the noise thread too, or the scope never returns
    struct Stop<'a>(&'a AtomicBool);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let (before, after, screens) = std::thread::scope(|scope| {
        // inside the scope, which joins the noise thread before it lets a panic out
        let _stop = Stop(&stop);
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
                // time-claim: one write of noise every 300 ms, so the idle views have changes to ignore
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        // time-claim: the first reads, and what follows them, are over within 3 s; the window measured starts after
        std::thread::sleep(Duration::from_secs(3));
        let before: Vec<Usage> = started.iter().map(|(.., pid)| usage(*pid)).collect();
        let mut seen: Vec<std::collections::HashSet<String>> = started.iter().map(|_| Default::default()).collect();
        if cfg!(target_os = "macos") {
            // the spinner's frames: the pane's text, sampled over the window
            // `poll` never gets a value, so it runs the whole window
            common::poll(window, || {
                for ((_, pane, _), set) in started.iter().zip(&mut seen) {
                    set.insert(tmux_out(env, &["capture-pane", "-p", "-t", pane]));
                }
                // time-claim: one sample of the frames every 100 ms
                std::thread::sleep(Duration::from_millis(100));
                None::<()>
            });
        } else {
            std::thread::sleep(window);
        }
        let after: Vec<Usage> = started.iter().map(|(.., pid)| usage(*pid)).collect();
        stop.store(true, Ordering::Relaxed);
        (before, after, seen.iter().map(|s| s.len()).collect::<Vec<_>>())
    });
    started
        .into_iter()
        .zip(before.into_iter().zip(after).zip(screens))
        .map(|((name, ..), ((b, a), screens))| {
            let diff = |a: Option<u64>, b: Option<u64>| Some(a? - b?);
            let used = Usage { rchar: diff(a.rchar, b.rchar), syscw: diff(a.syscw, b.syscw), cpu: a.cpu - b.cpu };
            eprintln!("idle {name}: {window:?}: read {:?} B, {:?} writes, {} ms of CPU, {screens} screens", used.rchar, used.syscw, used.cpu);
            (name, Idle { used, screens })
        })
        .collect()
}

/// On macOS the CPU time of the window is under `CPU_SHARE_PERCENT` of it (decision pro-d3); Linux asserts its counters.
fn assert_cpu(name: &str, idle: &Idle) {
    #[cfg(target_os = "macos")]
    assert!(
        idle.used.cpu * 100 <= window().as_millis() as u64 * CPU_SHARE_PERCENT,
        "{name} used {} ms of CPU in {:?} while nothing changed",
        idle.used.cpu,
        window()
    );
    #[cfg(not(target_os = "macos"))]
    let _ = (name, idle);
}

/// tui-F1: the widgets view re-read the whole Dispatch model every 2 s (1.9 MB on the live store; the StatusBar is
/// tmux's status line since #108, fed by the engine).
#[test]
fn idle_dispatch_views_read_next_to_nothing() {
    let env = env();
    env.sql(&fixture());
    for (name, idle) in idle(&env, &["widgets"]) {
        assert_cpu(&name, &idle);
        // macOS has no counter of bytes read: only the CPU time is asserted there (decision pro-d3)
        if let Some(rchar) = idle.used.rchar {
            assert!(rchar < READ_LIMIT, "{name} read {rchar} B while nothing changed");
        }
    }
}

/// tui-F3: the tree takes a 2.4 MB snapshot every 2 s, and the plan view one of its Plan, with no Event.
#[test]
fn idle_tree_and_plan_read_next_to_nothing() {
    let env = env();
    env.sql(&fixture());
    for (name, idle) in idle(&env, &["tree", "plan"]) {
        assert_cpu(&name, &idle);
        // macOS has no counter of bytes read: only the CPU time is asserted there (decision pro-d3)
        if let Some(rchar) = idle.used.rchar {
            assert!(rchar < READ_LIMIT, "{name} read {rchar} B while nothing changed");
        }
    }
}

/// tui-F2: every `terminal.draw` writes to the terminal, even with an empty diff. The widgets view draws when the CPU,
/// GPU or memory % it shows changed, sampled every 2 s (hs-d257, hs-d333), which is a change: once per 2 s at most.
#[test]
fn idle_views_write_nothing_to_the_terminal() {
    let env = env();
    env.sql(&fixture());
    for (name, idle) in idle(&env, &["tree", "plan", "widgets", "logs", "worker"]) {
        assert_cpu(&name, &idle);
        // macOS has no counter of write calls: only the CPU time is asserted there (decision pro-d3)
        if let Some(syscw) = idle.used.syscw {
            let allowed = if name == "widgets" { 2 + window().as_secs() / 2 } else { 0 };
            assert!(syscw <= allowed, "{name} wrote {syscw} times while nothing changed");
        }
    }
}

/// What F2's fix must keep: a spinner on screen turns (4 frames a second), the tree's and the plan view's.
#[test]
fn a_spinner_on_screen_keeps_turning() {
    let env = env();
    env.sql(&fixture());
    env.sql("UPDATE sessions SET state = 'working' WHERE id = 'lead-ship'; UPDATE jobs SET status = 'running' WHERE plan = 'ship' AND key = 'k1';");
    for (name, idle) in idle(&env, &["tree", "plan"]) {
        match idle.used.syscw {
            Some(syscw) => assert!(syscw >= 10, "{name} wrote {syscw} times in the window: the spinner stopped"),
            // macOS (decision pro-d3): no write counter; what the spinner shows is the pane text, which changes each frame
            None => assert!(idle.screens >= 3, "{name} showed {} screens in the window: the spinner stopped", idle.screens),
        }
    }
}
