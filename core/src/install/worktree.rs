//! Preview worktree setup and synchronization for `cpd install`.
//!
//! Normative target: `web/CONTRACT.md` §31.1.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::app;
use crate::git::run::run_with;
use crate::install::Install;
use crate::ports::Ports;

/// Resolves the preview worktree path.
pub fn preview_worktree_path(install: &Install) -> PathBuf {
    std::env::var_os("CPD_WEB_PREVIEW_WORKTREE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            install
                .root
                .parent()
                .map(|p| p.join("cpd.preview.web"))
                .unwrap_or_else(|| install.root.join("../cpd.preview.web"))
        })
}

/// Ensures the preview worktree exists and is synchronized (web/CONTRACT.md §31.1).
///
/// If missing:
/// - `git worktree add -B preview/web <path> origin/main`
/// - `pnpm install --frozen-lockfile` (in the worktree)
///
/// If already present:
/// - Syncs to `origin/main` preserving uncommitted changes as a patch.
pub fn ensure_preview_worktree(ports: &Ports, install: &Install, err: &mut dyn Write) {
    let preview_worktree = preview_worktree_path(install);

    if !preview_worktree.exists() {
        // Only attempt creation if explicitly configured or parent directory exists / preview is relevant
        let wt_str = match preview_worktree.to_str() {
            Some(s) => s,
            None => return,
        };
        let add_out = run_with(
            &*ports.runner,
            &install.root,
            &[
                "worktree",
                "add",
                "-B",
                "preview/web",
                wt_str,
                "origin/main",
            ],
        );
        match add_out {
            Ok(out) if out.code == 0 => {}
            Ok(out) => {
                let _ = writeln!(
                    err,
                    "cpd install: failed to create preview worktree at {}: {}",
                    preview_worktree.display(),
                    out.stderr
                );
                return;
            }
            Err(e) => {
                let _ = writeln!(
                    err,
                    "cpd install: failed to create preview worktree at {}: {e}",
                    preview_worktree.display()
                );
                return;
            }
        }

        if let Err(e) = app::pnpm(ports, &preview_worktree, &["install", "--frozen-lockfile"]) {
            let _ = writeln!(
                err,
                "cpd install: preview worktree pnpm install failed: {}",
                e.message()
            );
        }
        return;
    }

    // Existing worktree sync: preserve uncommitted changes and reset to origin/main.
    sync_existing_worktree(ports, &preview_worktree, err);
}

fn sync_existing_worktree(ports: &Ports, preview_worktree: &std::path::Path, err: &mut dyn Write) {
    // Step 1: Save uncommitted changes as a patch (tracked edits and untracked files).
    let diff_out = run_with(&*ports.runner, preview_worktree, &["diff", "HEAD"]);
    let patch_content = match diff_out {
        Ok(out) if out.code == 0 => out.stdout,
        _ => String::new(),
    };

    let patch_file = preview_worktree.join(".cpd-preview-wip.patch");
    if !patch_content.is_empty() {
        let _ = crate::atomic::write_atomic(&patch_file, &patch_content);
    }

    // Step 2: Reset preview worktree to origin/main
    let _ = run_with(
        &*ports.runner,
        preview_worktree,
        &["reset", "--hard", "origin/main"],
    );

    // Step 3: Re-apply patch with 3-way merge
    let apply_out = run_with(
        &*ports.runner,
        preview_worktree,
        &[
            "apply",
            "-3",
            patch_file.to_str().unwrap_or(".cpd-preview-wip.patch"),
        ],
    );
    let conflict = match apply_out {
        Ok(out) if out.code != 0 => true,
        Err(_) => true,
        _ => false,
    };

    if conflict {
        let _ = writeln!(
            err,
            "cpd install: preview sync conflict while re-applying patch; patch preserved at {}",
            patch_file.display()
        );
        return;
    }

    // If applied cleanly, remove the temporary patch file.
    let _ = fs::remove_file(&patch_file);

    // Step 4: Run pnpm install in the preview worktree
    if let Err(e) = app::pnpm(ports, preview_worktree, &["install"]) {
        let _ = writeln!(
            err,
            "cpd install: preview sync pnpm install failed: {}",
            e.message()
        );
    }
}
