//! pi's own directories and settings files (contract §6.5, §6.7): where pi keeps
//! its settings and sessions, and what tells that pi's install changed.

use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::error::{CpdError, ErrorCode};
use crate::host::exe::Watched;

/// pi's own directories, read once from the environment the way pi reads them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PiDirs {
    /// The home directory, for `~` in settings paths.
    pub home: PathBuf,
    /// `PI_CODING_AGENT_DIR`, else `~/.pi/agent`.
    pub agent_dir: PathBuf,
    /// `PI_CODING_AGENT_SESSION_DIR` when set.
    pub session_dir: Option<PathBuf>,
}

impl PiDirs {
    /// From this process's environment.
    pub fn from_env() -> Result<PiDirs, CpdError> {
        PiDirs::resolve(
            std::env::home_dir(),
            std::env::var_os("PI_CODING_AGENT_DIR"),
            std::env::var_os("PI_CODING_AGENT_SESSION_DIR"),
        )
    }

    /// Empty variables count as unset and `~` is expanded, as pi does. No home
    /// directory is `usage` (exit 2): the caller fixes its environment.
    pub fn resolve(
        home: Option<PathBuf>,
        agent_dir: Option<OsString>,
        session_dir: Option<OsString>,
    ) -> Result<PiDirs, CpdError> {
        let home = home.filter(|h| h.is_absolute()).ok_or_else(|| {
            CpdError::new(
                ErrorCode::Usage,
                "The home directory is unknown, so pi's settings cannot be found: set HOME.",
            )
            .with("variable", "HOME")
        })?;
        let set = |v: Option<OsString>| {
            v.filter(|v| !v.is_empty())
                .map(|v| expand_tilde(&v.to_string_lossy(), &home))
        };
        let agent_dir = set(agent_dir).unwrap_or_else(|| home.join(".pi").join("agent"));
        let session_dir = set(session_dir);
        Ok(PiDirs {
            home,
            agent_dir,
            session_dir,
        })
    }

    /// What tells that pi's install changed (contract §6.7 "Reload on install"):
    /// `<agent dir>/install/current-version` when it exists, else the `pi`
    /// executable on `PATH`, symlinks resolved. None when neither exists.
    pub fn install_file(&self, path_var: &OsStr) -> Option<PathBuf> {
        let current = self.agent_dir.join("install").join("current-version");
        if current.is_file() {
            return Some(current);
        }
        std::env::split_paths(path_var)
            .map(|dir| dir.join("pi"))
            .find(|candidate| {
                candidate
                    .metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
            .and_then(|found| found.canonicalize().ok())
    }

    /// [`install_file`](Self::install_file) on this process's `PATH`, recorded now.
    pub fn watch_install(&self) -> Option<Watched> {
        let path_var = std::env::var_os("PATH")?;
        Watched::at(self.install_file(&path_var)?)
    }

    /// `<agent dir>/settings.json`: pi's global settings.
    pub fn global_settings(&self) -> PathBuf {
        self.agent_dir.join("settings.json")
    }
}

/// `<cwd>/.pi/settings.json`: pi's project settings.
pub fn project_settings(cwd: &Path) -> PathBuf {
    cwd.join(".pi").join("settings.json")
}

pub(super) fn expand_tilde(input: &str, home: &Path) -> PathBuf {
    if input == "~" {
        home.to_path_buf()
    } else if let Some(rest) = input.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(input)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn pi_dirs_follow_pi_s_variables() {
        let home = PathBuf::from("/home/u");
        let d = PiDirs::resolve(Some(home.clone()), None, None).unwrap();
        assert_eq!(d.agent_dir, Path::new("/home/u/.pi/agent"));
        assert_eq!(
            d.global_settings(),
            Path::new("/home/u/.pi/agent/settings.json")
        );
        assert_eq!(d.session_dir, None);
        let d = PiDirs::resolve(
            Some(home.clone()),
            Some("~/agent".into()),
            Some("/s".into()),
        )
        .unwrap();
        assert_eq!(d.agent_dir, Path::new("/home/u/agent"));
        assert_eq!(d.session_dir.as_deref(), Some(Path::new("/s")));
        let d = PiDirs::resolve(Some(home), Some("".into()), Some("".into())).unwrap();
        assert_eq!(d.agent_dir, Path::new("/home/u/.pi/agent"));
        assert_eq!(d.session_dir, None);
        assert_eq!(
            PiDirs::resolve(None, None, None).unwrap_err().code(),
            ErrorCode::Usage
        );
    }

    fn executable(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn pi_s_install_is_current_version_when_it_exists_else_the_resolved_executable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let dirs = PiDirs::resolve(Some(root.join("home")), None, None).unwrap();
        let real = root.join("real/pi");
        executable(&real);
        fs::create_dir_all(root.join("bin")).unwrap();
        std::os::unix::fs::symlink(&real, root.join("bin/pi")).unwrap();
        let path_var = std::env::join_paths([root.join("none"), root.join("bin")]).unwrap();

        // No `install/`: the executable `PATH` finds, symlinks resolved.
        assert_eq!(dirs.install_file(&path_var), Some(real.clone()));
        // `install/` without `current-version`: the same.
        fs::create_dir_all(dirs.agent_dir.join("install")).unwrap();
        assert_eq!(dirs.install_file(&path_var), Some(real));
        // `current-version` wins.
        let current = dirs.agent_dir.join("install/current-version");
        fs::write(&current, "1.3.0\n").unwrap();
        assert_eq!(dirs.install_file(&path_var), Some(current));
    }

    #[test]
    fn with_no_current_version_and_no_pi_there_is_nothing_to_watch() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = PiDirs::resolve(Some(dir.path().join("home")), None, None).unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("pi"), "not executable").unwrap();
        assert_eq!(dirs.install_file(bin.as_os_str()), None);
        assert_eq!(dirs.install_file(OsStr::new("")), None);
    }

    #[test]
    fn a_change_of_current_version_or_of_the_executable_is_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let dirs = PiDirs::resolve(Some(root.join("home")), None, None).unwrap();
        let pi = root.join("bin/pi");
        executable(&pi);
        let path_var = root.join("bin").into_os_string();
        let watched = Watched::at(dirs.install_file(&path_var).unwrap()).unwrap();
        assert!(!watched.changed());
        let new = root.join("bin/pi.new");
        executable(&new);
        fs::rename(&new, &pi).unwrap();
        assert!(watched.changed());

        let current = dirs.agent_dir.join("install/current-version");
        fs::create_dir_all(current.parent().unwrap()).unwrap();
        fs::write(&current, "1.2.0\n").unwrap();
        let watched = Watched::at(dirs.install_file(&path_var).unwrap()).unwrap();
        assert!(!watched.changed());
        let new = current.with_extension("tmp");
        fs::write(&new, "1.3.0\n").unwrap();
        fs::rename(&new, &current).unwrap();
        assert!(watched.changed());
    }
}
