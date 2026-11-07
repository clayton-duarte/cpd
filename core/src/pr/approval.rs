//! The approval files under `$CPD_HOME/run/pr/approve/` (pro-d451): directory 0700, files 0600, written atomically.
//! One approval lets the calling Session (an agent: `$CPD_SESSION` set and non-empty) do one thing the policy otherwise
//! refuses it: one `pr reply` to one item, or one `pr post` or `pr repost` of one Workflow.
//!
//! Modeled on [`crate::host_repair_approval`]: the same private dir, atomic write and 10-minute lifetime, checked
//! at read. Consuming removes ONE matching file, and only after the guarded action succeeded.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use serde_json::json;

use super::Env;
use super::workflow_policy;
use crate::args::{PlanArg, WorkflowArg};
use crate::clock;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host_repair_approval::{APPROVAL_LIFETIME, write_private};
use crate::ids::{PlanId, SessionId, WorkflowKey};
use crate::output::Output;
use crate::ports::Ports;

/// What an approval lets the Session do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum Kind {
    /// One `pr reply` to the item `to`
    Reply,
    /// One `pr post` or one `pr repost` of the Workflow
    Post,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Reply => "reply",
            Kind::Post => "post",
        }
    }
}

/// `run/pr/approve/<uuid>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub session: String,
    pub kind: Kind,
    pub plan: String,
    pub workflow: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// RFC 3339
    pub at: String,
}

fn io(path: &Path, what: &str, e: impl std::fmt::Display) -> CpdError {
    CpdError::new(
        ErrorCode::Internal,
        format!("Cannot {what} {}: {e}.", path.display()),
    )
    .with("path", path.display().to_string())
}

/// The directory of the approval files.
pub fn dir(home: &CpdHome) -> PathBuf {
    home.run_dir().join("pr/approve")
}

/// The calling agent's Session: `$CPD_SESSION`, when set and non-empty. `None` is the cockpit or the User.
pub fn agent_session() -> Option<String> {
    std::env::var("CPD_SESSION").ok().filter(|s| !s.is_empty())
}

/// Writes `approval` and returns when it expires.
pub fn write(home: &CpdHome, approval: &Approval) -> Result<OffsetDateTime, CpdError> {
    let at = OffsetDateTime::parse(&approval.at, &Rfc3339).expect("`at` is written by cpd");
    let bytes = serde_json::to_vec(approval).expect("an approval serializes");
    write_private(
        home,
        &dir(home),
        &format!("{}.json", SessionId::new_v4()),
        &bytes,
    )?;
    Ok(at + APPROVAL_LIFETIME)
}

/// The approval an action holds: removed by [`Guard::consume`] once the action succeeded. `none` for a caller that
/// needs no approval.
#[derive(Debug)]
pub struct Guard(Option<PathBuf>);

impl Guard {
    pub fn none() -> Guard {
        Guard(None)
    }

    /// Removes the approval; called only after the guarded action succeeded.
    pub fn consume(self) -> Result<(), CpdError> {
        match self.0 {
            Some(path) => fs::remove_file(&path).map_err(|e| io(&path, "remove", e)),
            None => Ok(()),
        }
    }
}

/// What an action needs approved.
pub struct Need<'a> {
    pub kind: Kind,
    pub plan: &'a PlanId,
    pub workflow: &'a WorkflowKey,
    pub to: Option<&'a str>,
}

