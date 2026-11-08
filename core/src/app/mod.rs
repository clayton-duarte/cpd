//! `cpd app` and install's Electron step (commands.md §14.4a and §14.4 step 7a, docs/implementing.md "`cpd app`").
//!
//! The names are those of docs/implementing.md.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use crate::error::{CpdError, ErrorCode};
use crate::host::deadline::Failed;
use crate::host::pi::SESSION_ENV_KEYS;
use crate::host::spawn::{self, Pid, Signal};
use crate::ports::Ports;

/// How long a pnpm run may take.
const PNPM_DEADLINE: Duration = Duration::from_secs(10 * 60);
/// How long the preview script may take.
const PREVIEW_DEADLINE: Duration = Duration::from_secs(15 * 60);

#[cfg(test)]
mod tests;

/// What `cpd app` and the install's Electron step run on, built once at the CLI edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// The checkout (`app-root`, or `--root` for the install).
    pub root: PathBuf,
    /// `$HOME`.
    pub home: PathBuf,
    /// `$CPD_HOME`, else `<home>/.cpd`.
    pub cpd_home: PathBuf,
    /// `$CPD_PREVIEW_DIR`, else `<home>/.cache/cpd-preview`.
    pub preview_dir: PathBuf,
    /// Whether ensure runs `plutil` (`cfg!(target_os = "macos")`, set at the edge).
    pub macos: bool,
}

/// What the preview forms run: `preview-web.sh on|reload|off`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preview {
    On(Option<PathBuf>),
    Reload(Option<PathBuf>),
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchAction {
    Launched,
    Reloaded,
}

/// The answer of plain `cpd app`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub action: LaunchAction,
    pub pid: u32,
}

/// The Electron of the health file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Electron {
    pub pid: u32,
    pub running: bool,
}

/// The answer of `--status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub output: String,
    pub electron: Option<Electron>,
}

fn fail(code: ErrorCode, message: impl Into<String>) -> CpdError {
    CpdError::new(code, message)
}

fn root_file(home: &Path) -> PathBuf {
    home.join(".local/share/cpd/app-root")
}

/// `<home>/.local/share/cpd/app-root`; `app_not_installed` when it is missing.
pub fn read_root(home: &Path) -> Result<PathBuf, CpdError> {
    let file = root_file(home);
    let text = fs::read_to_string(&file).map_err(|e| {
        let message = if e.kind() == std::io::ErrorKind::NotFound {
            format!(
                "{} does not exist: run scripts/install-cpd.sh from a checkout",
                file.display()
            )
        } else {
            format!("cannot read {}: {e}", file.display())
        };
        fail(ErrorCode::AppNotInstalled, message)
    })?;
    let line = text.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return Err(fail(
            ErrorCode::AppNotInstalled,
            format!(
                "{} is empty: run scripts/install-cpd.sh from a checkout",
                file.display()
            ),
        ));
    }
    Ok(PathBuf::from(line))
}

/// Writes `app-root` (a temporary file, then a rename).
pub fn write_root(home: &Path, root: &Path) -> Result<(), CpdError> {
    let file = root_file(home);
    let io = |e: std::io::Error| {
        fail(
            ErrorCode::InstallFailed,
            format!("cannot write {}: {e}", file.display()),
        )
    };
    let dir = file.parent().expect("app-root has a parent");
    fs::create_dir_all(dir).map_err(io)?;
    let tmp = dir.join(format!(".app-root.tmp.{}", std::process::id()));
    fs::write(&tmp, format!("{}\n", root.display())).map_err(io)?;
    fs::rename(&tmp, &file).map_err(io)
}

