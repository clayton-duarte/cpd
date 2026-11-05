//! The cost read model's cache and total .

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use cpd_core::error::CpdError;
use cpd_core::metrics;

type Stamp = (SystemTime, u64);

/// Costs of session files by path, valid while the file's `(mtime, len)` stamp is unchanged.
pub(crate) struct CostCache {
    entries: HashMap<PathBuf, (Stamp, f64)>,
}

impl CostCache {
    pub(crate) fn new() -> CostCache {
        CostCache {
            entries: HashMap::new(),
        }
    }

    pub(crate) fn cost(
        &mut self,
        path: &Path,
        stat: impl FnOnce(&Path) -> Option<Stamp>,
        parse: impl FnOnce(&Path) -> Result<f64, CpdError>,
    ) -> Option<f64> {
        let Some(stamp) = stat(path) else {
            self.entries.remove(path);
            return None;
        };
        if let Some((cached, cost)) = self.entries.get(path)
            && *cached == stamp
        {
            return Some(*cost);
        }
        match parse(path) {
            Ok(cost) => {
                self.entries.insert(path.to_owned(), (stamp, cost));
                Some(cost)
            }
            Err(_) => self.entries.get(path).map(|(_, c)| *c),
        }
    }
}

/// The own cost of the session file at `path`, through the process-wide cache.
pub(crate) fn own_cost(path: &Path) -> Option<f64> {
    static CACHE: LazyLock<Mutex<CostCache>> = LazyLock::new(|| Mutex::new(CostCache::new()));
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.cost(
        path,
        |p| {
            let m = std::fs::metadata(p).ok()?;
            Some((m.modified().ok()?, m.len()))
        },
        |p| metrics::parse_file(p).map(|m| m.cost_usd),
    )
}

/// The stamp of a durable host store at `path`: the latest mtime and the summed length of the main file and its `-wal`
/// (writes land in the `-wal` until a checkpoint; a missing `-wal` is 0). `None` when the store is missing.
pub(crate) fn durable_store_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    let (mtime, len) = (m.modified().ok()?, m.len());
    Some(match std::fs::metadata(PathBuf::from(wal)) {
        Ok(w) => (mtime.max(w.modified().ok()?), len + w.len()),
        Err(_) => (mtime, len),
    })
}

const DURABLE_COST_SQL: &str = "SELECT coalesce(sum(
  (SELECT coalesce(sum(json_extract(v.value,'$.cost.total')),0) FROM json_each(r.content,'$.models') v)
+ (SELECT coalesce(sum(json_extract(v.value,'$.cost.total')),0) FROM json_each(r.content,'$.tools') v)
), 0)
FROM document_revisions r JOIN documents d ON d.id = r.document_id
WHERE d.kind = '\"pi.usage\"' AND d.retired_at IS NULL AND r.kind = 'base'";

fn parse_durable(path: &Path) -> Result<f64, CpdError> {
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(CpdError::bad_row)?;
    conn.query_row(DURABLE_COST_SQL, [], |r| r.get::<_, f64>(0))
        .map_err(CpdError::bad_row)
}

/// The cumulative cost of the durable host store at `path`, through the process-wide cache; `None` when missing.
pub(crate) fn durable_cost(path: &Path) -> Option<f64> {
    static CACHE: LazyLock<Mutex<CostCache>> = LazyLock::new(|| Mutex::new(CostCache::new()));
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.cost(path, durable_store_stamp, parse_durable)
}

/// An Attempt's cost: the stored `$.cost_usd` when `metrics` is written (`no_metrics` false), else the own cost of its
/// Worker Session's `file`; the two are never added.
pub(crate) fn attempt_cost(
    stored: Option<rusqlite::types::Value>,
    no_metrics: bool,
    file: Option<&str>,
) -> Option<f64> {
    if no_metrics {
        file.and_then(|f| own_cost(Path::new(f)))
    } else {
        match stored {
            Some(rusqlite::types::Value::Real(c)) => Some(c),
            Some(rusqlite::types::Value::Integer(c)) => Some(c as f64),
            _ => None,
        }
    }
}

/// `None` when every part is `None`, else the sum of the parts that have a value.
pub(crate) fn total(parts: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    parts
        .into_iter()
        .flatten()
        .fold(None, |acc, c| Some(acc.unwrap_or(0.0) + c))
}
