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
use std::time::{Duration, SystemTime};

use crate::host::spawn::{Pid as SpawnPid, Signal};
use crate::ports::Ports;
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
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            return false;
        };
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
                let Some(content) = message["content"].as_array() else {
                    return false;
                };
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
        Watch {
            limit: Duration::from_secs(u64::from(stall_seconds)),
            killed_at: None,
            span_seen_at: None,
        }
    }

    /// One tick: when `file` has been quiet for the limit and pi (`pi`) is
    /// inside a tool call, kills pi's descendants and returns the driver-log
    /// line; None when there is nothing to do. While a check span is open, or an
    /// Escalation of the Attempt is `open` (`span_open`, §7.7: true for either), nothing is killed and the quiet starts over.
    /// [`Watch::check`] on `ports`: the quiet of `file` is its modification time against `ports.clock.utc()`, the signals
    /// and the waits of `terminate_with` go to `ports.spawn` and naps of `ports.clock`, the command lines come from
    /// `ports.spawn.cmdline`, and `descendants` names pi's live descendants (the `/proc` or `libproc` walk, in production).
    pub(super) fn check_with(
        &mut self,
        ports: &Ports,
        descendants: &dyn Fn(u32) -> Vec<u32>,
        file: &Path,
        pi: u32,
        span_open: bool,
    ) -> Option<String> {
        if self.limit.is_zero() {
            return None;
        }
        let now = SystemTime::from(ports.clock.utc());
        if span_open {
            self.span_seen_at = Some(now);
            return None;
        }
        let modified = std::fs::metadata(file).and_then(|m| m.modified()).ok()?;
        let modified = self
            .span_seen_at
            .map_or(modified, |seen| modified.max(seen));
        let quiet = now.duration_since(modified).unwrap_or_default();
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
        let named: Vec<String> = pids
            .iter()
            .map(|&p| {
                format!(
                    "{p} ({})",
                    ports.spawn.cmdline(SpawnPid(p)).unwrap_or_default()
                )
            })
            .collect();
        let forced = terminate_with(ports, &pids);
        let mut line = format!(
            "A4: stall: the Worker's tool call has been idle {} s; killed pid {}",
            quiet.as_secs(),
            named.join(", pid ")
        );
        if !forced.is_empty() {
            let forced: Vec<String> = forced.iter().map(u32::to_string).collect();
            line += &format!(
                "; SIGKILL after {} s to pid {}",
                GRACE.as_secs(),
                forced.join(", ")
            );
        }
        Some(line)
    }
}

/// `pid`'s parent and state ('Z' for a zombie) from `libproc`'s BSD info.
#[cfg(target_os = "macos")]
pub(crate) fn stat(pid: u32) -> Option<(u32, char)> {
    // SZOMB, <sys/proc.h>: the process has ended and waits to be reaped.
    const SZOMB: u32 = 5;
    let info =
        libproc::proc_pid::pidinfo::<libproc::bsd_info::BSDInfo>(i32::try_from(pid).ok()?, 0)
            .ok()?;
    Some((
        info.pbi_ppid,
        if info.pbi_status == SZOMB { 'Z' } else { 'R' },
    ))
}

