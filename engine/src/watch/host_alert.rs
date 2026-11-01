//! E6, the host alert (contract §3.7, §6.1; hs-d618): mirrors the install state file into the tmux
//! global option `@cpd_host_alert` the version pill reads, calling tmux only when the value changed.
//!
//! When the status is `skipped` the option is set to the reason; otherwise, and for a missing file,
//! it is unset with `set -gu`. An invalid file is one `error` Event with code `install_state_invalid`
//! (once until it clears) and leaves the option as it is.

use super::engine::Engine;
use super::report::{About, Step};
use super::restart;
use cpd_core::error::CpdError;
use cpd_core::host::install_state::InstallState;

/// The tmux global option that turns the version pill red (contract §6.1).
pub const OPTION: &str = "@cpd_host_alert";

/// The tmux argv that sets [`OPTION`] to `next` (the alerting reason, or `None` to unset it) from
/// `last` (the value the step last wrote, `None` when it last unset the option): `None` when the
/// value is already right, so tmux is not called.
pub fn argv(last: Option<&str>, next: Option<&str>) -> Option<Vec<String>> {
    if last == next {
        return None;
    }
    Some(match next {
        Some(reason) => vec![
            "set".to_owned(),
            "-g".to_owned(),
            OPTION.to_owned(),
            reason.to_owned(),
        ],
        None => vec!["set".to_owned(), "-gu".to_owned(), OPTION.to_owned()],
    })
}

/// The call for the state that `read` returned: the [`argv`] of the state's alert, or `None` when the
/// read failed (the caller reports the error and leaves the option as it is).
pub fn call(
    last: Option<&str>,
    read: &Result<Option<InstallState>, CpdError>,
) -> Option<Vec<String>> {
    let next = read.as_ref().ok()?.as_ref().and_then(InstallState::alert);
    argv(last, next)
}

pub(super) fn run(engine: &mut Engine) {
    let read = InstallState::read(&engine.home);
    let next = match &read {
        Ok(state) => state.as_ref().and_then(InstallState::alert),
        // An invalid file is one error Event and leaves the option (and what was last set) as it is.
        Err(e) => return engine.fail(Step::HostAlert, About::Machine, e),
    };
    let Some(args) = argv(engine.host_alert.as_deref(), next) else {
        return engine.clear(Step::HostAlert, &About::Machine);
    };
    // No server, no status line: nothing is set or remembered, and it is set once a server appears.
    match engine.tmux.server_running() {
        Ok(true) => {}
        Ok(false) => {
            engine.host_alert = None;
            return engine.clear(Step::HostAlert, &About::Machine);
        }
        Err(e) => return failed(engine, &e),
    }
    match engine.tmux.run(&args) {
        Ok(_) => {
            engine.host_alert = next.map(str::to_owned);
            engine.clear(Step::HostAlert, &About::Machine);
        }
        Err(e) => failed(engine, &e),
    }
}

/// A tmux call of the step failed: nothing is remembered (a restarted server starts without the
/// option), and the next tick tries again. While a tmux restart is pending and has not failed, the
/// failure is expected, the restart takes the server away under the step, and is not reported.
fn failed(engine: &mut Engine, error: &CpdError) {
    engine.host_alert = None;
    if restart::holding(&engine.home).unwrap_or(false) {
        return;
    }
    engine.fail(Step::HostAlert, About::Machine, error);
}
