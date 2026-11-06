//! `pr view` (commands.md §9.8, contract §7.6): the checks, reviews and comments
//! of one Workflow's pull request, which the poller does not store.

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::{Args, Env};
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::ports::Ports;
use crate::swr::github::effective_in;

const COMMAND: &str = "pr view";

fn login(node: &Value) -> Value {
    node["author"]["login"].clone()
}

/// `pr view` reads the cache (swr-github.md §6): the effective row and the hydrated detail of the pull request. It never calls
/// GitHub; a pull request the poll has not hydrated yet prints no checks, reviews or comments.
pub(super) fn view(ports: &Ports, args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open_with(ports)?;
    let plan = env.plan(&id)?;
    let workflow = plan.select(&args.workflow)?;
    if !matches!(workflow.status.as_str(), "open" | "merged") {
        return Err(workflow.refuse(&id, COMMAND, "open or merged"));
    }
    let number = workflow
        .pull
        .as_ref()
        .expect("an open or merged Workflow has its pull request (workflows CHECK)")
        .number;
    let row = workflow
        .pull_request
        .expect("an open or merged Workflow has its pull_request row (workflows CHECK)");
    let raw = env.store.read(|conn| crate::pr::pulls::load(conn, row))?;
    let effective = env
        .store
        .read(|conn| effective_in(conn, &raw.repo, raw.number))?
        .ok_or_else(|| {
            CpdError::new(
                ErrorCode::ItemNotFound,
                format!("{}#{} is not in the cache.", raw.repo, raw.number),
            )
        })?;
    let detail = effective.detail.clone().unwrap_or(Value::Null);
    let checks: Vec<Value> = super::swr_read::checks_of_detail(&detail)
        .iter()
        .filter_map(|c| {
            let name = c.name.clone().or_else(|| c.context.clone())?;
            let conclusion = c
                .conclusion
                .clone()
                .filter(|s| !s.is_empty())
                .or_else(|| c.state.clone())
                .unwrap_or_default();
            let mut check = json!({"name": name, "conclusion": conclusion});
            if let Some(url) = c.url.as_ref().filter(|u| !u.is_empty()) {
                check["url"] = json!(url);
            }
            Some(check)
        })
        .collect();
    let nodes = |v: &Value| v["nodes"].as_array().cloned().unwrap_or_default();
    let reviews: Vec<Value> = nodes(&detail["latestReviews"])
        .iter()
        .map(|r| json!({"id": r["id"], "author": login(r), "state": r["state"], "body": r["body"]}))
        .collect();
    let comments: Vec<Value> = nodes(&detail["comments"])
        .iter()
        .map(|c| json!({"id": c["id"], "author": login(c), "body": c["body"]}))
        .collect();
    let inline_comments: Vec<Value> = nodes(&detail["reviewThreads"])
        .iter()
        .flat_map(|t| {
            nodes(&t["comments"])
                .into_iter()
                .map(|c| json!({"id": c["id"], "author": login(&c), "path": t["path"], "body": c["body"]}))
                .collect::<Vec<_>>()
        })
        .collect();
    let feedback = effective.feedback.clone();
    let post = env.store.read(|conn| {
        let Some(p) = super::posts::find(conn, id.as_str(), workflow.key.as_str())? else { return Ok(None) };
        // The queue row of the Workflow's merge reaction (at most one): its emoji is the last attempted, `reacted_at` and `refusal` are its.
        let row: Option<(String, Option<String>, Option<String>)> = conn
            .query_row("SELECT emoji, reacted_at, refusal FROM slack_reactions WHERE plan = ?1 AND workflow = ?2", [id.as_str(), workflow.key.as_str()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
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
            "key": workflow.key, "number": number, "url": effective.row.url, "title": effective.title, "jira": workflow.jira, "state": effective.state,
            "review_decision": effective.review_decision, "checks": effective.checks, "head_oid": effective.head_oid, "overlay": effective.overlay,
        },
        "checks": checks,
        "reviews": reviews,
        "comments": comments,
        "inline_comments": inline_comments,
        "feedback": feedback,
        "post": post,
    })))
}
