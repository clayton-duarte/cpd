//! `cpd install` (commands.md §14.4, docs/implementing.md "`cpd install`"): the install, in Rust over `Ports`.
//!
//! The steps, in order: the preflight, the migration check, the verdict of the built binary, the config migration, the adapter's production packages (pnpm, step 4a), the binary, the
//! adapter swap, the follow-ups, the tag. Only the migration check, the verdict and the binary exist so far as
//! tests (`migration_tests`, `adapter_tests`).

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

mod adapter;
mod follow_up;
mod packages;

use crate::app;
use crate::git::run::{Out, run_with};
use crate::home::CpdHome;
use crate::ports::Ports;

#[cfg(test)]
mod app_step_tests;
#[cfg(test)]
mod followup_tests;

/// How long the engine restart waits for the killed engine to release `locks/watch.lock` (docs/implementing.md,
/// "`cpd install`"): the default of `CPD_INSTALL_LOCK_WAIT_SECS`.
pub const DEFAULT_LOCK_WAIT: Duration = Duration::from_secs(60);

/// The built binary's `--version` answers at once: it opens the store, nothing more.
const VERSION_DEADLINE: Duration = Duration::from_secs(60);

/// What `cpd install --root <checkout>` is given, read once at the edge (the environment is not read below it).
pub struct Install {
    /// `--root`: the checkout, a git worktree with `pi/`, `scripts/` and `core/src/store/migrations/`.
    pub root: PathBuf,
    /// `$CARGO_TARGET_DIR`, else `<root>/target`: the built binary is `<target_dir>/release/cpd`.
    pub target_dir: PathBuf,
    /// `$HOME`: the destination is `<home>/.local/bin/cpd`, the adapter `<home>/.local/share/cpd/pi`.
    pub home: PathBuf,
    /// `$CPD_HOME`: its `cpd.db` is the store the verdict is about.
    pub cpd_home: CpdHome,
    /// The pid of the installing process: it names the temporary files (`.cpd.tmp.<pid>`).
    pub pid: u32,
    /// `$CPD_INSTALL_LOCK_WAIT_SECS`, else [`DEFAULT_LOCK_WAIT`]: the bound of the wait for `locks/watch.lock` to be free.
    pub lock_wait: Duration,
    /// `$CPD_PREVIEW_DIR`, else `<home>/.cache/cpd-preview`: a web preview is on while `<preview_dir>/web-state` exists.
    pub preview_dir: PathBuf,
    /// Whether the Electron step runs `plutil` on `Info.plist` (`cfg!(target_os = "macos")`, set at the CLI edge).
    pub macos: bool,
}

/// The migrations of the checkout (contract §3.7).
const MIGRATIONS: &str = "core/src/store/migrations";

/// Why the install stops: the line printed after `cpd install refused: ` (or as it is, for a failure) and the exit.
struct Stop(String);

#[cfg(test)]
mod adapter_tests;

/// What `cpd install` is asked beyond [`Install`]: `cpd install --no-follow-up` sets [`InstallOptions::no_follow_up`].
#[derive(Debug, Default, Clone, Copy)]
pub struct InstallOptions {
    /// `--no-follow-up` (docs/commands.md §14.4): write the binary and the adapter, but run no follow-up, so a
    /// running engine and its views are left as they are. The next install, without the flag, runs them.
    pub no_follow_up: bool,
}

/// Runs the install with the defaults; the human text (refusals, warnings, `installed ...`) goes to `err`. The
/// exit code: 0, or 1 for a refusal, a failure or a follow-up that failed.
pub fn run(ports: &Ports, install: &Install, err: &mut dyn Write) -> i32 {
    run_with_options(ports, install, InstallOptions::default(), err)
}

/// Runs the install with the caller's options; otherwise as [`run`].
pub fn run_with_options(
    ports: &Ports,
    install: &Install,
    options: InstallOptions,
    err: &mut dyn Write,
) -> i32 {
    match steps(ports, install, &options, err) {
        Ok(exit) => exit,
        Err(Stop(text)) => {
            let _ = writeln!(err, "{text}");
            1
        }
    }
}

fn refused(text: impl std::fmt::Display) -> Stop {
    Stop(format!("cpd install refused: {text}"))
}

