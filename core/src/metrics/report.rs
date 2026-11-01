//! `cpd metrics` (commands.md §14.7, hs-d798): the read-only aggregate of the
//! machine's Ledger, as one JSON value (default) or the Prometheus text
//! exposition format.
//!
//! Read-only: nothing here writes. The aggregate includes archived Plans
//! (contract §2.2: a Context's cost includes archived Plans). Fail-open
//! (hs-d798): this reads the local store only and cpd depends on no exporter,
//! Prometheus or homeserver.

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::env::Env;
use crate::error::CpdError;
use crate::output::Output;

/// `cpd metrics` takes an output format only.
#[derive(clap::Args, Clone, Debug)]
pub struct MetricsArgs {
    /// The output format: `json` (the default) or `prometheus`
    #[arg(long, value_enum, default_value_t = Format::Json)]
    pub format: Format,
}

/// The output formats of `cpd metrics` (commands.md §14.7).
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Prometheus,
}

/// `cpd metrics` (§14.7): the aggregate, printed in `args.format`.
pub fn run(args: MetricsArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let report = Report::read(&mut env)?;
    match args.format {
        Format::Json => Ok(Output::success(report.to_json())),
        Format::Prometheus => Ok(Output::success(json!({ "data": report.prometheus() }))),
    }
}

/// The counts and sums of one Tier or one model.
#[derive(Default)]
struct Agg {
    attempts: u64,
    merged: u64,
    failed_by_cause: BTreeMap<String, u64>,
    durations: Vec<f64>,
    cost_total: f64,
    cost_n: u64,
}

impl Agg {
    fn add(&mut self, outcome: Option<&str>, cause: Option<&str>, duration: Option<f64>, cost: Option<f64>) {
        self.attempts += 1;
        if outcome == Some("merged") {
            self.merged += 1;
        }
        if outcome == Some("failed") {
            let cause = cause.unwrap_or("unknown").to_owned();
            *self.failed_by_cause.entry(cause).or_insert(0) += 1;
        }
        if let Some(d) = duration {
            self.durations.push(d);
        }
        if let Some(c) = cost {
            self.cost_total += c;
            self.cost_n += 1;
        }
    }

    fn avg_duration(&self) -> f64 {
        if self.durations.is_empty() {
            return 0.0;
        }
        self.durations.iter().sum::<f64>() / self.durations.len() as f64
    }

    fn median_duration(&self) -> f64 {
        if self.durations.is_empty() {
            return 0.0;
        }
        let mut sorted = self.durations.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let n = sorted.len();
        if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
        }
    }

    fn avg_cost(&self) -> f64 {
        if self.cost_n == 0 {
            return 0.0;
        }
        self.cost_total / self.cost_n as f64
    }

    fn to_json(&self) -> Value {
        json!({
            "attempts": self.attempts,
            "merged": self.merged,
            "failed_by_cause": self.failed_by_cause,
            "duration_s": { "avg": self.avg_duration(), "median": self.median_duration() },
            "cost_usd": { "avg": self.avg_cost(), "total": self.cost_total },
        })
    }
}

/// The whole aggregate of `cpd metrics`.
struct Report {
    store: String,
    tiers: BTreeMap<String, Agg>,
    models: BTreeMap<String, Agg>,
    /// The attempts per (tier, model), the `cpd_attempts{tier,model}` family.
    tier_model: BTreeMap<(String, String), u64>,
    retries: BTreeMap<String, u64>,
    plans: BTreeMap<String, f64>,
    queue_avg: f64,
    queue_max: f64,
    job_blocked: u64,
}

