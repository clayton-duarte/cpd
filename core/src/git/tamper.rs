//! The tamper check (contract §7.5): an Attempt's diff may not touch the files
//! that define the Gate.

use std::path::Path;

use serde::Deserialize;
use serde_json::json;

use super::run::{has_origin, ok, run};
use crate::config::RepoTests;
use crate::error::{CpdError, ErrorCode};

/// Always protected, whatever the repo's list says.
const ALWAYS_EXACT: [&str; 2] = [".config/wt.toml", ".config/cpd.toml"];
const ALWAYS_PREFIX: &str = ".cpd/";

const LIST_FILE: &str = ".config/cpd.toml";

/// `.config/cpd.toml`: the repo's own protected list (§7.5) and its `[tests]` (§7.7). Unknown keys are refused.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RepoConfig {
    #[serde(default)]
    protected: Vec<String>,
    #[serde(default)]
    tests: Option<RepoTests>,
}

/// `.config/cpd.toml` as it is on `<lead>`'s tree (`git show <lead>:.config/cpd.toml`), never an Attempt's. No file is
/// the default; a bad file, an unknown key, an empty protected entry or an absolute one, or a `[tests]` that is not
/// as documented is `config_invalid`. `lead` is the Lead Branch, its SHA or any ref the repo has.
fn repo_config(dir: &Path, lead: &str) -> Result<RepoConfig, CpdError> {
    let invalid = |why: String| {
        CpdError::new(ErrorCode::ConfigInvalid, format!("{LIST_FILE} on {lead}: {why}"))
            .with("file", LIST_FILE)
            .with("rev", lead)
    };
    let spec = format!("{lead}:{LIST_FILE}");
    if ok(dir, &["ls-tree", "--name-only", lead, "--", LIST_FILE])?.trim().is_empty() {
        return Ok(RepoConfig::default());
    }
    let text = ok(dir, &["show", &spec])?;
    let config: RepoConfig = toml::from_str(&text).map_err(|e| invalid(e.to_string()))?;
    for entry in &config.protected {
        if entry.is_empty() || entry.starts_with('/') {
            return Err(invalid(format!("protected entry {entry:?} must be a non-empty relative path")));
        }
    }
    if let Some(tests) = &config.tests {
        tests.validate().map_err(invalid)?;
    }
    Ok(config)
}

/// The repo's protected list, read from `<lead>`'s tree (`git show <lead>:.config/cpd.toml`),
/// never from an Attempt's. No file is an empty list; see [`repo_config`] for what is `config_invalid`.
pub fn protected_list(dir: &Path, lead: &str) -> Result<Vec<String>, CpdError> {
    Ok(repo_config(dir, lead)?.protected)
}

/// The repo's `[tests]` (contract §7.7) as it is on `<rev>`'s tree; `None` when it has no such table.
pub fn repo_tests(dir: &Path, rev: &str) -> Result<Option<RepoTests>, CpdError> {
    Ok(repo_config(dir, rev)?.tests)
}

/// The ref a PR's base is compared at: `origin/<base>`, or `<base>` in a repo with no `origin`.
pub fn base_ref(dir: &Path, base: &str) -> Result<String, CpdError> {
    Ok(if has_origin(dir)? { format!("origin/{base}") } else { base.to_owned() })
}

/// The blob id of `path` in `rev`; `None` when `rev` has no such path.
fn blob(dir: &Path, rev: &str, path: &str) -> Result<Option<String>, CpdError> {
    let spec = format!("{rev}:{path}");
    let out = run(dir, &["rev-parse", "--verify", "--quiet", &spec])?;
    match out.code {
        0 => Ok(Some(out.stdout.trim().to_owned())),
        1 => Ok(None),
        code => Err(CpdError::new(ErrorCode::GitFailed, format!("git rev-parse {spec} exited {code}."))
            .with("stderr", out.stderr)),
    }
}

/// The protected paths that `diff_base...branch` touches, in the diff's order, except those a
/// base merge brings (§7.5): a protected path whose version on `branch` is the version on
/// `lead` or on `base_ref` (the same blob id, or absent in both).
///
/// The diff is `git diff --name-only --no-renames <diff_base>...<branch>`, so renaming a
/// protected file away still shows its old path. The protected paths are
/// `.config/wt.toml`, `.config/cpd.toml`, anything under `.cpd/`, and the list of
/// `.config/cpd.toml` on `lead` (an entry ending in `/` is a directory prefix, any
/// other an exact path). `lead` is the Lead tip; `base_ref` is [`base_ref`]'s.
pub fn touched_protected(
    dir: &Path,
    lead: &str,
    base_ref: &str,
    diff_base: &str,
    branch: &str,
) -> Result<Vec<String>, CpdError> {
    touched_protected_over(dir, lead, &[base_ref], diff_base, branch)
}

