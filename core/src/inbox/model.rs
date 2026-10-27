//! Inbox items (contract §2.2 "Inbox item", §4.4 `inbox_items`) and the
//! reconcile of engine step E8 (§3.7): the one write path that opens and
//! closes them. The commands only hide items (commands.md §21).

use std::collections::BTreeSet;
use std::fs::File;

use rusqlite::{Connection, Row, params};

use crate::error::{CpdError, ErrorCode};
use crate::events::{self, NewEvent};
use crate::home::CpdHome;
use crate::ids::{ContextId, JobKey, PlanId, RecordId, SessionId};
use crate::lock;
use crate::store::Store;

/// What an item is about (§2.2's kinds). `EngineDown` is never stored, and the
/// `pr-*` and `review-requested` kinds are stored once the PR poll's migration
/// lets `inbox_items.kind` hold them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Dialog,
    PlanReview,
    /// A tracked plannotator session is open (hs-d496); E8 computes none yet.
    ReviewWaiting,
    Unread,
    JobStuck,
    EngineError,
    EngineDown,
    Question,
    /// No longer opened, hs-d470; only history rows an older engine wrote.
    PrFailing,
    /// No longer opened, hs-d470; only history rows an older engine wrote.
    PrChanges,
    PrReady,
    ReviewRequested,
    /// A Context has pending record retirements (hs-d551).
    CurateRetire,
    /// A Context has pending documentation candidates (hs-d551).
    CurateDocs,
    /// A Lead or Manager blocked by the loop guard (hs-d552, hs-d571); always red.
    LoopGuard,
    /// A workflow run waits for the User's approval (pro-d100): one entry of `run/deploy-runs.json`.
    Deploy,
}

impl Kind {
    /// The kind as stored and printed.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Dialog => "dialog",
            Kind::PlanReview => "plan-review",
            Kind::ReviewWaiting => "review-waiting",
            Kind::Unread => "unread",
            Kind::JobStuck => "job-stuck",
            Kind::EngineError => "engine-error",
            Kind::EngineDown => "engine-down",
            Kind::Question => "question",
            Kind::PrFailing => "pr-failing",
            Kind::PrChanges => "pr-changes",
            Kind::PrReady => "pr-ready",
            Kind::ReviewRequested => "review-requested",
            Kind::CurateRetire => "curate-retire",
            Kind::CurateDocs => "curate-docs",
            Kind::LoopGuard => "loop-guard",
            Kind::Deploy => "deploy",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        use Kind::*;
        [Dialog, PlanReview, ReviewWaiting, Unread, JobStuck, EngineError, EngineDown, Question, PrFailing, PrChanges, PrReady, ReviewRequested, CurateRetire, CurateDocs, LoopGuard, Deploy]
            .into_iter()
            .find(|k| k.as_str() == s)
    }
}

/// How soon an item needs the User; the order is the rank of §2.2's "Order",
/// last is first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Urgency {
    White,
    /// A cpd notice that blocks nothing (hs-d570).
    Cyan,
    /// External support: a review asked of the User (pro-d58).
    Blue,
    Magenta,
    Yellow,
    Red,
}

impl Urgency {
    pub fn as_str(self) -> &'static str {
        match self {
            Urgency::White => "white",
            Urgency::Magenta => "magenta",
            Urgency::Yellow => "yellow",
            Urgency::Red => "red",
            Urgency::Cyan => "cyan",
            Urgency::Blue => "blue",
        }
    }

    pub fn parse(s: &str) -> Option<Urgency> {
        [Urgency::White, Urgency::Cyan, Urgency::Blue, Urgency::Magenta, Urgency::Yellow, Urgency::Red].into_iter().find(|u| u.as_str() == s)
    }
}

/// A thing that needs the User right now, as the rules compute it. Its
/// identity is (`kind`, `subject`, `urgency`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Condition {
    pub kind: Kind,
    pub urgency: Urgency,
    pub subject: String,
    pub label: String,
    pub url: Option<String>,
    pub context: Option<ContextId>,
    pub plan: Option<PlanId>,
    pub job: Option<JobKey>,
    pub session: Option<SessionId>,
    pub record: Option<RecordId>,
    /// The Event that caused it (§2.2).
    pub event: Option<i64>,
    /// When the item opens at, when not now: a `deploy` item's age counts from when its run began waiting.
    pub opened_at: Option<String>,
}