impl Report {
    /// Reads the aggregate from the store of `env`.
    fn read(env: &mut Env) -> Result<Report, CpdError> {
        let store = env.store.path().display().to_string();
        let (tiers, models, tier_model, retries, plans, queue_avg, queue_max, job_blocked) =
            env.store.read(|c| {
                let mut tiers: BTreeMap<String, Agg> = BTreeMap::new();
                let mut models: BTreeMap<String, Agg> = BTreeMap::new();
                let mut tier_model: BTreeMap<(String, String), u64> = BTreeMap::new();

                {
                    let mut stmt = c.prepare(
                        "SELECT tier, json_extract(metrics, '$.model'), outcome, cause,
                                json_extract(metrics, '$.duration_s'), json_extract(metrics, '$.cost_usd')
                         FROM attempts",
                    )?;
                    let rows = stmt.query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, Option<String>>(3)?,
                            r.get::<_, Option<f64>>(4)?,
                            r.get::<_, Option<f64>>(5)?,
                        ))
                    })?;
                    for row in rows {
                        let (tier, model, outcome, cause, duration, cost) = row?;
                        tiers
                            .entry(tier.clone())
                            .or_default()
                            .add(outcome.as_deref(), cause.as_deref(), duration, cost);
                        if let Some(model) = model {
                            *tier_model.entry((tier, model.clone())).or_insert(0) += 1;
                            models
                                .entry(model)
                                .or_default()
                                .add(outcome.as_deref(), cause.as_deref(), duration, cost);
                        }
                    }
                }

                let mut retries: BTreeMap<String, u64> = BTreeMap::new();
                {
                    let mut stmt =
                        c.prepare("SELECT json_extract(data, '$.cause') FROM events WHERE kind = 'attempt-retry'")?;
                    let rows = stmt.query_map([], |r| r.get::<_, Option<String>>(0))?;
                    for row in rows {
                        if let Some(cause) = row? {
                            *retries.entry(cause).or_insert(0) += 1;
                        }
                    }
                }

                let mut cost_by_plan: BTreeMap<String, f64> = BTreeMap::new();
                {
                    let mut stmt = c.prepare(
                        "SELECT plan, sum(json_extract(metrics, '$.cost_usd')) FROM attempts
                         WHERE metrics IS NOT NULL GROUP BY plan",
                    )?;
                    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<f64>>(1)?)))?;
                    for row in rows {
                        let (plan, cost) = row?;
                        cost_by_plan.insert(plan, cost.unwrap_or(0.0));
                    }
                }
                let mut merged_prs: BTreeMap<String, i64> = BTreeMap::new();
                {
                    let mut stmt = c.prepare("SELECT plan, count(*) FROM prs WHERE status = 'merged' GROUP BY plan")?;
                    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
                    for row in rows {
                        let (plan, n) = row?;
                        merged_prs.insert(plan, n);
                    }
                }
                let mut plans: BTreeMap<String, f64> = BTreeMap::new();
                for (plan, cost) in cost_by_plan {
                    if let Some(&n) = merged_prs.get(&plan) {
                        if n > 0 {
                            plans.insert(plan, cost / n as f64);
                        }
                    }
                }

                // `job-added` -> the first `attempt-started` of the same Job.
                let mut added: Vec<(i64, String, String, String)> = Vec::new();
                {
                    let mut stmt = c.prepare("SELECT id, plan, job, at FROM events WHERE kind = 'job-added' ORDER BY id")?;
                    let rows = stmt.query_map([], |r| {
                        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
                    })?;
                    for row in rows {
                        added.push(row?);
                    }
                }
                let mut waits: Vec<f64> = Vec::new();
                for (id, plan, job, at) in added {
                    let started: Option<String> = c
                        .query_row(
                            "SELECT at FROM events WHERE kind = 'attempt-started' AND plan = ?1 AND job = ?2
                             AND id > ?3 ORDER BY id LIMIT 1",
                            rusqlite::params![plan, job, id],
                            |r| r.get(0),
                        )
                        .optional()?;
                    if let (Some(from), Some(to)) = (parse_at(&at), started.as_deref().and_then(parse_at)) {
                        waits.push((to - from).max(0.0));
                    }
                }
                let queue_avg = if waits.is_empty() {
                    0.0
                } else {
                    waits.iter().sum::<f64>() / waits.len() as f64
                };
                let queue_max = waits.iter().cloned().fold(0.0_f64, f64::max);

                let job_blocked: i64 =
                    c.query_row("SELECT count(*) FROM events WHERE kind = 'job-blocked'", [], |r| r.get(0))?;

                Ok((tiers, models, tier_model, retries, plans, queue_avg, queue_max, job_blocked as u64))
            })?;

        Ok(Report { store, tiers, models, tier_model, retries, plans, queue_avg, queue_max, job_blocked })
    }

    /// The default JSON value (commands.md §14.7): `store`, `per_tier`, `per_model`, `retries`, `plans`, `queue`.
    fn to_json(&self) -> Value {
        let per_tier: BTreeMap<&String, Value> = self.tiers.iter().map(|(k, a)| (k, a.to_json())).collect();
        let per_model: BTreeMap<&String, Value> = self.models.iter().map(|(k, a)| (k, a.to_json())).collect();
        json!({
            "store": self.store,
            "per_tier": per_tier,
            "per_model": per_model,
            "retries": self.retries,
            "plans": self.plans,
            "queue": {
                "job_added_to_attempt_started": { "avg": self.queue_avg, "max": self.queue_max },
                "job_blocked_episodes": self.job_blocked,
            },
        })
    }

    /// The same aggregates in the Prometheus text exposition format (commands.md §14.7), output only.
    fn prometheus(&self) -> String {
        let mut out = String::new();

        family(&mut out, "cpd_attempts", "Attempts per tier and model.");
        for ((tier, model), n) in &self.tier_model {
            out.push_str(&format!("cpd_attempts{{tier=\"{}\",model=\"{}\"}} {n}\n", escape(tier), escape(model)));
        }

        family(&mut out, "cpd_attempts_merged", "Merged attempts per tier.");
        for (tier, a) in &self.tiers {
            out.push_str(&format!("cpd_attempts_merged{{tier=\"{}\"}} {}\n", escape(tier), a.merged));
        }

        family(&mut out, "cpd_attempts_failed", "Failed attempts per tier and cause.");
        for (tier, a) in &self.tiers {
            for (cause, n) in &a.failed_by_cause {
                out.push_str(&format!(
                    "cpd_attempts_failed{{tier=\"{}\",cause=\"{}\"}} {n}\n",
                    escape(tier),
                    escape(cause)
                ));
            }
        }

        family(&mut out, "cpd_attempt_duration_seconds", "Attempt duration in seconds per tier.");
        for (tier, a) in &self.tiers {
            out.push_str(&format!(
                "cpd_attempt_duration_seconds{{tier=\"{}\",stat=\"avg\"}} {}\n",
                escape(tier),
                a.avg_duration()
            ));
            out.push_str(&format!(
                "cpd_attempt_duration_seconds{{tier=\"{}\",stat=\"median\"}} {}\n",
                escape(tier),
                a.median_duration()
            ));
        }

        family(&mut out, "cpd_attempt_cost_usd_total", "Total attempt cost in USD per tier.");
        for (tier, a) in &self.tiers {
            out.push_str(&format!("cpd_attempt_cost_usd_total{{tier=\"{}\"}} {}\n", escape(tier), a.cost_total));
        }

        family(&mut out, "cpd_attempt_cost_usd_avg", "Average attempt cost in USD per tier.");
        for (tier, a) in &self.tiers {
            out.push_str(&format!("cpd_attempt_cost_usd_avg{{tier=\"{}\"}} {}\n", escape(tier), a.avg_cost()));
        }

        family(&mut out, "cpd_model_attempts", "Attempts per model.");
        for (model, a) in &self.models {
            out.push_str(&format!("cpd_model_attempts{{model=\"{}\"}} {}\n", escape(model), a.attempts));
        }

        family(&mut out, "cpd_model_cost_usd_total", "Total attempt cost in USD per model.");
        for (model, a) in &self.models {
            out.push_str(&format!("cpd_model_cost_usd_total{{model=\"{}\"}} {}\n", escape(model), a.cost_total));
        }

        family(&mut out, "cpd_attempt_retries", "Retries per cause.");
        for (cause, n) in &self.retries {
            out.push_str(&format!("cpd_attempt_retries{{cause=\"{}\"}} {n}\n", escape(cause)));
        }

        family(&mut out, "cpd_plan_cost_usd_per_merged_pr", "Attempt cost in USD per merged pull request, per Plan.");
        for (plan, v) in &self.plans {
            out.push_str(&format!("cpd_plan_cost_usd_per_merged_pr{{plan=\"{}\"}} {v}\n", escape(plan)));
        }

        family(&mut out, "cpd_queue_wait_seconds", "Job-added to first attempt-started wait in seconds.");
        out.push_str(&format!(
            "cpd_queue_wait_seconds{{phase=\"added_to_started\",stat=\"avg\"}} {}\n",
            self.queue_avg
        ));
        out.push_str(&format!(
            "cpd_queue_wait_seconds{{phase=\"added_to_started\",stat=\"max\"}} {}\n",
            self.queue_max
        ));

        family(&mut out, "cpd_job_blocked_episodes", "Job-blocked episodes.");
        out.push_str(&format!("cpd_job_blocked_episodes {}\n", self.job_blocked));

        out
    }
}

