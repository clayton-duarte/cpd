//! How cpd starts pi (contract §6.5, with commands.md §19 amendment 8), one
//! module per concern, all re-exported here:
//!
//! - [`dirs`]: pi's settings and session directories, and what tells that its
//!   install changed;
//! - [`launch`]: the argv and environment per role;
//! - [`packages`]: package identity and the `[-e …]` rule that skips packages
//!   pi's settings already declare;
//! - [`session_file`]: a Session's file after start.

mod dirs;
mod launch;
mod packages;
mod session_file;

pub use dirs::{PiDirs, project_settings};
pub use launch::{
    Launch, LeadLaunch, LeadStart, WORKER_EXCLUDED_TOOLS, WorkerLaunch, distiller, lead,
    lead_candidates, worker, worker_candidates, worker_env,
};
pub use packages::declared;
pub use packages::{PackageId, extensions_to_pass, package_identity};
pub use session_file::session_file;
