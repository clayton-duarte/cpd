//! `cpd secret` (commands.md §24, contract §8.4): who may do what, in which vault, and the grants they need,
//! then `op`.
//!
//! - [`rules`]: the rules in their order, one pure function;
//! - [`grants`]: the files under `$CPD_HOME/run/secret/`;
//! - [`op`]: the `op` calls.
//!
//! No command writes the store, and no value is in an error, a log or a file.

pub(crate) mod grants;
pub mod op;
pub mod rules;

#[cfg(test)]
mod runner_tests;

use std::fmt;
use std::io::{IsTerminal, Read};
use std::str::FromStr;

use clap::Args;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use self::grants::{Allowed, Approval};
use self::rules::{Action, Grant, Op};
use crate::args::SessionRef;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{AttemptId, SessionId};
use crate::output::Output;
use crate::session::{self, Role};

/// A vault, item or field name: 1 to 200 characters, no control character, no leading `-`; else `usage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretName(String);

impl SecretName {
    pub const MAX_CHARS: usize = 200;

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for SecretName {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<SecretName, CpdError> {
        let chars = s.chars().count();
        if chars == 0
            || chars > SecretName::MAX_CHARS
            || s.chars().any(char::is_control)
            || s.starts_with('-')
        {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!(
                    "A name must be 1 to {} characters, with no control character and no leading `-`.",
                    SecretName::MAX_CHARS
                ),
            ));
        }
        Ok(SecretName(s.to_owned()))
    }
}

impl fmt::Display for SecretName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The `secret` commands (commands.md §24).
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Get one field of an item (§24.1)
    Get(FieldArgs),
    /// List the items of a vault (§24.2)
    List(ListArgs),
    /// Create an item with one concealed field; the value is on stdin (§24.3)
    Create(FieldArgs),
    /// Set one field of an item; the value is on stdin (§24.4)
    Edit(FieldArgs),
    /// Archive an item (§24.5)
    Delete(ItemArgs),
    /// Let a Worker's Attempt read one field of one item (§24.6)
    Allow(AllowArgs),
    /// Record the User's approval for another vault (adapter) (§24.7)
    #[command(hide = true)]
    Approve(ApproveArgs),
}

#[derive(Args, Debug)]
pub struct FieldArgs {
    /// The item
    #[arg(value_name = "item")]
    item: SecretName,
    /// The vault
    #[arg(long, value_name = "v")]
    vault: SecretName,
    /// The field label
    #[arg(long, value_name = "f")]
    field: SecretName,
}

#[derive(Args, Debug)]
pub struct ItemArgs {
    /// The item
    #[arg(value_name = "item")]
    item: SecretName,
    /// The vault
    #[arg(long, value_name = "v")]
    vault: SecretName,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// The vault
    #[arg(long, value_name = "v")]
    vault: SecretName,
}

#[derive(Args, Debug)]
pub struct AllowArgs {
    /// The item
    #[arg(value_name = "item")]
    item: SecretName,
    /// The Attempt, of the Lead's own Plan
    #[arg(long, value_name = "id")]
    attempt: AttemptId,
    /// The vault: `cpd` or `homeserver`
    #[arg(long, value_name = "v")]
    vault: SecretName,
    /// The field label
    #[arg(long, value_name = "f")]
    field: SecretName,
}

#[derive(Args, Debug)]
pub struct ApproveArgs {
    /// The action approved
    #[arg(long, value_name = "action")]
    action: Op,
    /// The vault: not `cpd` or `homeserver`
    #[arg(long, value_name = "v")]
    vault: SecretName,
    /// The item (every action but `list`)
    #[arg(long, value_name = "i")]
    item: Option<SecretName>,
    /// The field label (`get`, `create` and `edit`)
    #[arg(long, value_name = "f")]
    field: Option<SecretName>,
}

/// Runs a `secret` command.
pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Get(a) => operate(Op::Get, &a.vault, Some(&a.item), Some(&a.field)),
        Command::List(a) => operate(Op::List, &a.vault, None, None),
        Command::Create(a) => operate(Op::Create, &a.vault, Some(&a.item), Some(&a.field)),
        Command::Edit(a) => operate(Op::Edit, &a.vault, Some(&a.item), Some(&a.field)),
        Command::Delete(a) => operate(Op::Delete, &a.vault, Some(&a.item), None),
        Command::Allow(a) => allow(a),
        Command::Approve(a) => approve(a),
    }
}

/// Who runs the command (contract §8.4 "Who the caller is").
struct Caller {
    session: SessionId,
    role: Role,
    plan: Option<String>,
    /// a Worker's Attempt: `CPD_ATTEMPT`, else the Session's
    attempt: Option<AttemptId>,
}

fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The Session of `CPD_SESSION` and its role, with the open store. No session: `secret_session_required`, before
/// the store is touched; unknown: `session_not_found`.
fn identify() -> Result<(Env, Caller), CpdError> {
    let Some(raw) = env_var("CPD_SESSION") else {
        return Err(CpdError::new(
            ErrorCode::SecretSessionRequired,
            "cpd secret needs a cpd Session: CPD_SESSION is not set.",
        ));
    };
    let reference: SessionRef = raw.parse().map_err(|_| session_not_found(&raw))?;
    let mut env = Env::open()?;
    let from_env = env_var("CPD_ATTEMPT")
        .map(|a| AttemptId::parse(&a))
        .transpose()?;
    let who = env.store.read(|conn| {
        let session = session::resolve(conn, &reference)?;
        let (role, plan, attempt): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT role, plan, attempt FROM sessions WHERE id = ?1",
                [session.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| session_not_found(session.as_str()))?;
        let role = match role.as_str() {
            "lead" => Role::Lead,
            "manager" => Role::Manager,
            "worker" => Role::Worker,
            other => return Err(CpdError::bad_row(format!("sessions.role {other:?}"))),
        };
        let role = if from_env.is_some() {
            Role::Worker
        } else {
            role
        };
        let attempt = match (&from_env, attempt) {
            (Some(a), _) => Some(a.clone()),
            (None, Some(a)) => Some(AttemptId::parse(&a)?),
            (None, None) => None,
        };
        Ok(Caller {
            session,
            role,
            plan,
            attempt,
        })
    })?;
    Ok((env, who))
}

fn session_not_found(s: &str) -> CpdError {
    CpdError::new(ErrorCode::SessionNotFound, format!("No Session {s}.")).with("session", s)
}

fn action_name(action: Action) -> &'static str {
    match action {
        Action::Op(op) => op.as_str(),
        Action::Allow => "allow",
        Action::Approve => "approve",
    }
}

/// The refusal of rules 2 and 3.
fn refusal(code: ErrorCode, who: &Caller, action: Action, vault: &SecretName) -> CpdError {
    let command = action_name(action);
    let message = match code {
        ErrorCode::SecretRoleDenied => format!(
            "A {} may not run `cpd secret {command}`.",
            who.role.as_str()
        ),
        ErrorCode::SecretVaultDenied => format!(
            "Vault {vault} is not allowed for `cpd secret {command}` by this caller: only {} are.",
            rules::VAULTS.join(" and ")
        ),
        _ => format!(
            "`cpd secret {command}` needs no approval for vault {vault}: approve is for another vault."
        ),
    };
    CpdError::new(code, message)
        .with("action", command)
        .with("vault", vault.as_str())
}

fn grant_required(
    grant: &str,
    op: Op,
    vault: &SecretName,
    item: Option<&SecretName>,
    field: Option<&SecretName>,
) -> CpdError {
    let next = if grant == "allowance" {
        "ask the Lead with cpd ask"
    } else {
        "ask the User with the tool cpd_secret_approve, then run the command again"
    };
    CpdError::new(
        ErrorCode::SecretGrantRequired,
        format!(
            "`cpd secret {}` in vault {vault} needs the {grant} of {}.",
            op.as_str(),
            if grant == "allowance" {
                "the Lead"
            } else {
                "the User"
            }
        ),
    )
    .with("grant", grant)
    .with("action", op.as_str())
    .with("vault", vault.as_str())
    .with("item", item.map(SecretName::as_str))
    .with("field", field.map(SecretName::as_str))
    .with("next", next)
}

/// The value on stdin: its bytes minus one trailing newline. A terminal, an empty value or text that is not UTF-8 is `usage`.
fn read_value() -> Result<String, CpdError> {
    let usage = |m: &str| CpdError::new(ErrorCode::Usage, m.to_owned());
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(usage(
            "The value is read from stdin, which is a terminal: pipe the value in.",
        ));
    }
    let mut bytes = Vec::new();
    stdin
        .read_to_end(&mut bytes)
        .map_err(|e| usage(&format!("Cannot read the value from stdin: {e}.")))?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.is_empty() {
        return Err(usage("The value on stdin is empty: pipe the value in."));
    }
    String::from_utf8(bytes).map_err(|_| usage("The value on stdin is not UTF-8 text."))
}