/// The `# HELP` and `# TYPE` lines of a gauge family.
fn family(out: &mut String, name: &str, help: &str) {
    out.push_str(&format!("# HELP {name} {help}\n"));
    out.push_str(&format!("# TYPE {name} gauge\n"));
}

/// A label value escaped per the Prometheus text format: backslash, double quote and newline.
fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// The seconds since the epoch of an RFC 3339 timestamp, or None when it does not parse.
fn parse_at(text: &str) -> Option<f64> {
    OffsetDateTime::parse(text, &Rfc3339).ok().map(|t| t.unix_timestamp_nanos() as f64 / 1_000_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_escapes_backslash_quote_and_newline() {
        assert_eq!(escape("a\\b\"c\nd"), "a\\\\b\\\"c\\nd");
    }

    #[test]
    fn median_of_even_count_averages_the_middle() {
        let mut a = Agg::default();
        for d in [1.0, 3.0, 2.0, 4.0] {
            a.add(None, None, Some(d), None);
        }
        assert_eq!(a.median_duration(), 2.5);
        assert_eq!(a.avg_duration(), 2.5);
    }

    #[test]
    fn empty_aggregate_is_zero() {
        let a = Agg::default();
        assert_eq!((a.avg_duration(), a.median_duration(), a.avg_cost()), (0.0, 0.0, 0.0));
    }
}
