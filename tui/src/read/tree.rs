//! The `tree` view's snapshot (commands.md §17): Contexts → Plans → Lead
//! Sessions, each with its state, label and name (`context list`, `session list`).
//! A Plan keeps its side forks in `sessions` (its Events and `CPD_PLAN` are the Plan's), but the tree draws them
//! under the Context ([`ContextNode::forks`], B1).

use std::collections::BTreeMap;
use std::path::Path;

use cpd_core::error::CpdError;
use cpd_core::events::highest_id;
use cpd_core::ids::ContextId;
use cpd_core::home::CpdHome;
use cpd_core::inbox::model::{self, Filter, Item, Kind, State, Urgency};
use cpd_core::store::Store;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::cost;
use super::dot::{self, Dot};
use super::shared::{Place, PlanHead, SessionNode, bad_row, id, plan_heads, sessions};

/// Every Context of the machine, by id.
#[derive(Clone, Debug, PartialEq)]
pub struct Tree {
    pub contexts: Vec<ContextNode>,
    /// The highest Event id in this snapshot's read transaction (§17).
    pub events_cursor: i64,
    /// The Inbox's open, visible items (not snoozed, not dismissed) in the order of `cpd inbox list`, as of this
    /// snapshot (§17), `engine-down` first when no engine runs and no live Manager exists. Every dot of the tree comes from the stored ones.
    pub inbox: Vec<Item>,
    /// Whether an engine holds `locks/watch.lock` (§1.4). `engine-down` is never stored: with none and no live Manager,
    /// `inbox` starts with it (id 0, which no stored item has), as `cpd inbox list` does.
    pub engine_running: bool,
}

#[derive(Clone, Debug, PartialEq)]
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
    /// Its archived Manager Sessions, oldest first (commands.md §17, hs-d630, hs-d633): the tree draws them in the
    /// Context's collapsible archived section, after the archived Plans and forks.
    pub archived_managers: Vec<SessionNode>,
    /// The dot of its row: its live (not archived) Manager Session's own dot, `Stopped` with none (§17, hs-d215):
    /// a `●` only from the Manager's own Inbox items (hs-d217), else the glyph of its state.
    pub dot: Dot,
    /// Where its live Manager's chat is, `None` with no Manager: the Context row is selected when it is `Chat`.
    pub manager_place: Option<Place>,
    /// Its live Manager is asleep (§2.2 "Asleep"): the Context row is drawn muted.
    pub manager_asleep: bool,
    /// When it was created (`contexts.created_at`): the tree orders by tier, then creation, newest first (hs-d686, pro-d48).
    pub created_at: String,
    /// Total cost in USD (`tree_at` fills it); `None`: no data.
    pub cost_usd: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanNode {
    pub head: PlanHead,
    /// Total cost in USD (`tree_at` fills it); `None`: no data.
    pub cost_usd: Option<f64>,
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
pub(super) fn store_home(store: &Store) -> Result<CpdHome, CpdError> {
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
        claimed_at: None,
    }
}

/// The tree's snapshot: one read transaction, a fixed set of queries whatever the number of Contexts and Plans.
/// The Inbox's open, visible items (not snoozed, not dismissed) at `now`, in the order of `cpd inbox list` (§17).
pub(super) fn visible_items(conn: &rusqlite::Connection, now: &str) -> Result<Vec<Item>, CpdError> {
    model::list(conn, &Filter { context: None, state: State::Visible { now: now.to_owned() } })
}

/// `engine-down` first in `inbox` when no engine holds the lock and no live Manager exists (§2.2): with a live Manager the
/// engine is restarted and a failure goes to the Manager (hs-d318), so it is no item. The tree's rule and the `widgets` view's.
fn engine_down_first(inbox: &mut Vec<Item>, engine_running: bool, live_manager: bool, now: String) {
    if !engine_running && !live_manager {
        inbox.insert(0, engine_down(now));
    }
}

/// The Inbox's visible items at `now` as `cpd inbox list` has them, `engine-down` first by [`engine_down_first`]: what the
/// `widgets` view draws (the tree reads the same in its own transaction).
pub(super) fn inbox_in(conn: &rusqlite::Connection, now: &str, engine_running: bool) -> Result<Vec<Item>, CpdError> {
    let mut inbox = visible_items(conn, now)?;
    let live_manager: bool =
        conn.query_row("SELECT EXISTS (SELECT 1 FROM sessions WHERE role = 'manager' AND archived_at IS NULL)", [], |r| r.get(0))?;
    engine_down_first(&mut inbox, engine_running, live_manager, now.to_owned());
    Ok(inbox)
}

