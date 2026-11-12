//! Shell app install for macOS in `cpd install`.
//!
//! Retired (decision pro-d567). CPD.app is retired; `ensure_shell` is a no-op
//! and does not download legacy `app-v*` release assets or touch `/Applications`.

use std::io::Write;

use crate::install::Install;
use crate::ports::Ports;

/// CPD.app is retired (decision pro-d567).
/// This function is kept for backward compatibility as a no-op.
pub fn ensure_shell(_ports: &Ports, _install: &Install, _err: &mut dyn Write) {}
