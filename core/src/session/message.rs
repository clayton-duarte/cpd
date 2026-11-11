//! Message bus: delegate and escalate (contract §5.9, pro-d533, pro-d534).

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::row::Role;
use crate::error::{CpdError, ErrorCode};
use crate::ids::SessionId;

/// Urgency of a bus message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Urgency {
    Fyi,
    Wake,
}

/// Kind of a bus message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    Delegate,
    Escalate,
    Notice,
}

/// A parsed delegate request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegateParams {
    pub to: Vec<SessionId>,
    pub urgency: Option<Urgency>,
    pub text: String,
    pub reply_to: Option<i64>,
}

/// A parsed escalate request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscalateParams {
    pub urgency: Option<Urgency>,
    pub text: String,
    pub rationale: Option<String>,
}

/// Validates delegate direction:
/// - Manager to Leads of its Context's Plans
/// - Lead to Workers of its Plan
/// - Worker has no delegate (`not_permitted`)
pub fn check_delegate(
    conn: &Connection,
    sender_session: &SessionId,
    sender_role: Role,
    to: &[SessionId],
) -> Result<(), CpdError> {
    if to.is_empty() {
        return Err(CpdError::new(ErrorCode::Usage, "delegate requires non-empty 'to' array."));
    }
    match sender_role {
        Role::Worker => Err(CpdError::new(
            ErrorCode::NotPermitted,
            "A Worker cannot delegate (not_permitted).",
        )),
        Role::Lead => {
            // Lead delegates only to Workers of its Plan
            let plan: Option<String> = conn
                .query_row(
                    "SELECT plan FROM sessions WHERE id = ?1",
                    [sender_session.as_str()],
                    |r| r.get(0),
                )
                .map_err(|e| CpdError::new(ErrorCode::StoreFailed, e.to_string()))?;
            let Some(plan) = plan else {
                return Err(CpdError::new(
                    ErrorCode::NotPermitted,
                    "Lead has no plan associated.",
                ));
            };
            for recipient in to {
                let allowed: bool = conn
                    .query_row(
                        "SELECT EXISTS (SELECT 1 FROM sessions WHERE id = ?1 AND role = 'worker' AND plan = ?2)",
                        rusqlite::params![recipient.as_str(), plan],
                        |r| r.get(0),
                    )
                    .unwrap_or(false);
                if !allowed {
                    return Err(CpdError::new(
                        ErrorCode::NotPermitted,
                        format!("Recipient {recipient} is not a Worker of Plan {plan}."),
                    ));
                }
            }
            Ok(())
        }
        Role::Manager => {
            // Manager delegates only to Leads of its Context's Plans
            let context: Option<String> = conn
                .query_row(
                    "SELECT context FROM sessions WHERE id = ?1",
                    [sender_session.as_str()],
                    |r| r.get(0),
                )
                .map_err(|e| CpdError::new(ErrorCode::StoreFailed, e.to_string()))?;
            let Some(context) = context else {
                return Err(CpdError::new(
                    ErrorCode::NotPermitted,
                    "Manager has no context associated.",
                ));
            };
            for recipient in to {
                let allowed: bool = conn
                    .query_row(
                        "SELECT EXISTS (
                            SELECT 1 FROM sessions s
                            JOIN plans p ON p.id = s.plan
                            WHERE s.id = ?1 AND s.role = 'lead' AND p.context = ?2
                        )",
                        rusqlite::params![recipient.as_str(), context],
                        |r| r.get(0),
                    )
                    .unwrap_or(false);
                if !allowed {
                    return Err(CpdError::new(
                        ErrorCode::NotPermitted,
                        format!("Recipient {recipient} is not a Lead in Context {context}."),
                    ));
                }
            }
            Ok(())
        }
    }
}

/// Resolves the parent for escalation:
/// - Worker -> Lead of its Plan
/// - Lead -> Manager of its Context
/// - Manager -> User (None)
pub fn resolve_escalation_target(
    conn: &Connection,
    sender_session: &SessionId,
    sender_role: Role,
) -> Result<Option<SessionId>, CpdError> {
    match sender_role {
        Role::Worker => {
            let plan: Option<String> = conn
                .query_row(
                    "SELECT plan FROM sessions WHERE id = ?1",
                    [sender_session.as_str()],
                    |r| r.get(0),
                )
                .map_err(|e| CpdError::new(ErrorCode::StoreFailed, e.to_string()))?;
            let Some(plan) = plan else {
                return Err(CpdError::new(
                    ErrorCode::Internal,
                    "Worker has no plan associated.",
                ));
            };
            let lead_id: String = conn
                .query_row(
                    "SELECT id FROM sessions WHERE role = 'lead' AND plan = ?1 AND archived_at IS NULL ORDER BY created_at DESC LIMIT 1",
                    [plan],
                    |r| r.get(0),
                )
                .map_err(|_| CpdError::new(ErrorCode::SessionNotFound, "No active Lead found for Worker's plan."))?;
            Ok(Some(SessionId::parse(&lead_id)?))
        }
        Role::Lead => {
            let context: Option<String> = conn
                .query_row(
                    "SELECT p.context FROM sessions s JOIN plans p ON p.id = s.plan WHERE s.id = ?1",
                    [sender_session.as_str()],
                    |r| r.get(0),
                )
                .map_err(|e| CpdError::new(ErrorCode::StoreFailed, e.to_string()))?;
            let Some(context) = context else {
                return Err(CpdError::new(
                    ErrorCode::Internal,
                    "Lead has no context associated.",
                ));
            };
            let mgr_id: String = conn
                .query_row(
                    "SELECT id FROM sessions WHERE role = 'manager' AND context = ?1 AND archived_at IS NULL ORDER BY created_at DESC LIMIT 1",
                    [context],
                    |r| r.get(0),
                )
                .map_err(|_| CpdError::new(ErrorCode::NoManager, "No active Manager found for Lead's context."))?;
            Ok(Some(SessionId::parse(&mgr_id)?))
        }
        Role::Manager => {
            // Reaches User
            Ok(None)
        }
    }
}