impl Condition {
    fn identity(&self) -> (Kind, &str, Urgency) {
        (self.kind, &self.subject, self.urgency)
    }
}

/// A row of `inbox_items`. Times are §1.3's RFC 3339 strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: i64,
    pub kind: Kind,
    pub urgency: Urgency,
    pub subject: String,
    pub label: String,
    pub url: Option<String>,
    pub context: Option<ContextId>,
    pub plan: Option<PlanId>,
    pub job: Option<JobKey>,
    pub session: Option<SessionId>,
    pub record: Option<RecordId>,
    pub event: Option<i64>,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub snoozed_until: Option<String>,
    pub dismissed_at: Option<String>,
    pub claimed_at: Option<String>,
}

impl Item {
    fn identity(&self) -> (Kind, &str, Urgency) {
        (self.kind, &self.subject, self.urgency)
    }
}

impl From<&Item> for Condition {
    /// The condition that keeps `item` as it is: the rules re-emit an open item whose cause they cannot read now.
    fn from(item: &Item) -> Condition {
        Condition {
            kind: item.kind,
            urgency: item.urgency,
            subject: item.subject.clone(),
            label: item.label.clone(),
            url: item.url.clone(),
            context: item.context.clone(),
            plan: item.plan.clone(),
            job: item.job.clone(),
            session: item.session.clone(),
            record: item.record.clone(),
            event: item.event,
            opened_at: None,
        }
    }
}

const COLUMNS: &str = "id, kind, urgency, subject, label, url, context, plan, job, session, record, event,
                       opened_at, closed_at, snoozed_until, dismissed_at, claimed_at";

/// A stored value that does not parse: the database was written outside cpd.
fn corrupt(id: i64, column: &str, value: &str) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("Inbox item {id} has an invalid {column}: {value:?}."))
        .with("item", id)
        .with("column", column)
}

fn id_column<T>(
    id: i64,
    column: &str,
    value: Option<String>,
    parse: fn(&str) -> Result<T, CpdError>,
) -> Result<Option<T>, CpdError> {
    value.map(|v| parse(&v).map_err(|_| corrupt(id, column, &v))).transpose()
}

impl Item {
    fn from_row(row: &Row<'_>) -> Result<Item, CpdError> {
        let id: i64 = row.get(0)?;
        let kind: String = row.get(1)?;
        let urgency: String = row.get(2)?;
        Ok(Item {
            id,
            kind: Kind::parse(&kind).ok_or_else(|| corrupt(id, "kind", &kind))?,
            urgency: Urgency::parse(&urgency).ok_or_else(|| corrupt(id, "urgency", &urgency))?,
            subject: row.get(3)?,
            label: row.get(4)?,
            url: row.get(5)?,
            context: id_column(id, "context", row.get(6)?, ContextId::parse)?,
            plan: id_column(id, "plan", row.get(7)?, PlanId::parse)?,
            job: id_column(id, "job", row.get(8)?, JobKey::parse)?,
            session: id_column(id, "session", row.get(9)?, SessionId::parse)?,
            record: id_column(id, "record", row.get(10)?, RecordId::parse)?,
            event: row.get(11)?,
            opened_at: row.get(12)?,
            closed_at: row.get(13)?,
            snoozed_until: row.get(14)?,
            dismissed_at: row.get(15)?,
            claimed_at: row.get(16)?,
        })
    }
}

fn items(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<Vec<Item>, CpdError> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params)?;
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        items.push(Item::from_row(row)?);
    }
    Ok(items)
}

/// One item, open or closed. A plain read: it takes no lock.
pub fn get(conn: &Connection, id: i64) -> Result<Option<Item>, CpdError> {
    Ok(items(conn, &format!("SELECT {COLUMNS} FROM inbox_items WHERE id = ?1"), [id])?.pop())
}

