//! `pr view` (commands.md §9.8, contract §7.6): the checks, reviews and comments
//! of one PR's pull request, which the poller does not store. Read-only: no
//! write transaction, no Event.

use serde_json::{Value, json};

use super::feedback::current_checks;
use super::gh;
use super::status::{ChecksSummary, PrState, ReviewDecision};
use super::{Args, Env};
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

    let view = gh::view(&*env.ports.runner, lead, number)?;
    let inline = gh::inline_comments(&*env.ports.runner, lead, number)?;

    let state = PrState::of_graphql(&view.state)?;
    let summary = ChecksSummary::of(&current_checks(&view));
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
    Ok(Output::success(json!({
        "plan": id,
        "pr": {
            "key": pr.key, "number": number, "url": view.url, "title": view.title, "state": state.as_str(),
            "review_decision": ReviewDecision::of(view.review_decision.as_deref()).as_str(), "checks": summary.as_str(),
        },
        "checks": checks,
        "reviews": reviews,
        "comments": comments,
        "inline_comments": inline_comments,
        "feedback": feedback,
    })))
}
