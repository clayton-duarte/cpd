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

/// The `status` pane (contract §20.1): the Workers counts and the incident state, read fresh from the
/// store on every snapshot, through the one open `Env` `serve` holds. The counts are the engine's own
/// numbers — the tenant Jobs of `workers` (the shared tenancy query, [`crate::resource::tenants`]), the
/// Ready Jobs (E3's rule, [`crate::resource::READY`]), the `queued` Jobs and the config `concurrency` —
/// read with the incident state in one read transaction, like the `@cpd_workers` step.
fn status_pane(env: &mut Env) -> Result<Value, CpdError> {
    // `M`, read fresh on every snapshot (engine/src/watch/workers.rs).
    let limit = crate::config::Config::load(&env.home)?.concurrency;
    let (running, waiting, queued, incident) = env.store.read(|conn| {
        let running =
            u32::try_from(crate::resource::tenants(conn, crate::resource::WORKERS)?.len())
                .unwrap_or(u32::MAX);
        let waiting = crate::resource::ready_count(conn)?;
        let queued: u32 = conn.query_row(crate::resource::QUEUED, [], |r| r.get(0))?;
        Ok((running, waiting, queued, incident_state(conn)?))
    })?;
    Ok(json!({
        "ok": true,
        "workers": {"queued": queued, "waiting": waiting, "running": running, "limit": limit},
        "incident": incident,
    }))
}

/// The incident fields of the `status` pane (contract §20.1): `on` is true exactly when the `incident`
/// row exists; `started_at` and `lead` are that row's, null while `on` is false or no Lead resolves.
fn incident_state(conn: &rusqlite::Connection) -> Result<Value, CpdError> {
    if !crate::incident::is_on(conn) {
        return Ok(json!({"on": false, "started_at": null, "lead": null}));
    }
    let started_at: String =
        conn.query_row("SELECT started_at FROM incident WHERE id = 1", [], |r| {
            r.get(0)
        })?;
    let lead = crate::incident::lead(conn)?.map(|id| id.as_str().to_owned());
    Ok(json!({"on": true, "started_at": started_at, "lead": lead}))
}

/// The four panes: what the list commands print (same functions), read through `env`.
fn panes(env: &mut Env) -> Result<Value, CpdError> {
    let crate::context::Command::List(_) = ContextCli::parse_from(["cpd", "list"]).command else {
        unreachable!("`list` parses to List")
    };
    let crate::plan::Command::List(plan) = PlanCli::parse_from(["cpd", "list"]).command else {
        unreachable!("`list` parses to List")
    };
    let crate::session::Command::List(session) = SessionCli::parse_from(["cpd", "list"]).command
    else {
        unreachable!("`list` parses to List")
    };
    let crate::inbox::cmd::Command::List(inbox) = InboxCli::parse_from(["cpd", "list"]).command
    else {
        unreachable!("`list` parses to List")
    };
    // A store written by a newer cpd ends the serve; any other pane failure is embedded.
    let pane = |result: Result<Output, CpdError>| match result {
        Err(e) if e.code() == ErrorCode::StoreTooNew => Err(e),
        other => Ok(printed(other)),
    };
    Ok(json!({
        "context": pane(crate::context::list::list_in(env))?,
        "plan": pane(crate::plan::list::list_in(env, plan))?,
        "session": pane(crate::session::row::list_in(env, session))?,
        "inbox": pane(crate::inbox::cmd::list_in(env, inbox))?,
        "status": status_pane(env)?,
    }))
}

fn plan_show(env: &mut Env, id: String, plan: &str, replaced: bool) -> Result<Frame, CpdError> {
    let args = match PlanCli::try_parse_from(["cpd", "show", &format!("--plan={plan}")]) {
        Ok(PlanCli {
            command: crate::plan::Command::Show(args),
        }) => args,
        Ok(_) => unreachable!("`show` parses to Show"),
        Err(e) => {
            return Ok(Frame::Error {
                id: Some(id),
                code: "invalid_plan_id".to_owned(),
                message: e.to_string(),
            });
        }
    };
    match crate::plan::show::show_in(env, args) {
        Ok(out) => Ok(Frame::PlanShow {
            id,
            value: printed(Ok(out)),
        }),
        // A replaced binary serves again instead of answering.
        Err(e) if replaced && e.code() == ErrorCode::StoreTooNew => Err(e),
        Err(e) => Ok(Frame::error(Some(id), &e)),
    }
}

