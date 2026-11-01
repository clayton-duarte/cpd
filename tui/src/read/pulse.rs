//! The Pulse view's snapshot: one Context's Plans, each with its Lead's dot, its 5-minute activity
//! buckets, merges, age and quiet. Everything is read from events, plans, jobs, attempts and sessions,
//! in one read transaction with a fixed number of queries (events are grouped by SQL, never read one by one).
//!
//! Each counted Event has a [`Kind`] (commands.md §13.2, Kinds; User, hs-d327), by its Event kind, and a bucket takes the
//! first kind present in the order of the enum's variants (the precedence of the bar's colour):
//!
//! | Kind | Colour | Event kinds |
//! |---|---|---|
//! | [`Kind::Error`] | red | `error`, `job-failed` |
//! | [`Kind::Plan`] | magenta | `plan-phase`, `plan-review`, `plan-done` |
//! | [`Kind::PrOpened`] | yellow | `pr-opened` |
//! | [`Kind::Escalation`] | blue | `escalation`, `escalation-answered`, `escalation-expired` |
//! | [`Kind::Merged`] | green | `job-merged`, `pr-merged` |
//! | [`Kind::Other`] | white | every other counted Event, the Lead turns included |

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;

use cpd_core::error::CpdError;
use cpd_core::events::highest_id;
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::store::Store;

use super::dot::Dot;
use super::shared::{PlanHead, SessionNode, bad_row, id, plan_heads, sessions};

/// A bucket's width.
pub const BUCKET_MINUTES: i64 = 5;
/// A Plan with queued or running work and no activity for this long is quiet.
pub const QUIET_MINUTES: i64 = 30;

const BUCKET_MS: i64 = BUCKET_MINUTES * 60_000;

/// The Manager in the chat slot: the newest `session-moved` Event with `data.place = 'chat'` over all Sessions is a role-`manager` Session's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagerSlot {
    pub session: SessionId,
    pub context: ContextId,
    pub moved_event: i64,
}

/// The Manager in the chat slot, `None` when a Lead is there, or nobody ever was.
pub fn manager_slot(store: &mut Store) -> Result<Option<ManagerSlot>, CpdError> {
    store.read(|conn| {
        let row: Option<(String, String, String, i64)> = conn
            .query_row(
                "SELECT s.id, s.role, s.context, e.id FROM events e JOIN sessions s ON s.id = e.session
                  WHERE e.kind = 'session-moved' AND json_extract(e.data, '$.place') = 'chat'
                  ORDER BY e.id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        match row {
            Some((session, role, context, moved_event)) if role == "manager" => Ok(Some(ManagerSlot {
                session: id("Session id", &session, SessionId::parse)?,
                context: id("Context id", &context, ContextId::parse)?,
                moved_event,
            })),
            _ => Ok(None),
        }
    })
}

/// One Context's Pulse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pulse {
    pub context: ContextId,
    pub rows: Vec<PulseRow>,
    /// Largest `Bucket::count` over all rows (0 when none).
    pub max: u32,
    /// The highest Event id in this snapshot's read transaction.
    pub events_cursor: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PulseRow {
    pub plan: PlanId,
    pub lead: SessionId,
    pub dot: Dot,
    /// Oldest first, `len` = the `buckets` asked for.
    pub buckets: Vec<Bucket>,
    /// `job-merged` + `pr-merged` Events in the window.
    pub merges: u32,
    /// Minutes since the latest activity Event; `Some(0)` while an Attempt is not ended; `None` with no activity Event ever.
    pub age_minutes: Option<i64>,
    pub quiet: bool,
}

/// The kind of activity of an Event. The variants are in the order of precedence: a bucket takes the smallest (`min`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Error,
    Plan,
    PrOpened,
    Escalation,
    Merged,
    Other,
}

