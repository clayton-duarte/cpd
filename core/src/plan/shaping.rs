//! The Manager's rights on a Lead-less shaping Plan (commands.md §4.1 "A Manager on a shaping Plan", §1.3; contract
//! §2.2 "Explore Jobs", §3.1 "The Context's Manager"; air-d19, air-d22).
//!
//! A **shaping Plan** is a Lead-less Plan: `lead_session` null, phase `shaping` or `done`. On one of its own Context a
//! Manager names the Plan with `--plan <id>` and runs the explore commands, each refused `plan_has_lead` on a Plan that
//! has a Lead and `plan_other_context` on another Context's Plan. A Lead, a Worker and the User are unrestricted: the
//! commands are the Plan's Lead commands unchanged.

use rusqlite::{Connection, OptionalExtension};

use crate::error::{CpdError, ErrorCode};
use crate::ids::{ContextId, PlanId, SessionId};
use crate::plan::row;

/// Who runs a shaping-Plan command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Caller {
    /// Not a Manager: every command is the Plan's Lead command, unchanged.
    Other,
    /// The Context's live Manager, on a Lead-less shaping Plan of its own Context.
    Shaping,
}

/// The guard of a Manager's shaping command: resolve nothing beyond the caller's role. A caller that is not a Manager
/// is unrestricted; a Manager is refused `plan_has_lead` on a Plan that has a Lead and `plan_other_context` on a Plan of
/// another Context.
pub(crate) fn guard(conn: &Connection, plan: &PlanId) -> Result<Caller, CpdError> {
    let Some((role, context)) = caller(conn)? else {
        return Ok(Caller::Other);
    };
    if role != "manager" {
        return Ok(Caller::Other);
    }
    let plan_row = row::load(conn, plan)?;
    if plan_row.lead_session.is_some() {
        return Err(CpdError::new(
            ErrorCode::PlanHasLead,
            format!("Plan {plan} has a Lead: a Manager never runs a Lead's command on it, and shapes only a Lead-less Plan of its own Context."),
        )
        .with("plan", plan.as_str()));
    }
    if plan_row.context != context {
        return Err(CpdError::new(
            ErrorCode::PlanOtherContext,
            format!("Plan {plan} belongs to Context {}, not {}: a Manager shapes only a Lead-less Plan of its own Context.", plan_row.context, context),
        )
        .with("plan", plan.as_str())
        .with("plan_context", plan_row.context.as_str())
        .with("context", context.as_str()));
    }
    Ok(Caller::Shaping)
}

/// `shaping_explore_only`: a shaping Plan holds only explore Jobs, and no Job of one takes a Workflow. `kind` is the
/// Job kind and `workflow` the `--workflow` flag, when given.
pub(crate) fn explore_only(
    plan: &PlanId,
    kind: &str,
    workflow: Option<&str>,
) -> Result<(), CpdError> {
    if !crate::plan::jobs::is_explore(kind) {
        return Err(CpdError::new(
            ErrorCode::ShapingExploreOnly,
            format!("Plan {plan} is a shaping Plan: it holds only scout and research Jobs, so --kind {kind} belongs to a Lead's Plan."),
        )
        .with("plan", plan.as_str())
        .with("kind", kind));
    }
    if let Some(workflow) = workflow {
        return Err(CpdError::new(
            ErrorCode::ShapingExploreOnly,
            format!("Plan {plan} is a shaping Plan: its explore Jobs belong to the Plan, so drop --workflow {workflow}."),
        )
        .with("plan", plan.as_str())
        .with("workflow", workflow));
    }
    Ok(())
}

/// The caller's role and Context, from `$CPD_SESSION`: a live Session, else none (the User at a terminal). A
/// `CPD_ATTEMPT` makes the caller a Worker, never a Manager.
pub(crate) fn caller(conn: &Connection) -> Result<Option<(String, ContextId)>, CpdError> {
    if std::env::var("CPD_ATTEMPT")
        .ok()
        .is_some_and(|v| !v.is_empty())
    {
        return Ok(None);
    }
    let Some(raw) = std::env::var("CPD_SESSION").ok().filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let Ok(id) = SessionId::parse(&raw) else {
        return Ok(None);
    };
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT role, context FROM sessions WHERE id = ?1 AND archived_at IS NULL",
            [id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((role, context)) = row else {
        return Ok(None);
    };
    Ok(Some((role, ContextId::parse(&context)?)))
}
