//! E4, completion (contract §3.7), per PR: a PR in status `dispatch` with no
//! Job `draft`, `queued` or `running`, at least one `merged`, and
//! `all_merged_at` null gets `all-merged` `{pr, merged, cancelled}` and that
//! `all_merged_at`, once. A `draft` is an unapproved batch: the PR waits for it.
//! `all-merged` is steered to the Lead (§5.3, hs-d334); a PR still in `dispatch`
//! `pr_open_escalate_minutes` after its clock start is escalated to the Context's Manager,
//! once per all-merged moment ([`escalate`]). A stacked PR's clock starts only once its base PR is
//! open or merged ([`clock_start`], hs-d301).

use super::SOURCE;
use super::engine::Engine;
use super::report::{About, Failure, Step};
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, NewEvent};
use crate::ids::{ContextId, PlanId, PrKey, SessionId};
use crate::store::WriteTxn;
use rusqlite::OptionalExtension;
use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The PRs E4 is for; `?1` and `?2` are one PR's Plan and key, or NULL for every PR.
const DONE: &str = "SELECT r.plan, r.key FROM prs r
  WHERE (?1 IS NULL OR r.plan = ?1) AND (?2 IS NULL OR r.key = ?2)
    AND r.status = 'dispatch' AND r.all_merged_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = r.plan AND j.pr = r.key AND j.status IN ('draft', 'queued', 'running'))
    AND EXISTS (SELECT 1 FROM jobs j WHERE j.plan = r.plan AND j.pr = r.key AND j.status = 'merged')
  ORDER BY r.plan, r.key";

pub(super) fn run(engine: &mut Engine) {
    complete_all(engine);
    escalate_all(engine);
}

fn complete_all(engine: &mut Engine) {
    let prs = engine.store.read(|conn| {
        let mut stmt = conn.prepare(DONE)?;
        let rows = stmt
            .query_map([None::<&str>, None::<&str>], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter().map(|(plan, key)| Ok((PlanId::parse(plan)?, PrKey::parse(key)?))).collect::<Result<Vec<_>, CpdError>>()
    });
    let prs = match prs {
        Ok(prs) => {
            engine.clear(Step::E4, &About::Machine);
            prs
        }
        Err(e) => return engine.fail(Step::E4, About::Machine, &e),
    };
    for (plan, key) in prs {
        let about = About::Plan(plan.clone());
        match engine.store.write(SOURCE, |tx| complete(tx, &plan, &key)) {
            Ok(done) => {
                if done {
                    engine.log.line(&format!("E4: every Job of PR {key} of Plan {plan} is done: all-merged"));
                }
                engine.clear(Step::E4, &about);
            }
            Err(e) => engine.fail(Step::E4, about, &e),
        }
    }
}

/// The keys of the Jobs of PR `pr` of `plan` in `status`, in key order.
fn keys(tx: &WriteTxn<'_>, plan: &PlanId, pr: &PrKey, status: &str) -> Result<Vec<String>, CpdError> {
    let mut stmt = tx.prepare("SELECT key FROM jobs WHERE plan = ?1 AND pr = ?2 AND status = ?3 ORDER BY key")?;
    let keys = stmt.query_map([plan.as_str(), pr.as_str(), status], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(keys)
}

/// Checks the condition again inside the transaction, then sets
/// `all_merged_at` and appends `all-merged`. False when it no longer holds.
fn complete(tx: &WriteTxn<'_>, plan: &PlanId, pr: &PrKey) -> Result<bool, CpdError> {
    let still: bool = tx.query_row(&format!("SELECT EXISTS ({DONE})"), [plan.as_str(), pr.as_str()], |r| r.get(0))?;
    if !still {
        return Ok(false);
    }
    tx.execute(
        "UPDATE prs SET all_merged_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE plan = ?1 AND key = ?2 AND all_merged_at IS NULL",
        [plan.as_str(), pr.as_str()],
    )?;
    let merged = keys(tx, plan, pr, "merged")?;
    let cancelled = keys(tx, plan, pr, "cancelled")?;
    let failed = keys(tx, plan, pr, "failed")?;
    let mut text = format!("No Job of PR {pr} of Plan {plan} is draft, queued or running.");
    if failed.is_empty() {
        text += " Open the pull request now: it waits for no slot and no other work.";
    }
    text += &format!(" Merged: {}.", merged.join(", "));
    if !cancelled.is_empty() {
        text += &format!(" Cancelled: {}.", cancelled.join(", "));
    }
    if failed.is_empty() {
        text += &format!(" Next: cpd pr open --pr {pr} --plan {plan}");
    } else {
        text += &format!(
            " Failed: {}; decide each with cpd job retry <job> --tier <T> --plan {plan} or cpd job cancel <job> --reason <text> --plan {plan}, then: cpd pr open --pr {pr} --plan {plan}",
            failed.join(", ")
        );
    }
    events::append(tx, NewEvent::all_merged(text, pr, &merged, &cancelled).plan(plan.clone()))?;
    Ok(true)
}

/// A PR whose Jobs are all merged and which is still in `dispatch`: E4's escalation looks at it.
/// The base PR `q` is the PR of the same repo whose Lead Branch is this PR's base (§2.2); the last column is the latest
/// `pr-base-changed` of this PR that had a base PR before.
const WAITING: &str = "SELECT r.plan, r.key, r.all_merged_at, p.title, p.context, p.lead_session,
         q.key IS NOT NULL, CASE WHEN q.status IN ('open', 'merged') THEN COALESCE(q.pr_opened_at, q.merged_at) END,
         (SELECT e.at FROM events e WHERE e.kind = 'pr-base-changed' AND e.plan = r.plan AND json_extract(e.data, '$.pr') = r.key
            AND json_extract(e.data, '$.from_pr') IS NOT NULL ORDER BY e.id DESC LIMIT 1)
  FROM prs r JOIN plans p ON p.id = r.plan LEFT JOIN prs q ON q.repo = r.repo AND q.lead_branch = r.base
  WHERE r.status = 'dispatch' AND r.all_merged_at IS NOT NULL
    AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = r.plan AND j.pr = r.key AND j.status IN ('draft', 'queued', 'running'))
  ORDER BY r.plan, r.key";

/// An escalation of this all-merged moment exists: a `pr_unopened` `session-message` for this PR newer than its latest `all-merged`.
const ESCALATED: &str = "SELECT EXISTS (SELECT 1 FROM events e WHERE e.kind = 'session-message'
    AND json_extract(e.data, '$.escalation') = 'pr_unopened'
    AND json_extract(e.data, '$.plan') = ?1 AND json_extract(e.data, '$.pr') = ?2
    AND e.id > COALESCE((SELECT max(a.id) FROM events a WHERE a.kind = 'all-merged' AND a.plan = ?1 AND json_extract(a.data, '$.pr') = ?2), 0))";

