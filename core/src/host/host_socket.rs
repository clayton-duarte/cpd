//! The host socket seam (contract §6.14, §6.19): how a caller probes a durable Session host's unix socket.
//! [`SystemHostSocket`] is the real one, one `UnixStream::connect`; [`FakeHostSocket`] is scripted in tests.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What a probe of the host socket found.
#[derive(Debug)]
pub enum Reach {
    /// The socket accepted a connection: the host is listening.
    Accepted,
    /// `ENOENT`: there is no socket file (yet).
    Absent,
    /// `ECONNREFUSED`: the file exists but nothing listens.
    Refused,
    /// Any other error from the connect.
    Failed(io::Error),
}

/// Probes a host's socket path; the real one connects once.
pub trait HostSocket: Send + Sync {
    fn reach(&self, path: &Path) -> Reach;
}

/// The real one: one `UnixStream::connect`; on success the stream is dropped at once (the wait only needs the accept).
pub struct SystemHostSocket;

impl HostSocket for SystemHostSocket {
    fn reach(&self, path: &Path) -> Reach {
        match std::os::unix::net::UnixStream::connect(path) {
            Ok(stream) => {
                drop(stream);
                Reach::Accepted
            }
            Err(e) => match e.kind() {
                io::ErrorKind::NotFound => Reach::Absent,
                io::ErrorKind::ConnectionRefused => Reach::Refused,
                _ => Reach::Failed(e),
            },
        }
    }
}

/// A [`HostSocket`] a test scripts: each `reach` records the path and returns the next scripted answer, `Absent` after.
#[cfg(feature = "fakes")]
pub struct FakeHostSocket {
    inner: Mutex<Fake>,
}

#[cfg(feature = "fakes")]
#[derive(Default)]
struct Fake {
    reaches: std::collections::VecDeque<Reach>,
    paths: Vec<PathBuf>,
}

#[cfg(feature = "fakes")]
impl FakeHostSocket {
    pub fn new() -> FakeHostSocket {
        FakeHostSocket { inner: Mutex::new(Fake::default()) }
    }

    /// The answers, in order; after they run out, `reach` returns [`Reach::Absent`].
    pub fn script(&self, reaches: Vec<Reach>) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).reaches = reaches.into();
    }

    /// Every path `reach` was asked for, in order.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).paths.clone()
    }
}

#[cfg(feature = "fakes")]
impl Default for FakeHostSocket {
    fn default() -> FakeHostSocket {
        FakeHostSocket::new()
    }
}

#[cfg(feature = "fakes")]
impl HostSocket for FakeHostSocket {
    fn reach(&self, path: &Path) -> Reach {
        let mut fake = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        fake.paths.push(path.to_path_buf());
        fake.reaches.pop_front().unwrap_or(Reach::Absent)
    }
}
