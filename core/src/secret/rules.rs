//! The rules of commands.md §24.0 / contract §8.4, in their order, as one pure function.

use crate::error::ErrorCode;
use crate::session::Role;

/// The vaults every caller may use (contract §8.4 Constants).
pub const VAULTS: [&str; 2] = ["cpd", "homeserver"];

/// What `cpd secret` runs on `op`.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Get,
    List,
    Create,
    Edit,
    Delete,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Get => "get",
            Op::List => "list",
            Op::Create => "create",
            Op::Edit => "edit",
            Op::Delete => "delete",
        }
    }
}

/// Every `cpd secret` command the rules decide on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Op(Op),
    Allow,
    Approve,
}

/// The grant the caller must hold after the vault check (rule 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grant {
    /// none: go to op (or write the grant, for `allow` and `approve`)
    None,
    /// a Worker's allowance for its Attempt
    Allowance,
    /// the User's approval for the caller's Session
    Approval,
}

/// Rules 2 to 4 for `role` running `action` in `vault`: the grant needed, or the refusal's code
/// (`secret_role_denied`, `secret_vault_denied`, `usage` for `approve` of an allowed vault).
pub fn check(role: Role, action: Action, vault: &str) -> Result<Grant, ErrorCode> {
    let role_ok = match action {
        Action::Op(Op::Get) => true,
        Action::Op(_) | Action::Approve => role != Role::Worker,
        Action::Allow => role == Role::Lead,
    };
    if !role_ok {
        return Err(ErrorCode::SecretRoleDenied);
    }
    if VAULTS.contains(&vault) {
        return match (action, role) {
            (Action::Approve, _) => Err(ErrorCode::Usage),
            (Action::Op(Op::Get), Role::Worker) => Ok(Grant::Allowance),
            _ => Ok(Grant::None),
        };
    }
    match (action, role) {
        (Action::Approve, _) => Ok(Grant::None),
        (Action::Allow, _) | (_, Role::Worker) => Err(ErrorCode::SecretVaultDenied),
        _ => Ok(Grant::Approval),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::{Allow, Approve};

    const OPS: [Op; 5] = [Op::Get, Op::List, Op::Create, Op::Edit, Op::Delete];

    #[test]
    fn the_role_comes_first() {
        for vault in ["cpd", "other"] {
            for op in [Op::List, Op::Create, Op::Edit, Op::Delete] {
                assert_eq!(
                    check(Role::Worker, Action::Op(op), vault),
                    Err(ErrorCode::SecretRoleDenied)
                );
            }
            assert_eq!(
                check(Role::Manager, Allow, vault),
                Err(ErrorCode::SecretRoleDenied)
            );
            assert_eq!(
                check(Role::Worker, Allow, vault),
                Err(ErrorCode::SecretRoleDenied)
            );
            assert_eq!(
                check(Role::Worker, Approve, vault),
                Err(ErrorCode::SecretRoleDenied)
            );
        }
    }

    #[test]
    fn a_worker_reads_the_allowed_vaults_under_an_allowance() {
        for vault in VAULTS {
            assert_eq!(
                check(Role::Worker, Action::Op(Op::Get), vault),
                Ok(Grant::Allowance)
            );
        }
        assert_eq!(
            check(Role::Worker, Action::Op(Op::Get), "other"),
            Err(ErrorCode::SecretVaultDenied)
        );
    }

    #[test]
    fn a_lead_or_manager_needs_an_approval_only_in_another_vault() {
        for role in [Role::Lead, Role::Manager] {
            for op in OPS {
                assert_eq!(check(role, Action::Op(op), "homeserver"), Ok(Grant::None));
                assert_eq!(check(role, Action::Op(op), "other"), Ok(Grant::Approval));
            }
            assert_eq!(check(role, Approve, "other"), Ok(Grant::None));
            assert_eq!(check(role, Approve, "cpd"), Err(ErrorCode::Usage));
        }
    }

    #[test]
    fn allow_is_never_for_another_vault() {
        assert_eq!(check(Role::Lead, Allow, "cpd"), Ok(Grant::None));
        assert_eq!(
            check(Role::Lead, Allow, "other"),
            Err(ErrorCode::SecretVaultDenied)
        );
    }
}
