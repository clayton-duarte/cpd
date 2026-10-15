//! The engine's log (commands.md §12): every line to stderr, and the same
//! line appended to `run/watch.log`.

use std::fs::{self, File, OpenOptions};
use std::io::Write;

use time::OffsetDateTime;
use time::macros::format_description;

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

pub(super) struct Log {
    file: File,
}

/// Now, as contract §1.3 writes times: RFC 3339 UTC with milliseconds.
pub(super) fn timestamp(at: OffsetDateTime) -> String {
    at.format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .expect("a UTC time always formats")
}

impl Log {
    /// Opens `run/watch.log` for appending, creating it and `run/`.
    pub(super) fn open(home: &CpdHome) -> Result<Log, CpdError> {
        let path = home.watch_log();
        let failed = |e: std::io::Error| {
            CpdError::new(ErrorCode::StoreFailed, format!("Cannot open the engine's log {}: {e}.", path.display()))
                .with("path", path.display().to_string())
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(failed)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path).map_err(failed)?;
        Ok(Log { file })
    }

    /// One line, prefixed with the time. A line that cannot be written is
    /// dropped: the log must never stop the engine.
    pub(super) fn line(&mut self, message: &str) {
        let line = format!("{} cpd watch: {message}\n", timestamp(OffsetDateTime::now_utc()));
        let _ = std::io::stderr().write_all(line.as_bytes());
        let _ = self.file.write_all(line.as_bytes());
    }
}
