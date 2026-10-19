//! What the TUI reads (commands.md §17): a typed, read-only model over the
//! store, which the views render.
//!
//! - [`tree()`]: the tree view, Contexts → Plans → Lead Sessions (`context list`, `session list`);
//! - [`plan_boxes()`] and [`chat_slot()`]: the plan view, one Plan's PRs with their Jobs, Attempts and
//!   Dependencies, and the Plan of the Lead in the chat slot; [`flow()`]: one Plan's Jobs, Escalations,
//!   review and first PR (`plan show`);
//! - [`pulse()`] and [`manager_slot()`]: the Pulse view, one Context's Plans with their 5-minute activity;
//! - [`widgets()`]: the Dispatch column, slots, the engine, the open pull requests,
//!   open Escalations and the latest Events (`plan list`);
//! - [`logs()`] and [`logs_after()`]: the Event log, the latest Events and those after a cursor (`events list`);
//! - [`worker()`]: the worker view, an Attempt and its Worker Session (`attempt show`).
//!
//! Each read is one [`Store::read`] transaction (contract §4.2): it takes no
//! lock, waits for no writer, writes nothing, spawns nothing, and runs a fixed
//! number of queries whatever the size of the Ledger (§17 refreshes a view about
//! every second). Each snapshot returns `events_cursor`, the highest Event id of
//! its own transaction, from which the view follows (`cpd events follow --since`).
//! Rows are parsed once here: a value the schema forbids is `store_failed`.
//!
//! The functions take the opened [`Store`] and no `CpdHome`: the one thing
//! outside the store a view needs, the config's `concurrency`, is passed in.
//!
//! [`Store`]: cpd_core::store::Store
//! [`Store::read`]: cpd_core::store::Store::read

mod dot;
mod flow;
mod logs;
mod pulse;
mod shared;
mod tree;
mod widgets;
mod worker;

pub use flow::{
    AttemptNode, ChatSlot, Flow, JobNode, OpenEscalation, PlanBoxes, PrNode, PrStatus, Pull, Review, WorkerRef, chat_slot, flow,
    plan_boxes,
};
pub use shared::{
    AttemptStatus, Cause, Checks, EscalationStatus, JobCounts, JobStatus, Origin, Outcome, Place, PlanHead, Pr,
    PrState, ReviewStatus, SessionNode,
};
pub use dot::{Dot, DotInputs};
pub use logs::{Logs, logs, logs_after};
pub use pulse::{BUCKET_MINUTES, Bucket, ManagerSlot, Pulse, PulseRow, QUIET_MINUTES, manager_slot, pulse};
pub use tree::{ContextNode, PlanNode, Tree, tree};
pub use widgets::{
    Engine, HeldSlot, MachineEscalation, OpenPr, PlanCard, PolledPr, PrRow, ReviewDecision, Slots, Widgets, widgets,
};
pub use worker::{AttemptDetail, AttemptEscalation, Worker, worker};
