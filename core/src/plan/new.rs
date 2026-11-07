//! `plan new` (commands.md §4.1, contract §3.1): registers the Plan and its
//! Lead, writes the `plan` record, the Lead's first message, and starts the
//! Lead in a parked window of the cockpit.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::lead::{self, Lead};
use super::row::{self, PlanRow};
use super::{Env, NewArgs, read_text_file, write_file};
use crate::agent_key::AgentKey;
use crate::args::Text;
use crate::config::WorkerRunner;
use crate::context::{By, ContextRow, Contexts, Draft, PlanRef};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::Output;
use crate::store::WriteTxn;

/// The Plan as `plan new` reads it back, with its Lead's name and cwd as stored.
/// No Workflow is needed (a Plan starts with none, R34 A12).
struct Created {
    plan: PlanRow,
    name: String,
    cwd: String,
    tier: String,
    runner: String,
}

fn find(conn: &Connection, id: &PlanId) -> Result<Option<Created>, CpdError> {
    let Some(plan) = row::find(conn, id)? else {
        return Ok(None);
    };
    let Some(lead) = plan.lead_session.as_ref() else {
        return Err(CpdError::bad_row(format!("Plan {id} has no Lead")));
    };
    let (name, cwd, tier, runner) = conn.query_row(
        "SELECT name, cwd, tier, runner FROM sessions WHERE id = ?1",
        [lead.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    Ok(Some(Created {
        plan,
        name,
        cwd,
        tier,
        runner,
    }))
}

pub(super) fn new(args: NewArgs) -> Result<Output, CpdError> {
    let brief: Option<Text> = args
        .brief_file
        .as_deref()
        .map(|f| read_text_file(f)?.parse())
        .transpose()?;
    let mut env = Env::open()?;
    let id = args.id.clone();
    let name = AgentKey::lead(args.context.clone(), id.clone()).to_string();
    let jira = args.jira.as_ref().map(|j| j.as_str().to_owned());

    let context = require_context(&mut env, &args.context)?;
    if let Some(tier) = args.tier {
        let home = env.home.clone();
        let models = env.store.read(|conn| {
            crate::policy::model::models_of_context(conn, &home, args.context.as_str())
        })?;
        let catalog = crate::config::catalog::load_with(&models, &mut env.store)?;
        catalog.check(tier)?;
        let lead = env.config.range(&catalog, crate::config::Role::Lead)?;
        lead.check(tier, crate::config::Role::Lead)?;
    }
    refuse_deleted(&mut env, &id)?;
    if let Some(plan) = env.store.read(|conn| find(conn, &id))? {
        // The caller's cwd is not a flag: a rerun from another directory is still `already`.
        let same = plan.plan.context == args.context
            && plan.plan.title == args.title.as_str()
            && plan.plan.jira == jira
            && plan.name == name
            && args.tier.is_none_or(|t| t.to_string() == plan.tier)
            && args.runner.is_none_or(|r| r.as_str() == plan.runner);
        if !same {
            return Err(CpdError::new(
                ErrorCode::PlanExists,
                format!("Plan {id} exists with other flags."),
            )
            .with("plan", summary(&plan.plan, &[])));
        }
        return already(&mut env, &plan, brief.as_ref(), args.by_user);
    }
    refuse_id_taken(&mut env, &id)?;
    refuse_archived(&context)?;
    let cwd = crate::session::agent_cwd()?;
    // The runner of the new Lead: read once at creation and stored, never changed (contract §6.19, Q10).
    let lead_runner = args.runner.unwrap_or(env.config.new_lead_runner());

    // 1. The Plan and its Lead, in one transaction.
    let session = SessionId::new_v4();
    let tier = args.tier.unwrap_or(env.config.lead_tier).to_string();
    let creation = Creation {
        id: &id,
        context: &args.context,
        title: args.title.as_str(),
        jira: jira.as_deref(),
        name: &name,
        cwd: &cwd,
        tier: &tier,
        runner: lead_runner,
        by_user: args.by_user,
        session: &session,
    };
    env.store.write("plan new", |tx| insert(tx, &creation))?;

    // 2. The `plan` record; 3. the first message; 4. the Lead's window.
    let finished = finish(&mut env, &creation, brief.as_ref(), "plan new")?;
    Ok(Output::success(json!({
        "plan": summary(&finished.plan, &[]),
        "lead": {"session": session, "name": name, "forked_from": Value::Null, "cwd": cwd,
                 "pane": finished.pane, "window": finished.window},
        "record": finished.record,
    })))
}

/// The Context exists and has an owner (`plan new`; `inbox claim` leaves the owner to `ensure`).
fn require_context(env: &mut Env, id: &ContextId) -> Result<ContextRow, CpdError> {
    let context = env.store.read(|conn| ContextRow::require(conn, id))?;
    if context.owner.is_none() {
        return Err(crate::policy::owner_unset(id));
    }
    Ok(context)
}

/// The Context is not archived.
pub(crate) fn refuse_archived(context: &ContextRow) -> Result<(), CpdError> {
    if context.archived_at.is_some() {
        return Err(CpdError::new(
            ErrorCode::Archived,
            format!(
                "Context {} is archived: unarchive it before creating a Plan in it.",
                context.id
            ),
        )
        .with("context", context.id.as_str())
        .with("next", format!("cpd context unarchive {}", context.id)));
    }
    Ok(())
}

/// A deleted Plan keeps its row, and so its id: never reused (contract §2.2 "Deleted").
pub(crate) fn refuse_deleted(env: &mut Env, id: &PlanId) -> Result<(), CpdError> {
    let deleted: Option<bool> = env.store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT deleted_at IS NOT NULL FROM plans WHERE id = ?1",
                [id.as_str()],
                |r| r.get(0),
            )
            .optional()?)
    })?;
    if deleted == Some(true) {
        return Err(CpdError::new(
            ErrorCode::PlanExists,
            format!("Plan {id} was deleted: its id is not reused."),
        )
        .with("plan", id.as_str()));
    }
    Ok(())
}

