//! `__ctl`, the test-only control command (`__ctl pane-exit %p --status n`, `__ctl screen %p text`,
//! `__ctl click status <range>`). Stub: the implementing Job fills it in.

/// Runs `__ctl <args>`; it is only accepted as the first argument of a call.
#[allow(dead_code)]
pub fn ctl(_args: &[String]) -> Result<String, String> {
    todo!("__ctl")
}
