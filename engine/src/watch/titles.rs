//! E10, Session titles (contract §3.7; hs-d257): starts `cpd session retitle <id> --width <cols>` (the chat slot's width, hs-d543; no slot, no run) for each Lead or Manager
//! Session that is idle, whose latest finished turn is newer than its title (or that has none) and whose
//! title is at least 10 minutes old. It only decides and spawns, as E9 does; the run holds
//! `locks/title-<session>.lock`, runs pi and writes the title itself. What E10 adds is the caps (one run
//! per Session, two machine-wide) and the report of a run that failed.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::detached_launch;
use super::engine::{Engine, incident_holds, stopping};
use super::report::{About, Failure, Step};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::chat_slot;
use cpd_core::host::spawn::Pid;
use cpd_core::ids::SessionId;

/// A title is rewritten at most this often per Session.
const THROTTLE: time::Duration = time::Duration::minutes(10);

/// Retitle runs in flight, machine-wide, at most.
const AT_ONCE: usize = 2;

/// A Session's titling is reported (an `error` Event) only once it has failed this long in a row, no success in
/// between (hs-d356). Until then a failed run is logged and retried.
const FAILING: Duration = Duration::from_secs(10 * 60);

/// How many lines of a failed run's output the `error` Event carries.
const TAIL_LINES: usize = 20;

/// A retitle run the engine started.
struct Run {
    pid: Pid,
    /// The length of its log when it started: what it wrote is what follows.
    offset: u64,
}

/// What E10 remembers: the runs it started and has not seen end, when each Session's titling began failing (until
/// a run succeeds), and the Sessions it reported.
#[derive(Default)]
pub(super) struct Runs {
    running: HashMap<SessionId, Run>,
    failing_since: HashMap<SessionId, Instant>,
    reported: HashSet<SessionId>,
    /// B07: Sessions whose run failed with auth. Never respawned, no timer, until the engine restarts (the User acts).
    paused: HashSet<SessionId>,
}

/// `locks/title-<session>.lock`, held by the Session's retitle run (contract §1.4).
fn lock_path(home: &CpdHome, session: &str) -> PathBuf {
    home.locks_dir().join(format!("title-{session}.lock"))
}

/// `run/titles/<session>.log`, appended by every run E10 starts for the Session (contract §1.4).
fn log_path(home: &CpdHome, session: &SessionId) -> PathBuf {
    home.run_dir().join("titles").join(format!("{session}.log"))
}

/// Whether a Session is due: it has a finished turn at `turn`, newer than `titled_at` (or it has no title),
/// and `titled_at` is null or at least [`THROTTLE`] old at `now`.
fn due(turn: Option<&str>, titled_at: Option<&str>, now: OffsetDateTime) -> Result<bool, CpdError> {
    let time = |what: &str, value: &str| {
        OffsetDateTime::parse(value, &Rfc3339).map_err(|e| {
            CpdError::new(
                ErrorCode::StoreFailed,
                format!("The store holds an invalid value: a Session's {what} is {value:?}: {e}."),
            )
        })
    };
    let Some(turn) = turn else { return Ok(false) };
    let Some(titled) = titled_at else {
        return Ok(true);
    };
    let titled = time("titled_at", titled)?;
    Ok(time("session-state Event's at", turn)? > titled && now - titled >= THROTTLE)
}

/// Each live Lead or Manager that is idle, with its latest finished turn (`working` to `idle`, hs-d257: a start,
/// `stopped` to `idle`, is not one) and `titled_at`.
const IDLE: &str = "SELECT s.id, s.titled_at,
    (SELECT e.at FROM events e WHERE e.session = s.id AND e.kind = 'session-state'
        AND json_extract(e.data, '$.from') = 'working' AND json_extract(e.data, '$.to') = 'idle'
      ORDER BY e.id DESC LIMIT 1)
  FROM sessions s WHERE s.role IN ('lead', 'manager') AND s.archived_at IS NULL AND s.state = 'idle' ORDER BY s.id";

