//! cpd-core: the shared core of cpd (everything except the root command tree, the engine and the cockpit).
//!
//! The normative documents are `docs/contract.md`, `docs/commands.md`
//! and `CONTEXT.md`; `docs/implementing.md` explains this crate's
//! layout. Every module is declared here once: a Job fills in its own area
//! module and never edits this file.

// Shared foundations (Job rs-workspace).
pub mod agent_key;
pub mod args;
pub mod atomic;
pub mod clock;
pub mod env;
pub mod error;
pub mod home;
pub mod ids;
pub mod lock;
pub mod output;

// One module per area, each filled in by its own Job.
pub mod ask;
pub mod attempt;
pub mod board;
pub mod config;
pub mod context;
pub mod driver;
pub mod events;
pub mod gate;
pub mod git;
pub mod host;
pub mod host_repair;
pub mod host_repair_approval;
pub mod inbox;
pub mod incident;
pub mod install;
pub mod janitor;
pub mod jira;
pub mod job;
pub mod mcp;
pub mod merge;
pub mod metrics;
pub mod models;
pub mod pill;
pub mod plan;
pub mod policy;
pub mod ports;
pub mod pr;
pub mod prune;
pub mod resource;
pub mod secret;
pub mod session;
pub mod slack;
pub mod spec;
pub mod store;
pub mod tag;
pub mod testsel;
pub mod version_sync;
pub mod workflow;

#[cfg(any(test, feature = "fakes"))]
pub mod fakes;

#[cfg(test)]
mod agent_key_tests;
#[cfg(test)]
mod ambient_ports_tests;
#[cfg(test)]
mod clock_tests;
#[cfg(test)]
mod incident_tests;
#[cfg(test)]
mod janitor_tests;
#[cfg(test)]
mod loops_ports_tests;

#[cfg(test)]
mod pill_tests;
#[cfg(test)]
mod ports_tests;
