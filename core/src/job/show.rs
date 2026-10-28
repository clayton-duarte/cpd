//! `job list` and `job show` (commands.md §5.6, §5.7): read-only.

use rusqlite::Connection;
use serde_json::{Value, json};

use super::{ListArgs, ShowArgs, Env, find, plan_phase};
use crate::error::CpdError;
use crate::ids::PlanId;
use crate::output::Output;
use crate::plan::jobs;

/// `job list` (§5.6).
pub(super) fn list(args: ListArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let listed = env.store.read(|conn| {
        plan_phase(conn, &plan)?;
        let all = jobs::load(conn, &plan)?;
        let everything = all.clone();
        let counts = attempt_counts(conn, &plan)?;
        Ok(all
            .into_iter()
            .filter(|j| args.statuses.is_empty() || args.statuses.contains(&j.status))
            .map(|j| {
                let needs = super::open::shown_needs(&everything, &j);
                let attempts = counts.iter().find(|(job, _)| *job == j.key).map_or(0, |(_, n)| *n);
                json!({"key": j.key, "title": j.title, "kind": j.kind, "status": j.status, "tier": j.tier, "appended": j.appended, "urgent": j.urgent,
                       "needs": needs, "attempts": attempts, "merged_sha": j.merged_sha})
            })
            .collect::<Vec<_>>())
    })?;
    Ok(Output::success(json!({"jobs": listed})))
}