/// One request line: the frames that answer it, or `None` for a `refresh`.
enum Request {
    Refresh,
    Answer(Frame),
}

fn request(env: &mut Env, line: &str, replaced: bool) -> Result<Request, CpdError> {
    let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(line) else {
        return Ok(Request::Answer(Frame::usage(
            None,
            "A request is one JSON object per line.",
        )));
    };
    let id = obj.get("id").and_then(Value::as_str).map(str::to_owned);
    let Some(t) = obj.get("t").and_then(Value::as_str) else {
        return Ok(Request::Answer(Frame::usage(
            id,
            "A request needs a string field `t`.",
        )));
    };
    let allowed: &[&str] = match t {
        "refresh" => &["t"],
        "plan_show" => &["t", "id", "plan"],
        other => {
            return Ok(Request::Answer(Frame::usage(
                id,
                format!("Unknown request `{other}`."),
            )));
        }
    };
    if let Some(extra) = obj.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Ok(Request::Answer(Frame::usage(
            id,
            format!("`{t}` takes no field `{extra}`."),
        )));
    }
    if t == "refresh" {
        return Ok(Request::Refresh);
    }
    match (id, obj.get("plan").and_then(Value::as_str)) {
        (Some(id), Some(plan)) => Ok(Request::Answer(plan_show(env, id, plan, replaced)?)),
        _ => Ok(Request::Answer(Frame::usage(
            None,
            "`plan_show` needs a string `id` and a string `plan`.",
        ))),
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
    let exe = match env.ports.spawn.self_exe() {
        Ok(path) => Some(crate::host::exe::Exe::at(path)?),
        Err(_) => None,
    };
    let replaced = || exe.as_ref().is_some_and(|exe| exe.replaced());
    // A new install: serve again in this process; returns only when the exec failed.
    let restart = |env: &Env| match &exe {
        Some(exe) => events::exec_again(
            &env.ports,
            exe,
            vec!["cockpit".to_owned(), "serve".to_owned()],
        ),
        None => unreachable!("a restart needs a replaced binary"),
    };
    match serve_frames(env, input, out, changes, &replaced) {
        Ok(Done::Restart) => Err(restart(env)),
        Ok(Done::Ended) => Ok(()),
        Err(e) if e.code() == ErrorCode::StoreTooNew && replaced() => Err(restart(env)),
        Err(e) => {
            // The last line: the store error. A closed stdout makes this a no-op.
            let _ = send(out, &Frame::error(None, &e));
            Err(e)
        }
    }
}

enum Done {
    Ended,
    Restart,
}

fn serve_frames(
    env: &mut Env,
    input: &mut dyn Iterator<Item = Input>,
    out: &mut dyn Write,
    changes: &mut dyn ChangeSource,
    replaced: &dyn Fn() -> bool,
) -> Result<Done, CpdError> {
    let mut cursor = env.store.read(events::highest_id)?;
    let mut seq = 0u64;
    macro_rules! emit {
        ($frame:expr) => {
            if !send(out, &$frame)? {
                return Ok(Done::Ended);
            }
        };
    }
    macro_rules! snapshot {
        () => {{
            seq += 1;
            emit!(Frame::Snapshot {
                seq,
                panes: panes(env)?
            });
        }};
    }
    emit!(Frame::Hello);
    snapshot!();
    // Ticks since the open batch's first change.
    let mut batch: Option<u64> = None;
    for item in input {
        match item {
            Input::Line(line) => {
                if replaced() {
                    return Ok(Done::Restart);
                }
                match request(env, &line, replaced())? {
                    Request::Refresh => snapshot!(),
                    Request::Answer(frame) => emit!(frame),
                }
            }
            Input::Tick => {
                if replaced() {
                    return Ok(Done::Restart);
                }
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
    Ok(Done::Ended)
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
    let started = Env::open().and_then(|env| Ok((env, Env::open()?.store)));
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
