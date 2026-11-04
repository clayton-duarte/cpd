//! `pr repost` (commands.md §9.8b2, pro-d388): one reminder in the Slack thread of the Workflow's post, broadcast to the channel,
//! through the helper; on success `last_posted_at` moves to now and nothing else of the `pr_posts` row does. It appends no Event.
//! Nothing is sent but by this command.

use serde_json::json;

use super::posts;
use crate::args::{PlanArg, WorkflowArg};
use crate::error::{CpdError, ErrorCode};
use crate::host::deadline::Failed;
use crate::output::Output;
use crate::ports::Ports;
use crate::slack;

/// The helper's deadline: above its own 30 s call.
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

/// The reminder's text.
const MESSAGE: &str = "Bibbidi-bobbidi-bump!";

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

    let call = slack::call(
        &env.config,
        "slack-post.ts",
        &[
            ("channel", &post.channel),
            ("thread-ts", &post.ts),
            ("reply-broadcast", "true"),
        ],
        Some(MESSAGE.as_bytes().to_vec()),
        DEADLINE,
    )?;
    let argv: Vec<String> = std::iter::once(call.program.clone())
        .chain(call.args.iter().cloned())
        .collect();
    let out = match env.ports.runner.run(&call) {
        Ok(out) => out,
        Err(Failed::Io(e)) => {
            return Err(CpdError::new(
                ErrorCode::SpawnFailed,
                format!("Cannot run {}: {e}.", argv.join(" ")),
            )
            .with("argv", json!(argv)));
        }
        Err(Failed::Timeout(e)) => return Err(e),
    };
    slack::answer(&out, &argv)?;

    let last = env
        .store
        .write("pr repost", |tx| {
            posts::mark_bumped(tx, id.as_str(), key.as_str())?;
            Ok(tx.query_row(
                "SELECT last_posted_at FROM pr_posts WHERE plan = ?1 AND workflow = ?2",
                [id.as_str(), key.as_str()],
                |r| r.get::<_, Option<String>>(0),
            )?)
        })
        .map_err(|e| {
            CpdError::new(
                ErrorCode::StoreFailed,
                format!(
                    "The reminder WAS posted but recording it failed: {}",
                    e.message()
                ),
            )
        })?;
    Ok(Output::success(json!({
        "plan": id, "workflow": key,
        "post": {"channel": post.channel, "ts": post.ts},
        "message": MESSAGE, "last_posted_at": last,
    })))
}
