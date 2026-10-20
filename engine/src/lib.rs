//! cpd-engine: the engine and the health check (docs/implementing.md, Layout).
//!
//! - [`watch`]: `cpd watch`, the engine, one per machine (commands.md §12);
//! - [`doctor`]: `cpd doctor`, the machine's check (commands.md §14.1).
//!
//! It depends on `cpd-core` and uses its public items. `cpd-tui` does not depend on it: `cli` passes the
//! cockpit the engine's tmux restart ([`watch::restart`]).

pub mod doctor;
pub mod watch;
