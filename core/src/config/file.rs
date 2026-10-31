//! `$CPD_HOME/config.toml` (contract §8.1, §8.2): parsed once into a typed
//! [`Config`], defaults built in, every leaf key's source recorded.
//!
//! The file is read key by key instead of through serde's
//! `deny_unknown_fields`: serde's errors carry no key path, and contract §8.1
//! wants the key path in `details`. Every key is named once, in [`Config::parse`].
//! Whatever the walk does not consume is an unknown key.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use toml::{Table, Value};

use super::runner::WorkerRunner;
use super::tier::{Letter, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{ContextId, MachineName};

/// Where a leaf key's value came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    File,
    Default,
}

/// Every leaf key's source, by dotted key path (`timeouts.attempt_minutes`).
pub type Sources = BTreeMap<String, Source>;

/// The effective configuration: the file merged over the built-in defaults.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Config {
    pub version: u32,
    pub machine: MachineName,
    /// Attempts not ended, machine-wide (§3.4).
    pub concurrency: u32,
    /// Automatic Attempts are capped at `1 + retry_cap` (§9).
    pub retry_cap: u32,
    pub lead_tier: Tier,
    /// The Tier of the Lead of a Plan in the review Context (`inbox.review_context`).
    pub review_lead_tier: Tier,
    /// The Tier of a Context's Manager (§3.1, hs-d133).
    pub manager_tier: Tier,
    /// The Tier of the distiller, the pi run that writes a Context digest (§4.6, hs-d194).
    pub context_distill_tier: Tier,
    /// The records a Context holds before it is curated (hs-d592).
    pub context_curation_min_records: u32,
    /// The loop-guard breaker trips when more than this many self-started turns run in 600 ms (§3.6, §8.2, hs-d552, hs-d571).
    pub loop_guard_turns: u32,
    /// The Tier of the `open` and `update` Jobs cpd adds (§2.2 "Job kinds", §8.2, hs-d340, hs-d497).
    pub open_tier: Tier,
    pub lead_pi_packages: Vec<String>,
    /// `""` is beside the binary: see [`Config::pi_adapter_dir`].
    pub pi_adapter: String,
    /// Which runner starts a Worker; read once per Attempt, when it is created (contract §3.4, §8.2).
    pub worker_runner: WorkerRunner,
    /// `context_engine_enabled`: the operator kill switch for the holographic engine.
    pub context_engine_enabled: bool,
    /// `""` is beside the binary: see [`Config::context_engine_bridge_path`].
    pub context_engine_bridge: String,
    /// `""` is the venv beside the bridge: see [`Config::context_engine_python_path`].
    pub context_engine_python: String,
    /// `""` is `$CPD_HOME/engines/holographic`: see [`Config::context_engine_dir`].
    pub context_engine_dir: String,
    /// The deadline of one search/sync spawn.
    pub context_engine_timeout_ms: u32,
    /// The deadline of a full reindex spawn.
    pub context_engine_reindex_timeout_ms: u32,
    pub timeouts: Timeouts,
    pub watch: Watch,
    pub inbox: Inbox,
    pub events: Events,
    pub tmux: Tmux,
    /// The `[presets.<name>]` tables of config.toml (contract §8.2 "`[presets.<name>]`"), by name; none by default.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub presets: BTreeMap<String, Preset>,
}

/// A `[presets.<name>]` table (contract §8.2 "`[presets.<name>]`", commands.md §14.6): a named set of the four
/// model refs `O`, `S`, `H` and `G` that `cpd preset apply` writes into the machine host policy's `[model]` table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Preset {
    #[serde(rename = "O")]
    pub o: String,
    #[serde(rename = "S")]
    pub s: String,
    #[serde(rename = "H")]
    pub h: String,
    #[serde(rename = "G")]
    pub g: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Timeouts {
    /// Active time per Attempt (§3.4).
    pub attempt_minutes: u32,
    /// Active time per explore Attempt (a `scout` or `research` Job), instead of `attempt_minutes`.
    pub explore_minutes: u32,
    /// An Escalation expires after this.
    pub ask_minutes: u32,
    /// Pre-start.
    pub setup_minutes: u32,
    /// Pre-commit and pre-merge.
    pub gate_minutes: u32,
    /// SQLite busy timeout (§4.2).
    pub store_busy_ms: u32,
    /// Fork-when-idle (§6.7).
    pub fork_quiet_seconds: u32,
    /// A distill run kills its pi after this (§4.6).
    pub distill_minutes: u32,
    /// A Worker's tool call idle this long has its processes killed (§3.4 A4); 0 = off.
    pub stall_seconds: u32,
    /// The grace pi gets to stop after `SIGTERM` before `SIGKILL` (§6.7).
    pub pi_stop_seconds: u32,
    /// A finished Lead or Manager is put to sleep after this (§3.7 E13).
    pub sleep_seconds: u32,
    /// An asleep Lead or Manager is archived after this (§3.7 E14, hs-d630).
    pub archive_sleep_seconds: u32,
}

