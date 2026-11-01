//! `cpd incident start | end | status` (commands.md §25, hs-d643): Plan-scoped incident mode.
//! When on (one row in the `incident` table plus one row per Plan in `incident_plans`), the
//! engine holds scheduling for every Plan not in the incident (`is_held`, contract §2.2
//! "Incident", §3.7). `start` and `end` are Manager-only; `status` is for any caller.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::args::Text;
use crate::clock;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{PlanId, SessionId};
use crate::output::Output;
use crate::store::{Store, WriteTxn};

/// The `incident` commands.
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Turn incident mode on (Manager) (commands.md §25.1)
    Start(StartArgs),
    /// Turn incident mode off (Manager) (commands.md §25.2)
    End,
    /// Report whether incident mode is on (commands.md §25.3)
    Status,
}

#[derive(clap::Args, Debug)]
pub struct StartArgs {
    /// The Plans in the incident (at least one)
    #[arg(long = "plan", value_name = "id", required = true)]
    pub plan: Vec<PlanId>,
    /// The explanation of the incident (1 or more characters; required only on a fresh start)
    #[arg(long, value_name = "text")]
    pub reason: Option<Text>,
    /// The `<plan>/<key>` Workflow that ends the incident when it merges
    #[arg(long, value_name = "<plan>/<key>")]
    pub until_workflow: Option<String>,
}

/// Runs one `incident` command. The caller's Session is `$CPD_SESSION` (commands.md §25).
pub fn run(command: Command) -> Result<Output, CpdError> {
    let Env { mut store, .. } = Env::open()?;
    let caller = match std::env::var_os("CPD_SESSION").filter(|v| !v.is_empty()) {
        Some(value) => Some(SessionId::parse(&value.to_string_lossy())?),
        None => None,
    };
    match command {
        Command::Start(args) => start(
            &mut store,
            caller.as_ref(),
            &args.plan,
            args.reason.as_ref().map_or("", |t| t.as_str()),
            args.until_workflow.as_deref(),
        ),
        Command::End => end(&mut store, caller.as_ref()),
        Command::Status => status(&mut store),
    }
}

/// The refusal of a caller that is not a live Manager (commands.md §25).
fn manager_only() -> CpdError {
    CpdError::new(
        ErrorCode::ManagerOnly,
        "Only a live Manager Session may change incident mode.",
    )
}

/// The caller must be a live (not archived) Manager Session.
fn require_manager<'a>(
    conn: &Connection,
    caller: Option<&'a SessionId>,
) -> Result<&'a SessionId, CpdError> {
    let caller = caller.ok_or_else(manager_only)?;
    let manager: Option<bool> = conn
        .query_row(
            "SELECT role = 'manager' FROM sessions WHERE id = ?1 AND archived_at IS NULL",
            [caller.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    match manager {
        Some(true) => Ok(caller),
        _ => Err(manager_only()),
    }
}

/// The current `incident` row as the output's `incident` value, if any. `pub(crate)` so the
/// `incident_hold` refusal (`pr open`/`push`/`rebase`) carries the same value as `incident status`.
pub(crate) fn row(conn: &Connection) -> Result<Option<Value>, CpdError> {
    let stored = conn
        .query_row(
            "SELECT started_at, started_by, reason, until_workflow FROM incident WHERE id = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((started_at, by, reason, until_workflow)) = stored else { return Ok(None) };
    let plans: Vec<String> = plans(conn)?.into_iter().map(|p| p.as_str().to_owned()).collect();
    Ok(Some(json!({"started_at": started_at, "by": by, "reason": reason, "plans": plans, "until_pr": until_workflow})))
}

/// Whether incident mode is on (contract §2.2): one row in `incident`.
pub fn is_on(conn: &Connection) -> bool {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM incident)", [], |r| {
        r.get::<_, bool>(0)
    })
    .unwrap_or(false)
}

/// Whether `plan` is held by incident mode (contract §2.2): the incident is on and `plan` is not
/// in `incident_plans`.
pub fn is_held(conn: &Connection, plan: &PlanId) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM incident) AND NOT EXISTS(SELECT 1 FROM incident_plans WHERE plan = ?1)",
        [plan.as_str()],
        |r| r.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