/// Which items [`list`] returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Open and not hidden at `now` (not dismissed, not claimed, not snoozed past `now`).
    Visible { now: String },
    /// Every open item, hidden ones too.
    Open,
    /// Closed items, newest `closed_at` first, at most `limit`.
    Closed { limit: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub context: Option<ContextId>,
    pub state: State,
}

/// The label of `engine-down` (§2.2), the one item that is never stored.
pub const ENGINE_DOWN_LABEL: &str = "engine is down";

/// Whether an engine holds `locks/watch.lock` (§1.4): a non-blocking shared
/// lock, which never fails because another reader holds one. A missing file
/// means no engine ever ran; this takes no lock and writes nothing.
pub fn engine_running(home: &CpdHome) -> Result<bool, CpdError> {
    let path = home.watch_lock();
    let failed = |what: &dyn std::fmt::Display| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot use the engine's lock {}: {what}.", path.display()))
            .with("lock", path.display().to_string())
    };
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(failed(&e)),
    };
    // The probe's shared lock is let go when `file` drops.
    let shared = lock::shared(&file).map_err(|e| failed(&format!("flock failed: {e}")))?;
    Ok(!shared)
}

/// The items of `filter`. Open ones come in §2.2's order: red, yellow,
/// magenta, blue, cyan, white, then the oldest `opened_at` first. A plain read.
pub fn list(conn: &Connection, filter: &Filter) -> Result<Vec<Item>, CpdError> {
    let context = filter.context.as_ref().map(ContextId::as_str);
    let select = format!("SELECT {COLUMNS} FROM inbox_items WHERE (?1 IS NULL OR context = ?1) AND");
    let rank = "CASE urgency WHEN 'red' THEN 0 WHEN 'yellow' THEN 1 WHEN 'magenta' THEN 2 WHEN 'blue' THEN 3 WHEN 'cyan' THEN 4 ELSE 5 END, opened_at, id";
    match &filter.state {
        State::Visible { now } => items(
            conn,
            &format!(
                "{select} closed_at IS NULL AND dismissed_at IS NULL AND claimed_at IS NULL AND (snoozed_until IS NULL OR snoozed_until <= ?2)
                 ORDER BY {rank}"
            ),
            params![context, now],
        ),
        State::Open => items(conn, &format!("{select} closed_at IS NULL ORDER BY {rank}"), params![context]),
        State::Closed { limit } => items(
            conn,
            &format!("{select} closed_at IS NOT NULL ORDER BY closed_at DESC, id DESC LIMIT ?2"),
            params![context, limit],
        ),
    }
}

/// Closes the open `pr-ready` item of the pull request `repo#number` (hs-n404), in the caller's transaction, at its time, as
/// [`reconcile`] does (`closed_at`, `inbox-closed`): every path that stores a pull request as merged calls it, so the item
/// never waits for E8's next pass. Nothing is written when no such item is open.
pub fn close_pr_ready(tx: &crate::store::WriteTxn<'_>, repo: &str, number: i64) -> Result<(), CpdError> {
    let subject = format!("pr:{repo}#{number}");
    for item in open_items(tx)?.into_iter().filter(|i| i.kind == Kind::PrReady && i.subject == subject) {
        tx.execute("UPDATE inbox_items SET closed_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1", params![item.id])?;
        let event = NewEvent::inbox_closed(format!("Inbox item {} closed: {}.", item.id, item.label), item.id, item.kind.as_str());
        events::append(tx, scope(event, &item.context, &item.plan, &item.job, &item.session))?;
    }
    Ok(())
}

/// What a [`reconcile`] changed: the ids of the items it opened and closed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub opened: Vec<i64>,
    pub closed: Vec<i64>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.opened.is_empty() && self.closed.is_empty()
    }
}

