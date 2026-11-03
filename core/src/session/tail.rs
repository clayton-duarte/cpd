//! `cpd session tail` (contract §6.20, P1.11): the shared Rust socket tail. It connects to a durable
//! Session host's unix socket (§6.14), folds the frames onto the `SnapshotEvent` (Q35 A: one library the
//! CLI tail and `cpd chat` share) and prints the transcript as plain lines.
//!
//! The fold never opens the store and never touches the filesystem: its input is decoded frames, its
//! output lines. That is what makes it one library for the CLI tail and for `cpd chat` (Q35), and it is
//! also why the tail can never write the store (Q33): the module holds no store type to call.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::TailArgs;
use super::delta::DeltaError;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::SessionId;
use crate::output::Output;
use crate::ports::Ports;

/// `sun_path[108]` minus the NUL: the longest socket path §6.14 accepts (contract §6.20).
pub const SOCKET_PATH_MAX: usize = 107;

/// The reconnect back-off: 100 ms doubling to a 1 s cap, over a 30 s deadline (contract §6.20, the
/// design defaults of §6.19's wait; not measured).
const RECONNECT_NAP: Duration = Duration::from_millis(100);
const RECONNECT_NAP_MAX: Duration = Duration::from_secs(1);
const RECONNECT_DEADLINE: Duration = Duration::from_secs(30);

/// A cursor (`epoch`, `seq`): what a reconnect attaches with (contract §6.14, §6.20).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub epoch: String,
    pub seq: u64,
}

/// One wire frame of the host socket (contract §6.14): a `snapshot` or an `events` batch.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Frame {
    View {
        epoch: String,
        seq: u64,
        view: Value,
    },
    Ops {
        epoch: String,
        seq: u64,
        ops: Vec<Value>,
    },
    #[serde(other)]
    Unknown,
}

/// A client to host line (contract §6.14, the tail's two): the client sends `hello` and `attach` and
/// nothing else (read-only, Q33/Q35).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToHost {
    Hello {
        protocol: u64,
        min_protocol: u64,
        client: String,
    },
    Attach {
        req: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<Cursor>,
    },
}

/// A host to client line (contract §6.14). Only the fields the tail reads are typed; every other
/// message (`result`, `pong`, `host_state`, a future type) is [`ToClient::Unknown`] and ignored.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToClient {
    Welcome {
        protocol: u64,
    },
    Attached {
        mode: String,
        epoch: String,
        #[serde(default)]
        from_seq: u64,
        #[serde(default)]
        head_seq: u64,
    },
    View {
        epoch: String,
        seq: u64,
        view: Value,
    },
    Ops {
        epoch: String,
        seq: u64,
        ops: Vec<Value>,
    },
    Error {
        code: String,
        #[serde(default)]
        message: String,
    },
    Bye {
        #[serde(default)]
        reason: String,
        #[serde(default)]
        reconnect: bool,
    },
    #[serde(other)]
    Unknown,
}

/// The shared fold (contract §6.20, Q35 A): keeps the replica of the conversation view, applies each
/// frame to it and returns the transcript lines the frame completes.
#[derive(Debug, Default)]
pub struct Fold {
    cursor: Cursor,
    /// Whether a frame has ever been applied: the first attach has no cursor, a reconnect does.
    started: bool,
    /// The replica of the `ConversationView`.
    view: Option<Value>,
    /// The id of the last entry printed.
    last_printed: Option<u64>,
    /// The stderr lines naming a gap, in order.
    gaps: Vec<String>,
}

impl Fold {
    /// A fold with no state yet: the base `view` frame of the next attach fills it.
    pub fn new() -> Fold {
        Fold::default()
    }

    /// Applies one wire frame and returns the transcript lines it completes.
    pub fn apply(&mut self, frame: &Frame) -> Result<Vec<String>, DeltaError> {
        match frame {
            Frame::Unknown => Ok(Vec::new()),
            Frame::View { epoch, seq, view } => {
                let lines = self.fold_view(view.clone());
                self.advance(epoch, *seq);
                Ok(lines)
            }
            Frame::Ops { epoch, seq, ops } => {
                let Some(replica) = self.view.take() else {
                    self.forget();
                    return Err(DeltaError("an ops frame before any view".to_owned()));
                };
                match super::delta::apply(replica, ops) {
                    Ok(view) => {
                        let lines = self.fold_view(view);
                        self.advance(epoch, *seq);
                        Ok(lines)
                    }
                    Err(e) => {
                        self.forget();
                        Err(e)
                    }
                }
            }
        }
    }

    fn advance(&mut self, epoch: &str, seq: u64) {
        self.started = true;
        self.cursor = Cursor {
            epoch: epoch.to_owned(),
            seq,
        };
    }

