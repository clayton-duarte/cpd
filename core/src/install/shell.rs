//! Shell app install for macOS in `cpd install`.
//!
//! Normative target: `web/CONTRACT.md` §30.3.

use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::install::Install;
use crate::ports::Ports;

const SHELL_RELEASE_TAG: &str = "app-v0.1.0";
const GH_DEADLINE: Duration = Duration::from_secs(300);

/// Ensures CPD.app shell is installed on macOS (web/CONTRACT.md §30.3).
/// Skipped on Linux or non-macOS platforms.
pub fn ensure_shell(ports: &Ports, install: &Install, err: &mut dyn Write) {
    if !install.macos {
        return;
    }

    let applications_dir = Path::new("/Applications");
    let bundle = applications_dir.join("CPD.app");

    // 1. Download release with gh
    let mut gh_cmd = Command::new("gh");
    gh_cmd.args([
        "release",
        "download",
        SHELL_RELEASE_TAG,
        "--repo",
        "clayton-duarte/cpd",
        "--pattern",
        "*.zip",
        "--dir",
        applications_dir.to_str().unwrap_or("/Applications"),
    ]);

    match ports.runner.output(&mut gh_cmd, "gh", GH_DEADLINE) {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let _ = writeln!(err, "cpd install: gh release download failed: {stderr}");
            return;
        }
        Err(e) => {
            let _ = writeln!(err, "cpd install: gh release download failed: {e:?}");
            return;
        }
    }

    // 2. Clear quarantine with xattr
    let mut xattr_cmd = Command::new("xattr");
    xattr_cmd.args(["-dr", "com.apple.quarantine"]).arg(&bundle);
    let _ = ports.runner.output(&mut xattr_cmd, "xattr", GH_DEADLINE);
}
