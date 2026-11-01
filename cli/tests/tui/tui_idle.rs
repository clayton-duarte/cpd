//! What the live views cost while nothing changes (commands.md §17, "Rates"; tui-F1, tui-F2, tui-F3 of
//! `docs/research/quality-tui.md`): a view re-reads its model only after an Event that concerns it, or when a pull request
//! row changed (which appends no Event), and draws only when its model, the size or a clock changed.
//!
//! Each test runs the views in the test process (`Tree::run`, `<view>::run_on`) on a `TestBackend`, on a store whose `jobs` table
//! is fat (a snapshot that scans it reads megabytes), lets them settle, and counts over a window of [`WINDOW`] of the
//! `ManualClock` with no change at all (no sleep, no process, no wall clock):
//! - the flushes of the terminal (`Display::flushes`): every `terminal.draw` is one, even with an empty diff. They are what
//!   `syscw` of the process counted when the views ran in a pane of tmux;
//! - the snapshots the view's loop took (`Tree::snapshots`, the loop's `Ctx::snapshot_read`): what a snapshot of the fat store
//!   costs is megabytes, so none may run. They are what `rchar` of the process counted: bytes read by a pane's process,
//!   SQLite's `pread`s included, which an in-process view has no counter of.
//!
//! The store is written all the time, as a live one is (a Worker's pane, a Session's label): a write that no view follows,
//! by a second connection of the test, on the file in the temp dir.
//!
//! The macOS branch of the old tests (the CPU time of the pane's process, decision pro-d3) went with the process: there is
//! no process to read it of.

use crate::common;

use std::ffi::OsString;
use std::time::Duration;

use clap::Parser;
use common::fx::Fx;
use common::tree::{self as drive, Tree, engine};
use cpd_core::error::CpdError;
use cpd_core::host::runner::Reply;
use cpd_core::output::Output;
use cpd_core::store::Store;
use cpd_tui::{logs, plan, widgets};

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The jobs of Plan `ship`: each row is about 4 KB, so one scan of the table is over a megabyte.
const FAT_JOBS: usize = 300;

/// How long the views are watched with no change.
const WINDOW: Duration = Duration::from_secs(5);

/// How long the views have to read their first model and settle before the window starts.
const SETTLE: Duration = Duration::from_secs(3);

/// The noise: one write of the store every 300 ms of the clock, so the idle views have changes to ignore.
const NOISE_EVERY: Duration = Duration::from_millis(300);

/// An `Fx` on fake ports with the config the views need. The engine's lock is held by the test (`engine`, the guard after the
/// `Fx`), so the Inbox has no `engine-down` item, whose age would be a clock on the tree's screen. The programs the machine's
/// name and load are read with (`hostname`, and macOS's `ioreg`, `sysctl`, `ps` and `vm_stat`) answer nothing.
fn env() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_models();
    fx.env.write_config(
        "version = 1\nmachine = \"m1\"\nconcurrency = 4\n\
         [events]\npoll_ms = 200\n",
    );
    for program in ["hostname", "ioreg", "sysctl", "ps", "vm_stat"] {
        fx.fake(program, Reply::ok(""));
    }
    fx
}

/// A Plan: its PR row, its Lead (idle) and `jobs` queued Jobs of `FAT_JOBS` rows if `fat`.
fn plan(id: &str, fat: bool) -> String {
    let mut s = format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
         VALUES ('{id}', 'ctx', 'Plan {id}', 'dispatch', 'lead-{id}', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
         VALUES ('{id}', 'repo', '/repo', 'main', 'dispatch', 'cpd/{id}', '/wt', cpd_txn(), '2026-01-02T00:00:00.000Z',
                 '2026-01-02T00:00:00.000Z');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('lead-{id}', 'lead', 'ctx', '{id}', NULL, 'l-{id}', '/s/{id}.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');"
    );
    if fat {
        s += &format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {FAT_JOBS})
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
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
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('{plan}', 'repo', '{key}', 'Job {key}', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', 'running', 0, '{FP}',
                 cpd_txn(), NULL, NULL, NULL, NULL, '2026-01-02T00:00:00.000Z');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, worker_session,
                               submitted_sha, submitted_at, merged_sha, started_at, ended_at)
         VALUES ('{plan}', '{key}-a1', '{key}', 1, 'S3', 'ladder', 'running', NULL, NULL, 'cpd/{plan}--{key}-a1', '/w/{key}-a1',
                 NULL, NULL, NULL, NULL, '2026-01-03T00:01:00.000Z', NULL);"
    )
}