pub(super) fn run(engine: &mut Engine) {
    if incident_holds(engine, Step::E10) {
        engine
            .log
            .line("session-titles: holding: incident mode is on");
        return;
    }
    reap(engine);
    let now = engine.ports.clock.utc();
    let idle = engine.store.read(|conn| {
        let mut stmt = conn.prepare(IDLE)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut due_now = Vec::new();
        for (id, titled_at, turn) in rows {
            if due(turn.as_deref(), titled_at.as_deref(), now)? {
                due_now.push(SessionId::parse(&id)?);
            }
        }
        Ok(due_now)
    });
    let due_now = match idle {
        Ok(due_now) => {
            engine.clear(Step::E10, &About::Machine);
            due_now
        }
        Err(e) => return engine.fail(Step::E10, About::Machine, &e),
    };
    // A Session that is not due any more (titled by another run, archived, working) has no failure to report.
    for session in engine
        .titles
        .reported
        .iter()
        .filter(|s| !due_now.contains(s))
        .cloned()
        .collect::<Vec<_>>()
    {
        engine.clear(Step::E10, &About::TitleRun(session.clone()));
        engine.titles.reported.remove(&session);
    }
    let due_now: Vec<SessionId> = due_now
        .into_iter()
        .filter(|s| !engine.titles.paused.contains(s))
        .collect();
    let held = match held(engine) {
        Ok(held) => held,
        Err(e) => return engine.fail(Step::E10, About::Machine, &e),
    };
    if due_now.is_empty() {
        return;
    }
    // hs-d543: the title is as wide as the chat slot; with no slot nothing is shown, so no retitle starts (the Sessions stay due).
    let width = match chat_slot_width(engine) {
        Ok(Some(width)) => width,
        Ok(None) => return,
        Err(e) => return engine.fail(Step::E10, About::Machine, &e),
    };
    let mut free = AT_ONCE.saturating_sub(held.len());
    for session in due_now {
        if stopping() || free == 0 {
            return;
        }
        if held.contains(session.as_str()) {
            continue;
        }
        match spawn(engine, &session, width) {
            Ok(run) => {
                engine.log.line(&format!(
                    "session-titles: started the retitle of Session {session} (pid {})",
                    run.pid.0
                ));
                engine.titles.running.insert(session, run);
                free -= 1;
            }
            Err(e) => engine.fail(Step::E10, About::TitleRun(session), &e),
        }
    }
}

/// The width in columns of the chat slot pane (contract §6.2), read from tmux; None when the cockpit has no chat slot pane.
fn chat_slot_width(engine: &Engine) -> Result<Option<u32>, CpdError> {
    let panes = engine.tmux.list_panes()?;
    chat_slot(&panes, &engine.config.tmux.cockpit_session)
        .map(|p| engine.tmux.pane_width(&p.pane))
        .transpose()
}

/// The Sessions with a run in flight: the runs this engine started, and any other holding its
/// `locks/title-<session>.lock` (a run that has not taken its lock yet is one of the first kind).
fn held(engine: &Engine) -> Result<HashSet<String>, CpdError> {
    let mut held: HashSet<String> = engine
        .titles
        .running
        .keys()
        .map(|s| s.as_str().to_owned())
        .collect();
    let dir = engine.home.locks_dir();
    let failed = |what: &str, e: &dyn std::fmt::Display| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("Cannot {what} {}: {e}.", dir.display()),
        )
        .with("path", dir.display().to_string())
    };
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(held),
        Err(e) => return Err(failed("read", &e)),
    };
    for entry in entries {
        let name = entry.map_err(|e| failed("read", &e))?.file_name();
        let Some(session) = name
            .to_str()
            .and_then(|n| n.strip_prefix("title-"))
            .and_then(|n| n.strip_suffix(".lock"))
        else {
            continue;
        };
        if held.contains(session) {
            continue;
        }
        let path = lock_path(&engine.home, session);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(failed("open", &e)),
        };
        match cpd_core::lock::shared(&file) {
            // Free: shared and dropped at once. (A run takes it exclusive, so a shared hold is only ours.)
            Ok(true) => {}
            Ok(false) => {
                held.insert(session.to_owned());
            }
            Err(e) => return Err(failed("probe", &e)),
        }
    }
    Ok(held)
}

