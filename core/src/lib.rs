//! cpd-core: the shared core of cpd (everything except the root command tree and the cockpit).
//!
//! The normative documents are `docs/v2/contract.md`, `docs/v2/commands.md`
//! and `docs/v2/CONTEXT.md`; `docs/v2/implementing.md` explains this crate's
//! layout. Every module is declared here once: a Job fills in its own area
//! module and never edits this file.

// Shared foundations (Job rs-workspace).
pub mod args;
pub mod error;
pub mod home;
pub mod ids;
pub mod output;

// One module per area, each filled in by its own Job.
pub mod ask;
pub mod attempt;
pub mod config;
pub mod context;
pub mod doctor;
pub mod driver;
pub mod events;
pub mod git;
pub mod host;
pub mod job;
pub mod merge;
pub mod metrics;
pub mod plan;
pub mod pr;
pub mod session;
pub mod spec;
pub mod store;
pub mod watch;
