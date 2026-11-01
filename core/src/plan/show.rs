//! `plan show` (commands.md §4.3): a read-only snapshot in one read
//! transaction with its `events_cursor` (§17), and the readers it shares with
//! `plan list` and `plan render`: the Plan row alone and its Workflows.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::fingerprint::{of_with as fingerprint_of, reviews_prs};
use super::jobs::{self, Job};
use super::prs::{Pull, WorkflowRow, load_needs};
use super::row::{self, PlanRow};
use super::{Env, ShowArgs};
use crate::error::CpdError;
use crate::events::highest_id;
use crate::ids::{PlanId, WorkflowKey};
use crate::output::Output;
use crate::pr::closed::closed_unmerged;
use crate::pr::pulls;

/// Every Workflow of `plan` in key order, `cancelled` ones included, with every
/// field of §2.2 Workflow (`needs`, `pr` and `pull_request` parsed). `jobs` is not here.
pub(super) fn workflow_values(conn: &Connection, plan: &PlanId) -> Result<Vec<Value>, CpdError> {
    let mut needs = load_needs(conn, plan)?;
    // The base Workflow is derived from `base` (contract §2.2 "The base Workflow"): the Workflow of the same repo whose Lead Branch it is.
    let mut stmt = conn.prepare(
        "SELECT key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                all_merged_at, start_failed_at, created_at, started_at, merged_at, title,
                (SELECT q.plan || '/' || q.key FROM workflows q WHERE q.repo = workflows.repo AND q.lead_branch = workflows.base), shape, head_ref, jira
           FROM workflows WHERE plan = ?1 ORDER BY key",
    )?;
    let closed = closed_unmerged(conn, Some(plan.as_str()))?;
    let rows = stmt
        .query_map([plan.as_str()], |r| {
            let pull = match (r.get::<_, Option<i64>>(6)?, r.get::<_, Option<String>>(7)?, r.get::<_, Option<String>>(8)?) {
                (Some(number), Some(url), Some(opened_at)) => json!({"number": number, "url": url, "opened_at": opened_at}),
                _ => Value::Null,
            };
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(9)?, json!({
                "key": r.get::<_, String>(0)?, "repo": r.get::<_, String>(1)?, "base": r.get::<_, String>(2)?,
                "base_workflow": r.get::<_, Option<String>>(16)?,
                "status": r.get::<_, String>(3)?,
                "closed_unmerged": closed.iter().any(|c| c.workflow.as_str() == r.get::<_, String>(0).as_deref().unwrap_or("")),
                "lead_branch": r.get::<_, String>(4)?,
                "lead_worktree": r.get::<_, Option<String>>(5)?, "pr": pull,
                "all_merged_at": r.get::<_, Option<String>>(10)?,
                "start_failed_at": r.get::<_, Option<String>>(11)?, "created_at": r.get::<_, String>(12)?,
                "started_at": r.get::<_, Option<String>>(13)?, "merged_at": r.get::<_, Option<String>>(14)?,
                "title": r.get::<_, Option<String>>(15)?, "shape": r.get::<_, String>(17)?,
                "head_ref": r.get::<_, Option<String>>(18)?, "jira": r.get::<_, Option<String>>(19)?,
            })))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(key, pull_request, mut value)| {
            value["feedback_open"] = Value::Null;
            value["pull_request"] = match pull_request {
                Some(row) => {
                    let loaded = pulls::load(conn, row)?;
                    if let Some(items) = &loaded.feedback {
                        value["feedback_open"] = json!(items.len());
                    }
                    loaded.to_json()
                }
                None => Value::Null,
            };
            value["needs"] = json!(needs.remove(&key).unwrap_or_default());
            Ok(value)
        })
        .collect()
}