/// A finished call as a result: its output, or the error of a start failure, a deadline or a non-zero exit.
fn finished(
    result: Result<Output, Failed>,
    code: ErrorCode,
    what: &str,
) -> Result<Output, CpdError> {
    match result {
        Ok(out) if out.status.success() => Ok(out),
        Ok(out) => Err(fail(
            code,
            format!(
                "{what} exited {}: {}{}",
                out.status
                    .code()
                    .map_or_else(|| "by a signal".to_owned(), |c| c.to_string()),
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )),
        Err(Failed::Io(e)) => Err(fail(code, format!("{what} could not run: {e}"))),
        Err(Failed::Timeout(e)) => Err(e),
    }
}

/// `pnpm --dir <root> <args>`, bounded.
pub fn pnpm(ports: &Ports, root: &Path, args: &[&str]) -> Result<(), CpdError> {
    let mut cmd = Command::new("pnpm");
    cmd.arg("--dir").arg(root).args(args);
    finished(
        ports.runner.output(&mut cmd, "pnpm", PNPM_DEADLINE),
        ErrorCode::InstallFailed,
        &format!("pnpm {}", args.join(" ")),
    )
    .map(|_| ())
}

/// Builds `web/dist-electron` when missing (or when `rebuild`), then the `chmod` and, on macOS, the `plutil` steps.
pub fn ensure(ports: &Ports, app: &App, rebuild: bool) -> Result<(), CpdError> {
    let web = app.root.join("web");
    if rebuild || !web.join("dist-electron/main.js").is_file() {
        pnpm(
            ports,
            &app.root,
            &["--filter", "cpd-web-client", "run", "build:electron"],
        )?;
    }
    let prebuilds = web.join("node_modules/node-pty/prebuilds");
    if let Ok(entries) = fs::read_dir(&prebuilds) {
        for entry in entries.flatten() {
            let helper = entry.path().join("spawn-helper");
            if let Ok(meta) = fs::metadata(&helper) {
                let mut perms = meta.permissions();
                perms.set_mode(perms.mode() | 0o111);
                fs::set_permissions(&helper, perms).map_err(|e| {
                    fail(
                        ErrorCode::InstallFailed,
                        format!("cannot chmod {}: {e}", helper.display()),
                    )
                })?;
            }
        }
    }
    if app.macos {
        let electron_dir = web.join("node_modules/electron");
        if !electron_dir.join("dist/Electron.app").exists()
            || !electron_dir.join("path.txt").is_file()
        {
            let mut cmd = Command::new("node");
            cmd.arg(electron_dir.join("install.js")).current_dir(&web);
            finished(
                ports.runner.output(&mut cmd, "node", PNPM_DEADLINE),
                ErrorCode::InstallFailed,
                "electron install.js",
            )?;
        }
        let plist = web.join("node_modules/electron/dist/Electron.app/Contents/Info.plist");
        for key in ["CFBundleName", "CFBundleDisplayName"] {
            let mut cmd = Command::new("plutil");
            cmd.args(["-replace", key, "-string", "CPD"])
                .arg(&plist)
                .current_dir(&web);
            finished(
                ports.runner.output(&mut cmd, "plutil", PNPM_DEADLINE),
                ErrorCode::InstallFailed,
                &format!("plutil -replace {key}"),
            )?;
        }
    }
    Ok(())
}

/// The Electron of the health file: `None` when there is no file or no `pid`; a file that does not parse is an error.
pub fn electron(ports: &Ports, app: &App) -> Result<Option<Electron>, CpdError> {
    let file = app.cpd_home.join("run/preview-health.json");
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(fail(
                ErrorCode::Internal,
                format!("cannot read {}: {e}", file.display()),
            ));
        }
    };
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        fail(
            ErrorCode::Internal,
            format!("{} does not parse: {e}", file.display()),
        )
    })?;
    let Some(pid) = value.get("pid").and_then(serde_json::Value::as_u64) else {
        return Ok(None);
    };
    let pid = u32::try_from(pid).map_err(|_| {
        fail(
            ErrorCode::Internal,
            format!("{}: pid {pid} is out of range", file.display()),
        )
    })?;
    Ok(Some(Electron {
        pid,
        running: ports.spawn.alive(Pid(pid)),
    }))
}