/// `watch.tick_seconds`: a decimal of at least 0.1 (§3.7, §8.2), kept in whole milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickSeconds(u32);

impl TickSeconds {
    const MIN_MILLIS: u32 = 100;

    pub fn duration(self) -> Duration {
        Duration::from_millis(u64::from(self.0))
    }

    fn parse(value: Value) -> Result<Self, String> {
        let seconds = match value {
            Value::Integer(i) => i as f64,
            Value::Float(f) => f,
            other => return Err(format!("must be a number, not {}", other.type_str())),
        };
        let millis = (seconds * 1000.0).round();
        if (seconds * 1000.0 - millis).abs() > 1e-6 {
            return Err(format!("must be a whole number of milliseconds, got {seconds}"));
        }
        if millis < f64::from(Self::MIN_MILLIS) {
            return Err(format!("must be at least 0.1, got {seconds}"));
        }
        if millis > f64::from(u32::MAX) {
            return Err(format!("must be at most {}, got {seconds}", u32::MAX / 1000));
        }
        Ok(Self(millis as u32))
    }
}

impl std::fmt::Display for TickSeconds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", f64::from(self.0) / 1000.0)
    }
}

/// A whole number of seconds is shown as an integer, as it was when the key was one.
impl Serialize for TickSeconds {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0 % 1000 == 0 { serializer.serialize_u32(self.0 / 1000) } else { serializer.serialize_f64(f64::from(self.0) / 1000.0) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Watch {
    pub tick_seconds: TickSeconds,
    pub pr_poll_seconds: u32,
    /// E11 reads the team channel this long after its previous read ended (§3.7, §8.2).
    pub team_review_seconds: u32,
    /// E15 reads the model lists again this long after the newest `polled_at` of `run/models-<provider>.json` (§3.7, §8.2).
    pub models_discovery_seconds: u32,
    /// E16 starts a `cpd janitor` pass this often (§3.7, §8.2).
    pub janitor_minutes: u32,
    /// The same `error` Event (step and code) is appended at most once per this (§3.7).
    pub error_repeat_seconds: u32,
    /// E5 raises an `error` once the same `gh` call of the poll has failed with an HTTP 5xx or a timeout this long (§3.7, §8.2).
    pub pr_poll_transient_minutes: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Inbox {
    /// A failed Job becomes a `job-stuck` item after this (E8, §2.2).
    pub stuck_job_minutes: u32,
    /// The Slack channel id of the team review channel; `""` switches E11 and the `review-requested` item off (§2.2, §8.2).
    pub team_channel: String,
    /// The User's Slack user id; set when `team_channel` is.
    pub slack_user_id: String,
    /// The Context whose Manager `cpd inbox claim` tells to review a claimed pull request; `""` refuses the claim (§8.2, §21.7).
    pub review_context: String,
    /// GitHub logins whose reviews do not count as a human's (hermes `BOT_REVIEWERS`).
    pub bot_reviewers: Vec<String>,
}

const DEFAULT_BOT_REVIEWERS: [&str; 3] = ["mission-lane-claude-code-assistant", "github-actions", "copilot-pull-request-reviewer"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Events {
    /// `events follow` (§5.4).
    pub poll_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Tmux {
    /// `tmux -L <socket>`: the cockpit's server.
    pub socket: String,
    pub cockpit_session: String,
    /// The hidden session for Workers and watch.
    pub work_session: String,
}

impl Config {
    /// Reads `$CPD_HOME/config.toml`. `config_missing` when there is no file,
    /// `config_invalid` (with `details.key`) for anything wrong inside it.
    pub fn load(home: &CpdHome) -> Result<Config, CpdError> {
        Ok(Config::load_with_sources(home)?.0)
    }

    /// [`Config::load`], and where each leaf key's value came from.
    pub fn load_with_sources(home: &CpdHome) -> Result<(Config, Sources), CpdError> {
        let path = home.config();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CpdError::new(
                    ErrorCode::ConfigMissing,
                    "config.toml is missing: it is required, because `machine` has no default.",
                )
                .with("path", path.display().to_string()));
            }
            Err(e) => {
                return Err(CpdError::new(ErrorCode::ConfigInvalid, format!("cannot read config.toml: {e}."))
                    .with("path", path.display().to_string()));
            }
        };
        Config::parse(&text, &path)
    }

