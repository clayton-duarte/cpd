//! The `tree` view's snapshot (commands.md §17): Contexts → Plans → Lead
//! Sessions, each with its state, label and name (`context list`, `session list`).
//! A Plan keeps its side forks in `sessions` (its Events and `CPD_PLAN` are the Plan's), but the tree draws them
//! under the Context ([`ContextNode::forks`], B1).

use std::collections::BTreeMap;

use cpd_core::error::CpdError;
use cpd_core::events::highest_id;
use cpd_core::ids::{ContextId, SessionId};
use cpd_core::home::CpdHome;
use cpd_core::inbox::model::{self, Filter, Item, Kind, State, Urgency};
use cpd_core::store::Store;

use super::dot::{self, Dot};
use super::shared::{PlanHead, SessionNode, id, plan_heads, sessions};

/// Every Context of the machine, by id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    pub contexts: Vec<ContextNode>,
    /// The highest Event id in this snapshot's read transaction (§17).
    pub events_cursor: i64,
    /// The Inbox's open, visible items (not snoozed, not dismissed) in the order of `cpd inbox list`, as of this
    /// snapshot (§17), `engine-down` first when no engine runs. Every dot of the tree comes from the stored ones.
    pub inbox: Vec<Item>,
    /// Whether an engine holds `locks/watch.lock` (§1.4). `engine-down` is never stored: with none, `inbox` starts
    /// with it (id 0, which no stored item has), as `cpd inbox list` does.
    pub engine_running: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextNode {
    pub id: ContextId,
    pub title: String,
    pub jira: Option<String>,
    pub records: usize,
    /// Questions no answer refers to.
    pub open_questions: usize,
    /// When it was archived (contract §4.5); archived Contexts are in the model.
    pub archived_at: Option<String>,
    /// Its Plans, oldest first.
    pub plans: Vec<PlanNode>,
    /// The dot of its row: a `●` only from its Manager Session's own items, else the most urgent glyph of its live
    /// (not archived) Sessions, `Stopped` with none (§17).
    pub dot: Dot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanNode {
    pub head: PlanHead,
    /// The Plan's Lead first, then its side forks, oldest first.
    pub sessions: Vec<SessionNode>,
}

impl PlanNode {
    /// The Lead of the Plan, which is in the list unless the database lost it.
    pub fn lead(&self) -> Option<&SessionNode> {
        self.sessions.iter().find(|s| s.plan_lead)
    }
}

impl ContextNode {
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }

    /// The Sessions forked by `session fork` (B1, contract §6.8), each with the Plan it belongs to, oldest first
    /// (ties by Plan, then by the order in the Plan). The tree draws them directly under the Context, after the
    /// Plans, whatever their Plan or source.
    pub fn forks(&self) -> Vec<(&PlanNode, &SessionNode)> {
        let mut forks: Vec<(&PlanNode, &SessionNode)> =
            self.plans.iter().flat_map(|p| p.sessions.iter().filter(|s| !s.plan_lead).map(move |s| (p, s))).collect();
        forks.sort_by(|a, b| a.1.created_at.cmp(&b.1.created_at)); // stable
        forks
    }
}

/// The home the store lives in: the store file is `<home>/cpd.db`.
fn store_home(store: &Store) -> Result<CpdHome, CpdError> {
    let root = store.path().parent().map(|p| p.as_os_str().to_owned());
    CpdHome::resolve(root, None)
}

/// The item `engine-down` (§2.2), opened `now`: never stored, so it has no id (0) and no links.
fn engine_down(now: String) -> Item {
    Item {
        id: 0,
        kind: Kind::EngineDown,
        urgency: Urgency::Red,
        subject: "engine".to_owned(),
        label: model::ENGINE_DOWN_LABEL.to_owned(),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: now,
        closed_at: None,
        snoozed_until: None,
        dismissed_at: None,
    }
}

