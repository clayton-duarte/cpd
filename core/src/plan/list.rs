//! `plan list` (commands.md §4.2): every Plan with its Workflows, a read-only
//! snapshot in one read transaction with its `events_cursor` (§17).

use std::collections::BTreeMap;

use rusqlite::{Connection, Row};
use serde_json::{Value, json};

use super::jobs::STATUSES;
use super::row;
use super::{Env, ListArgs};
use crate::error::CpdError;
use crate::events::highest_id;
use crate::output::Output;
use crate::pr::pulls::PullRequest;

pub(super) fn list(args: ListArgs) -> Result<Output, CpdError> {
    list_in(&mut Env::open()?, args)
}

/// `plan list` read through the open `env`.
pub(crate) fn list_in(env: &mut Env, args: ListArgs) -> Result<Output, CpdError> {
    let (plans, cursor) = env.store.read(|conn| snapshot(conn, &args))?;
    Ok(Output::success(
        json!({"plans": plans, "events_cursor": cursor}),
    ))
}

/// The Plans and the cursor, read through `conn` with four statements whatever
/// the size of the Ledger (as `tui/src/read` does): the Plans, the Jobs counted
/// by Plan and status, the Workflows with their pull request, the cursor.
fn snapshot(conn: &Connection, args: &ListArgs) -> Result<(Vec<Value>, i64), CpdError> {
    let context = args.context.as_ref().map(|c| c.as_str().to_owned());
    let phase = args.phase.map(|p| p.as_str());
    let heads = row::query(
        conn,
        "(?1 IS NULL OR context = ?1) AND (?2 IS NULL OR phase = ?2)",
        &[&context, &phase],
    )?;
    let mut counts = job_counts(conn)?;
    let mut workflows = workflows(conn)?;
    let out = heads
        .into_iter()
        .map(|plan| {
            json!({
                "id": plan.id, "context": plan.context, "title": plan.title, "phase": plan.phase.as_str(),
                "lead_session": plan.lead_session, "created_at": plan.created_at, "archived_at": plan.archived_at,
                "counts": counts_value(&counts.remove(plan.id.as_str()).unwrap_or_default()),
                "workflows": workflows.remove(plan.id.as_str()).unwrap_or_default(),
            })
        })
        .collect();
    Ok((out, highest_id(conn)?))
}

/// How many Jobs each Plan has in each status.
fn job_counts(conn: &Connection) -> Result<BTreeMap<String, BTreeMap<String, i64>>, CpdError> {
    let mut stmt = conn.prepare("SELECT plan, status, count(*) FROM jobs GROUP BY plan, status")?;
    let mut by_plan: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        let (plan, status, n) = row?;
        by_plan.entry(plan).or_default().insert(status, n);
    }
    Ok(by_plan)
}

/// Every status listed, as `jobs::counts` prints it.
fn counts_value(by_status: &BTreeMap<String, i64>) -> Value {
    Value::Object(
        STATUSES
            .iter()
            .map(|s| {
                (
                    (*s).to_owned(),
                    json!(by_status.get(*s).copied().unwrap_or(0)),
                )
            })
            .collect(),
    )
}

/// What the Dispatch column shows of every Workflow (§13.2, §17), by Plan, in key order:
/// `key`, `repo`, `status`, `pr` and `pull_request`.
fn workflows(conn: &Connection) -> Result<BTreeMap<String, Vec<Value>>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT r.plan, r.key, r.repo, r.status, r.pr_number, r.pr_url, r.pr_opened_at,
                q.id, q.repo, q.number, q.url, q.title, q.draft, q.state, q.review_decision, q.checks, q.created_at, q.merged_at,
                q.changed_at, q.mergeable, q.merge_state, q.authored, q.review_requested
           FROM workflows r LEFT JOIN pull_requests q ON q.id = r.pull_request ORDER BY r.plan, r.key",
    )?;
    let mut by_plan: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for row in stmt.query_map([], workflow_row)? {
        let (plan, workflow) = row?;
        by_plan.entry(plan).or_default().push(workflow);
    }
    Ok(by_plan)
}

fn workflow_row(r: &Row) -> rusqlite::Result<(String, Value)> {
    let workflow = match (
        r.get::<_, Option<i64>>(4)?,
        r.get::<_, Option<String>>(5)?,
        r.get::<_, Option<String>>(6)?,
    ) {
        (Some(number), Some(url), Some(opened_at)) => {
            json!({"number": number, "url": url, "opened_at": opened_at})
        }
        _ => Value::Null,
    };
    let pull_request = match r.get::<_, Option<i64>>(7)? {
        None => Value::Null,
        Some(id) => PullRequest {
            id,
            repo: r.get(8)?,
            number: r.get(9)?,
            url: r.get(10)?,
            title: r.get(11)?,
            draft: r.get(12)?,
            state: r.get(13)?,
            review_decision: r.get(14)?,
            checks: r.get(15)?,
            created_at: r.get(16)?,
            merged_at: r.get(17)?,
            changed_at: r.get(18)?,
            mergeable: r.get(19)?,
            merge_state: r.get(20)?,
            authored: r.get(21)?,
            review_requested: r.get(22)?,
            // Not read here: `to_json` shows neither (`plan show`'s `feedback_open` is its own read).
            updated_at: None,
            feedback: None,
            // Likewise not shown.
            base_ref: None,
            default_branch: None,
        }
        .to_json(),
    };
    let value = json!({"key": r.get::<_, String>(1)?, "repo": r.get::<_, String>(2)?, "status": r.get::<_, String>(3)?, "pr": workflow, "pull_request": pull_request});
    Ok((r.get(0)?, value))
}

#[cfg(test)]
mod tests {
    use super::super::sql_trace::{ledger, statements};
    use super::*;

    fn run(plans: usize) -> (Vec<Value>, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ledger(dir.path(), plans);
        let args = ListArgs {
            context: None,
            phase: None,
        };
        let (result, seen) = store
            .read(|conn| Ok(statements(conn, || snapshot(conn, &args))))
            .unwrap();
        (result.unwrap().0, seen)
    }

    #[test]
    fn plan_list_runs_the_same_statements_whatever_the_number_of_plans() {
        let (_, one) = run(1);
        let (plans, many) = run(6);
        assert_eq!(plans.len(), 6);
        assert_eq!(
            many.len(),
            one.len(),
            "statements for 6 Plans:\n{}",
            many.join("\n")
        );
    }

    #[test]
    fn plan_list_prints_the_counts_and_the_prs_of_every_plan() {
        let (plans, _) = run(2);
        assert_eq!(plans[1]["id"], "plan-2");
        assert_eq!(
            plans[1]["counts"],
            json!({"draft": 2, "queued": 0, "running": 0, "merged": 0, "failed": 0, "cancelled": 0})
        );
        assert_eq!(
            plans[1]["workflows"],
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
