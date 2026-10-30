//! What the TUI reads (commands.md §17): a typed, read-only model over the
//! store, which the views render.
//!
//! - [`tree()`]: the tree view, Contexts → Plans → Lead Sessions (`context list`, `session list`), and the
//!   Inbox's visible items, from which every dot of the tree comes (`inbox list`);
//! - [`plan_boxes()`] and [`chat_slot()`]: the plan view, one Plan's PRs with their Jobs, Attempts and
//!   Dependencies, and the Plan of the Lead in the chat slot; [`flow()`]: one Plan's Jobs, Escalations,
//!   review and first PR (`plan show`);
//! - [`pulse()`] and [`manager_slot()`]: the Pulse view, one Context's Plans with their 5-minute activity;
//! - [`widgets()`]: slots, the engine, the Plans, open Escalations and the latest Events
//!   (`plan list`); the views read only what they draw: [`inbox_at()`] (the right column) and [`slots()`] (the StatusBar);
//! - [`logs()`] and [`logs_after()`]: the Event log, the latest Events and those after a cursor (`events list`);
//!
//! Each read is one [`Store::read`] transaction (contract §4.2): it takes no
//! lock, waits for no writer, writes nothing, spawns nothing, and runs a fixed
//! number of queries whatever the size of the Ledger (§17 refreshes a view about
//! every second). Each snapshot returns `events_cursor`, the highest Event id of
//! its own transaction, from which the view follows ([`Follow`], in process).
//! Rows are parsed once here: a value the schema forbids is `store_failed`.
//!
//! The functions take the opened [`Store`] and no `CpdHome`: the one thing
//! outside the store a view needs, the config's `concurrency`, is passed in.
//!
//! [`Store`]: cpd_core::store::Store
//! [`Store::read`]: cpd_core::store::Store::read

mod active;
mod age;
mod cost;
#[cfg(test)]
mod cost_tests;
#[cfg(test)]
mod age_tests;
#[cfg(test)]
mod inbox_mark_tests;
mod dot;
#[cfg(test)]
mod dot_pr_tests;
mod flow;
mod follow;
mod logs;
#[cfg(test)]
mod elapsed_tests;
#[cfg(test)]
mod explore_attempt_tests;
#[cfg(test)]
mod workflow_cost_tests;
#[cfg(test)]
mod look_tests;
#[cfg(test)]
mod lead_dot_box_tests;
pub mod pr_look;
mod pulse;
#[cfg(test)]
mod pulse_tests;
mod shared;
#[cfg(test)]
pub(crate) mod stacked_ready_tests;
mod tree;
mod widgets;
pub mod watch;
#[cfg(test)]
mod watch_tests;

pub use age::age_text;
pub use flow::{
    AttemptNode, ChatSlot, Flow, JobNode, NeedsUser, OpenEscalation, PlanBoxes, PrNode, PrStatus, Pull, Review, UpdateState, WorkerRef, chat_slot, flow,
    plan_boxes, plan_boxes_at,
};
pub use shared::{
    AttemptStatus, Checks, JobCounts, JobStatus, Origin, Outcome, Place, PlanHead, Pr,
    PrState, ReviewStatus, SessionNode,
};
pub use dot::{Dot, OrderKey, Work};
pub use pr_look::{PrFacts, PrLook, pr_look};
pub use follow::{Changes, Follow};
pub use logs::{Logs, logs, logs_after};
pub use pulse::{BUCKET_MINUTES, Bucket, Kind, ManagerSlot, Pulse, PulseRow, QUIET_MINUTES, manager_slot, pulse};
pub use tree::{ContextNode, PlanNode, Tree, InboxMark, inbox_at, inbox_mark, snooze_end, snooze_end_at, tree, tree_at};
pub use widgets::{
    Engine, HeldSlot, MachineEscalation, PlanCard, PolledPr, PrRow, ReviewDecision, Slots, Widgets, open_pulls, slots, widgets,
};
