//! The tamper check (contract §7.5): an Attempt's diff may not touch the files
//! that define the Gate.

use std::collections::HashSet;
use std::path::Path;

use serde_json::json;

use super::run::{has_origin, ok, run};
use crate::error::{CpdError, ErrorCode};

/// Always protected, whatever the Workflow's policy says; the Workflow's `protected.paths` (§7.5) come on top of it.
const ALWAYS_PREFIX: &str = ".cpd/";

/// The ref a Workflow's base is compared at: `origin/<base>`, or `<base>` in a repo with no `origin`.
pub fn base_ref(dir: &Path, base: &str) -> Result<String, CpdError> {
    Ok(if has_origin(dir)? {
        format!("origin/{base}")
    } else {
        base.to_owned()
    })
}

/// The blob id of `path` in `rev`; `None` when `rev` has no such path.
fn blob(dir: &Path, rev: &str, path: &str) -> Result<Option<String>, CpdError> {
    let spec = format!("{rev}:{path}");
    let out = run(dir, &["rev-parse", "--verify", "--quiet", &spec])?;
    match out.code {
        0 => Ok(Some(out.stdout.trim().to_owned())),
        1 => Ok(None),
        code => Err(CpdError::new(
            ErrorCode::GitFailed,
            format!("git rev-parse {spec} exited {code}."),
        )
        .with("stderr", out.stderr)),
    }
}

/// The protected paths that `diff_base...branch` touches, in the diff's order, except those a
/// base merge brings (§7.5): a protected path whose version on `branch` is the version on
/// `lead` or on `base_ref` (the same blob id, or absent in both).
///
/// The diff is `git diff --name-only --no-renames <diff_base>...<branch>`, so renaming a
/// protected file away still shows its old path. The protected paths are
/// anything under `.cpd/` and the Workflow's `protected` list (an entry ending in `/` is a directory prefix, any
/// other an exact path). `lead` is the Lead tip; `base_ref` is [`base_ref`]'s.
pub fn touched_protected(
    dir: &Path,
    lead: &str,
    list: &[String],
    base_ref: &str,
    diff_base: &str,
    branch: &str,
) -> Result<Vec<String>, CpdError> {
    touched_protected_over(dir, lead, list, &[base_ref], diff_base, branch)
}

/// [`touched_protected`] with several base revisions: a protected path is not touched when its version on `branch`
/// is the one on `lead` or on **any** of `base_refs`. A **stacked** Workflow passes two (§7.5): `origin/<base>`, its base
/// Workflow's Lead Branch, and `origin/<root>`, the root base the stack lands on, so a Job that merges the newest root
/// base brings protected files the base Workflow's branch does not have yet without tampering.
pub fn touched_protected_over(
    dir: &Path,
    lead: &str,
    list: &[String],
    base_refs: &[&str],
    diff_base: &str,
    branch: &str,
) -> Result<Vec<String>, CpdError> {
    let range = format!("{diff_base}...{branch}");
    let changed = ok(dir, &["diff", "--name-only", "--no-renames", "-z", &range])?;
    let is_protected =
        |path: &str| path.starts_with(ALWAYS_PREFIX) || list.iter().any(|e| path_matches(e, path));
    let mut touched = Vec::new();
    for path in changed
        .split('\0')
        .filter(|p| !p.is_empty() && is_protected(p))
    {
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

/// What the widened tamper check needs beyond the protected list (contract §7.5 "The Job's touches", hs-n641): the Job's
/// kind, its Spec `touches`, the repo's `[generated] paths` and the Job's `touches_approved`.
#[derive(Clone, Copy, Debug)]
pub struct Touches<'a> {
    /// The Job's kind: `open`, `update` and the explore kinds run no touches check.
    pub kind: &'a str,
    /// The Job's Spec `touches`; an entry is an exact path, a directory ending in `/`, or a glob.
    pub touches: &'a [String],
    /// The repo policy's `[generated] paths` (§8.3), matched by the `protected` rule.
    pub generated: &'a [String],
    /// The Job's `touches_approved` (§2.2), read from the store when the check runs.
    pub approved: &'a [String],
}

impl Touches<'_> {
    /// Whether the Job's kind runs no touches check (contract §7.5): `open`, `update`, `scout` and `research`.
    pub fn skipped(&self) -> bool {
        matches!(self.kind, "open" | "update" | "scout" | "research")
    }
}

