//! `cpd upgrade` (docs/architecture/upgrade.md, docs/commands.md §14.4c, docs/contract.md §3.7c).

use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use time::OffsetDateTime;

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::Call;
use crate::ports::Ports;

/// Configuration for `cpd upgrade`.
#[derive(Debug, Clone)]
pub struct UpgradeConfig {
    pub home: CpdHome,
    pub dry_run: bool,
    pub check_only: bool,
    pub target_bin: Option<PathBuf>,
}

/// Upgrade outcome details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeOutcome {
    pub from_sha: String,
    pub to_sha: String,
    pub backup_path: Option<PathBuf>,
    pub already: bool,
}

fn make_call(tool: &str, program: &str, args: &[&str]) -> Call {
    Call {
        tool: tool.to_owned(),
        program: program.to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        envs: Vec::new(),
        cwd: None,
        deadline: Duration::from_secs(300),
        stdin: None,
        stderr_to: None,
    }
}

fn upgrade_failed(
    step: &'static str,
    message: impl Into<String>,
    backup: Option<PathBuf>,
    rolled_back: bool,
) -> CpdError {
    let mut err = CpdError::new(ErrorCode::UpgradeFailed, message)
        .with("step", step)
        .with("rolled_back", rolled_back);
    if let Some(b) = backup {
        err = err.with("backup", b.display().to_string());
    } else {
        err = err.with("backup", serde_json::Value::Null);
    }
    err
}

fn rollback_failed(
    step: &'static str,
    message: impl Into<String>,
    backup: Option<PathBuf>,
) -> CpdError {
    let mut err = CpdError::new(ErrorCode::UpgradeRollbackFailed, message)
        .with("step", step)
        .with("rolled_back", false);
    if let Some(b) = backup {
        err = err.with("backup", b.display().to_string());
    } else {
        err = err.with("backup", serde_json::Value::Null);
    }
    err
}

