//! The stall rule of A4 (contract §3.4, hs-d195): a Worker's tool call whose
//! session file has been quiet for `timeouts.stall_seconds` has the processes
//! pi started for it killed, so pi gets the tool's failure and the Worker
//! continues.
//!
//! What is killed: every descendant of the Worker's pi process, never pi
//! itself, as §3.4 states. pi 1.3.0's bash tool spawns its shell with
//! `detached: true` (`dist/core/tools/bash.js`), so each command runs in its
//! own session and process group, and killing that group would end the
//! command. The descendant walk is used instead: it is what the contract
//! says, and it also reaches a process that left the group (`setsid`) and the
//! processes of tools other than bash. Descendants are read from the parent
//! ids of `/proc/*/stat` (Linux; elsewhere none is found).

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::host::spawn::{Pid as SpawnPid, ProcessSpawn, Spawn};
use serde_json::Value;

/// How long the processes get between `SIGTERM` and `SIGKILL`.
const GRACE: Duration = Duration::from_secs(10);

/// Whether pi is inside a tool call, by its session file `text`: the last
/// assistant message has a `toolCall` with no `toolResult` for it after it.
/// False for a line that does not parse (one being written) and for a file
/// with no assistant message.
pub fn in_tool_call(text: &str) -> bool {
    let mut results: HashSet<String> = HashSet::new();
    for line in text.lines().rev().filter(|l| !l.trim().is_empty()) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else { return false };
        if entry["type"] != "message" {
            continue;
        }
        let message = &entry["message"];
        match message["role"].as_str() {
            Some("toolResult") => {
                if let Some(id) = message["toolCallId"].as_str() {
                    results.insert(id.to_owned());
                }
            }
            Some("assistant") => {
                let Some(content) = message["content"].as_array() else { return false };
                return content
                    .iter()
                    .filter(|c| c["type"] == "toolCall")
                    .any(|c| c["id"].as_str().is_none_or(|id| !results.contains(id)));
            }
            _ => {}
        }
    }
    false
}

/// The stall check of one Worker, across A4's ticks.
pub struct Watch {
    /// `timeouts.stall_seconds`; 0 is off.
    limit: Duration,
    /// The modification time of the file at the last kill: a stall is killed
    /// once, a new one needs a new write.
    killed_at: Option<SystemTime>,
    /// The last tick at which a check span was open (contract §7.7): the Worker's pi sat in `attempt submit`, silent, so the quiet
    /// of the session file counts from then at the earliest. Without it a Gate that ran past the limit would be killed in the instant
    /// the span ends, before its tool result was written.
    span_seen_at: Option<SystemTime>,
}

impl Watch {
    pub fn new(stall_seconds: u32) -> Watch {
        Watch { limit: Duration::from_secs(u64::from(stall_seconds)), killed_at: None, span_seen_at: None }
    }

    /// One tick: when `file` has been quiet for the limit and pi (`pi`) is
    /// inside a tool call, kills pi's descendants and returns the driver-log
    /// line; None when there is nothing to do. While a check span is open
    /// (`span_open`, §7.7) nothing is killed and the quiet starts over.
    pub fn check(&mut self, file: &Path, pi: u32, span_open: bool) -> Option<String> {
        if self.limit.is_zero() {
            return None;
        }
        if span_open {
            self.span_seen_at = Some(SystemTime::now());
            return None;
        }
        let modified = std::fs::metadata(file).and_then(|m| m.modified()).ok()?;
        let modified = self.span_seen_at.map_or(modified, |seen| modified.max(seen));
        let quiet = SystemTime::now().duration_since(modified).unwrap_or_default();
        if quiet < self.limit || self.killed_at == Some(modified) {
            return None;
        }
        if !std::fs::read_to_string(file).is_ok_and(|text| in_tool_call(&text)) {
            return None;
        }
        let pids = descendants(pi);
        if pids.is_empty() {
            return None;
        }
        self.killed_at = Some(modified);
        let named: Vec<String> = pids.iter().map(|&p| format!("{p} ({})", cmdline(p))).collect();
        let forced = terminate(&pids);
        let mut line = format!(
            "A4: stall: the Worker's tool call has been idle {} s; killed pid {}",
            quiet.as_secs(),
            named.join(", pid ")
        );
        if !forced.is_empty() {
            let forced: Vec<String> = forced.iter().map(u32::to_string).collect();
            line += &format!("; SIGKILL after {} s to pid {}", GRACE.as_secs(), forced.join(", "));
        }
        Some(line)
    }
}

