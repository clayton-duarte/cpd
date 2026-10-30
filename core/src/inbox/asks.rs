//! A repeated ask (contract §3.7 "A repeated ask"): a Job that repeats a question nobody answers. A pure read of the store;
//! every join is keyed by (plan, id), (plan, attempt) and (plan, job), never by id alone.

use std::collections::{BTreeMap, BTreeSet};

use time::OffsetDateTime;

use super::rules::{id, time_of};
use crate::error::CpdError;
use crate::ids::{ContextId, JobKey, PlanId};
use crate::store::Store;

/// Equal questions of one Job at which it is reported.
const THRESHOLD: usize = 3;
/// Characters of a question the key and the text keep.
const KEPT: usize = 60;

/// The case of one Job: its largest group of equal unanswered questions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepeatedAsk {
    pub plan: PlanId,
    pub job: JobKey,
    /// The Plan's Context.
    pub context: ContextId,
    /// Escalations in the group.
    pub count: usize,
    /// Distinct Attempts among them.
    pub attempts: usize,
    /// Whole minutes from the group's earliest `asked_at` to `now`.
    pub minutes: i64,
    /// The earliest Escalation's question: first 60 characters, control characters as spaces.
    pub question: String,
}

fn key(question: &str) -> String {
    question.chars().take(KEPT).map(|c| if c.is_ascii_digit() { '#' } else { c }).collect()
}

struct Group {
    count: usize,
    attempts: BTreeSet<String>,
    first: OffsetDateTime,
    question: String,
}

/// One case per (plan, job) with a group of [`THRESHOLD`] or more, ordered by plan then job.
pub fn repeated_asks(store: &mut Store, now: OffsetDateTime) -> Result<Vec<RepeatedAsk>, CpdError> {
    store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT a.plan, a.job, p.context, e.attempt, e.asked_at, e.question
               FROM escalations e
               JOIN attempts a ON a.plan = e.plan AND a.id = e.attempt
               JOIN jobs j ON j.plan = a.plan AND j.key = a.job
               JOIN plans p ON p.id = j.plan
              WHERE e.status IN ('expired', 'open') AND j.status IN ('queued', 'running', 'failed')
              ORDER BY a.plan, a.job",
        )?;
        let mut rows = stmt.query([])?;
        let mut jobs: BTreeMap<(String, String), (String, BTreeMap<String, Group>)> = BTreeMap::new();
        while let Some(row) = rows.next()? {
            let (plan, job, context, attempt, at, question): (String, String, String, String, String, String) =
                (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?);
            let at = time_of("Escalation time", &at)?;
            let (_, groups) = jobs.entry((plan, job)).or_insert_with(|| (context, BTreeMap::new()));
            let g = groups.entry(key(&question)).or_insert_with(|| Group {
                count: 0,
                attempts: BTreeSet::new(),
                first: at,
                question: question.clone(),
            });
            g.count += 1;
            g.attempts.insert(attempt);
            if at < g.first {
                g.first = at;
                g.question = question;
            }
        }
        let mut out = Vec::new();
        for ((plan, job), (context, groups)) in jobs {
            // Most Escalations, then the older first ask, then the smaller key (the map's order).
            let best = groups
                .into_iter()
                .filter(|(_, g)| g.count >= THRESHOLD)
                .min_by(|(ka, a), (kb, b)| b.count.cmp(&a.count).then(a.first.cmp(&b.first)).then(ka.cmp(kb)));
            let Some((_, g)) = best else { continue };
            out.push(RepeatedAsk {
                plan: id("Plan id", &plan, PlanId::parse)?,
                job: id("Job key", &job, JobKey::parse)?,
                context: id("Context id", &context, ContextId::parse)?,
                count: g.count,
                attempts: g.attempts.len(),
                minutes: (now - g.first).whole_minutes(),
                question: g.question.chars().take(KEPT).map(|c| if c.is_control() { ' ' } else { c }).collect(),
            });
        }
        Ok(out)
    })
}
