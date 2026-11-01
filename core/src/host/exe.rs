//! A running cpd's own executable, and the reload after an install (hs-d33,
//! hs-d38; contract §3.4 and §6.7 "Reload on install"). A process records the
//! identity of its path at start, asks later whether the path now holds
//! another file, and then `exec`s the same path: the new binary takes its
//! state from the store.

use std::cell::Cell;
use std::ffi::OsStr;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::deadline::Failed;
use super::runner::Runner;
use crate::error::{CpdError, ErrorCode};
use crate::ports::Ports;

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
        Ok(Identity {
            dev: meta.dev(),
            ino: meta.ino(),
            mtime: meta.mtime(),
            mtime_nsec: meta.mtime_nsec(),
        })
    }
}

/// Any file whose replacement is a change: its path and its identity when recorded.
/// Same test as [`Exe::replaced`], without the `exec`.
#[derive(Clone, Debug)]
pub struct Watched {
    path: PathBuf,
    identity: Identity,
}

impl Watched {
    /// Records the identity of `path`; `None` when it cannot be stat'ed now.
    pub fn at(path: PathBuf) -> Option<Watched> {
        let identity = Identity::of(&path).ok()?;
        Some(Watched { path, identity })
    }

    /// The path holds another file than when recorded. A path that cannot be
    /// stat'ed now is not a change.
    pub fn changed(&self) -> bool {
        Identity::of(&self.path).is_ok_and(|now| now != self.identity)
    }
}

/// How long a newly installed cpd has to answer `--version`.
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// An installed cpd that a reload must not `exec` (hs-d151).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// First time this installed file is refused: the reason, to be logged.
    New(String),
    /// This installed file was refused before and was not asked again.
    Known,
}

/// Records the path of this process's executable (`std::env::current_exe`).
/// Call it first thing in `main`: once an install replaces the file,
/// `current_exe()` reads `<path> (deleted)` on Linux. The first record wins.
/// If the path cannot be found nothing is recorded, and starting a driver
/// fails loud ([`started_as`]); other commands do not need it.
/// The record is what `ProcessSpawn::self_exe` answers (`host::spawn`).
pub fn record_start() {
    if let Ok(path) = std::env::current_exe() {
        super::spawn::record_start_as(path);
    }
}

/// The path recorded at process start: what a driver is started from, the
/// installed file there now. An error if nothing was recorded.
pub fn started_as() -> Result<&'static Path, CpdError> {
    super::spawn::recorded()
}

/// The path of this process's executable and its identity when the process started.
#[derive(Clone, Debug)]
pub struct Exe {
    path: PathBuf,
    identity: Identity,
    /// The identity of the installed file last found unable to open the store.
    refused: Cell<Option<Identity>>,
}

impl Exe {
    /// Records the path (`std::env::current_exe`) and its identity. Read once, at start.
    pub fn current() -> Result<Exe, CpdError> {
        let path = std::env::current_exe().map_err(|e| {
            CpdError::new(
                ErrorCode::SpawnFailed,
                format!("Cannot find cpd's own executable: {e}."),
            )
        })?;
        Exe::at(path)
    }

    pub(crate) fn at(path: PathBuf) -> Result<Exe, CpdError> {
        let identity = Identity::of(&path).map_err(|e| {
            CpdError::new(
                ErrorCode::SpawnFailed,
                format!("Cannot stat cpd's own executable {}: {e}.", path.display()),
            )
            .with("path", path.display().to_string())
        })?;
        Ok(Exe {
            path,
            identity,
            refused: Cell::new(None),
        })
    }

    /// The path holds another file than at start: a new install. A path that
    /// cannot be stat'ed is not a replacement (there is nothing to `exec`).
    pub fn replaced(&self) -> bool {
        Identity::of(&self.path).is_ok_and(|now| now != self.identity)
    }

