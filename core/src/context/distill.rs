//! `cpd context distill` (commands.md §3.12, contract §4.6 "The distill run";
//! hs-d194 D4, D5): builds a Context's digest with one-shot pi, validates it
//! ([`super::digest`]) and renames it into place.
//!
//! Started on demand, or detached by engine E9 (`watch/distill.rs`). The run
//! holds `locks/distill-<context>.lock`, a `flock(2)` lock the kernel frees when
//! the run dies, so a Context never has two runs and a crash leaves no stale lock.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use clap::Args;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::digest::{self, Digest, Invalid};
use super::render::Indexed;
use super::{ContextRow, Record, Select, inforce, open};
use crate::config::{Config, Letter};
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, Kind, NewEvent};
use crate::home::CpdHome;
use crate::host::pi;
use crate::ids::{ContextId, RecordId};
use crate::output::Output;
use crate::store::Store;

/// The `source` of the `error` Events of a failed run, and its `txns.command`.
const SOURCE: &str = "context distill";

/// The distiller's prompt (contract §4.6 step 3): versioned with the code that validates its answer.
const PROMPT: &str = include_str!("distill_prompt.md");

/// The repair turn's prompt (contract §4.6 step 5, hs-d244 D-c), shape B: the model writes only `## Additions`.
const REPAIR_PROMPT: &str = include_str!("distill_repair_prompt.md");

/// The most missing records a repair turn is tried for (contract §4.6 step 5): the measured range of
/// shape B is 1–16 (`docs/research/context-distill.md`); with more, the answer outgrows the digest's limit.
pub const REPAIR_MAX: usize = 16;

/// The `kind` of a `runs.jsonl` line: the run that wrote the digest, and the repair turn after an invalid one.
const KIND_DISTILL: &str = "distill";
const KIND_REPAIR: &str = "repair";

/// How many lines of pi's stderr a `distill_failed` Event carries.
const TAIL_LINES: usize = 60;

/// How often a run looks at pi while it waits.
const POLL: Duration = Duration::from_millis(50);

#[derive(Args, Debug)]
pub struct DistillArgs {
    #[command(flatten)]
    select: Select,
}

/// The lock of a Context's one distill run, held while this value lives.
#[derive(Debug)]
pub struct Lock {
    _file: File,
}

fn lock_failed(path: &Path, e: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("Cannot use the distill lock {}: {e}.", path.display()))
        .with("lock", path.display().to_string())
}

impl Lock {
    /// Takes the lock without waiting; `distill_running` (exit 3) when a run holds it.
    pub fn take(home: &CpdHome, context: &ContextId) -> Result<Lock, CpdError> {
        let path = home.distill_lock(context);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| lock_failed(&path, e))?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| lock_failed(&path, e))?;
        match file.try_lock() {
            Ok(()) => Ok(Lock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(CpdError::new(
                ErrorCode::DistillRunning,
                format!("A distill run of Context {context} is alive: there is one at a time per Context."),
            )
            .with("context", context.as_str())
            .with("lock", path.display().to_string())),
            Err(TryLockError::Error(e)) => Err(lock_failed(&path, e)),
        }
    }
}

/// Whether a run of the Context holds the lock (engine E9's probe): opens it
/// without creating it, so a Context that never had a run has none, and lets go at once.
pub fn running(home: &CpdHome, context: &ContextId) -> Result<bool, CpdError> {
    let path = home.distill_lock(context);
    let file = match OpenOptions::new().read(true).write(true).open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(lock_failed(&path, e)),
    };
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(e)) => Err(lock_failed(&path, e)),
    }
}