    /// Parses the text of a `config.toml`; `path` only names the file in errors.
    pub fn parse(text: &str, path: &Path) -> Result<(Config, Sources), CpdError> {
        let table: Table = text.parse().map_err(|e: toml::de::Error| {
            CpdError::new(ErrorCode::ConfigInvalid, format!("config.toml is not valid TOML: {}", e.message().trim_end()))
                .with("path", path.display().to_string())
        })?;
        let mut r = Reader { table, path, sources: Sources::new() };
        r.sections_are_tables(&["timeouts", "watch", "inbox", "events", "tmux", "presets"])?;

        let version = r.require("version", |v| int(v, 1, 1))?;
        let machine = r.require("machine", |v| MachineName::parse(text_of(&v)?).map_err(|e| e.message().to_owned()))?;
        let concurrency = r.get("concurrency", 3, |v| int(v, 1, u32::MAX))?;
        let retry_cap = r.get("retry_cap", 2, |v| int(v, 0, u32::MAX))?;
        // Tier rules (contract §9, hs-d506): no agent uses effort 5; Leads and the Manager may be `O`;
        // an auxiliary run (the distiller) is `H` only; `open_tier` is a Worker's Tier.
        // The default is the Tier `docs/research/opener-tier.md` recommends.
        let lead_tier = r.get("lead_tier", Tier::parse("O2").expect("a valid Tier"), |v| leader_tier(&v))?;
        let review_lead_tier = r.get("review_lead_tier", Tier::parse("S3").expect("a valid Tier"), |v| leader_tier(&v))?;
        let manager_tier = r.get("manager_tier", Tier::parse("O4").expect("a valid Tier"), |v| leader_tier(&v))?;
        let context_distill_tier = r.get("context_distill_tier", Tier::parse("GX").expect("a valid Tier"), |v| {
            let tier = Tier::parse(text_of(&v)?).map_err(|e| e.message().to_owned())?;
            if tier.letter() != Letter::G {
                return Err(format!("must be GX, not {tier}: an auxiliary run uses only GX, to keep cost down (hs-d562)"));
            }
            Ok(tier)
        })?;
        let context_curation_min_records = r.get("context_curation_min_records", 25, |v| int(v, 1, u32::MAX))?;
        let loop_guard_turns = r.get("loop_guard_turns", 20, |v| int(v, 1, u32::MAX))?;
        let open_tier = r.get("open_tier", Tier::parse("H3").expect("a valid Tier"), |v| {
            let tier = Tier::parse(text_of(&v)?).map_err(|e| e.message().to_owned())?;
            tier.check_worker().map_err(|e| e.message().to_owned())?;
            Ok(tier)
        })?;
        let lead_pi_packages = r.get(
            "lead_pi_packages",
            Vec::new(),
            |v| match v {
                Value::Array(items) => items.iter().map(|i| non_empty(text_of(i)?)).collect(),
                other => Err(format!("must be an array of text, not {}", other.type_str())),
            },
        )?;
        let pi_adapter = r.get("pi_adapter", String::new(), |v| {
            let dir = text_of(&v)?;
            if !dir.is_empty() && !Path::new(dir).join("package.json").is_file() {
                return Err(format!("{dir:?} does not contain package.json"));
            }
            Ok(dir.to_owned())
        })?;

        let worker_runner = r.get("worker_runner", WorkerRunner::Pi, |v| WorkerRunner::parse(text_of(&v)?))?;

        let context_engine_enabled = r.get("context_engine_enabled", true, |v| match v {
            Value::Boolean(b) => Ok(b),
            other => Err(format!("must be a boolean, not {}", other.type_str())),
        })?;
        let context_engine_bridge = r.get("context_engine_bridge", String::new(), |v| Ok(text_of(&v)?.to_owned()))?;
        let context_engine_python = r.get("context_engine_python", String::new(), |v| Ok(text_of(&v)?.to_owned()))?;
        let context_engine_dir = r.get("context_engine_dir", String::new(), |v| Ok(text_of(&v)?.to_owned()))?;
        let context_engine_timeout_ms = r.get("context_engine_timeout_ms", 30000, |v| int(v, 1, u32::MAX))?;
        let context_engine_reindex_timeout_ms = r.get("context_engine_reindex_timeout_ms", 600000, |v| int(v, 1, u32::MAX))?;

        let positive = |r: &mut Reader, key: &str, default: u32| r.get(key, default, |v| int(v, 1, u32::MAX));
        let timeouts = Timeouts {
            attempt_minutes: positive(&mut r, "timeouts.attempt_minutes", 30)?,
            explore_minutes: positive(&mut r, "timeouts.explore_minutes", 20)?,
            ask_minutes: positive(&mut r, "timeouts.ask_minutes", 60)?,
            setup_minutes: positive(&mut r, "timeouts.setup_minutes", 15)?,
            gate_minutes: positive(&mut r, "timeouts.gate_minutes", 30)?,
            store_busy_ms: positive(&mut r, "timeouts.store_busy_ms", 5000)?,
            fork_quiet_seconds: positive(&mut r, "timeouts.fork_quiet_seconds", 5)?,
            distill_minutes: positive(&mut r, "timeouts.distill_minutes", 15)?,
            stall_seconds: r.get("timeouts.stall_seconds", 1200, |v| int(v, 0, u32::MAX))?,
            pi_stop_seconds: positive(&mut r, "timeouts.pi_stop_seconds", 10)?,
            sleep_seconds: positive(&mut r, "timeouts.sleep_seconds", 3600)?,
            archive_sleep_seconds: positive(&mut r, "timeouts.archive_sleep_seconds", 86400)?,
        };
        let watch = Watch {
            tick_seconds: r.get("watch.tick_seconds", TickSeconds(5000), TickSeconds::parse)?,
            pr_poll_seconds: positive(&mut r, "watch.pr_poll_seconds", 5)?,
            team_review_seconds: positive(&mut r, "watch.team_review_seconds", 1800)?,
            models_discovery_seconds: positive(&mut r, "watch.models_discovery_seconds", 604_800)?,
            janitor_minutes: positive(&mut r, "watch.janitor_minutes", 60)?,
            error_repeat_seconds: positive(&mut r, "watch.error_repeat_seconds", 600)?,
            pr_poll_transient_minutes: positive(&mut r, "watch.pr_poll_transient_minutes", 10)?,
        };
        let slack_id = |first: char| move |v: Value| {
            let id = text_of(&v)?;
            let ok = id.is_empty()
                || (id.starts_with(first) && id.len() > 1 && id.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
            if ok { Ok(id.to_owned()) } else { Err(format!("{id:?} is not \"\" or a Slack id starting with {first}")) }
        };
        let inbox = Inbox {
            stuck_job_minutes: positive(&mut r, "inbox.stuck_job_minutes", 10)?,
            team_channel: r.get("inbox.team_channel", String::new(), slack_id('C'))?,
            slack_user_id: r.get("inbox.slack_user_id", String::new(), slack_id('U'))?,
            review_context: r.get("inbox.review_context", String::new(), |v| {
                let id = text_of(&v)?;
                if id.is_empty() { Ok(String::new()) } else { ContextId::parse(id).map(|c| c.as_str().to_owned()).map_err(|e| e.to_string()) }
            })?,
            bot_reviewers: r.get("inbox.bot_reviewers", DEFAULT_BOT_REVIEWERS.map(str::to_owned).to_vec(), |v| match v {
                Value::Array(items) => items.iter().map(|i| non_empty(text_of(i)?)).collect(),
                other => Err(format!("must be an array of text, not {}", other.type_str())),
            })?,
        };
        // The claim and mention rules cannot run without the User's id, nor without the channel.
        if inbox.team_channel.is_empty() != inbox.slack_user_id.is_empty() {
            let (key, other) = if inbox.team_channel.is_empty() {
                ("inbox.team_channel", "inbox.slack_user_id")
            } else {
                ("inbox.slack_user_id", "inbox.team_channel")
            };
            return Err(r.invalid(key, format!("must be set together with {other}: both or neither")));
        }
        let events = Events { poll_ms: r.get("events.poll_ms", 200, |v| int(v, 20, 10_000))? };
        let tmux = Tmux {
            socket: r.get("tmux.socket", "cpd".into(), |v| tmux_name(text_of(&v)?))?,
            cockpit_session: r.get("tmux.cockpit_session", "cpd".into(), |v| tmux_name(text_of(&v)?))?,
            work_session: r.get("tmux.work_session", "cpd-work".into(), |v| tmux_name(text_of(&v)?))?,
        };
        if tmux.cockpit_session == tmux.work_session {
            // The error names the later key, `work_session`, as the one to change.
            let key = "tmux.work_session";
            return Err(r.invalid(key, "must differ from tmux.cockpit_session"));
        }

        let presets = r.presets()?;

        r.finish()?;
        let sources = r.sources;
        let config = Config {
            version,
            machine,
            concurrency,
            retry_cap,
            lead_tier,
            review_lead_tier,
            manager_tier,
            context_distill_tier,
            context_curation_min_records,
            loop_guard_turns,
            open_tier,
            lead_pi_packages,
            pi_adapter,
            worker_runner,
            context_engine_enabled,
            context_engine_bridge,
            context_engine_python,
            context_engine_dir,
            context_engine_timeout_ms,
            context_engine_reindex_timeout_ms,
            timeouts,
            watch,
            inbox,
            events,
            tmux,
            presets,
        };
        Ok((config, sources))
    }

    /// The pi adapter package's directory: `pi_adapter`, else
    /// `<dir of the cpd binary>/../share/cpd/pi` (contract §8.2). Whether the
    /// default exists is for `doctor` to say.
    pub fn pi_adapter_dir(&self) -> Result<PathBuf, CpdError> {
        if !self.pi_adapter.is_empty() {
            return Ok(PathBuf::from(&self.pi_adapter));
        }
        let exe = std::env::current_exe().map_err(|e| {
            CpdError::new(ErrorCode::ConfigInvalid, format!("cannot find the cpd binary to locate the pi adapter: {e}."))
                .with("key", "pi_adapter")
        })?;
        let bin_dir = exe.parent().expect("an executable path has a parent");
        Ok(bin_dir.join("..").join("share").join("cpd").join("pi"))
    }

    /// The bridge script's path: `context_engine_bridge`, else
    /// `<dir of the cpd binary>/../share/cpd/holographic/bridge.py` (contract §8.2).
    /// Whether the default exists is for the engine to say (`engine_missing`).
    pub fn context_engine_bridge_path(&self) -> Result<PathBuf, CpdError> {
        if !self.context_engine_bridge.is_empty() {
            return Ok(PathBuf::from(&self.context_engine_bridge));
        }
        let exe = std::env::current_exe().map_err(|e| {
            CpdError::new(ErrorCode::ConfigInvalid, format!("cannot find the cpd binary to locate the holographic bridge: {e}."))
                .with("key", "context_engine_bridge")
        })?;
        let bin_dir = exe.parent().expect("an executable path has a parent");
        Ok(bin_dir.join("..").join("share").join("cpd").join("holographic").join("bridge.py"))
    }

    /// The interpreter's path: `context_engine_python`, else `<bridge dir>/venv/bin/python`, the
    /// dotfiles venv (contract §8.2).
    pub fn context_engine_python_path(&self) -> Result<PathBuf, CpdError> {
        if !self.context_engine_python.is_empty() {
            return Ok(PathBuf::from(&self.context_engine_python));
        }
        let bridge = self.context_engine_bridge_path()?;
        let dir = bridge.parent().expect("a bridge path has a parent");
        Ok(dir.join("venv").join("bin").join("python"))
    }

    /// The engine's data directory: `context_engine_dir`, else `$CPD_HOME/engines/holographic`.
    pub fn context_engine_dir(&self, home: &CpdHome) -> PathBuf {
        if self.context_engine_dir.is_empty() {
            home.root().join("engines").join("holographic")
        } else {
            PathBuf::from(&self.context_engine_dir)
        }
    }

    /// The deadline of one search/sync spawn.
    pub fn context_engine_timeout(&self) -> Duration {
        Duration::from_millis(u64::from(self.context_engine_timeout_ms))
    }

    /// The deadline of a full reindex spawn.
    pub fn context_engine_reindex_timeout(&self) -> Duration {
        Duration::from_millis(u64::from(self.context_engine_reindex_timeout_ms))
    }
}

/// Reads keys out of the parsed table, recording each one's source.
struct Reader<'a> {
    table: Table,
    path: &'a Path,
    sources: Sources,
}

impl Reader<'_> {
    fn invalid(&self, key: &str, why: impl std::fmt::Display) -> CpdError {
        let why = why.to_string();
        CpdError::new(ErrorCode::ConfigInvalid, format!("config.toml: `{key}` {}.", why.trim_end_matches('.')))
            .with("key", key)
            .with("path", self.path.display().to_string())
    }