fn steps(
    ports: &Ports,
    install: &Install,
    options: &InstallOptions,
    err: &mut dyn Write,
) -> Result<i32, Stop> {
    check_migrations(ports, install)?;
    adapter::preflight(install)?;
    check_verdict(ports, install)?;
    migrate_config(ports, install, err)?;
    packages::install_packages(ports, install)?;
    let dest = replace_binary(install)?;
    let displaced = adapter::swap(ports, install)?;
    adapter::verify(install)?;
    adapter::cleanup(ports, install, displaced.as_deref(), err);
    let _ = writeln!(
        err,
        "installed {} and {}",
        dest.display(),
        adapter::link_of(install).display()
    );
    let failed = if options.no_follow_up {
        let _ = writeln!(
            err,
            "cpd install: --no-follow-up; the follow-ups were not run"
        );
        false
    } else {
        follow_up::run(ports, install, &dest, err)
    };
    let failed = electron_step(ports, install, err) || failed;
    follow_up::tag(ports, install, err);
    Ok(i32::from(failed))
}

/// The Electron step (commands.md §14.4 step 7a): builds the checkout's Electron app, writes `app-root` and signals the
/// running Electron; whether a part failed. A failed part skips the rest.
fn electron_step(ports: &Ports, install: &Install, err: &mut dyn Write) -> bool {
    let app = app::App {
        root: install.root.clone(),
        home: install.home.clone(),
        cpd_home: install.cpd_home.root().to_path_buf(),
        preview_dir: install.preview_dir.clone(),
        macos: install.macos,
    };
    if !app.root.join("web/package.json").is_file() {
        let _ = writeln!(err, "no web client in the checkout: electron step skipped");
        return false;
    }
    let mut fail = |part: &str, e: crate::error::CpdError| {
        let _ = writeln!(
            err,
            "cpd install follow-up failed: electron {part}: {}",
            e.message()
        );
        true
    };
    if let Err(e) = app::pnpm(ports, &app.root, &["install", "--frozen-lockfile"]) {
        return fail("a", e);
    }
    if let Err(e) = app::ensure(ports, &app, true) {
        return fail("b", e);
    }
    if let Err(e) = app::write_root(&app.home, &app.root) {
        return fail("c", e);
    }
    if app.preview_dir.join("web-state").exists() {
        let _ = writeln!(
            err,
            "cpd install: web preview is on; Electron was not signalled"
        );
        return false;
    }
    match app::reload(ports, &app) {
        Ok(_) => false,
        Err(e) => fail("d", e),
    }
}

/// The branch check (contract §3.7): refuses a build that carries a migration file that is not, byte for byte, on
/// `origin/main`. A file only origin/main has does not count (the store check decides that).
fn check_migrations(ports: &Ports, install: &Install) -> Result<(), Stop> {
    let root = install.root.as_path();
    let fetched = run_with(
        &*ports.runner,
        root,
        &["fetch", "--quiet", "origin", "main"],
    );
    if !matches!(fetched, Ok(Out { code: 0, .. })) {
        return Err(refused(
            "cannot fetch origin/main to check that this build's migrations are on it.",
        ));
    }
    let dir = root.join(MIGRATIONS);
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') && entry.path().is_file() {
                names.push(name);
            }
        }
    }
    names.sort();
    let mut unmerged = Vec::new();
    for name in names {
        let on_main = run_with(
            &*ports.runner,
            root,
            &["show", &format!("origin/main:{MIGRATIONS}/{name}")],
        );
        let file = fs::read(dir.join(&name)).map_err(|e| {
            Stop(format!(
                "cpd install failed: cannot read {}: {e}",
                dir.join(&name).display()
            ))
        })?;
        match on_main {
            Ok(Out {
                code: 0, stdout, ..
            }) if stdout == String::from_utf8_lossy(&file) => {}
            _ => unmerged.push(name),
        }
    }
    if unmerged.is_empty() {
        return Ok(());
    }
    Err(refused(format!(
        "this build carries migration {} that is not on origin/main, and a preview would apply it to {} before the Workflow merges. \
         Merge main into this branch, or wait until the Workflow merges, then install again.",
        unmerged.join(", "),
        install.cpd_home.db().display()
    )))
}

