//! The `flow` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod ask;
mod attempt;
mod config;
mod context_delete;
mod context_distill;
mod context_manager;
mod context_pending;
mod context;
// Wrapped so the test names read `flow::driver_idle::...`; `#[path = "."]` keeps the file here, in this directory.
#[path = "."]
mod flow {
    mod driver_idle;
}
mod driver_reload;
mod driver;
mod job;
mod metrics;
mod plan_cancel;
mod plan_delete;
mod plan_outline;
mod plan_overlap;
mod plan;
mod plan_show;
mod review_plannotator;
mod policy;
mod review_job;
mod review;
mod spec;