    fn sections_are_tables(&self, sections: &[&str]) -> Result<(), CpdError> {
        for section in sections {
            match self.table.get(*section) {
                Some(Value::Table(_)) | None => {}
                Some(other) => return Err(self.invalid(section, format!("must be a table, not {}", other.type_str()))),
            }
        }
        Ok(())
    }

    /// Removes and returns the value at `key` (`name` or `section.name`).
    fn take(&mut self, key: &str) -> Option<Value> {
        match key.split_once('.') {
            None => self.table.remove(key),
            Some((section, name)) => match self.table.get_mut(section) {
                Some(Value::Table(t)) => t.remove(name),
                _ => None,
            },
        }
    }

    /// The value at `key`, converted; the default when the file has no such key.
    fn get<T>(&mut self, key: &str, default: T, convert: impl FnOnce(Value) -> Result<T, String>) -> Result<T, CpdError> {
        match self.take(key) {
            Some(value) => {
                let converted = convert(value).map_err(|why| self.invalid(key, format!("is invalid: {why}")))?;
                self.sources.insert(key.to_owned(), Source::File);
                Ok(converted)
            }
            None => {
                self.sources.insert(key.to_owned(), Source::Default);
                Ok(default)
            }
        }
    }

    /// Like [`Reader::get`] for a key without a default.
    fn require<T>(&mut self, key: &str, convert: impl FnOnce(Value) -> Result<T, String>) -> Result<T, CpdError> {
        let Some(value) = self.take(key) else {
            return Err(self.invalid(key, "is required but missing"));
        };
        let converted = convert(value).map_err(|why| self.invalid(key, format!("is invalid: {why}")))?;
        self.sources.insert(key.to_owned(), Source::File);
        Ok(converted)
    }

