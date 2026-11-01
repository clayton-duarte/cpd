//! The retired-term guard (docs/research/glossary-audit-rename.md, "Guard test (`ren-guard`)"): the object is a
//! Workflow (pro-d136), and `PR` means only the GitHub pull request. The guard scans the live prose, the skills,
//! the spec and help text and the non-test Rust and `pi/lib` text, and lists every offender as `path:line: text`.
//! Red today: cpd's live text still calls the Workflow object `PR`; the docs and implementation Jobs of the rename
//! lower the count until it is green.

use std::path::{Path, PathBuf};

/// The repo root: `cli/..`.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// The live surfaces of the map: the docs (not history), the skills, the adapter and the non-test Rust. `CONTEXT.md`
/// and `README.md` are added by `sources_of_interest`.
const DIRS: [&str; 7] = ["docs", "pi/skills", "pi/lib", "cli/src", "core/src", "engine/src", "tui/src"];

/// Directories that hold history or tests, never scanned: `docs/adr|archive|research` (the map allowed them), the
/// test areas, `node_modules` and the applied migrations (whose SQL keeps the old names by design).
const SKIP_DIRS: [&str; 7] = ["adr", "archive", "research", "node_modules", "tests", "test", "migrations"];

fn is_source(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "rs" || e == "ts" || e == "md")
}

/// A test file: a name with `test`, in a `tests`/`test` directory. Test fixtures that build an old store are an
/// allowed place, so no test file is scanned.
fn is_test_file(path: &Path) -> bool {
    path.file_name().unwrap().to_string_lossy().contains("test")
        || path.components().any(|c| c.as_os_str() == "tests" || c.as_os_str() == "test")
}

/// Appends every non-test source file under `dir`, sorted.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = read.map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if path.file_name().is_some_and(|n| SKIP_DIRS.contains(&n.to_string_lossy().as_ref())) {
                continue;
            }
            sources(&path, out);
        } else if is_source(&path) && !is_test_file(&path) {
            out.push(path);
        }
    }
}

fn sources_of_interest() -> Vec<PathBuf> {
    let root = root();
    let mut files = vec![root.join("CONTEXT.md"), root.join("README.md")];
    for dir in DIRS {
        sources(&root.join(dir), &mut files);
    }
    files
}

/// The lines of a file, as `(number, text)`: a `.rs` file stops at its first column-0 `#[cfg(test)]` (an inline test
/// module closes the file), so no test line is scanned.
fn lines(path: &Path) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(path).unwrap();
    let rs = path.extension().is_some_and(|e| e == "rs");
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if rs && line.starts_with("#[cfg(test)]") {
            break;
        }
        out.push((n + 1, line.to_string()));
    }
    out
}

/// The retired object vocabulary the map names (docs/research/glossary-audit-rename.md, "Guard test"): the forms
/// that name the Workflow object `PR`, or carry a cpd-1 word. The bare word `PR` is never listed: grep cannot tell
/// its sense (pull request versus object). Matched lower-case as substrings.
const FORBIDDEN: &[&str] = &[
    // The object command group (R3): `workflow add|edit|describe|cancel|start`.
    "cpd pr add",
    "cpd pr edit",
    "cpd pr describe",
    "cpd pr cancel",
    "cpd pr start",
    // The selector on the commands that moved (R4). `review submit --pr <url>` names the pull request and stays.
    "--pr <key>",
    // The tables, the object columns and the cpd-1 words.
    "prs",
    "pr_needs",
    "base_pr",
    "until-pr",
    "until_pr",
    "prkey",
    "prrow",
    // The six object Event kinds (`pr-opened`/`pr-merged` and `pr-feedback` name the pull request and stay).
    "pr-added",
    "pr-edited",
    "pr-described",
    "pr-cancelled",
    "pr-started",
    "pr-base-changed",
    // The object error codes (`pr_closed`/`pr_not_closed` name the pull request's state and stay; `pr_not_ready` is
    // the pull request's ready state code, kept by `core/src/error.rs`'s `PrNotReady`).
    "pr_required",
    "invalid_pr_id",
    "pr_exists",
    "pr_in_use",
    "pr_empty",
    "pr_locked",
    "pr_ended",
    "pr_title_required",
    "pr_start_failed",
    "pr_adopted",
    "pr_not_open",
    "pr_not_found",
    "pr_other_plan",
    "explore_pr_forbidden",
    "needs_other_pr",
    "until_pr_not_incident",
    "base_pr_unknown",
    "base_pr_unavailable",
    "base_pr_other_repo",
    // The Gate log name.
    "pr-<key>",
    // The retired phrases.
    "pr key",
    "pr keys",
    "plan's prs",
    "prs of the plan",
    "pr needs",
    "base pr",
    "waiting pr",
    "pr status",
    "workflow worktree",
    "dispatch or pr",
    "phase pr",
    "pr.gate",
    "sync job",
];