/// The Inbox's visible items at `now` (contract §1.3 text, the database's clock when `None`) and whether an engine holds the lock:
/// the `widgets` view's read of its Inbox section.
pub fn inbox_at(store: &mut Store, now: Option<&str>) -> Result<(Vec<Item>, bool), CpdError> {
    let engine_running = model::engine_running(&store_home(store)?)?;
    store.read(|conn| Ok((inbox_in(conn, &db_now(conn, now)?, engine_running)?, engine_running)))
}

/// The mark of the Inbox: the open rows' own columns and how many Managers are live (they decide `engine-down`). It moves
/// when an item is opened, closed, snoozed, dismissed, claimed, relabelled or re-linked, and on no write to another table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboxMark {
    rows: Vec<InboxRow>,
    managers: i64,
}

/// `id, urgency, label, url, plan, session, snoozed_until, dismissed_at, claimed_at` of an open item.
type InboxRow = (i64, String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);

/// [`InboxMark`] now: one cheap read of the open items, never the closed history.
pub fn inbox_mark(store: &mut Store) -> Result<InboxMark, CpdError> {
    store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, urgency, label, url, plan, session, snoozed_until, dismissed_at, claimed_at
               FROM inbox_items WHERE closed_at IS NULL ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let managers = conn.query_row("SELECT count(*) FROM sessions WHERE role = 'manager' AND archived_at IS NULL", [], |r| r.get(0))?;
        Ok(InboxMark { rows, managers })
    })
}

/// Each live Lead or fork's dot from the `visible` items (§17, "Which items fill which row"); an archived row keeps
/// its glyph and takes no item. Every view that draws a Lead's dot (the tree, Pulse) uses this one rule.
/// `heads` are the Plans: a Lead of a Plan with a failed Job being worked on gets the red loader (pro-d88), else the bright
/// colour of its Plan's phase (hs-d622); a Session that leads no Plan works cyan.
pub(super) fn fill_dots(visible: &[Item], heads: &[PlanHead], sessions: &mut [SessionNode]) {
    for s in sessions.iter_mut().filter(|s| !s.is_archived()) {
        let lead_of = s.plan.as_ref().filter(|_| s.plan_lead);
        let head = lead_of.and_then(|p| heads.iter().find(|h| &h.id == p));
        let failed = head.is_some_and(|h| h.fixing);
        let phase = head.map(|h| h.phase);
        s.dot = Dot::of(s.state, dot::urgency(visible, &s.id, lead_of), failed, phase);
    }
}

/// When the earliest snooze of an open, undismissed Inbox item that has not ended yet ends; none when no item is
/// snoozed. A snooze ends by the clock with no Event (§17): a view that shows the Inbox reads it again then. Read
/// before the view's snapshot, so that a snooze that ends between the two reads is not missed.
pub fn snooze_end(store: &mut Store) -> Result<Option<OffsetDateTime>, CpdError> {
    snooze_end_at(store, None)
}

/// [`snooze_end`] at `now` (contract §1.3 text), the database's clock when `None`: a view on a clock of its own passes its time.
pub fn snooze_end_at(store: &mut Store, now: Option<&str>) -> Result<Option<OffsetDateTime>, CpdError> {
    let end: Option<String> = store.read(|conn| {
        let now = db_now(conn, now)?;
        Ok(conn.query_row(
            "SELECT min(snoozed_until) FROM inbox_items
              WHERE closed_at IS NULL AND dismissed_at IS NULL AND snoozed_until > ?1",
            [now],
            |r| r.get(0),
        )?)
    })?;
    end.map(|end| OffsetDateTime::parse(&end, &Rfc3339).map_err(|_| bad_row(format!("Inbox snoozed_until {end:?}"))))
        .transpose()
}

/// `now` as given (contract §1.3 text), or the database's own clock.
pub(super) fn db_now(conn: &rusqlite::Connection, now: Option<&str>) -> Result<String, CpdError> {
    match now {
        Some(now) => Ok(now.to_owned()),
        None => Ok(conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |r| r.get(0))?),
    }
}

pub fn tree(store: &mut Store) -> Result<Tree, CpdError> {
    tree_at(store, None)
}

