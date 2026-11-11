//! E22, Permanent Contexts (contract §3.7 step E22; pro-d342): every tick ensures the Contexts `cpd`, `host` and
//! `codereview` and a live Manager for each. Each is ensured on its own, so one refusal does not stop the others;
//! the first error is reported once after the loop, and the step clears only when all three succeeded.

use super::engine::Engine;
use super::report::{About, Step};
use cpd_core::context::permanent::ensure;
use cpd_core::ids::PermanentContext;

pub(super) fn run(engine: &mut Engine) {
    let mut first = None;
    for context in PermanentContext::ALL {
        if let Err(e) = ensure(&engine.home, &engine.config, &mut engine.store, context) {
            first.get_or_insert(e);
        }
    }
    match first {
        Some(e) => engine.fail(Step::E22, About::Machine, &e),
        None => engine.clear(Step::E22, &About::Machine),
    }
}

#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
use clap::Parser;

#[cfg(test)]
use cpd_core::clock::ManualClock;
#[cfg(test)]
use cpd_core::host::Tmux;
#[cfg(test)]
use cpd_core::host::runner::FakeRunner;
#[cfg(test)]
use cpd_core::host::spawn::FakeSpawn;
#[cfg(test)]
use cpd_core::host::tmux::SERVER_OPTIONS;
#[cfg(test)]
use cpd_core::ports::Ports;

#[cfg(test)]
use crate::watch::hosts;
#[cfg(test)]
use crate::watch::testkit::Fx;

#[cfg(test)]
#[derive(Parser)]
struct ContextCli {
    #[command(subcommand)]
    command: cpd_core::context::Command,
}

#[cfg(test)]
#[derive(Parser)]
struct SessionCli {
    #[command(subcommand)]
    command: cpd_core::session::Command,
}

#[cfg(test)]
fn context_cmd(words: &[&str]) -> cpd_core::context::Command {
    ContextCli::try_parse_from(std::iter::once("cpd").chain(words.iter().copied()))
        .unwrap()
        .command
}

#[cfg(test)]
fn session_cmd(words: &[&str]) -> cpd_core::session::Command {
    SessionCli::try_parse_from(std::iter::once("cpd").chain(words.iter().copied()))
        .unwrap()
        .command
}

/// An engine on a temporary home configured with `manager_runner = "durable"`,
/// with fake ports and a fake tmux server.
#[cfg(test)]
fn durable_mgr_fx() -> (Fx, Arc<FakeSpawn>, FakeRunner) {
    let mut fx = Fx::new("manager_runner = \"durable\"");
    let runner = fx.on_fake_tmux();
    unsafe {
        std::env::set_var("TMUX_TMPDIR", fx.engine.home.root());
        std::env::set_var("CPD_HOME", fx.engine.home.root());
        std::env::remove_var("CPD_SESSION");
        std::env::remove_var("PI_SESSION_ID");
    }
    fx.engine.tmux =
        Tmux::new(fx.engine.config.tmux.socket.clone()).with_runner(Arc::new(runner.clone()));
    fx.tmux_server();
    fx.engine
        .tmux
        .run(&["new-session", "-d", "-s", "cpd", "sleep", "600"].map(str::to_owned))
        .unwrap();
    let spawn = Arc::new(FakeSpawn::new());
    let clock = Arc::new(ManualClock::new());
    fx.engine.ports = Ports {
        clock,
        runner: Arc::new(runner.clone()),
        spawn: spawn.clone(),
        ..Ports::real()
    };
    (fx, spawn, runner)
}