/// Context `ctx`; Plan `ship` (fat, its Lead in the chat slot) and Plan `other`, which has the running Job `o1`. Nothing is working, so nothing spins.
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

/// `cpd tui view` has no command to run in these tests: no click is sent.
fn no_command(_: Vec<OsString>) -> Result<Output, CpdError> {
    panic!("the view ran a cpd command, and no click was sent")
}

/// The arguments of `cpd tui view logs`.
fn logs_args() -> logs::Args {
    #[derive(Parser)]
    struct Command {
        #[command(flatten)]
        args: logs::Args,
    }
    Command::try_parse_from(["logs"]).unwrap().args
}

/// The views this file runs, by name: one terminal of 120 x 30 each.
const VIEWS: [&str; 4] = ["tree", "plan", "widgets", "logs"];

fn start(fx: &Fx, name: &str) -> Tree {
    let entry: Box<drive::Entry> = match name {
        "tree" => return Tree::start(fx, 120, 30),
        "plan" => Box::new(|ports, tty, out, terminal, input| plan::run_on(ports, no_command, tty, out, terminal, input)),
        "widgets" => Box::new(|ports, tty, out, terminal, input| widgets::run_on(ports, no_command, tty, out, terminal, input)),
        "logs" => Box::new(|ports, tty, out, terminal, input| logs::run_on(ports, logs_args(), tty, out, terminal, input)),
        other => panic!("no view {other:?}"),
    };
    Tree::run(fx, name, 120, 30, entry)
}

/// What a view did over the window with no change.
struct Idle {
    /// The flushes of its terminal.
    flushes: usize,
    /// The snapshots its loop took.
    snapshots: usize,
}

/// Lets `view` run for `span` of the clock while the store is written to: a nap of the loop to its next time, then a write.
fn noisy(fx: &Fx, view: &mut Tree, noise: &mut Store, span: Duration) {
    let start = fx.fakes.clock.elapsed();
    let mut n = 0;
    while fx.fakes.clock.elapsed() - start < span {
        noise
            .write("noise", |tx| {
                tx.execute("UPDATE sessions SET tmux_pane = ?1 WHERE id = 'lead-other'", [format!("%{n}")])?;
                Ok::<(), CpdError>(())
            })
            .unwrap();
        n += 1;
        view.advance(NOISE_EVERY);
    }
}

/// Runs `name`, lets it draw and settle, and returns what it did over the window with no change but the noise.
fn idle(fx: &Fx, name: &str) -> Idle {
    let mut view = start(fx, name);
    let mut noise = fx.env.store();
    noisy(fx, &mut view, &mut noise, SETTLE);
    assert!(view.flushes() > 0, "{name} drew nothing before the window");
    let before = (view.flushes(), view.snapshots());
    noisy(fx, &mut view, &mut noise, WINDOW);
    let idle = Idle { flushes: view.flushes() - before.0, snapshots: view.snapshots() - before.1 };
    eprintln!("idle {name}: {WINDOW:?}: {} snapshots, {} flushes of the terminal", idle.snapshots, idle.flushes);
    idle
}

/// tui-F1: the widgets view re-read the whole Dispatch model every 2 s (1.9 MB on the live store; the StatusBar is
/// tmux's status line since #108, fed by the engine).
#[test]
fn idle_dispatch_views_read_next_to_nothing() {
    let fx = env();
    let _engine = engine(&fx);
    fx.env.sql(&fixture());
    let idle = idle(&fx, "widgets");
    assert_eq!(idle.snapshots, 0, "widgets took a snapshot while nothing changed");
}