/// The cpd-1 word `workflow`/`workflows`, lower-case, outside the allowed phrases (`GitHub Actions workflow`,
/// `[deploys] workflows`, `workflow_runs`). The object is spelled **Workflow**; a GitHub Actions workflow is written
/// in full. Only checked for prose (`.md`); a Rust identifier or a `pi/lib` one may spell `workflow` lower-case.
fn bare_workflow(line: &str) -> bool {
    // A line that is about GitHub Actions or the deploy policy names the GitHub object, not the cpd one. A line that
    // names the renamed command or flag speaks the target vocabulary, so the bare-word rule skips it.
    let lower = line.to_lowercase();
    if lower.contains("github actions") || lower.contains("[deploys]") || lower.contains("workflow_runs") {
        return false;
    }
    if lower.contains("cpd workflow") || lower.contains("--workflow") || lower.contains("workflow add")
        || lower.contains("workflow edit") || lower.contains("workflow describe") || lower.contains("workflow cancel")
        || lower.contains("workflow start")
    {
        return false;
    }
    let mut offset = 0;
    while let Some(i) = line[offset..].find("workflow") {
        let at = offset + i;
        let after = &line[at + "workflow".len()..];
        let plural = after.starts_with('s');
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let before_ok = at == 0 || !line[..at].ends_with(word);
        let rest = if plural { &after[1..] } else { after };
        if before_ok && (rest.is_empty() || !rest.starts_with(word)) {
            return true;
        }
        offset = at + 1;
    }
    false
}

/// `path:line: text` for each line of the live surfaces holding a retired term, except `allowed` files (paths
/// relative to the root, with the reason in the caller).
fn offenders(forbidden: &[&str], allowed: &[&str]) -> Vec<String> {
    let root = root();
    let files = sources_of_interest();
    assert!(files.len() > 100, "the guard saw {} files: it scans the wrong directory", files.len());
    let mut found = Vec::new();
    for path in files {
        let rel = path.strip_prefix(&root).unwrap().to_string_lossy().to_string();
        if allowed.contains(&rel.as_str()) {
            continue;
        }
        let md = path.extension().is_some_and(|e| e == "md");
        let mut in_history = false;
        for (n, line) in lines(&path) {
            // CONTEXT.md's `_Avoid_` lines and its "Retired words" section name the words on purpose; the contract's
            // legacy record is history too.
            if rel == "CONTEXT.md" {
                if line.starts_with("## ") {
                    in_history = line.trim_end() == "## Retired words";
                }
                if line.trim_start().starts_with("_Avoid_:") || in_history {
                    continue;
                }
            }
            if rel == "docs/contract.md" {
                if line.starts_with("### ") {
                    in_history = false;
                }
                if line.contains("**The legacy record.**") {
                    in_history = true;
                }
                if in_history {
                    continue;
                }
            }
            let lower = line.to_lowercase();
            if forbidden.iter().any(|f| lower.contains(f)) || (md && bare_workflow(&line)) {
                found.push(format!("{rel}:{n}: {}", line.trim_end()));
            }
        }
    }
    found
}

#[test]
fn guard() {
    // The store's `schema.rs` holds the legacy table lists and the fixture lists the rename keeps (map: allowed); the
    // rest of the retired vocabulary moves with the docs and implementation Jobs. Test-only modules are declared under
    // `#[cfg(test)]`, so their name does not say so; the map allows test fixtures that build an old store.
    let allowed = [
        "core/src/store/schema.rs",
        "core/src/plan/sql_trace.rs",
        "core/src/fakes/git/scenario.rs",
        "core/src/fakes/tmux/kit.rs",
        "core/src/host/tmux/no_inbox.rs",
        "engine/src/watch/pill_texts.rs",
        "engine/src/watch/workers/no_inbox.rs",
        "tui/src/palette/colours.rs",
        "tui/src/tree/look.rs",
        "tui/src/dialog/pane.rs",
        "tui/src/pill_colours.rs",
        "tui/src/updating_label.rs",
        // R21 keeps `plan show`'s inner keys (base_pr), so the map allows the file.
        "core/src/plan/show.rs",
    ];
    let found = offenders(FORBIDDEN, &allowed);
    assert!(
        found.is_empty(),
        "the retired object vocabulary is still live ({} offenders):\n{}",
        found.len(),
        found.join("\n")
    );
}
