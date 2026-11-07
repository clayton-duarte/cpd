//! The `jira` source: a stub until its Workflow (swr.md §14).

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::{Cx, ExecOutcome, QueryKey, Source};
use crate::clock::timestamp;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::host::deadline::Failed;
use crate::host::runner::Call;
use crate::ids::{PlanId, WorkflowKey};
use crate::jira;
use crate::policy::{McpServer, Policy};
use crate::store::{Store, WriteTxn};
use crate::swr::cache::parse;
use crate::swr::endpoint::{self, Kind};
use crate::swr::outbox::{self, Coalesce, Mutation, Op};
use crate::swr::overlay;

/// The effective Jira fields of a ticket (swr-jira.md §6): the cached row with the overlay applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub status_name: String,
    pub status_category: String,
}

/// What a run needs from the policy, read when the row runs (swr-jira.md §8 step 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub target_name: String,
    pub later_names: Vec<String>,
    pub estimate_fields: Vec<String>,
}

/// The ranks, in order (swr-jira.md §4).
const TARGETS: [&str; 3] = ["in_progress", "in_review", "done"];

/// The rank of a target (swr-jira.md §4): `in_progress` 1, `in_review` 2, `done` 3.
pub fn rank(target: &str) -> Option<i64> {
    TARGETS
        .iter()
        .position(|t| *t == target)
        .map(|i| i as i64 + 1)
}

/// The query key and outbox key of a ticket (J1).
pub fn key_of(ticket: &str) -> String {
    format!("jira:issue:{ticket}")
}

fn category_of(target: &str) -> &'static str {
    if target == "done" {
        "done"
    } else {
        "indeterminate"
    }
}

fn unknown_target(target: &str) -> CpdError {
    CpdError::new(
        ErrorCode::Internal,
        format!("{target:?} is not a Jira target."),
    )
}

/// [`enqueue_transition`] inside the caller's transaction.
pub fn enqueue_transition_tx(
    tx: &WriteTxn<'_>,
    now: OffsetDateTime,
    ticket: &str,
    target: &str,
    status_name: Option<&str>,
    context: Value,
) -> Result<i64, CpdError> {
    let rank = rank(target).ok_or_else(|| unknown_target(target))?;
    let patch = status_name
        .map(|name| json!({"status_name": name, "status_category": category_of(target)}));
    outbox::enqueue_tx(
        tx,
        now,
        Op {
            system: "jira".into(),
            target_key: key_of(ticket),
            op_kind: "transition".into(),
            vars: json!({"ticket": ticket, "target": target, "rank": rank}),
            patch,
            context: Some(context),
        },
        Coalesce::Forward(rank),
    )
}

/// Enqueue the `transition` op of `ticket` with `Coalesce::Forward(rank)` (swr-jira.md §2, §4, §7).
/// `status_name` is the overlay's name (`None`: no overlay, §6); `context` is `{plan, workflow}`.
pub fn enqueue_transition(
    store: &mut Store,
    now: OffsetDateTime,
    ticket: &str,
    target: &str,
    status_name: Option<&str>,
    context: Value,
) -> Result<i64, CpdError> {
    store.write("jira enqueue", |tx| {
        enqueue_transition_tx(tx, now, ticket, target, status_name, context)
    })
}

/// The enqueue of the five sites (swr-jira.md §7), in their transaction: the transition of Workflow `workflow` of `plan` to
/// `target`. A Workflow with no ticket gets a row with no `target_key`, which `claim_next` never takes, until
/// [`adopt_ticket`]. No overlay: the sites hold no policy (J2 is not applied there).
pub fn enqueue_for_workflow(
    tx: &WriteTxn<'_>,
    plan: &str,
    workflow: &str,
    target: &str,
) -> Result<(), CpdError> {
    let now = txn_time(tx)?;
    let ticket: Option<String> = tx
        .query_row(
            "SELECT jira FROM workflows WHERE plan = ?1 AND key = ?2",
            [plan, workflow],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let context = json!({"plan": plan, "workflow": workflow});
    match ticket {
        Some(ticket) => {
            enqueue_transition_tx(tx, now, &ticket, target, None, context)?;
        }
        None => {
            let rank = rank(target).ok_or_else(|| unknown_target(target))?;
            let exists = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM swr_mutations WHERE system = 'jira' AND target_key IS NULL AND status = 'pending'
                   AND json_extract(vars_json, '$.target') = ?3
                   AND json_extract(context_json, '$.plan') = ?1 AND json_extract(context_json, '$.workflow') = ?2)",
                params![plan, workflow, target],
                |r| r.get::<_, bool>(0),
            )?;
            if !exists {
                tx.execute(
                    "INSERT INTO swr_mutations (system, target_key, op_kind, vars_json, status, submitted_at, context_json)
                     VALUES ('jira', NULL, 'transition', ?1, 'pending', ?2, ?3)",
                    params![
                        json!({"target": target, "rank": rank}).to_string(),
                        timestamp(now),
                        context.to_string()
                    ],
                )?;
            }
        }
    }
    Ok(())
}

