//! Shared files: the lockfile paths several Jobs may write at once (contract §2.6).

/// The file names that count as shared, at any depth.
const SHARED_NAMES: [&str; 4] = ["Cargo.lock", "pnpm-lock.yaml", "package-lock.json", "yarn.lock"];

/// True when the file name of `path` (the part after its last `/`) is a lockfile.
pub fn is_shared(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    SHARED_NAMES.contains(&name)
}
