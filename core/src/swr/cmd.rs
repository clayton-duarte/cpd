//! `cpd swr` (commands.md §32): read the cache and the outbox, and invalidate a key.

use rusqlite::{OptionalExtension, Row, params};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::badge::{self, Classification};
use super::{cache, endpoint};
use crate::clock::timestamp;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;

/// The `swr` commands (commands.md §32).
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// The cached queries (§32.1)
    List(ListArgs),
    /// One query and its active mutations (§32.2)
    Show(KeyArgs),
    /// Ask the engine to fetch a query now (§32.3)
    Invalidate(KeyArgs),
    /// The health of every endpoint (§32.4)
    Endpoints,
    /// The mutation outbox (§32.5, §32.6)
    #[command(subcommand)]
    Mutation(MutationCommand),
    /// The Lead completion badge (classifier doc §3)
    #[command(subcommand)]
    Badge(BadgeCommand),
    /// Endpoint health of a system (classifier doc §3.3)
    #[command(subcommand)]
    Health(HealthCommand),
}

/// The `swr health` commands.
#[derive(clap::Subcommand, Debug)]
pub enum HealthCommand {
    /// Record one call result on an endpoint
    Record(HealthRecordArgs),
}

#[derive(clap::Args, Debug)]
pub struct HealthRecordArgs {
    /// classifier, github, slack or jira
    pub system: String,
    /// The endpoint of the system
    pub endpoint: String,
    /// ok or error
    pub result: String,
    /// The error text, required with `error`
    #[arg(long, value_name = "text")]
    pub text: Option<String>,
}

/// The `swr badge` commands.
#[derive(clap::Subcommand, Debug)]
pub enum BadgeCommand {
    /// Print a Session's stored badge, or null
    Get(BadgeGetArgs),
    /// Store a Session's badge
    Put(BadgePutArgs),
}

#[derive(clap::Args, Debug)]
pub struct BadgeGetArgs {
    /// The Session id
    pub session_id: String,
}

