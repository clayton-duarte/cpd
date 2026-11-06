//! `cpd swr` (commands.md §32): read the cache and the outbox, and invalidate a key.

use rusqlite::{OptionalExtension, Row, params};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::cache;
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
    }
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
