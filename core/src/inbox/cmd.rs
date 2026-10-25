//! `cpd inbox list | show | snooze | dismiss` (commands.md §21). The commands
//! read the items and hide them; only the engine's E8 opens and closes them.

use std::process::{Command as Process, Stdio};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime, Time, UtcOffset};

use super::model::{self, Filter, Item, Kind, State, Urgency};
use crate::clock::timestamp;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, NewEvent};
use crate::host::deadline::Failed;
use crate::host::runner::{NO_DEADLINE, Runner, SystemRunner};
use crate::ids::ContextId;
use crate::output::Output;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// List what needs you (§21.1)
    List(ListArgs),
    /// Show one item (§21.2)
    Show(ShowArgs),
    /// Hide an item for a while (§21.3)
    Snooze(SnoozeArgs),
    /// Hide an item for the rest of its life (§21.4)
    Dismiss(DismissArgs),
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Only the items of this Context
    #[arg(long, value_name = "c")]
    context: Option<ContextId>,
    /// Also list the snoozed and dismissed items
    #[arg(long, conflicts_with = "closed")]
    all: bool,
    /// List closed items instead of open ones, newest first
    #[arg(long)]
    closed: bool,
    /// With --closed: at most this many items [default: 50]
    #[arg(long, value_name = "n", requires = "closed", value_parser = clap::value_parser!(u32).range(1..=5000))]
    limit: Option<u32>,
}

#[derive(clap::Args, Debug)]
pub struct ShowArgs {
    /// The item's id
    #[arg(value_name = "id")]
    id: i64,
}

/// How long `snooze` hides an item.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum Until {
    #[value(name = "15m")]
    Minutes15,
    #[value(name = "1h")]
    Hour1,
    #[value(name = "tomorrow")]
    Tomorrow,
}

#[derive(clap::Args, Debug)]
pub struct SnoozeArgs {
    /// The item's id
    #[arg(value_name = "id")]
    id: i64,
    /// 15m, 1h, or tomorrow (09:00 local time)
    #[arg(value_name = "15m|1h|tomorrow")]
    until: Until,
}

#[derive(clap::Args, Debug)]
pub struct DismissArgs {
    /// The item's id
    #[arg(value_name = "id")]
    id: i64,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::List(args) => list(args),
        Command::Show(args) => show(args),
        Command::Snooze(args) => snooze(args),
        Command::Dismiss(args) => dismiss(args),
    }
}

/// `--limit`'s default with `--closed` (§21.1).
const DEFAULT_LIMIT: u32 = 50;


fn parse_time(item: &Item, column: &str, value: &str) -> Result<OffsetDateTime, CpdError> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| {
        CpdError::new(ErrorCode::StoreFailed, format!("Inbox item {} has an invalid {column}: {value:?}.", item.id))
            .with("item", item.id)
            .with("column", column)
    })
}

/// The item as printed (§21): its row, and `age_seconds`.
fn item_json(item: &Item, now: OffsetDateTime) -> Result<Value, CpdError> {
    let opened = parse_time(item, "opened_at", &item.opened_at)?;
    let until = match &item.closed_at {
        Some(closed) => parse_time(item, "closed_at", closed)?,
        None => now,
    };
    Ok(json!({
        "id": item.id,
        "kind": item.kind.as_str(),
        "urgency": item.urgency.as_str(),
        "label": item.label,
        "subject": item.subject,
        "context": item.context.as_ref().map(ContextId::as_str),
        "plan": item.plan.as_ref().map(|p| p.as_str()),
        "job": item.job.as_ref().map(|j| j.as_str()),
        "session": item.session.as_ref().map(|s| s.as_str()),
        "record": item.record.as_ref().map(ToString::to_string),
        "event": item.event,
        "url": item.url,
        "opened_at": item.opened_at,
        "closed_at": item.closed_at,
        "snoozed_until": item.snoozed_until,
        "dismissed_at": item.dismissed_at,
        "age_seconds": (until - opened).whole_seconds(),
    }))
}

/// The item `engine-down` (§2.2): never stored, so it has no id and no links.
fn engine_down(now: OffsetDateTime) -> Value {
    json!({
        "id": null,
        "kind": Kind::EngineDown.as_str(),
        "urgency": Urgency::Red.as_str(),
        "label": model::ENGINE_DOWN_LABEL,
        "subject": "engine",
        "context": null,
        "plan": null,
        "job": null,
        "session": null,
        "record": null,
        "event": null,
        "url": null,
        "opened_at": timestamp(now),
        "closed_at": null,
        "snoozed_until": null,
        "dismissed_at": null,
        "age_seconds": 0,
    })
}

fn context_exists(conn: &Connection, context: &ContextId) -> Result<(), CpdError> {
    let found: Option<i64> =
        conn.query_row("SELECT 1 FROM contexts WHERE id = ?1", [context.as_str()], |r| r.get(0)).optional()?;
    found.map(|_| ()).ok_or_else(|| {
        CpdError::new(ErrorCode::ContextNotFound, format!("No Context {context}.")).with("context", context.as_str())
    })
}

