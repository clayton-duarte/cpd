//! `pr reply` (Plan fb-path): answers an item of the Workflow's stored feedback list with a reply ending in the
//! marker, as the User. It never resolves a thread, writes nothing in the store and appends no Event.

use std::path::PathBuf;

use serde_json::json;

use super::comments::{Kind, MARKER};
use super::{Env, gh};
use crate::args::{PlanArg, WorkflowArg};
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::ports::Ports;

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

    let lead = workflow.lead(&id)?;
    let runner = &*env.ports.runner;
    let url = match item.kind {
        Kind::Thread => {
            gh::reply_to_thread(runner, lead, &item.id, &format!("{text}\n\n{MARKER}"))?
        }
        Kind::Comment | Kind::Review => {
            let node = gh::pull_request_id(runner, lead, &pull.repo, pull.number)?;
            gh::reply_to_comment(
                runner,
                lead,
                &node,
                &format!("Re: {}\n\n{text}\n\n{MARKER}", item.url),
            )?
        }
    };
    Ok(Output::success(
        json!({"replied": {"pr": workflow.key, "to": args.to, "url": url}}),
    ))
}