/// The paths `diff_base...branch` changes outside the Job's `touches`, with the three exemptions of §7.5 (a base merge,
/// the repo's `[generated] paths`, the Job's `touches_approved`), in the diff's order. A skipped kind returns none.
pub fn touched_outside(
    dir: &Path,
    lead: &str,
    touches: &Touches<'_>,
    base_refs: &[&str],
    diff_base: &str,
    branch: &str,
) -> Result<Vec<String>, CpdError> {
    if touches.skipped() {
        return Ok(Vec::new());
    }
    let mut outside = Vec::new();
    for path in changed_paths(dir, diff_base, branch)? {
        let listed = touches.touches.iter().any(|e| touches_matches(e, &path))
            || touches.generated.iter().any(|e| path_matches(e, &path))
            || touches.approved.iter().any(|e| path_matches(e, &path));
        if listed {
            continue;
        }
        // A base merge brings it: its version on the branch equals the one on the Lead tip or a base ref (§7.5).
        let mine = blob(dir, branch, &path)?;
        let mut brought = mine == blob(dir, lead, &path)?;
        for base_ref in base_refs {
            brought = brought || mine == blob(dir, base_ref, &path)?;
        }
        if !brought {
            outside.push(path);
        }
    }
    Ok(outside)
}

/// Whether `path` matches a `touches` entry (contract §7.5): equal, a directory prefix when the entry ends in `/`, or a
/// glob under the repo's glob rules (`job add`'s tree check: `?` one character but not `/`, `*` a run but not `/`, `**`
/// any run, `[...]` a set). `touches` supports globs, unlike `protected`.
pub fn touches_matches(entry: &str, path: &str) -> bool {
    if entry.ends_with('/') {
        path.starts_with(entry)
    } else {
        crate::job::add::glob_matches(entry, path)
    }
}

/// Whether `path` is the file `entry` names, or inside it when `entry` ends in `/` (the matching of §7.5: no globs).
pub fn path_matches(entry: &str, path: &str) -> bool {
    if entry.ends_with('/') {
        path.starts_with(entry)
    } else {
        path == entry
    }
}

/// The paths `diff_base...branch` changes (`git diff --name-only --no-renames`), in the diff's order.
pub fn changed_paths(dir: &Path, diff_base: &str, branch: &str) -> Result<Vec<String>, CpdError> {
    let range = format!("{diff_base}...{branch}");
    let changed = ok(dir, &["diff", "--name-only", "--no-renames", "-z", &range])?;
    Ok(changed
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect())
}

