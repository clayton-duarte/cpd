//! What the machine holds in `$CPD_HOME` (commands.md §14.1): the store, the
//! tamper report and the engine.

use std::fs::File;
use std::path::Path;

use serde_json::{Value, json};

use super::check::{Check, Severity};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::lock;
use cpd_core::plan::jobs;
use cpd_core::store::Store;

const NO_STORE: &str = "the store did not open.";

/// The database opens, migrates and passes the schema check. A store that
/// opened is returned for the checks that read it, even when the schema check
/// failed: reads still work (contract §4.3).
pub fn store(opened: Result<Store, CpdError>) -> (Check, Option<Store>) {
    match opened {
        Err(e) => (refusal_check(&e), None),
        Ok(store) => {
            let check = match store.schema_mismatch() {
                None => Check::pass("store", Severity::Error),
                Some(m) => Check::fail(
                    "store",
                    Severity::Error,
                    "The schema is not this cpd's: a table or trigger was changed outside cpd.",
                )
                .found(m.found.clone())
                .expected(m.expected.clone()),
            };
            (check, Some(store))
        }
    }
}

/// The `store` check of a store that did not open. `found` is the verdict's refusal (`code`,
/// `message`, commands.md §14.1) when the refusal is one of the verdict's (contract §4.5); any other
/// failure keeps its details.
fn refusal_check(e: &CpdError) -> Check {
    let check = Check::from_error("store", Severity::Error, e);
    match e.code() {
        ErrorCode::StoreTooNew | ErrorCode::MigrationChanged | ErrorCode::MigrationOrder => {
            check.found(json!({"code": e.code().as_str(), "message": e.message()}))
        }
        _ => check,
    }
}

/// The tamper report (contract §4.3) is empty; `found` lists it.
pub fn tamper(store: Option<&mut Store>) -> Check {
    let name = "tamper";
    let Some(store) = store else {
        return Check::not_checked(name, Severity::Error, NO_STORE);
    };
    match tamper_report(store) {
        Err(e) => Check::from_error(name, Severity::Error, &e),
        Ok(report) if report.is_empty() => Check::pass(name, Severity::Error),
        Ok(report) => Check::fail(name, Severity::Error, "The Ledger was changed outside cpd.")
            .found(Value::Array(report)),
    }
}

/// The tamper report of contract §4.3: the store's part (the schema check and
/// the orphaned `history` rows), then, Plan by Plan, every Locked Job whose
/// recomputed fingerprint (§2.3) differs from the stored one.
fn tamper_report(store: &mut Store) -> Result<Vec<Value>, CpdError> {
    let mut report = store.tamper()?;
    let fingerprints = store.read(|conn| {
        let mut stmt = conn.prepare("SELECT id FROM plans ORDER BY id")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut found = Vec::new();
        for id in ids {
            let id = PlanId::parse(&id)?;
            found.extend(jobs::tampered(&id, &jobs::load(conn, &id)?));
        }
        Ok(found)
    })?;
    report.extend(fingerprints);
    Ok(report)
}

/// The engine runs (it holds `locks/watch.lock`) when any Plan is in `dispatch` or `deploy`.
pub fn watch(home: &CpdHome, store: Option<&mut Store>) -> Check {
    let name = "watch";
    let Some(store) = store else {
        return Check::not_checked(name, Severity::Warn, NO_STORE);
    };
    let plans = store.read(|conn| {
        Ok(conn.query_row(
            "SELECT count(*) FROM plans WHERE phase IN ('dispatch', 'deploy')",
            [],
            |r| r.get::<_, i64>(0),
        )?)
    });
    let plans = match plans {
        Ok(plans) => plans,
        Err(e) => return Check::from_error(name, Severity::Warn, &e),
    };
    if plans == 0 {
        return Check::pass(name, Severity::Warn).message("No Plan is in dispatch or deploy.");
    }
    match engine_runs(&home.watch_lock()) {
        Ok(true) => Check::pass(name, Severity::Warn).found(format!("{plans} Plan(s) in dispatch or deploy; the engine runs")),
        Ok(false) => Check::fail(name, Severity::Warn, format!("{plans} Plan(s) are in dispatch or deploy and no engine runs: start it with `cpd watch --ensure`.")),
        Err(message) => Check::fail(name, Severity::Warn, message),
    }
}

/// Whether another process holds `lock` (the engine's flock, contract §6.6).
/// Takes nothing: a missing file is no engine, and the probe's shared lock is
/// let go at once.
fn engine_runs(lock: &Path) -> Result<bool, String> {
    let file = match File::open(lock) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("cannot open {}: {e}.", lock.display())),
    };
    lock::shared(&file)
        .map(|free| !free)
        .map_err(|e| format!("cannot probe {}: {e}.", lock.display()))
}
