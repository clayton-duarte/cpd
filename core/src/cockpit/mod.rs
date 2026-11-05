//! `cpd cockpit serve` (commands.md §11a, contract §5.5): the cockpit's snapshot and event stream, in process.
//!
//! The loop is driven by an input iterator, not by the clock or a thread, so a test runs it frame by frame:
//! an [`Input::Tick`] is one `POLL_MS` of virtual time (the loop polls the [`ChangeSource`] once and closes a batch
//! whose first change is `COALESCE_MS` or more old), an [`Input::Line`] is one line of stdin, and the end of the
//! iterator is end of file of stdin: serving ends `Ok(())` and writes nothing more (a batch still open is dropped).
//! Every frame is one line of `out`, flushed when produced.

use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

use clap::Parser;
use serde_json::{Map, Value, json};

use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, FollowArgs, Scope};
use crate::output::Output;
use crate::ports::Ports;

/// The interval of the `data_version` poll (contract §5.5).
pub const POLL_MS: u64 = 10;
/// The window in which changes merge into one snapshot (contract §5.5).
pub const COALESCE_MS: u64 = 16;

/// One step of the serving loop.
pub enum Input {
    /// A line of stdin, without its newline.
    Line(String),
    /// `POLL_MS` of virtual time has passed: poll the [`ChangeSource`] once.
    Tick,
}

/// What reports that the store changed: the real one reads `PRAGMA data_version`; a test scripts it.
pub trait ChangeSource {
    /// `Some(new data_version)` when the store changed since the last poll, else `None`.
    fn poll(&mut self) -> Option<i64>;
}

/// A frame of stdout (commands.md §11a "Frames out").
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    Hello,
    Snapshot {
        seq: u64,
        panes: Value,
    },
    Event(Value),
    PlanShow {
        id: String,
        value: Value,
    },
    Error {
        id: Option<String>,
        code: String,
        message: String,
    },
}

fn json_str(s: &str) -> String {
    Value::String(s.to_owned()).to_string()
}

impl Frame {
    /// The frame as one line of JSON with its newline.
    pub fn to_line(&self) -> String {
        match self {
            Frame::Hello => "{\"t\":\"hello\",\"protocol\":1}\n".to_owned(),
            Frame::Snapshot { seq, panes } => {
                format!("{{\"t\":\"snapshot\",\"seq\":{seq},\"panes\":{panes}}}\n")
            }
            Frame::Event(event) => format!("{{\"t\":\"event\",\"event\":{event}}}\n"),
            Frame::PlanShow { id, value } => format!(
                "{{\"t\":\"plan_show\",\"id\":{},\"value\":{value}}}\n",
                json_str(id)
            ),
            Frame::Error { id, code, message } => {
                let id = id.as_deref().map_or("null".to_owned(), json_str);
                format!(
                    "{{\"t\":\"error\",\"id\":{id},\"error\":{{\"code\":{},\"message\":{}}}}}\n",
                    json_str(code),
                    json_str(message)
                )
            }
        }
    }

    fn error(id: Option<String>, e: &CpdError) -> Frame {
        Frame::Error {
            id,
            code: e.code().as_str().to_owned(),
            message: e.message().to_owned(),
        }
    }

    fn usage(id: Option<String>, message: impl Into<String>) -> Frame {
        Frame::Error {
            id,
            code: "usage".to_owned(),
            message: message.into(),
        }
    }
}

/// What the CLI prints for `result`, as a value: `{"ok": true, …fields}` or the failure envelope.
fn printed(result: Result<Output, CpdError>) -> Value {
    match result {
        Ok(Output::Success(fields)) => {
            let mut value = Map::new();
            value.insert("ok".into(), Value::Bool(true));
            value.extend(fields);
            Value::Object(value)
        }
        Ok(Output::Owned) => json!({"ok": true}),
        Err(e) => serde_json::to_value(&e).unwrap_or_else(|_| e.to_json()),
    }
}