/// `context distill` (§3.12).
pub(super) fn distill(args: DistillArgs) -> Result<Output, CpdError> {
    let (home, config, mut store) = open()?;
    let context = args.select.resolve(&mut store)?;
    store.read(|conn| ContextRow::require(conn, &context))?;
    let _lock = Lock::take(&home, &context)?;
    let records = store.read(|conn| {
        let mut stmt =
            conn.prepare(&format!("SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq", Record::COLUMNS))?;
        Ok(stmt.query_map([context.as_str()], Record::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?)
    })?;
    let run = Run { home: &home, config: &config, store: &mut store, context: &context };
    run.run(&records).map(Output::success)
}

struct Run<'a> {
    home: &'a CpdHome,
    config: &'a Config,
    store: &'a mut Store,
    context: &'a ContextId,
}

fn digest_json(digest: Option<&Digest>) -> Value {
    digest.map_or(Value::Null, |d| json!({"upto_seq": d.upto_seq, "file": d.file, "bytes": d.text.len()}))
}

impl Run<'_> {
    /// Steps 2–5 of the run (contract §4.6), the lock held: `records` are every record of the Context, in `seq` order.
    fn run(mut self, records: &[Indexed]) -> Result<Value, CpdError> {
        let Some(&(highest, _)) = records.last() else { return Ok(self.already(None, 0)) };
        let upto = u64::try_from(highest).expect("a record's seq is positive");
        let in_force = inforce::in_force(records, highest);
        if in_force.is_empty() {
            return Ok(self.already(None, 0));
        }
        let latest = digest::latest(self.home, self.context)?;
        if latest.as_ref().is_some_and(|d| d.upto_seq >= upto) {
            return Ok(self.already(latest.as_ref(), in_force.len()));
        }
        let dir = self.home.context_digest_dir(self.context);
        fs::create_dir_all(&dir).map_err(|e| io_failed("create", &dir, &e))?;
        let tmp = self.home.context_digest_tmp(self.context, upto);
        // Only `plan` and `brief` records came since: the records in force are the same, so are the topic sections.
        let carried = latest.filter(|d| in_force.iter().all(|(seq, _)| u64::try_from(*seq).is_ok_and(|s| s <= d.upto_seq)));
        let ran = match &carried {
            Some(d) => {
                fs::write(&tmp, digest::topic_sections(&d.text)).map_err(|e| io_failed("write", &tmp, &e))?;
                None
            }
            None => {
                self.write_inputs(&dir, &in_force)?;
                let (ran, text) = self.run_pi(upto, KIND_DISTILL, &["prompt.md", "records.md"])?;
                fs::write(&tmp, text).map_err(|e| io_failed("write", &tmp, &e))?;
                Some(ran)
            }
        };
        let mut bytes = fs::read(&tmp).map_err(|e| io_failed("read", &tmp, &e))?;
        // The model writes no index (the prompt says so); one it wrote anyway is not a topic section and is dropped.
        if let Some(topic) = std::str::from_utf8(&bytes).ok().map(digest::topic_sections).filter(|t| t.len() < bytes.len()) {
            bytes = topic.as_bytes().to_vec();
            fs::write(&tmp, &bytes).map_err(|e| io_failed("write", &tmp, &e))?;
        }
        let expect = digest::Expect::of(&in_force);
        let mut repair = None;
        if let Err(invalid) = digest::validate(&bytes, &expect, digest::LIMIT) {
            if let Some(ran) = &ran {
                self.log_run(ran, upto, "invalid");
            }
            // One repair turn, only for a digest that lacks citations and nothing else.
            let repairable = ran.is_some()
                && !invalid.not_utf8
                && !invalid.too_big
                && invalid.stray.is_empty()
                && (1..=REPAIR_MAX).contains(&invalid.missing.len());
            if !repairable {
                let kept = digest::keep_invalid(self.home, self.context, upto)?;
                return Err(self.invalid(upto, &invalid, &kept));
            }
            let (fixed, text) = self.repair(&dir, upto, &bytes, &invalid.missing, &in_force)?;
            bytes.extend_from_slice(b"\n");
            bytes.extend_from_slice(text.as_bytes());
            fs::write(&tmp, &bytes).map_err(|e| io_failed("write", &tmp, &e))?;
            if let Err(invalid) = digest::validate(&bytes, &expect, digest::LIMIT) {
                self.log_run(&fixed, upto, "invalid");
                let kept = digest::keep_invalid(self.home, self.context, upto)?;
                return Err(self.invalid(upto, &invalid, &kept));
            }
            self.log_run(&fixed, upto, "valid");
            repair = Some(fixed);
        } else if let Some(ran) = &ran {
            self.log_run(ran, upto, "valid");
        }
        let file = digest::install(self.home, self.context, upto, &expect)?;
        let text = fs::read_to_string(&file).map_err(|e| io_failed("read", &file, &e))?;
        let installed = Digest { upto_seq: upto, file, text };
        let mut out = json!({
            "context": self.context.as_str(),
            "distilled": ran.is_some(),
            "digest": digest_json(Some(&installed)),
            "records_in_force": in_force.len(),
        });
        if let Some(ran) = &ran {
            out["run"] = ran.figures();
        }
        if let Some(repair) = &repair {
            out["repair"] = repair.figures();
        }
        Ok(out)
    }

    fn already(&self, digest: Option<&Digest>, in_force: usize) -> Value {
        json!({
            "context": self.context.as_str(),
            "already": true,
            "digest": digest_json(digest),
            "records_in_force": in_force,
        })
    }

    /// Step 3: `prompt.md` and `records.md`, the records in force oldest first.
    fn write_inputs(&self, dir: &Path, in_force: &[&Indexed]) -> Result<(), CpdError> {
        let prompt = dir.join("prompt.md");
        fs::write(&prompt, PROMPT).map_err(|e| io_failed("write", &prompt, &e))?;
        let records = dir.join("records.md");
        fs::write(&records, records_md(in_force)).map_err(|e| io_failed("write", &records, &e))
    }

    /// Step 5's repair turn (shape B): `digest.md` (the invalid topic sections), `missing.md` (the blocks of the
    /// missing records, as in `records.md`) and `problem.md` (their ids), then pi with the repair prompt. Returns
    /// the run and its answer, `## Additions`, which the caller appends to the topic sections.
    fn repair(&mut self, dir: &Path, upto: u64, topic: &[u8], missing: &[RecordId], in_force: &[&Indexed]) -> Result<(Ran, String), CpdError> {
        let blocks: Vec<&Indexed> = in_force.iter().filter(|(_, r)| missing.contains(&r.id)).copied().collect();
        let problem = format!(
            "The digest in `digest.md` does not cite these {} decisions and open questions in force, which are in `missing.md`:\n{}\n",
            missing.len(),
            missing.iter().map(|id| format!("- {id}")).collect::<Vec<_>>().join("\n")
        );
        for (name, content) in [
            ("repair-prompt.md", REPAIR_PROMPT.as_bytes().to_vec()),
            ("digest.md", topic.to_vec()),
            ("missing.md", records_md(&blocks).into_bytes()),
            ("problem.md", problem.into_bytes()),
        ] {
            let path = dir.join(name);
            fs::write(&path, content).map_err(|e| io_failed("write", &path, &e))?;
        }
        let (ran, text) = self.run_pi(upto, KIND_REPAIR, &["repair-prompt.md", "digest.md", "missing.md", "problem.md"])?;
        // As for a digest, an index the model wrote anyway is not an addition.
        Ok((ran, digest::topic_sections(&text).trim_end().to_owned() + "\n"))
    }

    /// Step 4: the distiller in JSON mode over the files `files` of the directory, its stderr to the log, its
    /// stdout read by cpd: the run and its final assistant text (ending in a newline) are returned. A failure
    /// removes the `.md.tmp` and appends the `error` Event; every run that started pi appends its line to
    /// `runs.jsonl`, a failed one here.
    fn run_pi(&mut self, upto: u64, kind: &'static str, files: &[&str]) -> Result<(Ran, String), CpdError> {
        let tier = self.config.context_distill_tier;
        let models = &self.config.tiers;
        let model = match tier.letter() {
            Letter::O => &models.o,
            Letter::S => &models.s,
            Letter::H => &models.h,
        };
        let model = format!("{model}:{}", tier.level());
        let launch = pi::distiller(&model, files);
        let dir = self.home.context_digest_dir(self.context);
        let log = log_path(self.home, self.context);
        let limit = Duration::from_secs(u64::from(self.config.timeouts.distill_minutes) * 60);
        let mut log_file = OpenOptions::new().create(true).append(true).open(&log).map_err(|e| io_failed("open", &log, &e))?;
        // The tail of pi's stderr is what this run appended after this point.
        let offset = log_file.metadata().map_err(|e| io_failed("read", &log, &e))?.len();
        let mut command = Command::new(&launch.argv[0]);
        command.args(&launch.argv[1..]);
        for (key, _) in std::env::vars_os().filter(|(k, _)| k.to_string_lossy().starts_with("CPD_")) {
            command.env_remove(key);
        }
        command
            .envs(&launch.env)
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(log_file.try_clone().map_err(|e| io_failed("use", &log, &e))?);
        let started_at = OffsetDateTime::now_utc().format(&Rfc3339).expect("a UTC time is RFC 3339");
        let started = Instant::now();
        let waited = wait_limited(&mut command, limit);
        let mut ran = Ran { started_at, seconds: (started.elapsed().as_secs_f64() * 1000.0).round() / 1000.0, model, kind, usage: None };
        let failure = match waited {
            Err(e) => Failed::Spawn(e.to_string()),
            Ok(Waited::TimedOut) => Failed::Timeout(limit),
            Ok(Waited::Exited { status, stdout }) => {
                let parsed = stdout.map_err(|e| format!("pi's output cannot be read: {e}")).and_then(|bytes| pi_json::parse(&bytes));
                match (status.success(), parsed) {
                    (true, Ok(output)) => {
                        ran.usage = output.usage;
                        match output.answer {
                            Ok(text) => {
                                let text = if text.ends_with('\n') { text } else { format!("{text}\n") };
                                return Ok((ran, text));
                            }
                            Err(why) => Failed::Output(why),
                        }
                    }
                    (true, Err(why)) => Failed::Output(why),
                    (false, parsed) => {
                        ran.usage = parsed.ok().and_then(|output| output.usage);
                        Failed::Exit(status)
                    }
                }
            }
        };
        let _ = log_file.flush();
        self.log_run(&ran, upto, "failed");
        digest::discard(self.home, self.context, upto)?;
        Err(self.pi_failed(upto, &failure, tail_of(&log, offset)))
    }

    /// Appends the run's line to `runs.jsonl` (contract §4.6 "The run log"). A write failure fails nothing: it
    /// goes to the log.
    fn log_run(&self, ran: &Ran, upto: u64, result: &str) {
        let mut line = ran.figures();
        line["started_at"] = json!(ran.started_at);
        line["context"] = json!(self.context.as_str());
        line["upto_seq"] = json!(upto);
        line["result"] = json!(result);
        line["kind"] = json!(ran.kind);
        let path = runs_path(self.home, self.context);
        let written = OpenOptions::new().create(true).append(true).open(&path).and_then(|mut file| writeln!(file, "{line}"));
        if let Err(e) = written {
            let note = format!("cannot append to {}: {e}", path.display());
            if let Ok(mut log) = OpenOptions::new().create(true).append(true).open(log_path(self.home, self.context)) {
                let _ = writeln!(log, "cpd: {note}");
            }
        }
    }

    /// Appends the run's one `error` Event and returns the error of the same code.
    fn fail(&mut self, error: CpdError, text: String) -> CpdError {
        let data = json!({
            "source": SOURCE,
            "code": error.code().as_str(),
            "message": error.message(),
            "details": error.details(),
        });
        // No `context`, `plan` or `session`: §5.3 would wake every Lead of the Context for it.
        match self.store.write(SOURCE, |tx| events::append(tx, NewEvent::new(Kind::Error, text, data))) {
            Ok(_) => error,
            Err(e) => e.with("while_reporting", error.message()),
        }
    }

    fn invalid(&mut self, upto: u64, invalid: &Invalid, kept: &Path) -> CpdError {
        let ids = |ids: &[RecordId]| ids.iter().map(RecordId::to_string).collect::<Vec<_>>();
        let mut why = Vec::new();
        if invalid.not_utf8 {
            why.push("it is not UTF-8".to_owned());
        }
        if invalid.too_big {
            why.push(format!("it is {} bytes, over the {} limit", invalid.bytes, invalid.limit));
        }
        if !invalid.missing.is_empty() {
            why.push(format!("its topic sections do not cite {}, decisions or open questions in force", ids(&invalid.missing).join(", ")));
        }
        if !invalid.stray.is_empty() {
            why.push(format!("it cites {}, which are not in force", ids(&invalid.stray).join(", ")));
        }
        let message = format!("The digest of Context {} up to record {upto} is invalid: {}.", self.context, why.join("; "));
        let error = CpdError::new(ErrorCode::DistillInvalid, message)
            .with("context", self.context.as_str())
            .with("upto_seq", upto)
            .with("bytes", invalid.bytes)
            .with("limit", invalid.limit)
            .with("missing", ids(&invalid.missing))
            .with("stray", ids(&invalid.stray))
            .with("invalid", kept.display().to_string())
            .with("log", log_path(self.home, self.context).display().to_string());
        let text = format!(
            "{} The previous digest stays; the invalid one is kept at {}. Run it again: cpd context distill --context {}",
            error.message(),
            kept.display(),
            self.context
        );
        self.fail(error, text)
    }

    fn pi_failed(&mut self, upto: u64, failure: &Failed, tail: String) -> CpdError {
        let (reason, exit, what) = match failure {
            Failed::Spawn(why) => ("spawn", None, format!("pi could not start: {why}")),
            Failed::Exit(status) => {
                let how = status.code().map_or_else(|| format!("was ended by {status}"), |c| format!("exited with status {c}"));
                ("exit", Some(status), format!("pi {how}"))
            }
            Failed::Timeout(limit) => ("timeout", None, format!("pi ran longer than {} minutes and was killed", limit.as_secs() / 60)),
            Failed::Output(why) => ("output", None, format!("pi gave no usable answer: {why}")),
        };
        let message = format!("The distiller of Context {} failed: {what}.", self.context);
        let mut error = CpdError::new(ErrorCode::DistillFailed, message)
            .with("context", self.context.as_str())
            .with("upto_seq", upto)
            .with("reason", reason)
            .with("tail", tail)
            .with("log", log_path(self.home, self.context).display().to_string());
        if let Some(status) = exit {
            error = error.with("exit", status.code());
        }
        let text = format!("{} The previous digest stays. Run it again: cpd context distill --context {}", error.message(), self.context);
        self.fail(error, text)
    }
}