/// The paths `diff_base...sha` changes that differ from EVERY ref of `compare_with` (contract §7.3, "The written files"), in
/// the range diff's order: one range diff, then one tree diff and one merge-base lookup per ref (one more tree diff only
/// when a ref forks from `diff_base` elsewhere than `sha` does).
/// A path that equals a ref's version still counts when that ref left it unchanged since its merge base with
/// `diff_base`: the Job itself reverted it, a base merge did not bring it.
pub fn written_paths(
    dir: &Path,
    diff_base: &str,
    sha: &str,
    compare_with: &[&str],
) -> Result<Vec<String>, CpdError> {
    let mut paths = changed_paths(dir, diff_base, sha)?;
    let sha_fork = ok(dir, &["merge-base", diff_base, sha])?.trim().to_owned();
    for rev in compare_with {
        let out = ok(
            dir,
            &["diff", "--name-only", "--no-renames", "-z", rev, sha],
        )?;
        let differs: HashSet<&str> = out.split('\0').filter(|p| !p.is_empty()).collect();
        let fork = ok(dir, &["merge-base", diff_base, rev])?.trim().to_owned();
        // `rev` equals `sha` on a path outside `differs`, so "rev left it unchanged since `fork`" is "sha on it equals
        // `fork`'s": a path of the range diff differs from the range's own merge base, so when `fork` is that merge base
        // (the usual case) no path qualifies and no further diff runs.
        if fork == sha_fork {
            paths.retain(|p| differs.contains(p.as_str()));
            continue;
        }
        let fork_diff = ok(
            dir,
            &["diff", "--name-only", "--no-renames", "-z", &fork, sha],
        )?;
        let sha_moved: HashSet<&str> = fork_diff.split('\0').filter(|p| !p.is_empty()).collect();
        paths.retain(|p| differs.contains(p.as_str()) || !sha_moved.contains(p.as_str()));
    }
    Ok(paths)
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
        if matches(guarded)
            && !matches(approved)
            && blob(dir, branch, &path)? != blob(dir, lead, &path)?
        {
            touched.push(path);
        }
    }
    Ok(touched)
}

/// [`touched_protected`] as a refusal: `gate_tampered`, with `details.paths`. The
/// merge ends the Attempt `tampered` instead and uses the list directly.
pub fn check(
    dir: &Path,
    lead: &str,
    list: &[String],
    base_ref: &str,
    diff_base: &str,
    branch: &str,
) -> Result<(), CpdError> {
    check_over(dir, lead, list, &[base_ref], diff_base, branch)
}

/// [`check`] with several base revisions, as [`touched_protected_over`].
pub fn check_over(
    dir: &Path,
    lead: &str,
    list: &[String],
    base_refs: &[&str],
    diff_base: &str,
    branch: &str,
) -> Result<(), CpdError> {
    let paths = touched_protected_over(dir, lead, list, base_refs, diff_base, branch)?;
    if paths.is_empty() {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::GateTampered,
        "The diff touches protected paths: revert them.",
    )
    .with("paths", json!(paths)))
}

/// One test case found in a test file: its scoped identity and how many assertions its body holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCase {
    /// The scoped identity: the Rust module path (`m::a`) or the JS describe suites (`s > a`) before the name, and
    /// `#2`, `#3` after it for a later case of the same scope and name.
    pub name: String,
    pub assertions: usize,
}

const JS_CASES: [&str; 4] = ["it(", "test(", "it.only(", "test.only("];
const JS_SUITES: [&str; 4] = ["describe(", "describe.only(", "describe.skip(", "suite("];

/// The text of the first quoted title after an opening call such as `it(`.
fn quoted_title(rest: &str) -> Option<String> {
    let q = rest
        .chars()
        .next()
        .filter(|c| matches!(c, '"' | '\'' | '`'))?;
    let body = &rest[1..];
    body.find(q).map(|end| body[..end].to_owned())
}

/// The identifier after `keyword ` at the start of `t`.
fn ident_after(t: &str, keyword: &str) -> Option<String> {
    let rest = t.strip_prefix(keyword)?;
    let n: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!n.is_empty()).then_some(n)
}

