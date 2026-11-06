//! `pr approve` (internal; pro-d451): one private approval for the calling agent Session, written by the adapter's
//! tool after the User's "Allow once". See [`super::approval`].

use serde_json::json;

use super::approval::{self, Approval, Kind};
use crate::args::{PlanArg, WorkflowArg};
use crate::error::{CpdError, ErrorCode};
use crate::ids::WorkflowKey;
use crate::output::Output;
use crate::ports::Ports;

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

pub(super) fn approve(ports: &Ports, args: ApproveArgs) -> Result<Output, CpdError> {
    let Some(session) = approval::agent_session() else {
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
    let mut env = super::Env::open_with(ports)?;
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
        at: approval::stamp(at),
    };
    let expires = approval::approve(&env.home, &held)?;
    Ok(Output::success(json!({"approved": {
        "kind": args.kind.as_str(), "workflow": key, "to": args.to,
        "expires_at": approval::stamp(expires),
    }})))
}