    fn forget(&mut self) {
        self.view = None;
        self.started = false;
        self.cursor = Cursor::default();
    }

    /// Makes `view` the replica and returns the lines of its entries newer than the last printed.
    fn fold_view(&mut self, view: Value) -> Vec<String> {
        if let (Some(last), Some(before)) = (
            self.last_printed,
            view.get("before").and_then(Value::as_u64),
        ) && before > last
        {
            self.gaps.push(format!(
                "cpd session tail: entries missed: the last printed entry is {last}, the window starts after {before}"
            ));
        }
        let mut lines = Vec::new();
        if let Some(entries) = view.get("entries").and_then(Value::as_array) {
            for entry in entries {
                let Some(id) = entry.get("id").and_then(Value::as_u64) else {
                    continue;
                };
                if self.last_printed.is_none_or(|last| id > last) {
                    lines.extend(render_entry(entry));
                    self.last_printed = Some(id);
                }
            }
        }
        self.view = Some(view);
        lines
    }

    /// The cursor of the last frame applied (`epoch`, `seq`): what a reconnect sends.
    pub fn cursor(&self) -> Cursor {
        self.cursor.clone()
    }

    /// The gap lines the fold has noted so far (the tail writes each to stderr).
    pub fn gap_notices(&self) -> Vec<String> {
        self.gaps.clone()
    }
}

/// The transcript lines of one entry: its `model` messages, or nothing when it has none.
fn render_entry(entry: &Value) -> Vec<String> {
    match entry.get("model").and_then(Value::as_array) {
        Some(messages) => messages.iter().flat_map(render_message).collect(),
        None => Vec::new(),
    }
}

/// One completed message's transcript lines (contract §6.20 rule 3): one line per text block, the prefix
/// on the message's first line only; thinking and tool-call blocks are state, not transcript.
fn render_message(message: &Value) -> Vec<String> {
    let role = message.get("role").and_then(Value::as_str).unwrap_or("");
    let prefix = match role {
        "system" => "system: ".to_owned(),
        "user" => "user: ".to_owned(),
        "assistant" => "assistant: ".to_owned(),
        "toolResult" => format!(
            "tool {}: ",
            message
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or("")
        ),
        _ => return Vec::new(),
    };
    let mut lines = Vec::new();
    for block in text_blocks(message.get("content")) {
        let mut source = block.split('\n');
        if let Some(first) = source.next() {
            lines.push(if lines.is_empty() {
                format!("{prefix}{first}")
            } else {
                first.to_owned()
            });
        }
        lines.extend(source.map(str::to_owned));
    }
    if role == "toolResult"
        && message
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        match lines.last_mut() {
            Some(last) => last.push_str(" (error)"),
            None => lines.push(format!("{prefix}(error)")),
        }
    }
    lines
}

/// The text blocks of a message's `content` (a bare string, or an array of typed blocks).
fn text_blocks(content: Option<&Value>) -> Vec<String> {
    match content {
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str).map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// One connection to a host socket: one wire line in, one wire line out (contract §6.20).
pub trait Conn {
    fn send(&mut self, line: &str) -> io::Result<()>;
    /// The next complete line, or `None` at EOF.
    fn recv(&mut self) -> io::Result<Option<String>>;
}

/// The host socket port (contract §6.20): one `connect` per connection. A real adapter implements every
/// member; none is optional (§6.19's port rule).
pub trait HostConn: Send + Sync {
    fn connect(&self, path: &Path) -> io::Result<Box<dyn Conn>>;
}

/// The real adapter (contract §6.20): one `UnixStream` to the same unix path the host binds (Q34: no TCP).
pub struct SystemHostConn;

impl HostConn for SystemHostConn {
    fn connect(&self, path: &Path) -> io::Result<Box<dyn Conn>> {
        let stream = UnixStream::connect(path)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Box::new(SystemConn { stream, reader }))
    }
}

/// A real connection: the write half and a line-buffered read half (the same one-line-per-frame rule as
/// the host's `LineSplitter`, §6.14 step 5).
struct SystemConn {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Conn for SystemConn {
    fn send(&mut self, line: &str) -> io::Result<()> {
        self.stream.write_all(line.as_bytes())?;
        self.stream.flush()
    }

    fn recv(&mut self) -> io::Result<Option<String>> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        Ok(Some(line))
    }
}

/// A [`HostConn`] a test scripts: one server-side line script per `connect`, and the client lines it sent.
#[cfg(feature = "fakes")]
pub struct FakeHostConn {
    inner: std::sync::Mutex<FakeState>,
}

