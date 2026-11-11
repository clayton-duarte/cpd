//! Cross-machine probes for cpd doctor (web/CONTRACT.md §31).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::json;

use super::check::{Check, Severity};
use super::deadline;
use cpd_core::config::Config;
use cpd_core::home::CpdHome;

const PORTS: [u16; 2] = [3333, 33333];

struct FleetMachine {
    name: String,
    host: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeOutcome {
    Reachable {
        status: u16,
    },
    Unreachable {
        reason: &'static str,
        message: Option<String>,
    },
}

type MockProbeFn = Box<dyn Fn(&str, u16, Duration) -> ProbeOutcome>;

thread_local! {
    static MOCK_PROBE: RefCell<Option<MockProbeFn>> = const { RefCell::new(None) };
}

/// Scopes a mock probe function for testing cross-machine checks without real network calls.
pub fn with_mock_probe<R>(
    probe_fn: impl Fn(&str, u16, Duration) -> ProbeOutcome + 'static,
    f: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<MockProbeFn>);
    impl Drop for Restore {
        fn drop(&mut self) {
            MOCK_PROBE.with(|cell| *cell.borrow_mut() = self.0.take());
        }
    }
    let prev = MOCK_PROBE.with(|cell| cell.borrow_mut().replace(Box::new(probe_fn)));
    let _guard = Restore(prev);
    f()
}

/// Runs the `cross_machine` check if fleet is configured.
pub fn check(home: &CpdHome, config: Option<&Config>) -> Option<Check> {
    let machines = parse_fleet(home, config)?;
    if machines.is_empty() {
        return None;
    }

    let timeout = deadline();

    // Check if a mock probe is set in thread-local
    let has_mock = MOCK_PROBE.with(|cell| cell.borrow().is_some());

    let results = if has_mock {
        // Run mock probes sequentially on current thread to use thread_local
        let mut res = Vec::new();
        for (m_idx, m) in machines.iter().enumerate() {
            for (p_idx, &port) in PORTS.iter().enumerate() {
                let outcome =
                    MOCK_PROBE.with(|cell| cell.borrow().as_ref().unwrap()(&m.host, port, timeout));
                res.push((m_idx, p_idx, outcome));
            }
        }
        res
    } else {
        // Run real probes concurrently across threads (CONTRACT §31.3)
        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();
        for (m_idx, m) in machines.iter().enumerate() {
            for (p_idx, &port) in PORTS.iter().enumerate() {
                let host = m.host.clone();
                let tx = tx.clone();
                let handle = thread::spawn(move || {
                    let outcome = real_probe(&host, port, timeout);
                    let _ = tx.send((m_idx, p_idx, outcome));
                });
                handles.push(handle);
            }
        }
        drop(tx);

        let mut res = Vec::new();
        while let Ok(item) = rx.recv() {
            res.push(item);
        }
        for handle in handles {
            let _ = handle.join();
        }
        res
    };

    // Sort probe results by machine order, then port order (CONTRACT §31.4)
    let mut probe_map = BTreeMap::new();
    for (m_idx, p_idx, outcome) in results {
        probe_map.insert((m_idx, p_idx), outcome);
    }

    let mut found = Vec::new();
    let mut unreachable_summaries = Vec::new();

    for (m_idx, m) in machines.iter().enumerate() {
        for (p_idx, &port) in PORTS.iter().enumerate() {
            if let Some(outcome) = probe_map.remove(&(m_idx, p_idx)) {
                match outcome {
                    ProbeOutcome::Reachable { status } => {
                        found.push(json!({
                            "machine": m.name,
                            "host": m.host,
                            "port": port,
                            "reachable": true,
                            "status": status,
                        }));
                    }
                    ProbeOutcome::Unreachable { reason, message } => {
                        unreachable_summaries.push(format!("{}:{} {}", m.name, port, reason));
                        let mut obj = json!({
                            "machine": m.name,
                            "host": m.host,
                            "port": port,
                            "reachable": false,
                            "reason": reason,
                        });
                        if reason == "other"
                            && let Some(msg) = message
                        {
                            obj["message"] = json!(msg);
                        }
                        found.push(obj);
                    }
                }
            }
        }
    }

    let ok = unreachable_summaries.is_empty();
    let total_probes = machines.len() * PORTS.len();
    let mut c = if ok {
        Check::pass("cross_machine", Severity::Warn)
    } else {
        let msg = format!(
            "{} of {} fleet probes unreachable: {}",
            unreachable_summaries.len(),
            total_probes,
            unreachable_summaries.join(", ")
        );
        Check::fail("cross_machine", Severity::Warn, msg)
    };

    c = c
        .expected("every fleet machine answers on 3333 and 33333")
        .found(found);
    Some(c)
}

