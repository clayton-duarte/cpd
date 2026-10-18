//! `plan list` (commands.md §4.2): every Plan with its PRs, a read-only
//! snapshot in one read transaction with its `events_cursor` (§17).

use rusqlite::Connection;
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
    let (plans, cursor) = env.store.read(|conn| snapshot(conn, &args))?;
    Ok(Output::success(json!({"plans": plans, "events_cursor": cursor})))
}

/// The Plans and the cursor, read through `conn`.
fn snapshot(conn: &Connection, args: &ListArgs) -> Result<(Vec<Value>, i64), CpdError> {
    {
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
    }
}

#[cfg(test)]
mod tests {
    use super::super::sql_trace::{ledger, statements};
    use super::*;

    fn run(plans: usize) -> (Vec<Value>, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ledger(dir.path(), plans);
        let args = ListArgs { context: None, phase: None };
        let (result, seen) = store.read(|conn| Ok(statements(conn, || snapshot(conn, &args)))).unwrap();
        (result.unwrap().0, seen)
    }

    #[test]
    fn plan_list_runs_the_same_statements_whatever_the_number_of_plans() {
        let (_, one) = run(1);
        let (plans, many) = run(6);
        assert_eq!(plans.len(), 6);
        assert_eq!(many.len(), one.len(), "statements for 6 Plans:\n{}", many.join("\n"));
    }

    #[test]
    fn plan_list_prints_the_counts_and_the_prs_of_every_plan() {
        let (plans, _) = run(2);
        assert_eq!(plans[1]["id"], "plan-2");
        assert_eq!(plans[1]["counts"], json!({"draft": 2, "queued": 0, "running": 0, "merged": 0, "failed": 0, "cancelled": 0}));
        assert_eq!(
            plans[1]["prs"],
            json!([
                {"key": "api", "repo": "/r/api", "status": "open",
                 "pr": {"number": 2, "url": "https://github.com/acme/api/pull/2", "opened_at": "t"},
                 "pull_request": {"repo": "acme/api", "number": 2, "url": "https://github.com/acme/api/pull/2", "title": "Add api",
                                  "draft": false, "state": "open", "review_decision": "approved", "checks": "passed",
                                  "mergeable": "mergeable", "merge_state": "clean", "created_at": "t", "merged_at": null}},
                {"key": "db", "repo": "/r/db", "status": "waiting", "pr": null, "pull_request": null},
            ])
        );
    }
}