    /// The path of the executable.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the file now at the path can open the store: it runs `<path>
    /// --version` and reads the store verdict.
    /// A file refused once is not asked again until the path holds another file
    /// ([`Refusal::Known`]).
    pub fn check_store(&self) -> Result<(), Refusal> {
        self.check_store_within(VERSION_TIMEOUT)
    }

    fn check_store_within(&self, timeout: Duration) -> Result<(), Refusal> {
        let now =
            Identity::of(&self.path).map_err(|e| Refusal::New(format!("cannot be read: {e}")))?;
        if self.refused.get() == Some(now) {
            return Err(Refusal::Known);
        }
        asks_to_open_store_with(&*Ports::current().runner, &self.path, timeout).map_err(|reason| {
            self.refused.set(Some(now));
            Refusal::New(reason)
        })
    }

    /// [`Exe::exec`] through `spawn`: `Spawn::exec` of a `Launch` whose program is the path and whose arguments are `args`.
    /// Returns only on failure, as `spawn_failed`.
    pub fn exec_with<I, S>(&self, spawn: &dyn super::spawn::Spawn, args: I) -> CpdError
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let launch = super::spawn::Launch {
            program: self.path.display().to_string(),
            args: args
                .into_iter()
                .map(|a| a.as_ref().to_string_lossy().into_owned())
                .collect(),
            ..super::spawn::Launch::default()
        };
        let e = spawn.exec(&launch);
        CpdError::new(
            ErrorCode::SpawnFailed,
            format!("Cannot restart cpd from {}: {e}.", self.path.display()),
        )
        .with("path", self.path.display().to_string())
    }

    /// Replaces this process with the executable at the same path, `args` as its
    /// arguments (the same program name). Returns only on failure.
    pub fn exec<I, S>(&self, args: I) -> CpdError
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.exec_with(&*Ports::current().spawn, args)
    }
}

