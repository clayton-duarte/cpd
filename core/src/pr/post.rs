//! `pr post` (commands.md §9.8b): posts the Workflow's stored pull request to Slack through the helper and records it in
//! `pr_posts`, then queues the auto-merge of the pull request on the outbox (`auto_merge`: deferred while the Workflow is
//! stacked; its outcome never undoes or fails the post). It appends no Event. No lock: two simultaneous runs could both
//! send, and the second's insert then fails as `already_posted`.

use serde_json::{Value, json};

use super::auto_merge;
use super::postable::unready;
use super::posts::{self, PostRecord};
use super::{Env, workflow_policy};
use crate::args::{PlanArg, WorkflowArg};
use crate::clock::timestamp;
use crate::error::{CpdError, ErrorCode};
use crate::host::deadline::Failed;
use crate::ids::{PlanId, WorkflowKey};
use crate::output::Output;
use crate::ports::Ports;
use crate::slack;
use crate::swr::source::github_write::{enqueue, merge_method, target_of};

/// The helper's deadline: above its own 30 s call.
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(clap::Args, Debug)]
pub struct PostArgs {
    #[command(flatten)]
    workflow: WorkflowArg,
    #[command(flatten)]
    plan: PlanArg,
}

/// Test constructor of the arguments: Workflow `key` of Plan `plan`.
#[cfg(test)]
pub(super) fn args(plan: PlanId, key: WorkflowKey) -> PostArgs {
    PostArgs {
        workflow: WorkflowArg {
            workflow: Some(key),
        },
        plan: PlanArg { plan: Some(plan) },
    }
}

fn record_json(post: &PostRecord) -> Value {
    json!({"channel": post.channel, "ts": post.ts, "link": post.link, "posted_at": post.posted_at,
           "reaction": post.reaction, "reacted_at": post.reacted_at})
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
    // (h)
    let message = crate::policy::render_post(&policy.pr.post.template, &pull.title, &pull.url)
        .map_err(|e| {
            CpdError::new(
                ErrorCode::PolicyInvalid,
                format!("The pr.post.template does not render: {e}."),
            )
            .with("key", "pr.post.template")
        })?;
    // (i)
    let call = slack::call(
        &env.config,
        "slack-post.ts",
        &[("channel", &channel)],
        Some(message.clone().into_bytes()),
        DEADLINE,
    )?;
    let argv: Vec<String> = std::iter::once(call.program.clone())
        .chain(call.args.iter().cloned())
        .collect();
    // (i) the row is written only AFTER the send resolves; a post that failed never happened.
    let since = timestamp(env.ports.clock.utc());
    let mut record = PostRecord {
        channel: channel.clone(),
        ts: String::new(),
        link: String::new(),
        posted_at: since.clone(),
        reaction: Some(policy.pr.post.merged_reaction.clone()).filter(|r| !r.is_empty()),
        reacted_at: None,
        auto_merge: "pending".to_owned(),
        auto_merge_at: None,
    };
    let sent = match env.ports.runner.run(&call) {
        Ok(out) => slack::answer(&out, &argv),
        Err(Failed::Io(e)) => Err(CpdError::new(
            ErrorCode::SpawnFailed,
            format!("Cannot run {}: {e}.", argv.join(" ")),
        )
        .with("argv", json!(argv))),
        Err(Failed::Timeout(e)) => Err(e),
    }?;
    let text = |name: &str| sent[name].as_str().unwrap_or_default().to_owned();
    record.channel = text("channel");
    record.ts = text("ts");
    record.link = text("link");
    // (j)
    let stored = env.store.write("pr post", |tx| {
        let new = posts::insert(tx, id.as_str(), key.as_str(), &record)?;
        if new {
            crate::swr::source::jira::enqueue_for_workflow(
                tx,
                id.as_str(),
                key.as_str(),
                "in_review",
            )?;
        }
        Ok(new)
    });
    match stored {
        Ok(true) => {}
        Ok(false) => {
            let done = env
                .store
                .read(|conn| posts::find(conn, id.as_str(), key.as_str()))?;
            return Err(match done {
                Some(done) => already_posted(&key, &done),
                None => CpdError::bad_row(format!(
                    "Workflow {key} has no post after its insert was refused"
                )),
            });
        }
        Err(e) => {
            return Err(CpdError::new(
                ErrorCode::StoreFailed,
                format!(
                    "The message WAS posted ({}) but recording it failed: {}",
                    record.link,
                    e.message()
                ),
            )
            .with("link", record.link.as_str())
            .with("channel", record.channel.as_str())
            .with("ts", record.ts.as_str()));
        }
    }
    let merge = request_auto_merge(&mut env, &id, &key, &pull)?;
    Ok(Output::success(json!({
        "plan": id, "pr": key, "number": pull.number, "url": pull.url,
        "post": record_json(&record), "message": message, "auto_merge": merge,
    })))
}

/// The auto-merge request, after the post is stored: `none` when the repo does not allow it, deferred while the Workflow is
/// stacked (E18 queues it once it is unstacked), else an `auto_merge` mutation on the outbox (swr-github.md §9); its outcome never
/// undoes or fails the post, and the drain settles `pr_posts.auto_merge`.
fn request_auto_merge(
    env: &mut Env,
    id: &PlanId,
    key: &WorkflowKey,
    pull: &super::pulls::PullRequest,
) -> Result<Value, CpdError> {
    let allowed: bool = env.store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT auto_merge_allowed FROM swr_repos WHERE repo = ?1",
                [&pull.repo],
                |r| r.get(0),
            )
            .unwrap_or(false))
    })?;
    if !allowed {
        env.store
            .write("pr post", |tx| {
                posts::set_auto_merge(tx, id.as_str(), key.as_str(), "none")
            })
            .map_err(|e| {
                CpdError::new(
                    ErrorCode::StoreFailed,
                    format!(
                        "The message WAS posted but recording its auto-merge outcome failed: {}",
                        e.message()
                    ),
                )
            })?;
        return Ok(state(
            "not_allowed",
            Some(
                "the repo does not allow auto-merge: cpd merges the Workflow when it is ready (the cockpit's Merge, or the Lead with `cpd pr merge` after a human approves)",
            ),
        ));
    }
    let stacked = env
        .store
        .read(|conn| auto_merge::stacked(conn, id.as_str(), key.as_str()))?;
    if stacked {
        return Ok(state(
            "deferred",
            Some(
                "the Workflow is stacked on another Workflow's branch: the request waits until it is unstacked",
            ),
        ));
    }
    let mutation = enqueue(
        &mut env.store,
        env.ports.clock.utc(),
        &target_of(&pull.repo, pull.number),
        "auto_merge",
        json!({"repo": pull.repo, "number": pull.number}),
        Some(json!({"auto_merge": "requested"})),
    )?;
    Ok(json!({"state": "queued", "reason": null, "mutation_id": mutation}))
}

fn state(state: &str, reason: Option<&str>) -> Value {
    json!({"state": state, "reason": reason.map(one_line)})
}

/// A reason on one line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
