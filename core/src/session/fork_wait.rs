//! The fork's wait for its source (contract §6.7 step 1, commands.md §10.8):
//! the pane waits until the source Lead has finished its turn, and the two
//! tests of "finished" that a reload and a review Worker's fork share.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use super::Env;
use super::row::{self, State};
use super::run::{existing_file, patiently_with};
use crate::clock::Clock;
use crate::error::CpdError;
use crate::host::limit::limit;
use crate::host::pi::PiDirs;
use crate::ids::SessionId;
use crate::ports::Ports;

/// How often the fork waits for its source to be idle (§6.7 step 1; tests: `CPD_TEST_SOURCE_POLL`, [`limit`]).
const SOURCE_POLL: Duration = Duration::from_secs(2);

/// Waits until the source is idle (§6.7 step 1) and returns its file: the file
/// exists, its state is `idle` or `stopped`, its last `message` entry is an
/// assistant message with `stopReason` `stop`, and it has not been modified
/// for `fork_quiet_seconds`. Checks every 2 s, showing the waiting line. A source
/// that is `stopped` with no file can never become idle (hs-d196): the pane says so
/// and waits for Enter to try again; `None` at the end of input. A source that is
/// working is waited for without a time bound.
pub(super) fn wait_for_source_with(
    ports: &Ports,
    env: &mut Env,
    dirs: &PiDirs,
    source: &SessionId,
    name: &str,
) -> Result<Option<PathBuf>, CpdError> {
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