/// `pid`'s parent and state ('Z' for a zombie) from `libproc`'s BSD info.
#[cfg(target_os = "macos")]
fn stat(pid: u32) -> Option<(u32, char)> {
    // SZOMB, <sys/proc.h>: the process has ended and waits to be reaped.
    const SZOMB: u32 = 5;
    let info = libproc::proc_pid::pidinfo::<libproc::bsd_info::BSDInfo>(i32::try_from(pid).ok()?, 0).ok()?;
    Some((info.pbi_ppid, if info.pbi_status == SZOMB { 'Z' } else { 'R' }))
}

/// `pid`'s parent and state from `/proc/<pid>/stat`.
#[cfg(not(target_os = "macos"))]
fn stat(pid: u32) -> Option<(u32, char)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `pid (comm) state ppid …`; comm may hold spaces and parentheses.
    let rest = &text[text.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let ppid = fields.next()?.parse().ok()?;
    Some((ppid, state))
}

/// Every pid: `libproc`'s process list on macOS, the numbers in `/proc` elsewhere.
#[cfg(target_os = "macos")]
fn all_pids() -> Vec<u32> {
    libproc::processes::pids_by_type(libproc::processes::ProcFilter::All).unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
fn all_pids() -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok()).collect()
}

/// Every live descendant of `root`, not `root` itself, in pid order.
fn descendants(root: u32) -> Vec<u32> {
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for pid in all_pids() {
        if let Some((ppid, state)) = stat(pid)
            && state != 'Z'
        {
            children.entry(ppid).or_default().push(pid);
        }
    }
    let mut found = Vec::new();
    let mut todo = vec![root];
    while let Some(parent) = todo.pop() {
        for &child in children.get(&parent).into_iter().flatten() {
            found.push(child);
            todo.push(child);
        }
    }
    found.sort_unstable();
    found
}

