//! What the TUI reads (commands.md §17): a typed, read-only model over the
//! store, which the views render.
//!
//! - [`tree()`]: the tree view, Contexts → Plans → Lead Sessions (`context list`, `session list`), and the
//!   Inbox's visible items, from which every dot of the tree comes (`inbox list`);
//! - [`plan_boxes()`] and [`chat_slot()`]: the plan view, one Plan's PRs with their Jobs, Attempts and
//!   Dependencies, and the Plan of the Lead in the chat slot; [`flow()`]: one Plan's Jobs, Escalations,
//!   review and first PR (`plan show`);
//! - [`pulse()`] and [`manager_slot()`]: the Pulse view, one Context's Plans with their 5-minute activity;
//! - [`widgets()`]: slots, the engine, the Plans, the open pull requests, open Escalations and the latest Events
//!   (`plan list`); the views read only what they draw: [`open_prs()`] (the Dispatch column) and [`slots()`] (the StatusBar);
//! - [`logs()`] and [`logs_after()`]: the Event log, the latest Events and those after a cursor (`events list`);
//! - [`worker()`]: the worker view, an Attempt or a Job, and its Worker Session (`attempt show`); [`latest_attempt()`]: the Attempt
//!   a Job's Worker pane opens (the tree's `job-stuck` click).
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

mod age;
#[cfg(test)]
mod age_tests;
mod dot;
mod flow;
mod follow;
mod logs;
#[cfg(test)]
mod look_tests;
pub mod pr_look;
mod pulse;
#[cfg(test)]
mod pulse_tests;
mod shared;
mod tree;
mod widgets;
pub mod watch;
#[cfg(test)]
mod watch_tests;
mod worker;

pub use age::age_text;
pub use flow::{
    AttemptNode, ChatSlot, Flow, JobNode, NeedsUser, OpenEscalation, PlanBoxes, PrNode, PrStatus, Pull, Review, UpdateState, WorkerRef, chat_slot, flow,
    plan_boxes,
};
pub use shared::{
    AttemptStatus, Checks, EscalationStatus, JobCounts, JobStatus, Origin, Outcome, Place, PlanHead, Pr,
    PrState, ReviewStatus, SessionNode,
};
pub use dot::{Dot, OrderKey};
pub use pr_look::{PrFacts, PrLook, pr_look};
pub use follow::{Changes, Follow};
pub use logs::{Logs, logs, logs_after};
pub use pulse::{BUCKET_MINUTES, Bucket, Kind, ManagerSlot, Pulse, PulseRow, QUIET_MINUTES, manager_slot, pulse};
pub use tree::{ContextNode, PlanNode, Tree, inbox_at, snooze_end, snooze_end_at, tree, tree_at};
pub use widgets::{
    Engine, HeldSlot, MachineEscalation, OpenPr, PlanCard, PolledPr, PrRow, ReviewDecision, Slots, Widgets, open_prs, open_pulls, slots, widgets,
};
pub use worker::{AttemptDetail, AttemptEscalation, Target, Worker, WorkerJob, latest_attempt, worker};