#[derive(clap::Args, Debug)]
pub struct BadgePutArgs {
    /// The Session id
    pub session_id: String,
    /// The Session's last transcript item id at classify time
    #[arg(long, value_name = "id")]
    pub entry: String,
    /// A completion label
    #[arg(long, value_name = "label")]
    pub label: String,
    /// An integer from 0 to 100
    #[arg(long, value_name = "n")]
    pub score: i64,
    /// The badge text
    #[arg(long, value_name = "text")]
    pub text: String,
    /// done, decision or blocked
    #[arg(long, value_name = "tone")]
    pub tone: String,
    /// A JSON array of {label, score}
    #[arg(long, value_name = "json")]
    pub others: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Only the queries of this datasource
    #[arg(long, value_name = "system")]
    pub system: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct KeyArgs {
    /// A query key, `<system>:<kind>:<id>`
    pub key: String,
}

/// The `swr mutation` commands.
#[derive(clap::Subcommand, Debug)]
pub enum MutationCommand {
    /// One mutation (§32.5)
    Show(IdArgs),
    /// The mutations, filtered by status (§32.6)
    List(StatusArgs),
}

#[derive(clap::Args, Debug)]
pub struct IdArgs {
    /// A mutation id, a positive integer
    pub id: String,
}

#[derive(clap::Args, Debug)]
pub struct StatusArgs {
    /// Only the mutations in this status
    #[arg(long, value_name = "status")]
    pub status: Option<String>,
}

const SYSTEMS: [&str; 5] = ["github", "slack", "jira", "models", "classifier"];
const STATUSES: [&str; 6] = [
    "pending",
    "in_flight",
    "revalidating",
    "done",
    "failed",
    "superseded",
];
/// The endpoints in the order `swr endpoints` prints them.
const ENDPOINTS: [&str; 5] = ["github", "slack", "jira", "ollama.lab", "bifrost"];

fn usage(message: String) -> CpdError {
    CpdError::new(ErrorCode::Usage, message)
}

pub fn run(ports: &Ports, command: Command) -> Result<Output, CpdError> {
    // Validate the arguments before the store is opened.
    match &command {
        Command::List(ListArgs {
            system: Some(system),
        }) if !SYSTEMS.contains(&system.as_str()) => {
            return Err(usage(format!("--system is one of {}.", SYSTEMS.join(", "))));
        }
        Command::Show(a) | Command::Invalidate(a) => check_key(&a.key)?,
        Command::Mutation(MutationCommand::Show(a)) => {
            id_of(a)?;
        }
        Command::Mutation(MutationCommand::List(StatusArgs {
            status: Some(status),
        })) if !STATUSES.contains(&status.as_str()) => {
            return Err(usage(format!(
                "--status is one of {}.",
                STATUSES.join(", ")
            )));
        }
        Command::Health(HealthCommand::Record(a)) => check_health(a)?,
        Command::Badge(BadgeCommand::Put(a)) => {
            check_label(&a.label)?;
            check_score(a.score)?;
            if a.entry.is_empty() || a.text.is_empty() || a.session_id.is_empty() {
                return Err(usage("--entry and --text are non-empty.".into()));
            }
            if !TONES.contains(&a.tone.as_str()) {
                return Err(usage(format!("--tone is one of {}.", TONES.join(", "))));
            }
            others_of(a.others.as_deref())?;
        }
        _ => {}
    }
    let mut env = Env::open_with(ports)?;
    let now = ports.clock.utc();
    let store = &mut env.store;
    match command {
        Command::List(a) => list(store, now, a.system.as_deref()),
        Command::Show(a) => show(store, now, &a.key),
        Command::Invalidate(a) => invalidate(store, &a.key),
        Command::Endpoints => endpoints(store),
        Command::Mutation(MutationCommand::Show(a)) => mutation_show(store, id_of(&a)?),
        Command::Mutation(MutationCommand::List(a)) => mutation_list(store, a.status.as_deref()),
        Command::Health(HealthCommand::Record(a)) => health_record(store, now, &a),
        Command::Badge(BadgeCommand::Get(a)) => {
            let badge = badge::get(store, &a.session_id)?;
            Ok(Output::success(
                json!({"badge": badge.map(|b| badge_json(&b))}),
            ))
        }
        Command::Badge(BadgeCommand::Put(a)) => {
            let others = others_of(a.others.as_deref())?;
            let c = Classification {
                entry_id: a.entry,
                label: a.label,
                score: a.score,
                text: a.text,
                tone: a.tone,
                others,
            };
            let put = badge::put(store, now, &a.session_id, &c)?;
            Ok(Output::success(json!({
                "session_id": a.session_id,
                "entry_id": c.entry_id,
                "changed": put.changed,
                "synced_at": timestamp(put.synced_at),
            })))
        }
    }
}

fn check_health(a: &HealthRecordArgs) -> Result<(), CpdError> {
    let pairs: [(&str, &[&str]); 4] = [
        ("github", &["github"]),
        ("slack", &["slack"]),
        ("jira", &["jira"]),
        ("classifier", &["ollama.lab", "bifrost"]),
    ];
    let known = pairs
        .iter()
        .any(|(s, e)| *s == a.system && e.contains(&a.endpoint.as_str()));
    if !known {
        return Err(usage(
            "<system> and <endpoint> are github github, slack slack, jira jira, or classifier ollama.lab|bifrost."
                .into(),
        ));
    }
    match (a.result.as_str(), a.text.as_deref()) {
        ("ok", None) => Ok(()),
        ("ok", Some(_)) => Err(usage("--text is refused with ok.".into())),
        ("error", Some(t)) if !t.trim().is_empty() => Ok(()),
        ("error", _) => Err(usage("--text is required and non-empty with error.".into())),
        _ => Err(usage("<result> is ok or error.".into())),
    }
}

fn health_record(
    store: &mut Store,
    now: OffsetDateTime,
    a: &HealthRecordArgs,
) -> Result<Output, CpdError> {
    if a.result == "ok" {
        endpoint::record_ok(store, &a.endpoint, now)?;
    } else {
        let text = a.text.as_deref().unwrap_or_default();
        endpoint::record_failure(store, &a.endpoint, now, text, endpoint::Kind::Transient)?;
    }
    let row = endpoint::get(store, &a.endpoint)?.ok_or_else(|| {
        CpdError::new(
            ErrorCode::Internal,
            "The endpoint row is missing.".to_owned(),
        )
    })?;
    Ok(Output::success(json!({
        "system": a.system, "endpoint": a.endpoint,
        "status": row.status, "error_count": row.error_count,
        "failing_since": row.failing_since.map(timestamp),
    })))
}

const LABELS: [&str; 5] = [
    "safe_to_archive",
    "unanswered_ask",
    "open_work",
    "unrecorded_decision",
    "unescalated_issue",
];
const TONES: [&str; 3] = ["done", "decision", "blocked"];

fn badge_json(b: &badge::Badge) -> Value {
    let c = &b.classification;
    let mut v = json!({
        "session_id": b.session_id, "entry_id": c.entry_id, "label": c.label, "score": c.score,
        "text": c.text, "tone": c.tone, "synced_at": timestamp(b.synced_at),
    });
    if let Some(o) = &c.others {
        v["others"] = o.clone();
    }
    v
}

fn check_score(n: i64) -> Result<(), CpdError> {
    if (0..=100).contains(&n) {
        Ok(())
    } else {
        Err(usage("A score is an integer from 0 to 100.".into()))
    }
}

fn check_label(label: &str) -> Result<(), CpdError> {
    if LABELS.contains(&label) {
        Ok(())
    } else {
        Err(usage(format!("A label is one of {}.", LABELS.join(", "))))
    }
}

fn others_of(text: Option<&str>) -> Result<Option<Value>, CpdError> {
    let Some(text) = text else { return Ok(None) };
    let bad = || usage("--others is a non-empty JSON array of {label, score}.".into());
    let value: Value = serde_json::from_str(text).map_err(|_| bad())?;
    let items = value.as_array().filter(|a| !a.is_empty()).ok_or_else(bad)?;
    for item in items {
        let label = item.get("label").and_then(Value::as_str).ok_or_else(bad)?;
        let score = item.get("score").and_then(Value::as_i64).ok_or_else(bad)?;
        check_label(label)?;
        check_score(score)?;
    }
    Ok(Some(value))
}

fn check_key(key: &str) -> Result<(), CpdError> {
    let parts: Vec<&str> = key.splitn(3, ':').collect();
    if parts.len() == 3 && parts.iter().all(|p| !p.is_empty()) {
        Ok(())
    } else {
        Err(usage("A query key is `<system>:<kind>:<id>`.".into()).with("key", key.to_owned()))
    }
}

fn id_of(a: &IdArgs) -> Result<i64, CpdError> {
    a.id.parse::<i64>()
        .ok()
        .filter(|id| *id >= 1 && !a.id.starts_with('+'))
        .ok_or_else(|| {
            usage("A mutation id is a positive integer.".into()).with("id", a.id.clone())
        })
}

const QUERY_COLUMNS: &str = "key, system, status, fetched_at, stale_after, invalidated, error_count, last_error, inactive_at";

fn query_json(r: &Row<'_>, now: OffsetDateTime) -> Result<Value, rusqlite::Error> {
    let fetched: Option<String> = r.get(3)?;
    let age = fetched
        .as_deref()
        .and_then(|t| OffsetDateTime::parse(t, &Rfc3339).ok())
        .map(|t| (now - t).whole_seconds());
    Ok(json!({
        "key": r.get::<_, String>(0)?,
        "system": r.get::<_, String>(1)?,
        "status": r.get::<_, String>(2)?,
        "fetched_at": fetched,
        "stale_after": r.get::<_, Option<String>>(4)?,
        "invalidated": r.get::<_, i64>(5)? != 0,
        "error_count": r.get::<_, i64>(6)?,
        "last_error": r.get::<_, Option<String>>(7)?,
        "inactive_at": r.get::<_, Option<String>>(8)?,
        "age_seconds": age,
    }))
}

fn list(store: &mut Store, now: OffsetDateTime, system: Option<&str>) -> Result<Output, CpdError> {
    let queries = store.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT {QUERY_COLUMNS} FROM swr_queries WHERE ?1 IS NULL OR system = ?1 ORDER BY key"
        ))?;
        let rows = st.query_map(params![system], |r| query_json(r, now))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;
    Ok(Output::success(json!({"queries": queries})))
}