/// `run/context-digests/<context>/distill.log`: every run's stderr, appended (contract §1.4).
pub fn log_path(home: &CpdHome, context: &ContextId) -> PathBuf {
    home.context_digest_dir(context).join("distill.log")
}

/// `run/context-digests/<context>/runs.jsonl`: one line per run that started pi, appended (contract §1.4, §4.6).
pub fn runs_path(home: &CpdHome, context: &ContextId) -> PathBuf {
    home.context_digest_dir(context).join("runs.jsonl")
}

fn io_failed(what: &str, path: &Path, e: &io::Error) -> CpdError {
    CpdError::new(ErrorCode::FileUnreadable, format!("Cannot {what} {}: {e}.", path.display())).with("path", path.display().to_string())
}

/// `records.md` (contract §4.6 step 3): one block per record, `[<id>] <kind> (<date>)`, `answers <question id>`
/// for an answer, then the full text. A superseding decision's `supersedes` is not shown: what it names is not in force.
fn records_md(in_force: &[&Indexed]) -> String {
    let mut out = String::new();
    for (_, record) in in_force {
        let date = record.at.get(..10).unwrap_or(&record.at);
        out.push_str(&format!("[{}] {} ({date})\n", record.id, record.kind().as_str()));
        if let Some(question) = record.question() {
            out.push_str(&format!("answers {question}\n"));
        }
        out.push_str(record.text.trim_end());
        out.push_str("\n\n");
    }
    out
}