/// SIGUSR1 to the running Electron of the health file; its pid, or `None` when none runs.
pub fn reload(ports: &Ports, app: &App) -> Result<Option<u32>, CpdError> {
    match electron(ports, app)? {
        Some(Electron { pid, running: true }) => {
            ports.spawn.signal(Pid(pid), Signal::Usr1).map_err(|e| {
                fail(
                    ErrorCode::SpawnFailed,
                    format!("cannot signal Electron (pid {pid}): {e}"),
                )
            })?;
            Ok(Some(pid))
        }
        _ => Ok(None),
    }
}

/// Plain `cpd app`: ensure, then SIGUSR1 to the running Electron, else spawn one.
pub fn launch(ports: &Ports, app: &App) -> Result<Launch, CpdError> {
    ensure(ports, app, false)?;
    if let Some(pid) = reload(ports, app)? {
        return Ok(Launch {
            action: LaunchAction::Reloaded,
            pid,
        });
    }
    let web = app.root.join("web");
    let log = app.cpd_home.join("run/app.log");
    if let Some(dir) = log.parent() {
        fs::create_dir_all(dir).map_err(|e| {
            fail(
                ErrorCode::SpawnFailed,
                format!("cannot create {}: {e}", dir.display()),
            )
        })?;
    }
    let electron = web.join("node_modules/.bin/electron");
    let pid = ports
        .spawn
        .spawn(&spawn::Launch {
            program: electron.display().to_string(),
            args: vec![".".to_owned()],
            cwd: Some(web),
            setsid: true,
            stdout_to: Some(log),
            // The launching Session's identity is not the app's: strip exactly the
            // five per-session cpd variables (contract §6.6). CPD_HOME, PATH and
            // every other variable stay.
            env_remove: SESSION_ENV_KEYS.iter().map(|k| (*k).to_owned()).collect(),
            ..spawn::Launch::default()
        })
        .map_err(|e| {
            fail(
                ErrorCode::SpawnFailed,
                format!("cannot start {}: {e}", electron.display()),
            )
        })?;
    Ok(Launch {
        action: LaunchAction::Launched,
        pid: pid.0,
    })
}

/// `sh <root>/scripts/preview-web.sh <args>`: its stdout then stderr.
fn script(ports: &Ports, app: &App, args: &[&str]) -> Result<String, CpdError> {
    let mut cmd = Command::new("sh");
    cmd.arg(app.root.join("scripts/preview-web.sh")).args(args);
    let argv = format!("preview-web.sh {}", args.join(" "));
    match ports
        .runner
        .output(&mut cmd, "preview-web", PREVIEW_DEADLINE)
    {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            if out.status.success() {
                Ok(format!("{stdout}{stderr}"))
            } else {
                Err(fail(
                    ErrorCode::SpawnFailed,
                    format!("{argv} exited {}", out.status.code().unwrap_or(-1)),
                )
                .with("argv", argv)
                .with("exit", out.status.code().unwrap_or(-1))
                .with("stdout", stdout)
                .with("stderr", stderr))
            }
        }
        Err(Failed::Io(e)) => Err(fail(
            ErrorCode::SpawnFailed,
            format!("{argv} could not run: {e}"),
        )),
        Err(Failed::Timeout(e)) => Err(e),
    }
}

/// `--preview`, `--reload`, `--off`: the text of `scripts/preview-web.sh`.
pub fn preview(ports: &Ports, app: &App, step: Preview) -> Result<String, CpdError> {
    let (verb, tree) = match &step {
        Preview::On(t) => ("on", t),
        Preview::Reload(t) => ("reload", t),
        Preview::Off => ("off", &None),
    };
    let tree = tree.as_ref().map(|t| t.display().to_string());
    let mut args = vec![verb];
    args.extend(tree.as_deref());
    script(ports, app, &args)
}

/// `--status`.
pub fn status(ports: &Ports, app: &App) -> Result<Status, CpdError> {
    let output = script(ports, app, &["status"])?;
    Ok(Status {
        output,
        electron: electron(ports, app)?,
    })
}