enum Opener {
    Case(String),
    /// A scope: its label and the separator that follows it in an identity.
    Scope(String, &'static str),
}

/// What `t` (a line without its leading attributes) opens: a Rust `mod`, a JS suite, a JS case, or (after a
/// `#[test]`) a Rust `fn`.
fn opener(t: &str, rust_test: bool) -> Option<Opener> {
    for open in JS_CASES {
        if let Some(rest) = t.strip_prefix(open) {
            return quoted_title(rest).map(Opener::Case);
        }
    }
    for open in JS_SUITES {
        if let Some(rest) = t.strip_prefix(open) {
            return quoted_title(rest).map(|l| Opener::Scope(l, " > "));
        }
    }
    let mut t = t;
    while let Some(rest) = t.strip_prefix("pub ").or_else(|| t.strip_prefix("async ")) {
        t = rest;
    }
    if t.starts_with("mod ") && !t.trim_end().ends_with(';') {
        return ident_after(t, "mod ").map(|l| Opener::Scope(l, "::"));
    }
    if rust_test {
        return ident_after(t, "fn ").map(Opener::Case);
    }
    None
}

/// The assertions in `s`: each `assert…(`/`assert…!(`/`assert.…` and each `expect(` at a word start.
fn count_assertions(s: &str) -> usize {
    let b = s.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut n = 0;
    let mut i = 0;
    while i < b.len() {
        if i > 0 && word(b[i - 1]) {
            i += 1;
            continue;
        }
        if b[i..].starts_with(b"expect(") {
            n += 1;
        } else if b[i..].starts_with(b"assert") {
            let mut j = i;
            while j < b.len() && word(b[j]) {
                j += 1;
            }
            if j < b.len() && matches!(b[j], b'!' | b'(' | b'.') {
                n += 1;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    n
}

/// Replace comments and literals with spaces while retaining offsets and newlines. The declaration scanner uses
/// the original text for JS titles, but only code contributes braces or assertions. Rust lifetimes stay code.
fn code_only(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = bytes.to_vec();
    let rust = src.contains("#[test") || src.contains("#[tokio::test");
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else if bytes[i] == b'r' && {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] == b'#' {
                j += 1;
            }
            j < bytes.len() && bytes[j] == b'"'
        } {
            let mut quote = i + 1;
            while bytes[quote] == b'#' {
                quote += 1;
            }
            let hashes = quote - i - 1;
            i = quote + 1;
            while i < bytes.len() {
                if bytes[i] == b'"'
                    && bytes
                        .get(i + 1..i + 1 + hashes)
                        .is_some_and(|s| s.iter().all(|b| *b == b'#'))
                {
                    i += 1 + hashes;
                    break;
                }
                i += 1;
            }
        } else if matches!(bytes[i], b'"' | b'\'' | b'`') {
            let quote = bytes[i];
            // A Rust lifetime has no closing quote after the character. A char literal does.
            if rust && quote == b'\'' && bytes.get(i + 1) != Some(&b'\\') {
                let next = src[i + 1..].chars().next().map_or(0, char::len_utf8);
                if bytes.get(i + 1 + next) != Some(&b'\'') {
                    i += 1;
                    continue;
                }
            }
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else {
            i += 1;
            continue;
        }
        for b in &mut out[start..i] {
            if *b != b'\n' && *b != b'\r' {
                *b = b' ';
            }
        }
    }
    String::from_utf8(out).expect("masking whole literals preserves UTF-8")
}

/// Braces in code whose comments and literals have been masked.
fn braces(s: &str) -> (i32, i32) {
    (
        s.bytes().filter(|b| *b == b'{').count() as i32,
        s.bytes().filter(|b| *b == b'}').count() as i32,
    )
}

struct Frame {
    depth: i32,
    seen: bool,
    label: String,
    sep: &'static str,
    case: Option<usize>,
}

/// The test cases in `src`, in order: Rust `#[test]` fns and `it`/`test` calls (node:test, vitest, jest), each with
/// its scoped identity and the assertions of its body, a declaration line's included. Scopes are Rust `mod`s and JS
/// `describe` suites; braces end a case.
pub fn test_cases(src: &str) -> Vec<TestCase> {
    let mut cases: Vec<TestCase> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut depth = 0;
    let mut rust_test = false;
    let code = code_only(src);
    for (line, masked) in src.lines().zip(code.lines()) {
        let mut t = masked.trim_start();
        while t.starts_with("#[") {
            let Some(end) = t.find(']') else { break };
            if t.starts_with("#[test") || t.starts_with("#[tokio::test") {
                rust_test = true;
            }
            t = t[end + 1..].trim_start();
        }
        if t.is_empty() {
            continue;
        }
        let depth_before = depth;
        let mut fresh = false;
        let original = &line[line.len() - t.len()..];
        if let Some(found) = opener(original, rust_test) {
            let (label, sep, case) = match found {
                Opener::Case(name) => {
                    rust_test = false;
                    let scope: String = stack
                        .iter()
                        .filter(|f| f.case.is_none())
                        .map(|f| format!("{}{}", f.label, f.sep))
                        .collect();
                    let mut id = format!("{scope}{name}");
                    let mut n = 1;
                    while cases.iter().any(|c| c.name == id) {
                        n += 1;
                        id = format!("{scope}{name}#{n}");
                    }
                    cases.push(TestCase {
                        name: id,
                        assertions: 0,
                    });
                    (name, "", Some(cases.len() - 1))
                }
                Opener::Scope(label, sep) => (label, sep, None),
            };
            stack.push(Frame {
                depth: depth_before,
                seen: false,
                label,
                sep,
                case,
            });
            fresh = true;
        }
        let (open, close) = braces(t);
        depth += open - close;
        for f in &mut stack {
            f.seen |= depth > f.depth;
        }
        if fresh
            && open > 0
            && let Some(f) = stack.last_mut()
        {
            f.seen = true;
        }
        if let Some(i) = stack.iter().rev().find_map(|f| f.case) {
            cases[i].assertions += count_assertions(t);
        }
        if fresh
            && open == 0
            && t.trim_end().ends_with(");")
            && let Some(f) = stack.last_mut()
        {
            f.seen = true;
            f.depth = depth;
        }
        while stack.last().is_some_and(|f| f.seen && depth <= f.depth) {
            stack.pop();
        }
    }
    cases
}

/// Whether `approved` names the case `id`: by its scoped identity, or by its bare name.
fn is_approved(approved: &[String], id: &str) -> bool {
    let bare = id.rsplit([':', '>']).next().unwrap_or(id).trim();
    approved.iter().any(|a| a == id || a == bare)
}

/// `Ok` when `head` keeps every test case of `base` (matched by scoped identity) with no fewer assertions, or the
/// case is in `approved`; otherwise `tests_removed`.
pub fn check_tests_kept(base: &str, head: &str, approved: &[String]) -> Result<(), CpdError> {
    let head_cases = test_cases(head);
    let mut lost = Vec::new();
    for b in test_cases(base) {
        if is_approved(approved, &b.name) {
            continue;
        }
        match head_cases.iter().find(|h| h.name == b.name) {
            Some(h) if h.assertions >= b.assertions => {}
            _ => lost.push(b.name),
        }
    }
    if lost.is_empty() {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::TestsRemoved,
        format!(
            "The diff removes or weakens existing tests: {}.",
            lost.join(", ")
        ),
    )
    .with("tests", json!(lost)))
}

/// The text of `path` at `rev`; `None` when `rev` has no such path.
pub fn file_at(dir: &Path, rev: &str, path: &str) -> Result<Option<String>, CpdError> {
    let spec = format!("{rev}:{path}");
    let out = run(dir, &["show", &spec])?;
    match out.code {
        0 => Ok(Some(out.stdout)),
        128 => Ok(None),
        code => Err(CpdError::new(
            ErrorCode::GitFailed,
            format!("git show {spec} exited {code}."),
        )
        .with("stderr", out.stderr)),
    }
}

/// Record the Lead's excuse for removing test `name`: adds it to `approved` and returns the event text the User
/// sees. A blank `reason` is a usage error and records nothing.
pub fn approve_test_removal(
    approved: &mut Vec<String>,
    name: &str,
    reason: &str,
) -> Result<String, CpdError> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(CpdError::new(
            ErrorCode::Usage,
            "A test-removal approval needs a --reason.",
        ));
    }
    if !approved.iter().any(|a| a == name) {
        approved.push(name.to_owned());
    }
    Ok(format!("Test removal approved: {name} — {reason}"))
}
