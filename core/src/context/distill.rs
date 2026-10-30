//! `cpd context distill` (commands.md §3.12, contract §4.6 "The distill run";
//! hs-d549, hs-d551): curates a batch of at most 60 uncurated records with one-shot pi, checks its JSON lines
//! and writes the curations and proposals with [`propose`].
//!
//! Started on demand, or detached by engine E9 (`watch/distill.rs`). The run
//! holds `locks/distill-<context>.lock`, a `flock(2)` lock the kernel frees when
//! the run dies, so a Context never has two runs and a crash leaves no stale lock.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use clap::Args;
use serde::Deserialize;
use serde_json::{Value, json};

use super::curation::{Curation, NotCurable, not_curable, Proposal, Reason, propose_rejecting};
use super::render::Indexed;
use super::{ContextRow, Record, Select, inforce};
use crate::config::{Config, Letter, Tier};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, NewEvent};
use crate::home::CpdHome;
use crate::host::deadline::Failed as RunFailed;
use crate::host::pi;
use crate::host::deadline;
use crate::host::runner::{Call, Runner};
use crate::ids::{ContextId, RecordId};
use crate::lock;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;

/// The `source` of the `error` Events of a failed run, and its `txns.command`.
const SOURCE: &str = "context distill";

/// The distiller's prompt (contract §4.6 step 3): versioned with the code that validates its answer.
const PROMPT: &str = include_str!("distill_prompt.md");

/// How many lines of pi's stderr a `distill_failed` Event carries.
const TAIL_LINES: usize = 60;

#[derive(Args, Debug)]
pub struct DistillArgs {
    #[command(flatten)]
    select: Select,
    /// The most records to curate in this run
    #[arg(long, value_name = "n", default_value_t = BATCH_MAX, value_parser = clap::value_parser!(u8).range(1..=60))]
    batch: u8,
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
        if lock::try_lock(&file).map_err(|e| lock_failed(&path, e))? {
            return Ok(Lock { _file: file });
        }
        Err(CpdError::new(
            ErrorCode::DistillRunning,
            format!("A distill run of Context {context} is alive: there is one at a time per Context."),
        )
        .with("context", context.as_str())
        .with("lock", path.display().to_string()))
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
    // The probe's lock is let go when `file` drops.
    let free = lock::try_lock(&file).map_err(|e| lock_failed(&path, e))?;
    Ok(!free)
}
/// `context distill` (§3.12).
pub(super) fn distill(args: DistillArgs) -> Result<Output, CpdError> {
    let Env { home, config, mut store, ports, .. } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    store.read(|conn| ContextRow::require(conn, &context))?;
    let floor = config.context_curation_min_records;
    match store.read(|conn| not_curable(conn, &context, floor))? {
        Some(NotCurable::Archived) => return Ok(Output::success(json!({"context": context.as_str(), "skipped": "archived"}))),
        Some(NotCurable::Young { records }) => {
            return Ok(Output::success(json!({"context": context.as_str(), "skipped": "young", "records": records, "floor": floor})));
        }
        Some(NotCurable::Deleted) | None => {}
    }
    let _lock = Lock::take(&home, &context)?;
    let state = store.read(|conn| State::read(conn, &context))?;
    let run = Run { home: &home, config: &config, store: &mut store, context: &context, ports: &ports };
    run.run(&state, usize::from(args.batch)).map(Output::success)
}

/// The most records one step curates.
pub const BATCH_MAX: u8 = 60;

/// The most bytes of record text one batch holds (the first record is always taken).
const BATCH_BYTES: usize = 49152;

/// The bounds of a curation line (contract §4.6 step 5).
const TOPIC_MAX: usize = 40;
const SHORT_MAX: usize = 120;

/// What a run reads in one transaction: every record of a Context, the records out of force, and the curations.
pub struct State {
    records: Vec<Indexed>,
    retired: HashSet<RecordId>,
    /// `record id -> (topic, short)` of every curated record.
    curated: HashMap<RecordId, (String, String)>,
}

