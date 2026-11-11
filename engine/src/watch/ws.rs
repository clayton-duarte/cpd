//! Core engine WebSocket listener and live broadcast stream (contract §5.8).
//!
//! - Unix socket: `$CPD_HOME/run/core.sock` (mode 0600, path <= 107 bytes).
//! - WebSocket: `/cpd-ws` on 127.0.0.1 (OS-assigned port).
//!   Loopback, null or absent Origin accepted; foreign Origin refused (403).
//! - Messages:
//!   - `snapshot`: active Contexts, Plans, Jobs, and `cursor` in one read transaction.
//!   - `events.subscribe`: takes `cursor` and optional `filter`, replies `{}`,
//!     then streams `{"method":"event","params":{"event":<Event>}}`.
//!   - `events.committed`: wake-up notification over `core.sock`.

// Pure Rust SHA-1 implementation (FIPS 180-1) for WebSocket handshake
mod sha1 {
    pub struct Sha1 {
        state: [u32; 5],
        count: u64,
        buffer: [u8; 64],
    }

    impl Sha1 {
        pub fn new() -> Self {
            Sha1 {
                state: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
                count: 0,
                buffer: [0u8; 64],
            }
        }

        pub fn update(&mut self, data: &[u8]) {
            let idx = (self.count / 8 % 64) as usize;
            self.count += (data.len() as u64) * 8;

            let mut data_idx = 0;
            let mut len = data.len();

            if idx > 0 {
                let space = 64 - idx;
                if len >= space {
                    self.buffer[idx..64].copy_from_slice(&data[..space]);
                    self.transform();
                    data_idx += space;
                    len -= space;
                } else {
                    self.buffer[idx..idx + len].copy_from_slice(&data[..len]);
                    return;
                }
            }

            while len >= 64 {
                self.buffer.copy_from_slice(&data[data_idx..data_idx + 64]);
                self.transform();
                data_idx += 64;
                len -= 64;
            }

            if len > 0 {
                self.buffer[..len].copy_from_slice(&data[data_idx..data_idx + len]);
            }
        }

        pub fn finalize(mut self) -> [u8; 20] {
            let count = self.count;
            let idx = (count / 8 % 64) as usize;

            self.buffer[idx] = 0x80;
            if idx < 56 {
                self.buffer[idx + 1..56].fill(0);
            } else {
                self.buffer[idx + 1..64].fill(0);
                self.transform();
                self.buffer[..56].fill(0);
            }

            self.buffer[56..64].copy_from_slice(&count.to_be_bytes());
            self.transform();

            let mut out = [0u8; 20];
            for (i, word) in self.state.iter().enumerate() {
                out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
            }
            out
        }

        #[allow(clippy::needless_range_loop)]
        fn transform(&mut self) {
            let mut w = [0u32; 80];
            for i in 0..16 {
                w[i] = u32::from_be_bytes([
                    self.buffer[i * 4],
                    self.buffer[i * 4 + 1],
                    self.buffer[i * 4 + 2],
                    self.buffer[i * 4 + 3],
                ]);
            }
            for i in 16..80 {
                w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
            }

            let mut a = self.state[0];
            let mut b = self.state[1];
            let mut c = self.state[2];
            let mut d = self.state[3];
            let mut e = self.state[4];

            for i in 0..80 {
                let (f, k) = match i {
                    0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                    20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                    40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                    _ => (b ^ c ^ d, 0xCA62C1D6),
                };

                let temp = a
                    .rotate_left(5)
                    .wrapping_add(f)
                    .wrapping_add(e)
                    .wrapping_add(k)
                    .wrapping_add(w[i]);
                e = d;
                d = c;
                c = b.rotate_left(30);
                b = a;
                a = temp;
            }

            self.state[0] = self.state[0].wrapping_add(a);
            self.state[1] = self.state[1].wrapping_add(b);
            self.state[2] = self.state[2].wrapping_add(c);
            self.state[3] = self.state[3].wrapping_add(d);
            self.state[4] = self.state[4].wrapping_add(e);
        }
    }
}

use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::Event;
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::store::Store;
use serde_json::{Value, json};
use sha1::Sha1;

pub const SOCKET_PATH_MAX: usize = 107;
pub const CORE_SOCK_NAME: &str = "run/core.sock";
pub const CORE_PORT_NAME: &str = "run/core.port";