/// Why pi failed.
enum Failed {
    Spawn(String),
    Exit(ExitStatus),
    Timeout(Duration),
    /// pi exited 0 but its JSON stream cannot be read or holds no usable final assistant text.
    Output(String),
}

/// What one pi run took and reported; the figures of its `runs.jsonl` line and of `run` in the command's output.
struct Ran {
    started_at: String,
    seconds: f64,
    /// The `<model>:<level>` given to pi.
    model: String,
    /// [`KIND_DISTILL`] or [`KIND_REPAIR`]: the `kind` of the run's `runs.jsonl` line.
    kind: &'static str,
    /// `None` when pi reported none (a spawn failure, a kill, a stream without an assistant message).
    usage: Option<pi_json::Usage>,
}

impl Ran {
    fn figures(&self) -> Value {
        let usage = self.usage.as_ref();
        json!({
            "seconds": self.seconds,
            "model": self.model,
            "tokens_in": usage.map(|u| u.tokens_in),
            "tokens_out": usage.map(|u| u.tokens_out),
            "cost": usage.map(|u| u.cost),
        })
    }
}

/// pi's `--mode json` stream (pi `docs/json.md`, `docs/message-types.md`), parsed once at the boundary.
mod pi_json {
    use serde::Deserialize;
    use serde_json::Value;