/// Parses `[fleet.<machine>]` tables from config.toml.
fn parse_fleet(home: &CpdHome, config: Option<&Config>) -> Option<Vec<FleetMachine>> {
    let path = home.config();
    let text = fs::read_to_string(&path).ok()?;

    let mut fleet_map = BTreeMap::new();
    let mut current_section: Option<String> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let sec = trimmed[1..trimmed.len() - 1].trim();
            if let Some(machine_key) = sec.strip_prefix("fleet.") {
                let m = machine_key.trim();
                current_section = Some(m.to_owned());
                fleet_map.entry(m.to_owned()).or_insert_with(String::new);
            } else {
                current_section = None;
            }
            continue;
        }

        if let Some(ref m) = current_section
            && let Some((k, v)) = trimmed.split_once('=')
            && k.trim() == "host"
        {
            let host_val = v.trim().trim_matches('"').trim_matches('\'').trim();
            fleet_map.insert(m.clone(), host_val.to_owned());
        }
    }

    if fleet_map.is_empty() {
        return None;
    }

    let mut machines = Vec::new();
    for (name, host) in fleet_map {
        if host.is_empty() {
            continue;
        }
        if let Some(c) = config
            && name.as_str() == c.machine.as_str()
        {
            continue;
        }
        machines.push(FleetMachine { name, host });
    }

    if machines.is_empty() {
        None
    } else {
        Some(machines)
    }
}

fn real_probe(host: &str, port: u16, timeout: Duration) -> ProbeOutcome {
    // Resolve address
    let addrs: Vec<_> = match format!("{host}:{port}").to_socket_addrs() {
        Ok(iter) => iter.collect(),
        Err(_) => {
            return ProbeOutcome::Unreachable {
                reason: "unresolved",
                message: None,
            };
        }
    };

    if addrs.is_empty() {
        return ProbeOutcome::Unreachable {
            reason: "unresolved",
            message: None,
        };
    }

    // Connect with timeout
    let mut stream = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => {
                let kind = e.kind();
                if kind == std::io::ErrorKind::ConnectionRefused {
                    return ProbeOutcome::Unreachable {
                        reason: "refused",
                        message: None,
                    };
                } else if kind == std::io::ErrorKind::TimedOut {
                    return ProbeOutcome::Unreachable {
                        reason: "timeout",
                        message: None,
                    };
                }
            }
        }
    }

    let mut stream = match stream {
        Some(s) => s,
        None => {
            return ProbeOutcome::Unreachable {
                reason: "refused",
                message: None,
            };
        }
    };

    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    // Send HTTP/1.1 GET request: no Origin, no credentials, redirects not followed (CONTRACT §31.3)
    let request = format!("GET / HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    if let Err(e) = stream.write_all(request.as_bytes()) {
        let kind = e.kind();
        if kind == std::io::ErrorKind::TimedOut || kind == std::io::ErrorKind::WouldBlock {
            return ProbeOutcome::Unreachable {
                reason: "timeout",
                message: None,
            };
        }
        return ProbeOutcome::Unreachable {
            reason: "other",
            message: Some(e.to_string()),
        };
    }

    // Read status line
    let mut buf = [0u8; 1024];
    let mut received = Vec::new();
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                received.extend_from_slice(&buf[..n]);
                if received.windows(2).any(|w| w == b"\r\n") || received.contains(&b'\n') {
                    break;
                }
            }
            Err(e) => {
                let kind = e.kind();
                if kind == std::io::ErrorKind::TimedOut || kind == std::io::ErrorKind::WouldBlock {
                    return ProbeOutcome::Unreachable {
                        reason: "timeout",
                        message: None,
                    };
                }
                return ProbeOutcome::Unreachable {
                    reason: "other",
                    message: Some(e.to_string()),
                };
            }
        }
    }

    if received.is_empty() {
        return ProbeOutcome::Unreachable {
            reason: "no_http_response",
            message: None,
        };
    }

    let header_str = String::from_utf8_lossy(&received);
    let first_line = match header_str.lines().next() {
        Some(line) => line,
        None => {
            return ProbeOutcome::Unreachable {
                reason: "no_http_response",
                message: None,
            };
        }
    };

    // Parse status line, e.g. "HTTP/1.1 200 OK"
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() >= 2
        && parts[0].starts_with("HTTP/")
        && let Ok(code) = parts[1].parse::<u16>()
    {
        return ProbeOutcome::Reachable { status: code };
    }

    ProbeOutcome::Unreachable {
        reason: "no_http_response",
        message: None,
    }
}
