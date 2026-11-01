//! `cpd tui` in the test process, on the ports of an `Fx` and its fake tmux server: the cockpit's own code builds
//! and repairs the layout, sets the options and answers a click, and the test reads what it asked of tmux.
//!
//! The cockpit runs other areas' commands through a `RunCommand`: [`run_command`] records them and answers
//! `cpd watch --ensure` as the engine would (the first call starts it, the next ones find it running); every other
//! command runs in this process on the ports of the test (`session open` moves a Lead into the chat slot).

use std::cell::{Cell, RefCell};
use std::ffi::OsString;

use clap::Parser;
use cpd::cli::{Cli, Command};
use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use cpd_engine::watch::restart;
use cpd_tui::{Restart, RestartOutcome};
use serde_json::{Value, json};

use super::fx::Fx;

thread_local! {
    static RAN: RefCell<Vec<Vec<String>>> = const { RefCell::new(Vec::new()) };
    static ENGINE_STARTED: Cell<bool> = const { Cell::new(false) };
}

/// The tmux restart of an engine that is never reached from the cockpit's own server.
pub const NO_RESTART: Restart = Restart {
    failed: |_, _| Ok(None),
    clear: |_| Ok(()),
    mark: |_, _, _| Ok(false),
    now: |_, _, _| Ok(RestartOutcome::NotNeeded),
};

/// The engine's restart, as `cpd` hands it to the cockpit (`cli/src/cli.rs` `RESTART`): the engine's own `restart::*`, on
/// the ports of the test (the server's version is the route's; the pending file is a file of the test's home).
pub const RESTART: Restart = Restart {
    failed: restart_failed,
    clear: restart::clear,
    mark: restart::mark,
    now: restart_now,
};

fn restart_failed(
    home: &CpdHome,
    zone: &cpd_core::clock::TimeZone,
) -> Result<Option<String>, CpdError> {
    Ok(restart::read(home)?
        .filter(|p| p.failed.is_some())
        .map(|p| p.describe(zone)))
}

fn restart_now(
    home: &CpdHome,
    config: &Config,
    store: &mut Store,
) -> Result<RestartOutcome, CpdError> {
    Ok(match restart::restart(home, config, store, &mut |_| ())? {
        restart::Outcome::NotNeeded => RestartOutcome::NotNeeded,
        restart::Outcome::Running => RestartOutcome::Running,
        restart::Outcome::Busy(why) => RestartOutcome::Busy(why),
        restart::Outcome::Restarted { reopened } => RestartOutcome::Restarted { reopened },
    })
}

fn run_command(argv: Vec<OsString>) -> Result<Output, CpdError> {
    run_command_on(&Ports::current(), argv)
}

fn run_command_on(ports: &Ports, argv: Vec<OsString>) -> Result<Output, CpdError> {
    let words: Vec<String> = argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    RAN.with(|ran| ran.borrow_mut().push(words.clone()));
    if words.iter().map(String::as_str).collect::<Vec<_>>() == ["cpd", "watch", "--ensure"] {
        let first = !ENGINE_STARTED.replace(true);
        return Ok(Output::success(if first {
            json!({"started": true, "pane": "%99"})
        } else {
            json!({"started": false, "already": true})
        }));
    }
    cpd::cli::run_with(ports, argv)
}

/// The commands the cockpit ran through its `RunCommand` in this test, each as its `argv` (`cpd` first).
pub fn ran() -> Vec<Vec<String>> {
    RAN.with(|ran| ran.borrow().clone())
}

/// `cpd tui <args>` as the command tree parses it, run on the ports of `fx`; its result.
pub fn tui_result(fx: &Fx, args: &[&str]) -> Result<Output, CpdError> {
    tui_on(&fx.fakes.ports, args)
}

/// [`tui_result`] on `ports`.
fn tui_on(ports: &Ports, args: &[&str]) -> Result<Output, CpdError> {
    let argv = ["cpd", "tui"].iter().chain(args).map(OsString::from);
    let Cli {
        command: Some(Command::Tui(args)),
    } = Cli::try_parse_from(argv).expect("cpd tui parses")
    else {
        unreachable!("cpd tui is the Tui command")
    };
    Ports::scope(ports, || {
        cpd_tui::run_with(
            ports,
            args,
            cpd::cli::version(),
            run_command,
            run_command_on,
            RESTART,
        )
    })
}

/// Bare `cpd` in a terminal, on the ports of `fx`: the follow-ups, then the attach, which the `FakeSpawn` records
/// (`fx.fakes.spawn.launches()`) and fails with `faked exec`, so the result is that error (the engine's `watch --ensure`
/// is recorded, [`ran`]). With `restart` as the engine's restart.
pub fn bare(fx: &Fx, restart: Restart) -> Result<Output, CpdError> {
    Ports::scope(&fx.fakes.ports, || {
        cpd_tui::bare(cpd::cli::version(), run_command, restart)
    })
}

/// The `cpd tui` the engine's restart runs once the server is killed (`restart::rebuild_cockpit`): it runs the program
/// that started this process (`started_as`, the test binary here), so the route of that name answers `tui` by running the
/// cockpit in this process, on the ports of the call. Needs `cpd_core::host::exe::record_start()` first.
pub fn route_the_restarts_cpd_tui(fx: &Fx) {
    let exe = cpd_core::host::exe::started_as().expect("record_start ran");
    let name = exe
        .file_name()
        .expect("the test binary has a name")
        .to_string_lossy()
        .into_owned();
    let _ = (*fx.fakes.runner)
        .clone()
        .route(&name, std::sync::Arc::new(CpdTui));
}

/// The `cpd tui` of a restart, in this process.
struct CpdTui;

impl Handler for CpdTui {
    fn handle(&self, call: &Call) -> Reply {
        assert_eq!(
            call.args,
            ["tui"],
            "the restart runs `cpd tui` and nothing else"
        );
        match tui_on(&Ports::current(), &[]) {
            Ok(Output::Success(fields)) => Reply::ok(&format!("{}\n", Value::Object(fields))),
            other => Reply::exit(3, "", &format!("{other:?}\n")),
        }
    }
}

/// `cpd tui <args>`: the JSON it prints; panics on an error or on a command that owns its stdout.
pub fn tui(fx: &Fx, args: &[&str]) -> Value {
    match tui_result(fx, args) {
        Ok(Output::Success(fields)) => Value::Object(fields),
        other => panic!("cpd tui {args:?}: {other:?}"),
    }
}

/// `cpd tui action <args>`, which must print nothing, on stdout or stderr, and exit 0 (commands.md §1.2): the command
/// owns its stdout and has none to write.
pub fn action(fx: &Fx, args: &[&str]) {
    let (result, stderr) = fx.with_stderr(|| tui_result(fx, &[&["action"][..], args].concat()));
    assert!(
        matches!(result, Ok(Output::Owned)),
        "cpd tui action {args:?}: {result:?}"
    );
    assert_eq!(stderr, "", "cpd tui action {args:?}");
}