/// A Plan id is refused when a Context holds it (deleted ones included): agent keys stay unambiguous.
fn refuse_id_taken(env: &mut Env, id: &PlanId) -> Result<(), CpdError> {
    let taken: bool = env.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM contexts WHERE id = ?1)",
            [id.as_str()],
            |r| r.get(0),
        )?)
    })?;
    if taken {
        return Err(CpdError::new(
            ErrorCode::IdTaken,
            format!("Plan id {id} is taken by a Context."),
        )
        .with("plan", id.as_str()));
    }
    Ok(())
}

/// A Plan and its Lead to create (`plan new`, `inbox claim`).
pub(crate) struct Creation<'a> {
    pub id: &'a PlanId,
    pub context: &'a ContextId,
    pub title: &'a str,
    pub jira: Option<&'a str>,
    /// The Lead's name.
    pub name: &'a str,
    pub cwd: &'a str,
    pub tier: &'a str,
    pub runner: WorkerRunner,
    pub by_user: bool,
    /// The Lead's Session id.
    pub session: &'a SessionId,
}

/// Step 1, inside the caller's transaction: inserts the Lead Session and the Plan, appends `plan-created` and
/// `session-registered`.
pub(crate) fn insert(tx: &WriteTxn<'_>, c: &Creation<'_>) -> Result<(), CpdError> {
    let Creation {
        id,
        context,
        title,
        jira,
        name,
        cwd,
        tier,
        runner,
        by_user,
        session,
    } = c;
    let exists = tx
        .query_row("SELECT 1 FROM plans WHERE id = ?1", [id.as_str()], |_| {
            Ok(())
        })
        .optional()?;
    if exists.is_some() {
        return Err(CpdError::new(
            ErrorCode::PlanExists,
            format!("Plan {id} was created meanwhile."),
        )
        .with("plan", id.as_str()));
    }
    tx.execute(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                               state, state_label, state_at, created_at, by_user, runner)
         SELECT ?1, 'lead', ?2, ?3, NULL, ?4, NULL, NULL, ?5, ?6, NULL, 'stopped', NULL, at, at, ?7, ?8
           FROM txns WHERE id = cpd_txn()",
        rusqlite::params![session.as_str(), context.as_str(), id.as_str(), name, cwd, tier, by_user, runner.as_str()],
    )?;
    tx.execute(
        "INSERT INTO plans (id, context, title, jira, phase, lead_session, created_at)
         SELECT ?1, ?2, ?3, ?4, 'grounding', ?5, at FROM txns WHERE id = cpd_txn()",
        rusqlite::params![id.as_str(), context.as_str(), title, jira, session.as_str()],
    )?;
    let text = format!(
        "Plan {id} \"{title}\" created in Context {context}, phase grounding. Next: cpd plan show --plan {id}"
    );
    let event = NewEvent::plan_created(text, title, *jira, session, Value::Null, cwd);
    append(tx, event.plan((*id).clone()))?;
    let text = format!("Lead {name} of Plan {id} registered, a fresh Lead.");
    let event = NewEvent::session_registered(text, "lead", name, Value::Null, tier);
    append(tx, event.plan((*id).clone()).session((*session).clone()))?;
    Ok(())
}

/// What [`finish`] did.
pub(crate) struct Finished {
    pub plan: PlanRow,
    /// The `plan` record's id.
    pub record: String,
    pub pane: Value,
    pub window: Value,
}