    /// What the assistant messages of a run reported, summed.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Usage {
        /// Every token read by the model: `input`, `cacheRead` and `cacheWrite` (pi reports them apart, and
        /// a cached prompt is nearly all of the `cacheRead` or `cacheWrite`).
        pub tokens_in: u64,
        /// `output`, which already includes `reasoning`.
        pub tokens_out: u64,
        /// `cost.total`, in the unit pi reports (US dollars).
        pub cost: f64,
    }

    /// What a stream says: the run's usage and its answer, or why there is none.
    #[derive(Debug, PartialEq)]
    pub struct Output {
        pub usage: Option<Usage>,
        pub answer: Result<String, String>,
    }

    #[derive(Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        kind: String,
        message: Option<Value>,
    }

    #[derive(Deserialize)]
    struct Assistant {
        content: Vec<Block>,
        usage: Raw,
        #[serde(rename = "stopReason")]
        stop_reason: String,
        #[serde(rename = "errorMessage")]
        error_message: Option<String>,
    }

    #[derive(Deserialize)]
    struct Block {
        #[serde(rename = "type")]
        kind: String,
        text: Option<String>,
    }

    #[derive(Deserialize)]
    struct Raw {
        input: u64,
        output: u64,
        #[serde(rename = "cacheRead")]
        cache_read: u64,
        #[serde(rename = "cacheWrite")]
        cache_write: u64,
        cost: Cost,
    }

    #[derive(Deserialize)]
    struct Cost {
        total: f64,
    }

    /// Reads the stream: strict JSONL (one object per LF-terminated line), where each assistant `message_end`
    /// is a finished message. The answer is the text of the last one, which must have stopped normally
    /// (`stop`: pi exits 0 for an `error`, `aborted` or `length` stop in this mode) and not be blank.
    /// `Err` when a line is not JSON or an assistant message lacks what pi's docs promise.
    pub fn parse(bytes: &[u8]) -> Result<Output, String> {
        let mut usage: Option<Usage> = None;
        let mut last: Option<Assistant> = None;
        for (n, line) in bytes.split(|b| *b == b'\n').enumerate() {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                continue;
            }
            let at = n + 1;
            let event: Line = serde_json::from_slice(line).map_err(|e| format!("line {at} of pi's output is not an event: {e}"))?;
            let Some(message) = event.message.filter(|m| event.kind == "message_end" && m["role"] == "assistant") else { continue };
            let message: Assistant = serde_json::from_value(message).map_err(|e| format!("the assistant message on line {at} is malformed: {e}"))?;
            let sum = usage.get_or_insert(Usage { tokens_in: 0, tokens_out: 0, cost: 0.0 });
            sum.tokens_in += message.usage.input + message.usage.cache_read + message.usage.cache_write;
            sum.tokens_out += message.usage.output;
            sum.cost += message.usage.cost.total;
            last = Some(message);
        }
        let answer = match last {
            None => Err("the stream has no assistant message".to_owned()),
            Some(m) if m.stop_reason != "stop" => {
                let why = m.error_message.map(|e| format!(": {e}")).unwrap_or_default();
                Err(format!("the final assistant message stopped with `{}`{why}", m.stop_reason))
            }
            Some(m) => {
                let text: String = m.content.iter().filter(|b| b.kind == "text").filter_map(|b| b.text.as_deref()).collect();
                if text.trim().is_empty() { Err("the final assistant message has no text".to_owned()) } else { Ok(text) }
            }
        };
        Ok(Output { usage, answer })
    }
}