/// The set of Plans in the incident, `incident_plans` (contract §2.2).
pub fn plans(conn: &Connection) -> Result<Vec<PlanId>, CpdError> {
    let mut stmt = conn.prepare("SELECT plan FROM incident_plans ORDER BY plan")?;
    let plans = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|r| {
            PlanId::parse(&r?).map_err(|e| CpdError::bad_row(format!("incident_plans.plan: {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(plans)
}

/// The Lead Session a click on the incident pill opens (commands.md §9.13, hs-d678). The stored
/// `incident.lead` is used when set; a null one (a row written before the migration) falls back to
/// the alphabetically first Plan's Lead. `None` when there is no incident row or no resolvable Lead.
pub fn lead(conn: &Connection) -> Result<Option<SessionId>, CpdError> {
    let stored: Option<Option<String>> = conn
        .query_row("SELECT lead FROM incident WHERE id = 1", [], |r| r.get(0))
        .optional()?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    let id = match stored {
        Some(id) => id,
        None => match conn
            .query_row(
                "SELECT p.lead_session FROM incident_plans ip JOIN plans p ON p.id = ip.plan ORDER BY ip.plan LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            Some(id) => id,
            None => return Ok(None),
        },
    };
    SessionId::parse(&id)
        .map(Some)
        .map_err(|e| CpdError::bad_row(format!("incident.lead: {e}")))
}

/// The `<plan>/<key>` auto-end value split into its Plan and key. `usage` for anything else.
fn parse_until_workflow(value: &str) -> Result<(PlanId, String), CpdError> {
    let usage = || {
        CpdError::new(ErrorCode::Usage, "An --until-workflow is `<plan>/<key>` with a non-empty Plan and key.")
            .with("until_pr", value)
    };
    let (plan, key) = value.split_once('/').ok_or_else(usage)?;
    if plan.is_empty() || key.is_empty() || key.contains('/') {
        return Err(usage());
    }
    let plan = PlanId::parse(plan).map_err(|_| usage())?;
    Ok((plan, key.to_owned()))
}

/// `cpd incident start` (commands.md §25.1, hs-d664): one transaction. A fresh start (no `incident`
/// row) inserts the row and one `incident_plans` row per named Plan, requests the stop of every
/// `starting` or `running` Attempt with no `stop_request`, and appends one `incident-started` Event;
/// `reason` must be non-empty. A start while `incident` is live replaces `incident_plans` with
/// exactly the named Plans, keeps `started_at`, `started_by` and `reason`, runs no stop-request
/// sweep, and appends one `incident-plans` Event only when the set or `until_workflow` changed. `until_workflow`
/// is the optional `<plan>/<key>` auto-end Workflow; when it is absent the stored `until_plan`/`until_workflow`
/// are cleared.
pub fn start(
    store: &mut Store,
    caller: Option<&SessionId>,
    plans: &[PlanId],
    reason: &str,
    until_workflow: Option<&str>,
) -> Result<Output, CpdError> {
    if plans.is_empty() {
        return Err(CpdError::new(
            ErrorCode::Usage,
            "Incident mode needs at least one --plan.",
        ));
    }
    let until = until_workflow.map(parse_until_workflow).transpose()?;
    store.write("incident start", |tx| {
        let caller = require_manager(tx, caller)?;
        for plan in plans {
            let known: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM plans WHERE id = ?1)", [plan.as_str()], |r| r.get(0))?;
            if !known {
                return Err(CpdError::plan_not_found(plan));
            }
        }
        let lead: String = tx.query_row("SELECT lead_session FROM plans WHERE id = ?1", [plans[0].as_str()], |r| r.get(0))?;
        if let Some((until_plan, key)) = &until {
            if !plans.contains(until_plan) {
                return Err(CpdError::new(
                    ErrorCode::UntilWorkflowNotIncident,
                    format!("The --until-workflow Plan {until_plan} is not one of the incident's Plans."),
                )
                .with("plan", until_plan.as_str())
                .with("until_pr", until_workflow.unwrap_or_default()));
            }
            let known: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM workflows WHERE plan = ?1 AND key = ?2)",
                params![until_plan.as_str(), key],
                |r| r.get(0),
            )?;
            if !known {
                return Err(CpdError::new(
                    ErrorCode::WorkflowNotFound,
                    format!("No Workflow {until_plan}/{key}."),
                )
                .with("plan", until_plan.as_str())
                .with("pr", key.as_str()));
            }
        }

        let live: Option<Option<String>> = tx
            .query_row("SELECT until_workflow FROM incident WHERE id = 1", [], |r| r.get(0))
            .optional()?;
        if let Some(stored_until_workflow) = live {
            let new_until_workflow = until.as_ref().map(|(plan, key)| format!("{plan}/{key}"));
            let stored: Vec<PlanId> = self::plans(tx)?;
            let stored_set: BTreeSet<&str> = stored.iter().map(PlanId::as_str).collect();
            let new_set: BTreeSet<&str> = plans.iter().map(PlanId::as_str).collect();
            let set_changed = stored_set != new_set;
            let until_changed = new_until_workflow.as_deref() != stored_until_workflow.as_deref();
            if !set_changed && !until_changed {
                let incident = row(tx)?.expect("the row just read");
                return Ok(Output::success(json!({"changed": false, "incident": incident})));
            }
            if set_changed {
                tx.execute("DELETE FROM incident_plans", [])?;
                for plan in plans {
                    tx.execute("INSERT INTO incident_plans (plan, started_txn) VALUES (?1, ?2)", params![plan.as_str(), tx.id()])?;
                }
                tx.execute("UPDATE incident SET lead = ?1 WHERE id = 1", [lead.as_str()])?;
            }
            if until_changed {
                tx.execute(
                    "UPDATE incident SET until_plan = ?1, until_workflow = ?2 WHERE id = 1",
                    params![until.as_ref().map(|(plan, _)| plan.as_str()), new_until_workflow.as_deref()],
                )?;
            }
            let named: Vec<&str> = plans.iter().map(PlanId::as_str).collect();
            append(tx, NewEvent::incident_plans("Incident mode Plans updated.", caller.as_str(), &named, new_until_workflow.as_deref()))?;
            let incident = row(tx)?.expect("the row just updated");
            return Ok(Output::success(json!({"changed": true, "incident": incident})));
        }

        if reason.is_empty() {
            return Err(CpdError::new(ErrorCode::Usage, "The incident needs a reason of at least 1 character."));
        }
        let started_at = clock::now();
        tx.execute(
            "INSERT INTO incident (id, started_at, started_by, reason, started_txn, until_plan, until_workflow, lead)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                started_at,
                caller.as_str(),
                reason,
                tx.id(),
                until.as_ref().map(|(plan, _)| plan.as_str()),
                until_workflow,
                lead,
            ],
        )?;
        for plan in plans {
            tx.execute("INSERT INTO incident_plans (plan, started_txn) VALUES (?1, ?2)", params![plan.as_str(), tx.id()])?;
        }
        tx.execute(
            "UPDATE attempts SET stop_request = 'cancelled'
             WHERE status IN ('starting', 'running') AND stop_request IS NULL",
            [],
        )?;
        let named: Vec<&str> = plans.iter().map(PlanId::as_str).collect();
        append(tx, NewEvent::incident_started("Incident mode on.", caller.as_str(), reason, &started_at, &named, until_workflow))?;
        let incident = row(tx)?.expect("the row just inserted");
        Ok(Output::success(json!({"changed": true, "incident": incident})))
    })
}