/// Steps 2 to 4, after the transaction: the `plan` record, the `brief` record, the first message (not for a
/// `by_user` Plan) and the Lead's window. `source` is the command of the Events they append.
pub(crate) fn finish(
    env: &mut Env,
    c: &Creation<'_>,
    brief: Option<&Text>,
    source: &str,
) -> Result<Finished, CpdError> {
    let plan = env
        .store
        .read(|conn| find(conn, c.id))?
        .ok_or_else(|| CpdError::bad_row(format!("Plan {} was just created", c.id)))?
        .plan;
    let record = write_record(env, &plan, source)?;
    if let Some(brief) = brief {
        write_brief(env, &plan, brief, source)?;
    }
    if !c.by_user {
        write_message(env, &plan, brief)?;
    }
    let started = start_lead(env, &plan, c.cwd, c.session, c.name, c.runner, source)?;
    let (pane, window) = match &started {
        Some(started) => (json!(started.pane.as_str()), json!(started.window.as_str())),
        None => (Value::Null, Value::Null),
    };
    Ok(Finished {
        plan,
        record,
        pane,
        window,
    })
}

/// `plan new` again with the same flags: redoes the steps that did not happen
/// (the `plan` record, the first message, the Lead's window) and returns
/// `"already": true`.
fn already(
    env: &mut Env,
    created: &Created,
    brief: Option<&Text>,
    by_user: bool,
) -> Result<Output, CpdError> {
    let plan = &created.plan;
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
        None => write_record(env, plan, "plan new")?,
    };
    if let Some(brief) = brief {
        let has_brief: bool = env.store.read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM context_records WHERE context = ?1 AND kind = 'brief' AND plan_id = ?2 AND plan_machine = ?3",
                    [plan.context.as_str(), plan.id.as_str(), machine.as_str()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })?;
        if !has_brief {
            write_brief(env, plan, brief, "plan new")?;
        }
    }
    let lead = plan
        .lead_session
        .as_ref()
        .ok_or_else(|| CpdError::bad_row(format!("Plan {} has no Lead", plan.id)))?;
    let (state, pane, runner): (String, Option<String>, String) = env.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT state, tmux_pane, runner FROM sessions WHERE id = ?1",
            [lead.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    })?;
    let name = created.name.clone();
    if !by_user && !env.home.lead_message(&plan.id).exists() {
        write_message(env, plan, brief)?;
    }
    let (pane, window) = match pane {
        // A durable Lead starts no pane (contract §6.19); the engine starts its host.
        None if state == "stopped" && runner != "durable" => {
            let started = start_lead(
                env,
                plan,
                &created.cwd,
                lead,
                &name,
                WorkerRunner::Pi,
                "plan new",
            )?
            .expect("a pi Lead starts a window");
            (
                Some(started.pane.as_str().to_owned()),
                Some(started.window.as_str().to_owned()),
            )
        }
        pane => (pane, env.store.read(|conn| lead::last_window(conn, lead))?),
    };
    let workflows = env.store.read(|conn| listed(conn, &plan.id))?;
    Ok(Output::success(json!({
        "plan": summary(plan, &workflows),
        "lead": {"session": plan.lead_session, "name": name, "forked_from": Value::Null, "cwd": created.cwd, "pane": pane, "window": window},
        "record": record,
        "already": true,
    })))
}

/// The Plan as `plan new` prints it; `prs` is empty for a new Plan.
fn summary(plan: &PlanRow, workflows: &[Value]) -> Value {
    json!({"id": plan.id, "context": plan.context, "title": plan.title, "jira": plan.jira, "phase": plan.phase.as_str(),
           "lead_session": plan.lead_session, "workflows": workflows})
}