#[cfg(feature = "fakes")]
#[derive(Default)]
struct FakeState {
    /// The server-side script of each not-yet-served connection.
    scripts: std::collections::VecDeque<std::collections::VecDeque<Option<String>>>,
    /// The client lines sent on each connection, in connect order.
    sent: Vec<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
    /// Every path `connect` was asked for, in order.
    paths: Vec<std::path::PathBuf>,
}

#[cfg(feature = "fakes")]
impl FakeHostConn {
    pub fn new() -> FakeHostConn {
        FakeHostConn {
            inner: std::sync::Mutex::new(FakeState::default()),
        }
    }

    /// Queues the lines the host sends on the next connection, in order; `None` is a read EOF, and the
    /// script runs out after its last line.
    pub fn serve(&self, lines: Vec<Option<String>>) {
        self.state().scripts.push_back(lines.into());
    }

    /// Every path `connect` was asked for, in order.
    pub fn paths(&self) -> Vec<std::path::PathBuf> {
        self.state().paths.clone()
    }

    /// The lines the client sent on connection `connection` (0-based).
    pub fn sent(&self, connection: usize) -> Vec<String> {
        self.state()
            .sent
            .get(connection)
            .map(|lines| lines.lock().unwrap_or_else(|e| e.into_inner()).clone())
            .unwrap_or_default()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(feature = "fakes")]
impl Default for FakeHostConn {
    fn default() -> FakeHostConn {
        FakeHostConn::new()
    }
}

#[cfg(feature = "fakes")]
impl HostConn for FakeHostConn {
    fn connect(&self, path: &Path) -> io::Result<Box<dyn Conn>> {
        let mut state = self.state();
        state.paths.push(path.to_path_buf());
        let script = state.scripts.pop_front().unwrap_or_default();
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        state.sent.push(sent.clone());
        Ok(Box::new(FakeConn { script, sent }))
    }
}

#[cfg(feature = "fakes")]
struct FakeConn {
    script: std::collections::VecDeque<Option<String>>,
    sent: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

#[cfg(feature = "fakes")]
impl Conn for FakeConn {
    fn send(&mut self, line: &str) -> io::Result<()> {
        self.sent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(line.to_owned());
        Ok(())
    }

    fn recv(&mut self) -> io::Result<Option<String>> {
        Ok(self.script.pop_front().flatten())
    }
}

/// The dispatch body of `cpd session tail <id>` (contract §6.20, commands.md §10.19): the command prints
/// plain transcript lines on stdout and nothing else, so it returns [`Output::Owned`].
pub(super) fn command(args: TailArgs) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    tail(&home, &args.id, &mut out)?;
    Ok(Output::Owned)
}

/// What one connection ended with: the stream is over, or the client reconnects with the cursor.
#[derive(PartialEq, Eq)]
enum Flow {
    Done,
    Reconnect,
}

/// `cpd session tail <id>` (contract §6.20): the command's body, what `session::run`'s dispatch calls.
/// Connects, handshakes, attaches and prints; reconnects with the cursor until the stream is over or the
/// 30 s deadline passes.
pub fn tail(home: &CpdHome, session: &SessionId, out: &mut dyn Write) -> Result<(), CpdError> {
    let path = home.run_dir().join("host").join(format!("{session}.sock"));
    let length = path.as_os_str().as_bytes().len();
    if length > SOCKET_PATH_MAX {
        return Err(CpdError::new(
            ErrorCode::Usage,
            format!("The host socket path for Session {session} is {length} bytes, over the {SOCKET_PATH_MAX}-byte limit: socket_path_too_long."),
        )
        .with("session", session.as_str())
        .with("socket", path.display().to_string())
        .with("length", length)
        .with("max", SOCKET_PATH_MAX));
    }

    let ports = Ports::current();
    let mut fold = Fold::new();
    let mut req = 0u64;
    let mut waited = Duration::ZERO;
    let mut nap = RECONNECT_NAP;
    loop {
        if one(&ports, &path, &mut fold, &mut req, out)? == Flow::Done {
            return Ok(());
        }
        if waited >= RECONNECT_DEADLINE {
            return Err(CpdError::new(
                ErrorCode::HostTimeout,
                format!("Session {session}'s host did not answer on {} within {}s: the engine is not running or the host keeps dying.", path.display(), RECONNECT_DEADLINE.as_secs()),
            )
            .with("session", session.as_str())
            .with("socket", path.display().to_string()));
        }
        ports.clock.sleep(nap);
        waited += nap;
        nap = (nap * 2).min(RECONNECT_NAP_MAX);
    }
}

/// One connection: connect, `hello`, and one `attach` per `welcome`; then apply and print frames until the
/// stream ends (`bye reconnect:false`, stdout closing) or the host says goodbye with `reconnect:true`.
fn one(
    ports: &Ports,
    path: &Path,
    fold: &mut Fold,
    req: &mut u64,
    out: &mut dyn Write,
) -> Result<Flow, CpdError> {
    let mut conn = match ports.host_conn.connect(path) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!(
                "cpd session tail: cannot connect to {}: {e}",
                path.display()
            );
            return Ok(Flow::Reconnect);
        }
    };
    send(
        &mut *conn,
        &ToHost::Hello {
            protocol: 2,
            min_protocol: 2,
            client: "cpd session tail".to_owned(),
        },
    )?;
    loop {
        let line = match conn.recv() {
            Ok(Some(line)) => line,
            Ok(None) => return Ok(Flow::Reconnect),
            Err(e) => {
                eprintln!("cpd session tail: lost the host socket: {e}");
                return Ok(Flow::Reconnect);
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let message: ToClient = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(e) => {
                eprintln!("cpd session tail: ignoring a malformed host line: {e}");
                continue;
            }
        };
        match message {
            ToClient::Welcome { .. } => {
                *req += 1;
                let cursor = if fold.started {
                    Some(fold.cursor())
                } else {
                    None
                };
                send(
                    &mut *conn,
                    &ToHost::Attach {
                        req: format!("r{req}"),
                        cursor,
                    },
                )?;
            }
            ToClient::View { epoch, seq, view } => {
                if fold.started {
                    eprintln!("view {epoch} seq {seq}: view replaced");
                }
                let applied = fold.apply(&Frame::View { epoch, seq, view });
                match print_applied(fold, applied, out)? {
                    Applied::Printed => {}
                    Applied::Closed => return Ok(Flow::Done),
                    Applied::Refused => return Ok(Flow::Reconnect),
                }
            }
            ToClient::Ops { epoch, seq, ops } => {
                let applied = fold.apply(&Frame::Ops { epoch, seq, ops });
                match print_applied(fold, applied, out)? {
                    Applied::Printed => {}
                    Applied::Closed => return Ok(Flow::Done),
                    Applied::Refused => return Ok(Flow::Reconnect),
                }
            }
            ToClient::Error { code, message } => {
                if code == "protocol_mismatch" {
                    return Err(CpdError::new(
                        ErrorCode::Internal,
                        format!(
                            "this host speaks protocol 2, the client speaks 2 (the installed cpd): {message}"
                        ),
                    ));
                }
                eprintln!("cpd session tail: the host answered error {code}: {message}");
            }
            ToClient::Bye { reason, reconnect } => {
                return Ok(if reconnect {
                    Flow::Reconnect
                } else {
                    let _ = reason;
                    Flow::Done
                });
            }
            ToClient::Attached { .. } | ToClient::Unknown => {}
        }
    }
}