impl State {
    pub fn read(conn: &rusqlite::Connection, context: &ContextId) -> Result<State, CpdError> {
        let mut stmt =
            conn.prepare(&format!("SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq", Record::COLUMNS))?;
        let records = stmt.query_map([context.as_str()], Record::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut stmt = conn.prepare("SELECT record, topic, short FROM record_curations WHERE context = ?1")?;
        let rows = stmt
            .query_map([context.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let curated = rows.into_iter().map(|(id, topic, short)| Ok((RecordId::parse(&id)?, (topic, short)))).collect::<Result<_, CpdError>>()?;
        Ok(State { records, retired: inforce::retired(conn, context)?, curated })
    }

    /// The highest `seq` of the Context, `None` when it has no record.
    pub fn highest(&self) -> Option<i64> {
        self.records.last().map(|(seq, _)| *seq)
    }

    /// The ids of the records in force.
    pub fn in_force_ids(&self) -> HashSet<String> {
        self.in_force().iter().map(|(_, r)| r.id.to_string()).collect()
    }

    fn in_force(&self) -> Vec<&Indexed> {
        self.highest().map_or_else(Vec::new, |h| inforce::in_force_without(&self.records, h, &self.retired))
    }

    /// The records in force that are not curated yet, oldest first.
    pub fn uncurated(&self) -> Vec<&Indexed> {
        self.in_force().into_iter().filter(|(_, r)| !self.curated.contains_key(&r.id)).collect()
    }
}

struct Run<'a> {
    home: &'a CpdHome,
    config: &'a Config,
    store: &'a mut Store,
    context: &'a ContextId,
    ports: &'a Ports,
}

/// One problem of an output: `{id?, line?, what}`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Problem {
    pub(super) id: Option<RecordId>,
    pub(super) line: Option<usize>,
    pub(super) what: String,
}

impl Problem {
    fn json(&self) -> Value {
        let mut v = json!({"what": self.what});
        if let Some(id) = &self.id {
            v["id"] = json!(id.to_string());
        }
        if let Some(line) = self.line {
            v["line"] = json!(line);
        }
        v
    }
}

/// How a retirement proposal of the model reads.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRetire {
    reason: RawReason,
    target: String,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum RawReason {
    Duplicate,
    Superseded,
    Documented,
}

/// One line of the model's output, as the schema of contract §4.6 step 4 has it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLine {
    id: String,
    topic: String,
    short: String,
    retire: Option<RawRetire>,
    doc: bool,
}

/// A checked output (GXD1): what [`propose_rejecting`] takes, and the lines it rejects.
pub(super) struct Checked {
    pub(super) curations: Vec<Curation>,
    pub(super) proposals: Vec<Proposal>,
    pub(super) rejected: Vec<Problem>,
}

/// The batch of a step with the data the checks need.
pub(super) struct Batch<'a> {
    pub(super) records: &'a [&'a Indexed],
    /// The ids and `seq`s of every record in force of the Context (the batch and the known ones).
    pub(super) in_force: HashMap<RecordId, i64>,
    /// `docs.md`'s hits of each batch record.
    pub(super) hits: HashMap<RecordId, Vec<String>>,
}

