//! `run/models-<provider>.json` (contract §1.4, §3.7 E15): the ids a provider offered at its last good list, the diff
//! of each write, and the write itself, whole, by `.tmp` and rename, under `locks/models.lock`.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::helper::{Answer, Entry, Listed, ProviderFailure};
use crate::clock::timestamp;
use crate::config::Timeouts;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::lock;
use crate::ports::Ports;

/// How often a waiter retries `locks/models.lock`.
const RETRY: Duration = Duration::from_millis(20);

/// The last change: the diff of the write that made it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changes {
    pub at: String,
    pub appeared: Vec<String>,
    pub went_away: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// `copilot-available` for pi's API list, `openai-models` for a `GET <baseUrl>/models`.
    pub kind: String,
    pub auth: String,
    pub base: Option<String>,
}

/// A cache file (contract §1.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub version: u32,
    pub provider: String,
    /// The time of the last GOOD list.
    pub polled_at: String,
    pub pi_version: String,
    pub source: Source,
    pub ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<Changes>,
}

/// What reading one file gave.
#[derive(Clone, Debug)]
// The variant carries the one value this enum exists to hand back; boxing is a type change out of scope.
#[allow(clippy::large_enum_variant)]
pub enum Read {
    Good(File),
    /// Not JSON of the contract's shape: treated as missing, and reported.
    Unparseable { provider: String, path: PathBuf },
}

fn path_of(home: &CpdHome, provider: &str) -> PathBuf {
    home.run_dir().join(format!("models-{provider}.json"))
}

fn io_failed(what: &str, path: &Path, e: std::io::Error) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("Cannot {what} {}: {e}.", path.display())).with("path", path.display().to_string())
}

fn read_file(provider: &str, path: &Path) -> Result<Option<Read>, CpdError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_failed("read", path, e)),
    };
    Ok(Some(match serde_json::from_str::<File>(&text) {
        Ok(file) if file.version == 1 && file.provider == provider && OffsetDateTime::parse(&file.polled_at, &Rfc3339).is_ok() => Read::Good(file),
        _ => Read::Unparseable { provider: provider.to_owned(), path: path.to_owned() },
    }))
}

/// Every `run/models-<provider>.json`, in name order.
pub fn read_all(home: &CpdHome) -> Result<Vec<Read>, CpdError> {
    let dir = home.run_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_failed("list", &dir, e)),
    };
    let mut names = BTreeSet::new();
    for entry in entries {
        let name = entry.map_err(|e| io_failed("list", &dir, e))?.file_name().to_string_lossy().into_owned();
        if let Some(provider) = name.strip_prefix("models-").and_then(|n| n.strip_suffix(".json")).filter(|p| super::safe_name(p)) {
            names.insert(provider.to_owned());
        }
    }
    let mut reads = Vec::new();
    for provider in names {
        reads.extend(read_file(&provider, &path_of(home, &provider))?);
    }
    Ok(reads)
}

/// The newest `polled_at` among the files that parse: `None` when there is none (E15 is due at once).
pub fn newest_polled_at(home: &CpdHome) -> Result<Option<OffsetDateTime>, CpdError> {
    Ok(read_all(home)?
        .iter()
        .filter_map(|r| match r {
            Read::Good(f) => OffsetDateTime::parse(&f.polled_at, &Rfc3339).ok(),
            Read::Unparseable { .. } => None,
        })
        .max())
}

/// A provider written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub provider: String,
    pub polled_at: String,
    pub count: usize,
    /// The diff of this write when it is not empty.
    pub changes: Option<Changes>,
}

#[derive(Clone, Debug)]
pub enum Outcome {
    Written(Written),
    Failed(ProviderFailure),
}

/// What [`apply`] did, in the helper's order.
#[derive(Clone, Debug, Default)]
pub struct Applied {
    pub outcomes: Vec<Outcome>,
    /// Files that were read as missing: `(provider, path)`.
    pub unparseable: Vec<(String, PathBuf)>,
}

/// `locks/models.lock`, held while the value lives; `lock_timeout` after `busy_ms`.
fn take_lock(home: &CpdHome, busy_ms: u32) -> Result<std::fs::File, CpdError> {
    let path = home.locks_dir().join("models.lock");
    fs::create_dir_all(home.locks_dir()).map_err(|e| io_failed("create the directory of", &path, e))?;
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).map_err(|e| io_failed("open", &path, e))?;
    let wait = Duration::from_millis(u64::from(busy_ms));
    let started = Instant::now();
    loop {
        if lock::try_lock(&file).map_err(|e| io_failed("lock", &path, e))? {
            return Ok(file);
        }
        if started.elapsed() >= wait {
            return Err(CpdError::new(ErrorCode::LockTimeout, format!("Another model-list write held {} for longer than {} ms.", path.display(), busy_ms))
                .with("lock", path.display().to_string())
                .with("waited_ms", busy_ms));
        }
        std::thread::sleep(RETRY);
    }
}

