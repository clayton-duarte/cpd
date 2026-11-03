//! The `sessions` row as read (contract §2.2 Session), with what commands.md
//! §10.1 derives from other tables, and `session list` and `session show`.

use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::{Env, ListArgs, ShowArgs, session_not_found};
use crate::args::SessionArg;
use crate::config::{Letter, Tier};
use crate::error::CpdError;
use crate::events::highest_id;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::Output;
use crate::policy::model::Models;

/// A Session's role (§2.2).
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Manager,
    Lead,
    Worker,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Manager => "manager",
            Role::Lead => "lead",
            Role::Worker => "worker",
        }
    }

    fn parse(s: &str) -> Option<Role> {
        [Role::Manager, Role::Lead, Role::Worker]
            .into_iter()
            .find(|r| r.as_str() == s)
    }
}

/// What runs a Session (contract §2.2 `runner`): pi in a tmux pane, or a durable host with no pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Runner {
    Pi,
    Durable,
}

impl Runner {
    pub fn as_str(self) -> &'static str {
        match self {
            Runner::Pi => "pi",
            Runner::Durable => "durable",
        }
    }

    fn parse(s: &str) -> Option<Runner> {
        [Runner::Pi, Runner::Durable]
            .into_iter()
            .find(|r| r.as_str() == s)
    }
}

/// A Session's agent state (§3.6): `blocked` needs the User, `waiting` needs another agent (hs-d303).
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Working,
    Waiting,
    Blocked,
    Idle,
    Stopped,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Working => "working",
            State::Waiting => "waiting",
            State::Blocked => "blocked",
            State::Idle => "idle",
            State::Stopped => "stopped",
        }
    }

    /// The states that say why: `waiting` and `blocked` have a label, no other state has one (§3.6).
    pub fn has_label(self) -> bool {
        matches!(self, State::Waiting | State::Blocked)
    }

    fn parse(s: &str) -> Option<State> {
        [
            State::Working,
            State::Waiting,
            State::Blocked,
            State::Idle,
            State::Stopped,
        ]
        .into_iter()
        .find(|v| v.as_str() == s)
    }
}

/// One Session, parsed at the boundary.
#[derive(Clone, Debug)]
pub(super) struct Row {
    pub id: SessionId,
    pub role: Role,
    pub context: ContextId,
    pub plan: Option<PlanId>,
    pub attempt: Option<String>,
    pub name: String,
    pub file: Option<String>,
    pub forked_from: Option<SessionId>,
    pub cwd: String,
    pub tier: String,
    pub tmux_pane: Option<String>,
    pub state: State,
    pub state_label: Option<String>,
    pub state_at: String,
    pub created_at: String,
    /// The `place` of its latest `session-moved` Event, else `none`.
    pub place: String,
    /// It is its Plan's `lead_session`.
    pub plan_lead: bool,
    /// The id of its `session-registered` Event.
    pub registered_event: Option<i64>,
    /// When it was archived (contract §2.2 "Archived"), else none.
    pub archived_at: Option<String>,
    /// When engine E13 put it to sleep (contract §2.2 "Asleep"), else none.
    pub asleep_at: Option<String>,
    /// Its dynamic title (contract §2.2, hs-d257), written by `session retitle`, and when.
    pub dynamic_title: Option<String>,
    pub titled_at: Option<String>,
    /// The User created it from the tree: no first message until their first prompt (hs-d542).
    pub by_user: bool,
    /// The failed model switch the pi adapter recorded (`session switch-failed`, commands.md §10.18a): `{tier, code, message, at}`.
    pub switch_failed: Option<Value>,
    /// What runs it (`sessions.runner`).
    pub runner: Runner,
}