/// The live approval of `session` for `need`, else `approval_required`. Expired files are removed and ignored; files of
/// another Session, kind, Workflow or item are left as they are.
pub fn require(
    home: &CpdHome,
    session: &str,
    need: &Need,
    now: OffsetDateTime,
) -> Result<Guard, CpdError> {
    let dir = dir(home);
    let entries = match fs::read_dir(&dir) {
        Ok(e) => Some(e),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(io(&dir, "read", e)),
    };
    let mut files = entries
        .map(|e| e.collect::<Result<Vec<_>, _>>())
        .transpose()
        .map_err(|e| io(&dir, "read", e))?
        .unwrap_or_default();
    files.sort_by_key(|e| e.file_name());
    for entry in files {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io(&path, "read", e)),
        };
        let held: Approval = serde_json::from_slice(&bytes).map_err(|e| io(&path, "parse", e))?;
        let at = OffsetDateTime::parse(&held.at, &Rfc3339).map_err(|e| io(&path, "parse", e))?;
        if now >= at + APPROVAL_LIFETIME {
            fs::remove_file(&path).map_err(|e| io(&path, "remove", e))?;
            continue;
        }
        if held.session == session
            && held.kind == need.kind
            && held.plan == need.plan.as_str()
            && held.workflow == need.workflow.as_str()
            && held.to.as_deref() == need.to
        {
            return Ok(Guard(Some(path)));
        }
    }
    let mut error = CpdError::new(
        ErrorCode::ApprovalRequired,
        format!(
            "An agent needs the User's approval for pr {} of Workflow {}{}: none is held.",
            need.kind.as_str(),
            need.workflow,
            need.to.map_or_else(String::new, |t| format!(" to {t}"))
        ),
    )
    .with("workflow", need.workflow.as_str())
    .with("kind", need.kind.as_str());
    if let Some(to) = need.to {
        error = error.with("to", to);
    }
    Err(error)
}

/// The guard of `pr post` and `pr repost`: for an agent under `pr.post.require_approval`, its approval of kind `post`.
pub(super) fn post_guard(
    env: &mut Env,
    plan: &PlanId,
    workflow: &WorkflowKey,
) -> Result<Guard, CpdError> {
    let Some(session) = agent_session() else {
        return Ok(Guard::none());
    };
    if !workflow_policy(env, plan, workflow)?
        .pr
        .post
        .require_approval
    {
        return Ok(Guard::none());
    }
    let need = Need {
        kind: Kind::Post,
        plan,
        workflow,
        to: None,
    };
    require(&env.home, &session, &need, env.ports.clock.utc())
}

/// `at` as the approval holds it.
pub fn stamp(at: OffsetDateTime) -> String {
    clock::timestamp(at)
}

#[derive(clap::Args, Debug)]
pub struct ApproveArgs {
    /// What is approved: one `reply` to the item --to, or one `post` or `repost`
    #[arg(long, value_enum)]
    kind: Kind,
    /// The Workflow the approval is for
    #[arg(long, value_name = "key")]
    workflow: WorkflowKey,
    /// The id of the item a `reply` is for (required for reply, refused for post)
    #[arg(long, value_name = "item-id")]
    to: Option<String>,
    #[command(flatten)]
    plan: PlanArg,
}

fn usage(message: &str) -> CpdError {
    CpdError::new(ErrorCode::Usage, message)
}

/// `pr approve`.
pub(super) fn approve(ports: &Ports, args: ApproveArgs) -> Result<Output, CpdError> {
    let Some(session) = agent_session() else {
        return Err(usage(
            "pr approve needs $CPD_SESSION: it approves for the calling Session.",
        ));
    };
    match (args.kind, &args.to) {
        (Kind::Reply, None) => return Err(usage("--kind reply needs --to <item-id>.")),
        (Kind::Post, Some(_)) => return Err(usage("--kind post takes no --to.")),
        _ => {}
    }
    let id = args.plan.require()?.clone();
    let mut env = Env::open_with(ports)?;
    let plan = env.plan(&id)?;
    let key = plan
        .select(&WorkflowArg {
            workflow: Some(args.workflow),
        })?
        .key
        .clone();
    let at = env.ports.clock.utc();
    let held = Approval {
        session,
        kind: args.kind,
        plan: id.to_string(),
        workflow: key.to_string(),
        to: args.to.clone(),
        at: stamp(at),
    };
    let expires = write(&env.home, &held)?;
    Ok(Output::success(json!({"approved": {
        "kind": args.kind.as_str(), "workflow": key, "to": args.to,
        "expires_at": stamp(expires),
    }})))
}