/// The open `curate-*` items whose condition has the same identity and another label: their count moved (hs-d549,
/// hs-d551), and the item, subject `curate-retire:<context>`, follows it without closing, so a dismissal stays.
fn relabelled<'a>(open: &'a [Item], conditions: &'a [Condition]) -> Vec<(&'a Item, &'a Condition)> {
    open.iter()
        .filter(|i| matches!(i.kind, Kind::CurateRetire | Kind::CurateDocs))
        .filter_map(|i| conditions.iter().find(|c| c.identity() == i.identity()).map(|c| (i, c)))
        .filter(|(i, c)| i.label != c.label)
        .collect()
}

/// The open items with no condition, and the conditions with no open item,
/// each in order. `conditions` is a set: of two with one identity, the first counts.
fn diff<'a>(open: &'a [Item], conditions: &'a [Condition]) -> (Vec<&'a Item>, Vec<&'a Condition>) {
    let wanted: BTreeSet<_> = conditions.iter().map(Condition::identity).collect();
    let held: BTreeSet<_> = open.iter().map(Item::identity).collect();
    let gone = open.iter().filter(|i| !wanted.contains(&i.identity())).collect();
    let mut seen = BTreeSet::new();
    let new = conditions.iter().filter(|c| !held.contains(&c.identity()) && seen.insert(c.identity())).collect();
    (gone, new)
}

pub fn open_items(conn: &Connection) -> Result<Vec<Item>, CpdError> {
    items(conn, &format!("SELECT {COLUMNS} FROM inbox_items WHERE closed_at IS NULL ORDER BY id"), [])
}

/// Engine step E8 (§3.7): in one write transaction, opens an item for each
/// condition with no open item of its identity (`kind`, `subject`,
/// `urgency`), closes each open item with no condition, and appends
/// `inbox-opened` and `inbox-closed` for them, closed ones first. Writes
/// nothing, not even a `txns` row, when nothing changed. `now` is §1.3's time.
pub fn reconcile(store: &mut Store, conditions: &[Condition], now: &str) -> Result<Changes, CpdError> {
    let open = store.read(open_items)?;
    let (gone, new) = diff(&open, conditions);
    if gone.is_empty() && new.is_empty() && relabelled(&open, conditions).is_empty() {
        return Ok(Changes::default());
    }
    store.write("inbox reconcile", |tx| {
        // Another writer may have changed the items since the read: decide again.
        let open = open_items(tx)?;
        let (gone, new) = diff(&open, conditions);
        let mut changes = Changes::default();
        for (item, c) in relabelled(&open, conditions) {
            tx.execute("UPDATE inbox_items SET label = ?2 WHERE id = ?1", params![item.id, c.label])?;
        }
        for item in gone {
            tx.execute("UPDATE inbox_items SET closed_at = ?2 WHERE id = ?1", params![item.id, now])?;
            let event = NewEvent::inbox_closed(format!("Inbox item {} closed: {}.", item.id, item.label), item.id, item.kind.as_str());
            events::append(tx, scope(event, &item.context, &item.plan, &item.job, &item.session))?;
            changes.closed.push(item.id);
        }
        for c in new {
            tx.execute(
                "INSERT INTO inbox_items (kind, urgency, subject, label, url, context, plan, job, session, record, event, opened_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    c.kind.as_str(),
                    c.urgency.as_str(),
                    c.subject,
                    c.label,
                    c.url,
                    c.context.as_ref().map(ContextId::as_str),
                    c.plan.as_ref().map(PlanId::as_str),
                    c.job.as_ref().map(JobKey::as_str),
                    c.session.as_ref().map(SessionId::as_str),
                    c.record.as_ref().map(RecordId::to_string),
                    c.event,
                    c.opened_at.as_deref().unwrap_or(now),
                ],
            )?;
            let id = tx.last_insert_rowid();
            let event = NewEvent::inbox_opened(
                format!("Inbox item {id} opened: {}.", c.label),
                id,
                c.kind.as_str(),
                c.urgency.as_str(),
                &c.label,
            );
            events::append(tx, scope(event, &c.context, &c.plan, &c.job, &c.session))?;
            changes.opened.push(id);
        }
        Ok(changes)
    })
}

