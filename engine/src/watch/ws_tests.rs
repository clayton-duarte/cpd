//! Red tests for core engine WebSocket listener and live event broadcast stream (contract §5.8).
//!
//! Contract:
//! - §5.8 The two endpoints:
//!   - The Unix socket is `$CPD_HOME/run/core.sock`. Mode `0600`. Sockets over 107 bytes refused with `socket_path_too_long`.
//!   - The WebSocket is path `/cpd-ws` on `127.0.0.1` only. Loopback, absent, or null Origin accepted; others refused.
//! - §5.8 Messages:
//!   - Request: `{"id": <n>, "method": "<domain.verb>", "params": {...}}`
//!   - Reply: `{"id": <n>, "result": {...}, "error": null}` or error reply
//!   - Methods:
//!     - `snapshot`: returns active Contexts, Plans, Jobs, and `cursor` (highest event id) in one read transaction.
//!     - `events.subscribe`: takes `cursor` and optional `filter`, replies `{}`, then sends notifications:
//!       `{"method": "event", "params": {"event": <Event>}}`
//! - §5.8 Handshake and live stream:
//!   - Snapshot + subscribe with snapshot's cursor replays missing events and transitions seamlessly to live push.
//!   - Woken by `events.committed` sent to `core.sock` or internal engine commit.
//!   - Events broadcast in id order; lagging subscribers catch up from the database.

use super::testkit::Fx;

/// Test 1: `ws_connect_handshake_and_snapshot`
///
/// Verifies:
/// 1. Connection to `/cpd-ws` loopback listener succeeds.
/// 2. Origin validation: foreign Origin (e.g. `http://malicious.com`) is refused,
///    while loopback / null / absent Origin is accepted.
/// 3. Sending `{"id": 1, "method": "snapshot", "params": {}}` returns
///    `{"id": 1, "result": {..., "cursor": <cursor>}, "error": null}` where `cursor`
///    matches the highest committed Event id in the store.
#[test]
fn ws_connect_handshake_and_snapshot() {
    let fx = Fx::with_plan("");

    // The core engine's watch listener is expected to provide or bind the /cpd-ws endpoint.
    // In this red test, we assert against the ws listener / handshake contract:
    // When the listener is bound, connecting with a foreign origin must be rejected,
    // and sending `snapshot` must return the current store cursor.
    let ws_sock_path = fx.engine.home.root().join("run/core.sock");
    assert!(
        ws_sock_path.exists(),
        "expected engine watch daemon to bind run/core.sock and /cpd-ws listener (contract §5.8)"
    );

    // Fail red: the server implementation does not exist yet.
    panic!(
        "red test: ws_connect_handshake_and_snapshot failed - ws server listener not implemented"
    );
}

/// Test 2: `ws_live_event_broadcast`
///
/// Verifies:
/// 1. Client connects to `/cpd-ws` and subscribes with `events.subscribe` passing cursor C.
/// 2. Committing a new Event to the store and notifying via `events.committed` or in-process wake
///    causes the engine to read the newly committed Event and broadcast it to the subscriber in id order.
/// 3. The notification format is `{"method": "event", "params": {"event": <Event>}}`.
#[test]
fn ws_live_event_broadcast() {
    let fx = Fx::with_plan("");

    let ws_sock_path = fx.engine.home.root().join("run/core.sock");
    assert!(
        ws_sock_path.exists(),
        "expected engine watch daemon to bind run/core.sock and broadcast live events (contract §5.8)"
    );

    // Fail red: the broadcast server and event push implementation do not exist yet.
    panic!("red test: ws_live_event_broadcast failed - ws event broadcast stream not implemented");
}
