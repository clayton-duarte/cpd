//! E25, the bug protocol (contract §3.7 E25, §2.2 "Bug record", "The wake and the breach check";
//! air-n120, air-d105, air-d106): every tick, right after E21 and before E8, it reads the open
//! `bug_records` rows and
//!   - wakes each bug's owner once, ever: a waking `session-message` (`from` null, `wake` true) whose
//!     text begins `Bug <id> is open`, so a sleeping or idle owner is resumed;
//!   - appends one `bug-breach` Event per bug and reason when the bug is past the 15-minute bound with
//!     no mitigation (`unmitigated_past_bound`) or repeats an open bug's normalized symptom
//!     (`recurring_symptom`), and, in the same transaction, one waking `session-message` whose text
//!     begins `Bug <id> breached`;
//!   - leaves the User's alert to the bug's `bug` Inbox item, which E8's rule keeps open while the bug
//!     is unmitigated and closes when it is mitigated or closed (core/src/inbox/model.rs).
//!
//! The owner is the Plan's Lead when the record names a Plan, else the live Manager of Context `cpd`
//! (air-d104, pro-d204). The writes are their own records, as E21's are, so a quiet tick writes
//! nothing. Not held by incident mode: a breach is what an incident must not silence.

use rusqlite::{Connection, OptionalExtension};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::SOURCE;
use super::engine::Engine;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{self, NewEvent};
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::session::tell;
use cpd_core::store::{Store, WriteTxn};

/// The mitigation bound of contract §2.2: 900 s (15 minutes), a code constant.
const BOUND: i64 = 900;

/// One open, unmitigated `bug_records` row, as E25 reads it.
struct Bug {
    id: String,
    plan: Option<String>,
    title: String,
    symptom: String,
    reported_at: String,
}

/// What one tick is to write.
enum Action {
    /// The once-per-bug owner wake (`Bug <id> is open`).
    Wake {
        id: String,
        owner: SessionId,
        title: String,
    },
    /// One `bug-breach` Event and, for a live owner, its waking message (`Bug <id> breached`).
    Breach {
        id: String,
        owner: Option<SessionId>,
        plan: Option<PlanId>,
        title: String,
        reason: &'static str,
        symptom: String,
    },
}

/// The step: its failure only logged (the step's `error` Event is out of this Job's file scope).
pub(super) fn run(engine: &mut Engine) {
    match tick(engine) {
        Ok(0) => {}
        Ok(n) => engine.log.line(&format!("E25: {n} bug protocol write(s)")),
        Err(e) => engine.log.line(&format!(
            "E25 failed: {} ({})",
            e.message(),
            e.code().as_str()
        )),
    }
}

/// One tick: nothing when there is nothing to write, else the actions in one transaction.
fn tick(engine: &mut Engine) -> Result<usize, CpdError> {
    let now = engine.ports.clock.utc();
    let bugs = engine.store.read(open_bugs)?;
    if bugs.is_empty() {
        return Ok(0);
    }
    let actions = actions(&mut engine.store, now, &bugs)?;
    if actions.is_empty() {
        return Ok(0);
    }
    engine.store.write(SOURCE, |tx| apply(tx, &actions))
}

/// The open `bug_records` rows with no mitigation, `id` ascending.
fn open_bugs(conn: &Connection) -> Result<Vec<Bug>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, plan, title, symptom, reported_at FROM bug_records
          WHERE status = 'open' AND mitigation IS NULL ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Bug {
                id: r.get(0)?,
                plan: r.get(1)?,
                title: r.get(2)?,
                symptom: r.get(3)?,
                reported_at: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The owner of `bug` when it has a live Session: the Plan's Lead, else the live Manager of Context `cpd`.
fn owner_of(conn: &Connection, bug: &Bug) -> Result<Option<SessionId>, CpdError> {
    let id: Option<String> = match &bug.plan {
        Some(plan) => conn
            .query_row(
                "SELECT s.id FROM plans p JOIN sessions s ON s.id = p.lead_session
                  WHERE p.id = ?1 AND s.role = 'lead' AND s.archived_at IS NULL",
                [plan],
                |r| r.get(0),
            )
            .optional()?,
        None => conn
            .query_row(
                "SELECT id FROM sessions WHERE role = 'manager' AND context = 'cpd' AND archived_at IS NULL
                  ORDER BY created_at, id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?,
    };
    id.map(|s| SessionId::parse(&s)).transpose()
}

/// Whether the owner's open wake of bug `id` is already recorded (the message is the record).
fn woken(conn: &Connection, id: &str) -> Result<bool, CpdError> {
    let prefix = format!("Bug {id} is open");
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM events WHERE kind = 'session-message'
                          AND substr(json_extract(data, '$.text'), 1, ?1) = ?2)",
        rusqlite::params![prefix.chars().count() as i64, prefix],
        |r| r.get(0),
    )?)
}