#[derive(Parser)]
struct ContextCli {
    #[command(subcommand)]
    command: crate::context::Command,
}
#[derive(Parser)]
struct PlanCli {
    #[command(subcommand)]
    command: crate::plan::Command,
}
#[derive(Parser)]
struct SessionCli {
    #[command(subcommand)]
    command: crate::session::Command,
}
#[derive(Parser)]
struct InboxCli {
    #[command(subcommand)]
    command: crate::inbox::cmd::Command,
}

/// The four panes: what the list commands print (same functions), read through `$CPD_HOME` on `env`'s ports.
fn panes(ports: &Ports) -> Value {
    Ports::scope(ports, || {
        json!({
            "context": printed(crate::context::run(ContextCli::parse_from(["cpd", "list"]).command)),
            "plan": printed(crate::plan::run(PlanCli::parse_from(["cpd", "list"]).command)),
            "session": printed(crate::session::run(SessionCli::parse_from(["cpd", "list"]).command)),
            "inbox": printed(crate::inbox::cmd::run(InboxCli::parse_from(["cpd", "list"]).command)),
        })
    })
}

fn plan_show(ports: &Ports, id: String, plan: &str) -> Frame {
    let command = match PlanCli::try_parse_from(["cpd", "show", &format!("--plan={plan}")]) {
        Ok(cli) => cli.command,
        Err(e) => {
            return Frame::Error {
                id: Some(id),
                code: "invalid_plan_id".to_owned(),
                message: e.to_string(),
            };
        }
    };
    match Ports::scope(ports, || crate::plan::run(command)) {
        Ok(out) => Frame::PlanShow {
            id,
            value: printed(Ok(out)),
        },
        Err(e) => Frame::error(Some(id), &e),
    }
}

/// One request line: the frames that answer it, or `None` for a `refresh`.
enum Request {
    Refresh,
    Answer(Frame),
}

fn request(ports: &Ports, line: &str) -> Request {
    let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(line) else {
        return Request::Answer(Frame::usage(None, "A request is one JSON object per line."));
    };
    let id = obj.get("id").and_then(Value::as_str).map(str::to_owned);
    let Some(t) = obj.get("t").and_then(Value::as_str) else {
        return Request::Answer(Frame::usage(id, "A request needs a string field `t`."));
    };
    let allowed: &[&str] = match t {
        "refresh" => &["t"],
        "plan_show" => &["t", "id", "plan"],
        other => {
            return Request::Answer(Frame::usage(id, format!("Unknown request `{other}`.")));
        }
    };
    if let Some(extra) = obj.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Request::Answer(Frame::usage(id, format!("`{t}` takes no field `{extra}`.")));
    }
    if t == "refresh" {
        return Request::Refresh;
    }
    match (id, obj.get("plan").and_then(Value::as_str)) {
        (Some(id), Some(plan)) => Request::Answer(plan_show(ports, id, plan)),
        _ => Request::Answer(Frame::usage(
            None,
            "`plan_show` needs a string `id` and a string `plan`.",
        )),
    }
}

/// Writes `frame` and flushes. `Ok(false)` when the reader is gone (EPIPE).
fn send(out: &mut dyn Write, frame: &Frame) -> Result<bool, CpdError> {
    match out
        .write_all(frame.to_line().as_bytes())
        .and_then(|()| out.flush())
    {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(CpdError::new(
            ErrorCode::Internal,
            format!("Cannot write to stdout: {e}."),
        )),
    }
}

/// The Events committed after `cursor`, as `events follow` prints them, and the new cursor.
fn new_events(env: &mut Env, cursor: i64) -> Result<(Vec<Value>, i64), CpdError> {
    let args = FollowArgs {
        scope: Scope {
            plan: None,
            context: None,
            session: None,
        },
        kinds: Vec::new(),
        since: Some(cursor),
        until_idle: true,
    };
    let mut buf = Vec::new();
    events::follow_loop(env, args, &mut buf, &|| false, &[])?;
    let mut cursor = cursor;
    let mut events = Vec::new();
    for line in String::from_utf8_lossy(&buf).lines() {
        let event: Value = serde_json::from_str(line)
            .map_err(|e| CpdError::bad_row(format!("an Event line: {e}")))?;
        if let Some(id) = event.get("id").and_then(Value::as_i64) {
            cursor = cursor.max(id);
        }
        events.push(event);
    }
    Ok((events, cursor))
}