/// What an escalation needs of a PR.
struct Waiting {
    plan: PlanId,
    pr: PrKey,
    title: String,
    context: ContextId,
    lead: SessionId,
    minutes: i64,
}

/// What E4's escalation did for one PR.
enum Escalated {
    /// Not due, already escalated, or no longer waiting.
    Nothing,
    /// The Manager got the message.
    Sent(SessionId),
    /// The Context has no Manager: the Plan's Lead got an `error`.
    NoManager,
}

/// What E4's escalation reads of a PR in [`WAITING`].
struct Row {
    plan: String,
    pr: String,
    all_merged_at: String,
    title: String,
    context: String,
    lead: String,
    /// Its base is another PR's Lead Branch.
    stacked: bool,
    /// When that base PR became ready (open or merged); null while it is not.
    base_ready_at: Option<String>,
    /// The `at` of its latest `pr-base-changed` that had a base PR before.
    unstacked_at: Option<String>,
}

fn time(row: &Row, what: &str, at: &str) -> Result<OffsetDateTime, CpdError> {
    OffsetDateTime::parse(at, &Rfc3339).map_err(|e| {
        CpdError::new(ErrorCode::StoreFailed, format!("The store holds an invalid value: PR {} of Plan {} has {what} {at:?}: {e}.", row.pr, row.plan))
    })
}

/// When the PR's escalation clock starts (contract §3.7 E4, hs-d301): the later of `all_merged_at` and, for a stacked
/// PR, the moment its base PR was ready (`None`: it is not, so there is no clock); for a PR unstacked by its base PR's
/// merge, the latest repoint instead. A PR never stacked has `all_merged_at` alone.
fn clock_start(row: &Row) -> Result<Option<OffsetDateTime>, CpdError> {
    let all_merged = time(row, "all_merged_at", &row.all_merged_at)?;
    let candidate = match (row.stacked, &row.base_ready_at, &row.unstacked_at) {
        (true, None, _) => return Ok(None),
        (true, Some(at), _) => Some(time(row, "a base PR ready at", at)?),
        (false, _, Some(at)) => Some(time(row, "a pr-base-changed at", at)?),
        (false, _, None) => None,
    };
    Ok(Some(candidate.map_or(all_merged, |c| c.max(all_merged))))
}

