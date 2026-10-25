//! E6, the host alert (contract §3.7, §6.1; hs-d618): mirrors the install state file into the tmux
//! global option `@cpd_host_alert` the version pill reads, calling tmux only when the value changed.
//!
//! When the status is `skipped` the option is set to the reason; otherwise, and for a missing file,
//! it is unset with `set -gu`. An invalid file is one `error` Event with code `install_state_invalid`
//! (once until it clears) and leaves the option as it is. The test Job writes the red tests; the
//! engine step that calls these pure functions is not in the tick yet.

use cpd_core::error::CpdError;
use cpd_core::host::install_state::InstallState;

/// The tmux global option that turns the version pill red (contract §6.1).
pub const OPTION: &str = "@cpd_host_alert";

/// The tmux argv that sets [`OPTION`] to `next` (the alerting reason, or `None` to unset it) from
/// `last` (the value the step last wrote, `None` when it last unset the option): `None` when the
/// value is already right, so tmux is not called.
pub fn argv(last: Option<&str>, next: Option<&str>) -> Option<Vec<String>> {
    todo!()
}

/// The call for the state that `read` returned: the [`argv`] of the state's alert, or `None` when the
/// read failed (the caller reports the error and leaves the option as it is).
pub fn call(last: Option<&str>, read: &Result<Option<InstallState>, CpdError>) -> Option<Vec<String>> {
    todo!()
}
