//! `plan show` (commands.md §4.3): a read-only snapshot in one read
//! transaction with its `events_cursor` (§17), and the readers it shares with
//! `plan list` and `plan render`: the Plan row alone and its PRs.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::fingerprint::plan_fingerprint;
use super::jobs::{self, Job};
use super::row::Phase;
use super::{Env, ShowArgs};
use crate::error::CpdError;
use crate::events::highest_id;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::Output;
use crate::pr::pulls;

/// A Plan as `plan list`, `plan show` and `plan render` read it: the `plans`
/// row alone, so a Plan with no PR (just created) is found too.
#[derive(Clone, Debug)]
pub(super) struct Head {
    pub id: PlanId,
    pub context: ContextId,
    pub title: String,
    pub jira: Option<String>,
    pub phase: Phase,
    pub lead_session: SessionId,
    pub created_at: String,
    pub locked_at: Option<String>,
    pub done_at: Option<String>,
    /// When it was archived (contract §2.2 "Archived"), else none.
    pub archived_at: Option<String>,
}

impl Head {
    /// Every field of §2.2 Plan except `prs`, as `plan show` prints them.
    fn to_json(&self) -> Value {
        json!({
            "id": self.id, "context": self.context, "title": self.title, "jira": self.jira, "phase": self.phase.as_str(),
            "lead_session": self.lead_session, "created_at": self.created_at, "locked_at": self.locked_at, "done_at": self.done_at,
            "archived_at": self.archived_at,
        })
    }
}

type RawHead = (String, String, String, Option<String>, String, String, String, Option<String>, Option<String>, Option<String>);

fn parse_head(raw: RawHead) -> Result<Head, CpdError> {
    let (id, context, title, jira, phase, lead_session, created_at, locked_at, done_at, archived_at) = raw;
    let id = PlanId::parse(&id).map_err(|_| CpdError::bad_row(format!("Plan id {id:?}")))?;
    let field = |name: &str, value: &str| CpdError::bad_row(format!("Plan {id} has {name} {value:?}"));
    Ok(Head {
        context: ContextId::parse(&context).map_err(|_| field("context", &context))?,
        phase: Phase::parse(&phase).ok_or_else(|| field("phase", &phase))?,
        lead_session: SessionId::parse(&lead_session).map_err(|_| field("lead_session", &lead_session))?,
        title,
        jira,
        created_at,
        locked_at,
        done_at,
        archived_at,
        id,
    })
}

const HEAD_COLUMNS: &str = "id, context, title, jira, phase, lead_session, created_at, locked_at, done_at, archived_at";

fn head_row(r: &rusqlite::Row) -> rusqlite::Result<RawHead> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?))
}

/// The Plan `id`, else `plan_not_found`.
pub(super) fn load_head(conn: &Connection, id: &PlanId) -> Result<Head, CpdError> {
    let raw = conn
        .query_row(&format!("SELECT {HEAD_COLUMNS} FROM plans WHERE id = ?1"), [id.as_str()], head_row)
        .optional()?
        .ok_or_else(|| CpdError::plan_not_found(id))?;
    parse_head(raw)
}

/// Every Plan matching `filter` (SQL after `WHERE`), oldest first.
pub(super) fn query_heads(conn: &Connection, filter: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<Head>, CpdError> {
    let mut stmt = conn.prepare(&format!("SELECT {HEAD_COLUMNS} FROM plans WHERE {filter} ORDER BY created_at, id"))?;
    let rows = stmt.query_map(params, head_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(parse_head).collect()
}

/// Every PR of `plan` in key order, `cancelled` ones included, with every
/// field of §2.2 PR (`needs`, `pr` and `pull_request` parsed). `jobs` is not here.
pub(super) fn pr_values(conn: &Connection, plan: &PlanId) -> Result<Vec<Value>, CpdError> {
    let mut needs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT pr, needs FROM pr_needs WHERE plan = ?1 ORDER BY pr, needs")?;
    for row in stmt.query_map([plan.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (pr, need) = row?;
        needs.entry(pr).or_default().push(need);
    }
    let mut stmt = conn.prepare(
        "SELECT key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                all_merged_at, start_failed_at, created_at, started_at, merged_at
           FROM prs WHERE plan = ?1 ORDER BY key",
    )?;
    let rows = stmt
        .query_map([plan.as_str()], |r| {
            let pull = match (r.get::<_, Option<i64>>(6)?, r.get::<_, Option<String>>(7)?, r.get::<_, Option<String>>(8)?) {
                (Some(number), Some(url), Some(opened_at)) => json!({"number": number, "url": url, "opened_at": opened_at}),
                _ => Value::Null,
            };
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(9)?, json!({
                "key": r.get::<_, String>(0)?, "repo": r.get::<_, String>(1)?, "base": r.get::<_, String>(2)?,
                "status": r.get::<_, String>(3)?, "lead_branch": r.get::<_, String>(4)?,
                "lead_worktree": r.get::<_, Option<String>>(5)?, "pr": pull,
                "all_merged_at": r.get::<_, Option<String>>(10)?,
                "start_failed_at": r.get::<_, Option<String>>(11)?, "created_at": r.get::<_, String>(12)?,
                "started_at": r.get::<_, Option<String>>(13)?, "merged_at": r.get::<_, Option<String>>(14)?,
            })))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(key, pull_request, mut value)| {
            value["pull_request"] = match pull_request {
                Some(row) => pulls::load(conn, row)?.to_json(),
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
    let snapshot = env.store.read(|conn| {
        let plan = load_head(conn, &id)?;
        let prs = pr_values(conn, &id)?;
        let jobs = jobs::load(conn, &id)?;
        let attempts = attempts(conn, &id)?;
        let escalations = open_escalations(conn, &id)?;
        let review = latest_review(conn, &id)?;
        Ok((plan, prs, jobs, attempts, escalations, review, highest_id(conn)?))
    })?;
    let (plan, mut prs, jobs, mut attempts, escalations, review, cursor): (Head, Vec<Value>, Vec<Job>, _, _, _, _) = snapshot;
    let mut tamper = env.store.tamper()?;
    tamper.extend(jobs::tampered(&id, &jobs));
    for pr in &mut prs {
        let key = pr["key"].as_str().expect("a PR key").to_owned();
        pr["jobs"] = json!(jobs.iter().filter(|j| j.pr == key).map(|j| j.key.as_str()).collect::<Vec<_>>());
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
        "prs": prs,
        "jobs": job_values,
        "escalations": escalations,
        "review": review,
        "counts": jobs::counts(&jobs),
        "tamper": tamper,
        "events_cursor": cursor,
    })))
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
            Ok(json!({"id": r.get::<_, String>(0)?, "attempt": r.get::<_, String>(1)?,
                      "question": r.get::<_, String>(2)?, "asked_at": r.get::<_, String>(3)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The latest Plan review, with `current`: whether its fingerprint is still
/// the plan fingerprint. Null when the Plan has none.
fn latest_review(conn: &Connection, plan: &PlanId) -> Result<Value, CpdError> {
    let review = conn
        .query_row(
            "SELECT id, status, fingerprint, feedback, requested_at, decided_at FROM plan_reviews
              WHERE plan = ?1 ORDER BY number DESC LIMIT 1",
            [plan.as_str()],
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
            let current = fingerprint == plan_fingerprint(conn, plan)?;
            json!({"id": id, "status": status, "fingerprint": fingerprint, "current": current,
                   "feedback": feedback, "requested_at": requested_at, "decided_at": decided_at})
        }
    })
}
