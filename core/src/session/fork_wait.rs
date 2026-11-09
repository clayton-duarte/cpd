//! The fork's wait for its source (contract §6.7 step 1, commands.md §10.8):
//! the pane waits until the source Lead has finished its turn, and the two
//! tests of "finished" that a reload and a review Worker's fork share.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::Env;
use super::row::{self, Row, State};
use super::run::{existing_file, patiently_with};
use super::state::scoped;
use crate::clock::Clock;
use crate::error::CpdError;
use crate::events::{NewEvent, append};
use crate::host::limit::limit;
use crate::host::pi::PiDirs;
use crate::ids::SessionId;
use crate::ports::Ports;
use crate::store::WriteTxn;

/// How often the fork waits for its source to be idle (§6.7 step 1; tests: `CPD_TEST_SOURCE_POLL`, [`limit`]).
const SOURCE_POLL: Duration = Duration::from_secs(2);

/// Waits until the source is idle (§6.7 step 1) and returns its file: the file
/// exists, its state is `idle` or `stopped`, its last `message` entry is an
/// assistant message with `stopReason` `stop`, and it has not been modified
/// for `fork_quiet_seconds`. Checks every 2 s, showing the waiting line. A source
/// that is `stopped` with no file can never become idle (hs-d196): the pane says so
/// and waits for Enter to try again; `None` at the end of input. A source that is
/// working is waited for without a time bound.
#[cfg(test)]
pub(super) fn wait_for_source_with(
    ports: &Ports,
    env: &mut Env,
    dirs: &PiDirs,
    source: &SessionId,
    name: &str,
) -> Result<Option<PathBuf>, CpdError> {
    wait_for_source(ports, env, dirs, source, name, None)
}

/// [`wait_for_source_with`] for the fork `fork` (when known): while the source is `blocked` it reports it once per
/// episode (contract §6.7 "A blocked source").
pub(super) fn wait_for_source(
    ports: &Ports,
    env: &mut Env,
    dirs: &PiDirs,
    source: &SessionId,
    name: &str,
    fork: Option<&Row>,
) -> Result<Option<PathBuf>, CpdError> {
    let mut reported = false;
    let quiet = Duration::from_secs(u64::from(env.config.timeouts.fork_quiet_seconds));
    let mut shown = false;
    loop {
        let src = patiently_with(&*ports.clock, "reading the source Lead", || {
            env.store.read(|conn| row::load(conn, source))
        })?;
        if let Some(file) = existing_file(dirs, &src)
            && matches!(src.state, State::Idle | State::Stopped)
            && quiet_since_with(&*ports.clock, &file, quiet)
            && std::fs::read_to_string(&file).is_ok_and(|text| turn_finished(&text))
        {
            return Ok(Some(file));
        }
        if src.state != State::Blocked {
            reported = false;
        } else if !reported {
            reported = true;
            if let Some(fork) = fork {
                report_blocked(ports, env, fork, &src)?;
            }
        }
        if src.state == State::Stopped && existing_file(dirs, &src).is_none() {
            print!(
                "\n  The source Lead {source} is stopped and has no session file, so it will never finish a turn.\n  Press Enter to retry.\n"
            );
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            match crate::host::input::read_line(&mut line) {
                Ok(0) | Err(_) => return Ok(None),
                Ok(_) => {}
            }
            shown = false;
            continue;
        }
        if !shown {
            print!(
                "\n  Waiting for the source Lead to finish its turn, then forking it as \"{name}\"…\n"
            );
            let _ = std::io::stdout().flush();
            shown = true;
        }
        ports.clock.sleep(limit("SOURCE_POLL", SOURCE_POLL));
    }
}