/// A ticket was set on the Workflow (swr-jira.md §7): each of its `pending` rows that has no ticket, or another one, is
/// enqueued again under the ticket, in rank order, with the Forward rule, and the old rows are `superseded`. Today's
/// queue reads the Workflow's ticket when a row runs, so a row follows a changed ticket as well. The target of the
/// Workflow's current status is enqueued too (merged: `done`; open: `in_review`; dispatch with a started Job, one with an
/// Attempt: `in_progress`; waiting, cancelled: none); the Forward rule makes an already-covered target a no-op.
pub fn adopt_ticket(
    tx: &WriteTxn<'_>,
    plan: &str,
    workflow: &str,
    ticket: &str,
) -> Result<(), CpdError> {
    let now = txn_time(tx)?;
    let mut stmt = tx.prepare(
        "SELECT id, json_extract(vars_json, '$.target') FROM swr_mutations
         WHERE system = 'jira' AND status = 'pending' AND (target_key IS NULL OR target_key <> ?3)
           AND json_extract(context_json, '$.plan') = ?1 AND json_extract(context_json, '$.workflow') = ?2
         ORDER BY json_extract(vars_json, '$.rank'), id",
    )?;
    let rows = stmt
        .query_map([plan, workflow, &key_of(ticket)], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for (id, target) in rows {
        enqueue_transition_tx(
            tx,
            now,
            ticket,
            &target,
            None,
            json!({"plan": plan, "workflow": workflow}),
        )?;
        tx.execute(
            "UPDATE swr_mutations SET status = 'superseded' WHERE id = ?1",
            [id],
        )?;
    }
    let status: Option<String> = tx
        .query_row(
            "SELECT status FROM workflows WHERE plan = ?1 AND key = ?2",
            [plan, workflow],
            |r| r.get(0),
        )
        .optional()?;
    let target = match status.as_deref() {
        Some("merged") => Some("done"),
        Some("open") => Some("in_review"),
        Some("dispatch") => {
            let started: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
                   WHERE j.plan = ?1 AND j.workflow = ?2)",
                [plan, workflow],
                |r| r.get(0),
            )?;
            started.then_some("in_progress")
        }
        _ => None,
    };
    if let Some(target) = target {
        enqueue_transition_tx(
            tx,
            now,
            ticket,
            target,
            None,
            json!({"plan": plan, "workflow": workflow}),
        )?;
    }
    Ok(())
}

/// The instant of the transaction.
fn txn_time(tx: &WriteTxn<'_>) -> Result<OffsetDateTime, CpdError> {
    let at: String = tx.query_row("SELECT at FROM txns WHERE id = cpd_txn()", [], |r| r.get(0))?;
    parse(Some(at))
        .map_err(CpdError::from)?
        .ok_or_else(|| CpdError::new(ErrorCode::Internal, "The transaction has no time."))
}

