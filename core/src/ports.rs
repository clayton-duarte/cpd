//! `Ports`: the seams a test fakes (docs/implementing.md, "Ports, Spawn and run_with").

use std::cell::RefCell;
use std::sync::Arc;

#[cfg(feature = "fakes")]
use crate::clock::ManualClock;
use crate::clock::{Clock, SystemClock};
#[cfg(feature = "fakes")]
use crate::host::host_socket::FakeHostSocket;
use crate::host::host_socket::{HostSocket, SystemHostSocket};
#[cfg(feature = "fakes")]
use crate::host::runner::FakeRunner;
use crate::host::runner::{Runner, SystemRunner};
#[cfg(feature = "fakes")]
use crate::host::spawn::FakeSpawn;
use crate::host::spawn::{ProcessSpawn, Spawn};
#[cfg(feature = "fakes")]
use crate::session::tail::FakeHostConn;
use crate::session::tail::{HostConn, SystemHostConn};

/// Time, short processes, long-lived or replacing processes, the host socket and the host connection.
#[derive(Clone)]
pub struct Ports {
    pub clock: Arc<dyn Clock>,
    pub runner: Arc<dyn Runner>,
    pub spawn: Arc<dyn Spawn>,
    pub host_socket: Arc<dyn HostSocket>,
    /// The one Rust seam P1.11 adds (contract §6.20): the socket tail's connection.
    pub host_conn: Arc<dyn HostConn>,
}

impl Ports {
    /// `SystemClock`, `SystemRunner`, `ProcessSpawn`, `SystemHostSocket` and `SystemHostConn`.
    pub fn real() -> Ports {
        Ports {
            clock: Arc::new(SystemClock),
            runner: Arc::new(SystemRunner),
            spawn: Arc::new(ProcessSpawn),
            host_socket: Arc::new(SystemHostSocket),
            host_conn: Arc::new(SystemHostConn),
        }
    }

    /// Runs `f` with `ports` as the ambient [`Ports::current`] of this thread, then puts the previous value back, also
    /// when `f` panics (a drop guard), so scopes nest. A thread `f` spawns does not see the scope: it captures
    /// `Ports::current()` before the spawn and scopes it inside.
    pub fn scope<R>(ports: &Ports, f: impl FnOnce() -> R) -> R {
        /// Puts the previous value back on drop, so a panic of `f` restores it too.
        struct Restore(Option<Ports>);
        impl Drop for Restore {
            fn drop(&mut self) {
                let previous = self.0.take();
                SCOPED.with(|cell| *cell.borrow_mut() = previous);
            }
        }
        let previous = SCOPED.with(|cell| cell.borrow_mut().replace(ports.clone()));
        let _restore = Restore(previous);
        f()
    }

    /// The ports scoped on this thread by [`Ports::scope`], else [`Ports::real`]. A wrapper that has no `Ports` of its
    /// own reads this, never `SystemRunner`, `ProcessSpawn` or `Ports::real()`.
    pub fn current() -> Ports {
        SCOPED
            .with(|cell| cell.borrow().clone())
            .unwrap_or_else(Ports::real)
    }
}

thread_local! {
    /// The ports of the innermost [`Ports::scope`] on this thread.
    static SCOPED: RefCell<Option<Ports>> = const { RefCell::new(None) };
}

/// The fakes over one `Ports`, typed so a test scripts them and reads their record.
#[cfg(feature = "fakes")]
pub struct Fakes {
    pub ports: Ports,
    pub clock: Arc<ManualClock>,
    pub runner: Arc<FakeRunner>,
    pub spawn: Arc<FakeSpawn>,
    pub host_socket: Arc<FakeHostSocket>,
    pub host_conn: Arc<FakeHostConn>,
}

#[cfg(feature = "fakes")]
impl Default for Fakes {
    fn default() -> Fakes {
        Fakes::new()
    }
}

#[cfg(feature = "fakes")]
impl Fakes {
    /// A `ManualClock`, a `FakeRunner` on that clock, a `FakeSpawn` and a `FakeHostSocket`, and the `Ports` over them.
    pub fn new() -> Fakes {
        let clock = Arc::new(ManualClock::new());
        let runner = Arc::new(FakeRunner::new((*clock).clone()));
        let spawn = Arc::new(FakeSpawn::new());
        let host_socket = Arc::new(FakeHostSocket::new());
        let host_conn = Arc::new(FakeHostConn::new());
        let ports = Ports {
            clock: clock.clone(),
            runner: runner.clone(),
            spawn: spawn.clone(),
            host_socket: host_socket.clone(),
            host_conn: host_conn.clone(),
        };
        Fakes {
            ports,
            clock,
            runner,
            spawn,
            host_socket,
            host_conn,
        }
    }
}