/// Message from worker threads to the hub.
enum HubMsg {
    Wake,
    Subscribe {
        tx: SyncSender<Value>,
        cursor: i64,
        filter: Option<EventFilter>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct EventFilter {
    pub plan: Option<PlanId>,
    pub context: Option<ContextId>,
    pub session: Option<SessionId>,
    pub kinds: HashSet<String>,
}

impl EventFilter {
    pub fn matches(&self, event: &Event) -> bool {
        if self
            .plan
            .as_ref()
            .is_some_and(|p| event.plan.as_deref() != Some(p.as_str()))
        {
            return false;
        }
        if self
            .context
            .as_ref()
            .is_some_and(|c| event.context.as_deref() != Some(c.as_str()))
        {
            return false;
        }
        if self
            .session
            .as_ref()
            .is_some_and(|s| event.session.as_deref() != Some(s.as_str()))
        {
            return false;
        }
        if !self.kinds.is_empty() && !self.kinds.contains(event.kind.as_str()) {
            return false;
        }
        true
    }
}

/// The WebSocket / Unix socket server handle.
pub struct WsServer {
    home: CpdHome,
    stop: Arc<AtomicBool>,
    port: u16,
    hub_tx: Sender<HubMsg>,
    sock_path: PathBuf,
    _threads: Vec<thread::JoinHandle<()>>,
}

impl WsServer {
    /// Bind both `run/core.sock` and `/cpd-ws` (127.0.0.1:0), start the background listener threads.
    pub fn bind(home: &CpdHome, _store: &Store) -> Result<WsServer, CpdError> {
        let sock_path = home.root().join(CORE_SOCK_NAME);
        let path_bytes = sock_path.as_os_str().as_encoded_bytes();
        if path_bytes.len() > SOCKET_PATH_MAX {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!(
                    "The socket path is {} bytes, over the {SOCKET_PATH_MAX}-byte limit: socket_path_too_long.",
                    path_bytes.len()
                ),
            )
            .with("max", SOCKET_PATH_MAX));
        }

        if let Some(parent) = sock_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                CpdError::new(
                    ErrorCode::Internal,
                    format!("Cannot create dir {}: {e}", parent.display()),
                )
            })?;
        }

        // Stale socket cleanup
        if sock_path.exists() {
            let _ = fs::remove_file(&sock_path);
        }

        let unix_listener = UnixListener::bind(&sock_path).map_err(|e| {
            CpdError::new(
                ErrorCode::Internal,
                format!("Cannot bind Unix socket {}: {e}", sock_path.display()),
            )
        })?;

        // Mode 0600
        let _ = fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o600));

        let tcp_listener = TcpListener::bind("127.0.0.1:0").map_err(|e| {
            CpdError::new(
                ErrorCode::Internal,
                format!("Cannot bind TCP listener on 127.0.0.1:0: {e}"),
            )
        })?;
        let port = tcp_listener
            .local_addr()
            .map_err(|e| CpdError::new(ErrorCode::Internal, format!("Cannot get local addr: {e}")))?
            .port();

        // Write port file so clients can discover the port
        let port_path = home.root().join(CORE_PORT_NAME);
        let _ = fs::write(&port_path, format!("{port}\n"));

        let stop = Arc::new(AtomicBool::new(false));
        let (hub_tx, hub_rx) = mpsc::channel();

        let db_path = home.db();
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| {
            CpdError::new(
                ErrorCode::StoreFailed,
                format!("Cannot open database {}: {e}", db_path.display()),
            )
        })?;
        let _ = conn.busy_timeout(Duration::from_millis(5000));
        let conn_arc = Arc::new(Mutex::new(conn));

        // Hub thread
        let conn_hub = conn_arc.clone();
        let stop_clone = stop.clone();
        let hub_handle = thread::spawn(move || {
            run_hub(conn_hub, hub_rx, stop_clone);
        });

        // Unix socket listener thread
        let stop_unix = stop.clone();
        let hub_tx_unix = hub_tx.clone();
        let conn_unix = conn_arc.clone();
        let home_unix = home.clone();
        let unix_handle = thread::spawn(move || {
            let _ = unix_listener.set_nonblocking(true);
            while !stop_unix.load(Ordering::SeqCst) {
                match unix_listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let hub_tx = hub_tx_unix.clone();
                        let conn = conn_unix.clone();
                        let home = home_unix.clone();
                        thread::spawn(move || {
                            handle_unix_client(stream, hub_tx, conn, home);
                        });
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });

        // TCP listener thread
        let stop_tcp = stop.clone();
        let hub_tx_tcp = hub_tx.clone();
        let conn_tcp = conn_arc;
        let home_tcp = home.clone();
        let tcp_handle = thread::spawn(move || {
            let _ = tcp_listener.set_nonblocking(true);
            while !stop_tcp.load(Ordering::SeqCst) {
                match tcp_listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let hub_tx = hub_tx_tcp.clone();
                        let conn = conn_tcp.clone();
                        let home = home_tcp.clone();
                        thread::spawn(move || {
                            handle_tcp_client(stream, hub_tx, conn, home);
                        });
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(WsServer {
            home: home.clone(),
            stop,
            port,
            hub_tx,
            sock_path,
            _threads: vec![hub_handle, unix_handle, tcp_handle],
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn wake(&self) {
        let _ = self.hub_tx.send(HubMsg::Wake);
    }
}

impl Drop for WsServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.hub_tx.send(HubMsg::Wake);
        let _ = fs::remove_file(&self.sock_path);
        let _ = fs::remove_file(self.home.root().join(CORE_PORT_NAME));
    }
}

/// Send `events.committed` to `run/core.sock`. Best-effort.
pub fn notify_events_committed(home: &CpdHome) {
    let sock_path = home.root().join(CORE_SOCK_NAME);
    if let Ok(mut stream) = UnixStream::connect(sock_path) {
        let msg = json!({
            "method": "events.committed",
            "params": {}
        });
        let mut line = msg.to_string();
        line.push('\n');
        let _ = stream.write_all(line.as_bytes());
    }
}

/// A subscriber connected to the hub.
struct Subscriber {
    tx: SyncSender<Value>,
    last_sent_id: i64,
    filter: Option<EventFilter>,
}

fn run_hub(conn: Arc<Mutex<rusqlite::Connection>>, rx: Receiver<HubMsg>, stop: Arc<AtomicBool>) {
    let mut subscribers: Vec<Subscriber> = Vec::new();
    let mut high_water: i64 = conn
        .lock()
        .unwrap()
        .query_row("SELECT coalesce(max(id), 0) FROM events", [], |r| r.get(0))
        .unwrap_or(0);

    while !stop.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(HubMsg::Wake) => {
                drain_events_to_subscribers(&conn, &mut subscribers, &mut high_water);
            }
            Ok(HubMsg::Subscribe { tx, cursor, filter }) => {
                let mut last_id = cursor;
                let c = conn.lock().unwrap();
                let replayed = (|| -> Result<Vec<Event>, rusqlite::Error> {
                    let mut stmt = c.prepare(
                        "SELECT id, at, kind, context, plan, job, attempt, session, text, data
                         FROM events WHERE id > ?1 ORDER BY id ASC",
                    )?;
                    let rows = stmt.query_map([cursor], |r| {
                        Ok(Event {
                            id: r.get(0)?,
                            at: r.get(1)?,
                            kind: r.get(2)?,
                            context: r.get(3)?,
                            plan: r.get(4)?,
                            job: r.get(5)?,
                            attempt: r.get(6)?,
                            session: r.get(7)?,
                            text: r.get(8)?,
                            data: serde_json::from_str(&r.get::<_, String>(9)?)
                                .unwrap_or(Value::Null),
                        })
                    })?;
                    let mut evs = Vec::new();
                    for r in rows {
                        evs.push(r?);
                    }
                    Ok(evs)
                })()
                .unwrap_or_default();
                drop(c);

                let mut active = true;
                for ev in replayed {
                    if filter.as_ref().is_none_or(|f| f.matches(&ev)) {
                        let notification = json!({
                            "method": "event",
                            "params": {
                                "event": ev
                            }
                        });
                        if tx.send(notification).is_err() {
                            active = false;
                            break;
                        }
                    }
                    last_id = ev.id;
                }

                if active {
                    subscribers.push(Subscriber {
                        tx,
                        last_sent_id: last_id,
                        filter,
                    });
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let current_high: i64 = conn
                    .lock()
                    .unwrap()
                    .query_row("SELECT coalesce(max(id), 0) FROM events", [], |r| r.get(0))
                    .unwrap_or(high_water);
                if current_high > high_water {
                    drain_events_to_subscribers(&conn, &mut subscribers, &mut high_water);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn drain_events_to_subscribers(
    conn: &Arc<Mutex<rusqlite::Connection>>,
    subscribers: &mut Vec<Subscriber>,
    high_water: &mut i64,
) {
    let c = conn.lock().unwrap();
    let new_events = (|| -> Result<Vec<Event>, rusqlite::Error> {
        let mut stmt = c.prepare(
            "SELECT id, at, kind, context, plan, job, attempt, session, text, data
             FROM events WHERE id > ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([*high_water], |r| {
            Ok(Event {
                id: r.get(0)?,
                at: r.get(1)?,
                kind: r.get(2)?,
                context: r.get(3)?,
                plan: r.get(4)?,
                job: r.get(5)?,
                attempt: r.get(6)?,
                session: r.get(7)?,
                text: r.get(8)?,
                data: serde_json::from_str(&r.get::<_, String>(9)?).unwrap_or(Value::Null),
            })
        })?;
        let mut evs = Vec::new();
        for r in rows {
            evs.push(r?);
        }
        Ok(evs)
    })()
    .unwrap_or_default();
    drop(c);

    if new_events.is_empty() {
        return;
    }

    for ev in &new_events {
        subscribers.retain_mut(|sub| {
            if ev.id <= sub.last_sent_id {
                return true;
            }
            if sub.filter.as_ref().is_none_or(|f| f.matches(ev)) {
                let notification = json!({
                    "method": "event",
                    "params": {
                        "event": ev
                    }
                });
                if sub.tx.send(notification).is_err() {
                    return false;
                }
            }
            sub.last_sent_id = ev.id;
            true
        });
        if ev.id > *high_water {
            *high_water = ev.id;
        }
    }
}

fn handle_unix_client(
    stream: UnixStream,
    hub_tx: Sender<HubMsg>,
    conn: Arc<Mutex<rusqlite::Connection>>,
    home: CpdHome,
) {
    let reader_stream = stream.try_clone().expect("clone unix stream");
    let writer = stream;
    let reader = BufReader::new(reader_stream);

    let (push_tx, push_rx) = mpsc::sync_channel::<Value>(256);
    let writer_mutex = Arc::new(Mutex::new(writer));

    // Spawn push thread for writer
    let wm = writer_mutex.clone();
    thread::spawn(move || {
        while let Ok(msg) = push_rx.recv() {
            let mut line = msg.to_string();
            line.push('\n');
            if let Ok(mut w) = wm.lock() {
                if w.write_all(line.as_bytes()).is_err() {
                    break;
                }
            } else {
                break;
            }
        }
    });

    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = process_json_rpc_line(&line, &hub_tx, &conn, &push_tx, &home);
        if let Some(reply) = reply {
            let mut out = reply.to_string();
            out.push('\n');
            let write_failed = writer_mutex
                .lock()
                .map(|mut w| w.write_all(out.as_bytes()).is_err())
                .unwrap_or(true);
            if write_failed {
                break;
            }
        }
    }
}

fn handle_tcp_client(
    mut stream: TcpStream,
    hub_tx: Sender<HubMsg>,
    conn: Arc<Mutex<rusqlite::Connection>>,
    home: CpdHome,
) {
    // 1. Perform HTTP / WebSocket handshake
    let mut reader = BufReader::new(stream.try_clone().expect("clone tcp stream"));
    let mut req_lines = Vec::new();
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).unwrap_or(0);
        if bytes_read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        req_lines.push(line.trim().to_string());
    }

    if req_lines.is_empty() {
        return;
    }

    let first = &req_lines[0];
    let parts: Vec<&str> = first.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "GET" || !parts[1].starts_with("/cpd-ws") {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        return;
    }

    // Origin check per contract §5.8 and ADR 0015:
    // absent, null or loopback accepted; foreign refused.
    let mut origin = None;
    let mut sec_key = None;
    for l in &req_lines[1..] {
        if let Some((k, v)) = l.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim();
            if k == "origin" {
                origin = Some(v.to_string());
            } else if k == "sec-websocket-key" {
                sec_key = Some(v.to_string());
            }
        }
    }

    if origin.as_ref().is_some_and(|orig| !is_origin_allowed(orig)) {
        let _ = stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n");
        return;
    }

    let Some(key) = sec_key else {
        let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
        return;
    };

    // Compute Sec-WebSocket-Accept
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let accept_hash = hasher.finalize();
    let accept_b64 = base64_encode(&accept_hash);

    let handshake_resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept_b64
    );
    if stream.write_all(handshake_resp.as_bytes()).is_err() {
        return;
    }

    // 2. WebSocket frame loop
    let (push_tx, push_rx) = mpsc::sync_channel::<Value>(256);
    let writer_stream = stream.try_clone().expect("clone tcp stream");
    let writer_mutex = Arc::new(Mutex::new(writer_stream));

    let wm = writer_mutex.clone();
    thread::spawn(move || {
        while let Ok(msg) = push_rx.recv() {
            let frame = make_ws_text_frame(&msg.to_string());
            if let Ok(mut w) = wm.lock() {
                if w.write_all(&frame).is_err() {
                    break;
                }
            } else {
                break;
            }
        }
    });

    let mut read_stream = reader;
    loop {
        match read_ws_frame(&mut read_stream) {
            Ok(WsFrame::Text(text)) => {
                let reply = process_json_rpc_line(&text, &hub_tx, &conn, &push_tx, &home);
                if let Some(reply) = reply {
                    let frame = make_ws_text_frame(&reply.to_string());
                    if let Ok(mut w) = writer_mutex.lock() {
                        let _ = w.write_all(&frame);
                        let _ = w.flush();
                    }
                }
            }
            Ok(WsFrame::Close) | Err(_) => break,
            Ok(WsFrame::Ping(data)) => {
                let pong = make_ws_control_frame(0xA, &data);
                if let Ok(mut w) = writer_mutex.lock() {
                    let _ = w.write_all(&pong);
                    let _ = w.flush();
                }
            }
        }
    }
}

fn is_origin_allowed(origin: &str) -> bool {
    if origin == "null" {
        return true;
    }
    if let Some(stripped) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    {
        let host = stripped
            .split(':')
            .next()
            .unwrap_or(stripped)
            .split('/')
            .next()
            .unwrap_or(stripped);
        if host == "localhost" || host == "127.0.0.1" || host == "[::1]" {
            return true;
        }
    }
    false
}

fn process_json_rpc_line(
    line: &str,
    hub_tx: &Sender<HubMsg>,
    conn: &Arc<Mutex<rusqlite::Connection>>,
    push_tx: &SyncSender<Value>,
    home: &CpdHome,
) -> Option<Value> {
    let Ok(val) = serde_json::from_str::<Value>(line) else {
        return Some(json!({
            "id": null,
            "result": null,
            "error": {"code": "bad_request", "message": "malformed JSON"}
        }));
    };

    let id = val.get("id").cloned();
    let method = val.get("method").and_then(|m| m.as_str());

    match method {
        Some("events.committed") => {
            let _ = hub_tx.send(HubMsg::Wake);
            None
        }
        Some("snapshot") => {
            let c = conn.lock().unwrap();
            let snapshot = (|| -> Result<Value, rusqlite::Error> {
                let cursor: i64 =
                    c.query_row("SELECT coalesce(max(id), 0) FROM events", [], |r| r.get(0))?;
                // Query active Contexts, Plans, Jobs
                let mut ctx_stmt = c.prepare("SELECT id, title FROM contexts ORDER BY id")?;
                let contexts = ctx_stmt
                    .query_map([], |r| {
                        Ok(json!({ "id": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)? }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;

                let mut plan_stmt = c.prepare("SELECT id, title, phase FROM plans ORDER BY id")?;
                let plans = plan_stmt
                    .query_map([], |r| {
                        Ok(json!({
                            "id": r.get::<_, String>(0)?,
                            "title": r.get::<_, String>(1)?,
                            "phase": r.get::<_, String>(2)?
                        }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;

                let mut job_stmt =
                    c.prepare("SELECT plan, key, status FROM jobs ORDER BY plan, key")?;
                let jobs = job_stmt
                    .query_map([], |r| {
                        Ok(json!({
                            "plan": r.get::<_, String>(0)?,
                            "key": r.get::<_, String>(1)?,
                            "status": r.get::<_, String>(2)?
                        }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;

                let owners: Vec<String> =
                    cpd_core::policy::known_owners(&cpd_core::policy::policy_dir(home))
                        .into_iter()
                        .map(|o| o.as_str().to_owned())
                        .collect();

                Ok(json!({
                    "contexts": contexts,
                    "plans": plans,
                    "jobs": jobs,
                    "cursor": cursor,
                    "owners": owners
                }))
            })();
            drop(c);

            match snapshot {
                Ok(result) => Some(json!({
                    "id": id,
                    "result": result,
                    "error": null
                })),
                Err(e) => Some(json!({
                    "id": id,
                    "result": null,
                    "error": {"code": "store_failed", "message": e.to_string()}
                })),
            }
        }
        Some("events.subscribe") => {
            let params = val.get("params").cloned().unwrap_or(json!({}));
            let cursor = params.get("cursor").and_then(|c| c.as_i64()).unwrap_or(0);
            let filter_val = params.get("filter");
            let mut filter = EventFilter::default();
            if let Some(f) = filter_val {
                if let Some(p) = f.get("plan").and_then(|v| v.as_str()) {
                    filter.plan = PlanId::parse(p).ok();
                }
                if let Some(c) = f.get("context").and_then(|v| v.as_str()) {
                    filter.context = ContextId::parse(c).ok();
                }
                if let Some(s) = f.get("session").and_then(|v| v.as_str()) {
                    filter.session = SessionId::parse(s).ok();
                }
                if let Some(kinds) = f.get("kind").and_then(|v| v.as_array()) {
                    for k in kinds {
                        if let Some(ks) = k.as_str() {
                            filter.kinds.insert(ks.to_string());
                        }
                    }
                }
            }

            let _ = hub_tx.send(HubMsg::Subscribe {
                tx: push_tx.clone(),
                cursor,
                filter: if filter_val.is_some() {
                    Some(filter)
                } else {
                    None
                },
            });

            Some(json!({
                "id": id,
                "result": {},
                "error": null
            }))
        }
        Some(_) => Some(json!({
            "id": id,
            "result": null,
            "error": {"code": "unknown_method", "message": "Method not found"}
        })),
        None => Some(json!({
            "id": id,
            "result": null,
            "error": {"code": "bad_request", "message": "Missing method"}
        })),
    }
}

enum WsFrame {
    Text(String),
    Ping(Vec<u8>),
    Close,
}

fn read_ws_frame(stream: &mut impl Read) -> io::Result<WsFrame> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header)?;

    let opcode = header[0] & 0x0F;
    let masked = (header[1] & 0x80) != 0;
    let mut payload_len = (header[1] & 0x7F) as usize;

    if payload_len == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext)?;
        payload_len = u16::from_be_bytes(ext) as usize;
    } else if payload_len == 127 {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext)?;
        payload_len = u64::from_be_bytes(ext) as usize;
    }

    let mask = if masked {
        let mut m = [0u8; 4];
        stream.read_exact(&mut m)?;
        Some(m)
    } else {
        None
    };

    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload)?;

    if let Some(mask) = mask {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[i % 4];
        }
    }

    match opcode {
        0x1 => {
            let text = String::from_utf8(payload)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(WsFrame::Text(text))
        }
        0x8 => Ok(WsFrame::Close),
        0x9 => Ok(WsFrame::Ping(payload)),
        _ => Ok(WsFrame::Close),
    }
}

fn make_ws_text_frame(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut frame = Vec::new();
    frame.push(0x81); // FIN + Text

    let len = bytes.len();
    if len <= 125 {
        frame.push(len as u8);
    } else if len <= 65535 {
        frame.push(126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }

    frame.extend_from_slice(bytes);
    frame
}

fn make_ws_control_frame(opcode: u8, data: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.push(0x80 | (opcode & 0x0F));
    frame.push(data.len() as u8);
    frame.extend_from_slice(data);
    frame
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as usize;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as usize
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as usize
        } else {
            0
        };

        result.push(TABLE[(b0 >> 2) & 0x3F] as char);
        result.push(TABLE[((b0 << 4) | (b1 >> 4)) & 0x3F] as char);

        if i + 1 < data.len() {
            result.push(TABLE[((b1 << 2) | (b2 >> 6)) & 0x3F] as char);
        } else {
            result.push('=');
        }

        if i + 2 < data.len() {
            result.push(TABLE[b2 & 0x3F] as char);
        } else {
            result.push('=');
        }

        i += 3;
    }
    result
}
