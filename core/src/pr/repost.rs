//! `pr repost` (commands.md §9.8b2, pro-d388): one reminder in the Slack thread of the Workflow's post, broadcast to the channel,
//! through the outbox (E24 runs the helper); when the send completes `last_posted_at` moves to now and nothing else of the `pr_posts` row does. It appends no Event.
//! Nothing is sent but by this command.

use serde_json::json;

use super::posts;
use crate::args::{PlanArg, WorkflowArg};
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::ports::Ports;
use crate::swr::source::slack;

#[derive(clap::Args, Debug)]
pub struct RepostArgs {
    #[command(flatten)]
    workflow: WorkflowArg,
    #[command(flatten)]
    plan: PlanArg,
}

pub(super) fn repost(ports: &Ports, args: RepostArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = super::Env::open_with(ports)?;
    let plan = env.plan(&id)?;
    let key = plan.select(&args.workflow)?.key.clone();

    let post = env
        .store
        .read(|conn| posts::find(conn, id.as_str(), key.as_str()))?
        .ok_or_else(|| {
            CpdError::new(
                ErrorCode::PostNotFound,
                format!("Workflow {key} of Plan {id} has no Slack post to repost."),
            )
            .with("plan", id.as_str())
            .with("workflow", key.as_str())
        })?;

    let vars = json!({
        "channel": post.channel, "plan": id.as_str(), "text": slack::REPOST_TEXT,
        "thread_ts": post.ts, "workflow": key.as_str(),
    });
    let queued = env
        .store
        .write("pr repost", |tx| slack::repost_in(tx, &post.channel, &vars))?;
    Ok(Output::success(json!({
        "plan": id, "workflow": key,
        "post": {"channel": post.channel, "ts": post.ts},
        "message": slack::REPOST_TEXT, "queued": queued.queued, "mutation_id": queued.mutation_id,
    })))
}
