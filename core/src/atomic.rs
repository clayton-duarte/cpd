//! One atomic file write: bytes to a temporary file beside the target, then the temporary file renamed over the
//! target, so a reader never sees half of a file. The temporary file is a sibling of the target, so the rename
//! stays on one filesystem. A failure of the write or the rename removes the temporary file; the `io::Error` is
//! returned unchanged, and each caller maps it to its own message.

use std::path::Path;

/// Writes `bytes` to `path` whole: to a sibling temporary file, then renamed over `path`. The parent directory is
/// created. The temporary file is `.<file name>.<pid>.tmp`, one rule for every caller, the pid keeping two writers
/// apart. A failure removes the temporary file.
pub fn write_atomic(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a path without a file name",
        )
    })?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes)
        .and_then(|()| std::fs::rename(&tmp, path))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
}