const SELECT: &str = "SELECT s.id, s.role, s.context, s.plan, s.attempt, s.name, s.file, s.forked_from, s.cwd, s.tier,
       s.tmux_pane, s.state, s.state_label, s.state_at, s.created_at,
       (SELECT json_extract(e.data, '$.place') FROM events e
         WHERE e.session = s.id AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1),
       l.lead_session IS NOT NULL,
       (SELECT min(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-registered'),
       s.archived_at, s.title, s.titled_at, s.asleep_at, s.by_user, s.switch_failed, s.runner
  FROM sessions s
  LEFT JOIN (SELECT DISTINCT lead_session FROM plans) l ON l.lead_session = s.id";

/// A row as SQLite returns it, before parsing.
struct Raw {
    id: String,
    role: String,
    context: String,
    plan: Option<String>,
    attempt: Option<String>,
    name: String,
    file: Option<String>,
    forked_from: Option<String>,
    cwd: String,
    tier: String,
    tmux_pane: Option<String>,
    state: String,
    state_label: Option<String>,
    state_at: String,
    created_at: String,
    place: Option<String>,
    plan_lead: bool,
    registered_event: Option<i64>,
    archived_at: Option<String>,
    title: Option<String>,
    titled_at: Option<String>,
    asleep_at: Option<String>,
    by_user: bool,
    switch_failed: Option<String>,
    runner: String,
}

fn raw(r: &rusqlite::Row) -> rusqlite::Result<Raw> {
    Ok(Raw {
        id: r.get(0)?,
        role: r.get(1)?,
        context: r.get(2)?,
        plan: r.get(3)?,
        attempt: r.get(4)?,
        name: r.get(5)?,
        file: r.get(6)?,
        forked_from: r.get(7)?,
        cwd: r.get(8)?,
        tier: r.get(9)?,
        tmux_pane: r.get(10)?,
        state: r.get(11)?,
        state_label: r.get(12)?,
        state_at: r.get(13)?,
        created_at: r.get(14)?,
        place: r.get(15)?,
        plan_lead: r.get(16)?,
        registered_event: r.get(17)?,
        archived_at: r.get(18)?,
        title: r.get(19)?,
        titled_at: r.get(20)?,
        asleep_at: r.get(21)?,
        by_user: r.get(22)?,
        switch_failed: r.get(23)?,
        runner: r.get(24)?,
    })
}

impl Row {
    /// The header of its pane (contract §6.1): its dynamic title when it has one, else its
    /// name (its key for an agent).
    pub fn title(&self, _conn: &Connection) -> Result<String, CpdError> {
        Ok(self
            .dynamic_title
            .clone()
            .unwrap_or_else(|| self.name.clone()))
    }

    /// Sets the pill of `pane`: see [`set_pill`].
    pub fn set_pill(
        &self,
        tmux: &crate::host::Tmux,
        pane: &crate::host::PaneId,
    ) -> Result<(), CpdError> {
        set_pill(
            tmux,
            pane,
            self.dynamic_title.as_deref(),
            &self.name,
            self.forked_from.is_some(),
        )
    }

    /// Its agent key: its name, unless it is a fork (not an agent).
    pub fn key(&self) -> Option<&str> {
        match &self.forked_from {
            Some(_) => None,
            None => Some(&self.name),
        }
    }

    fn parse(raw: Raw) -> Result<Row, CpdError> {
        let id =
            SessionId::parse(&raw.id).map_err(|_| CpdError::bad_row(format!("id {:?}", raw.id)))?;
        let field =
            |name: &str, value: &str| CpdError::bad_row(format!("{id} has {name} {value:?}"));
        Ok(Row {
            role: Role::parse(&raw.role).ok_or_else(|| field("role", &raw.role))?,
            context: ContextId::parse(&raw.context).map_err(|_| field("context", &raw.context))?,
            plan: raw
                .plan
                .map(|p| PlanId::parse(&p).map_err(|_| field("plan", &p)))
                .transpose()?,
            forked_from: raw
                .forked_from
                .map(|f| SessionId::parse(&f).map_err(|_| field("forked_from", &f)))
                .transpose()?,
            state: State::parse(&raw.state).ok_or_else(|| field("state", &raw.state))?,
            attempt: raw.attempt,
            name: raw.name,
            file: raw.file,
            cwd: raw.cwd,
            tier: raw.tier,
            tmux_pane: raw.tmux_pane,
            state_label: raw.state_label,
            state_at: raw.state_at,
            created_at: raw.created_at,
            place: raw.place.unwrap_or_else(|| "none".to_owned()),
            plan_lead: raw.plan_lead,
            registered_event: raw.registered_event,
            archived_at: raw.archived_at,
            asleep_at: raw.asleep_at,
            dynamic_title: raw.title,
            titled_at: raw.titled_at,
            by_user: raw.by_user,
            runner: Runner::parse(&raw.runner).ok_or_else(|| field("runner", &raw.runner))?,
            switch_failed: raw
                .switch_failed
                .map(|j| {
                    serde_json::from_str::<Value>(&j)
                        .ok()
                        .filter(Value::is_object)
                        .ok_or_else(|| field("switch_failed", &j))
                })
                .transpose()?,
            id,
        })
    }

    /// The fields of `session list` (commands.md §10.1).
    pub fn list_json(&self) -> Value {
        json!({
            "id": self.id,
            "role": self.role.as_str(),
            "context": self.context,
            "plan": self.plan,
            "attempt": self.attempt,
            "name": self.name,
            "key": self.key(),
            "state": self.state.as_str(),
            "state_label": self.state_label,
            "state_at": self.state_at,
            "archived_at": self.archived_at,
            "asleep_at": self.asleep_at,
            "place": self.place,
            "tmux_pane": self.tmux_pane,
            "file": self.file,
            "forked_from": self.forked_from,
            "plan_lead": self.plan_lead,
            "tier": self.tier,
            "cwd": self.cwd,
            "title": self.dynamic_title,
            "titled_at": self.titled_at,
            "by_user": self.by_user,
            "runner": self.runner.as_str(),
        })
    }
}

/// Every Session matching `filter` (SQL after `WHERE`, with `params`), oldest first.
fn query(conn: &Connection, filter: &str, params: Vec<Sql>) -> Result<Vec<Row>, CpdError> {
    // The Lead of a deleted Plan stays in `sessions` and is hidden with its Plan (contract §2.2 "Deleted").
    let sql = format!(
        "{SELECT} WHERE ({filter}) AND NOT EXISTS (SELECT 1 FROM plans d WHERE d.id = s.plan AND d.deleted_at IS NOT NULL) ORDER BY s.created_at, s.id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params), raw)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(Row::parse).collect()
}

/// Every Lead and Manager that is not archived, oldest first.
pub(super) fn live(conn: &Connection) -> Result<Vec<Row>, CpdError> {
    query(
        conn,
        "s.role IN ('lead', 'manager') AND s.archived_at IS NULL",
        Vec::new(),
    )
}

/// The fields a live model switch needs (commands.md §14.6 step 7): the same live selection `session retier` uses.
#[derive(Clone, Debug)]
pub(crate) struct ModelSwitchRow {
    pub id: SessionId,
    pub role: Role,
    pub context: ContextId,
    pub plan: Option<PlanId>,
    pub tier: String,
}

/// Every live Lead and Manager that is not archived, oldest first, as the model-switch step of `cpd preset apply`
/// reads them (commands.md §14.6 step 7).
pub(crate) fn live_model_switch(conn: &Connection) -> Result<Vec<ModelSwitchRow>, CpdError> {
    Ok(query(
        conn,
        "s.role IN ('lead', 'manager') AND s.archived_at IS NULL",
        Vec::new(),
    )?
    .into_iter()
    .map(|r| ModelSwitchRow {
        id: r.id,
        role: r.role,
        context: r.context,
        plan: r.plan,
        tier: r.tier,
    })
    .collect())
}

/// The model and thinking level pi is told for `tier` (commands.md §10.2, §10.8): the `<letter>` ref of the Session's `Models` (its Context's key) and
/// the level of the Tier's digit. The one function `session show` and `session run` share, so a Session retiered and a
/// Session started later are on the same model.
pub(crate) fn model_and_level(
    models: &Models,
    id: &SessionId,
    tier: &str,
) -> Result<(String, &'static str), CpdError> {
    let tier =
        Tier::parse(tier).map_err(|_| CpdError::bad_row(format!("{id} has tier {tier:?}")))?;
    let model = match tier.letter() {
        Letter::O => &models.o,
        Letter::S => &models.s,
        Letter::H => &models.h,
        Letter::G => &models.g,
    };
    Ok((model.clone(), tier.level()))
}

/// The models of the Session's Context (contract §8.3 "Models" (5)): its owner's key, or the machine layer.
pub(super) fn models_of(env: &mut Env, row: &Row) -> Result<Models, CpdError> {
    let home = env.home.clone();
    env.store
        .read(|conn| crate::policy::model::models_of_context(conn, &home, row.context.as_str()))
}

/// Sets `@cpd_title` of `pane`, the one place the pill is chosen: the dynamic title (Title Case)
/// when there is one, else the agent key verbatim, else (a fork) the name in Title Case.
pub(crate) fn set_pill(
    tmux: &crate::host::Tmux,
    pane: &crate::host::PaneId,
    dynamic: Option<&str>,
    name: &str,
    fork: bool,
) -> Result<(), CpdError> {
    match (dynamic, fork) {
        (Some(title), _) => tmux.set_pane_title(pane, title),
        (None, false) => tmux.set_pane_key(pane, name),
        (None, true) => tmux.set_pane_title(pane, name),
    }
}

/// The Session `id`, if it exists.
pub(super) fn find(conn: &Connection, id: &SessionId) -> Result<Option<Row>, CpdError> {
    Ok(query(conn, "s.id = ?", vec![Sql::Text(id.to_string())])?.pop())
}

/// The Session `id`, else `session_not_found`.
pub(super) fn load(conn: &Connection, id: &SessionId) -> Result<Row, CpdError> {
    find(conn, id)?.ok_or_else(|| session_not_found(id))
}

/// Sets `@cpd_title` on the Session's live pane from its current row, if it has one.
/// `session rename` runs it, also when the name was already the new one: a rerun
/// repairs the header.
pub(super) fn retitle(id: &SessionId) -> Result<(), CpdError> {
    let mut env = Env::open()?;
    let row = env.store.read(|conn| load(conn, id))?;
    let tmux = env.tmux();
    for pane in tmux
        .list_panes()?
        .iter()
        .filter(|p| !p.dead && p.cpd_session.as_ref() == Some(id))
    {
        row.set_pill(&tmux, &pane.pane)?;
    }
    Ok(())
}

/// `session list` (commands.md §10.1).
pub(super) fn list(args: ListArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let mut filter = vec!["1".to_owned()];
    let mut params = Vec::new();
    if !args.archived {
        filter.push("s.archived_at IS NULL".into());
    }
    if let Some(context) = &args.context {
        filter.push("s.context = ?".into());
        params.push(Sql::Text(context.to_string()));
    }
    if let Some(plan) = &args.plan {
        filter.push("s.plan = ?".into());
        params.push(Sql::Text(plan.to_string()));
    }
    let mut any_of = |column: &str, values: Vec<&'static str>| {
        if !values.is_empty() {
            filter.push(format!(
                "s.{column} IN ({})",
                vec!["?"; values.len()].join(", ")
            ));
            params.extend(values.into_iter().map(|v| Sql::Text(v.to_owned())));
        }
    };
    any_of("role", args.roles.iter().map(|r| r.as_str()).collect());
    any_of("state", args.states.iter().map(|s| s.as_str()).collect());
    let (sessions, cursor) = env.store.read(|conn| {
        Ok((
            query(conn, &filter.join(" AND "), params)?,
            highest_id(conn)?,
        ))
    })?;
    let sessions: Vec<Value> = sessions.iter().map(Row::list_json).collect();
    Ok(Output::success(
        json!({"sessions": sessions, "events_cursor": cursor}),
    ))
}

/// `session show` (commands.md §10.2).
pub(super) fn show(args: ShowArgs) -> Result<Output, CpdError> {
    let reference = SessionArg { session: args.id }.require()?.clone();
    let mut env = Env::open()?;
    let id = env.resolve(&reference)?;
    let (row, escalations) = env.store.read(|conn| {
        let row = load(conn, &id)?;
        let escalations = match (&row.plan, row.plan_lead) {
            (Some(plan), true) => Some(open_escalations(conn, plan)?),
            _ => None,
        };
        Ok((row, escalations))
    })?;
    let mut session = row.list_json();
    session["created_at"] = json!(row.created_at);
    session["registered_event"] = json!(row.registered_event);
    let models = models_of(&mut env, &row)?;
    let (model, thinking) = model_and_level(&models, &row.id, &row.tier)?;
    session["model"] = json!(model);
    session["thinking"] = json!(thinking);
    session["loop_guard_turns"] = json!(env.config.loop_guard_turns);
    // Its Tier is what `next` relaunches pi on: `session run` starts pi on the row's tier (commands.md §10.2, §10.8).
    session["switch"] = match &row.switch_failed {
        Some(failed) => {
            let mut switch = failed.clone();
            switch["next"] = json!(format!("cpd session run {}", row.id));
            switch
        }
        None => Value::Null,
    };
    let mut out = json!({"session": session});
    if let Some(escalations) = escalations {
        out["open_escalations"] = json!(escalations);
    }
    Ok(Output::success(out))
}

/// The ids of the Plan's `open` Escalations, oldest first.
fn open_escalations(conn: &Connection, plan: &PlanId) -> Result<Vec<String>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM escalations WHERE plan = ?1 AND status = 'open' ORDER BY asked_at, id",
    )?;
    let ids = stmt
        .query_map([plan.as_str()], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

/// The Plan's `lead_session`, if the Plan exists.
pub(super) fn plan_lead(
    conn: &Connection,
    plan: &PlanId,
) -> Result<Option<(ContextId, String)>, CpdError> {
    let found: Option<(String, String)> = conn
        .query_row(
            "SELECT context, lead_session FROM plans WHERE id = ?1",
            [plan.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    found
        .map(|(context, lead)| {
            let context = ContextId::parse(&context)
                .map_err(|_| CpdError::bad_row(format!("Plan {plan} has context {context:?}")))?;
            Ok((context, lead))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::CpdHome;
    use crate::store::Store;

    /// sql-F5, commands-F4: no table is scanned once per Session. Every `SCAN` of
    /// the plan of `SELECT` hangs off the top level, never off a correlated subquery.
    #[test]
    fn select_scans_no_table_per_session() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        let plan: Vec<(i64, i64, String)> = store
            .read(|conn| {
                let mut stmt = conn.prepare(&format!(
                    "EXPLAIN QUERY PLAN {SELECT} WHERE 1 ORDER BY s.created_at, s.id"
                ))?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(3)?)))?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap();
        let per_session = |mut parent: i64| loop {
            match plan.iter().find(|(id, ..)| *id == parent) {
                Some((_, _, detail)) if detail.starts_with("CORRELATED") => return true,
                Some((_, up, _)) => parent = *up,
                None => return false,
            }
        };
        let scans: Vec<&str> = plan
            .iter()
            .filter(|(_, parent, detail)| detail.starts_with("SCAN") && per_session(*parent))
            .map(|(.., detail)| detail.as_str())
            .collect();
        assert!(
            scans.is_empty(),
            "scanned once per Session: {scans:?}\n{plan:#?}"
        );
    }
}