/// Collects the runs that ended. A run that failed is logged; it is reported (once per Session until a run succeeds)
/// only when the Session's titling has failed for [`FAILING`] in a row. The Session stays due, so it is retried.
fn reap(engine: &mut Engine) {
    let sessions: Vec<SessionId> = engine.titles.running.keys().cloned().collect();
    for session in sessions {
        let about = About::TitleRun(session.clone());
        let spawned = engine.ports.spawn.clone();
        let status = engine
            .titles
            .running
            .get(&session)
            .map(|r| spawned.try_wait(r.pid));
        let status = match status {
            Some(Ok(Some(status))) => status,
            Some(Ok(None)) | None => continue,
            Some(Err(e)) => {
                engine.titles.running.remove(&session);
                let error = CpdError::new(
                    ErrorCode::SpawnFailed,
                    format!("Cannot wait for the retitle run of Session {session}: {e}."),
                );
                engine.fail(Step::E10, about, &error);
                continue;
            }
        };
        let run = engine
            .titles
            .running
            .remove(&session)
            .expect("it was just looked at");
        let log = log_path(&engine.home, &session);
        let output = output_since(&log, run.offset);
        if status.success() {
            engine.log.line(&format!(
                "session-titles: the retitle of Session {session} ended"
            ));
            engine.titles.failing_since.remove(&session);
            engine.titles.reported.remove(&session);
            engine.clear(Step::E10, &about);
        } else if lost_the_lock(&output) {
            // Another run held the Session's lock: that run is the one that counts (commands.md §10.14).
            engine.log.line(&format!(
                "session-titles: the retitle of Session {session} found another run of it"
            ));
        } else if is_auth_failure(&output) {
            let tail: Vec<&str> = output.lines().rev().take(TAIL_LINES).collect::<Vec<_>>();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            engine.log.line(&format!(
                "E10: the retitle of Session {session} failed with auth; paused"
            ));
            let failure = Failure {
                code: "title_auth".to_owned(),
                message: "Its retitle run failed to authenticate (401 / token_expired); retitling is paused until the engine restarts, after you fix pi's login.".to_owned(),
                details: json!({"session": session, "tail": tail.join("\n"), "log": log.display().to_string()}),
            };
            engine.titles.paused.insert(session);
            engine.fail(Step::E10, about, failure);
        } else {
            let since = *engine
                .titles
                .failing_since
                .entry(session.clone())
                .or_insert_with(|| engine.ports.clock.now());
            let exit = status
                .code()
                .map_or("a signal".to_owned(), |c| format!("exit {c}"));
            engine.log.line(&format!(
                "session-titles: the retitle of Session {session} failed ({exit})"
            ));
            if engine.ports.clock.now().saturating_duration_since(since) < FAILING {
                continue;
            }
            let tail: Vec<&str> = output
                .lines()
                .rev()
                .take(TAIL_LINES)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let failure = Failure {
                code: ErrorCode::TitleFailed.as_str().to_owned(),
                message: format!("Its retitle run failed ({exit})."),
                details: json!({"session": session, "tail": tail.join("\n"), "log": log.display().to_string()}),
            };
            engine.titles.reported.insert(session);
            engine.fail(Step::E10, about, failure);
        }
    }
}

/// Whether a run's output reports an authentication failure (B07): `401` or `token_expired`, by text only (pi's exact
/// stderr is unverified). Same match as `core/src/session/retitle.rs`.
fn is_auth_failure(text: &str) -> bool {
    text.contains("token_expired")
        || text
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| w == "401")
}

/// What a run wrote to its log, from `offset` on (lossy: it is for people).
fn output_since(log: &PathBuf, offset: u64) -> String {
    let mut text = Vec::new();
    if let Ok(mut file) = File::open(log) {
        let _ = file
            .seek(SeekFrom::Start(offset))
            .and_then(|_| file.read_to_end(&mut text));
    }
    String::from_utf8_lossy(&text).into_owned()
}

