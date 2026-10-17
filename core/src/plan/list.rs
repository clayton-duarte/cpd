//! `plan list` (commands.md §4.2): every Plan with its PRs, a read-only
//! snapshot in one read transaction with its `events_cursor` (§17).

use serde_json::{Value, json};

use super::jobs;
use super::show::{pr_values, query_heads};
use super::{Env, ListArgs};
use crate::error::CpdError;
use crate::events::highest_id;
use crate::output::Output;

/// What the Dispatch column shows of a PR (§13.2, §17).
const PR_FIELDS: [&str; 5] = ["key", "repo", "status", "pr", "pull_request"];

pub(super) fn list(args: ListArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let (plans, cursor) = env.store.read(|conn| {
        let context = args.context.as_ref().map(|c| c.as_str().to_owned());
        let phase = args.phase.map(|p| p.as_str());
        let heads = query_heads(conn, "(?1 IS NULL OR context = ?1) AND (?2 IS NULL OR phase = ?2)", &[&context, &phase])?;
        let mut out = Vec::new();
        for plan in heads {
            let jobs = jobs::load(conn, &plan.id)?;
            let prs: Vec<Value> = pr_values(conn, &plan.id)?
                .into_iter()
                .map(|pr| Value::Object(PR_FIELDS.iter().map(|f| ((*f).to_owned(), pr[*f].clone())).collect()))
                .collect();
            out.push(json!({
                "id": plan.id, "context": plan.context, "title": plan.title, "phase": plan.phase.as_str(),
                "lead_session": plan.lead_session, "archived_at": plan.archived_at, "counts": jobs::counts(&jobs), "prs": prs,
            }));
        }
        Ok((out, highest_id(conn)?))
    })?;
    Ok(Output::success(json!({"plans": plans, "events_cursor": cursor})))
}