/// The skip rule on the cached row (swr-jira.md §5): `true` when the row exists and is not behind.
pub fn skips_on_cache(store: &mut Store, ticket: &str, wanted: &Wanted) -> Result<bool, CpdError> {
    let row: Option<(String, String)> = store.read(|c| {
        Ok(c.query_row(
            "SELECT status_name, status_category FROM swr_jira_issues WHERE ticket = ?1",
            [ticket],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
    })?;
    Ok(row.is_some_and(|(name, category)| {
        name == wanted.target_name || category == "done" || wanted.later_names.contains(&name)
    }))
}

/// The effective state of `ticket`: `swr_jira_issues` with `patch_json` applied in `submitted_at` order (§6).
pub fn effective(store: &mut Store, ticket: &str) -> Result<Option<Issue>, CpdError> {
    let row: Option<(String, String)> = store.read(|c| {
        Ok(c.query_row(
            "SELECT status_name, status_category FROM swr_jira_issues WHERE ticket = ?1",
            [ticket],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
    })?;
    let Some((mut status_name, mut status_category)) = row else {
        return Ok(None);
    };
    for m in overlay::active(store, "jira", &key_of(ticket))? {
        let Some(patch) = m.patch_json else { continue };
        let patch: Value = serde_json::from_str(&patch)
            .map_err(|e| CpdError::new(ErrorCode::Internal, format!("A bad Jira patch: {e}.")))?;
        if let Some(name) = patch["status_name"].as_str() {
            status_name = name.to_owned();
        }
        if let Some(category) = patch["status_category"].as_str() {
            status_category = category.to_owned();
        }
    }
    Ok(Some(Issue {
        status_name,
        status_category,
    }))
}

const OBSERVED: &str = "(EXISTS (SELECT 1 FROM workflows w WHERE w.jira = swr_jira_issues.ticket
                          AND w.status IN ('waiting', 'dispatch', 'open'))
                        OR EXISTS (SELECT 1 FROM swr_mutations m WHERE m.target_key = 'jira:issue:' || swr_jira_issues.ticket
                          AND m.status IN ('pending', 'in_flight', 'revalidating')))";

/// Set `inactive_at` on the `swr_jira_issues` rows nothing observes, and clear it on the rest (swr-jira.md §10).
/// Rows already right open no write transaction.
pub fn mark_inactive(store: &mut Store, now: OffsetDateTime) -> Result<(), CpdError> {
    let at = timestamp(now);
    let clear = format!("inactive_at IS NOT NULL AND {OBSERVED}");
    let set = format!("inactive_at IS NULL AND NOT {OBSERVED}");
    let todo = store.read(|c| {
        Ok(c.query_row(
            &format!("SELECT EXISTS (SELECT 1 FROM swr_jira_issues WHERE {clear} OR {set})"),
            [],
            |r| r.get::<_, bool>(0),
        )?)
    })?;
    if !todo {
        return Ok(());
    }
    store.write("jira mark inactive", |tx| {
        tx.execute(
            &format!("UPDATE swr_jira_issues SET inactive_at = NULL WHERE {clear}"),
            [],
        )?;
        tx.execute(
            &format!("UPDATE swr_jira_issues SET inactive_at = ?1 WHERE {set}"),
            [at],
        )?;
        Ok::<(), CpdError>(())
    })
}

/// The policy name of `target` for Workflow `workflow` of `plan`, resolved now.
pub fn target_name(
    store: &mut Store,
    home: &crate::home::CpdHome,
    m: &Mutation,
) -> Result<String, CpdError> {
    let (plan, workflow, target) = ids_of(m)?;
    let (plan, key) = (PlanId::parse(&plan)?, WorkflowKey::parse(&workflow)?);
    let policy = store.read(|c| crate::pr::policy_of_workflow(c, home, &plan, &key))?;
    Ok(wanted_of(&policy, &target)?.target_name)
}

/// The `plan`, `workflow` and `target` of a row.
fn ids_of(m: &Mutation) -> Result<(String, String, String), CpdError> {
    let bad = |what: &str| {
        CpdError::new(
            ErrorCode::Internal,
            format!("A Jira transition has bad {what}."),
        )
    };
    let context: Value = serde_json::from_str(m.context_json.as_deref().unwrap_or("{}"))
        .map_err(|_| bad("context"))?;
    let vars: Value = serde_json::from_str(&m.vars_json).map_err(|_| bad("vars"))?;
    match (
        context["plan"].as_str(),
        context["workflow"].as_str(),
        vars["target"].as_str(),
    ) {
        (Some(p), Some(w), Some(t)) => Ok((p.to_owned(), w.to_owned(), t.to_owned())),
        _ => Err(bad("plan, workflow or target")),
    }
}

/// Park a refused transition (swr-jira.md §8): `failed`, `refused_with` in its context, and the one plan-scoped `error`
/// Event `jira_refused` (`source` is its source), in one transaction.
pub fn park_refused(
    store: &mut Store,
    m: &Mutation,
    now: OffsetDateTime,
    source: &str,
    reason: &str,
    status_name: &str,
) -> Result<(), CpdError> {
    let vars: Value = serde_json::from_str(&m.vars_json)
        .map_err(|e| CpdError::new(ErrorCode::Internal, format!("Bad Jira vars: {e}.")))?;
    let context: Value = serde_json::from_str(m.context_json.as_deref().unwrap_or("{}"))
        .map_err(|e| CpdError::new(ErrorCode::Internal, format!("Bad Jira context: {e}.")))?;
    let ticket = vars["ticket"].as_str().unwrap_or_default();
    let target = vars["target"].as_str().unwrap_or_default();
    let (plan, workflow) = (
        context["plan"].as_str().unwrap_or_default(),
        context["workflow"].as_str().unwrap_or_default(),
    );
    let error = format!("jira_refused: {reason}");
    let text = format!(
        "Jira refused to move ticket {ticket} of Workflow {workflow} of Plan {plan} to `{status_name}`: {reason}. Nothing is tried again until the Workflow's ticket (cpd workflow edit {workflow} --jira <KEY>) or the policy key pr.jira.{target} changes."
    );
    let details = json!({
        "id": m.id, "plan": plan, "pr": workflow, "ticket": ticket, "target": target, "status": status_name, "reason": reason,
    });
    let event = NewEvent::error(
        text.clone(),
        source,
        ErrorCode::JiraRefused.as_str(),
        &text,
        details,
    )
    .plan(PlanId::parse(plan)?);
    store.write("jira park refused", |tx| {
        tx.execute(
            "UPDATE swr_mutations SET status = 'failed', error = ?2, completed_at = ?3,
               context_json = json_set(coalesce(context_json, '{}'), '$.refused_with', ?4) WHERE id = ?1",
            params![
                m.id,
                error,
                timestamp(now),
                format!("{ticket}|{status_name}")
            ],
        )?;
        append(tx, event)?;
        Ok::<(), CpdError>(())
    })
}

/// Enqueue again each parked (refused) transition whose ticket or status name now differs from what was refused
/// (swr-jira.md §9), once: the parked row's `refused_with` moves to `reopened_with`. Returns the number enqueued.
pub fn reopen_refused(
    store: &mut Store,
    home: &crate::home::CpdHome,
    now: OffsetDateTime,
) -> Result<usize, CpdError> {
    let parked: Vec<Mutation> = store.read(|c| {
        let mut stmt = c.prepare(
            "SELECT id, system, target_key, op_kind, vars_json, patch_json, status, submitted_at, started_at,
                    completed_at, next_attempt_at, attempt_count, error, context_json
             FROM swr_mutations WHERE system = 'jira' AND status = 'failed'
               AND json_extract(context_json, '$.refused_with') IS NOT NULL ORDER BY id",
        )?;
        Ok(stmt
            .query_map([], outbox::mutation)?
            .collect::<Result<Vec<_>, _>>()?)
    })?;
    let mut enqueued = 0;
    for m in parked {
        let vars: Value = serde_json::from_str(&m.vars_json).unwrap_or(Value::Null);
        let context: Value =
            serde_json::from_str(m.context_json.as_deref().unwrap_or("{}")).unwrap_or(Value::Null);
        let (Some(plan), Some(workflow), Some(target), Some(tried)) = (
            context["plan"].as_str(),
            context["workflow"].as_str(),
            vars["target"].as_str(),
            context["refused_with"].as_str(),
        ) else {
            continue;
        };
        let ticket: Option<String> = store.read(|c| {
            Ok(c.query_row(
                "SELECT jira FROM workflows WHERE plan = ?1 AND key = ?2",
                [plan, workflow],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
        })?;
        let Some(ticket) = ticket else { continue };
        // A policy that does not resolve is reported when a row of the Workflow runs; here it only holds the reopen.
        let Ok(name) = target_name(store, home, &m) else {
            continue;
        };
        let rank = rank(target).ok_or_else(|| unknown_target(target))?;
        if name.is_empty() || format!("{ticket}|{name}") == tried {
            continue;
        }
        let key = key_of(&ticket);
        let blocked = store.read(|c| {
            Ok(c.query_row(
                "SELECT EXISTS (SELECT 1 FROM swr_mutations WHERE system = 'jira' AND target_key = ?1
                   AND status IN ('pending', 'in_flight') AND json_extract(vars_json, '$.rank') >= ?2)",
                params![key, rank],
                |r| r.get::<_, bool>(0),
            )?)
        })?;
        let (id, with) = (m.id, format!("{ticket}|{name}"));
        store.write("jira reopen", |tx| {
            if !blocked {
                enqueue_transition_tx(
                    tx,
                    now,
                    &ticket,
                    target,
                    Some(&name),
                    json!({"plan": plan, "workflow": workflow}),
                )?;
            }
            tx.execute(
                "UPDATE swr_mutations SET context_json = json_set(json_remove(context_json, '$.refused_with'), '$.reopened_with', ?2)
                 WHERE id = ?1",
                params![id, with],
            )?;
            Ok::<(), CpdError>(())
        })?;
        enqueued += usize::from(!blocked);
    }
    Ok(enqueued)
}

/// The mutation `id` is `done` without a Jira write (swr-jira.md §8 `skip`).
fn skip(store: &mut Store, id: i64, now: OffsetDateTime) -> Result<(), CpdError> {
    store.write("jira skip", |tx| {
        tx.execute(
            "UPDATE swr_mutations SET status = 'done', completed_at = ?2 WHERE id = ?1",
            params![id, timestamp(now)],
        )?;
        Ok::<(), CpdError>(())
    })
}

/// One transaction (J5): the facts of the helper's read go to the cache, and the mutation is `done`.
fn settle(
    store: &mut Store,
    m: &Mutation,
    ticket: &str,
    answer: &jira::Answer,
    cached: (Option<&str>, Option<&str>),
    now: OffsetDateTime,
) -> Result<(), CpdError> {
    let at = timestamp(now);
    let key = key_of(ticket);
    let estimates = answer.estimates.to_string();
    store.write("jira settle", |tx| {
        tx.execute(
            "INSERT INTO swr_jira_issues (ticket, cloud_id, summary, status_name, status_category, estimates_json, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (ticket) DO UPDATE SET cloud_id = excluded.cloud_id, summary = excluded.summary,
               status_name = excluded.status_name, status_category = excluded.status_category,
               estimates_json = excluded.estimates_json, synced_at = excluded.synced_at, inactive_at = NULL",
            params![ticket, answer.cloud_id, answer.summary, answer.status_name, answer.status_category, estimates, at],
        )?;
        if cached.0.is_none() || cached.1.is_none() {
            tx.execute(
                "INSERT INTO swr_atlassian (id, cloud_id, account_id, fetched_at) VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT (id) DO UPDATE SET cloud_id = excluded.cloud_id, account_id = excluded.account_id,
                   fetched_at = excluded.fetched_at",
                params![
                    cached.0.unwrap_or(&answer.cloud_id),
                    cached.1.unwrap_or(&answer.account_id),
                    at
                ],
            )?;
        }
        tx.execute(
            "INSERT INTO swr_queries (key, system, status, fetched_at, invalidated) VALUES (?1, 'jira', 'success', ?2, 0)
             ON CONFLICT (key) DO UPDATE SET status = 'success', fetched_at = excluded.fetched_at, invalidated = 0,
               stale_after = NULL, last_error = NULL",
            params![key, at],
        )?;
        tx.execute(
            "UPDATE swr_mutations SET status = 'done', completed_at = ?2 WHERE id = ?1",
            params![m.id, at],
        )?;
        Ok::<(), CpdError>(())
    })
}

/// The `swr_atlassian` site id and account id when fetched less than 24 h before `now`.
fn cached_ids(
    store: &mut Store,
    now: OffsetDateTime,
) -> Result<(Option<String>, Option<String>), CpdError> {
    let row: Option<(Option<String>, Option<String>, Option<String>)> = store.read(|c| {
        Ok(c.query_row(
            "SELECT cloud_id, account_id, fetched_at FROM swr_atlassian WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
    })?;
    let Some((cloud_id, account_id, fetched)) = row else {
        return Ok((None, None));
    };
    let fetched = parse(fetched).map_err(CpdError::from)?;
    if fetched.is_some_and(|at| now - at < time::Duration::hours(24)) {
        Ok((cloud_id, account_id))
    } else {
        Ok((None, None))
    }
}

fn clear_ids(store: &mut Store) -> Result<(), CpdError> {
    store.write("jira clear cloud id", |tx| {
        tx.execute(
            "UPDATE swr_atlassian SET cloud_id = NULL, account_id = NULL WHERE id = 1",
            [],
        )?;
        Ok::<(), CpdError>(())
    })
}

/// What `execute` reports for an error: `<code>: <message>`, or `jira_refused: <reason>`.
fn failed(e: &CpdError) -> ExecOutcome {
    let error = if e.code() == ErrorCode::JiraRefused {
        let reason = e
            .details()
            .get("reason")
            .and_then(|r| r.as_str())
            .unwrap_or("unknown")
            .to_owned();
        format!("jira_refused: {reason}")
    } else {
        format!("{}: {}", e.code().as_str(), e.message())
    };
    ExecOutcome::Failed { error }
}

/// The wanted names of a policy for `target` (swr-jira.md §3.1).
pub fn wanted_of(policy: &Policy, target: &str) -> Result<Wanted, CpdError> {
    let jira = &policy.pr.jira;
    let name = |t: &str| match t {
        "in_progress" => jira.in_progress.clone(),
        "in_review" => jira.in_review.clone(),
        _ => jira.done.clone(),
    };
    let rank = rank(target).ok_or_else(|| unknown_target(target))? as usize;
    Ok(Wanted {
        target_name: name(target),
        later_names: TARGETS[rank..]
            .iter()
            .map(|t| name(t))
            .filter(|n| !n.is_empty())
            .collect(),
        estimate_fields: if target == "done" {
            jira.estimate_fields.clone()
        } else {
            Vec::new()
        },
    })
}

pub struct Jira;

impl Jira {
    /// Run one claimed transition on the Runner port (swr-jira.md §8 steps 4 to 6): the cache test, then `helper` with the
    /// request of §3.1 as its stdin (`cloud_id` from `swr_atlassian` when fresh), then the answer mapped by the §8 table.
    /// `Done` means the mutation is already settled here; `Failed` carries `<code>: <message>` for the engine.
    pub fn run(
        &self,
        cx: &mut Cx<'_>,
        m: &Mutation,
        wanted: &Wanted,
        server: &McpServer,
        helper: &Call,
    ) -> ExecOutcome {
        let now = cx.ports.clock.utc();
        match self.try_run(cx, m, wanted, server, helper, now) {
            Ok(outcome) => outcome,
            Err(e) => failed(&e),
        }
    }

    fn try_run(
        &self,
        cx: &mut Cx<'_>,
        m: &Mutation,
        wanted: &Wanted,
        server: &McpServer,
        helper: &Call,
        now: OffsetDateTime,
    ) -> Result<ExecOutcome, CpdError> {
        let vars: Value = serde_json::from_str(&m.vars_json)
            .map_err(|e| CpdError::new(ErrorCode::Internal, format!("Bad Jira vars: {e}.")))?;
        let (Some(ticket), Some(target)) = (vars["ticket"].as_str(), vars["target"].as_str())
        else {
            return Err(CpdError::new(
                ErrorCode::Internal,
                "A Jira transition has no ticket or target.",
            ));
        };
        if skips_on_cache(cx.store, ticket, wanted)? {
            skip(cx.store, m.id, now)?;
            return Ok(ExecOutcome::Done);
        }
        let (cloud_id, account_id) = cached_ids(cx.store, now)?;
        let request = jira::Request {
            ticket: ticket.to_owned(),
            target: target.to_owned(),
            cloud_id: cloud_id.clone(),
            account_id: account_id.clone(),
            target_name: wanted.target_name.clone(),
            later_names: wanted.later_names.clone(),
            estimate_fields: wanted.estimate_fields.clone(),
        };
        let mut call = helper.clone();
        call.stdin = Some(jira::stdin(server, &request));
        let argv: Vec<String> = std::iter::once(call.program.clone())
            .chain(call.args.iter().cloned())
            .collect();
        let ran = match cx.ports.runner.run(&call) {
            Ok(out) => jira::answer(&out, &argv),
            Err(Failed::Io(e)) => Err(CpdError::new(
                ErrorCode::SpawnFailed,
                format!("Cannot run {}: {e}.", argv.join(" ")),
            )
            .with("argv", json!(argv))),
            Err(Failed::Timeout(e)) => Err(e),
        };
        match ran {
            Ok(answer) => {
                settle(
                    cx.store,
                    m,
                    ticket,
                    &answer,
                    (cloud_id.as_deref(), account_id.as_deref()),
                    now,
                )?;
                endpoint::record_ok(cx.store, "jira", now)?;
                Ok(ExecOutcome::Done)
            }
            Err(e) => {
                let reason = e
                    .details()
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .map(str::to_owned);
                match e.code() {
                    ErrorCode::JiraRefused => endpoint::record_ok(cx.store, "jira", now)?,
                    ErrorCode::JiraAuth | ErrorCode::JiraNotConfigured => endpoint::record_failure(
                        cx.store,
                        "jira",
                        now,
                        e.message(),
                        Kind::Permanent,
                    )?,
                    _ => endpoint::record_failure(
                        cx.store,
                        "jira",
                        now,
                        e.message(),
                        Kind::Transient,
                    )?,
                }
                let stale_id = (e.code() == ErrorCode::JiraRefused
                    && reason.as_deref() == Some("issue_not_found"))
                    || e.code() == ErrorCode::JiraFailed;
                if cloud_id.is_some() && stale_id {
                    clear_ids(cx.store)?;
                }
                Err(e)
            }
        }
    }

    /// The whole of a claimed row (swr-jira.md §8 steps 2 to 6): the Workflow, the policy, the server and the helper.
    pub fn execute_row(&self, cx: &mut Cx<'_>, m: &Mutation) -> Result<ExecOutcome, CpdError> {
        let now = cx.ports.clock.utc();
        let context: Value = m
            .context_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| CpdError::new(ErrorCode::Internal, format!("Bad Jira context: {e}.")))?
            .unwrap_or(Value::Null);
        let vars: Value = serde_json::from_str(&m.vars_json)
            .map_err(|e| CpdError::new(ErrorCode::Internal, format!("Bad Jira vars: {e}.")))?;
        let (Some(plan), Some(workflow), Some(target)) = (
            context["plan"].as_str(),
            context["workflow"].as_str(),
            vars["target"].as_str(),
        ) else {
            return Err(CpdError::new(
                ErrorCode::Internal,
                "A Jira transition has no plan, workflow or target.",
            ));
        };
        let status: Option<String> = cx.store.read(|c| {
            Ok(c.query_row(
                "SELECT status FROM workflows WHERE plan = ?1 AND key = ?2",
                [plan, workflow],
                |r| r.get(0),
            )
            .optional()?)
        })?;
        if status.as_deref().is_none_or(|s| s == "cancelled") {
            skip(cx.store, m.id, now)?;
            return Ok(ExecOutcome::Done);
        }
        let plan_id = PlanId::parse(plan)?;
        let key = WorkflowKey::parse(workflow)?;
        let home = cx.home.clone();
        let policy = cx
            .store
            .read(|c| crate::pr::policy_of_workflow(c, &home, &plan_id, &key))?;
        let wanted = wanted_of(&policy, target)?;
        if wanted.target_name.is_empty() {
            skip(cx.store, m.id, now)?;
            return Ok(ExecOutcome::Done);
        }
        let server = match jira::server(cx.home) {
            Ok(server) => server,
            Err(e) => {
                endpoint::record_failure(cx.store, "jira", now, e.message(), Kind::Permanent)?;
                return Err(e);
            }
        };
        let helper = jira::helper(cx.config)?;
        Ok(self.run(cx, m, &wanted, &server, &helper))
    }
}

impl Source for Jira {
    fn system(&self) -> &'static str {
        "jira"
    }
    fn endpoints(&self) -> &'static [&'static str] {
        &["jira"]
    }
    fn due(&self, _cx: &mut Cx<'_>, _now: OffsetDateTime) -> Result<Vec<QueryKey>, CpdError> {
        Ok(Vec::new())
    }
    fn fetch(
        &self,
        _cx: &mut Cx<'_>,
        _keys: &[QueryKey],
        _poll_started_at: OffsetDateTime,
    ) -> Result<(), CpdError> {
        Ok(())
    }
    fn execute(&self, cx: &mut Cx<'_>, m: &Mutation) -> ExecOutcome {
        self.execute_row(cx, m).unwrap_or_else(|e| failed(&e))
    }
    fn coalesce(&self, _op_kind: &str) -> Coalesce {
        // The rank is the vars' `rank`; the op is always enqueued with `Forward` by `enqueue_transition`.
        Coalesce::Forward(1)
    }
}