/// `event` with the links an item has (§5.2: `context?, plan?, job?, session?`).
fn scope(
    mut event: NewEvent,
    context: &Option<ContextId>,
    plan: &Option<PlanId>,
    job: &Option<JobKey>,
    session: &Option<SessionId>,
) -> NewEvent {
    if let Some(context) = context {
        event = event.context(context.clone());
    }
    if let Some(plan) = plan {
        event = event.plan(plan.clone());
    }
    if let Some(job) = job {
        event = event.job(job.clone());
    }
    if let Some(session) = session {
        event = event.session(session.clone());
    }
    event
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::home::CpdHome;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        store
            .write("fixture", |tx| {
                tx.execute_batch(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                     INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
                       VALUES ('s1', 'lead', 'ctx', 'One', '/', 'O3', 'idle', 't', 't');
                     INSERT INTO events (txn, at, kind, context, session, text, data)
                       VALUES (cpd_txn(), 't', 'session-state', 'ctx', 's1', 'x', '{}');",
                )?;
                Ok(())
            })
            .unwrap();
        (dir, store)
    }

    fn condition(kind: Kind, urgency: Urgency, subject: &str) -> Condition {
        Condition {
            kind,
            urgency,
            subject: subject.to_owned(),
            label: format!("{subject} label"),
            url: None,
            context: None,
            plan: None,
            job: None,
            session: None,
            record: None,
            event: None,
            opened_at: None,
        }
    }

    fn dialog(subject: &str, urgency: Urgency) -> Condition {
        condition(Kind::Dialog, urgency, subject)
    }

    fn count(store: &mut Store, sql: &str) -> i64 {
        store.read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }

    fn all(store: &mut Store, state: State) -> Vec<Item> {
        store.read(|c| list(c, &Filter { context: None, state })).unwrap()
    }

    fn visible(store: &mut Store) -> Vec<Item> {
        all(store, State::Visible { now: "2026-01-01T00:00:00.000Z".into() })
    }

    #[test]
    fn urgency_orders_white_cyan_blue_magenta_yellow_red_and_parses_cyan() {
        assert!(Urgency::White < Urgency::Cyan && Urgency::Cyan < Urgency::Blue && Urgency::Blue < Urgency::Magenta && Urgency::Magenta < Urgency::Yellow && Urgency::Yellow < Urgency::Red);
        assert_eq!(Urgency::parse("cyan"), Some(Urgency::Cyan));
        assert_eq!(Urgency::Cyan.as_str(), "cyan");
    }

    #[test]
    fn kinds_and_urgencies_round_trip_their_names() {
        for name in [
            "dialog", "plan-review", "unread", "job-stuck", "engine-error", "engine-down", "question", "pr-failing",
            "pr-changes", "pr-ready", "review-requested",
        ] {
            assert_eq!(Kind::parse(name).unwrap().as_str(), name);
        }
        for name in ["white", "cyan", "blue", "magenta", "yellow", "red"] {
            assert_eq!(Urgency::parse(name).unwrap().as_str(), name);
        }
        assert_eq!((Kind::parse("nope"), Urgency::parse("nope")), (None, None));
    }

    #[test]
    fn a_condition_with_no_open_item_opens_one_and_appends_inbox_opened_with_its_links() {
        let (_dir, mut store) = store();
        let c = Condition {
            context: Some(ContextId::parse("ctx").unwrap()),
            session: Some(SessionId::parse("s1").unwrap()),
            event: Some(1),
            opened_at: None,
            url: Some("http://x".into()),
            label: "inbox asks you".into(),
            ..dialog("session:s1", Urgency::Magenta)
        };
        let changes = reconcile(&mut store, &[c], "2026-01-01T00:00:00.000Z").unwrap();
        assert_eq!((changes.opened.clone(), changes.closed), (vec![1], vec![]));

        let item = store.read(|c| get(c, 1)).unwrap().unwrap();
        assert_eq!((item.kind, item.urgency, item.subject.as_str()), (Kind::Dialog, Urgency::Magenta, "session:s1"));
        assert_eq!((item.label.as_str(), item.url.as_deref(), item.event), ("inbox asks you", Some("http://x"), Some(1)));
        assert_eq!(item.opened_at, "2026-01-01T00:00:00.000Z");
        assert_eq!((item.closed_at, item.snoozed_until, item.dismissed_at), (None, None, None));

        let event: (String, String, Option<String>, Option<String>, String) = store
            .read(|c| {
                Ok(c.query_row("SELECT kind, data, context, session, text FROM events WHERE kind = 'inbox-opened'", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?)
            })
            .unwrap();
        assert_eq!(event.0, "inbox-opened");
        let data: serde_json::Value = serde_json::from_str(&event.1).unwrap();
        assert_eq!(data, json!({"item": 1, "kind": "dialog", "urgency": "magenta", "label": "inbox asks you"}));
        assert_eq!((event.2.as_deref(), event.3.as_deref()), (Some("ctx"), Some("s1")));
    }

    #[test]
    fn unchanged_conditions_write_nothing() {
        let (_dir, mut store) = store();
        let conditions = [dialog("session:a", Urgency::Red), dialog("session:b", Urgency::White)];
        reconcile(&mut store, &conditions, "2026-01-01T00:00:00.000Z").unwrap();
        let (txns, events, history) = (
            count(&mut store, "SELECT count(*) FROM txns"),
            count(&mut store, "SELECT count(*) FROM events"),
            count(&mut store, "SELECT count(*) FROM history"),
        );
        // The same conditions, in another order and with another label, are the same conditions.
        let again = [
            Condition { label: "renamed".into(), ..dialog("session:b", Urgency::White) },
            dialog("session:a", Urgency::Red),
        ];
        let changes = reconcile(&mut store, &again, "2026-01-01T00:05:00.000Z").unwrap();
        assert!(changes.is_empty());
        assert_eq!(count(&mut store, "SELECT count(*) FROM txns"), txns);
        assert_eq!(count(&mut store, "SELECT count(*) FROM events"), events);
        assert_eq!(count(&mut store, "SELECT count(*) FROM history"), history);
    }

    #[test]
    fn a_gone_condition_closes_its_item_and_appends_inbox_closed() {
        let (_dir, mut store) = store();
        reconcile(&mut store, &[dialog("session:a", Urgency::Red), dialog("session:b", Urgency::Red)], "t1").unwrap();
        let changes = reconcile(&mut store, &[dialog("session:b", Urgency::Red)], "t2").unwrap();
        assert_eq!((changes.opened, changes.closed), (vec![], vec![1]));
        let closed = store.read(|c| get(c, 1)).unwrap().unwrap();
        assert_eq!((closed.opened_at.as_str(), closed.closed_at.as_deref()), ("t1", Some("t2")));
        assert_eq!(store.read(|c| get(c, 2)).unwrap().unwrap().closed_at, None);
        let last: (String, String) = store
            .read(|c| Ok(c.query_row("SELECT kind, data FROM events ORDER BY id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?))
            .unwrap();
        assert_eq!(last.0, "inbox-closed");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&last.1).unwrap(), json!({"item": 1, "kind": "dialog"}));
    }

    #[test]
    fn an_urgency_change_closes_the_item_and_opens_a_new_one() {
        let (_dir, mut store) = store();
        reconcile(&mut store, &[dialog("session:a", Urgency::Magenta)], "t1").unwrap();
        let changes = reconcile(&mut store, &[dialog("session:a", Urgency::Red)], "t2").unwrap();
        assert_eq!((changes.opened, changes.closed), (vec![2], vec![1]));
        let open = all(&mut store, State::Open);
        assert_eq!(open.iter().map(|i| (i.id, i.urgency, i.opened_at.as_str())).collect::<Vec<_>>(), vec![(2, Urgency::Red, "t2")]);
    }

    #[test]
    fn closed_items_stay_and_a_returning_condition_is_a_new_item() {
        let (_dir, mut store) = store();
        reconcile(&mut store, &[dialog("session:a", Urgency::Red)], "t1").unwrap();
        reconcile(&mut store, &[], "t2").unwrap();
        assert_eq!(count(&mut store, "SELECT count(*) FROM inbox_items"), 1);
        assert!(all(&mut store, State::Open).is_empty());
        reconcile(&mut store, &[dialog("session:a", Urgency::Red)], "t3").unwrap();
        assert_eq!(count(&mut store, "SELECT count(*) FROM inbox_items"), 2);
        let closed = all(&mut store, State::Closed { limit: 50 });
        assert_eq!(closed.iter().map(|i| i.id).collect::<Vec<_>>(), vec![1]);
        assert_eq!(all(&mut store, State::Open).iter().map(|i| i.id).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn the_list_is_red_yellow_magenta_white_then_the_oldest_first() {
        let (_dir, mut store) = store();
        for (subject, urgency, at) in [
            ("w1", Urgency::White, "t1"),
            ("r2", Urgency::Red, "t5"),
            ("m1", Urgency::Magenta, "t2"),
            ("y1", Urgency::Yellow, "t3"),
            ("r1", Urgency::Red, "t4"),
            ("m2", Urgency::Magenta, "t6"),
        ] {
            let mut open: Vec<Condition> =
                all(&mut store, State::Open).iter().map(|i| dialog(&i.subject, i.urgency)).collect();
            open.push(dialog(subject, urgency));
            reconcile(&mut store, &open, at).unwrap();
        }
        let order: Vec<String> = visible(&mut store).into_iter().map(|i| i.subject).collect();
        assert_eq!(order, ["r1", "r2", "y1", "m1", "m2", "w1"]);
    }

    #[test]
    fn hidden_items_are_not_visible_and_a_passed_snooze_returns_at_its_rank() {
        let (_dir, mut store) = store();
        reconcile(&mut store, &[dialog("a", Urgency::Red), dialog("b", Urgency::Red), dialog("c", Urgency::Red)], "t1").unwrap();
        store
            .write("test", |tx| {
                Ok(tx.execute_batch(
                    "UPDATE inbox_items SET snoozed_until = '2026-01-01T01:00:00.000Z' WHERE subject = 'a';
                     UPDATE inbox_items SET dismissed_at = 't' WHERE subject = 'b'",
                )?)
            })
            .unwrap();
        let subjects = |items: Vec<Item>| items.into_iter().map(|i| i.subject).collect::<Vec<_>>();
        assert_eq!(subjects(visible(&mut store)), ["c"]);
        assert_eq!(subjects(all(&mut store, State::Open)), ["a", "b", "c"]);
        let later = State::Visible { now: "2026-01-01T01:00:00.000Z".into() };
        assert_eq!(subjects(all(&mut store, later)), ["a", "c"]);
    }

    #[test]
    fn the_list_keeps_one_context_and_closed_come_newest_first_up_to_the_limit() {
        let (_dir, mut store) = store();
        let ctx = Some(ContextId::parse("ctx").unwrap());
        let in_ctx = Condition { context: ctx.clone(), ..dialog("in", Urgency::Red) };
        reconcile(&mut store, &[in_ctx, dialog("out", Urgency::Red)], "t1").unwrap();
        let filter = Filter { context: ctx, state: State::Open };
        let found = store.read(|c| list(c, &filter)).unwrap();
        assert_eq!(found.iter().map(|i| i.subject.as_str()).collect::<Vec<_>>(), ["in"]);

        reconcile(&mut store, &[], "t2").unwrap();
        reconcile(&mut store, &[dialog("x", Urgency::Red)], "t3").unwrap();
        reconcile(&mut store, &[], "t4").unwrap();
        let closed = all(&mut store, State::Closed { limit: 2 });
        assert_eq!(closed.iter().map(|i| i.subject.as_str()).collect::<Vec<_>>(), ["x", "out"]);
    }

    #[test]
    fn a_kind_that_is_not_stored_is_refused_and_nothing_is_written() {
        let (_dir, mut store) = store();
        let err = reconcile(&mut store, &[condition(Kind::EngineDown, Urgency::Red, "engine")], "t1").unwrap_err();
        assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
        assert_eq!(count(&mut store, "SELECT count(*) FROM inbox_items"), 0);
    }
}
