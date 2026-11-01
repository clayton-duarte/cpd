//! The Events the tree acts on (commands.md §17): the view takes a new snapshot when one of these came, because the
//! tree shows counts and states that the Events' `data` does not carry. The view follows them in process, with
//! [`crate::read::Follow`]. Nothing else makes it read again but a change of a `pull_requests` row, which has no Event:
//! so every Event that moves a row the snapshot reads is here (tui-F3).

use cpd_core::events::Kind;

/// The kinds the tree acts on: §17's list (the `inbox-*` kinds included: the Inbox says who needs the User and fills
/// the Inbox section), with the Job, Attempt, Escalation, Workflow and Plan review kinds that change a Plan's counters, and
/// the Context records it counts.
pub const KINDS: &[Kind] = &[
    Kind::ContextCreated,
    Kind::ContextGrounded,
    Kind::ContextRecord,
    Kind::PlanCreated,
    Kind::PlanPhase,
    Kind::PlanReview,
    Kind::PlanDone,
    Kind::JobAdded,
    Kind::JobEdited,
    Kind::JobMerged,
    Kind::JobFailed,
    Kind::JobRetried,
    Kind::JobCancelled,
    Kind::AttemptStarted,
    Kind::AttemptRetry,
    Kind::AttemptStatus,
    Kind::AllMerged,
    Kind::WorkflowAdded,
    Kind::WorkflowEdited,
    Kind::WorkflowCancelled,
    Kind::WorkflowStarted,
    Kind::PrOpened,
    Kind::PrMerged,
    Kind::Escalation,
    Kind::EscalationAnswered,
    Kind::EscalationExpired,
    Kind::ReviewOpened,
    Kind::ReviewClosed,
    Kind::SessionRegistered,
    Kind::SessionState,
    Kind::SessionMoved,
    Kind::SessionRenamed,
    Kind::ContextRenamed,
    Kind::PlanRenamed,
    Kind::ContextArchived,
    Kind::PlanArchived,
    Kind::SessionArchived,
    Kind::SessionAsleep,
    Kind::PlanDeleted,
    Kind::ContextDeleted,
    Kind::InboxOpened,
    Kind::InboxClosed,
    Kind::InboxSnoozed,
    Kind::InboxDismissed,
    Kind::InboxClaimed,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_follows_the_kinds_of_17_and_each_once() {
        for kind in [
            Kind::ContextCreated,
            Kind::ContextGrounded,
            Kind::ContextRecord,
            Kind::PlanCreated,
            Kind::AttemptStarted,
            Kind::AttemptRetry,
            Kind::AllMerged,
            Kind::WorkflowAdded,
            Kind::WorkflowEdited,
            Kind::WorkflowCancelled,
            Kind::WorkflowStarted,
            Kind::PrOpened,
            Kind::PrMerged,
            Kind::PlanPhase,
            Kind::PlanDone,
            Kind::SessionRegistered,
            Kind::SessionState,
            Kind::SessionMoved,
            Kind::SessionRenamed,
            Kind::ContextRenamed,
            Kind::PlanRenamed,
            Kind::ContextArchived,
            Kind::PlanArchived,
            Kind::SessionArchived,
            Kind::SessionAsleep,
            Kind::PlanDeleted,
            Kind::ContextDeleted,
            Kind::InboxOpened,
            Kind::InboxClosed,
            Kind::InboxSnoozed,
            Kind::InboxDismissed,
            Kind::InboxClaimed,
        ] {
            assert!(KINDS.contains(&kind), "{kind}");
        }
        for (i, kind) in KINDS.iter().enumerate() {
            assert!(!KINDS[..i].contains(kind), "{kind} twice");
        }
    }
}
