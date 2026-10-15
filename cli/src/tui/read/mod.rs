//! What the TUI reads (commands.md §17): a typed, read-only model over the
//! store, which the three views and the worker view render.
//!
//! - [`tree()`]: the tree view, Contexts → Plans → Lead Sessions (`context list`, `session list`);
//! - [`flow()`] and [`chat_slot()`]: the plan view, one Plan's Jobs, Attempts, Dependencies,
//!   Escalations, review and PR (`plan show`), and the Plan of the Lead in the chat slot;
//! - [`widgets()`]: the Dispatch column, slots, the engine, PRs and their feedback,
//!   open Escalations and the latest Events (`plan list`);
//! - [`worker()`]: the worker view, an Attempt and its Worker Session (`attempt show`).
//!
//! Each read is one [`Store::read`] transaction (contract §4.2): it takes no
//! lock, waits for no writer, writes nothing, spawns nothing, and runs a fixed
//! number of queries whatever the size of the Ledger (§17 refreshes a view about
//! every second). Each snapshot returns `events_cursor`, the highest Event id of
//! its own transaction, from which the view follows (`cpd2 events follow --since`).
//! Rows are parsed once here: a value the schema forbids is `store_failed`.
//!
//! The functions take the opened [`Store`] and no `CpdHome`: the one thing
//! outside the store a view needs, the config's `concurrency`, is passed in.
//!
//! [`Store`]: crate::store::Store
//! [`Store::read`]: crate::store::Store::read

mod flow;
mod shared;
mod tree;
mod widgets;
mod worker;

pub use flow::{AttemptNode, ChatSlot, Flow, JobNode, OpenEscalation, Review, WorkerRef, chat_slot, flow};
pub use shared::{
    AttemptStatus, Cause, Checks, EscalationStatus, JobCounts, JobStatus, Origin, Outcome, Place, PlanHead, Pr,
    PrState, ReviewStatus, SessionNode,
};
pub use tree::{ContextNode, PlanNode, Tree, tree};
pub use widgets::{
    Engine, HeldSlot, MachineEscalation, PlanCard, PolledPr, PrFeedback, PrRow, ReviewDecision, Slots, Widgets, widgets,
};
pub use worker::{AttemptDetail, AttemptEscalation, Worker, worker};