/// [`touched_protected`] with several base revisions: a protected path is not touched when its version on `branch`
/// is the one on `lead` or on **any** of `base_refs`. A **stacked** PR passes two (§7.5): `origin/<base>`, its base
/// PR's Lead Branch, and `origin/<root>`, the root base the stack lands on, so a Job that merges the newest root
/// base brings protected files the base PR's branch does not have yet without tampering.
pub fn touched_protected_over(
    dir: &Path,
    lead: &str,
    base_refs: &[&str],
    diff_base: &str,
    branch: &str,
) -> Result<Vec<String>, CpdError> {
    let list = protected_list(dir, lead)?;
    let range = format!("{diff_base}...{branch}");
    let changed = ok(dir, &["diff", "--name-only", "--no-renames", "-z", &range])?;
    let is_protected = |path: &str| {
        ALWAYS_EXACT.contains(&path)
            || path.starts_with(ALWAYS_PREFIX)
            || list.iter().any(|e| if e.ends_with('/') { path.starts_with(e.as_str()) } else { path == e })
    };
    let mut touched = Vec::new();
    for path in changed.split('\0').filter(|p| !p.is_empty() && is_protected(p)) {
        let mine = blob(dir, branch, path)?;
        let mut brought = mine == blob(dir, lead, path)?;
        for base_ref in base_refs {
            brought = brought || mine == blob(dir, base_ref, path)?;
        }
        if !brought {
            touched.push(path.to_owned());
        }
    }
    Ok(touched)
}

/// Whether `path` is the file `entry` names, or inside it when `entry` ends in `/` (the matching of §7.5: no globs).
pub fn path_matches(entry: &str, path: &str) -> bool {
    if entry.ends_with('/') { path.starts_with(entry) } else { path == entry }
}

/// The paths `diff_base...branch` changes (`git diff --name-only --no-renames`), in the diff's order.
pub fn changed_paths(dir: &Path, diff_base: &str, branch: &str) -> Result<Vec<String>, CpdError> {
    let range = format!("{diff_base}...{branch}");
    let changed = ok(dir, &["diff", "--name-only", "--no-renames", "-z", &range])?;
    Ok(changed.split('\0').filter(|p| !p.is_empty()).map(str::to_owned).collect())
}

/// The red-test guard (§7.5 "The red tests", §7.7): the paths `diff_base...branch` changes that an entry of `guarded` matches
/// and no entry of `approved` does, except a path whose version on `branch` is the one on `lead` (a merge brought it).
pub fn touched_guarded(
    dir: &Path,
    lead: &str,
    diff_base: &str,
    branch: &str,
    guarded: &[String],
    approved: &[String],
) -> Result<Vec<String>, CpdError> {
    let mut touched = Vec::new();
    for path in changed_paths(dir, diff_base, branch)? {
        let matches = |entries: &[String]| entries.iter().any(|e| path_matches(e, &path));
        if matches(guarded) && !matches(approved) && blob(dir, branch, &path)? != blob(dir, lead, &path)? {
            touched.push(path);
        }
    }
    Ok(touched)
}

/// [`touched_protected`] as a refusal: `gate_tampered`, with `details.paths`. The
/// merge ends the Attempt `tampered` instead and uses the list directly.
pub fn check(dir: &Path, lead: &str, base_ref: &str, diff_base: &str, branch: &str) -> Result<(), CpdError> {
    check_over(dir, lead, &[base_ref], diff_base, branch)
}

/// [`check`] with several base revisions, as [`touched_protected_over`].
pub fn check_over(dir: &Path, lead: &str, base_refs: &[&str], diff_base: &str, branch: &str) -> Result<(), CpdError> {
    let paths = touched_protected_over(dir, lead, base_refs, diff_base, branch)?;
    if paths.is_empty() {
        return Ok(());
    }
    Err(CpdError::new(ErrorCode::GateTampered, "The diff touches protected paths: revert them.")
        .with("paths", json!(paths)))
}
