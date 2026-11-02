//! The key of a Manager, a Lead or a Worker (pro-d345).

use crate::ids::{ContextId, JobKey, PlanId};

/// The key of a Manager, a Lead or a Worker (pro-d345): the only writer of their `sessions.name`.
///
/// This type is the only writer of the `sessions.name` of a Manager, a Lead and a Worker; there is no SQL CHECK.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentKey {
    Manager(ContextId),
    Lead(ContextId, PlanId),
    Worker(ContextId, PlanId, JobKey),
}

impl AgentKey {
    pub const MAX_LEN: usize = 98;

    pub fn manager(context: ContextId) -> Self {
        Self::Manager(context)
    }

    pub fn lead(context: ContextId, plan: PlanId) -> Self {
        Self::Lead(context, plan)
    }

    pub fn worker(context: ContextId, plan: PlanId, job: JobKey) -> Self {
        Self::Worker(context, plan, job)
    }
}

impl std::fmt::Display for AgentKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manager(c) => write!(f, "{c}"),
            Self::Lead(c, p) => write!(f, "{c}:{p}"),
            Self::Worker(c, p, j) => write!(f, "{c}:{p}:{j}"),
        }
    }
}