/// `cpd incident end` (commands.md §25.2): one transaction that deletes the row and every
/// `incident_plans` row and appends one `incident-ended` Event with `cause` `"manual"`.
pub fn end(store: &mut Store, caller: Option<&SessionId>) -> Result<Output, CpdError> {
    store.write("incident end", |tx| {
        let caller = require_manager(tx, caller)?;
        if row(tx)?.is_none() {
            return Ok(Output::success(json!({"changed": false, "incident": null})));
        }
        tx.execute("DELETE FROM incident_plans", [])?;
        tx.execute("DELETE FROM incident WHERE id = 1", [])?;
        let ended_at = clock::now();
        append(
            tx,
            NewEvent::incident_ended("Incident mode off.", caller.as_str(), &ended_at, "manual"),
        )?;
        Ok(Output::success(json!({"changed": true, "incident": null})))
    })
}

/// `cpd incident status` (commands.md §25.3): a read of the row and its Plan set, for any caller.
pub fn status(store: &mut Store) -> Result<Output, CpdError> {
    let incident = store.read(row)?;
    Ok(Output::success(
        json!({"on": incident.is_some(), "incident": incident}),
    ))
}

/// The auto-end (contract §2.2, hs-d643): called in a write transaction right after a merge is
/// recorded. When the stored `until_workflow` Workflow is merged, deletes the `incident` row and every
/// `incident_plans` row and appends one `incident-ended` Event (`cause` `"until-workflow"`, `by`
/// `"engine"`), returning true; false when incident mode is off, no `until_workflow` is stored, or the
/// Workflow is not merged.
pub fn end_if_until_workflow_merged(tx: &WriteTxn<'_>) -> Result<bool, CpdError> {
    let stored = tx
        .query_row(
            "SELECT until_plan, until_workflow FROM incident WHERE id = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?;
    let Some((Some(until_plan), Some(until_workflow))) = stored else { return Ok(false) };
    let key = until_workflow.split_once('/').map_or(until_workflow.as_str(), |(_, key)| key);
    let merged: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM workflows WHERE plan = ?1 AND key = ?2 AND status = 'merged')",
        params![until_plan, key],
        |r| r.get(0),
    )?;
    if !merged {
        return Ok(false);
    }
    tx.execute("DELETE FROM incident_plans", [])?;
    tx.execute("DELETE FROM incident WHERE id = 1", [])?;
    let ended_at = clock::now();
    append(tx, NewEvent::incident_ended("Incident mode off.", "engine", &ended_at, "until-workflow"))?;
    Ok(true)
}

/// The coarse human duration from the incident row's `started_at` to `now` (hs-d640, contract
/// §6.1): under 60 s `{n}s`; under 1 h `{n}m`; under 1 d `{n}h {n}m`; otherwise `{n}d {n}h`;
/// values truncated, a `started_at` in the future reads `0s`. `started_at` is RFC 3339, the format
/// `clock::timestamp` writes; anything else is a loud `StoreFailed`.
pub fn elapsed(started_at: &str, now: time::OffsetDateTime) -> Result<String, CpdError> {
    let started =
        time::OffsetDateTime::parse(started_at, &time::format_description::well_known::Rfc3339)
            .map_err(|e| {
                CpdError::new(
            ErrorCode::StoreFailed,
            format!("The incident holds a started_at that is not RFC 3339: {started_at:?} ({e})."),
        )
        .with("started_at", started_at)
            })?;
    let seconds = (now - started).whole_seconds().max(0);
    Ok(if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h {}m", seconds / 3_600, (seconds % 3_600) / 60)
    } else {
        format!("{}d {}h", seconds / 86_400, (seconds % 86_400) / 3_600)
    })
}