/// [`tree`] with the Inbox's snoozes read at `now` (contract §1.3 text), the database's clock when `None`.
pub fn tree_at(store: &mut Store, now: Option<&str>) -> Result<Tree, CpdError> {
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
        let now = db_now(conn, now)?;
        let mut inbox = visible_items(conn, &now)?;

        let mut leads: Vec<SessionNode> = sessions(conn, "s.role = 'lead'", &[])?;
        let mut heads = plan_heads(conn, None)?;
        fill_dots(&inbox, &heads, &mut leads);
        for s in &mut leads {
            s.cost_usd = s.file.as_deref().and_then(|f| cost::own_cost(Path::new(f)));
        }
        // Every Attempt of every Plan, in one query: the stored `metrics` is the only source when written, else the
        // Worker Session's file.
        let mut attempt_costs: BTreeMap<String, Vec<Option<f64>>> = BTreeMap::new();
        let mut stmt = conn.prepare(
            "SELECT a.plan, json_extract(a.metrics, '$.cost_usd'), a.metrics IS NULL, s.file
               FROM attempts a LEFT JOIN sessions s ON s.id = a.worker_session",
        )?;
        for row in stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<rusqlite::types::Value>>(1)?, r.get::<_, bool>(2)?, r.get::<_, Option<String>>(3)?))
        })? {
            let (plan, stored, no_metrics, file) = row?;
            let cost = cost::attempt_cost(stored, no_metrics, file.as_deref());
            attempt_costs.entry(plan).or_default().push(cost);
        }
        // Every Manager Session, live or archived (`core/src/context/manager.rs`). The live one fills the Context
        // row's dot, a `●` only from the Manager's own items (hs-d215, hs-d217); each archived one goes into the
        // Context's archived section, oldest first (hs-d630, hs-d633).
        let mut all_managers: Vec<SessionNode> = sessions(conn, "s.role = 'manager'", &[])?;
        for s in &mut all_managers {
            s.cost_usd = s.file.as_deref().and_then(|f| cost::own_cost(Path::new(f)));
        }
        let mut manager_costs: BTreeMap<ContextId, Vec<Option<f64>>> = BTreeMap::new();
        for s in &all_managers {
            manager_costs.entry(s.context.clone()).or_default().push(s.cost_usd);
        }
        let managers: BTreeMap<ContextId, (Dot, Place, bool)> = all_managers
            .iter()
            .filter(|s| !s.is_archived())
            .map(|s| {
                let dot = Dot::of(s.state, dot::urgency(&inbox, &s.id, None), false, None);
                (s.context.clone(), (dot, s.place, s.is_asleep()))
            })
            .collect();
        let mut archived_managers: BTreeMap<ContextId, Vec<SessionNode>> = BTreeMap::new();
        for manager in all_managers.into_iter().filter(SessionNode::is_archived) {
            archived_managers.entry(manager.context.clone()).or_default().push(manager);
        }

        let mut stmt = conn.prepare(
            "SELECT id, title, jira, archived_at, created_at
               FROM contexts WHERE deleted_at IS NULL ORDER BY id",
        )?;
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
                    let attempts = attempt_costs.remove(&head.id.to_string()).unwrap_or_default();
                    let cost_usd = cost::total(own.iter().map(|s| s.cost_usd).chain(attempts));
                    PlanNode { head, sessions: own, cost_usd }
                })
                .collect::<Vec<PlanNode>>();
            let cost_usd = cost::total(plans.iter().map(|p| p.cost_usd).chain(manager_costs.remove(&context).unwrap_or_default()));
            let archived_at: Option<String> = r.get(3)?;
            contexts.push(ContextNode {
                dot: managers.get(&context).map_or(Dot::Stopped, |m| m.0),
                manager_place: managers.get(&context).map(|m| m.1),
                manager_asleep: managers.get(&context).is_some_and(|m| m.2),
                created_at: r.get(4)?,
                title: r.get(1)?,
                jira: r.get(2)?,
                records: records.remove(&cid).unwrap_or(0),
                open_questions: questions.remove(&cid).unwrap_or(0),
                archived_at,
                plans,
                archived_managers: archived_managers.remove(&context).unwrap_or_default(),
                cost_usd,
                id: context,
            });
        }
        engine_down_first(&mut inbox, engine_running, !managers.is_empty(), now);
        Ok(Tree { contexts, events_cursor: highest_id(conn)?, inbox, engine_running })
    })
}
