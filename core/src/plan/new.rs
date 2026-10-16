//! `plan new` (commands.md §4.1, contract §3.1): registers the Plan and its
//! Lead, writes the `plan` record, the Lead's first message, and starts the
//! Lead in a parked window of the cockpit.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::lead::{self, Lead};
use super::{Env, NewArgs, bad_row, read_text_file, write_file};
use crate::args::Text;
use crate::context::render::markdown;
use crate::context::{By, ContextRow, Contexts, Draft, PlanRef, Record, plan_repos};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::Output;

/// The Plan as `plan new` reads it back: no PR is needed (a Plan starts with none, R34 A12).
#[derive(Clone, Debug)]
struct Created {
    id: PlanId,
    context: ContextId,
    title: String,
    jira: Option<String>,
    phase: String,
    lead_session: SessionId,
    /// The Lead's name and cwd, as stored.
    name: String,
    cwd: String,
}

fn find(conn: &Connection, id: &PlanId) -> Result<Option<Created>, CpdError> {
    let raw = conn
        .query_row(
            "SELECT p.context, p.title, p.jira, p.phase, p.lead_session, s.name, s.cwd
               FROM plans p JOIN sessions s ON s.id = p.lead_session WHERE p.id = ?1",
            [id.as_str()],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get(3)?, r.get::<_, String>(4)?, r.get(5)?, r.get(6)?)),
        )
        .optional()?;
    raw.map(|(context, title, jira, phase, lead_session, name, cwd)| {
        Ok(Created {
            id: id.clone(),
            context: ContextId::parse(&context).map_err(|_| bad_row(format!("Plan {id} has context {context:?}")))?,
            lead_session: SessionId::parse(&lead_session).map_err(|_| bad_row(format!("Plan {id} has lead_session {lead_session:?}")))?,
            title,
            jira,
            phase,
            name,
            cwd,
        })
    })
    .transpose()
}

/// The realpath of the directory `plan new` runs from: where the Lead starts (§6.4).
fn caller_cwd() -> Result<String, CpdError> {
    let setup = |e: std::io::Error| CpdError::new(ErrorCode::SetupFailed, format!("Cannot resolve the current directory: {e}."));
    Ok(std::env::current_dir().and_then(|d| d.canonicalize()).map_err(setup)?.display().to_string())
}