/// The PRs of `rows` unopened for at least `limit` minutes at `now`, with the age of their clock start in whole minutes.
fn overdue(rows: Vec<Row>, now: OffsetDateTime, limit: u32) -> Result<Vec<Waiting>, CpdError> {
    let mut out = Vec::new();
    for row in rows {
        let Some(start) = clock_start(&row)? else { continue };
        let minutes = (now - start).whole_minutes();
        if minutes >= i64::from(limit) {
            out.push(Waiting {
                plan: PlanId::parse(&row.plan)?,
                pr: PrKey::parse(&row.pr)?,
                title: row.title,
                context: ContextId::parse(&row.context)?,
                lead: SessionId::parse(&row.lead)?,
                minutes,
            });
        }
    }
    Ok(out)
}

/// E4's second half (§3.7, hs-d334): each PR in `dispatch` with no Job to run, `pr_open_escalate_minutes` after its
/// clock start ([`clock_start`]), is told once to the Context's Manager; with no Manager, to the Lead as an `error` (the step rule).
fn escalate_all(engine: &mut Engine) {
    let now = OffsetDateTime::now_utc();
    let limit = engine.config.pr_open_escalate_minutes;
    let due = engine.store.read(|conn| {
        let mut stmt = conn.prepare(WAITING)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Row {
                    plan: r.get(0)?,
                    pr: r.get(1)?,
                    all_merged_at: r.get(2)?,
                    title: r.get(3)?,
                    context: r.get(4)?,
                    lead: r.get(5)?,
                    stacked: r.get(6)?,
                    base_ready_at: r.get(7)?,
                    unstacked_at: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        overdue(rows, now, limit)
    });
    let due = match due {
        Ok(due) => due,
        Err(e) => return engine.fail(Step::E4, About::Machine, &e),
    };
    let mut unreported = Vec::new();
    for w in due {
        let about = About::Plan(w.plan.clone());
        match engine.store.write(SOURCE, |tx| escalate(tx, &w)) {
            Ok(Escalated::Nothing) => {}
            Ok(Escalated::Sent(manager)) => {
                engine.log.line(&format!("E4: PR {} of Plan {} is unopened after {} minutes: told Manager {manager}", w.pr, w.plan, w.minutes));
            }
            Ok(Escalated::NoManager) => {
                let message = format!(
                    "PR {} has had all its Jobs merged for {} minutes and its pull request is not opened, and the Context has no Manager.",
                    w.pr, w.minutes
                );
                let failure = Failure { code: "pr_unopened".to_owned(), message, details: json!({"pr": w.pr, "minutes": w.minutes}) };
                engine.fail(Step::E4, about, failure);
                unreported.push(w.plan.clone());
            }
            Err(e) => engine.fail(Step::E4, about, &e),
        }
    }
    // A Plan that no longer has a PR waiting without a Manager has recovered.
    let recovered: Vec<About> = engine
        .open_failures(Step::E4, "pr_unopened")
        .into_iter()
        .filter(|about| matches!(about, About::Plan(plan) if !unreported.contains(plan)))
        .collect();
    for about in recovered {
        engine.clear(Step::E4, &about);
    }
}

/// Decides again inside the transaction, then sends the Manager its message.
fn escalate(tx: &WriteTxn<'_>, w: &Waiting) -> Result<Escalated, CpdError> {
    let still: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM prs r WHERE r.plan = ?1 AND r.key = ?2 AND r.status = 'dispatch' AND r.all_merged_at IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = r.plan AND j.pr = r.key AND j.status IN ('draft', 'queued', 'running')))",
        [w.plan.as_str(), w.pr.as_str()],
        |r| r.get(0),
    )?;
    if !still {
        return Ok(Escalated::Nothing);
    }
    let manager: Option<String> = tx
        .query_row("SELECT id FROM sessions WHERE context = ?1 AND role = 'manager' AND archived_at IS NULL", [w.context.as_str()], |r| r.get(0))
        .optional()?;
    let Some(manager) = manager else {
        return Ok(Escalated::NoManager);
    };
    let sent: bool = tx.query_row(ESCALATED, [w.plan.as_str(), w.pr.as_str()], |r| r.get(0))?;
    if sent {
        return Ok(Escalated::Nothing);
    }
    let manager = SessionId::parse(&manager)?;
    let text = format!(
        "Plan {} ({}): PR {} has had all its Jobs merged for {} minutes and its pull request is not opened. The Lead was told to run cpd pr open --pr {} and has not. Look at the Lead: cpd session show {}, and tell it or the User.",
        w.plan, w.title, w.pr, w.minutes, w.pr, w.lead
    );
    let event = NewEvent::session_message_pr_unopened(text, w.lead.as_str(), w.plan.as_str(), w.pr.as_str(), w.minutes)
        .context(w.context.clone())
        .session(manager.clone());
    events::append(tx, event)?;
    Ok(Escalated::Sent(manager))
}