/// `pid`'s command line, its arguments joined by spaces. macOS: `libproc` has
/// no argv reader (only the name and path), so `ps -o command=` gives it.
#[cfg(target_os = "macos")]
fn cmdline(pid: u32) -> String {
    let out = std::process::Command::new("ps").args(["-o", "command=", "-p", &pid.to_string()]).output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// `pid`'s command line from `/proc/<pid>/cmdline`, its arguments joined by spaces.
#[cfg(not(target_os = "macos"))]
fn cmdline(pid: u32) -> String {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let words: Vec<String> =
        raw.split(|&b| b == 0).filter(|w| !w.is_empty()).map(|w| String::from_utf8_lossy(w).into_owned()).collect();
    words.join(" ")
}

fn signal(pid: u32, signal: Signal) {
    // A process already gone is what we want.
    let _ = kill(Pid::from_raw(pid as i32), signal);
}

/// `SIGTERM` to `pids`, then `SIGKILL` to those still alive after
/// [`GRACE`]; returns the pids that needed `SIGKILL`.
fn terminate(pids: &[u32]) -> Vec<u32> {
    for &pid in pids {
        signal(pid, Signal::SIGTERM);
    }
    let deadline = Instant::now() + GRACE;
    while Instant::now() < deadline {
        if !pids.iter().any(|&p| ProcessSpawn.alive(SpawnPid(p))) {
            return Vec::new();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let left: Vec<u32> = pids.iter().copied().filter(|&p| ProcessSpawn.alive(SpawnPid(p))).collect();
    for &pid in &left {
        signal(pid, Signal::SIGKILL);
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = r#"{"type":"session"}"#;
    const CALL: &str = r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"x"},{"type":"toolCall","id":"c1","name":"bash"}],"stopReason":"toolUse"}}"#;
    const RESULT: &str = r#"{"type":"message","message":{"role":"toolResult","toolCallId":"c1","content":[]}}"#;
    const TEXT: &str = r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"done"}],"stopReason":"stop"}}"#;
    const TWO: &str = r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"c1"},{"type":"toolCall","id":"c2"}]}}"#;
    const RESULT_2: &str = r#"{"type":"message","message":{"role":"toolResult","toolCallId":"c2","content":[]}}"#;
    const OTHER: &str = r#"{"type":"session_info","name":"x"}"#;

    #[test]
    fn a_tool_call_without_its_result_is_inside_a_call() {
        assert!(in_tool_call(&[SESSION, CALL].join("\n")));
        assert!(in_tool_call(&[SESSION, CALL, OTHER, ""].join("\n")), "other entries after it do not count");
    }

    #[test]
    fn a_tool_call_with_its_result_is_not() {
        assert!(!in_tool_call(&[SESSION, CALL, RESULT].join("\n")));
    }

    #[test]
    fn a_call_of_several_is_inside_while_one_has_no_result() {
        assert!(in_tool_call(&[TWO, RESULT_2].join("\n")));
        assert!(!in_tool_call(&[TWO, RESULT_2, RESULT].join("\n")));
    }

    #[test]
    fn text_only_an_empty_file_or_a_partial_line_is_not() {
        assert!(!in_tool_call(&[SESSION, CALL, RESULT, TEXT].join("\n")));
        assert!(!in_tool_call(""));
        assert!(!in_tool_call(SESSION));
        assert!(!in_tool_call(&[SESSION, CALL, r#"{"type":"mess"#].join("\n")));
    }

    /// A session file that has been inside a tool call for an hour, and a stand-in for pi: a shell with a child of its own.
    fn stalled() -> (tempfile::TempDir, std::path::PathBuf, std::process::Child) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.jsonl");
        std::fs::write(&file, [SESSION, CALL].join("\n")).unwrap();
        let hour_ago = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(hour_ago).unwrap();
        let pi = std::process::Command::new("sh").args(["-c", "sleep 30 & wait"]).spawn().unwrap();
        // Its child exists once `sh` has forked it.
        for _ in 0..200 {
            if !descendants(pi.id()).is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        (dir, file, pi)
    }

    #[test]
    fn a_stall_is_killed_unless_a_check_span_is_open() {
        let (_dir, file, mut pi) = stalled();
        let child = descendants(pi.id());
        assert_eq!(child.len(), 1, "{child:?}");
        let mut watch = Watch::new(60);
        assert_eq!(watch.check(&file, pi.id(), true), None, "an open span is not a stall");
        assert!(ProcessSpawn.alive(SpawnPid(child[0])));
        // The span ended a moment ago: the quiet counts from the last tick that saw it, not from the file.
        assert_eq!(watch.check(&file, pi.id(), false), None);
        assert!(ProcessSpawn.alive(SpawnPid(child[0])));
        // With no span ever seen, the same file is a stall.
        let line = Watch::new(60).check(&file, pi.id(), false).expect("killed");
        assert!(line.contains(&child[0].to_string()), "{line}");
        pi.wait().unwrap();
    }

    #[test]
    fn descendants_are_found_and_the_root_is_not_one() {
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let me = std::process::id();
        let found = descendants(me);
        assert!(found.contains(&child.id()), "{found:?}");
        assert!(!found.contains(&me));
        assert_eq!(cmdline(child.id()), "sleep 30");
        assert!(terminate(&[child.id()]).is_empty());
        child.wait().unwrap();
    }
}
