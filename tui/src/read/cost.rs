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
