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
use cpd_core::error::CpdError;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_tui::{Restart, RestartOutcome};
use serde_json::{Value, json};

use super::fx::Fx;

thread_local! {
    static RAN: RefCell<Vec<Vec<String>>> = const { RefCell::new(Vec::new()) };
    static ENGINE_STARTED: Cell<bool> = const { Cell::new(false) };
}

/// The tmux restart of an engine that is never reached from the cockpit's own server.
const NO_RESTART: Restart = Restart {
    failed: |_| Ok(None),
    clear: |_| Ok(()),
    mark: |_, _, _| Ok(false),
    now: |_, _, _| Ok(RestartOutcome::NotNeeded),
};

fn run_command(argv: Vec<OsString>) -> Result<Output, CpdError> {
    run_command_on(&Ports::current(), argv)
}

fn run_command_on(ports: &Ports, argv: Vec<OsString>) -> Result<Output, CpdError> {
    let words: Vec<String> = argv.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    RAN.with(|ran| ran.borrow_mut().push(words.clone()));
    if words.iter().map(String::as_str).collect::<Vec<_>>() == ["cpd", "watch", "--ensure"] {
        let first = !ENGINE_STARTED.replace(true);
        return Ok(Output::success(if first { json!({"started": true, "pane": "%99"}) } else { json!({"started": false, "already": true}) }));
    }
    cpd::cli::run_with(ports, argv)
}

/// The commands the cockpit ran through its `RunCommand` in this test, each as its `argv` (`cpd` first).
pub fn ran() -> Vec<Vec<String>> {
    RAN.with(|ran| ran.borrow().clone())
}

/// `cpd tui <args>` as the command tree parses it, run on the ports of `fx`; its result.
pub fn tui_result(fx: &Fx, args: &[&str]) -> Result<Output, CpdError> {
    let argv = ["cpd", "tui"].iter().chain(args).map(OsString::from);
    let Cli { command: Some(Command::Tui(args)) } = Cli::try_parse_from(argv).expect("cpd tui parses") else { unreachable!("cpd tui is the Tui command") };
    Ports::scope(&fx.fakes.ports, || cpd_tui::run_with(&fx.fakes.ports, args, cpd::cli::version(), run_command, run_command_on, NO_RESTART))
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
    assert!(matches!(result, Ok(Output::Owned)), "cpd tui action {args:?}: {result:?}");
    assert_eq!(stderr, "", "cpd tui action {args:?}");
}