fn attempt_counts(conn: &Connection, plan: &PlanId) -> Result<Vec<(String, i64)>, CpdError> {
    let mut stmt = conn.prepare("SELECT job, count(*) FROM attempts WHERE plan = ?1 GROUP BY job")?;
    let rows = stmt.query_map([plan.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// `job show` (§5.7).
pub(super) fn show(args: ShowArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let home = env.home.clone();
    let concurrency = env.config.concurrency;
    let (job, needed_by, attempts, needs, waiting_for) = env.store.read(|conn| {
        plan_phase(conn, &plan)?;
        let all = jobs::load(conn, &plan)?;
        let job = find(&all, &plan, &args.key)?.clone();
        // An `open` Job has no Dependency rows: its needs are its Ready rule's list, derived now (§3.2).
        let needs = super::open::shown_needs(&all, &job);
        let needed_by: Vec<&str> = all.iter().filter(|j| j.needs.contains(&job.key)).map(|j| j.key.as_str()).collect();
        let needed_by = needed_by.into_iter().map(str::to_owned).collect::<Vec<_>>();
        let attempts = attempts(conn, &plan, &job.key)?;
        // §5.7: only a queued Job that is not a tenant, for each resource it needs that has no free slot right now: `workers` first, then its Spec's.
        let wanted = crate::resource::wanted(conn, &plan, &job.key)?;
        let tenant: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM job_tenants WHERE plan = ?1 AND job = ?2 AND pool = 'workers')",
            [plan.as_str(), job.key.as_str()],
            |r| r.get(0),
        )?;
        let waiting_for = if job.status == "queued" && !tenant {
            let mut held = Vec::new();
            let workers = crate::resource::workers(conn, concurrency)?;
            if workers.pool().free == 0 {
                held.push(json!({"resource": crate::resource::WORKERS, "scope": "host", "slots": workers.slots, "holders": workers.holders}));
            }
            if let Some(wanted) = &wanted {
                let key = crate::ids::PrKey::parse(&wanted.pr)?;
                let policy = crate::pr::policy_of_pr(conn, &home, &plan, &key)?;
                for pool in crate::resource::pools_of(&policy.resources, &wanted.owner, &wanted.repo) {
                    if !wanted.names.iter().any(|n| n == pool.name()) {
                        continue;
                    }
                    // Only a resource with no free slot is waited for.
                    if crate::resource::free_slots(conn, &home, &pool)? == 0 {
                        let holders: Vec<_> = crate::resource::occupants(conn, &home, &pool)?
                            .into_iter()
                            .map(|(slot, holder)| json!({"slot": slot, "holder": holder}))
                            .collect();
                        held.push(json!({"resource": pool.name(), "scope": pool.scope(), "slots": pool.slots(), "holders": holders}));
                    }
                }
            }
            (wanted.is_some() || !held.is_empty()).then_some(held)
        } else {
            None
        };
        Ok((job, needed_by, attempts, needs, waiting_for))
    })?;
    let tamper = jobs::tampered(&plan, std::slice::from_ref(&job));
    let mut fields = job.to_json();
    fields["needs"] = json!(needs);
    if let Some(waiting_for) = waiting_for {
        fields["waiting_for"] = json!(waiting_for);
    }
    Ok(Output::success(json!({
        "job": fields, "needs": needs, "needed_by": needed_by, "attempts": attempts, "tamper": tamper,
    })))
}

/// Every Attempt of the Job with all its fields, oldest first, each with its Escalations.
fn attempts(conn: &Connection, plan: &PlanId, job: &str) -> Result<Vec<Value>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, number, tier, origin, status, outcome, cause, branch, worktree, base_sha, driver_pid, worker_session,
                spec_path, submitted_sha, submitted_at, check_tail, merged_sha, nudged, stop_request, metrics, started_at, ended_at
           FROM attempts WHERE plan = ?1 AND job = ?2 ORDER BY number",
    )?;
    let rows = stmt
        .query_map([plan.as_str(), job], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(19)?,
                json!({
                    "id": r.get::<_, String>(0)?, "number": r.get::<_, i64>(1)?, "tier": r.get::<_, String>(2)?,
                    "origin": r.get::<_, String>(3)?, "status": r.get::<_, String>(4)?,
                    "outcome": r.get::<_, Option<String>>(5)?, "cause": r.get::<_, Option<String>>(6)?,
                    "branch": r.get::<_, String>(7)?, "worktree": r.get::<_, String>(8)?,
                    "base_sha": r.get::<_, Option<String>>(9)?, "driver_pid": r.get::<_, Option<i64>>(10)?,
                    "worker_session": r.get::<_, Option<String>>(11)?, "spec_path": r.get::<_, Option<String>>(12)?,
                    "submitted_sha": r.get::<_, Option<String>>(13)?, "submitted_at": r.get::<_, Option<String>>(14)?,
                    "check_tail": r.get::<_, Option<String>>(15)?, "merged_sha": r.get::<_, Option<String>>(16)?,
                    "nudged": r.get::<_, bool>(17)?, "stop_request": r.get::<_, Option<String>>(18)?,
                    "started_at": r.get::<_, String>(20)?, "ended_at": r.get::<_, Option<String>>(21)?,
                }),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for (id, metrics, mut attempt) in rows {
        attempt["metrics"] = match metrics {
            None => Value::Null,
            Some(text) => serde_json::from_str(&text).map_err(|_| CpdError::bad_row(format!("Attempt {plan}/{id} has metrics {text:?}")))?,
        };
        attempt["escalations"] = json!(escalations(conn, plan, &id)?);
        out.push(attempt);
    }
    Ok(out)
}

fn escalations(conn: &Connection, plan: &PlanId, attempt: &str) -> Result<Vec<Value>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, number, question, status, answer, asker_pid, asked_at, answered_at, expired_at
           FROM escalations WHERE plan = ?1 AND attempt = ?2 ORDER BY number",
    )?;
    let rows = stmt
        .query_map([plan.as_str(), attempt], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?, "number": r.get::<_, i64>(1)?, "question": r.get::<_, String>(2)?,
                "status": r.get::<_, String>(3)?, "answer": r.get::<_, Option<String>>(4)?,
                "asker_pid": r.get::<_, i64>(5)?, "asked_at": r.get::<_, String>(6)?,
                "answered_at": r.get::<_, Option<String>>(7)?, "expired_at": r.get::<_, Option<String>>(8)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