/// `plan show` (§4.3).
pub(super) fn show(args: ShowArgs) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let (plan, mut workflows, jobs, mut attempts, escalations, review, cursor) =
        env.store.read(|conn| snapshot(conn, &id))?;
    let mut tamper = env.store.tamper()?;
    tamper.extend(jobs::tampered(&id, &jobs));
    for workflow in &mut workflows {
        let key = workflow["key"].as_str().expect("a Workflow key").to_owned();
        workflow["jobs"] = json!(
            jobs.iter()
                .filter(|j| j.workflow_str() == key)
                .map(|j| j.key.as_str())
                .collect::<Vec<_>>()
        );
    }
    let job_values: Vec<Value> = jobs
        .iter()
        .map(|j| {
            let mut v = j.to_json();
            v["attempts"] = json!(attempts.remove(&j.key).unwrap_or_default());
            v
        })
        .collect();
    Ok(Output::success(json!({
        "plan": plan.to_json(),
        "workflows": workflows,
        "jobs": job_values,
        "escalations": escalations,
        "review": review,
        "counts": jobs::counts(&jobs),
        "tamper": tamper,
        "events_cursor": cursor,
    })))
}

type Snapshot = (
    PlanRow,
    Vec<Value>,
    Vec<Job>,
    BTreeMap<String, Vec<Value>>,
    Vec<Value>,
    Value,
    i64,
);

/// Everything `plan show` prints that is read from the database, each row read once.
fn snapshot(conn: &Connection, id: &PlanId) -> Result<Snapshot, CpdError> {
    let plan = row::load(conn, id)?;
    let workflows = workflow_values(conn, id)?;
    let jobs = jobs::load(conn, id)?;
    let attempts = attempts(conn, id)?;
    let escalations = open_escalations(conn, id)?;
    let review = latest_review(conn, id, &plan, &jobs, &workflows)?;
    Ok((
        plan,
        workflows,
        jobs,
        attempts,
        escalations,
        review,
        highest_id(conn)?,
    ))
}

/// `plan_fingerprint` from rows already read: the Workflows are the `pr_values` ones, as the
/// [`WorkflowRow`]s the fingerprint takes. It reads `key`, `repo`, `base`, `needs` and `status`;
/// `pull_request` (expanded, not an id, in `pr_values`) is left `None`.
fn current_fingerprint(plan: &PlanRow, jobs: &[Job], workflows: &[Value]) -> String {
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n.as_str().map(str::to_owned))
            .collect()
    };
    let text = |p: &Value, field: &str| {
        p[field]
            .as_str()
            .unwrap_or_else(|| panic!("a Workflow {field}"))
            .to_owned()
    };
    let rows: Vec<WorkflowRow> = if reviews_prs(Some(plan.phase.as_str())) {
        workflows
            .iter()
            .map(|p| WorkflowRow {
                key: WorkflowKey::parse(&text(p, "key")).expect("a Workflow key"),
                repo: text(p, "repo"),
                base: text(p, "base"),
                lead_branch: text(p, "lead_branch"),
                lead_worktree: p["lead_worktree"].as_str().map(PathBuf::from),
                status: text(p, "status"),
                needs: strings(&p["needs"]),
                pull: p["pr"].is_object().then(|| Pull {
                    number: p["pr"]["number"].as_i64().expect("a pull request number"),
                    url: text(&p["pr"], "url"),
                    opened_at: text(&p["pr"], "opened_at"),
                }),
                pull_request: None,
                name: None,
                title: p["title"].as_str().map(str::to_owned),
                jira: p["jira"].as_str().map(str::to_owned),
                shape: text(p, "shape"),
                head_ref: p["head_ref"].as_str().map(str::to_owned),
                started_at: p["started_at"].as_str().map(str::to_owned),
            })
            .collect()
    } else {
        Vec::new()
    };
    fingerprint_of(jobs, &rows, plan.outline.as_ref())
}

