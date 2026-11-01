//! Shared files: the lockfile paths several Jobs may write at once (contract §2.6).

/// True when the file name of `path` (the part after its last `/`) is a lockfile.
pub fn is_shared(path: &str) -> bool {
    let _ = path;
    todo!()
}