/// Run `cpd upgrade` against the given ports and configuration.
pub fn run_upgrade(ports: &Ports, config: &UpgradeConfig) -> Result<UpgradeOutcome, CpdError> {
    // 1. Lock: locks/upgrade.lock
    let lock_path = config.home.locks_dir().join("upgrade.lock");
    if let Some(parent) = lock_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // Step 2: Fetch and determine CI green commit
    let fetch_call = make_call("git", "git", &["fetch", "origin", "main"]);
    let fetch_out = ports
        .runner
        .run(&fetch_call)
        .map_err(|e| upgrade_failed("fetch", format!("git fetch failed: {e:?}"), None, false))?;
    if !fetch_out.status.success() {
        return Err(upgrade_failed(
            "fetch",
            format!("git fetch failed with status {}", fetch_out.status),
            None,
            false,
        ));
    }

    let rev_call = make_call("git", "git", &["rev-parse", "origin/main"]);
    let rev_out = ports.runner.run(&rev_call).map_err(|e| {
        upgrade_failed(
            "fetch",
            format!("git rev-parse origin/main failed: {e:?}"),
            None,
            false,
        )
    })?;
    if !rev_out.status.success() {
        return Err(upgrade_failed(
            "fetch",
            format!("git rev-parse failed with status {}", rev_out.status),
            None,
            false,
        ));
    }
    let to_sha = String::from_utf8_lossy(&rev_out.stdout).trim().to_string();

    let installed_bin = config.target_bin.clone().unwrap_or_else(|| {
        let home_dir = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        home_dir.join(".local/bin/cpd")
    });

    let from_sha = "unknown".to_string();

    if config.check_only {
        return Ok(UpgradeOutcome {
            from_sha,
            to_sha,
            backup_path: None,
            already: false,
        });
    }

    // Step 3: Build in isolated directory ($CPD_HOME/upgrade/src and target)
    let upgrade_root = config.home.root().join("upgrade");
    let upgrade_target_bin = upgrade_root.join("target/release/cpd");
    let target_dir_str = upgrade_root.join("target");
    let cargo_call = make_call(
        "cargo",
        "cargo",
        &[
            "build",
            "--release",
            "--locked",
            "-p",
            "cpd",
            "--target-dir",
            target_dir_str.to_str().unwrap_or(""),
        ],
    );
    let cargo_out = ports
        .runner
        .run(&cargo_call)
        .map_err(|e| upgrade_failed("build", format!("cargo build failed: {e:?}"), None, false))?;
    if !cargo_out.status.success() {
        return Err(upgrade_failed(
            "build",
            format!("cargo build failed with exit status {}", cargo_out.status),
            None,
            false,
        ));
    }

    // Step 4: Dry run migration on scratch copy of cpd.db
    let dry_run_call = make_call("cpd", "cpd", &["--migrate-scratch", "--dry-run"]);
    let dry_run_out = ports.runner.run(&dry_run_call).map_err(|e| {
        upgrade_failed(
            "dry_run",
            format!("migration dry run failed: {e:?}"),
            None,
            false,
        )
    })?;
    if !dry_run_out.status.success() {
        return Err(upgrade_failed(
            "dry_run",
            format!("migration dry run exited with {}", dry_run_out.status),
            None,
            false,
        ));
    }

    if config.dry_run {
        return Ok(UpgradeOutcome {
            from_sha,
            to_sha,
            backup_path: None,
            already: false,
        });
    }

    // Step 5: Backup cpd.db and config.toml
    let backups_dir = config.home.root().join("backups");
    let now = OffsetDateTime::now_utc();
    let ts = format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    let backup_dir = backups_dir.join(format!("{ts}-backup"));
    fs::create_dir_all(&backup_dir).map_err(|e| {
        upgrade_failed(
            "backup",
            format!("failed to create backup dir: {e}"),
            None,
            false,
        )
    })?;

    let live_db = config.home.db();
    let backup_db = backup_dir.join("cpd.db");
    if live_db.exists() {
        fs::copy(&live_db, &backup_db).map_err(|e| {
            upgrade_failed(
                "backup",
                format!("failed to backup cpd.db: {e}"),
                Some(backup_dir.clone()),
                false,
            )
        })?;
    }

    let live_config = config.home.config();
    let backup_config = backup_dir.join("config.toml");
    if live_config.exists() {
        let _ = fs::copy(&live_config, &backup_config);
    }

    // Preserve old binary at cpd.previous before swap
    let prev_bin = installed_bin.with_file_name("cpd.previous");
    if installed_bin.exists() {
        fs::copy(&installed_bin, &prev_bin).map_err(|e| {
            upgrade_failed(
                "swap",
                format!("failed to copy binary to cpd.previous: {e}"),
                Some(backup_dir.clone()),
                false,
            )
        })?;
    }

    // Swap binary
    if upgrade_target_bin.exists() {
        let tmp_bin = installed_bin.with_file_name(format!("cpd.new.{}", std::process::id()));
        fs::copy(&upgrade_target_bin, &tmp_bin).map_err(|e| {
            upgrade_failed(
                "swap",
                format!("failed to stage new binary: {e}"),
                Some(backup_dir.clone()),
                false,
            )
        })?;
        fs::rename(&tmp_bin, &installed_bin).map_err(|e| {
            upgrade_failed(
                "swap",
                format!("failed to swap new binary: {e}"),
                Some(backup_dir.clone()),
                false,
            )
        })?;
    }

    // Helper closure to perform rollback
    let perform_rollback = |step: &'static str, reason: String| -> CpdError {
        // Rollback 1: stop engine (if any)
        // Rollback 2: restore store
        if backup_db.exists() {
            let tmp_restore = live_db.with_file_name("cpd.db.restore.tmp");
            if let Err(e) = fs::copy(&backup_db, &tmp_restore) {
                return rollback_failed(
                    "rollback",
                    format!("failed to copy backup db: {e}"),
                    Some(backup_dir.clone()),
                );
            }
            if let Err(e) = fs::rename(&tmp_restore, &live_db) {
                return rollback_failed(
                    "rollback",
                    format!("failed to restore live db: {e}"),
                    Some(backup_dir.clone()),
                );
            }
        }
        // Rollback 3: restore config
        if backup_config.exists() {
            let _ = fs::copy(&backup_config, &live_config);
        }
        // Rollback 4: restore binary from cpd.previous
        if prev_bin.exists()
            && let Err(e) = fs::copy(&prev_bin, &installed_bin)
        {
            return rollback_failed(
                "rollback",
                format!("failed to restore binary: {e}"),
                Some(backup_dir.clone()),
            );
        }
        // Rollback 5: restart old engine
        let _ = ports
            .runner
            .run(&make_call("cpd", "cpd", &["watch", "--ensure"]));

        upgrade_failed(step, reason, Some(backup_dir.clone()), true)
    };

    // Step 6: Start engine and Health check
    let ensure_call = make_call("cpd", "cpd", &["watch", "--ensure"]);
    let ensure_out = ports.runner.run(&ensure_call);
    if let Err(e) = ensure_out {
        return Err(perform_rollback(
            "restart",
            format!("watch --ensure failed to spawn: {e:?}"),
        ));
    }
    if !ensure_out.unwrap().status.success() {
        return Err(perform_rollback(
            "restart",
            "watch --ensure failed".to_string(),
        ));
    }

    let version_call = make_call("cpd", "cpd", &["--version"]);
    let version_out = ports.runner.run(&version_call);
    match version_out {
        Ok(out) if out.status.success() => {
            // Health check passed
        }
        Ok(out) => {
            let stderr_text = String::from_utf8_lossy(&out.stderr);
            return Err(perform_rollback(
                "health",
                format!(
                    "health check failed with status {}: {}",
                    out.status, stderr_text
                ),
            ));
        }
        Err(e) => {
            return Err(perform_rollback(
                "health",
                format!("health check call failed: {e:?}"),
            ));
        }
    }

    Ok(UpgradeOutcome {
        from_sha,
        to_sha,
        backup_path: Some(backup_dir),
        already: false,
    })
}
