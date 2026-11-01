//! Step 4a of `cpd install` (commands.md §14.4, contract §3.7): the adapter's production packages, by pnpm.

use std::fs;
use std::process::{Command, Output};
use std::time::Duration;

use super::{Install, Stop, refused};
use crate::ports::Ports;

/// How long `pnpm install --prod --frozen-lockfile` may run before the install is refused.
const PNPM_DEADLINE: Duration = Duration::from_secs(10 * 60);

/// Installs the root `package.json`'s dependencies beside `pi/`, in `<home>/.local/share/cpd`; does nothing
/// when there is no package.json or it has no dependencies.
pub(super) fn install_packages(ports: &Ports, install: &Install) -> Result<(), Stop> {
    let manifest = install.root.join("package.json");
    let bytes = match fs::read(&manifest) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(refused(format!("cannot read {}: {e}", manifest.display()))),
    };
    let json: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| refused(format!("{} is not valid JSON: {e}", manifest.display())))?;
    if !json
        .get("dependencies")
        .and_then(|d| d.as_object())
        .is_some_and(|d| !d.is_empty())
    {
        return Ok(());
    }
    let lock = install.root.join("pnpm-lock.yaml");
    if !lock.is_file() {
        return Err(refused(
            "package.json has dependencies but the checkout has no pnpm-lock.yaml",
        ));
    }
    let share = install.home.join(".local/share/cpd");
    fs::create_dir_all(&share)
        .map_err(|e| refused(format!("cannot create {}: {e}", share.display())))?;
    let workspace = install.root.join("pnpm-workspace.yaml");
    let mut files = vec![manifest, lock];
    if workspace.is_file() {
        files.push(workspace);
    }
    for file in files {
        let dest = share.join(file.file_name().expect("a file name"));
        fs::copy(&file, &dest).map_err(|e| {
            refused(format!(
                "cannot copy {} to {}: {e}",
                file.display(),
                dest.display()
            ))
        })?;
    }
    let mut cmd = Command::new("pnpm");
    cmd.args(["install", "--prod", "--frozen-lockfile"])
        .current_dir(&share);
    let text = |out: &Output| {
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };
    let failed = |detail: String| {
        refused(format!(
            "pnpm install --prod --frozen-lockfile failed in {}:\n{detail}",
            share.display()
        ))
    };
    match ports.runner.output(&mut cmd, "pnpm", PNPM_DEADLINE) {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(failed(text(&out).trim_end().to_string())),
        Err(e) => Err(failed(format!("{e:?}"))),
    }
}
