//! `plan cancel` (commands.md §4.12, E1): abandons a Plan. Every step is idempotent, so a rerun
//! after any failure continues where it stopped. The pieces are the other commands': `job cancel`
//! (its cancel without the shape check), `gh pr close`, `session close` ([`close_lead`]) and the
//! archive of `plan archive` (§4.10).

use std::env;
use std::time::Duration;

use rusqlite::Connection;
use serde_json::{Value, json};

use super::archive::{Objects, refuse_chat_slot};
use super::{CancelArgs, Closing, Env, close_reviews, jobs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::host::Tmux;
use crate::ids::{AttemptId, ContextId, JobKey, PlanId, SessionId};
use crate::output::Output;
use crate::ports::Ports;
use crate::session::close_lead;
use crate::store::Store;
use crate::{job, pr};

const COMMAND: &str = "plan cancel";

/// `--reason` when none is given.
const DEFAULT_REASON: &str = "plan cancelled";

/// How long the command waits for the stopped Attempts to end (`cancel_timeout`).
const WAIT: Duration = Duration::from_secs(60);

/// How often the wait looks again.
const POLL: Duration = Duration::from_millis(250);

/// `plan cancel` (§4.12).
pub(super) fn plan_cancel(args: CancelArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let reason = args.reason.as_ref().map_or(DEFAULT_REASON, |r| r.as_str());
    let mut env = Env::open()?;
    let objects = env.store.read(|conn| Objects::of_plan(conn, &plan))?;
    let tmux = Tmux::new(env.config.tmux.socket.clone());
    let cockpit = env.config.tmux.cockpit_session.clone();

    // 1. Check.
    refuse_chat_slot(&mut env.store, &tmux, &cockpit, &objects.sessions)?;

    // 2. Reviews: a pending Plan review is superseded, nobody decides it now.
    let pending = env.store.read(|conn| {
        Ok(conn.query_row("SELECT EXISTS (SELECT 1 FROM plan_reviews WHERE plan = ?1 AND status = 'pending')", [plan.as_str()], |r| r.get::<_, bool>(0))?)
    })?;
    if pending {
        env.store.write(COMMAND, |tx| close_reviews(tx, &plan, &Closing::Cancelled))?;
    }

    // 3. Jobs.
    let cancelled = cancel_jobs(&mut env.store, &plan, reason)?;
    wait_until_ended_with(&env.ports, &mut env.store, &plan, WAIT, POLL)?;

    // 4. Escalations.
    let expired = expire_escalations(&mut env.store, &plan, reason)?;

    // 5. Pull requests.
    let (closed_prs, left_open) = close_prs(&mut env, &plan)?;

    // 6. tmux. The Lead that runs this command, if it is one of the Plan's, is closed last: its
    // pane dies with the process, and the rows of step 7 are written by then.
    let own = env::var("CPD_SESSION").ok().and_then(|id| objects.sessions.iter().find(|s| s.as_str() == id).cloned());
    let mut closed = Vec::new();
    for id in objects.sessions.iter().filter(|id| Some(*id) != own.as_ref()) {
        let lead = close_lead(&mut env.store, &tmux, &cockpit, id, COMMAND)?;
        closed.extend(lead.pane.filter(|_| !lead.already));
    }

    // 7. Rows.
    let archived = archive_rows(&mut env.store, &plan, &objects.sessions)?;
    if let Some(id) = &own {
        let lead = close_lead(&mut env.store, &tmux, &cockpit, id, COMMAND)?;
        closed.extend(lead.pane.filter(|_| !lead.already));
    }

    let already = cancelled.is_empty() && !expired && closed_prs.is_empty() && left_open.is_empty() && closed.is_empty() && archived.is_empty();
    let mut out = json!({"cancelled": cancelled, "closed_prs": closed_prs, "left_open": left_open, "closed": closed, "archived": archived});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// Step 3: `job cancel`, without its shape check, for every Job that is not `merged` and not `cancelled`. The keys it newly
/// cancelled, or asked the engine to stop.
pub(super) fn cancel_jobs(store: &mut Store, plan: &PlanId, reason: &str) -> Result<Vec<String>, CpdError> {
    let all = store.read(|conn| jobs::load(conn, plan))?;
    let mut keys = Vec::new();
    for job in all.iter().filter(|j| !matches!(j.status.as_str(), "merged" | "cancelled")) {
        let Output::Success(out) = job::cancel::cancel_without_shape(plan, JobKey::parse(&job.key)?, reason)? else { continue };
        if !out.contains_key("already") {
            keys.push(job.key.clone());
        }
    }
    Ok(keys)
}

/// The Attempts of `plan` that are not `ended`.
fn live_attempts(conn: &Connection, plan: &PlanId) -> Result<Vec<String>, CpdError> {
    let mut stmt = conn.prepare("SELECT id FROM attempts WHERE plan = ?1 AND status <> 'ended' ORDER BY id")?;
    Ok(stmt.query_map([plan.as_str()], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
}

/// [`wait_until_ended_with`] on the real ports.
#[cfg(test)]
fn wait_until_ended(store: &mut Store, plan: &PlanId, limit: Duration, every: Duration) -> Result<(), CpdError> {
    wait_until_ended_with(&Ports::real(), store, plan, limit, every)
}

/// Polls, up to `limit`, until no Attempt of `plan` is left that is not `ended`; else `cancel_timeout`. The deadline is
/// read and the naps are taken on `ports.clock`.
pub(crate) fn wait_until_ended_with(ports: &Ports, store: &mut Store, plan: &PlanId, limit: Duration, every: Duration) -> Result<(), CpdError> {
    let deadline = ports.clock.now() + limit;
    loop {
        let live = store.read(|conn| live_attempts(conn, plan))?;
        if live.is_empty() {
            return Ok(());
        }
        if ports.clock.now() >= deadline {
            return Err(CpdError::new(
                ErrorCode::CancelTimeout,
                format!("Attempts of Plan {plan} are still running {}s after the stop request: {}. Rerun cpd plan cancel.", limit.as_secs(), live.join(", ")),
            )
            .with("plan", plan.as_str())
            .with("attempts", live));
        }
        ports.clock.sleep(every);
    }
}

/// Step 4: each `open` Escalation of `plan` becomes `expired`. Whether any did.
fn expire_escalations(store: &mut Store, plan: &PlanId, reason: &str) -> Result<bool, CpdError> {
    store.write(COMMAND, |tx| {
        let mut stmt = tx.prepare("SELECT id, attempt FROM escalations WHERE plan = ?1 AND status = 'open' ORDER BY asked_at, id")?;
        let open = stmt.query_map([plan.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, attempt) in &open {
            tx.execute(
                "UPDATE escalations SET status = 'expired', expired_at = (SELECT at FROM txns WHERE id = cpd_txn())
                  WHERE plan = ?1 AND id = ?2 AND status = 'open'",
                [plan.as_str(), id],
            )?;
            let attempt = AttemptId::parse(attempt)?;
            let text = format!("Escalation {id} of Attempt {attempt} expired: Plan {plan} was cancelled. See: cpd attempt show {attempt} --plan {plan}");
            append(tx, NewEvent::escalation_expired(text, id, reason).plan(plan.clone()).job(attempt.job().clone()).attempt(attempt))?;
        }
        Ok(!open.is_empty())
    })
}

/// Step 5: `gh pr close` for each PR of `plan` in status `open` whose pull request is not `merged`
/// or `closed` as the poller last read it. The PR rows keep their status. `{pr, number}` of each. An adopted PR's pull
/// request is never closed: it is listed apart, as `{pr, number}`, in the second vector.
fn close_prs(env: &mut Env, plan: &PlanId) -> Result<(Vec<Value>, Vec<Value>), CpdError> {
    let open = env.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT r.key, p.repo, p.number, r.head_ref IS NOT NULL FROM workflows r JOIN pull_requests p ON p.id = r.pull_request
              WHERE r.plan = ?1 AND r.status = 'open' AND p.state = 'open' ORDER BY r.key",
        )?;
        Ok(stmt.query_map([plan.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, bool>(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
    })?;
    let (mut closed, mut left_open) = (Vec::new(), Vec::new());
    for (key, repo, number, adopted) in open {
        if adopted {
            left_open.push(json!({"pr": key, "number": number}));
            continue;
        }
        pr::close_pr(env.home.root(), &repo, number)?;
        closed.push(json!({"pr": key, "number": number}));
    }
    Ok((closed, left_open))
}

/// Step 7: `archived_at` on the Plan and its Sessions, one transaction, with the Events of
/// `plan archive` (§4.10). The objects it changed.
fn archive_rows(store: &mut Store, plan: &PlanId, sessions: &[SessionId]) -> Result<Vec<Value>, CpdError> {
    store.write(COMMAND, |tx| {
        let set = "SET archived_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1 AND archived_at IS NULL";
        let mut changed = Vec::new();
        if tx.execute(&format!("UPDATE plans {set}"), [plan.as_str()])? > 0 {
            let event = NewEvent::plan_archived(format!("Plan {plan} archived."), true);
            append(tx, event.plan(plan.clone()))?;
            changed.push(json!({"kind": "plan", "id": plan}));
        }
        for id in sessions {
            if tx.execute(&format!("UPDATE sessions {set}"), [id.as_str()])? > 0 {
                let (name, context): (String, String) =
                    tx.query_row("SELECT name, context FROM sessions WHERE id = ?1", [id.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?;
                let event = NewEvent::session_archived(format!("Session {name} archived."), true);
                append(tx, event.session(id.clone()).context(ContextId::parse(&context)?).plan(plan.clone()))?;
                changed.push(json!({"kind": "session", "id": id}));
            }
        }
        Ok(changed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attempt_that_does_not_end_is_cancel_timeout_listing_it() {
        let dir = tempfile::tempdir().unwrap();
        let home = crate::home::CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        store
            .write("fixture", |tx| {
                tx.execute_batch(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
                     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                       VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't');
                     INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                       VALUES ('pay', 'api', '/r', 'main', 'waiting', 'cpd/pay', cpd_txn(), 't');
                     INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at)
                       VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff', cpd_txn(), 't');
                     INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                       VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pay--a-a1', '/w', 't');",
                )?;
                Ok(())
            })
            .unwrap();
        let plan = PlanId::parse("pay").unwrap();
        let err = wait_until_ended(&mut store, &plan, Duration::from_millis(30), Duration::from_millis(5)).unwrap_err();
        assert_eq!(err.code(), ErrorCode::CancelTimeout);
        assert_eq!(err.details()["attempts"], json!(["a-a1"]));
    }
}