/// Whether the `bug-breach` Event of bug `id` and `reason` is already recorded.
fn breached(conn: &Connection, id: &str, reason: &str) -> Result<bool, CpdError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM events WHERE kind = 'bug-breach'
                          AND json_extract(data, '$.bug') = ?1 AND json_extract(data, '$.reason') = ?2)",
        rusqlite::params![id, reason],
        |r| r.get(0),
    )?)
}

/// The writes this tick owes, read from the store: the missing owner wakes and the missing breaches.
fn actions(store: &mut Store, now: OffsetDateTime, bugs: &[Bug]) -> Result<Vec<Action>, CpdError> {
    let normalized: Vec<String> = bugs.iter().map(|b| normalized(&b.symptom)).collect();
    let mut out = Vec::new();
    for (i, bug) in bugs.iter().enumerate() {
        let owner = store.read(|conn| owner_of(conn, bug))?;
        if let Some(owner) = &owner
            && !store.read(|conn| woken(conn, &bug.id))?
        {
            out.push(Action::Wake {
                id: bug.id.clone(),
                owner: owner.clone(),
                title: bug.title.clone(),
            });
        }
        let past = age(now, &bug.reported_at)? >= BOUND;
        let recurring = bugs.iter().enumerate().any(|(j, other)| {
            j != i
                && normalized[j] == normalized[i]
                && (&other.reported_at, &other.id) < (&bug.reported_at, &bug.id)
        });
        for reason in [
            past.then_some("unmitigated_past_bound"),
            recurring.then_some("recurring_symptom"),
        ]
        .into_iter()
        .flatten()
        {
            if !store.read(|conn| breached(conn, &bug.id, reason))? {
                out.push(Action::Breach {
                    id: bug.id.clone(),
                    owner: owner.clone(),
                    plan: bug
                        .plan
                        .as_deref()
                        .map(PlanId::parse)
                        .transpose()
                        .map_err(|e| corrupt(&bug.id, e))?,
                    title: bug.title.clone(),
                    reason,
                    symptom: bug.symptom.clone(),
                });
            }
        }
    }
    Ok(out)
}

/// The actions in the caller's transaction: the owner messages and the `bug-breach` Events.
fn apply(tx: &WriteTxn<'_>, actions: &[Action]) -> Result<usize, CpdError> {
    let mut n = 0;
    for action in actions {
        match action {
            Action::Wake { id, owner, title } => {
                tell(tx, owner, &wake_text(id, title))?;
                n += 1;
            }
            Action::Breach {
                id,
                owner,
                plan,
                title,
                reason,
                symptom,
            } => {
                let text = breach_text(id, title, reason);
                let mut event =
                    NewEvent::bug_protocol_breach(text.clone(), id, reason, symptom, BOUND);
                if let Some(plan) = plan {
                    event = event.plan(plan.clone());
                }
                events::append(tx, event)?;
                if let Some(owner) = owner {
                    tell(tx, owner, &text)?;
                }
                n += 1;
            }
        }
    }
    Ok(n)
}

/// The wake message of contract §2.2: its text begins `Bug <id> is open`.
fn wake_text(id: &str, title: &str) -> String {
    format!(
        "Bug {id} is open: \"{title}\". Mitigate it with cpd protocol bug mitigate {id} --mitigation \"<text>\", \
         or close it with cpd protocol bug close {id} --root-fix <ref> --regression-test <test>. Next: cpd protocol bug show {id}"
    )
}

/// The breach message of contract §2.2: its text begins `Bug <id> breached`.
fn breach_text(id: &str, title: &str, reason: &str) -> String {
    format!(
        "Bug {id} breached the bug report protocol ({reason}): \"{title}\". Mitigate it with cpd protocol bug mitigate {id} --mitigation \"<text>\", \
         or close it with cpd protocol bug close {id} --root-fix <ref> --regression-test <test>. Next: cpd protocol bug show {id}"
    )
}

/// Contract §2.2's normalization: the symptom lowercased, every run of whitespace one space, every
/// character that is not a letter or a digit dropped; two normalized symptoms match on exact equality.
fn normalized(symptom: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in symptom.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        } else if c.is_whitespace() {
            space = true;
        }
    }
    out
}

/// The whole seconds between `now` and the bug's `reported_at`.
fn age(now: OffsetDateTime, reported_at: &str) -> Result<i64, CpdError> {
    let at = OffsetDateTime::parse(reported_at, &Rfc3339).map_err(|e| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("bug reported_at {reported_at:?} does not parse: {e}"),
        )
        .with("reported_at", reported_at)
    })?;
    Ok((now - at).whole_seconds())
}

/// A `bug_records.plan` that does not parse: the database was written outside cpd.
fn corrupt(id: &str, e: CpdError) -> CpdError {
    CpdError::new(
        ErrorCode::StoreFailed,
        format!(
            "Bug {id} names a Plan that does not parse: {}.",
            e.message()
        ),
    )
    .with("bug", id)
}
