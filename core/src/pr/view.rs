//! `pr view` (commands.md §9.8, contract §7.6): the checks, reviews and comments
//! of one PR's pull request, which the poller does not store. Read-only: no
//! write transaction, no Event.

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::gh;
use super::status::{ChecksSummary, PrState, ReviewDecision};
use super::{Args, Env, pr_policy};
use crate::error::CpdError;
use crate::output::Output;
use crate::ports::Ports;

const COMMAND: &str = "pr view";

fn login(user: &Option<gh::Login>) -> Option<&str> {
    user.as_ref().map(|u| u.login.as_str())
}

pub(super) fn view(ports: &Ports, args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open_with(ports)?;
    let plan = env.plan(&id)?;
    let pr = plan.select(&args.pr)?;
    if !matches!(pr.status.as_str(), "open" | "merged") {
        return Err(pr.refuse(&id, COMMAND, "open or merged"));
    }
    let number = pr.pull.as_ref().expect("an open or merged PR has its pull request (prs CHECK)").number;
    let lead = pr.lead(&id)?;

    let policy = pr_policy(&mut env, &id, &pr.key)?;
    let view = gh::view(&*env.ports.runner, lead, number)?;
    let inline = gh::inline_comments(&*env.ports.runner, lead, number)?;

    let state = PrState::of_graphql(&view.state)?;
    let summary = ChecksSummary::of(view.checks.as_deref().unwrap_or_default(), &policy.pr.human_gated_contexts);
    let checks: Vec<Value> = view
        .checks
        .iter()
        .flatten()
        .filter_map(|c| {
            let name = c.name.clone().or_else(|| c.context.clone())?;
            let conclusion = c.conclusion.clone().filter(|s| !s.is_empty()).or_else(|| c.state.clone()).unwrap_or_default();
            let mut check = json!({"name": name, "conclusion": conclusion});
            if let Some(url) = c.url.as_ref().filter(|u| !u.is_empty()) {
                check["url"] = json!(url);
            }
            Some(check)
        })
        .collect();
    let reviews: Vec<Value> =
        view.reviews.iter().map(|r| json!({"id": r.id.0, "author": login(&r.author), "state": r.state, "body": r.body})).collect();
    let comments: Vec<Value> = view.comments.iter().map(|c| json!({"id": c.id.0, "author": login(&c.author), "body": c.body})).collect();
    let inline_comments: Vec<Value> =
        inline.iter().map(|c| json!({"id": c.id.0, "author": login(&c.user), "path": c.path, "body": c.body})).collect();

    let row = pr.pull_request.expect("an open or merged PR has its pull_request row (prs CHECK)");
    let feedback = env.store.read(|conn| crate::pr::pulls::load(conn, row))?.feedback;
    let post = env.store.read(|conn| {
        let Some(p) = super::posts::find(conn, id.as_str(), pr.key.as_str())? else { return Ok(None) };
        // The queue row of the PR's merge reaction (at most one): its emoji is the last attempted, `reacted_at` and `refusal` are its.
        let row: Option<(String, Option<String>, Option<String>)> = conn
            .query_row("SELECT emoji, reacted_at, refusal FROM slack_reactions WHERE plan = ?1 AND pr = ?2", [id.as_str(), pr.key.as_str()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        let (reaction, reacted_at, refusal) = match row {
            Some((emoji, reacted_at, refusal)) => (Some(emoji), reacted_at, refusal),
            None => (p.reaction.clone(), None, None),
        };
        Ok(Some(json!({"channel": p.channel, "ts": p.ts, "link": p.link, "posted_at": p.posted_at, "reaction": reaction, "reacted_at": reacted_at, "refusal": refusal})))
    })?;
    Ok(Output::success(json!({
        "plan": id,
        "pr": {
            "key": pr.key, "number": number, "url": view.url, "title": view.title, "jira": pr.jira, "state": state.as_str(),
            "review_decision": ReviewDecision::of(view.review_decision.as_deref()).as_str(), "checks": summary.as_str(),
        },
        "checks": checks,
        "reviews": reviews,
        "comments": comments,
        "inline_comments": inline_comments,
        "feedback": feedback,
        "post": post,
    })))
}
