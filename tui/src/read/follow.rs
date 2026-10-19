//! [`Follow`]: how every view learns that Events came (commands.md §17), in its own process: one `events` query per
//! poll, from a cursor the follow keeps, with no subprocess and no thread. The views decide when to poll (the plan
//! view and the log every `events.poll_ms`, the others with their refresh) and what an Event changes.

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{Event, Kind, Selection, highest_id};
use cpd_core::store::Store;

/// The Events after a cursor, of the kinds asked for.
pub struct Follow {
    selection: Selection,
    cursor: i64,
}

impl Follow {
    /// Follows the Events of `kinds` (every kind when empty) after `cursor`, which a snapshot's `events_cursor` gives.
    pub fn new(store: &mut Store, kinds: &[Kind], cursor: i64) -> Result<Follow, CpdError> {
        let selection = store.read(|conn| Selection::resolve(conn, None, None, None, kinds.to_vec()))?;
        Ok(Follow { selection, cursor })
    }

    /// The Events committed since the last successful poll, oldest first. The cursor moves only when this returns
    /// `Ok`, so a failed poll is made again and loses nothing. `cursor_ahead` when the highest Event id fell below the
    /// cursor: the database is not the one the view started from, and no Event can be told from it.
    pub fn poll(&mut self, store: &mut Store) -> Result<Vec<Event>, CpdError> {
        let from = self.cursor;
        let (highest, events) = store.read(|conn| {
            let highest = highest_id(conn)?;
            if highest < from {
                return Err(CpdError::new(
                    ErrorCode::CursorAhead,
                    format!("The Event cursor {from} is above the highest Event id, {highest}: the database changed."),
                )
                .with("cursor", from)
                .with("highest", highest));
            }
            Ok((highest, self.selection.fetch(conn, from, Some(highest), i64::MAX)?))
        })?;
        self.cursor = highest;
        Ok(events)
    }
}