/// `inbox list` (§21.1).
fn list(args: ListArgs) -> Result<Output, CpdError> {
    let Env { home, mut store, .. } = Env::open()?;
    let now = OffsetDateTime::now_utc();
    let filter = |state| Filter { context: args.context.clone(), state };
    let (open, visible, closed, cursor, manager) = store.read(|conn| {
        if let Some(context) = &args.context {
            context_exists(conn, context)?;
        }
        let open = model::list(conn, &filter(State::Open))?;
        let visible = model::list(conn, &filter(State::Visible { now: timestamp(now) }))?;
        let closed = match args.closed {
            true => model::list(conn, &filter(State::Closed { limit: args.limit.unwrap_or(DEFAULT_LIMIT) }))?,
            false => Vec::new(),
        };
        // A live Manager (role `manager`, not archived) handles a stopped engine: the tree restarts it and a failure goes to
        // the Manager (hs-d318), so `engine-down` is the User's only when there is none.
        let manager: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sessions WHERE role = 'manager' AND archived_at IS NULL)",
            [],
            |r| r.get(0),
        )?;
        Ok((open, visible, closed, events::highest_id(conn)?, manager))
    })?;
    let is_visible = |item: &Item| visible.iter().any(|v| v.id == item.id);
    let hidden = |item: &Item| if item.dismissed_at.is_some() { "dismissed" } else { "snoozed" };

    let mut items = Vec::new();
    // An absent engine with no live Manager is an open item with no Context: neither `--closed` nor `--context` lists it.
    let running = model::engine_running(&home)?;
    if !running && !manager && !args.closed && args.context.is_none() {
        items.push(engine_down(now));
    }
    let listed: &[Item] = if args.closed {
        &closed
    } else if args.all {
        &open
    } else {
        &visible
    };
    for item in listed {
        let mut value = item_json(item, now)?;
        if !is_visible(item) && !args.closed {
            value["hidden"] = json!(hidden(item));
        }
        items.push(value);
    }
    let count = |what: &str| open.iter().filter(|i| !is_visible(i) && hidden(i) == what).count();
    Ok(Output::success(json!({
        "engine": {"running": running},
        "items": items,
        "hidden": {"snoozed": count("snoozed"), "dismissed": count("dismissed")},
        "events_cursor": cursor,
    })))
}

fn not_found(id: i64) -> CpdError {
    CpdError::new(ErrorCode::ItemNotFound, format!("No Inbox item {id}.")).with("item", id)
}

fn closed(item: &Item) -> CpdError {
    CpdError::new(ErrorCode::ItemClosed, format!("Inbox item {} is closed: it cannot be hidden.", item.id))
        .with("item", item.id)
        .with("closed_at", item.closed_at.clone())
}

/// The item `id`, which must be open.
fn open_item(conn: &Connection, id: i64) -> Result<Item, CpdError> {
    let item = model::get(conn, id)?.ok_or_else(|| not_found(id))?;
    match item.closed_at {
        Some(_) => Err(closed(&item)),
        None => Ok(item),
    }
}

/// `inbox show` (§21.2).
fn show(args: ShowArgs) -> Result<Output, CpdError> {
    let Env { mut store, .. } = Env::open()?;
    let now = OffsetDateTime::now_utc();
    let item = store.read(|conn| model::get(conn, args.id)?.ok_or_else(|| not_found(args.id)))?;
    Ok(Output::success(json!({"item": item_json(&item, now)?})))
}

/// The offset of local time, as `date +%z` prints it (`+0200`, `-0530`).
fn local_offset() -> Result<UtcOffset, CpdError> {
    local_offset_with(&SystemRunner)
}