/// `run/models-<provider>.json` whole: a `.tmp` and a rename, so a reader never sees half of it.
fn write_file(path: &Path, file: &File) -> Result<(), CpdError> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(file).expect("a cache file serializes")).map_err(|e| io_failed("write", &tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| io_failed("replace", path, e))
}

fn sorted(ids: &[String]) -> Vec<String> {
    ids.iter().cloned().collect::<BTreeSet<_>>().into_iter().collect()
}

fn diff(old: &[String], new: &[String], at: &str) -> Option<Changes> {
    let (old, new): (BTreeSet<_>, BTreeSet<_>) = (old.iter().collect(), new.iter().collect());
    let appeared: Vec<String> = new.difference(&old).map(|s| (*s).clone()).collect();
    let went_away: Vec<String> = old.difference(&new).map(|s| (*s).clone()).collect();
    (!appeared.is_empty() || !went_away.is_empty()).then(|| Changes { at: at.to_owned(), appeared, went_away })
}

fn write_listed(home: &CpdHome, pi_version: &str, listed: &Listed, now: &str, applied: &mut Applied) -> Result<Outcome, CpdError> {
    let provider = &listed.provider;
    let ids = sorted(&listed.ids);
    if ids.is_empty() {
        return Ok(Outcome::Failed(ProviderFailure {
            provider: provider.clone(),
            code: "models_empty".to_owned(),
            message: format!("The list of {provider} holds no id: the last good list stays."),
            details: serde_json::Map::new(),
        }));
    }
    let path = path_of(home, provider);
    let previous = match read_file(provider, &path)? {
        Some(Read::Good(file)) => Some(file),
        Some(Read::Unparseable { provider, path }) => {
            applied.unparseable.push((provider, path));
            None
        }
        None => None,
    };
    // Lists of another auth type are not comparable (an API-key login lists more than its plan allows).
    let comparable = previous.filter(|p| p.source.auth == listed.auth);
    let this = comparable.as_ref().and_then(|p| diff(&p.ids, &ids, now));
    // The file keeps the last change, not the last poll.
    let kept = this.clone().or_else(|| comparable.and_then(|p| p.changes));
    let known = listed.known.as_deref().map(sorted);
    let kind = if listed.auth == "custom" { "openai-models" } else { "copilot-available" };
    let file = File {
        version: 1,
        provider: provider.clone(),
        polled_at: now.to_owned(),
        pi_version: pi_version.to_owned(),
        source: Source { kind: kind.to_owned(), auth: listed.auth.clone(), base: listed.base.clone() },
        ids,
        known,
        changes: kept,
    };
    fs::create_dir_all(home.run_dir()).map_err(|e| io_failed("create", &home.run_dir(), e))?;
    write_file(&path, &file)?;
    Ok(Outcome::Written(Written { provider: provider.clone(), polled_at: now.to_owned(), count: file.ids.len(), changes: this }))
}

/// Writes the good lists of `answer` (contract §3.7 E15), each against its own previous file, under
/// `locks/models.lock`; a failed or empty one writes nothing, and a provider the answer does not name keeps its file.
/// Errors: `lock_timeout`, `store_failed`.
pub fn apply(home: &CpdHome, busy: &Timeouts, answer: &Answer) -> Result<Applied, CpdError> {
    apply_at(home, busy, answer, Ports::current().clock.utc())
}

/// [`apply`] with `polled_at` the time `at` (the engine's clock, so a test's virtual time is what the files say).
pub fn apply_at(home: &CpdHome, busy: &Timeouts, answer: &Answer, at: OffsetDateTime) -> Result<Applied, CpdError> {
    let _lock = take_lock(home, busy.store_busy_ms)?;
    let now = timestamp(at);
    let mut applied = Applied::default();
    for entry in &answer.entries {
        let outcome = match entry {
            Entry::Listed(listed) => write_listed(home, &answer.pi_version, listed, &now, &mut applied)?,
            Entry::Failed(failure) => Outcome::Failed(failure.clone()),
        };
        applied.outcomes.push(outcome);
    }
    Ok(applied)
}
