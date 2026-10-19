//! The engine's log (commands.md §12): every line to stderr, and the same
//! line appended to `run/watch.log`.

use std::fs::{self, File, OpenOptions};
use std::io::Write;

use crate::clock;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

pub(super) struct Log {
    file: File,
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
        let line = format!("{} cpd watch: {message}\n", clock::now());
        let _ = std::io::stderr().write_all(line.as_bytes());
        let _ = self.file.write_all(line.as_bytes());
    }
}