/// [`local_offset`] on `runner`: one call of `date +%z`.
pub fn local_offset_with(runner: &dyn Runner) -> Result<UtcOffset, CpdError> {
    let fail = |message: String| CpdError::new(ErrorCode::SpawnFailed, message).with("argv", json!(["date", "+%z"]));
    let out = runner
        .output(Process::new("date").arg("+%z").stdin(Stdio::null()), "date", NO_DEADLINE)
        .map_err(|e| match e {
            Failed::Io(e) => fail(format!("Cannot run `date +%z`: {e}.")),
            Failed::Timeout(e) => e,
        })?;
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    if !out.status.success() {
        return Err(fail(format!("`date +%z` failed: {stderr}")).with("stderr", stderr));
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    parse_offset(&text)
        .ok_or_else(|| fail(format!("`date +%z` printed {text:?}, not an offset like +0200.")).with("stdout", text))
}

fn parse_offset(text: &str) -> Option<UtcOffset> {
    let (sign, digits) = match text.split_at_checked(1)? {
        ("+", digits) => (1, digits),
        ("-", digits) => (-1, digits),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (hours, minutes): (i8, i8) = (digits[..2].parse().ok()?, digits[2..].parse().ok()?);
    UtcOffset::from_hms(sign * hours, sign * minutes, 0).ok()
}

/// When `until` is, counting from `now` (§21.3).
fn snooze_until(until: Until, now: OffsetDateTime) -> Result<OffsetDateTime, CpdError> {
    match until {
        Until::Minutes15 => Ok(now + Duration::minutes(15)),
        Until::Hour1 => Ok(now + Duration::hours(1)),
        Until::Tomorrow => {
            let local = now.to_offset(local_offset()?);
            let today = local.replace_time(Time::from_hms(9, 0, 0).expect("09:00 is a time"));
            Ok(if today > local { today } else { today + Duration::days(1) })
        }
    }
}

/// `inbox snooze` (§21.3).
fn snooze(args: SnoozeArgs) -> Result<Output, CpdError> {
    let Env { mut store, .. } = Env::open()?;
    let now = OffsetDateTime::now_utc();
    let until = timestamp(snooze_until(args.until, now)?);
    let item = store.write("inbox snooze", |tx| {
        let item = open_item(tx, args.id)?;
        tx.execute("UPDATE inbox_items SET snoozed_until = ?2 WHERE id = ?1", params![item.id, until])?;
        let event = NewEvent::inbox_snoozed(format!("Inbox item {} snoozed until {until}: {}.", item.id, item.label), item.id, &until);
        events::append(tx, scoped(event, &item))?;
        model::get(tx, item.id)?.ok_or_else(|| not_found(item.id))
    })?;
    Ok(Output::success(json!({"item": item_json(&item, now)?})))
}

/// `inbox dismiss` (§21.4).
fn dismiss(args: DismissArgs) -> Result<Output, CpdError> {
    let Env { mut store, .. } = Env::open()?;
    let now = OffsetDateTime::now_utc();
    // Already dismissed writes nothing, not even a `txns` row.
    let before = store.read(|conn| open_item(conn, args.id))?;
    if before.dismissed_at.is_some() {
        return Ok(Output::success(json!({"already": true, "item": item_json(&before, now)?})));
    }
    let (item, already) = store.write("inbox dismiss", |tx| {
        let item = open_item(tx, args.id)?;
        if item.dismissed_at.is_some() {
            return Ok((item, true));
        }
        tx.execute("UPDATE inbox_items SET dismissed_at = ?2 WHERE id = ?1", params![item.id, timestamp(now)])?;
        let event = NewEvent::inbox_dismissed(format!("Inbox item {} dismissed: {}.", item.id, item.label), item.id);
        events::append(tx, scoped(event, &item))?;
        Ok((model::get(tx, item.id)?.ok_or_else(|| not_found(item.id))?, false))
    })?;
    let mut fields = json!({"item": item_json(&item, now)?});
    if already {
        fields["already"] = json!(true);
    }
    Ok(Output::success(fields))
}

/// `event` with the links of `item` (§5.2: `context?, plan?, job?, session?`).
fn scoped(mut event: NewEvent, item: &Item) -> NewEvent {
    if let Some(context) = &item.context {
        event = event.context(context.clone());
    }
    if let Some(plan) = &item.plan {
        event = event.plan(plan.clone());
    }
    if let Some(job) = &item.job {
        event = event.job(job.clone());
    }
    if let Some(session) = &item.session {
        event = event.session(session.clone());
    }
    event
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::CpdHome;
    use crate::inbox::model::{Condition, reconcile};
    use crate::store::Store;

    fn condition(urgency: Urgency, subject: &str) -> Condition {
        Condition {
            kind: Kind::Dialog,
            urgency,
            subject: subject.to_owned(),
            label: subject.to_owned(),
            url: None,
            context: None,
            plan: None,
            job: None,
            session: None,
            record: None,
            event: None,
        }
    }

    #[test]
    fn inbox_list_orders_red_yellow_magenta_blue_white_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        // opened newest tier-first, so only the tier rank (and then age) can produce the expected order
        for (at, rows) in [
            ("t1", vec![(Urgency::White, "w-old")]),
            ("t2", vec![(Urgency::White, "w-old"), (Urgency::Blue, "b-old")]),
            ("t3", vec![(Urgency::White, "w-old"), (Urgency::Blue, "b-old"), (Urgency::Magenta, "m-old")]),
            ("t4", vec![(Urgency::White, "w-old"), (Urgency::Blue, "b-old"), (Urgency::Magenta, "m-old"), (Urgency::Yellow, "y-old")]),
            (
                "t5",
                vec![
                    (Urgency::White, "w-old"),
                    (Urgency::Blue, "b-old"),
                    (Urgency::Magenta, "m-old"),
                    (Urgency::Yellow, "y-old"),
                    (Urgency::Red, "r-old"),
                    (Urgency::Blue, "b-new"),
                ],
            ),
        ] {
            let conditions: Vec<Condition> = rows.into_iter().map(|(u, s)| condition(u, s)).collect();
            reconcile(&mut store, &conditions, at).unwrap();
        }
        let items = store
            .read(|c| model::list(c, &Filter { context: None, state: State::Open }))
            .unwrap();
        let order: Vec<&str> = items.iter().map(|i| i.subject.as_str()).collect();
        assert_eq!(order, ["r-old", "y-old", "m-old", "b-old", "b-new", "w-old"]);
    }
}