impl Kind {
    /// The kind of a counted Event of Event kind `event` (the table in the module doc).
    pub fn of(event: &str) -> Kind {
        match event {
            "error" | "job-failed" => Kind::Error,
            "plan-phase" | "plan-review" | "plan-done" => Kind::Plan,
            "pr-opened" => Kind::PrOpened,
            "escalation" | "escalation-answered" | "escalation-expired" => Kind::Escalation,
            "job-merged" | "pr-merged" => Kind::Merged,
            _ => Kind::Other,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bucket {
    pub count: u32,
    /// The first [`Kind`] among the bucket's Events; `None` when `count` is 0.
    pub kind: Option<Kind>,
    pub job_merged: bool,
    pub pr_merged: bool,
    pub worker_ran: bool,
}

impl Bucket {
    /// Counts `count` Events of Event kind `event` in the bucket: the count adds up and the kind is the first in precedence.
    fn add(&mut self, event: &str, count: u32) {
        let kind = Kind::of(event);
        self.count += count;
        self.kind = Some(self.kind.map_or(kind, |k| k.min(kind)));
        self.job_merged |= event == "job-merged";
        self.pr_merged |= event == "pr-merged";
    }
}

/// The Events that count as a Plan's activity: these kinds, and the Lead turns (`session-state` to `working` of a `lead` Session).
const ACTIVITY: &str = "(e.kind IN ('plan-phase', 'plan-review', 'plan-done', 'job-added', 'job-edited', 'job-merged',
        'job-failed', 'job-retried', 'job-cancel-requested', 'job-cancelled', 'attempt-started', 'attempt-status',
        'attempt-retry', 'attempt-nudge', 'all-merged', 'pr-feedback', 'workflow-added', 'workflow-edited', 'workflow-cancelled',
        'workflow-started', 'pr-opened', 'pr-merged', 'escalation', 'escalation-answered', 'escalation-expired', 'error')
    OR (e.kind = 'session-state' AND json_extract(e.data, '$.to') = 'working' AND s.role = 'lead'))";

/// Unix ms of an RFC 3339 UTC timestamp with milliseconds (`2026-10-18T14:57:18.848Z`), as SQL over `col`.
fn ms(col: &str) -> String {
    format!(
        "(CAST(strftime('%s', {col}) AS INTEGER) * 1000 + CAST(substr({col}, 21, 3) AS INTEGER))"
    )
}

/// `context`'s Pulse at `now_ms` (Unix ms), with `buckets` 5-minute buckets ending at the current, partial bucket.
pub fn pulse(
    store: &mut Store,
    context: &ContextId,
    now_ms: i64,
    buckets: usize,
) -> Result<Pulse, CpdError> {
    let start_ms = now_ms.div_euclid(BUCKET_MS) * BUCKET_MS - (buckets as i64 - 1) * BUCKET_MS;
    store.read(|conn| {
        let start_at: String =
            conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', ?1 / 1000.0, 'unixepoch')", [start_ms], |r| r.get(0))?;
        let heads: Vec<PlanHead> = plan_heads(conn, None)?
            .into_iter()
            .filter(|h| h.context == *context && !h.is_archived())
            .filter(|h| h.done_at.as_ref().is_none_or(|d| *d >= start_at))
            .collect();
        let mut leads: Vec<SessionNode> = sessions(conn, "s.context = ?1 AND s.role = 'lead'", &[&context.as_str()])?;
        // The Lead's dot comes from the Inbox, as in the tree (§17).
        let now: String = conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |r| r.get(0))?;
        super::tree::fill_dots(&super::tree::visible_items(conn, &now)?, &heads, &mut leads);

        // Window: count, kind, merges per (plan, bucket), from the counts of each Event kind.
        let mut window: BTreeMap<(String, usize), Bucket> = BTreeMap::new();
        let mut merged: BTreeMap<String, u32> = BTreeMap::new();
        let mut stmt = conn.prepare(&format!(
            "SELECT e.plan, ({ms} - ?2) / ?3, e.kind, count(*)
               FROM events e LEFT JOIN sessions s ON s.id = e.session
              WHERE e.context = ?1 AND e.plan IS NOT NULL AND {ms} >= ?2 AND {ACTIVITY}
              GROUP BY e.plan, 2, e.kind",
            ms = ms("e.at")
        ))?;
        let mut rows = stmt.query(rusqlite::params![context.as_str(), start_ms, BUCKET_MS])?;
        while let Some(r) = rows.next()? {
            let (plan, index, event, count): (String, i64, String, i64) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
            if (0..buckets as i64).contains(&index) {
                window.entry((plan.clone(), index as usize)).or_default().add(&event, count as u32);
                if Kind::of(&event) == Kind::Merged {
                    *merged.entry(plan).or_default() += count as u32;
                }
            }
        }

        // The latest activity, whenever it was.
        let mut latest: BTreeMap<String, i64> = BTreeMap::new();
        let mut stmt = conn.prepare(&format!(
            "SELECT e.plan, max({ms}) FROM events e LEFT JOIN sessions s ON s.id = e.session
              WHERE e.context = ?1 AND e.plan IS NOT NULL AND {ACTIVITY} GROUP BY e.plan",
            ms = ms("e.at")
        ))?;
        for row in stmt.query_map([context.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (plan, at) = row?;
            latest.insert(plan, at);
        }

        // Attempts: (start, end or now) per Plan, and the Plans with an Attempt not ended.
        let mut spans: BTreeMap<String, Vec<(i64, i64)>> = BTreeMap::new();
        let mut running: Vec<String> = Vec::new();
        let mut stmt = conn.prepare(&format!(
            "SELECT a.plan, {started}, CASE WHEN a.ended_at IS NULL THEN NULL ELSE {ended} END, a.status <> 'ended'
               FROM attempts a JOIN plans p ON p.id = a.plan WHERE p.context = ?1",
            started = ms("a.started_at"),
            ended = ms("a.ended_at")
        ))?;
        for row in stmt.query_map([context.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?, r.get::<_, bool>(3)?))
        })? {
            let (plan, from, to, live) = row?;
            if live {
                running.push(plan.clone());
            }
            spans.entry(plan).or_default().push((from, to.unwrap_or(now_ms)));
        }

        let mut out = Vec::new();
        let mut max = 0;
        for head in heads {
            let key = head.id.as_str().to_owned();
            let lead = leads
                .iter()
                .find(|s| s.plan_lead && s.plan.as_ref() == Some(&head.id))
                .ok_or_else(|| bad_row(format!("Plan {} has no Lead Session", head.id)))?;
            let mut cells: Vec<Bucket> = (0..buckets).map(|i| window.remove(&(key.clone(), i)).unwrap_or_default()).collect();
            for (i, cell) in cells.iter_mut().enumerate() {
                let (from, to) = (start_ms + i as i64 * BUCKET_MS, start_ms + (i as i64 + 1) * BUCKET_MS);
                cell.worker_ran = spans.get(&key).is_some_and(|v| v.iter().any(|&(a, b)| a < to && b >= from));
                max = max.max(cell.count);
            }
            let attempt_live = running.contains(&key);
            let age_minutes = if attempt_live { Some(0) } else { latest.get(&key).map(|&at| (now_ms - at).max(0) / 60_000) };
            let work = head.counts.queued + head.counts.running > 0;
            let old = age_minutes.is_none_or(|m| m >= QUIET_MINUTES);
            out.push((lead.dot.order_key(&head.created_at), PulseRow {
                plan: head.id.clone(),
                lead: lead.id.clone(),
                dot: lead.dot,
                buckets: cells,
                merges: merged.get(&key).copied().unwrap_or(0),
                age_minutes,
                quiet: work && !attempt_live && old && !lead.dot.needs_you(),
            }));
        }
        // tree order (hs-d686, pro-d48): the Lead's dot tier, then the Plan's creation, newest first; stable
        out.sort_by(|a, b| a.0.cmp(&b.0));
        let out: Vec<PulseRow> = out.into_iter().map(|(_, row)| row).collect();
        Ok(Pulse { context: context.clone(), rows: out, max, events_cursor: highest_id(conn)? })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_counted_event_kind_has_its_kind_of_activity() {
        for (events, kind) in [
            (&["error", "job-failed"][..], Kind::Error),
            (&["plan-phase", "plan-review", "plan-done"], Kind::Plan),
            (&["pr-opened"], Kind::PrOpened),
            (
                &["escalation", "escalation-answered", "escalation-expired"],
                Kind::Escalation,
            ),
            (&["job-merged", "pr-merged"], Kind::Merged),
            (&["job-added", "job-retried", "attempt-started", "attempt-status", "all-merged", "pr-feedback", "workflow-started", "session-state"], Kind::Other),
        ] {
            for event in events {
                assert_eq!(Kind::of(event), kind, "{event}");
            }
        }
    }

    #[test]
    fn a_bucket_with_several_kinds_takes_red_magenta_yellow_blue_green_white_in_that_order() {
        let order = [
            "error",
            "plan-phase",
            "pr-opened",
            "escalation",
            "job-merged",
            "job-added",
        ];
        for first in 0..order.len() {
            // every kind from `first` on, added in any order: the first of them wins
            for reversed in [false, true] {
                let mut events: Vec<&str> = order[first..].to_vec();
                if reversed {
                    events.reverse();
                }
                let mut bucket = Bucket::default();
                for event in events {
                    bucket.add(event, 2);
                }
                assert_eq!(
                    bucket.kind,
                    Some(Kind::of(order[first])),
                    "from {}",
                    order[first]
                );
                assert_eq!(
                    bucket.count,
                    2 * (order.len() - first) as u32,
                    "the count is the sum: the scale does not change"
                );
            }
        }
    }

    #[test]
    fn an_empty_bucket_has_no_kind_and_the_merge_flags_follow_the_event_kinds() {
        assert_eq!(Bucket::default().kind, None);
        let mut bucket = Bucket::default();
        bucket.add("job-merged", 1);
        assert_eq!((bucket.job_merged, bucket.pr_merged), (true, false));
        bucket.add("pr-merged", 1);
        assert_eq!((bucket.job_merged, bucket.pr_merged), (true, true));
    }
}