/// Whether the run's output is the refusal `title_failed` with reason `running`.
fn lost_the_lock(output: &str) -> bool {
    output
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .any(|v| {
            v["error"]["code"] == "title_failed" && v["error"]["details"]["reason"] == "running"
        })
}

/// Spawns `cpd session retitle <session> --width <width>` through `engine.ports.spawn`, detached (`setsid`), stdin from
/// `/dev/null`, stdout and stderr appended to the Session's `run/titles/<session>.log`.
fn spawn(engine: &Engine, session: &SessionId, width: u32) -> Result<Run, CpdError> {
    let failed = |why: String| {
        CpdError::new(
            ErrorCode::SpawnFailed,
            format!("Cannot start the retitle run of Session {session}: {why}."),
        )
        .with("session", session.as_str())
    };
    let exe = engine.ports.spawn.self_exe()?;
    let log_path = log_path(&engine.home, session);
    let dir = log_path.parent().expect("a log lives in a directory");
    fs::create_dir_all(dir).map_err(|e| failed(format!("cannot create {}: {e}", dir.display())))?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| failed(format!("cannot open {}: {e}", log_path.display())))?;
    let offset = log
        .metadata()
        .map_err(|e| failed(format!("cannot read {}: {e}", log_path.display())))?
        .len();
    let launch = detached_launch(
        &exe,
        &[
            "session",
            "retitle",
            session.as_str(),
            "--width",
            &width.to_string(),
        ],
        &engine.home,
        log_path,
    );
    let pid = engine
        .ports
        .spawn
        .spawn(&launch)
        .map_err(|e| failed(format!("cannot run {}: {e}", exe.display())))?;
    Ok(Run { pid, offset })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> OffsetDateTime {
        OffsetDateTime::parse(text, &Rfc3339).unwrap()
    }

    const NOW: &str = "2026-10-20T12:00:00Z";

    #[test]
    fn a_session_with_no_finished_turn_is_never_due() {
        assert!(!due(None, None, at(NOW)).unwrap());
        assert!(!due(None, Some("2026-10-20T11:00:00Z"), at(NOW)).unwrap());
    }

    #[test]
    fn a_session_with_a_turn_and_no_title_is_due_at_once() {
        assert!(due(Some("2026-10-20T11:59:59Z"), None, at(NOW)).unwrap());
    }

    #[test]
    fn a_title_is_rewritten_for_a_newer_turn_once_it_is_ten_minutes_old() {
        let turn = Some("2026-10-20T11:55:00Z");
        assert!(
            !due(turn, Some("2026-10-20T11:50:00.001Z"), at(NOW)).unwrap(),
            "not ten minutes old"
        );
        assert!(
            due(turn, Some("2026-10-20T11:50:00.000Z"), at(NOW)).unwrap(),
            "ten minutes old"
        );
        assert!(
            !due(turn, Some("2026-10-20T11:55:00Z"), at(NOW)).unwrap(),
            "the turn is not newer than the title"
        );
        assert!(
            !due(turn, Some("2026-10-20T11:56:00Z"), at(NOW)).unwrap(),
            "the title is newer than the turn"
        );
    }

    #[test]
    fn an_unreadable_time_is_an_error() {
        assert_eq!(
            due(Some("2026-10-20T11:00:00Z"), Some("yesterday"), at(NOW))
                .unwrap_err()
                .code(),
            ErrorCode::StoreFailed
        );
    }

    #[test]
    fn only_the_refusal_running_is_a_lost_lock() {
        let running = r#"{"ok":false,"error":{"code":"title_failed","message":"m","details":{"session":"s","reason":"running"}}}"#;
        let empty = r#"{"ok":false,"error":{"code":"title_failed","message":"m","details":{"session":"s","reason":"empty"}}}"#;
        assert!(lost_the_lock(&format!("noise\n{running}\n")));
        assert!(!lost_the_lock(empty));
        assert!(!lost_the_lock("pi: boom"));
    }
}