/// The blocked episode's report: the `error` Event, the wakes and the pane line (contract §6.7 "A blocked source").
fn report_blocked(ports: &Ports, env: &mut Env, fork: &Row, src: &Row) -> Result<(), CpdError> {
    let cause = src.state_label.clone().unwrap_or_default();
    let text = format!(
        "Fork \"{}\" waits for its source \"{}\", which is blocked: {cause}.",
        fork.name, src.name
    );
    let wake = format!("{text} It forks once the source is idle or stopped.");
    let details =
        json!({"fork": fork.id, "source": src.id, "cause": cause, "state_since": src.state_at});
    patiently_with(&*ports.clock, "reporting the blocked source", || {
        env.store.write("session run", |tx| {
            let event = NewEvent::error(
                text.clone(),
                "session run",
                "fork_source_blocked",
                &text,
                details.clone(),
            );
            append(tx, scoped(event, fork))?;
            if let Some(plan) = &src.plan {
                let lead: Option<String> = tx
                    .query_row(
                        "SELECT s.id FROM sessions s JOIN plans p ON p.lead_session = s.id \
                         WHERE p.id = ?1 AND s.forked_from IS NULL AND s.id <> ?2",
                        [plan.as_str(), src.id.as_str()],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(lead) = lead {
                    wake_target(tx, &SessionId::parse(&lead)?, &wake)?;
                }
            }
            let manager: Option<String> = tx
                .query_row(
                    "SELECT id FROM sessions WHERE role = 'manager' AND archived_at IS NULL AND context = ?1 \
                     ORDER BY created_at, id LIMIT 1",
                    [src.context.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(manager) = manager {
                wake_target(tx, &SessionId::parse(&manager)?, &wake)?;
            }
            Ok(())
        })
    })?;
    print!(
        "\n  The source {} is blocked ({cause}); the fork waits until it is idle or stopped.\n",
        src.name
    );
    let _ = std::io::stdout().flush();
    Ok(())
}

/// One waking `session-message` (`from` null) to `target`, its `text` the message itself (contract §6.7 "A blocked source").
fn wake_target(tx: &WriteTxn<'_>, target: &SessionId, text: &str) -> Result<(), CpdError> {
    let event = NewEvent::session_message(text, None::<String>, text, true, None)?;
    append(tx, scoped(event, &row::load(tx, target)?))?;
    Ok(())
}

/// [`quiet_since`] on `clock`: the file's modification time against `clock.utc()`.
pub(crate) fn quiet_since_with(clock: &dyn Clock, file: &Path, quiet: Duration) -> bool {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .is_ok_and(|modified| {
            SystemTime::from(clock.utc())
                .duration_since(modified)
                .is_ok_and(|age| age >= quiet)
        })
}

/// The last `message` entry of a pi session file is a finished assistant
/// turn (`stopReason` `stop`). A line that is not JSON after it means pi is
/// writing: not finished.
pub(crate) fn turn_finished(text: &str) -> bool {
    for line in text.lines().rev().filter(|l| !l.trim().is_empty()) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        if entry["type"] == "message" {
            let message = &entry["message"];
            return message["role"] == "assistant" && message["stopReason"] == "stop";
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = r#"{"type":"message","id":"a","message":{"role":"user","content":"hi"}}"#;
    const DONE: &str =
        r#"{"type":"message","id":"b","message":{"role":"assistant","stopReason":"stop"}}"#;
    const TOOL: &str =
        r#"{"type":"message","id":"c","message":{"role":"assistant","stopReason":"toolUse"}}"#;
    const NAME: &str = r#"{"type":"session_info","name":"x"}"#;

    #[test]
    fn a_turn_is_finished_when_the_last_message_is_an_assistant_stop() {
        assert!(turn_finished(&[USER, DONE].join("\n")));
        assert!(
            turn_finished(&[USER, DONE, NAME, ""].join("\n")),
            "other entries after it do not count"
        );
        assert!(!turn_finished(&[USER, TOOL].join("\n")));
        assert!(!turn_finished(&[DONE, USER].join("\n")));
        assert!(
            !turn_finished(&[USER, DONE, "{\"type\":\"mess"].join("\n")),
            "a half-written line is not finished"
        );
        assert!(!turn_finished(NAME));
        assert!(!turn_finished(""));
    }
}
