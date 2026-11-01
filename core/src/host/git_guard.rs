//! The `git` wrapper that refuses `git stash` in every pi Session (contract
//! §6.5 "`PATH` and the `git` wrapper", hs-d310): `refs/stash` is one ref
//! shared by every worktree of a repo, so two Workers swapped each other's
//! changes through it.
//!
//! cpd writes [`WRAPPER`] to `$CPD_HOME/bin/git` ([`install`]) and puts
//! `$CPD_HOME/bin` first on the `PATH` of the pi it starts ([`path_with_bin`]).

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

/// What the wrapper prints on stderr when it refuses (contract §6.5).
pub const REFUSAL: &str = "git stash is refused in cpd Sessions (hs-d310): refs/stash is shared by every worktree of a repo; commit on your branch instead.";

/// What the wrapper prints on stderr when no other `git` is on `PATH` (contract §6.5).
pub const NO_GIT: &str = "git: no git on PATH besides the cpd wrapper";

/// What every version of the wrapper starts with: a program that looks for a real `git` (a test harness) tells a
/// wrapper by it.
pub const SIGNATURE: &str = "#!/bin/sh\n# cpd's git wrapper (hs-d310)";

/// The wrapper, an `sh` script (contract §6.5): refuses `stash` after git's global options,
/// else `exec`s the first `git` on `PATH` outside the directories of cpd's wrappers (its own and those that `exec`'d it).
pub const WRAPPER: &str = r#"#!/bin/sh
# cpd's git wrapper (hs-d310): refuses `git stash`, runs the real git otherwise.

# is_stash ARGS...: succeeds when the first word that is not a global option is `stash`.
is_stash() {
  while [ $# -gt 0 ]; do
    case $1 in
      -C|-c|--git-dir|--work-tree|--namespace|--super-prefix|--config-env)
        [ $# -ge 2 ] || return 1
        shift 2 ;;
      -*) shift ;;
      stash) return 0 ;;
      *) return 1 ;;
    esac
  done
  return 1
}

if is_stash "$@"; then
  echo 'git stash is refused in cpd Sessions (hs-d310): refs/stash is shared by every worktree of a repo; commit on your branch instead.' >&2
  exit 1
fi

