//! `pr post` (commands.md §9.8b): posts the Workflow's stored pull request to Slack through the helper and records it in
//! `pr_posts` through the outbox. The auto-merge is queued by the Slack source when the send confirms (swr-slack.md §6.2), once
//! and behind the GitHub executor; its outcome never undoes or fails the post. It appends no Event. No lock: two simultaneous runs could both
//! send, and the second's insert then fails as `already_posted`.

use serde_json::json;

use super::postable::unready;
use super::posts::{self, PostRecord};
use super::{Env, workflow_policy};
use crate::args::{PlanArg, WorkflowArg};
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::ports::Ports;
use crate::swr::source::github_write::merge_method;
use crate::swr::source::slack;

#[derive(clap::Args, Debug)]
pub struct PostArgs {
    #[command(flatten)]
    workflow: WorkflowArg,
    #[command(flatten)]
    plan: PlanArg,
}

pub(super) fn post(ports: &Ports, args: PostArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open_with(ports)?;
    let plan = env.plan(&id)?;
    let workflow = plan.select(&args.workflow)?.clone();
    let key = workflow.key.clone();

    // (a)
    if workflow.status != "open" {
        return Err(CpdError::new(
            ErrorCode::WorkflowNotOpen,
            format!(
                "Workflow {key} of Plan {id} is {}: pr post needs an open Workflow.",
                workflow.status
            ),
        )
        .with("pr", key.as_str())
        .with("status", workflow.status.as_str()));
    }
    // (b)
    if let Some(done) = env
        .store
        .read(|conn| posts::find(conn, id.as_str(), key.as_str()))?
    {
        return Err(already_posted(&key, &done));
    }
    // (c)
    let policy = workflow_policy(&mut env, &id, &key)?;
    // (d)
    if !policy.pr.post.enabled {
        return Err(CpdError::new(
            ErrorCode::PostDisabled,
            format!("Workflow {key}: posting is not enabled for its owner."),
        )
        .with("pr", key.as_str())
        .with("key", "pr.post.enabled"));
    }
    // (e)
    let row = workflow
        .pull_request
        .expect("an open Workflow has its pull_request row (prs CHECK)");
    let pull = env.store.read(|conn| {
        let mut pull = super::pulls::load(conn, row)?;
        crate::swr::github::effective_row(conn, &mut pull)?;
        Ok(pull)
    })?;
    if !pull.authored {
        return Err(CpdError::new(
            ErrorCode::NotAuthored,
            format!(
                "Pull request {} of Workflow {key} is not the User's own.",
                pull.number
            ),
        )
        .with("pr", key.as_str())
        .with("number", pull.number));
    }
    // (f)
    let channel = [
        policy.pr.post.channel.as_str(),
        env.config.inbox.team_channel.as_str(),
    ]
    .into_iter()
    .find(|c| !c.is_empty())
    .ok_or_else(|| {
        CpdError::new(
            ErrorCode::PostUnconfigured,
            "No Slack channel is configured to post to.",
        )
        .with("keys", json!(["pr.post.channel", "inbox.team_channel"]))
    })?
    .to_owned();
    // (g)
    let why = unready(&pull);
    if !why.is_empty() {
        let names: Vec<&str> = why.iter().map(|w| w.as_str()).collect();
        return Err(CpdError::new(
            ErrorCode::PrNotReady,
            format!("Workflow {key} is not ready to post: {}.", names.join(", ")),
        )
        .with("pr", key.as_str())
        .with("number", pull.number)
        .with("url", pull.url.as_str())
        .with("reasons", json!(names)));
    }
    // (g2) the repo's settings from the cache: a repo with no merge method refuses before the send
    match merge_method(&mut env.store, &pull.repo)? {
        Some(Ok(_)) => {}
        Some(Err(m)) => {
            return Err(CpdError::new(
                ErrorCode::NoMergeMethod,
                format!(
                    "{} allows no merge method: squash, merge commit and rebase are all off in its settings, so cpd cannot merge #{}. Allow one in the repo's settings.",
                    pull.repo, pull.number
                ),
            )
            .with("repo", pull.repo.as_str())
            .with("number", pull.number)
            .with("squash", m.squash)
            .with("merge", m.merge)
            .with("rebase", m.rebase));
        }
        None => {
            crate::swr::cache::invalidate(&mut env.store, &format!("github:repo:{}", pull.repo))?;
            return Err(CpdError::new(
                ErrorCode::ItemNotFound,
                format!(
                    "The settings of {} are not in the cache yet: the next poll reads them; try again then.",
                    pull.repo
                ),
            )
            .with("repo", pull.repo.as_str()));
        }
    }
    let approval = super::approval::post_guard(&mut env, &id, &key)?;
    // (h)
    let message = crate::policy::render_post(&policy.pr.post.template, &pull.title, &pull.url)
        .map_err(|e| {
            CpdError::new(
                ErrorCode::PolicyInvalid,
                format!("The pr.post.template does not render: {e}."),
            )
            .with("key", "pr.post.template")
        })?;
    // (i) the send is the outbox's: E24 runs the helper and writes the `pr_posts` row when it answers (swr-slack.md §5.1).
    let vars = json!({
        "channel": channel, "number": pull.number, "plan": id.as_str(), "repo": pull.repo.as_str(),
        "text": message, "workflow": key.as_str(),
    });
    let queued = env.store.write("pr post", |tx| {
        if let Some(done) = posts::find(tx, id.as_str(), key.as_str())? {
            return Err(already_posted(&key, &done));
        }
        if let Some(mutation_id) = slack::active_post(tx, id.as_str(), key.as_str())? {
            return Err(already_queued(&key, mutation_id));
        }
        slack::post_in(tx, &channel, &vars)
    })?;
    approval.consume()?;
    Ok(Output::success(json!({
        "plan": id, "pr": key, "number": pull.number, "url": pull.url, "channel": channel,
        "queued": queued.queued, "mutation_id": queued.mutation_id, "message": message,
    })))
}

/// `already_posted` of a post still in the outbox.
fn already_queued(key: &crate::ids::WorkflowKey, mutation_id: i64) -> CpdError {
    CpdError::new(
        ErrorCode::AlreadyPosted,
        format!("Workflow {key} has a post queued (mutation {mutation_id})."),
    )
    .with("pr", key.as_str())
    .with("workflow", key.as_str())
    .with("mutation_id", mutation_id)
}

fn already_posted(key: &crate::ids::WorkflowKey, done: &PostRecord) -> CpdError {
    CpdError::new(
        ErrorCode::AlreadyPosted,
        format!("Workflow {key} was already posted: {}.", done.link),
    )
    .with("pr", key.as_str())
    .with("channel", done.channel.as_str())
    .with("ts", done.ts.as_str())
    .with("link", done.link.as_str())
    .with("posted_at", done.posted_at.as_str())
}