/// Contract §6.23 item 1 & Acceptance:
/// Durable Managers are supervised directly by the engine's `hosts` step without tmux.
/// `context manager --runner durable` creates a Manager with no pane and makes no tmux call,
/// and the engine's `hosts` step starts and supervises its durable host.
#[test]
fn engine_supervises_durable_manager() {
    let (mut fx, spawn, runner) = durable_mgr_fx();

    // 1. Create a durable Manager via context manager command
    let ports = fx.engine.ports.clone();
    Ports::scope(&ports, || {
        cpd_core::context::run(context_cmd(&[
            "manager",
            "--context",
            "ctx",
            "--runner",
            "durable",
        ]))
    })
    .expect("context manager succeeds");

    // Verify that the durable manager row has null pane and runner 'durable'
    let (mgr_id, state, runner_str, pane): (String, String, String, Option<String>) = fx
        .engine
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT id, state, runner, tmux_pane FROM sessions WHERE context = 'ctx' AND role = 'manager'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .expect("manager row exists");

    assert_eq!(state, "stopped");
    assert_eq!(runner_str, "durable");
    assert!(pane.is_none(), "durable manager has no pane");

    // Verify that context manager made no new-window or pane calls in tmux
    let new_windows = runner
        .calls()
        .iter()
        .filter(|c| c.args.iter().any(|a| a == "new-window"))
        .count();
    assert_eq!(
        new_windows, 0,
        "no tmux window is created for a durable manager"
    );

    // 2. The engine's hosts step supervises it: starts the durable host
    hosts::run(&mut fx.engine);

    let launches = spawn.launches();
    assert_eq!(
        launches.len(),
        1,
        "hosts step launched the host: {launches:?}"
    );
    let session_arg = launches[0]
        .args
        .iter()
        .position(|a| a == "--session")
        .expect("--session in args");
    assert_eq!(
        launches[0].args.get(session_arg + 1).map(String::as_str),
        Some(mgr_id.as_str())
    );

    // 3. Supervision contract check (§6.23): keep-alive hooks are retired from cpd watch.
    // SERVER_OPTIONS must contain no window-unlinked hook and no cpd watch --ensure string.
    let has_window_unlinked = SERVER_OPTIONS
        .iter()
        .any(|opt| opt.iter().any(|arg| arg == "window-unlinked"));
    assert!(
        !has_window_unlinked,
        "window-unlinked hook must be removed from SERVER_OPTIONS (§6.23)"
    );

    let has_watch_ensure = SERVER_OPTIONS
        .iter()
        .any(|opt| opt.iter().any(|arg| arg.contains("cpd watch --ensure")));
    assert!(
        !has_watch_ensure,
        "cpd watch --ensure must be removed from SERVER_OPTIONS (§6.23)"
    );
}

/// Contract §6.23 & Plan 6 Acceptance:
/// Manager wake without tmux: a message delivered to a durable Manager appends
/// its session-message Event and starts no tmux pane or window. Also verifies that
/// SERVER_OPTIONS has retired the watch keep-alive hooks.
#[test]
fn manager_wake_without_tmux() {
    let (mut fx, _spawn, runner) = durable_mgr_fx();

    // Register a stopped durable manager in Context 'ctx'
    fx.fixture(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
         VALUES ('mgr1', 'manager', 'ctx', NULL, 'ctx-manager', '/', 'O4', 'stopped', 't', 't', 'durable');"
    );

    let calls_before = runner.calls().len();

    // Deliver a message to the Manager (session send wakes the session, commands.md §10.13, contract §5.3)
    let ports = fx.engine.ports.clone();
    Ports::scope(&ports, || {
        cpd_core::session::run(session_cmd(&[
            "send",
            "mgr1",
            "Wake up manager",
            "--unblocks",
            "testing wake",
        ]))
    })
    .expect("session send succeeds");

    // The session-message Event exists
    let msg_count: i64 = fx
        .engine
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM events WHERE kind = 'session-message' AND session = 'mgr1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(msg_count, 1, "session-message Event is appended");

    // No tmux calls were made to wake the durable manager
    let calls_after = runner.calls().len();
    assert_eq!(calls_before, calls_after, "no tmux calls made during wake");

    // Verify retired watch keep-alive hooks in server options (§6.23)
    for opt in SERVER_OPTIONS.iter() {
        assert!(
            !opt.iter().any(|arg| arg == "window-unlinked"),
            "window-unlinked hook must not be present in SERVER_OPTIONS"
        );
        assert!(
            !opt.iter().any(|arg| arg.contains("cpd watch --ensure")),
            "cpd watch --ensure must not be present in SERVER_OPTIONS"
        );
    }
}