/// The verdict of the built binary (the store check, contract §3.7): its `--version`, with the caller's `CPD_HOME`.
fn check_verdict(ports: &Ports, install: &Install) -> Result<(), Stop> {
    let built = install.target_dir.join("release/cpd");
    let mut cmd = Command::new(&built);
    cmd.arg("--version")
        .env("CPD_HOME", install.cpd_home.root());
    let out = ports.runner.output(&mut cmd, "cpd", VERSION_DEADLINE);
    let text = |out: &Output| {
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };
    let out = match out {
        Ok(out) if out.status.success() => out,
        Ok(out) => {
            return Err(refused(format!(
                "{} --version failed:\n{}",
                built.display(),
                text(&out).trim_end()
            )));
        }
        Err(e) => {
            return Err(refused(format!(
                "{} --version failed:\n{e:?}",
                built.display()
            )));
        }
    };
    let json: Option<serde_json::Value> = serde_json::from_slice(&out.stdout).ok();
    let Some(store) = json
        .as_ref()
        .and_then(|v| v.get("store"))
        .filter(|s| s.is_null() || s.is_object())
    else {
        return Err(refused(format!(
            "{} --version reports no store verdict:\n{}",
            built.display(),
            text(&out).trim_end()
        )));
    };
    if store.get("opens").and_then(|o| o.as_bool()) != Some(false) {
        return Ok(());
    }
    let refusal = store.get("refusal");
    let field = |name: &str| {
        refusal
            .and_then(|r| r.get(name))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    };
    let fix = match field("code") {
        "store_too_new" => "\nmerge main into this branch, then install again",
        "migration_changed" => {
            "\nthe branch's migration differs from the one this store applied; restore it from origin/main or wait for the store to be repaired"
        }
        "migration_order" => {
            "\nrename the branch's breaking migration to a newer name with scripts/new-migration.sh, then install again"
        }
        _ => "",
    };
    Err(refused(format!(
        "this build cannot open {}: {}{fix}",
        install.cpd_home.db().display(),
        field("message")
    )))
}

/// The config migration and validation (contract §3.7, commands.md §14.4 step 4): the built binary runs
/// `config --migrate` with the caller's `CPD_HOME`; a refusal leaves the config and the installed binary as
/// they are. Each applied migration is reported on stderr.
fn migrate_config(ports: &Ports, install: &Install, err: &mut dyn Write) -> Result<(), Stop> {
    let built = install.target_dir.join("release/cpd");
    let config = install.cpd_home.config();
    let mut cmd = Command::new(&built);
    cmd.args(["config", "--migrate"])
        .env("CPD_HOME", install.cpd_home.root());
    let out = ports.runner.output(&mut cmd, "cpd", VERSION_DEADLINE);
    let text = |out: &Output| {
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };
    let out = match out {
        Ok(out) if out.status.success() => out,
        Ok(out) => {
            return Err(refused(format!(
                "this build cannot migrate {}: {}",
                config.display(),
                follow_up::describe(&text(&out))
            )));
        }
        Err(e) => {
            return Err(refused(format!(
                "this build cannot migrate {}: {e:?}",
                config.display()
            )));
        }
    };
    let json: Option<serde_json::Value> = serde_json::from_slice(&out.stdout).ok();
    if json
        .as_ref()
        .and_then(|v| v.get("ok"))
        .and_then(|ok| ok.as_bool())
        != Some(true)
    {
        return Err(refused(format!(
            "this build cannot migrate {}: {}",
            config.display(),
            follow_up::describe(&text(&out))
        )));
    }
    if let Some(names) = json
        .as_ref()
        .and_then(|v| v.get("migrated"))
        .and_then(|m| m.as_array())
    {
        for name in names.iter().filter_map(|name| name.as_str()) {
            let _ = writeln!(err, "cpd install: config.toml migrated: {name}");
        }
    }
    Ok(())
}

/// The binary replace: a copy at `.cpd.tmp.<pid>` of the destination's directory, mode 755, renamed over the
/// destination, so a running `cpd` keeps its old inode and the path is always the old or the new one.
fn replace_binary(install: &Install) -> Result<PathBuf, Stop> {
    let built = install.target_dir.join("release/cpd");
    let dest_dir = install.home.join(".local/bin");
    let dest = dest_dir.join("cpd");
    let tmp = dest_dir.join(format!(".cpd.tmp.{}", install.pid));
    let failed = |what: &str, e: std::io::Error| Stop(format!("cpd install failed: {what}: {e}"));
    fs::create_dir_all(&dest_dir)
        .map_err(|e| failed(&format!("cannot create {}", dest_dir.display()), e))?;
    let put = || -> std::io::Result<()> {
        fs::copy(&built, &tmp)?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
        fs::rename(&tmp, &dest)
    };
    put().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        failed(
            &format!("cannot install {} as {}", built.display(), dest.display()),
            e,
        )
    })?;
    Ok(dest)
}

#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod steps_tests;