/// What printing an applied frame ended with.
enum Applied {
    Printed,
    /// Stdout closed: the stream ends cleanly.
    Closed,
    /// An op was refused: the fold forgot its state, the client reconnects with no cursor.
    Refused,
}

/// Prints what a fold applied; a refused op and a gap are written to stderr.
fn print_applied(
    fold: &mut Fold,
    applied: Result<Vec<String>, DeltaError>,
    out: &mut dyn Write,
) -> Result<Applied, CpdError> {
    match applied {
        Ok(lines) => {
            for gap in fold.gaps.drain(..) {
                eprintln!("{gap}");
            }
            Ok(if write_lines(out, &lines)? {
                Applied::Printed
            } else {
                Applied::Closed
            })
        }
        Err(e) => {
            eprintln!("cpd session tail: {e}; reconnecting without a cursor");
            Ok(Applied::Refused)
        }
    }
}

/// Writes one line per transcript line, at once. A closed stdout (EPIPE) ends the stream cleanly (`false`);
/// any other write error is a failure.
fn write_lines(out: &mut dyn Write, lines: &[String]) -> Result<bool, CpdError> {
    for line in lines {
        if let Err(e) = out
            .write_all(line.as_bytes())
            .and_then(|()| out.write_all(b"\n"))
        {
            if e.kind() == io::ErrorKind::BrokenPipe {
                return Ok(false);
            }
            return Err(CpdError::new(
                ErrorCode::Internal,
                format!("Cannot write the transcript: {e}."),
            ));
        }
    }
    let _ = out.flush();
    Ok(true)
}

/// Sends one client line, encoded as NDJSON (contract §6.14).
fn send(conn: &mut dyn Conn, message: &ToHost) -> Result<(), CpdError> {
    let mut line = serde_json::to_string(message).map_err(|e| {
        CpdError::new(
            ErrorCode::Internal,
            format!("Cannot encode a host line: {e}."),
        )
    })?;
    line.push('\n');
    conn.send(&line).map_err(|e| {
        CpdError::new(
            ErrorCode::Internal,
            format!("Cannot write to the host socket: {e}."),
        )
    })
}