pub(super) fn new(args: NewArgs) -> Result<Output, CpdError> {
    let brief = args.brief_file.as_deref().map(read_text_file).transpose()?;
    let mut env = Env::open()?;
    let id = args.id.clone();
    let name = args.name.as_ref().map_or_else(|| id.to_string(), |n| n.as_str().to_owned());
    let jira = args.jira.as_ref().map(|j| j.as_str().to_owned());

    env.store.read(|conn| ContextRow::require(conn, &args.context))?;
    if let Some(plan) = env.store.read(|conn| find(conn, &id))? {
        // The caller's cwd is not a flag: a rerun from another directory is still `already`.
        let same = plan.context == args.context && plan.title == args.title.as_str() && plan.jira == jira && plan.name == name;
        if !same {
            return Err(CpdError::new(ErrorCode::PlanExists, format!("Plan {id} exists with other flags.")).with("plan", summary(&plan, &[])));
        }
        return already(&mut env, &plan, brief.as_deref());
    }
    let cwd = caller_cwd()?;

    // 1. The Plan and its Lead, in one transaction.
    let session = SessionId::new_v4();
    let tier = env.config.lead_tier.to_string();
    let (forked_from, grounded_seq) = env.store.write("plan new", |tx| {
        let exists = tx.query_row("SELECT 1 FROM plans WHERE id = ?1", [id.as_str()], |_| Ok(())).optional()?;
        if exists.is_some() {
            return Err(CpdError::new(ErrorCode::PlanExists, format!("Plan {id} was created meanwhile.")).with("plan", id.as_str()));
        }
        let (forked_from, grounded_seq): (Option<String>, Option<i64>) = tx.query_row(
            "SELECT lead_session, grounded_seq FROM contexts WHERE id = ?1",
            [args.context.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        tx.execute(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                                   state, state_label, state_at, created_at)
             SELECT ?1, 'lead', ?2, ?3, NULL, ?4, NULL, ?5, ?6, ?7, NULL, 'stopped', NULL, at, at
               FROM txns WHERE id = cpd_txn()",
            rusqlite::params![session.as_str(), args.context.as_str(), id.as_str(), name, forked_from, cwd, tier],
        )?;
        tx.execute(
            "INSERT INTO plans (id, context, title, jira, phase, lead_session, created_at)
             SELECT ?1, ?2, ?3, ?4, 'grounding', ?5, at FROM txns WHERE id = cpd_txn()",
            rusqlite::params![id.as_str(), args.context.as_str(), args.title.as_str(), jira, session.as_str()],
        )?;
        let text = format!("Plan {id} \"{}\" created in Context {}, phase grounding. Next: cpd plan show --plan {id}", args.title, args.context);
        let data = json!({"title": args.title.as_str(), "jira": jira, "lead_session": session, "forked_from": forked_from, "cwd": cwd});
        append(tx, NewEvent::new(Kind::PlanCreated, text, data).plan(id.clone()))?;
        let text = match &forked_from {
            Some(source) => format!("Lead {name} of Plan {id} registered, a fork of the grounded Lead {source}."),
            None => format!("Lead {name} of Plan {id} registered, a fresh Lead."),
        };
        let data = json!({"role": "lead", "name": name, "forked_from": forked_from, "tier": tier});
        append(tx, NewEvent::new(Kind::SessionRegistered, text, data).plan(id.clone()).session(session.clone()))?;
        Ok((forked_from, grounded_seq))
    })?;
    let plan = env.store.read(|conn| find(conn, &id))?.ok_or_else(|| bad_row(format!("Plan {id} was just created")))?;

    // 2. The `plan` record; 3. the first message; 4. the Lead's window.
    let record = write_record(&mut env, &plan)?;
    let since = forked_from.as_ref().and(grounded_seq).unwrap_or(0);
    write_message(&mut env, &plan, brief.as_deref(), since)?;
    let started = start_lead(&mut env, &plan, &session, &name)?;
    Ok(Output::success(json!({
        "plan": summary(&plan, &[]),
        "lead": {"session": session, "name": name, "forked_from": forked_from, "cwd": cwd,
                 "pane": started.pane.as_str(), "window": started.window.as_str()},
        "record": record,
    })))
}

/// `plan new` again with the same flags: redoes the steps that did not happen
/// (the `plan` record, the first message, the Lead's window) and returns
/// `"already": true`.
fn already(env: &mut Env, plan: &Created, brief: Option<&str>) -> Result<Output, CpdError> {
    let machine = env.config.machine.clone();
    let record: Option<String> = env.store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT id FROM context_records WHERE context = ?1 AND kind = 'plan' AND plan_id = ?2 AND plan_machine = ?3
                 ORDER BY seq LIMIT 1",
                [plan.context.as_str(), plan.id.as_str(), machine.as_str()],
                |r| r.get(0),
            )
            .optional()?)
    })?;
    let record = match record {
        Some(record) => record,
        None => write_record(env, plan)?,
    };
    let (lead, grounded_seq): ((Option<String>, String, Option<String>), Option<i64>) = env.store.read(|conn| {
        let lead = conn.query_row(
            "SELECT forked_from, state, tmux_pane FROM sessions WHERE id = ?1",
            [plan.lead_session.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let seq = conn.query_row("SELECT grounded_seq FROM contexts WHERE id = ?1", [plan.context.as_str()], |r| r.get(0))?;
        Ok((lead, seq))
    })?;
    let (forked_from, state, pane) = lead;
    let name = plan.name.clone();
    if !env.home.lead_message(&plan.id).exists() {
        let since = forked_from.as_ref().and(grounded_seq).unwrap_or(0);
        write_message(env, plan, brief, since)?;
    }
    let (pane, window) = match pane {
        None if state == "stopped" => {
            let started = start_lead(env, plan, &plan.lead_session, &name)?;
            (Some(started.pane.as_str().to_owned()), Some(started.window.as_str().to_owned()))
        }
        pane => (pane, env.store.read(|conn| lead::last_window(conn, &plan.lead_session))?),
    };
    let prs = env.store.read(|conn| listed(conn, &plan.id))?;
    Ok(Output::success(json!({
        "plan": summary(plan, &prs),
        "lead": {"session": plan.lead_session, "name": name, "forked_from": forked_from, "cwd": plan.cwd, "pane": pane, "window": window},
        "record": record,
        "already": true,
    })))
}

/// The Plan as `plan new` prints it; `prs` is empty for a new Plan.
fn summary(plan: &Created, prs: &[Value]) -> Value {
    json!({"id": plan.id, "context": plan.context, "title": plan.title, "jira": plan.jira, "phase": plan.phase,
           "lead_session": plan.lead_session, "prs": prs})
}

/// The Plan's PRs, in key order: only an `already` rerun can find any.
fn listed(conn: &Connection, id: &PlanId) -> Result<Vec<Value>, CpdError> {
    let mut stmt = conn.prepare("SELECT key, repo, status FROM prs WHERE plan = ?1 ORDER BY key")?;
    let rows = stmt.query_map([id.as_str()], |r| {
        Ok(json!({"key": r.get::<_, String>(0)?, "repo": r.get::<_, String>(1)?, "status": r.get::<_, String>(2)?}))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Step 2: the Context record of kind `plan` (§4.6), `text` = the title.
/// Returns its id.
fn write_record(env: &mut Env, plan: &Created) -> Result<String, CpdError> {
    let by = By { session: env_id("CPD_SESSION", SessionId::parse)?.or(env_id("PI_SESSION_ID", SessionId::parse)?), plan: env_id("CPD_PLAN", PlanId::parse)? };
    let draft = Draft::Plan(PlanRef { id: plan.id.clone(), machine: env.config.machine.clone(), jira: plan.jira.clone() });
    let title: Text = plan.title.parse().map_err(|_| bad_row(format!("Plan {} has title {:?}", plan.id, plan.title)))?;
    let written = Contexts::new(&env.home, &env.config, &mut env.store).write(&plan.context, "plan new", by, &title, draft)?;
    Ok(written.record.id.to_string())
}

/// A non-empty variable of the caller's environment, parsed (the writer of a
/// record, contract §2.2 "Context record" `by`).
fn env_id<T>(name: &str, parse: fn(&str) -> Result<T, CpdError>) -> Result<Option<T>, CpdError> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(|v| parse(&v.to_string_lossy())).transpose()
}

/// Step 3: `run/<plan>/lead-message.md`: the header, the brief, then the
/// Context rendered as `context show` does, from index position `since`.
fn write_message(env: &mut Env, plan: &Created, brief: Option<&str>, since: i64) -> Result<(), CpdError> {
    let (context, records, repos) = env.store.read(|conn| {
        let context = ContextRow::require(conn, &plan.context)?;
        let mut stmt = conn.prepare(&format!("SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq", Record::COLUMNS))?;
        let records = stmt.query_map([plan.context.as_str()], Record::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let repos = plan_repos(conn, &env.config.machine, &records)?;
        Ok((context, records, repos))
    })?;
    let mut message = vec![header(plan)];
    if let Some(brief) = brief {
        message.push(brief.trim_end().to_owned());
    }
    message.push(markdown(&context.title, context.jira.as_deref(), &records, since, &repos));
    write_file(&env.home.lead_message(&plan.id), &(message.join("\n\n") + "\n"))
}

fn header(plan: &Created) -> String {
    format!(
        "You are the Lead of Plan {} \"{}\" in Context {}. Phase grounding: follow the cpd-lead skill.",
        plan.id, plan.title, plan.context
    )
}

/// Step 4: the Lead's window `park-<plan>` (contract §6.2).
fn start_lead(env: &mut Env, plan: &Created, session: &SessionId, name: &str) -> Result<lead::Started, CpdError> {
    let window = crate::host::layout::park_window(session, Some(&plan.id));
    let lead = Lead { session, context: &plan.context, plan: Some(&plan.id), name, cwd: Path::new(&plan.cwd) };
    lead::start(env, &lead, &window, "plan new")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ContextId;

    #[test]
    fn the_header_names_plan_and_context() {
        let plan = Created {
            id: PlanId::parse("pay-4521").unwrap(),
            context: ContextId::parse("payments").unwrap(),
            title: "Retry failed payouts".into(),
            jira: None,
            phase: "grounding".into(),
            lead_session: SessionId::parse("s1").unwrap(),
            name: "pay-4521".into(),
            cwd: "/home/u".into(),
        };
        assert_eq!(
            header(&plan),
            "You are the Lead of Plan pay-4521 \"Retry failed payouts\" in Context payments. Phase grounding: follow the cpd-lead skill."
        );
    }
}
