//! `pr reply` (Plan fb-path): answers an item of the Workflow's stored feedback list with a reply ending in the
//! marker, as the User, through the outbox (swr-github.md §9). It never resolves a thread.

use std::path::PathBuf;

use serde_json::json;

use super::comments::Kind;
use super::{Env, approval, workflow_policy};
use crate::args::{PlanArg, WorkflowArg};
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::ports::Ports;
use crate::swr::source::github_write::{enqueue, queued, target_of};

#[derive(clap::Args, Debug)]
pub struct ReplyArgs {
    #[command(flatten)]
    workflow: WorkflowArg,
    #[command(flatten)]
    plan: PlanArg,
    /// The id of the open item to answer (`cpd pr view`, `feedback`)
    #[arg(long, value_name = "item-id")]
    to: String,
    /// The reply's text (exactly one of --body and --body-file)
    #[arg(long, value_name = "text", conflicts_with = "body_file")]
    body: Option<String>,
    /// A file holding the reply's text
    #[arg(long = "body-file", value_name = "path")]
    body_file: Option<PathBuf>,
}

fn usage(message: &str) -> CpdError {
    CpdError::new(ErrorCode::Usage, message)
}

/// `pr.feedback.reply = "bots_only"`. `policy::Reply` is not exported from `policy`, so the value is read as its
/// serialized name.
fn bots_only(policy: &crate::policy::Policy) -> bool {
    serde_json::to_value(policy.pr.feedback.reply).is_ok_and(|v| v == "bots_only")
}

pub(super) fn reply(ports: &Ports, args: ReplyArgs) -> Result<Output, CpdError> {
    let text = match (&args.body, &args.body_file) {
        (Some(text), None) => text.clone(),
        (None, Some(path)) => {
            std::fs::read_to_string(path).map_err(|e| CpdError::file_unreadable(path, e))?
        }
        _ => return Err(usage("Give exactly one of --body and --body-file.")),
    };
    let text = text.trim();
    if text.is_empty() {
        return Err(usage("The reply's body is empty."));
    }

    let id = args.plan.require()?.clone();
    let mut env = Env::open_with(ports)?;
    let plan = env.plan(&id)?;
    let workflow = plan.select(&args.workflow)?;
    if workflow.status != "open" {
        return Err(CpdError::new(
            ErrorCode::WorkflowNotOpen,
            format!(
                "Workflow {} of Plan {id} is {}: pr reply needs an open Workflow.",
                workflow.key, workflow.status
            ),
        )
        .with("plan", id.as_str())
        .with("pr", workflow.key.as_str())
        .with("status", workflow.status.as_str()));
    }
    let row = workflow
        .pull_request
        .expect("an open Workflow has its pull_request row (prs CHECK)");
    let pull = env.store.read(|conn| super::pulls::load(conn, row))?;
    let list = pull.feedback.unwrap_or_default();
    let Some(item) = list.iter().find(|i| i.id == args.to) else {
        let open: Vec<&str> = list.iter().map(|i| i.id.as_str()).collect();
        return Err(CpdError::new(
            ErrorCode::FeedbackNotFound,
            format!(
                "Workflow {} has no open feedback item {:?}.",
                workflow.key, args.to
            ),
        )
        .with("pr", workflow.key.as_str())
        .with("to", args.to.as_str())
        .with("open", json!(open)));
    };

    let key = workflow.key.clone();
    let approval = match approval::agent_session() {
        Some(session)
            if bots_only(&workflow_policy(&mut env, &id, &key)?)
                && !(item.bot && item.kind == Kind::Thread) =>
        {
            let need = approval::Need {
                kind: approval::Kind::Reply,
                plan: &id,
                workflow: &key,
                to: Some(&args.to),
            };
            approval::require(&env.home, &session, &need, env.ports.clock.utc())?
        }
        _ => approval::Guard::none(),
    };
    let id_text = item.id.clone();
    let mutation = enqueue(
        &mut env.store,
        ports.clock.utc(),
        &target_of(&pull.repo, pull.number),
        "reply",
        json!({"to": id_text, "body": text}),
        Some(json!({"feedback_without": id_text})),
    )?;
    let out = queued(
        &mut env.store,
        &pull.repo,
        pull.number,
        mutation,
        json!({"replied": {"pr": workflow.key, "to": args.to, "url": item.url}}),
    )?;
    approval.consume()?;
    Ok(out)
}
