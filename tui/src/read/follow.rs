//! [`Follow`]: how every view learns that Events came (commands.md §17), in its own process: one `events` query per
//! poll, from a cursor the follow keeps, with no subprocess and no thread. The views decide when to poll (the plan
//! view and the log every `events.poll_ms`, the others with their refresh) and what an Event changes.

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{Event, Kind, Selection, highest_id};
use cpd_core::store::Store;
use rusqlite::Connection;

/// The Events after a cursor, of the kinds asked for.
pub struct Follow {
    selection: Selection,
    cursor: i64,
    /// What [`pulls_mark`] said at the last [`Follow::poll_changes`]; none before the first.
    pulls: Option<(i64, String)>,
}

/// What [`Follow::poll_changes`] found.
pub struct Changes {
    /// The Events committed since the last poll, oldest first.
    pub events: Vec<Event>,
    /// A `pull_requests` row was added or changed since the last poll (or this is the first poll): `pr poll` writes
    /// the checks, the review and the `mergeable` of a pull request with no Event (contract §7.6, hs-d96).
    pub pulls: bool,
}

impl Changes {
    /// Something a view of the pull requests or of what the Events changed must read again.
    pub fn any(&self) -> bool {
        self.pulls || !self.events.is_empty()
    }
}

/// How many pull request rows there are and the latest `changed_at` among them (contract §4.4: the time of the last
/// change of any other field): one cheap read of a small table, in place of a snapshot of the whole model.
fn pulls_mark(conn: &Connection) -> Result<(i64, String), CpdError> {
    Ok(conn.query_row(
        "SELECT count(*), coalesce(max(changed_at), '') FROM pull_requests",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

impl Follow {
    /// Follows the Events of `kinds` (every kind when empty) after `cursor`, which a snapshot's `events_cursor` gives.
    pub fn new(store: &mut Store, kinds: &[Kind], cursor: i64) -> Result<Follow, CpdError> {
        let selection =
            store.read(|conn| Selection::resolve(conn, None, None, None, kinds.to_vec()))?;
        Ok(Follow {
            selection,
            cursor,
            pulls: None,
        })
    }

    /// The Events committed since the last successful poll, oldest first. The cursor moves only when this returns
    /// `Ok`, so a failed poll is made again and loses nothing. `cursor_ahead` when the highest Event id fell below the
    /// cursor: the database is not the one the view started from, and no Event can be told from it.
    pub fn poll(&mut self, store: &mut Store) -> Result<Vec<Event>, CpdError> {
        Ok(self.advance(store, false)?.events)
    }

    /// [`Follow::poll`], and whether the pull request rows changed since the last call of this one, for the views that
    /// show them: a change of those has no Event. The first call says they did, so that none is missed between the
    /// view's snapshot and its first poll.
    pub fn poll_changes(&mut self, store: &mut Store) -> Result<Changes, CpdError> {
        self.advance(store, true)
    }

    fn advance(&mut self, store: &mut Store, pulls: bool) -> Result<Changes, CpdError> {
        let from = self.cursor;
        let (highest, events, mark) = store.read(|conn| {
            let highest = highest_id(conn)?;
            if highest < from {
                return Err(CpdError::new(
                    ErrorCode::CursorAhead,
                    format!("Event cursor {from} is above the highest Event id, {highest}: the database changed."),
                )
                .with("cursor", from)
                .with("highest", highest));
            }
            let mark = if pulls { Some(pulls_mark(conn)?) } else { None };
            Ok((highest, self.selection.fetch(conn, from, Some(highest), i64::MAX)?, mark))
        })?;
        self.cursor = highest;
        let pulls =
            mark.is_some_and(|mark| self.pulls.replace(mark.clone()).as_ref() != Some(&mark));
        Ok(Changes { events, pulls })
    }
}
