//! Tests for core engine WebSocket listener and live event broadcast stream (contract §5.8).
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

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use super::testkit::Fx;
use super::ws::{WsServer, notify_events_committed};
use cpd_core::events::NewEvent;
use serde_json::{Value, json};

fn ws_connect(port: u16, origin: Option<&str>) -> (BufReader<TcpStream>, u16) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let mut req = format!(
        "GET /cpd-ws HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n"
    );
    if let Some(orig) = origin {
        req.push_str(&format!("Origin: {orig}\r\n"));
    }
    req.push_str("\r\n");

    stream.write_all(req.as_bytes()).unwrap();

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line).unwrap();

    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Consume remaining headers
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }

    (reader, status_code)
}

fn send_ws_text(reader: &mut BufReader<TcpStream>, text: &str) {
    let bytes = text.as_bytes();
    let mut frame = Vec::new();
    frame.push(0x81); // FIN + Text
    // Client-to-server frames MUST be masked per RFC 6455
    let mask = [0x12, 0x34, 0x56, 0x78];
    let len = bytes.len();
    if len <= 125 {
        frame.push(0x80 | (len as u8));
    } else if len <= 65535 {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    for (i, b) in bytes.iter().enumerate() {
        frame.push(b ^ mask[i % 4]);
    }
    reader.get_mut().write_all(&frame).unwrap();
}

fn read_ws_text(reader: &mut BufReader<TcpStream>) -> Value {
    let mut header = [0u8; 2];
    reader.read_exact(&mut header).unwrap();
    let mut payload_len = (header[1] & 0x7F) as usize;
    if payload_len == 126 {
        let mut ext = [0u8; 2];
        reader.read_exact(&mut ext).unwrap();
        payload_len = u16::from_be_bytes(ext) as usize;
    } else if payload_len == 127 {
        let mut ext = [0u8; 8];
        reader.read_exact(&mut ext).unwrap();
        payload_len = u64::from_be_bytes(ext) as usize;
    }

    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).unwrap();
    serde_json::from_slice(&payload).unwrap()
}

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
    let mut fx = Fx::with_plan("");

    let server = WsServer::bind(&fx.engine.home, &fx.engine.store).unwrap();
    let port = server.port();

    let ws_sock_path = fx.engine.home.root().join("run/core.sock");
    assert!(ws_sock_path.exists(), "core.sock must exist");

    // Mode 0600 check
    let metadata = std::fs::metadata(&ws_sock_path).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);

    // Foreign origin refused (403)
    let (_stream, status) = ws_connect(port, Some("http://malicious.com"));
    assert_eq!(status, 403, "foreign origin must be rejected with 403");

    // Loopback origin accepted (101)
    let (_stream, status) = ws_connect(port, Some("http://localhost:3000"));
    assert_eq!(status, 101, "loopback origin must be accepted");

    // Null origin accepted (101)
    let (_stream, status) = ws_connect(port, Some("null"));
    assert_eq!(status, 101, "null origin must be accepted");

    // Absent origin accepted (101)
    let (mut stream, status) = ws_connect(port, None);
    assert_eq!(status, 101, "absent origin must be accepted");

    // Send snapshot request
    send_ws_text(
        &mut stream,
        &json!({
            "id": 1,
            "method": "snapshot",
            "params": {}
        })
        .to_string(),
    );

    let reply = read_ws_text(&mut stream);
    assert_eq!(reply["id"], 1);
    assert!(reply["error"].is_null());
    let cursor = reply["result"]["cursor"].as_i64().expect("cursor integer");

    let store_cursor = fx.engine.store.read(cpd_core::events::highest_id).unwrap();
    assert_eq!(
        cursor, store_cursor,
        "snapshot cursor must match highest event id in store"
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
    let mut fx = Fx::with_plan("");

    let server = WsServer::bind(&fx.engine.home, &fx.engine.store).unwrap();
    let port = server.port();

    let (mut stream, status) = ws_connect(port, None);
    assert_eq!(status, 101);

    // Initial store cursor
    let initial_cursor = fx.engine.store.read(cpd_core::events::highest_id).unwrap();

    // Subscribe with initial cursor
    send_ws_text(
        &mut stream,
        &json!({
            "id": 2,
            "method": "events.subscribe",
            "params": {
                "cursor": initial_cursor
            }
        })
        .to_string(),
    );

    let sub_reply = read_ws_text(&mut stream);
    assert_eq!(sub_reply["id"], 2);
    assert!(sub_reply["error"].is_null());

    // Commit an event into the store
    let event_id = fx
        .engine
        .store
        .write("test", |tx| {
            cpd_core::events::append(
                tx,
                NewEvent::watch_started("watch engine started for test", 9999),
            )
        })
        .unwrap();

    assert!(event_id > initial_cursor);

    // Notify via events.committed over core.sock
    notify_events_committed(&fx.engine.home);

    // Read the pushed event notification from WebSocket stream
    let push = read_ws_text(&mut stream);
    assert_eq!(push["method"], "event");
    let event = &push["params"]["event"];
    assert_eq!(event["id"], event_id);
    assert_eq!(event["kind"], "watch-started");
}