/// The five op commands: the guard, then op.
fn operate(
    op: Op,
    vault: &SecretName,
    item: Option<&SecretName>,
    field: Option<&SecretName>,
) -> Result<Output, CpdError> {
    let (mut env, who) = identify()?;
    let action = Action::Op(op);
    match rules::check(who.role, action, vault.as_str())
        .map_err(|c| refusal(c, &who, action, vault))?
    {
        Grant::None => {}
        Grant::Allowance => {
            let entry = Allowed {
                vault: vault.as_str().to_owned(),
                item: item.map(SecretName::as_str).unwrap_or_default().to_owned(),
                field: field.map(SecretName::as_str).unwrap_or_default().to_owned(),
            };
            let held = match &who.attempt {
                Some(attempt) => env
                    .store
                    .read(|conn| grants::allowance_held(&env.home, conn, attempt, &entry))?,
                None => false,
            };
            if !held {
                return Err(grant_required("allowance", op, vault, item, field));
            }
        }
        Grant::Approval => {
            let want = Approval {
                session: who.session.to_string(),
                action: op.as_str().to_owned(),
                vault: vault.as_str().to_owned(),
                item: item.map(|i| i.as_str().to_owned()),
                field: field.map(|f| f.as_str().to_owned()),
                at: String::new(),
            };
            if !grants::take_approval_on(&env.ports, &env.home, &want)? {
                return Err(grant_required("approval", op, vault, item, field));
            }
        }
    }
    let (v, i, f) = (
        vault.as_str(),
        item.map_or("", SecretName::as_str),
        field.map_or("", SecretName::as_str),
    );
    let fields = match op {
        Op::Get => op::get(v, i, f)?,
        Op::List => op::list(v)?,
        Op::Create => op::create(v, i, f, &read_value()?)?,
        Op::Edit => op::edit(v, i, f, &read_value()?)?,
        Op::Delete => op::delete(v, i)?,
    };
    let mut out = serde_json::Map::new();
    out.insert("vault".into(), json!(v));
    if item.is_some() {
        out.insert("item".into(), json!(i));
    }
    if field.is_some() {
        out.insert("field".into(), json!(f));
    }
    out.extend(fields);
    Ok(Output::success(Value::Object(out)))
}

/// `secret allow` (commands.md §24.6).
fn allow(a: AllowArgs) -> Result<Output, CpdError> {
    let (mut env, who) = identify()?;
    rules::check(who.role, Action::Allow, a.vault.as_str())
        .map_err(|c| refusal(c, &who, Action::Allow, &a.vault))?;
    let not_found = || {
        CpdError::new(
            ErrorCode::AttemptNotFound,
            format!("No Attempt {} in your Plan.", a.attempt),
        )
        .with("attempt", a.attempt.to_string())
    };
    let plan = who.plan.as_deref().ok_or_else(not_found)?;
    let status: Option<String> = env.store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT status FROM attempts WHERE plan = ?1 AND id = ?2",
                [plan.to_owned(), a.attempt.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    })?;
    if status.ok_or_else(not_found)? == "ended" {
        return Err(CpdError::new(
            ErrorCode::SecretAttemptEnded,
            format!("Attempt {} has ended.", a.attempt),
        )
        .with("attempt", a.attempt.to_string()));
    }
    let entry = Allowed {
        vault: a.vault.as_str().to_owned(),
        item: a.item.as_str().to_owned(),
        field: a.field.as_str().to_owned(),
    };
    let already = grants::allow(&env.home, &a.attempt, entry)?;
    Ok(Output::success(json!({
        "attempt": a.attempt.to_string(),
        "vault": a.vault.as_str(),
        "item": a.item.as_str(),
        "field": a.field.as_str(),
        "already": already,
    })))
}

/// `secret approve` (commands.md §24.7).
fn approve(a: ApproveArgs) -> Result<Output, CpdError> {
    let usage = |m: &str| CpdError::new(ErrorCode::Usage, m.to_owned());
    let name = a.action.as_str();
    if (a.action == Op::List) == a.item.is_some() {
        return Err(usage(&format!(
            "--item is required for every action but list, and forbidden for list (action {name})."
        )));
    }
    if matches!(a.action, Op::Get | Op::Create | Op::Edit) != a.field.is_some() {
        return Err(usage(&format!(
            "--field is required for get, create and edit, and forbidden otherwise (action {name})."
        )));
    }
    let (env, who) = identify()?;
    rules::check(who.role, Action::Approve, a.vault.as_str())
        .map_err(|c| refusal(c, &who, Action::Approve, &a.vault))?;
    let approval = Approval {
        session: who.session.to_string(),
        action: name.to_owned(),
        vault: a.vault.as_str().to_owned(),
        item: a.item.as_ref().map(|i| i.as_str().to_owned()),
        field: a.field.as_ref().map(|f| f.as_str().to_owned()),
        at: String::new(),
    };
    let expires = grants::approve_on(&env.ports, &env.home, &approval)?;
    Ok(Output::success(json!({
        "action": name,
        "vault": a.vault.as_str(),
        "item": approval.item,
        "field": approval.field,
        "expires_at": crate::clock::timestamp(expires),
    })))
}