/// Every Attempt of the Plan by Job, oldest first.
fn attempts(conn: &Connection, plan: &PlanId) -> Result<BTreeMap<String, Vec<Value>>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT job, id, number, tier, origin, status, outcome, cause, worker_session, started_at, ended_at
           FROM attempts WHERE plan = ?1 ORDER BY job, number",
    )?;
    let mut by_job: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let rows = stmt.query_map([plan.as_str()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            json!({
                "id": r.get::<_, String>(1)?, "number": r.get::<_, i64>(2)?, "tier": r.get::<_, String>(3)?,
                "origin": r.get::<_, String>(4)?, "status": r.get::<_, String>(5)?,
                "outcome": r.get::<_, Option<String>>(6)?, "cause": r.get::<_, Option<String>>(7)?,
                "worker_session": r.get::<_, Option<String>>(8)?,
                "started_at": r.get::<_, String>(9)?, "ended_at": r.get::<_, Option<String>>(10)?,
            }),
        ))
    })?;
    for row in rows {
        let (job, attempt) = row?;
        by_job.entry(job).or_default().push(attempt);
    }
    Ok(by_job)
}

/// The Plan's `open` Escalations, oldest first.
fn open_escalations(conn: &Connection, plan: &PlanId) -> Result<Vec<Value>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, attempt, question, asked_at FROM escalations WHERE plan = ?1 AND status = 'open' ORDER BY asked_at, id",
    )?;
    let rows = stmt
        .query_map([plan.as_str()], |r| {
            Ok(
                json!({"id": r.get::<_, String>(0)?, "attempt": r.get::<_, String>(1)?,
                      "question": r.get::<_, String>(2)?, "asked_at": r.get::<_, String>(3)?}),
            )
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The latest Plan review, with `current`: whether its fingerprint is still
/// the plan fingerprint of `jobs` and `prs` (already read, `pr_values`). Null
/// when the Plan has none.
fn latest_review(
    conn: &Connection,
    id: &PlanId,
    plan: &PlanRow,
    jobs: &[Job],
    workflows: &[Value],
) -> Result<Value, CpdError> {
    let review = conn
        .query_row(
            "SELECT id, status, fingerprint, feedback, requested_at, decided_at FROM plan_reviews
              WHERE plan = ?1 ORDER BY number DESC LIMIT 1",
            [id.as_str()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?;
    Ok(match review {
        None => Value::Null,
        Some((id, status, fingerprint, feedback, requested_at, decided_at)) => {
            let current = fingerprint == current_fingerprint(plan, jobs, workflows);
            json!({"id": id, "status": status, "fingerprint": fingerprint, "current": current,
                   "feedback": feedback, "requested_at": requested_at, "decided_at": decided_at})
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::super::sql_trace::{ledger, statements};
    use super::*;

    #[test]
    fn plan_show_reads_no_statement_twice() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ledger(dir.path(), 1);
        let id = PlanId::parse("plan-1").unwrap();
        let (shown, seen) = store
            .read(|conn| Ok(statements(conn, || snapshot(conn, &id))))
            .unwrap();
        assert_eq!(
            shown.unwrap().5["current"],
            false,
            "the fixture's review is of another fingerprint"
        );
        let mut once = BTreeSet::new();
        for sql in &seen {
            assert!(
                once.insert(sql),
                "read twice: {sql}\nall:\n{}",
                seen.join("\n")
            );
        }
    }

    #[test]
    fn the_review_is_current_while_the_fingerprint_of_the_jobs_and_prs_read_is_the_stored_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ledger(dir.path(), 1);
        let id = PlanId::parse("plan-1").unwrap();
        let fingerprint = store
            .read(|conn| super::super::plan_fingerprint(conn, &id))
            .unwrap();
        store
            .write("fixture", |tx| {
                Ok(tx
                    .execute("UPDATE plan_reviews SET fingerprint = ?1", [&fingerprint])
                    .map(|_| ())?)
            })
            .unwrap();
        let current = |store: &mut crate::store::Store| {
            store.read(|conn| snapshot(conn, &id)).unwrap().5["current"].clone()
        };
        assert_eq!(current(&mut store), true);
        store
            .write("fixture", |tx| {
                Ok(tx
                    .execute("UPDATE workflows SET base = 'develop' WHERE key = 'db'", [])
                    .map(|_| ())?)
            })
            .unwrap();
        assert_eq!(
            current(&mut store),
            false,
            "a Workflow's base is in the fingerprint"
        );
    }
}