/// What became of a child that was given a time limit.
#[derive(Debug)]
enum Waited {
    /// It ended; `stdout` is everything it printed (empty when its stdout was not piped).
    Exited { status: ExitStatus, stdout: io::Result<Vec<u8>> },
    TimedOut,
}

/// Runs `command` and waits at most `limit`, reading its piped stdout meanwhile so a full pipe never stalls
/// it; a child still alive then is killed and reaped.
fn wait_limited(command: &mut Command, limit: Duration) -> io::Result<Waited> {
    let mut child = command.spawn()?;
    let reader = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            out.read_to_end(&mut bytes).map(|_| bytes)
        })
    });
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait()? {
            let stdout = reader.map_or(Ok(Vec::new()), |r| r.join().expect("the reader of pi's output does not panic"));
            return Ok(Waited::Exited { status, stdout });
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Ok(Waited::TimedOut);
        }
        std::thread::sleep(POLL);
    }
}

/// The last [`TAIL_LINES`] lines of `log` after byte `offset`.
fn tail_of(log: &Path, offset: u64) -> String {
    let bytes = fs::read(log).unwrap_or_default();
    let appended = bytes.get(usize::try_from(offset).unwrap_or(usize::MAX)..).unwrap_or_default();
    let text = String::from_utf8_lossy(appended);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{By, Draft};

    fn record(id: &str, at: &str, text: &str, draft: Draft) -> Indexed {
        (1, Record { id: RecordId::parse(id).unwrap(), at: at.into(), by: By { session: None, plan: None }, text: text.into(), draft })
    }

    #[test]
    fn records_md_gives_one_block_per_record_with_its_full_text() {
        let q = record("hs-q1", "2026-10-01T10:00:00.000Z", "Which port?", Draft::Question);
        let a = record(
            "hs-a1",
            "2026-10-02T10:00:00.000Z",
            "Port 80.\nSecond line.\n",
            Draft::Answer { question: RecordId::parse("hs-q1").unwrap() },
        );
        let d = record("hs-d2", "2026-10-03T10:00:00.000Z", "Use it.", Draft::Decision { supersedes: Some(RecordId::parse("hs-d1").unwrap()) });
        assert_eq!(
            records_md(&[&q, &a, &d]),
            "[hs-q1] question (2026-10-01)\nWhich port?\n\n\
             [hs-a1] answer (2026-10-02)\nanswers hs-q1\nPort 80.\nSecond line.\n\n\
             [hs-d2] decision (2026-10-03)\nUse it.\n\n"
        );
    }

    fn assistant(text: &str, stop: &str, input: u64, cache_read: u64, cache_write: u64, output: u64, cost: f64) -> String {
        json!({"type": "message_end", "message": {
            "role": "assistant",
            "content": [{"type": "thinking", "thinking": "hm"}, {"type": "text", "text": text}],
            "usage": {"input": input, "output": output, "cacheRead": cache_read, "cacheWrite": cache_write, "reasoning": 1,
                      "totalTokens": 0, "cost": {"total": cost}},
            "stopReason": stop,
        }})
        .to_string()
    }

    #[test]
    fn the_stream_gives_the_final_assistant_text_and_the_summed_usage() {
        let user = json!({"type": "message_end", "message": {"role": "user", "content": "x"}});
        let stream = format!(
            "{{\"type\":\"session\"}}\n{user}\n\n{}\r\n{{\"type\":\"turn_end\",\"message\":{{\"role\":\"assistant\"}}}}\n{}\n",
            assistant("first", "stop", 1, 2, 3, 4, 0.5),
            assistant("# Digest\n[hs-d1]", "stop", 10, 0, 100, 20, 0.25)
        );
        let out = pi_json::parse(stream.as_bytes()).unwrap();
        assert_eq!(out.answer, Ok("# Digest\n[hs-d1]".to_owned()));
        assert_eq!(out.usage, Some(pi_json::Usage { tokens_in: 116, tokens_out: 24, cost: 0.75 }));
    }

    #[test]
    fn a_stream_without_a_usable_answer_still_reports_its_usage() {
        let out = pi_json::parse(assistant("cut", "length", 1, 0, 0, 2, 0.1).as_bytes()).unwrap();
        assert_eq!(out.usage, Some(pi_json::Usage { tokens_in: 1, tokens_out: 2, cost: 0.1 }));
        assert!(out.answer.unwrap_err().contains("`length`"));
        assert!(pi_json::parse(assistant(" \n", "stop", 1, 0, 0, 2, 0.1).as_bytes()).unwrap().answer.is_err());
        let none = pi_json::parse(b"{\"type\":\"agent_start\"}\n").unwrap();
        assert_eq!((none.usage, none.answer.is_err()), (None, true));
    }

    #[test]
    fn a_stream_that_is_not_pi_json_cannot_be_parsed() {
        assert!(pi_json::parse(b"# Digest\n").unwrap_err().contains("line 1"));
        assert!(pi_json::parse(b"[1]\n").is_err());
        let no_usage = r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"stop"}}"#;
        assert!(pi_json::parse(no_usage.as_bytes()).unwrap_err().contains("malformed"));
    }

    #[test]
    fn a_child_that_outlives_its_limit_is_killed() {
        let started = Instant::now();
        let waited = wait_limited(Command::new("sleep").arg("30"), Duration::from_millis(200)).unwrap();
        assert!(matches!(waited, Waited::TimedOut), "{waited:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_child_that_ends_in_time_reports_its_status() {
        let Waited::Exited { status, .. } = wait_limited(Command::new("sh").args(["-c", "exit 3"]), Duration::from_secs(30)).unwrap() else {
            panic!("it ended in time")
        };
        assert_eq!(status.code(), Some(3));
        assert!(wait_limited(&mut Command::new("/nonexistent/pi"), Duration::from_secs(1)).is_err());
    }

    #[test]
    fn the_tail_is_what_the_run_appended_after_the_offset_and_at_most_60_lines() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("distill.log");
        let old = "earlier run\n";
        let lines: String = (1..=100).map(|i| format!("line {i}\n")).collect();
        fs::write(&log, format!("{old}{lines}")).unwrap();
        let tail = tail_of(&log, old.len() as u64);
        assert_eq!(tail.lines().count(), 60);
        assert!(tail.starts_with("line 41\n") && tail.ends_with("line 100"), "{tail}");
        assert_eq!(tail_of(&log, 1 << 40), "");
    }
}