/// `pid`'s parent and state from `/proc/<pid>/stat`.
#[cfg(not(target_os = "macos"))]
pub(crate) fn stat(pid: u32) -> Option<(u32, char)> {
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
pub(crate) fn all_pids() -> Vec<u32> {
    libproc::processes::pids_by_type(libproc::processes::ProcFilter::All).unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn all_pids() -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .collect()
}

/// Every live descendant of `root`, not `root` itself, in pid order.
pub(crate) fn descendants(root: u32) -> Vec<u32> {
    descendants_in(
        root,
        all_pids()
            .into_iter()
            .filter_map(|pid| stat(pid).map(|(ppid, state)| (pid, ppid, state))),
    )
}

/// [`descendants`] over a table of `(pid, parent pid, state)`: a zombie is not a descendant, and nothing under it is
/// reached through it.
fn descendants_in(root: u32, procs: impl IntoIterator<Item = (u32, u32, char)>) -> Vec<u32> {
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (pid, ppid, state) in procs {
        if state != 'Z' {
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

/// `SIGTERM` to `pids`, then `SIGKILL` to those still alive after
/// [`GRACE`]; returns the pids that needed `SIGKILL`. Signals and the liveness test go to `ports.spawn`, the wait is
/// naps of 100 ms on `ports.clock`.
fn terminate_with(ports: &Ports, pids: &[u32]) -> Vec<u32> {
    let (clock, spawn) = (&*ports.clock, &*ports.spawn);
    for &pid in pids {
        // A process already gone is what we want.
        let _ = spawn.signal(SpawnPid(pid), Signal::Term);
    }
    let deadline = clock.now() + GRACE;
    while clock.now() < deadline {
        if !pids.iter().any(|&p| spawn.alive(SpawnPid(p))) {
            return Vec::new();
        }
        clock.sleep(Duration::from_millis(100));
    }
    let left: Vec<u32> = pids
        .iter()
        .copied()
        .filter(|&p| spawn.alive(SpawnPid(p)))
        .collect();
    for &pid in &left {
        let _ = spawn.signal(SpawnPid(pid), Signal::Kill);
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Clock;
    use crate::driver::ports_tests::{modified_at, process};
    use crate::host::spawn::Spawn;
    use crate::ports::Fakes;

    const SESSION: &str = r#"{"type":"session"}"#;
    const CALL: &str = r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"x"},{"type":"toolCall","id":"c1","name":"bash"}],"stopReason":"toolUse"}}"#;
    const RESULT: &str =
        r#"{"type":"message","message":{"role":"toolResult","toolCallId":"c1","content":[]}}"#;
    const TEXT: &str = r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"done"}],"stopReason":"stop"}}"#;
    const TWO: &str = r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"c1"},{"type":"toolCall","id":"c2"}]}}"#;
    const RESULT_2: &str =
        r#"{"type":"message","message":{"role":"toolResult","toolCallId":"c2","content":[]}}"#;
    const OTHER: &str = r#"{"type":"session_info","name":"x"}"#;

    #[test]
    fn a_tool_call_without_its_result_is_inside_a_call() {
        assert!(in_tool_call(&[SESSION, CALL].join("\n")));
        assert!(
            in_tool_call(&[SESSION, CALL, OTHER, ""].join("\n")),
            "other entries after it do not count"
        );
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
        assert!(!in_tool_call(
            &[SESSION, CALL, r#"{"type":"mess"#].join("\n")
        ));
    }

    /// A session file that has been inside a tool call for an hour of virtual time, and a stand-in for pi on the fake: a process
    /// with a child of its own (the table the walk reads is `descendants_in`'s argument, not `/proc`).
    fn stalled(fakes: &Fakes) -> (tempfile::TempDir, std::path::PathBuf, SpawnPid, SpawnPid) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.jsonl");
        std::fs::write(&file, [SESSION, CALL].join("\n")).unwrap();
        modified_at(&file, fakes.clock.utc());
        fakes.clock.advance(Duration::from_secs(3600));
        let (pi, child) = (process(&fakes.spawn), process(&fakes.spawn));
        fakes.spawn.set_cmdline(child, "sleep 30");
        (dir, file, pi, child)
    }

    /// The process table of that stand-in: `sh` (pi) with the `sleep` it forked, and what is no descendant.
    fn table(pi: SpawnPid, child: SpawnPid) -> Vec<(u32, u32, char)> {
        vec![
            (pi.0, 1, 'S'),
            (child.0, pi.0, 'S'),
            (child.0 + 1000, 1, 'S'),
        ]
    }

    #[test]
    fn a_stall_is_killed_unless_a_check_span_is_open() {
        let fakes = Fakes::new();
        let (_dir, file, pi, child) = stalled(&fakes);
        let kids = |root: u32| descendants_in(root, table(pi, child));
        assert_eq!(kids(pi.0), [child.0]);
        let mut watch = Watch::new(60);
        assert_eq!(
            watch.check_with(&fakes.ports, &kids, &file, pi.0, true),
            None,
            "an open span is not a stall"
        );
        assert!(fakes.spawn.alive(child));
        // The span ended a moment ago: the quiet counts from the last tick that saw it, not from the file.
        assert_eq!(
            watch.check_with(&fakes.ports, &kids, &file, pi.0, false),
            None
        );
        assert!(fakes.spawn.alive(child));
        // With no span ever seen, the same file is a stall.
        let line = Watch::new(60)
            .check_with(&fakes.ports, &kids, &file, pi.0, false)
            .expect("killed");
        assert!(line.contains(&child.0.to_string()), "{line}");
        assert!(line.contains("(sleep 30)"), "{line}");
        assert!(!fakes.spawn.alive(child), "the descendant was terminated");
        assert!(fakes.spawn.alive(pi), "never pi itself");
    }

    #[test]
    fn descendants_are_found_and_the_root_is_not_one() {
        let fakes = Fakes::new();
        let (root, child) = (process(&fakes.spawn), process(&fakes.spawn));
        let (grandchild, zombie, stranger) = (2000, 2001, 2002);
        let procs = vec![
            (root.0, 1, 'S'),
            (child.0, root.0, 'S'),
            (grandchild, child.0, 'R'),
            (zombie, root.0, 'Z'),
            (stranger, 1, 'S'),
        ];
        let found = descendants_in(root.0, procs.clone());
        assert_eq!(
            found,
            [child.0, grandchild],
            "the whole subtree, in pid order, with no zombie and no stranger"
        );
        assert!(!found.contains(&root.0));
        assert_eq!(descendants_in(child.0, procs), [grandchild]);
        fakes.spawn.set_cmdline(child, "sleep 30");
        assert_eq!(fakes.spawn.cmdline(child).as_deref(), Some("sleep 30"));
        assert!(terminate_with(&fakes.ports, &[child.0]).is_empty());
        assert!(!fakes.spawn.alive(child));
    }
}
