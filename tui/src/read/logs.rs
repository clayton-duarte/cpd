//! The `logs` view's snapshot (commands.md §17): the latest Events of the machine, oldest first, and the Events
//! after a cursor, which the view appends while it follows. The rows are the `events list` selection
//! ([`Selection::fetch`]), so there is one query for Events.

use cpd_core::error::CpdError;
use cpd_core::events::{Event, Selection, highest_id};
use cpd_core::store::Store;

/// What the Event log shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Logs {
    /// Oldest first.
    pub events: Vec<Event>,
    /// The highest Event id in this read transaction (§17): the next [`logs_after`] starts there.
    pub events_cursor: i64,
}

/// The latest `recent` Events, oldest first.
pub fn logs(store: &mut Store, recent: u32) -> Result<Logs, CpdError> {
    store.read(|conn| {
        let events_cursor = highest_id(conn)?;
        // The id before the oldest of the latest `recent`: Event ids are not assumed to be contiguous.
        let before: i64 = conn.query_row(
            "SELECT coalesce(min(id), 1) - 1 FROM (SELECT id FROM events ORDER BY id DESC LIMIT ?1)",
            [recent],
            |r| r.get(0),
        )?;
        let events = Selection::resolve(conn, None, None, None, Vec::new())?.fetch(conn, before, Some(events_cursor), i64::MAX)?;
        Ok(Logs { events, events_cursor })
    })
}

/// Every Event after `cursor`, oldest first.
pub fn logs_after(store: &mut Store, cursor: i64) -> Result<Logs, CpdError> {
    store.read(|conn| {
        let events_cursor = highest_id(conn)?;
        let events = Selection::resolve(conn, None, None, None, Vec::new())?.fetch(
            conn,
            cursor,
            Some(events_cursor),
            i64::MAX,
        )?;
        Ok(Logs {
            events,
            events_cursor,
        })
    })
}