/// tui-F3: the tree takes a 2.4 MB snapshot every 2 s, and the plan view one of its Plan, with no Event.
#[test]
fn idle_tree_and_plan_read_next_to_nothing() {
    let fx = env();
    let _engine = engine(&fx);
    fx.env.sql(&fixture());
    for name in ["tree", "plan"] {
        let idle = idle(&fx, name);
        assert_eq!(idle.snapshots, 0, "{name} took a snapshot while nothing changed");
    }
}

/// tui-F2: every `terminal.draw` writes to the terminal, even with an empty diff. The widgets view draws when the CPU,
/// GPU or memory % it shows changed, sampled every 2 s (hs-d257, hs-d333), which is a change: once per 2 s at most.
#[test]
fn idle_views_write_nothing_to_the_terminal() {
    let fx = env();
    let _engine = engine(&fx);
    fx.env.sql(&fixture());
    for name in VIEWS {
        let idle = idle(&fx, name);
        let allowed = if name == "widgets" { 2 + (WINDOW.as_secs() / 2) as usize } else { 0 };
        assert!(idle.flushes <= allowed, "{name} wrote {} times while nothing changed", idle.flushes);
    }
}

/// The counters of the tests above count: a commit of an Event that concerns a view makes its loop take a snapshot, and a key
/// is a frame of the terminal. (Without this, an idle view that is never counted would pass them all.)
#[test]
fn a_commit_that_concerns_a_view_makes_it_take_a_snapshot_and_a_key_makes_it_flush() {
    let fx = env();
    let _engine = engine(&fx);
    fx.env.sql(&fixture());
    for name in ["tree", "plan"] {
        let mut view = start(&fx, name);
        let mut noise = fx.env.store();
        noisy(&fx, &mut view, &mut noise, SETTLE);
        let before = view.snapshots();
        fx.env.sql(
            "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
             VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', 'session-state', 'ctx', 'ship', 'lead-ship', 'State.', '{\"from\":\"idle\",\"label\":null,\"to\":\"working\"}');",
        );
        noisy(&fx, &mut view, &mut noise, Duration::from_secs(2));
        assert!(view.snapshots() > before, "{name} took no snapshot after an Event that concerns it");
        let flushes = view.flushes();
        view.keys(&["j"]);
        assert!(view.flushes() > flushes, "{name} drew no frame for a key");
    }
}

/// An Event that changes no Inbox row concerns the widgets view no more: it takes no snapshot for it.
#[test]
fn a_session_state_event_alone_makes_the_widgets_view_take_no_snapshot() {
    let fx = env();
    let _engine = engine(&fx);
    fx.env.sql(&fixture());
    let mut view = start(&fx, "widgets");
    let mut noise = fx.env.store();
    noisy(&fx, &mut view, &mut noise, SETTLE);
    let before = view.snapshots();
    fx.env.sql(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', 'session-state', 'ctx', 'ship', 'lead-ship', 'State.', '{}');",
    );
    noisy(&fx, &mut view, &mut noise, Duration::from_secs(2));
    assert_eq!(view.snapshots(), before, "widgets took a snapshot while nothing changed");
}

/// The widgets view reads the Inbox when an open item changed in place (a label change appends no Event).
#[test]
fn a_change_of_an_open_inbox_item_makes_the_widgets_view_take_a_snapshot() {
    let fx = env();
    let _engine = engine(&fx);
    fx.env.sql(&fixture());
    fx.env.sql(
        "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at)
         VALUES ('pr-ready', 'blue', 'acme/repo#1', 'Ready', '2026-01-03T00:00:00.000Z');",
    );
    let mut view = start(&fx, "widgets");
    let mut noise = fx.env.store();
    noisy(&fx, &mut view, &mut noise, SETTLE);
    let before = view.snapshots();
    fx.env.sql("UPDATE inbox_items SET label = 'Ready now' WHERE subject = 'acme/repo#1';");
    noisy(&fx, &mut view, &mut noise, Duration::from_secs(2));
    assert!(view.snapshots() > before, "widgets took no snapshot after an open Inbox item changed");
}