    /// Whatever is left in the table is a key cpd does not have.
    fn finish(&self) -> Result<(), CpdError> {
        for (name, value) in &self.table {
            match value {
                Value::Table(t) if KNOWN_SECTIONS.contains(&name.as_str()) => {
                    if let Some(inner) = t.keys().next() {
                        return Err(self.invalid(&format!("{name}.{inner}"), "is not a known key"));
                    }
                }
                _ => return Err(self.invalid(name, "is not a known key")),
            }
        }
        Ok(())
    }

    /// The `[presets.<name>]` tables (contract §8.2): each a table of exactly `O`, `S`, `H` and `G`, each a model
    /// ref of §8.3's shape. A name that is not `^[a-z][a-z0-9-]{0,31}$`, an extra key, a missing letter or a
    /// malformed ref is `config_invalid` with the key path (`presets.<name>`, `presets.<name>.<letter>`).
    fn presets(&mut self) -> Result<BTreeMap<String, Preset>, CpdError> {
        let mut presets = BTreeMap::new();
        let Some(Value::Table(names)) = self.take("presets") else {
            return Ok(presets);
        };
        for (name, value) in names {
            if !is_preset_name(&name) {
                return Err(self.invalid(&format!("presets.{name}"), "is not a preset name `^[a-z][a-z0-9-]{0,31}$`"));
            }
            let Value::Table(letters) = value else {
                return Err(self.invalid(&format!("presets.{name}"), "must be a table of the four model refs O, S, H and G"));
            };
            for key in letters.keys() {
                if !matches!(key.as_str(), "O" | "S" | "H" | "G") {
                    return Err(self.invalid(&format!("presets.{name}.{key}"), "is not a known key"));
                }
            }
            for letter in ["O", "S", "H", "G"] {
                if !letters.contains_key(letter) {
                    return Err(self.invalid(&format!("presets.{name}"), format!("is missing the {letter} model ref")));
                }
            }
            let mut preset = Preset { o: String::new(), s: String::new(), h: String::new(), g: String::new() };
            for (letter, slot) in [("O", &mut preset.o), ("S", &mut preset.s), ("H", &mut preset.h), ("G", &mut preset.g)] {
                let value = letters.get(letter).expect("the four letters were checked above");
                let key = format!("presets.{name}.{letter}");
                let text = text_of(value).map_err(|why| self.invalid(&key, format!("is invalid: {why}")))?;
                *slot = preset_ref(text).map_err(|why| self.invalid(&key, format!("is invalid: {why}")))?;
                self.sources.insert(key, Source::File);
            }
            presets.insert(name, preset);
        }
        Ok(presets)
    }
}

const KNOWN_SECTIONS: [&str; 6] = ["timeouts", "watch", "inbox", "events", "tmux", "presets"];

/// A Lead's or the Manager's Tier: any letter, but never effort 5 (hs-d506).
fn leader_tier(value: &Value) -> Result<Tier, String> {
    let tier = Tier::parse(text_of(value)?).map_err(|e| e.message().to_owned())?;
    if tier.letter() == Letter::G {
        return Err("must not be a G Tier, not GX: a Lead or the Manager is never on G (hs-d562)".to_owned());
    }
    if !tier.is_lead_allowed() {
        return Err(format!("must have effort 0-4, not {tier}: no agent uses effort 5, to keep cost down (hs-d506)"));
    }
    Ok(tier)
}

fn text_of(value: &Value) -> Result<&str, String> {
    match value {
        Value::String(s) => Ok(s),
        other => Err(format!("must be text, not {}", other.type_str())),
    }
}

fn int(value: Value, min: u32, max: u32) -> Result<u32, String> {
    match value {
        Value::Integer(i) => match u32::try_from(i) {
            Ok(n) if (min..=max).contains(&n) => Ok(n),
            _ if max == u32::MAX && i > i64::from(max) => Err(format!("must be at most {max}, got {i}")),
            _ if max == u32::MAX => Err(format!("must be at least {min}, got {i}")),
            _ if min == max => Err(format!("must be {min}, got {i}")),
            _ => Err(format!("must be between {min} and {max}, got {i}")),
        },
        other => Err(format!("must be an integer, not {}", other.type_str())),
    }
}

fn non_empty(s: &str) -> Result<String, String> {
    if s.is_empty() { Err("must not contain an empty string".into()) } else { Ok(s.to_owned()) }
}

/// `^[a-z][a-z0-9-]{0,31}$` (contract §8.2): a preset name starts with a lowercase letter, then lowercase
/// letters, digits and `-`, at most 32 characters.
fn is_preset_name(name: &str) -> bool {
    let mut chars = name.chars();
    let first = chars.next();
    first.is_some_and(|c| c.is_ascii_lowercase())
        && name.chars().count() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A preset model ref (contract §8.2): §8.3's `<provider>/<id>` shape, 1–200 characters, no NUL.
fn preset_ref(s: &str) -> Result<String, String> {
    if s.is_empty() || s.chars().count() > 200 || s.contains('\0') {
        return Err(format!("{s:?} is not <provider>/<model-id> of 1 to 200 characters"));
    }
    crate::policy::model::model_ref(s)
}

/// `^[A-Za-z0-9_-]{1,32}$`.
fn tmux_name(s: &str) -> Result<String, String> {
    let ok = (1..=32).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok { Ok(s.to_owned()) } else { Err(format!("{s:?} is not 1-32 characters of A-Z a-z 0-9 _ -")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_lead_pi_packages_is_empty() {
        let (config, _) = Config::parse("version = 1\nmachine = \"hs\"\n", Path::new("/config.toml")).unwrap();
        assert!(config.lead_pi_packages.is_empty());
    }
}