/// The Plan's Workflows, in key order: only an `already` rerun can find any.
fn listed(conn: &Connection, id: &PlanId) -> Result<Vec<Value>, CpdError> {
    let mut stmt =
        conn.prepare("SELECT key, repo, status FROM workflows WHERE plan = ?1 ORDER BY key")?;
    let rows = stmt.query_map([id.as_str()], |r| {
        Ok(json!({"key": r.get::<_, String>(0)?, "repo": r.get::<_, String>(1)?, "status": r.get::<_, String>(2)?}))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Step 2: the Context record of kind `plan` (§4.6), `text` = the title.
/// Returns its id.
fn write_record(env: &mut Env, plan: &PlanRow, source: &str) -> Result<String, CpdError> {
    let draft = Draft::Plan(PlanRef {
        id: plan.id.clone(),
        machine: env.config.machine.clone(),
        jira: plan.jira.clone(),
    });
    let title: Text = plan
        .title
        .parse()
        .map_err(|_| CpdError::bad_row(format!("Plan {} has title {:?}", plan.id, plan.title)))?;
    let written = Contexts::new(&env.config, &mut env.store).write(
        &plan.context,
        source,
        writer()?,
        &title,
        draft,
    )?;
    Ok(written.record.id.to_string())
}

/// Step 2, with `--brief-file`: the Context record of kind `brief` (B3-A4), `text` = the brief,
/// `plan` = `{id, machine}`. Returns its id.
fn write_brief(
    env: &mut Env,
    plan: &PlanRow,
    brief: &Text,
    source: &str,
) -> Result<String, CpdError> {
    let draft = Draft::Brief(PlanRef {
        id: plan.id.clone(),
        machine: env.config.machine.clone(),
        jira: None,
    });
    let written = Contexts::new(&env.config, &mut env.store).write(
        &plan.context,
        source,
        writer()?,
        brief,
        draft,
    )?;
    Ok(written.record.id.to_string())
}

/// The caller as the writer of a record (contract §2.2 "Context record" `by`).
fn writer() -> Result<By, CpdError> {
    Ok(By {
        session: env_id("CPD_SESSION", SessionId::parse)?
            .or(env_id("PI_SESSION_ID", SessionId::parse)?),
        plan: env_id("CPD_PLAN", PlanId::parse)?,
    })
}

/// A non-empty variable of the caller's environment, parsed (the writer of a
/// record, contract §2.2 "Context record" `by`).
fn env_id<T>(name: &str, parse: fn(&str) -> Result<T, CpdError>) -> Result<Option<T>, CpdError> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(|v| parse(&v.to_string_lossy()))
        .transpose()
}

/// Step 3: `run/<plan>/lead-message.md`: the header, the brief when one exists, then the pointer line
/// (docs/research/context-retrieval-design.md "First messages").
fn write_message(env: &mut Env, plan: &PlanRow, brief: Option<&Text>) -> Result<(), CpdError> {
    let topics = env
        .store
        .read(|conn| crate::context::curated::topic_counts(conn, &plan.context))?;
    let message = lead_message(plan, brief.map(Text::as_str), &topics);
    write_file(&env.home.lead_message(&plan.id), &message)
}

/// The Lead's first message: the header, the brief when one exists, the `Topics:` line when `topics` is not
/// empty, then the pointer line; no store read, the callers pass the topics
/// (docs/research/context-retrieval-design.md "First messages"). The one rendering `plan new`, the `already`
/// rerun and `session first-message` share.
pub fn lead_message(plan: &PlanRow, brief: Option<&str>, topics: &[(String, usize)]) -> String {
    let mut message = vec![header(plan)];
    if let Some(brief) = brief {
        message.push(brief.trim_end().to_owned());
    }
    if !topics.is_empty() {
        message.push(format!(
            "Topics: {}",
            crate::context::curated::topics_line(topics)
        ));
    }
    message.push(LEAD_POINTER.to_owned());
    message.join("\n\n") + "\n"
}

/// The on-demand reads a Lead's first message points at; its pane has `CPD_PLAN`, so `--context` is omitted
/// (docs/research/context-retrieval-design.md "First messages").
const LEAD_POINTER: &str = "Context on demand: cpd context search <query> · cpd context show --since <seq> · cpd context show --record <id>";

fn header(plan: &PlanRow) -> String {
    format!(
        "You are the Lead of Plan {} \"{}\" in Context {}. Phase grounding: follow the cpd-lead skill.",
        plan.id, plan.title, plan.context
    )
}

/// Step 4: the Lead's window `park-<plan>` (contract §6.2). A durable Lead starts no pane (contract §6.19): the
/// engine's `hosts` step starts its host at the next tick, and there is no pane or window.
fn start_lead(
    env: &mut Env,
    plan: &PlanRow,
    cwd: &str,
    session: &SessionId,
    name: &str,
    runner: WorkerRunner,
    source: &str,
) -> Result<Option<lead::Started>, CpdError> {
    if runner == WorkerRunner::Durable {
        return Ok(None);
    }
    let window = crate::host::layout::park_window(session, Some(&plan.id));
    let lead = Lead {
        session,
        context: &plan.context,
        plan: Some(&plan.id),
        name,
        cwd: Path::new(cwd),
    };
    lead::start(env, &lead, &window, source).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ContextId;
    use crate::plan::Phase;

    #[test]
    fn the_header_names_plan_and_context() {
        let plan = PlanRow {
            id: PlanId::parse("pay-4521").unwrap(),
            context: ContextId::parse("payments").unwrap(),
            title: "Retry failed payouts".into(),
            jira: None,
            phase: Phase::Grounding,
            lead_session: Some(SessionId::parse("s1").unwrap()),
            created_at: "t".into(),
            locked_at: None,
            done_at: None,
            archived_at: None,
            outline: None,
        };
        assert_eq!(
            header(&plan),
            "You are the Lead of Plan pay-4521 \"Retry failed payouts\" in Context payments. Phase grounding: follow the cpd-lead skill."
        );
    }
}
