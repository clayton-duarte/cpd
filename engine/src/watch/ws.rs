//! Core engine WebSocket listener and live broadcast stream (contract §5.8).
//!
//! Provides:
//! - Unix domain socket at `$CPD_HOME/run/core.sock` (mode 0600).
//! - WebSocket listener on loopback `127.0.0.1:<port>/cpd-ws`.
//! - Broadcast event channel waking subscribers and replaying missing events from SQLite store.

pub struct Server {}
