//! A running cpd's own executable, and the reload after an install (hs-d33,
//! hs-d38; contract §3.4 and §6.7 "Reload on install"). A process records the
//! identity of its path at start, asks later whether the path now holds
//! another file, and then `exec`s the same path: the new binary takes its
//! state from the store.

use std::ffi::OsStr;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{CpdError, ErrorCode};

/// What tells one installed file from another: device, inode and modification time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    dev: u64,
    ino: u64,
    mtime: i64,
    mtime_nsec: i64,
}

impl Identity {
    fn of(path: &Path) -> std::io::Result<Identity> {
        let meta = std::fs::metadata(path)?;
        Ok(Identity { dev: meta.dev(), ino: meta.ino(), mtime: meta.mtime(), mtime_nsec: meta.mtime_nsec() })
    }
}

/// The path of this process's executable and its identity when the process started.
#[derive(Clone, Debug)]
pub struct Exe {
    path: PathBuf,
    identity: Identity,
}

impl Exe {
    /// Records the path (`std::env::current_exe`) and its identity. Read once, at start.
    pub fn current() -> Result<Exe, CpdError> {
        let path = std::env::current_exe().map_err(|e| {
            CpdError::new(ErrorCode::SpawnFailed, format!("Cannot find cpd's own executable: {e}."))
        })?;
        Exe::at(path)
    }

    fn at(path: PathBuf) -> Result<Exe, CpdError> {
        let identity = Identity::of(&path).map_err(|e| {
            CpdError::new(ErrorCode::SpawnFailed, format!("Cannot stat cpd's own executable {}: {e}.", path.display()))
                .with("path", path.display().to_string())
        })?;
        Ok(Exe { path, identity })
    }

    /// The path holds another file than at start: a new install. A path that
    /// cannot be stat'ed is not a replacement (there is nothing to `exec`).
    pub fn replaced(&self) -> bool {
        Identity::of(&self.path).is_ok_and(|now| now != self.identity)
    }

    /// Replaces this process with the executable at the same path, `args` as its
    /// arguments (the same program name). Returns only on failure.
    pub fn exec<I, S>(&self, args: I) -> CpdError
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let e = Command::new(&self.path).args(args).exec();
        CpdError::new(ErrorCode::SpawnFailed, format!("Cannot restart cpd from {}: {e}.", self.path.display()))
            .with("path", self.path.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn write_script(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn current_is_this_test_binary_and_is_not_replaced() {
        let exe = Exe::current().unwrap();
        assert_eq!(exe.path, std::env::current_exe().unwrap());
        assert!(!exe.replaced());
    }

    #[test]
    fn a_file_renamed_over_the_path_is_a_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cpd");
        write_script(&path, "#!/bin/sh\nexit 0\n");
        let exe = Exe::at(path.clone()).unwrap();
        assert!(!exe.replaced());
        let new = dir.path().join("cpd.new");
        write_script(&new, "#!/bin/sh\nexit 0\n");
        fs::rename(&new, &path).unwrap();
        assert!(exe.replaced());
    }

    #[test]
    fn a_file_changed_in_place_is_a_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cpd");
        write_script(&path, "#!/bin/sh\nexit 0\n");
        let exe = Exe::at(path.clone()).unwrap();
        let then = fs::metadata(&path).unwrap().modified().unwrap();
        fs::File::options().write(true).open(&path).unwrap().set_modified(then + std::time::Duration::from_secs(2)).unwrap();
        assert!(exe.replaced());
    }

    #[test]
    fn a_missing_path_is_not_a_replacement_and_cannot_be_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cpd");
        write_script(&path, "#!/bin/sh\nexit 0\n");
        let exe = Exe::at(path.clone()).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(!exe.replaced());
        assert_eq!(Exe::at(path).unwrap_err().code(), ErrorCode::SpawnFailed);
    }

    #[test]
    fn exec_failure_returns_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cpd");
        write_script(&path, "#!/bin/sh\nexit 0\n");
        let exe = Exe::at(path.clone()).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(exe.exec(["session", "run", "x"]).code(), ErrorCode::SpawnFailed);
    }

    #[test]
    fn exec_runs_the_path_with_the_given_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        // /bin/sh, not a script written here: executing a file this process just
        // wrote can fail with ETXTBSY while another test thread forks.
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "host::exe::tests::exec_child", "--nocapture", "--ignored"])
            .env("CPD_EXE_TEST_OUT", &out)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(fs::read_to_string(out).unwrap(), "x\n");
    }

    /// Run by the test above in a child process: it execs and never returns.
    #[test]
    #[ignore = "run by exec_runs_the_path_with_the_given_arguments"]
    fn exec_child() {
        let out = std::env::var("CPD_EXE_TEST_OUT").expect("run by its parent test");
        let exe = Exe::at(PathBuf::from("/bin/sh")).unwrap();
        panic!("{}", exe.exec(["-c".to_owned(), format!("echo x > '{out}'")]));
    }
}
