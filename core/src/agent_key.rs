//! The key of a Manager, a Lead or a Worker (pro-d345).

use crate::ids::{ContextId, JobKey, PlanId};

/// The key of a Manager, a Lead or a Worker (pro-d345): the only writer of their `sessions.name`.
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
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}