# The real git: the first git on PATH once the directories of cpd's wrappers are out of it: this wrapper's own and
# those of the wrappers that exec'd it, kept in CPD_GIT_WRAPPERS (physical paths, ':' apart). Another wrapper on PATH
# (the $CPD_HOME/bin of another home) is thus passed over, and a chain of wrappers ends at the real git instead of
# exec'ing each other for ever. PATH itself goes on as it was given.
# Only builtins: a PATH without coreutils must not break it.
case $0 in */*) self_raw=${0%/*} ;; *) self_raw=. ;; esac
self=$(cd "$self_raw" 2>/dev/null && pwd -P)
CPD_GIT_WRAPPERS=${CPD_GIT_WRAPPERS:+$CPD_GIT_WRAPPERS:}$self
export CPD_GIT_WRAPPERS
rest=
set -f
old_ifs=$IFS
IFS=:
for dir in $PATH; do
  [ "$dir" = "$self_raw" ] && continue
  if [ -n "$dir" ]; then
    phys=$(cd "$dir" 2>/dev/null && pwd -P)
    if [ -n "$phys" ]; then
      case :$CPD_GIT_WRAPPERS: in *:"$phys":*) continue ;; esac
    fi
  fi
  rest=${rest:+$rest:}$dir
done
IFS=$old_ifs
set +f

real=$(PATH=$rest command -v git)
if [ -z "$real" ]; then
  echo 'git: no git on PATH besides the cpd wrapper' >&2
  exit 127
fi
exec "$real" "$@"
"#;

/// `$CPD_HOME/bin` first, then `path` (the `PATH` of the process that starts the Session; `None` when it has none).
pub fn path_with_bin(home: &CpdHome, path: Option<OsString>) -> String {
    let mut dirs = vec![home.bin_dir()];
    if let Some(path) = &path {
        dirs.extend(std::env::split_paths(path));
    }
    // A directory with `:` in its name cannot be on a `PATH`; none of ours has one. Non-UTF-8 is lossy.
    std::env::join_paths(dirs)
        .expect("no PATH entry contains ':'")
        .to_string_lossy()
        .into_owned()
}

/// Writes [`WRAPPER`] to `$CPD_HOME/bin/git` with mode 0755 unless the file already has exactly that
/// content and mode; else writes it to a temporary file in `bin/` and renames it into place.
/// Idempotent. Returns the wrapper's path.
pub fn install(home: &CpdHome) -> Result<PathBuf, CpdError> {
    let path = home.git_wrapper();
    if is_current(&path) {
        return Ok(path);
    }
    let failed = |what: &str, e: std::io::Error| {
        CpdError::new(
            ErrorCode::SetupFailed,
            format!("Cannot {what} the git wrapper {}: {e}.", path.display()),
        )
        .with("path", path.display().to_string())
    };
    let dir = home.bin_dir();
    fs::create_dir_all(&dir).map_err(|e| failed("create the directory of", e))?;
    let tmp = dir.join(format!(".git.{}.tmp", std::process::id()));
    let write = || -> std::io::Result<()> {
        let _ = fs::remove_file(&tmp);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(&tmp)?;
        file.write_all(WRAPPER.as_bytes())?;
        // The mode of `open` is cut by the umask: set it exactly.
        file.set_permissions(fs::Permissions::from_mode(0o755))?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &path)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        failed("write", e)
    })?;
    Ok(path)
}

fn is_current(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    meta.permissions().mode() & 0o7777 == 0o755
        && fs::read(path).is_ok_and(|content| content == WRAPPER.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(dir: &Path) -> CpdHome {
        CpdHome::resolve(Some(dir.into()), None).unwrap()
    }

    #[test]
    fn the_path_starts_with_the_bin_dir_then_the_callers_path() {
        let h = CpdHome::resolve(Some("/cpd-home".into()), None).unwrap();
        assert_eq!(
            path_with_bin(&h, Some("/usr/bin:/bin".into())),
            "/cpd-home/bin:/usr/bin:/bin"
        );
        assert_eq!(path_with_bin(&h, None), "/cpd-home/bin");
    }

    #[test]
    fn the_wrapper_starts_with_its_signature() {
        assert!(WRAPPER.starts_with(SIGNATURE));
    }

    #[test]
    fn the_refusal_and_the_missing_git_text_of_the_wrapper_are_the_documented_ones() {
        assert!(WRAPPER.contains(&format!("echo '{REFUSAL}' >&2")));
        assert!(WRAPPER.contains(&format!("echo '{NO_GIT}' >&2")));
    }

    #[test]
    fn install_twice_leaves_one_identical_executable_file() {
        let tmp = tempfile::tempdir().unwrap();
        let h = home(tmp.path());
        let path = install(&h).unwrap();
        assert_eq!(path, tmp.path().join("bin/git"));
        let first = fs::metadata(&path).unwrap();
        assert_eq!(install(&h).unwrap(), path);
        let second = fs::metadata(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), WRAPPER);
        assert_eq!(second.permissions().mode() & 0o7777, 0o755);
        // Not rewritten: the same inode, and nothing else in bin/.
        assert_eq!(
            std::os::unix::fs::MetadataExt::ino(&first),
            std::os::unix::fs::MetadataExt::ino(&second)
        );
        assert_eq!(fs::read_dir(tmp.path().join("bin")).unwrap().count(), 1);
    }

    #[test]
    fn install_replaces_a_different_file_or_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let h = home(tmp.path());
        fs::create_dir_all(h.bin_dir()).unwrap();
        fs::write(h.git_wrapper(), "old").unwrap();
        install(&h).unwrap();
        assert_eq!(fs::read_to_string(h.git_wrapper()).unwrap(), WRAPPER);
        fs::set_permissions(h.git_wrapper(), fs::Permissions::from_mode(0o644)).unwrap();
        install(&h).unwrap();
        assert_eq!(
            fs::metadata(h.git_wrapper()).unwrap().permissions().mode() & 0o7777,
            0o755
        );
        assert_eq!(fs::read_dir(h.bin_dir()).unwrap().count(), 1);
    }
}