impl Batch<'_> {
    /// Checks `text` (contract §4.6 step 5) line by line (GXD1): a line that passes is accepted, one that fails is
    /// rejected and its record stays uncurated. A record missing from the output stays uncurated without a
    /// rejection; a line whose id is not a record of the batch is ignored; both lines of a repeated id are rejected.
    pub(super) fn check(&self, text: &str) -> Checked {
        let mut checked = Checked { curations: Vec::new(), proposals: Vec::new(), rejected: Vec::new() };
        let batch: HashMap<&RecordId, i64> = self.records.iter().map(|(seq, r)| (&r.id, *seq)).collect();
        let lines: Vec<(usize, &str)> = text.lines().enumerate().map(|(i, l)| (i + 1, l)).filter(|(_, l)| !l.trim().is_empty()).collect();
        let mut parsed: Vec<(usize, Result<(RawLine, RecordId), String>)> = Vec::new();
        for (n, line) in lines {
            match serde_json::from_str::<RawLine>(line) {
                Err(e) => parsed.push((n, Err(format!("not a curation line: {e}")))),
                // An id that is not a record of the batch is ignored.
                Ok(raw) => match RecordId::parse(&raw.id) {
                    Ok(id) if batch.contains_key(&id) => parsed.push((n, Ok((raw, id)))),
                    _ => {}
                },
            }
        }
        let mut count: HashMap<RecordId, usize> = HashMap::new();
        for (_, id) in parsed.iter().filter_map(|(_, p)| p.as_ref().ok()) {
            *count.entry(id.clone()).or_default() += 1;
        }
        for (n, line) in parsed {
            let problem = |id: Option<&RecordId>, what: String| Problem { id: id.cloned(), line: Some(n), what };
            let (raw, id) = match line {
                Ok(parsed) => parsed,
                Err(what) => {
                    checked.rejected.push(problem(None, what));
                    continue;
                }
            };
            if count[&id] > 1 {
                checked.rejected.push(problem(Some(&id), "listed more than once".to_owned()));
                continue;
            }
            let mut bad = Vec::new();
            if raw.topic.is_empty() || raw.topic.len() > TOPIC_MAX || !raw.topic.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-')) {
                bad.push(format!("topic {:?} must be 1 to {TOPIC_MAX} characters of lowercase letters, digits and `-`", raw.topic));
            }
            if raw.short.is_empty() || raw.short.len() > SHORT_MAX || raw.short.contains('\n') {
                bad.push(format!("short must be 1 to {SHORT_MAX} bytes on one line"));
            }
            let seq = batch[&id];
            let mut proposal = None;
            if let Some(retire) = &raw.retire {
                match self.retire(&id, seq, retire) {
                    Ok(p) => proposal = Some(p),
                    Err(what) => bad.push(what),
                }
            }
            if !bad.is_empty() {
                checked.rejected.extend(bad.into_iter().map(|what| problem(Some(&id), what)));
                continue;
            }
            checked.curations.push(Curation {
                record: id.clone(),
                topic: raw.topic,
                short: raw.short,
                doc_paths: raw.doc.then(|| self.hits.get(&id).cloned().unwrap_or_default()),
            });
            checked.proposals.extend(proposal);
        }
        checked
    }

    fn retire(&self, id: &RecordId, seq: i64, retire: &RawRetire) -> Result<Proposal, String> {
        let reason = match retire.reason {
            RawReason::Duplicate => Reason::Duplicate,
            RawReason::Superseded => Reason::Superseded,
            RawReason::Documented => Reason::Documented,
        };
        match reason {
            Reason::Documented => {
                if !self.hits.get(id).is_some_and(|h| h.contains(&retire.target)) {
                    return Err(format!("documented target {:?} is not one of the record's grep hits", retire.target));
                }
            }
            Reason::Duplicate | Reason::Superseded => {
                let target = RecordId::parse(&retire.target).map_err(|_| format!("{} target {:?} is not a record id", reason.as_str(), retire.target))?;
                let Some(&target_seq) = self.in_force.get(&target) else {
                    return Err(format!("{} target {target} is not a record in force of the Context", reason.as_str()));
                };
                if &target == id {
                    return Err(format!("{} target is the record itself", reason.as_str()));
                }
                if reason == Reason::Superseded && target_seq <= seq {
                    return Err(format!("superseded target {target} is not newer than the record"));
                }
                if reason == Reason::Duplicate && target_seq >= seq {
                    return Err(format!("duplicate target {target} is not older than {id}: keep the first, retire the later"));
                }
            }
        }
        Ok(Proposal { record: id.clone(), reason, target: retire.target.clone() })
    }
}

