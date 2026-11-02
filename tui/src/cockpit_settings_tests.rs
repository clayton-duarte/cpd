//! `cpd tui action settings` (the StatusBar's settings button): the action opens a tmux popup on the clicking client
//! that runs `cpd tui view settings`, as `legend` and `logs` do. Driven through `cockpit::action` on the fake tmux
//! route of a `FakeRunner`; no real tmux.

use std::ffi::OsString;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use cpd_core::clock::ManualClock;
use cpd_core::error::CpdError;
use cpd_core::fakes::tmux::TmuxRoute;
use cpd_core::host::runner::{Call, FakeRunner, Reply};
use cpd_core::output::Output;
use cpd_core::ports::{Fakes, Ports};

use crate::cockpit;

const CLIENT: &str = "/dev/pts/4";

/// `$CPD_HOME` is the process's: one test holds it at a time, and it is put back on drop.
static ENV: Mutex<()> = Mutex::new(());

struct Home {
    _dir: tempfile::TempDir,
    previous: Option<OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl Home {
    fn new() -> Home {
        let lock = ENV.lock().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("CPD_HOME");
        // SAFETY: `ENV` serialises the tests of this file, which are the only ones that set it.
        unsafe { std::env::set_var("CPD_HOME", dir.path()) };
        Home {
            _dir: dir,
            previous,
            _lock: lock,
        }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        // SAFETY: as in `Home::new`, under the same lock.
        unsafe {
            match &self.previous {
                Some(v) => std::env::set_var("CPD_HOME", v),
                None => std::env::remove_var("CPD_HOME"),
            }
        }
    }
}

/// Ports whose tmux answers the client's size (120 x 30) and takes every popup and message.
fn ports() -> (Ports, Arc<FakeRunner>) {
    let route = TmuxRoute::new()
        .script(
            |argv| argv.iter().any(|a| a == "display"),
            Reply::ok("120 30\n"),
        )
        .script(|_| true, Reply::ok(""));
    let fakes = Fakes::new();
    let runner = Arc::new(FakeRunner::new(ManualClock::new()).route("tmux", Arc::new(route)));
    let ports = Ports {
        runner: runner.clone(),
        ..fakes.ports.clone()
    };
    (ports, runner)
}

fn run_command(_: Vec<OsString>) -> Result<Output, CpdError> {
    Ok(Output::Owned)
}

fn run_on(_: &Ports, _: Vec<OsString>) -> Result<Output, CpdError> {
    Ok(Output::Owned)
}

fn act(ports: &Ports, name: &str) {
    let out = cockpit::action(ports, Some(name), None, CLIENT, run_command, run_on);
    assert!(matches!(out, Output::Owned));
}

fn calls_of(runner: &FakeRunner, tmux_command: &str) -> Vec<Call> {
    runner
        .calls()
        .into_iter()
        .filter(|c| c.args.iter().any(|a| a == tmux_command))
        .collect()
}

#[test]
fn the_settings_action_opens_a_popup_running_the_settings_view_on_the_clicking_client() {
    let _home = Home::new();
    let (ports, runner) = ports();
    act(&ports, "settings");
    let popups = calls_of(&runner, "display-popup");
    assert_eq!(
        popups.len(),
        1,
        "one popup; messages: {:?}",
        calls_of(&runner, "display-message")
    );
    let args = &popups[0].args;
    let at = args.iter().position(|a| a == "display-popup").unwrap();
    assert_eq!(args[at + 1..at + 3], ["-c", CLIENT], "{args:?}");
    assert!(
        args.last().unwrap().contains("cpd tui view settings"),
        "{args:?}"
    );
}

#[test]
fn an_unknown_action_says_settings_is_one_of_the_actions() {
    let _home = Home::new();
    let (ports, runner) = ports();
    act(&ports, "nope");
    let messages = calls_of(&runner, "display-message");
    assert_eq!(messages.len(), 1, "{messages:?}");
    let text = messages[0].args.last().unwrap();
    assert!(text.contains("unknown action"), "{text}");
    assert!(text.contains("settings"), "{text}");
}
