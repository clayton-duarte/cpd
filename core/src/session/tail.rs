//! `cpd session tail` (contract §6.20, P1.11): the shared Rust socket tail. It connects to a durable
//! Session host's unix socket (§6.14), folds the frames onto the `SnapshotEvent` (Q35 A: one library the
//! CLI tail and `cpd chat` share) and prints the transcript as plain lines.
//!
//! **Stub.** This is the tests Job's skeleton: [`tail`] and [`Fold`] answer `not implemented`; the wire
//! types, the real adapter [`SystemHostConn`] and the fold are the implementation Job's. The tests build
//! against these names today and go red because the behaviour is not here yet.

use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::SessionId;
use crate::ports::Ports;

/// `sun_path[108]` minus the NUL: the longest socket path §6.14 accepts (contract §6.20).
pub const SOCKET_PATH_MAX: usize = 107;

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
    Snapshot { epoch: String, seq: u64, snapshot: serde_json::Value },
    Events { epoch: String, seq: u64, events: Vec<serde_json::Value> },
    #[serde(other)]
    Unknown,
}

/// The shared fold (contract §6.20, Q35 A): applies each frame to the `SnapshotEvent` state and returns
/// the transcript lines the frame completes (the plain-line grammar of contract §6.20).
#[derive(Debug, Default)]
pub struct Fold {
    cursor: Cursor,
}

impl Fold {
    /// A fold with no state yet: the base `snapshot` frame of the next attach fills it.
    pub fn new() -> Fold {
        Fold::default()
    }

    /// Applies one wire frame and returns the transcript lines it completes.
    pub fn apply(&mut self, _frame: &Frame) -> Vec<String> {
        unimplemented!("not implemented")
    }

    /// The cursor of the last frame applied (`epoch`, `seq`): what a reconnect sends.
    pub fn cursor(&self) -> Cursor {
        self.cursor.clone()
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
    fn connect(&self, _path: &Path) -> io::Result<Box<dyn Conn>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "not implemented"))
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
        FakeHostConn { inner: std::sync::Mutex::new(FakeState::default()) }
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
        self.sent.lock().unwrap_or_else(|e| e.into_inner()).push(line.to_owned());
        Ok(())
    }

    fn recv(&mut self) -> io::Result<Option<String>> {
        Ok(self.script.pop_front().flatten())
    }
}

/// `cpd session tail <id>` (contract §6.20): the command's body, what `session::run`'s dispatch calls.
pub fn tail(_home: &CpdHome, _session: &SessionId, _out: &mut dyn Write) -> Result<(), CpdError> {
    let _ = Ports::current();
    Err(CpdError::new(ErrorCode::Internal, "not implemented"))
}