fn query_row(store: &mut Store, now: OffsetDateTime, key: &str) -> Result<Value, CpdError> {
    store
        .read(|c| {
            Ok(c.query_row(
                &format!("SELECT {QUERY_COLUMNS} FROM swr_queries WHERE key = ?1"),
                [key],
                |r| query_json(r, now),
            )
            .optional()?)
        })?
        .ok_or_else(|| {
            CpdError::new(ErrorCode::QueryNotFound, "No such query.").with("key", key.to_owned())
        })
}

fn show(store: &mut Store, now: OffsetDateTime, key: &str) -> Result<Output, CpdError> {
    let query = query_row(store, now, key)?;
    let mutations = store.read(|c| {
        let mut st = c.prepare(
            "SELECT id, op_kind, status, patch_json FROM swr_mutations
             WHERE target_key = ?1 AND status IN ('pending', 'in_flight', 'revalidating')
             ORDER BY submitted_at, id",
        )?;
        let rows = st.query_map([key], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;
    let mutations = mutations
        .into_iter()
        .map(|(id, op_kind, status, patch)| {
            Ok(json!({"id": id, "op_kind": op_kind, "status": status, "patch": parse(patch)?}))
        })
        .collect::<Result<Vec<Value>, CpdError>>()?;
    Ok(Output::success(
        json!({"query": query, "mutations": mutations}),
    ))
}

fn invalidate(store: &mut Store, key: &str) -> Result<Output, CpdError> {
    let already = query_row(store, OffsetDateTime::UNIX_EPOCH, key)?["invalidated"] == true;
    if !already {
        cache::invalidate(store, key)?;
    }
    Ok(Output::success(
        json!({"key": key, "invalidated": true, "already": already}),
    ))
}

fn endpoints(store: &mut Store) -> Result<Output, CpdError> {
    let rows = store.read(|c| {
        let mut st = c.prepare(
            "SELECT endpoint, COALESCE(status, 'pending'), fetch_status, paused_until, failing_since, reported_at,
                    last_error, error_count, last_ok_at, rate_remaining, rate_reset_at
             FROM swr_endpoints",
        )?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                json!({
                    "endpoint": r.get::<_, String>(0)?,
                    "status": r.get::<_, String>(1)?,
                    "fetch_status": r.get::<_, String>(2)?,
                    "paused_until": r.get::<_, Option<String>>(3)?,
                    "failing_since": r.get::<_, Option<String>>(4)?,
                    "reported_at": r.get::<_, Option<String>>(5)?,
                    "last_error": r.get::<_, Option<String>>(6)?,
                    "error_count": r.get::<_, i64>(7)?,
                    "last_ok_at": r.get::<_, Option<String>>(8)?,
                    "rate_remaining": r.get::<_, Option<i64>>(9)?,
                    "rate_reset_at": r.get::<_, Option<String>>(10)?,
                }),
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;
    let mut out: Vec<Value> = ENDPOINTS
        .iter()
        .map(|name| {
            rows.iter().find(|(n, _)| n == name).map_or_else(
                || {
                    json!({"endpoint": name, "status": "pending", "fetch_status": "idle", "paused_until": null,
                           "failing_since": null, "reported_at": null, "last_error": null, "error_count": 0,
                           "last_ok_at": null, "rate_remaining": null, "rate_reset_at": null})
                },
                |(_, v)| v.clone(),
            )
        })
        .collect();
    // An endpoint a source added that this list does not know follows, in name order.
    let mut extra: Vec<&(String, Value)> = rows
        .iter()
        .filter(|(n, _)| !ENDPOINTS.contains(&n.as_str()))
        .collect();
    extra.sort_by(|a, b| a.0.cmp(&b.0));
    out.extend(extra.into_iter().map(|(_, v)| v.clone()));
    Ok(Output::success(json!({"endpoints": out})))
}

const MUTATION_COLUMNS: &str = "id, system, target_key, op_kind, status, vars_json, patch_json, context_json, submitted_at, started_at, completed_at, next_attempt_at, attempt_count, error";

fn parse(text: Option<String>) -> Result<Value, CpdError> {
    match text {
        None => Ok(Value::Null),
        Some(t) => serde_json::from_str(&t).map_err(|e| {
            CpdError::new(
                ErrorCode::Internal,
                format!("A stored mutation column is not JSON: {e}."),
            )
        }),
    }
}

fn mutation_json(r: &Row<'_>) -> Result<Result<Value, CpdError>, rusqlite::Error> {
    let (vars, patch, context): (String, Option<String>, Option<String>) =
        (r.get(5)?, r.get(6)?, r.get(7)?);
    let columns = (|| {
        Ok::<Value, rusqlite::Error>(json!({
            "id": r.get::<_, i64>(0)?,
            "system": r.get::<_, String>(1)?,
            "target_key": r.get::<_, String>(2)?,
            "op_kind": r.get::<_, String>(3)?,
            "status": r.get::<_, String>(4)?,
            "submitted_at": r.get::<_, String>(8)?,
            "started_at": r.get::<_, Option<String>>(9)?,
            "completed_at": r.get::<_, Option<String>>(10)?,
            "next_attempt_at": r.get::<_, Option<String>>(11)?,
            "attempt_count": r.get::<_, i64>(12)?,
            "error": r.get::<_, Option<String>>(13)?,
        }))
    })()?;
    Ok((|| {
        let mut v = columns;
        v["vars"] = parse(Some(vars))?;
        v["patch"] = parse(patch)?;
        v["context"] = parse(context)?;
        Ok(v)
    })())
}

fn mutation_show(store: &mut Store, id: i64) -> Result<Output, CpdError> {
    let found = store.read(|c| {
        Ok(c.query_row(
            &format!("SELECT {MUTATION_COLUMNS} FROM swr_mutations WHERE id = ?1"),
            [id],
            mutation_json,
        )
        .optional()?)
    })?;
    match found {
        Some(m) => Ok(Output::success(json!({"mutation": m?}))),
        None => Err(CpdError::new(ErrorCode::MutationNotFound, "No such mutation.").with("id", id)),
    }
}

fn mutation_list(store: &mut Store, status: Option<&str>) -> Result<Output, CpdError> {
    let rows = store.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT {MUTATION_COLUMNS} FROM swr_mutations WHERE ?1 IS NULL OR status = ?1 ORDER BY id"
        ))?;
        let rows = st.query_map(params![status], mutation_json)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;
    let mutations = rows.into_iter().collect::<Result<Vec<Value>, CpdError>>()?;
    Ok(Output::success(json!({"mutations": mutations})))
}