impl Run<'_> {
    /// The run (contract §4.6 "The distill run"), the lock held.
    fn run(mut self, state: &State, batch_size: usize) -> Result<Value, CpdError> {
        let uncurated = state.uncurated();
        let Some(highest) = state.highest().filter(|_| !uncurated.is_empty()) else {
            return Ok(json!({"context": self.context.as_str(), "already": true, "remaining": 0}));
        };
        let upto = u64::try_from(highest).expect("a record's seq is positive");
        let mut bytes = 0;
        let batch_records: Vec<&Indexed> = uncurated
            .iter()
            .take(batch_size)
            .enumerate()
            .take_while(|(i, (_, r))| {
                bytes += r.text.len();
                *i == 0 || bytes <= BATCH_BYTES
            })
            .map(|(_, record)| *record)
            .collect();
        let in_force = state.in_force();
        let hits = self.grep_hits(&batch_records)?;
        let dir = self.home.context_digest_dir(self.context);
        fs::create_dir_all(&dir).map_err(|e| io_failed("create", &dir, &e))?;
        self.write_inputs(&dir, &batch_records, &in_force, state, &hits)?;
        let batch = Batch { records: &batch_records, in_force: in_force.iter().map(|(seq, r)| (r.id.clone(), *seq)).collect(), hits };
        let start = self.config.context_distill_tier;
        let steps: Vec<Tier> = vec![start];
        let (ran, checked, text) = match self.step(start, upto, &batch)? {
            Ok(done) => done,
            Err(Fault::Invalid(problems, kept)) => return Err(self.invalid(upto, &problems, &kept)),
            Err(Fault::Pi(failure, tail)) => return Err(self.pi_failed(upto, &failure, tail)),
        };
        let (curated, docs) = (checked.curations.len(), checked.curations.iter().filter(|c| c.doc_paths.is_some()).count());
        let retire = propose_rejecting(self.store, self.context, &checked.curations, &checked.proposals, checked.rejected.len())?;
        remove_invalid(self.home, self.context)?;
        if !checked.rejected.is_empty() || ran.cut {
            let kept = self.home.context_digest_invalid(self.context, upto);
            fs::write(&kept, &text).map_err(|e| io_failed("write", &kept, &e))?;
        }
        self.log_run(&ran, upto, "valid", &checked.rejected);
        Ok(json!({
            "context": self.context.as_str(),
            "distilled": true,
            "steps": steps.iter().map(Tier::to_string).collect::<Vec<_>>(),
            "curated": curated,
            "rejected": rejected_json(&checked.rejected),
            "retire": retire,
            "docs": docs,
            "remaining": uncurated.len() - curated,
            "run": ran.figures(),
        }))
    }

    /// One step (contract §4.6 steps 4 and 5) at `tier`: the distiller, then the checks. The outer `Err` is a failure
    /// that ends the run at once (a file); the inner one is the step's fault, which ends the run (hs-d560).
    fn step(&mut self, tier: Tier, upto: u64, batch: &Batch<'_>) -> Result<Result<(Ran, Checked, String), Fault>, CpdError> {
        let (ran, text) = match self.run_pi(tier, upto, &["prompt.md", "batch.md", "known.md", "docs.md"])? {
            Ok(done) => done,
            Err(fault) => return Ok(Err(fault)),
        };
        let mut checked = batch.check(&text);
        if !checked.curations.is_empty() {
            return Ok(Ok((ran, checked, text)));
        }
        // No line accepted (GXD1): the run is invalid, and its rejections are all of its lines.
        if checked.rejected.is_empty() {
            checked.rejected.push(Problem { id: None, line: None, what: "the output has no curation line".to_owned() });
        }
        self.log_run(&ran, upto, "invalid", &checked.rejected);
        let kept = self.home.context_digest_invalid(self.context, upto);
        remove_invalid(self.home, self.context)?;
        fs::write(&kept, &text).map_err(|e| io_failed("write", &kept, &e))?;
        Ok(Err(Fault::Invalid(checked.rejected, kept)))
    }

    /// For each batch record, the `<repo>:<path>` hits of `git grep -l -w <id> -- docs '*.md'` on each repo of the
    /// Context's Plans' PRs.
    fn grep_hits(&mut self, batch: &[&Indexed]) -> Result<HashMap<RecordId, Vec<String>>, CpdError> {
        let repos: Vec<String> = self.store.read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT prs.repo FROM prs JOIN plans ON plans.id = prs.plan WHERE plans.context = ?1 ORDER BY prs.repo",
            )?;
            Ok(stmt.query_map([self.context.as_str()], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        grep_hits_with(&*Ports::current().runner, &repos, batch)
    }

    /// Step 3: `prompt.md`, `batch.md`, `known.md` and `docs.md`.
    fn write_inputs(
        &self,
        dir: &Path,
        batch: &[&Indexed],
        in_force: &[&Indexed],
        state: &State,
        hits: &HashMap<RecordId, Vec<String>>,
    ) -> Result<(), CpdError> {
        let mut known: Vec<(&str, String)> = Vec::new();
        for (_, record) in in_force {
            if let Some((topic, short)) = state.curated.get(&record.id) {
                known.push((topic, format!("[{}] {topic} | {short}\n", record.id)));
            }
        }
        let topics: BTreeSet<&str> = known.iter().map(|(t, _)| *t).collect();
        let mut known_md: String = topics.iter().map(|t| format!("{t}\n")).collect();
        known_md.push('\n');
        known_md.extend(known.iter().map(|(_, line)| line.as_str()));
        let mut docs_md = String::new();
        for (_, record) in batch {
            docs_md.push_str(&format!("[{}]\n", record.id));
            for hit in &hits[&record.id] {
                docs_md.push_str(hit);
                docs_md.push('\n');
            }
            docs_md.push('\n');
        }
        for (name, content) in [("prompt.md", PROMPT.to_owned()), ("batch.md", records_md(batch)), ("known.md", known_md), ("docs.md", docs_md)] {
            let path = dir.join(name);
            fs::write(&path, content).map_err(|e| io_failed("write", &path, &e))?;
        }
        Ok(())
    }

    /// Step 4: the distiller in JSON mode over the files `files` of the directory, its stderr to the log, its
    /// stdout read by cpd: the run and its final assistant text (ending in a newline) are returned. A failure
    /// appends the `error` Event upstream; every run that started pi appends its line to `runs.jsonl`, a failed one here.
    fn run_pi(&mut self, tier: Tier, upto: u64, files: &[&str]) -> Result<Result<(Ran, String), Fault>, CpdError> {
        let (home, context) = (self.home, self.context);
        let models = self.store.read(|conn| crate::policy::model::models_of_context(conn, home, context.as_str()))?;
        let model = match tier.letter() {
            Letter::O => &models.o,
            Letter::S => &models.s,
            Letter::H => &models.h,
            Letter::G => &models.g,
        };
        let model = format!("{model}:{}", tier.level());
        let launch = pi::distiller(&model, files);
        let dir = self.home.context_digest_dir(self.context);
        let log = log_path(self.home, self.context);
        let limit = Duration::from_secs(u64::from(self.config.timeouts.distill_minutes) * 60);
        let mut log_file = OpenOptions::new().create(true).append(true).open(&log).map_err(|e| io_failed("open", &log, &e))?;
        // The tail of pi's stderr is what this run appended after this point.
        let offset = log_file.metadata().map_err(|e| io_failed("read", &log, &e))?.len();
        let environ: Vec<String> = std::env::vars_os().map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        let timed = run_pi_timed_with(self.ports, &launch, &dir, &log, limit, &environ);
        let (started_at, seconds, waited) = match timed {
            Ok(Timed { started_at, seconds, waited }) => (started_at, seconds, Ok(waited)),
            Err(e) => (crate::clock::timestamp(self.ports.clock.utc()), 0.0, Err(e)),
        };
        let mut ran = Ran {
            started_at,
            seconds,
            model,
            tier,
            usage: None,
            cut: false,
        };
        let failure = match waited {
            Err(e) => Failed::Spawn(e.to_string()),
            Ok(Waited::TimedOut) => Failed::Timeout(limit),
            Ok(Waited::Exited { status, stdout }) => {
                let parsed = stdout.map_err(|e| format!("pi's output cannot be read: {e}")).and_then(|bytes| pi_json::parse(&bytes));
                match (status.success(), parsed) {
                    (true, Ok(output)) => {
                        ran.usage = output.usage;
                        ran.cut = output.cut;
                        match output.answer {
                            Ok(text) => {
                                let text = if text.ends_with('\n') { text } else { format!("{text}\n") };
                                return Ok(Ok((ran, text)));
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
        self.log_run(&ran, upto, "failed", &[]);
        Ok(Err(Fault::Pi(failure, tail_of(&log, offset))))
    }

    /// Appends the run's line to `runs.jsonl` (contract §4.6 "The run log"). A write failure fails nothing: it
    /// goes to the log.
    fn log_run(&self, ran: &Ran, upto: u64, result: &str, rejected: &[Problem]) {
        let mut line = ran.figures();
        line["started_at"] = json!(ran.started_at);
        line["context"] = json!(self.context.as_str());
        line["upto_seq"] = json!(upto);
        line["result"] = json!(result);
        line["rejected"] = rejected_json(rejected);
        line["kind"] = json!("distill");
        line["tier"] = json!(ran.tier.to_string());
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
        let event = NewEvent::error(text, SOURCE, error.code().as_str(), error.message(), error.details());
        // No `context`, `plan` or `session`: §5.3 would wake every Lead of the Context for it.
        match self.store.write(SOURCE, |tx| events::append(tx, event)) {
            Ok(_) => error,
            Err(e) => e.with("while_reporting", error.message()),
        }
    }

    fn invalid(&mut self, upto: u64, problems: &[Problem], kept: &Path) -> CpdError {
        let listed: Vec<String> = problems.iter().take(5).map(|p| p.id.as_ref().map_or_else(|| p.what.clone(), |id| format!("{id}: {}", p.what))).collect();
        let more = if problems.len() > 5 { format!(" (and {} more)", problems.len() - 5) } else { String::new() };
        let message = format!("The curation of Context {} up to record {upto} is invalid: {}{more}.", self.context, listed.join("; "));
        let error = CpdError::new(ErrorCode::DistillInvalid, message)
            .with("context", self.context.as_str())
            .with("upto_seq", upto)
            .with("errors", problems.iter().map(Problem::json).collect::<Vec<_>>())
            .with("invalid", kept.display().to_string())
            .with("log", log_path(self.home, self.context).display().to_string());
        let text = format!(
            "{} The invalid output is kept at {}. Run it again: cpd context distill --context {}",
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
        let text = format!("{} Run it again: cpd context distill --context {}", error.message(), self.context);
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

/// GXD2: the rejected lines as `[{"id": <id or null>, "line": <n>, "what": <why>}]`.
fn rejected_json(rejected: &[Problem]) -> Value {
    Value::Array(rejected.iter().map(|p| json!({"id": p.id.as_ref().map(ToString::to_string), "line": p.line, "what": p.what})).collect())
}

/// For each record of `batch`, the `<repo>:<path>` hits of `git -C <repo> grep -l -w <id> -- docs '*.md'` on each of
/// `repos`, in order, through the [`Runner`]. Exit 1 is no hit; any other failure is `git_failed`.
pub(super) fn grep_hits_with(runner: &dyn Runner, repos: &[String], batch: &[&Indexed]) -> Result<HashMap<RecordId, Vec<String>>, CpdError> {
    let mut hits = HashMap::new();
    for (_, record) in batch {
        let mut found = Vec::new();
        for repo in repos {
            let mut cmd = Command::new(GIT);
            cmd.args(["-C", repo, "grep", "-l", "-w", &record.id.to_string(), "--", "docs", "*.md"]).stdin(Stdio::null());
            let out = runner.output(&mut cmd, "git", GREP_DEADLINE).map_err(|e| match e {
                RunFailed::Io(e) => git_failed(repo, &e.to_string()),
                RunFailed::Timeout(e) => e,
            })?;
            // 1: no match.
            if !out.status.success() && out.status.code() != Some(1) {
                return Err(git_failed(repo, String::from_utf8_lossy(&out.stderr).trim()));
            }
            found.extend(String::from_utf8_lossy(&out.stdout).lines().map(|path| format!("{repo}:{path}")));
        }
        hits.insert(record.id.clone(), found);
    }
    Ok(hits)
}

/// The program of the grep; the runner picks the real or the fake one.
const GIT: &str = "git";

/// How long one `git grep` of a repo may take.
const GREP_DEADLINE: Duration = Duration::from_secs(60);

fn git_failed(repo: &str, why: &str) -> CpdError {
    CpdError::new(ErrorCode::GitFailed, format!("Cannot search {repo} for the record ids: {why}.")).with("repo", repo)
}

/// Removes the kept invalid outputs of the Context (`<upto_seq>.jsonl.invalid`).
fn remove_invalid(home: &CpdHome, context: &ContextId) -> Result<(), CpdError> {
    let dir = home.context_digest_dir(context);
    let Ok(entries) = fs::read_dir(&dir) else { return Ok(()) };
    for entry in entries.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl.invalid")) {
        fs::remove_file(entry.path()).map_err(|e| io_failed("remove", &entry.path(), &e))?;
    }
    Ok(())
}

fn io_failed(what: &str, path: &Path, e: &io::Error) -> CpdError {
    CpdError::new(ErrorCode::FileUnreadable, format!("Cannot {what} {}: {e}.", path.display())).with("path", path.display().to_string())
}

/// `records.md` (contract §4.6 step 3): one block per record, `[<id>] <kind> (<date>)`, `answers <question id>`
/// for an answer, then the full text. A superseding decision's `supersedes` is not shown: what it names is not in force.
pub(super) fn records_md(in_force: &[&Indexed]) -> String {
    let mut out = String::new();
    for (_, record) in in_force {
        let date = format!("{} UTC", record.at.get(..10).unwrap_or(&record.at));
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

/// Why the distill step failed; it becomes the run's error Event.
enum Fault {
    /// The step's problems and the file that keeps its output.
    Invalid(Vec<Problem>, PathBuf),
    Pi(Failed, String),
}

/// What one pi run took and reported; the figures of its `runs.jsonl` line and of `run` in the command's output.
struct Ran {
    started_at: String,
    seconds: f64,
    /// The `<model>:<level>` given to pi.
    model: String,
    /// The Tier of the pi run.
    tier: Tier,
    /// `None` when pi reported none (a spawn failure, a kill, a stream without an assistant message).
    usage: Option<pi_json::Usage>,
    /// GXD3: pi's last message stopped with `length`, and the answer is its complete lines.
    cut: bool,
}

impl Ran {
    fn figures(&self) -> Value {
        let usage = self.usage.as_ref();
        let mut figures = json!({
            "seconds": self.seconds,
            "model": self.model,
            "tokens_in": usage.map(|u| u.tokens_in),
            "tokens_out": usage.map(|u| u.tokens_out),
            "cost": usage.map(|u| u.cost),
        });
        if self.cut {
            figures["cut"] = json!(true);
        }
        figures
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
        /// GXD3: the last message stopped with `length` and the answer is its text up to its last newline.
        pub cut: bool,
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

    /// The text of a `length` stop up to its last newline (GXD3), and without its final line when that is not a
    /// complete JSON object: that is the cut line, which is dropped even when a newline follows it. `None` when no
    /// line is left.
    fn complete_lines(text: &str) -> Option<String> {
        let mut lines: Vec<&str> = text[..=text.rfind('\n')?].lines().collect();
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        if lines.last().is_some_and(|l| !serde_json::from_str::<Value>(l).is_ok_and(|v| v.is_object())) {
            lines.pop();
        }
        if lines.is_empty() { None } else { Some(lines.iter().map(|l| format!("{l}\n")).collect()) }
    }

    /// Reads the stream: strict JSONL (one object per LF-terminated line), where each assistant `message_end`
    /// is a finished message. The answer is the text of the last one, which must have stopped normally
    /// (`stop`: pi exits 0 for an `error`, `aborted` or `length` stop in this mode) and not be blank; a `length`
    /// stop gives its complete lines (GXD3, [`complete_lines`]), `cut`.
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
        let mut cut = false;
        let answer = match last {
            None => Err("the stream has no assistant message".to_owned()),
            Some(m) => {
                let text: String = m.content.iter().filter(|b| b.kind == "text").filter_map(|b| b.text.as_deref()).collect();
                match m.stop_reason.as_str() {
                    "stop" if text.trim().is_empty() => Err("the final assistant message has no text".to_owned()),
                    "stop" => Ok(text),
                    // GXD3: the answer is the text up to its last newline; the cut, incomplete line is dropped.
                    "length" => match complete_lines(&text) {
                        Some(complete) => {
                            cut = true;
                            Ok(complete)
                        }
                        None => Err("the final assistant message stopped with `length` before its first complete line".to_owned()),
                    },
                    other => {
                        let why = m.error_message.map(|e| format!(": {e}")).unwrap_or_default();
                        Err(format!("the final assistant message stopped with `{other}`{why}"))
                    }
                }
            }
        };
        Ok(Output { usage, answer, cut })
    }
}

/// What became of a child that was given a time limit.
#[derive(Debug)]
pub(crate) enum Waited {
    /// It ended; `stdout` is everything it printed (empty when its stdout was not piped).
    Exited { status: ExitStatus, stdout: io::Result<Vec<u8>> },
    TimedOut,
}

/// [`Runner::run`] of `call` for at most `limit`; a call that runs past it is [`Waited::TimedOut`].
pub(super) fn wait_limited_with(runner: &dyn Runner, call: &Call, limit: Duration) -> io::Result<Waited> {
    let call = Call { deadline: limit, ..call.clone() };
    match runner.run(&call) {
        Ok(out) => Ok(Waited::Exited { status: out.status, stdout: Ok(out.stdout) }),
        Err(deadline::Failed::Timeout(_)) => Ok(Waited::TimedOut),
        Err(deadline::Failed::Io(e)) => Err(e),
    }
}

/// Step 4's process on `runner`: `launch` in `dir`, its stderr appended to `log`, no stdin, under `limit`. The
/// variables of `environ` (the names of the process's environment) that start with `CPD_` are removed.
pub(super) fn run_pi_with(runner: &dyn Runner, launch: &pi::Launch, dir: &Path, log: &Path, limit: Duration, environ: &[String]) -> io::Result<Waited> {
    let mut envs: Vec<(String, Option<String>)> = environ.iter().filter(|k| k.starts_with("CPD_")).map(|k| (k.clone(), None)).collect();
    envs.extend(launch.env.iter().map(|(k, v)| (k.clone(), Some(v.clone()))));
    let call = Call {
        tool: "pi".to_owned(),
        program: launch.argv[0].clone(),
        args: launch.argv[1..].to_vec(),
        envs,
        cwd: Some(dir.to_path_buf()),
        deadline: limit,
        stdin: None,
        stderr_to: Some(log.to_path_buf()),
    };
    wait_limited_with(runner, &call, limit)
}

/// How long a pi run took, as `runs.jsonl` writes it (contract §4.6 "The run log").
#[derive(Debug)]
pub(crate) struct Timed {
    /// `ports.clock.utc()` when the run began, RFC 3339 with milliseconds.
    pub(crate) started_at: String,
    /// The seconds the run took on `ports.clock`, rounded to the millisecond.
    pub(crate) seconds: f64,
    pub(crate) waited: Waited,
}

/// [`run_pi_with`] on `ports`, timed by `ports.clock` (`started_at` and `seconds` of the run log): what `Run::run_pi` calls.
pub(crate) fn run_pi_timed_with(
    ports: &Ports,
    launch: &pi::Launch,
    dir: &Path,
    log: &Path,
    limit: Duration,
    environ: &[String],
) -> io::Result<Timed> {
    let started_at = crate::clock::timestamp(ports.clock.utc());
    let started = ports.clock.now();
    let waited = run_pi_with(&*ports.runner, launch, dir, log, limit, environ)?;
    let seconds = (ports.clock.now().saturating_duration_since(started).as_secs_f64() * 1000.0).round() / 1000.0;
    Ok(Timed { started_at, seconds, waited })
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
            "[hs-q1] question (2026-10-01 UTC)\nWhich port?\n\n\
             [hs-a1] answer (2026-10-02 UTC)\nanswers hs-q1\nPort 80.\nSecond line.\n\n\
             [hs-d2] decision (2026-10-03 UTC)\nUse it.\n\n"
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
    fn a_length_stop_gives_its_complete_lines() {
        let answer = |text: &str| pi_json::parse(assistant(text, "length", 1, 0, 0, 2, 0.1).as_bytes()).unwrap();
        // The text up to its last newline.
        let out = answer("{\"a\":1}\n{\"b\":2}\n{\"c\"");
        assert_eq!((out.answer, out.cut), (Ok("{\"a\":1}\n{\"b\":2}\n".to_owned()), true));
        // A final line that is not a complete object is the cut line, even with a newline after it.
        let out = answer("{\"a\":1}\n{\"c\":\n");
        assert_eq!((out.answer, out.cut), (Ok("{\"a\":1}\n".to_owned()), true));
        let out = answer("{\"a\":1}\n");
        assert_eq!((out.answer, out.cut), (Ok("{\"a\":1}\n".to_owned()), true));
        for text in ["", "\n \n", "{\"a\":1}", "{\"c\":\n"] {
            assert!(answer(text).answer.unwrap_err().contains("`length`"), "{text:?}");
        }
        let out = pi_json::parse(assistant("one\n", "error", 1, 0, 0, 2, 0.1).as_bytes()).unwrap();
        assert!(!out.cut);
        assert!(out.answer.unwrap_err().contains("`error`"));
    }

    #[test]
    fn a_stream_that_is_not_pi_json_cannot_be_parsed() {
        assert!(pi_json::parse(b"# Digest\n").unwrap_err().contains("line 1"));
        assert!(pi_json::parse(b"[1]\n").is_err());
        let no_usage = r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"stop"}}"#;
        assert!(pi_json::parse(no_usage.as_bytes()).unwrap_err().contains("malformed"));
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