/// Serves until `input` ends. `Err` is a store error that ends the process (its last frame is an `error` frame).
pub fn serve(
    env: &mut Env,
    input: &mut dyn Iterator<Item = Input>,
    out: &mut dyn Write,
    changes: &mut dyn ChangeSource,
) -> Result<(), CpdError> {
    match serve_frames(env, input, out, changes) {
        Err(e) => {
            // The last line: the store error. A closed stdout makes this a no-op.
            let _ = send(out, &Frame::error(None, &e));
            Err(e)
        }
        ok => ok,
    }
}

fn serve_frames(
    env: &mut Env,
    input: &mut dyn Iterator<Item = Input>,
    out: &mut dyn Write,
    changes: &mut dyn ChangeSource,
) -> Result<(), CpdError> {
    let mut cursor = env.store.read(events::highest_id)?;
    let mut seq = 0u64;
    let ports = env.ports.clone();
    macro_rules! emit {
        ($frame:expr) => {
            if !send(out, &$frame)? {
                return Ok(());
            }
        };
    }
    macro_rules! snapshot {
        () => {{
            seq += 1;
            emit!(Frame::Snapshot {
                seq,
                panes: panes(&ports)
            });
        }};
    }
    emit!(Frame::Hello);
    snapshot!();
    // Ticks since the open batch's first change.
    let mut batch: Option<u64> = None;
    for item in input {
        match item {
            Input::Line(line) => match request(&ports, &line) {
                Request::Refresh => snapshot!(),
                Request::Answer(frame) => emit!(frame),
            },
            Input::Tick => {
                if let Some(age) = batch.as_mut() {
                    *age += 1;
                }
                if batch.is_some_and(|age| age * POLL_MS >= COALESCE_MS) {
                    batch = None;
                    let (events, next) = new_events(env, cursor)?;
                    cursor = next;
                    for event in events {
                        emit!(Frame::Event(event));
                    }
                    snapshot!();
                }
                if changes.poll().is_some() && batch.is_none() {
                    batch = Some(0);
                }
            }
        }
    }
    Ok(())
}

/// The real [`ChangeSource`]: `PRAGMA data_version` on its own connection.
struct DataVersion {
    store: crate::store::Store,
    seen: Option<i64>,
}

impl ChangeSource for DataVersion {
    fn poll(&mut self) -> Option<i64> {
        // A failed poll is no change; the next Event read meets the same store error and ends serving.
        let version = self.store.data_version().ok()?;
        let changed = self.seen.is_some_and(|seen| seen != version);
        self.seen = Some(version);
        changed.then_some(version)
    }
}

/// `cpd cockpit serve`: stdin and stdout of the process, until stdin ends.
pub fn run_stdio() -> Result<Output, CpdError> {
    let mut stdout = std::io::stdout().lock();
    let started = Env::open().and_then(|env| {
        let store =
            crate::store::Store::open(&env.home, u64::from(env.config.timeouts.store_busy_ms))?;
        Ok((env, store))
    });
    let (mut env, store) = match started {
        Ok(v) => v,
        Err(e) => return fail(&mut stdout, e),
    };
    let mut source = DataVersion { store, seen: None };
    source.poll();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut input = std::iter::from_fn(|| match rx.recv_timeout(Duration::from_millis(POLL_MS)) {
        Ok(line) => Some(Input::Line(line)),
        Err(mpsc::RecvTimeoutError::Timeout) => Some(Input::Tick),
        Err(mpsc::RecvTimeoutError::Disconnected) => None,
    });
    match serve(&mut env, &mut input, &mut stdout, &mut source) {
        Ok(()) => Ok(Output::Owned),
        Err(e) => std::process::exit(i32::from(e.exit().code())),
    }
}

/// A store error before `hello`: the same last line, then its exit.
fn fail(out: &mut dyn Write, e: CpdError) -> Result<Output, CpdError> {
    let _ = send(out, &Frame::error(None, &e));
    std::process::exit(i32::from(e.exit().code()))
}

#[cfg(test)]
mod tests;