/// The tree's snapshot: one read transaction, a fixed set of queries whatever the number of Contexts and Plans.
pub fn tree(store: &mut Store) -> Result<Tree, CpdError> {
    let home = store_home(store)?;
    let engine_running = model::engine_running(&home)?;
    store.read(|conn| {
        let mut records: BTreeMap<String, usize> = BTreeMap::new();
        let mut stmt = conn.prepare("SELECT context, count(*) FROM context_records GROUP BY context")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))? {
            let (context, n) = row?;
            records.insert(context, n);
        }
        let mut questions: BTreeMap<String, usize> = BTreeMap::new();
        let mut stmt = conn.prepare(
            "SELECT q.context, count(*) FROM context_records q WHERE q.kind = 'question'
               AND NOT EXISTS (SELECT 1 FROM context_records a
                               WHERE a.context = q.context AND a.kind = 'answer' AND a.question = q.id)
              GROUP BY q.context",
        )?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))? {
            let (context, n) = row?;
            questions.insert(context, n);
        }

        // Read first: the read transaction is one snapshot, so the items and the Sessions agree.
        let now: String = conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |r| r.get(0))?;
        let mut inbox = model::list(conn, &Filter { context: None, state: State::Visible { now: now.clone() } })?;

        let mut managers: BTreeMap<String, SessionId> = BTreeMap::new();
        let mut stmt = conn.prepare("SELECT context, id FROM sessions WHERE role = 'manager' AND archived_at IS NULL")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (context, session) = row?;
            managers.insert(context, id("Manager Session id", &session, SessionId::parse)?);
        }

        let mut leads: Vec<SessionNode> = sessions(conn, "s.role = 'lead'", &[])?;
        for s in leads.iter_mut().filter(|s| !s.is_archived()) {
            // An archived row keeps its glyph and takes no item (§17).
            s.dot = Dot::of(s.state, dot::urgency(&inbox, &s.id, s.plan.as_ref().filter(|_| s.plan_lead)));
        }
        let mut heads = plan_heads(conn, None)?;

        let mut stmt = conn
            .prepare("SELECT id, title, jira, archived_at FROM contexts ORDER BY id")?;
        let mut rows = stmt.query([])?;
        let mut contexts = Vec::new();
        while let Some(r) = rows.next()? {
            let cid: String = r.get(0)?;
            let context = id("Context id", &cid, ContextId::parse)?;
            let (mine, rest): (Vec<PlanHead>, Vec<PlanHead>) = heads.drain(..).partition(|h| h.context == context);
            heads = rest;
            let plans = mine
                .into_iter()
                .map(|head| {
                    let mut own: Vec<SessionNode> = leads.iter().filter(|s| s.plan.as_ref() == Some(&head.id)).cloned().collect();
                    own.sort_by_key(|s| !s.plan_lead); // stable: the Lead first, the rest oldest first
                    PlanNode { head, sessions: own }
                })
                .collect::<Vec<PlanNode>>();
            let archived_at: Option<String> = r.get(3)?;
            let manager_item = match (&archived_at, managers.get(&cid)) {
                (None, Some(manager)) => dot::urgency(&inbox, manager, None),
                _ => None,
            };
            // The Context's `●` is its Manager's alone; the glyph is the most urgent of its live Sessions' states.
            let dot = match manager_item {
                Some(urgency) => Dot::from(urgency),
                None => plans
                    .iter()
                    .flat_map(|p| &p.sessions)
                    .filter(|s| !s.is_archived())
                    .map(|s| Dot::glyph(s.state))
                    .max()
                    .unwrap_or(Dot::Stopped),
            };
            contexts.push(ContextNode {
                dot,
                title: r.get(1)?,
                jira: r.get(2)?,
                records: records.remove(&cid).unwrap_or(0),
                open_questions: questions.remove(&cid).unwrap_or(0),
                archived_at,
                plans,
                id: context,
            });
        }
        if !engine_running {
            inbox.insert(0, engine_down(now));
        }
        Ok(Tree { contexts, events_cursor: highest_id(conn)?, inbox, engine_running })
    })
}
