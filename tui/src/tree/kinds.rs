//! The Events the tree acts on (commands.md §17): the view takes a new snapshot when one of these came, because the
//! tree shows counts and states that the Events' `data` does not carry. The view follows them in process, with
//! [`crate::read::Follow`].

use cpd_core::events::Kind;

/// The kinds the tree acts on: §17's list, with the Job, Attempt, Escalation and Plan review kinds that
/// change a Plan's counters and whether it needs the User.
pub const KINDS: &[Kind] = &[
    Kind::ContextCreated,
    Kind::ContextGrounded,
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
    Kind::AttemptStatus,
    Kind::Escalation,
    Kind::EscalationAnswered,
    Kind::EscalationExpired,
    Kind::SessionRegistered,
    Kind::SessionState,
    Kind::SessionMoved,
    Kind::SessionRenamed,
    Kind::ContextRenamed,
    Kind::PlanRenamed,
    Kind::ContextArchived,
    Kind::PlanArchived,
    Kind::SessionArchived,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_follows_the_kinds_of_17_and_each_once() {
        for kind in [
            Kind::ContextCreated,
            Kind::ContextGrounded,
            Kind::PlanCreated,
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
        ] {
            assert!(KINDS.contains(&kind), "{kind}");
        }
        for (i, kind) in KINDS.iter().enumerate() {
            assert!(!KINDS[..i].contains(kind), "{kind} twice");
        }
    }
}
