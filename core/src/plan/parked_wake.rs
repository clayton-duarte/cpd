//! The wake state of a Plan's parked drafts (contract §3.7 E20, hs-d731, hs-d733): from the store only, one definition for the
//! engine step that wakes the Lead and the Inbox kind `drafts-parked`.

use rusqlite::{Connection, params};

use super::parked::parked_drafts;
use crate::error::CpdError;
use crate::ids::PlanId;

/// The Event kinds that change whether a draft is parked (T is the highest id of them).
const CHANGING: &str = "'job-added', 'job-edited', 'job-merged', 'job-failed', 'job-retried', 'job-cancelled', 'plan-review', 'pr-added', \
                        'pr-edited', 'pr-started', 'pr-cancelled', 'pr-merged', 'plan-phase'";

/// What the Lead has been told of a Plan's parked drafts.
#[derive(Debug, PartialEq, Eq)]
pub struct Wake {
    /// S: the keys of the parked drafts, in Job order.
    pub parked: Vec<String>,
    /// T: the highest id of an Event of the Plan that changes a draft's legality (0 when none).
    pub t: i64,
    /// W: the id of the newest message from the User's side to the Lead that begins [`prefix`].
    pub wake: Option<i64>,
    /// The Lead finished a turn after W: a `session-state` Event to `idle` from `working`, `waiting` or `blocked`.
    pub finished_turn_after_wake: bool,
}

/// What every wake message of `plan` begins with: the record that it was sent.
pub fn prefix(plan: &PlanId) -> String {
    format!("Drafts of Plan {plan} are parked: ")
}

/// The wake state of `plan` and its Lead Session `lead`.
pub fn wake_state(conn: &Connection, plan: &PlanId, lead: &str) -> Result<Wake, CpdError> {
    let parked = parked_drafts(conn, plan)?;
    let t: i64 = conn.query_row(
        &format!(
            "SELECT coalesce(max(id), 0) FROM events WHERE plan = ?1 AND kind IN ({CHANGING})"
        ),
        [plan.as_str()],
        |r| r.get(0),
    )?;
    let wake: Option<i64> = conn.query_row(
        "SELECT max(id) FROM events WHERE session = ?1 AND kind = 'session-message'
            AND json_extract(data, '$.from') IS NULL AND instr(json_extract(data, '$.text'), ?2) = 1",
        params![lead, prefix(plan)],
        |r| r.get(0),
    )?;
    let finished_turn_after_wake = match wake {
        None => false,
        Some(w) => conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM events WHERE session = ?1 AND id > ?2 AND kind = 'session-state'
                             AND json_extract(data, '$.from') IN ('working', 'waiting', 'blocked') AND json_extract(data, '$.to') = 'idle')",
            params![lead, w],
            |r| r.get(0),
        )?,
    };
    Ok(Wake {
        parked,
        t,
        wake,
        finished_turn_after_wake,
    })
}