/// Runs `<program> --version` on `runner` (stdin null, killed after `timeout`)
/// and decides from the `store` verdict in its JSON whether that cpd can open the
/// store: the database does not exist (`store` null) or `store.opens` is true.
/// `Err` is the reason it cannot, or why it could not be asked.
pub(crate) fn asks_to_open_store_with(
    runner: &dyn Runner,
    program: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let mut command = Command::new(program);
    command.arg("--version").stdin(Stdio::null());
    let out = match runner.output(&mut command, "cpd", timeout) {
        Ok(out) => out,
        Err(Failed::Io(e)) => return Err(format!("cannot run --version: {e}")),
        Err(Failed::Timeout(_)) => {
            return Err(format!(
                "--version did not answer in {}s",
                timeout.as_secs_f32()
            ));
        }
    };
    if !out.status.success() {
        return Err(format!("--version failed: {}", out.status));
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("--version printed no JSON: {e}"))?;
    // The verdict (contract §4.5; commands.md §1.2): `null` without a database, else `opens`.
    match json.get("store") {
        Some(serde_json::Value::Null) => Ok(()),
        Some(store) => match store.get("opens").and_then(|v| v.as_bool()) {
            Some(true) => Ok(()),
            Some(false) => {
                let message = store.pointer("/refusal/message").and_then(|v| v.as_str());
                Err(format!(
                    "cannot open the store: {}",
                    message.ok_or("--version reports no refusal message")?
                ))
            }
            None => Err("--version reports a store verdict without a boolean opens".to_owned()),
        },
        None => Err("--version does not report the store verdict".to_owned()),
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
    fn started_as_is_the_recorded_path_not_current_exe() {
        assert!(started_as().is_err(), "nothing recorded yet");
        crate::host::spawn::record_start_as(PathBuf::from("/recorded/cpd"));
        crate::host::spawn::record_start_as(PathBuf::from("/second/cpd"));
        assert_eq!(started_as().unwrap(), Path::new("/recorded/cpd"));
        assert_ne!(started_as().unwrap(), std::env::current_exe().unwrap());
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
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(then + std::time::Duration::from_secs(2))
            .unwrap();
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
    fn a_watched_file_renamed_over_or_rewritten_in_place_has_changed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("current-version");
        assert!(Watched::at(path.clone()).is_none(), "nothing to watch yet");
        fs::write(&path, "1.2.0\n").unwrap();
        let renamed = Watched::at(path.clone()).unwrap();
        assert!(!renamed.changed());
        let new = dir.path().join("current-version.tmp");
        fs::write(&new, "1.3.0\n").unwrap();
        fs::rename(&new, &path).unwrap();
        assert!(renamed.changed());

        let rewritten = Watched::at(path.clone()).unwrap();
        assert!(!rewritten.changed());
        let then = fs::metadata(&path).unwrap().modified().unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(then + std::time::Duration::from_secs(2))
            .unwrap();
        assert!(rewritten.changed());
    }

    #[test]
    fn a_watched_file_that_is_missing_has_not_changed_and_one_that_comes_back_has() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("current-version");
        fs::write(&path, "1.2.0\n").unwrap();
        let watched = Watched::at(path.clone()).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(!watched.changed());
        // The inode may be reused and the clock coarse: give the new file its own mtime.
        fs::write(&path, "1.3.0\n").unwrap();
        let then = fs::metadata(&path).unwrap().modified().unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(then + std::time::Duration::from_secs(2))
            .unwrap();
        assert!(watched.changed());
    }

    const REFUSES: &str = r#"{"ok":true,"version":"x","store":{"opens":false,"refusal":{"code":"store_too_new","message":"too new"},"unknown":[],"pending":[]}}"#;

    #[test]
    fn a_refused_file_is_asked_once_and_a_new_file_is_asked_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cpd");
        let calls = dir.path().join("calls");
        let old = format!(
            "#!/bin/sh\necho x >> '{}'\necho '{REFUSES}'\n",
            calls.display()
        );
        write_script(&path, &old);
        let exe = Exe::at(path.clone()).unwrap();
        // The check runs the script by path; the pre-existing file is the one the
        // identity belongs to. A `sh` script that was just written can hit ETXTBSY.
        let ran = || {
            fs::read_to_string(&calls)
                .map(|t| t.lines().count())
                .unwrap_or(0)
        };
        let asked = |exe: &Exe| loop {
            match exe.check_store() {
                Err(Refusal::New(r)) if r.contains("Text file busy") => {
                    exe.refused.set(None);
                    std::thread::sleep(Duration::from_millis(50));
                }
                other => break other,
            }
        };
        assert!(
            matches!(asked(&exe), Err(Refusal::New(r)) if r == "cannot open the store: too new")
        );
        assert_eq!(ran(), 1);
        assert_eq!(exe.check_store(), Err(Refusal::Known));
        assert_eq!(exe.check_store(), Err(Refusal::Known));
        assert_eq!(ran(), 1, "the same file is not asked again");

        let new = dir.path().join("cpd.new");
        write_script(&new, &old);
        fs::rename(&new, &path).unwrap();
        assert!(
            matches!(asked(&exe), Err(Refusal::New(_))),
            "another file is asked"
        );
        assert_eq!(ran(), 2);
        assert_eq!(exe.check_store(), Err(Refusal::Known));
    }

    #[test]
    fn exec_failure_returns_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cpd");
        write_script(&path, "#!/bin/sh\nexit 0\n");
        let exe = Exe::at(path.clone()).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(
            exe.exec(["session", "run", "x"]).code(),
            ErrorCode::SpawnFailed
        );
    }

    #[test]
    fn exec_runs_the_path_with_the_given_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        // /bin/sh, not a script written here: executing a file this process just
        // wrote can fail with ETXTBSY while another test thread forks.
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "host::exe::tests::exec_child",
                "--nocapture",
                "--ignored",
            ])
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
        panic!(
            "{}",
            exe.exec(["-c".to_owned(), format!("echo x > '{out}'")])
        );
    }
}
