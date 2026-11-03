//! The real-process ratchet and the retries pin of the Gate (plan gate-lint, PR `final`). Text scans only: no process.
//! The scan's patterns are built from parts so that this file holds none of the scanned text.

use std::collections::BTreeMap;

/// The files whose local `self`/`fx` forwards `git` to `TestEnv`, so `self.git(`, `self.git_repo(`, `fx.git(` and
/// `fx.git_repo(` are real git there and only there.
pub const REAL_GIT_SELF_FILES: &[&str] = &[
    "cli/tests/common/mod.rs",
    "core/tests/migrations.rs",
    "core/tests/repin.rs",
    "cli/tests/workflow/workflow_adopt_life.rs",
    "cli/tests/workflow/workflow_adopt_pub.rs",
    "cli/tests/workflow/workflow_adopt_rebase.rs",
];

/// The files whose subject is the real program: every file under `cli/tests/common/` and `cli/tests/tooling/` and the
/// listed sources. The scan skips them.
pub const ALLOW: &[&str] = &[
    "cli/tests/common/",
    "cli/tests/tooling/",
    "core/tests/migrations.rs",
    "core/tests/repin.rs",
    "core/src/policy/origin.rs",
    "core/src/host/deadline.rs",
    "core/src/host/exe.rs",
    "core/src/store/lock.rs",
    "core/src/fakes/tmux/mod.rs",
];

/// The baseline: (file, program, count) of the real-process sites outside `ALLOW` at the base of PR `final`.
/// Measured by this scan: git 96 in 9 files, python3 1, ps 1, sh 1, env 1. The 77 git sites of the three `pr_adopt_*`
/// files go through their local `Fx`, which forwards to `TestEnv`: they are real; record pro-d314's 'git 44, git_repo 4'
/// counted them as fake and is superseded. Each row is a debt: lower or remove it when the site goes away.
pub const DEBT: &[(&str, &str, usize)] = &[
    // the engine watch test inits a real repo through TestEnv
    ("cli/tests/engine/watch.rs", "git", 2),
    // the gate flow test builds a real repo through TestEnv
    ("cli/tests/flow/gate.rs", "git", 5),
    // the git-layer test runs real git on temporary repos
    ("cli/tests/workflow/git.rs", "git", 1),
    // the git-layer test asks the real ps whether a pid is dead
    ("cli/tests/workflow/git.rs", "ps", 1),
    // adopt tests build real repos through TestEnv
    ("cli/tests/workflow/workflow_adopt_add.rs", "git", 4),
    // its local Fx forwards git to TestEnv (real git)
    ("cli/tests/workflow/workflow_adopt_life.rs", "git", 21),
    // its local Fx forwards git to TestEnv (real git)
    ("cli/tests/workflow/workflow_adopt_pub.rs", "git", 17),
    // its local Fx forwards git to TestEnv (real git)
    ("cli/tests/workflow/workflow_adopt_rebase.rs", "git", 39),
    // the lead-branch test builds a real repo through TestEnv
    ("cli/tests/workflow/workflow_lead_branch.rs", "git", 3),
    // pr open tests build real repos through TestEnv
    ("cli/tests/workflow/pr_open.rs", "git", 4),
    // a store test starts the real sh through TestEnv
    ("cli/tests/store/store.rs", "sh", 1),
    // a tui test runs the real env to read its environment
    ("cli/tests/tui/tui.rs", "env", 1),
];

const PROGRAMS: [&str; 7] = ["tmux", "sh", "python3", "node", "ps", "kill", "env"];

/// The text of `path` the scan reads, or `None` when the file is out of scope.
fn scanned_part<'a>(path: &str, text: &'a str) -> Option<&'a str> {
    if !path.ends_with(".rs") {
        return None;
    }
    if path.starts_with("cli/tests/") || path.starts_with("core/tests/") {
        return Some(text);
    }
    let in_src = ["core", "cli", "engine", "tui"]
        .iter()
        .any(|c| path.starts_with(&format!("{c}/src/")));
    if !in_src {
        return None;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == "tests.rs" || name.ends_with("_tests.rs") || name.starts_with("testkit") {
        return Some(text);
    }
    text.find("\n#[cfg(test)]")
        .map(|i| &text[i + 1..])
        .or_else(|| text.starts_with("#[cfg(test)]").then_some(text))
}

/// The text with its comment lines blanked.
fn without_comments(text: &str) -> String {
    text.lines()
        .map(|l| {
            if l.trim_start().starts_with("//") {
                ""
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The count of a `Command::new("<prog>")` whose statement calls `.output()` or `.status()`, and of `.command("<prog>")`.
fn started(text: &str, prog: &str) -> usize {
    let new = concat!("Command", "::new(\"").to_owned() + prog + "\")";
    let command = concat!(".com", "mand(\"").to_owned() + prog + "\")";
    let direct = text
        .match_indices(&new)
        .filter(|(i, _)| {
            let rest = &text[*i..];
            let stmt = &rest[..rest.find(';').unwrap_or(rest.len())];
            stmt.contains(".output()") || stmt.contains(".status()")
        })
        .count();
    direct + text.matches(&command).count()
}

/// The receiver identifier before each `.<method>(` of a line.
fn receivers<'a>(line: &'a str, method: &str) -> Vec<&'a str> {
    let pat = format!(".{method}(");
    line.match_indices(&pat)
        .map(|(i, _)| {
            let head = &line[..i];
            let start = head
                .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
                .map_or(0, |p| p + 1);
            &head[start..]
        })
        .collect()
}

fn git_sites(path: &str, text: &str) -> usize {
    let self_real = REAL_GIT_SELF_FILES.contains(&path);
    let mut n = started(text, "git");
    for line in text.lines() {
        for method in ["git", "git_repo"] {
            n += receivers(line, method)
                .into_iter()
                .filter(|r| {
                    matches!(*r, "env" | "clone" | "repo")
                        || (self_real && matches!(*r, "self" | "fx"))
                })
                .count();
        }
    }
    n
}

/// One line `<file>: <program> <n> sites (allowed <m>)` for every (file, program) whose count of real-process sites
/// differs from its `debt` row (exact-match ratchet). `files` are (repo-relative path, text); the files of `allow` and
/// comment lines (`//`) are skipped. `git` counts a receiver `env`, `clone` or `repo` before `.git(`/`.git_repo(`, the
/// `self`/`fx` forms in `REAL_GIT_SELF_FILES`, a `Command::new("git")` whose statement (to the next `;`) calls
/// `.output()` or `.status()`, and `.command("git")`; a command handed to a runner is a fake. `tmux`, `sh`, `python3`,
/// `node`, `ps`, `kill` and `env` count a `Command::new` or `.command` of that name under the same statement rule.
pub fn real_process_mismatches(
    files: &[(String, String)],
    allow: &[&str],
    debt: &[(&str, &str, usize)],
) -> Vec<String> {
    let mut found: BTreeMap<(String, &str), usize> = BTreeMap::new();
    for (path, text) in files {
        if allow.iter().any(|a| path.starts_with(a)) {
            continue;
        }
        let Some(part) = scanned_part(path, text) else {
            continue;
        };
        let code = without_comments(part);
        let git = git_sites(path, &code);
        if git > 0 {
            found.insert((path.clone(), "git"), git);
        }
        for prog in PROGRAMS {
            let n = started(&code, prog);
            if n > 0 {
                found.insert((path.clone(), prog), n);
            }
        }
    }
    let mut keys: Vec<(String, &str)> = found.keys().cloned().collect();
    keys.extend(debt.iter().map(|(f, p, _)| ((*f).to_owned(), *p)));
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter_map(|(file, prog)| {
            let n = found.get(&(file.clone(), prog)).copied().unwrap_or(0);
            let m = debt
                .iter()
                .find(|(f, p, _)| *f == file && *p == prog)
                .map_or(0, |d| d.2);
            (n != m).then(|| format!("{file}: {prog} {n} sites (allowed {m})"))
        })
        .collect()
}

fn retries_is_zero(v: &toml::Value) -> bool {
    match v {
        toml::Value::Integer(n) => *n == 0,
        toml::Value::Table(t) => t.get("count").and_then(toml::Value::as_integer) == Some(0),
        _ => false,
    }
}

/// One line per problem: `[profile.default]` without `retries = 0`; any other `retries` of a `[profile.*]` table or a
/// `[[profile.*.overrides]]` entry that is not 0 (an integer or `{ count = N }`); any `--retries` or `NEXTEST_RETRIES`
/// in a script text (`scripts` are (path, text)).
pub fn retries_problems(nextest_toml: &str, scripts: &[(String, String)]) -> Vec<String> {
    let mut problems = Vec::new();
    let doc: toml::Table = match nextest_toml.parse() {
        Ok(t) => t,
        Err(e) => return vec![format!("nextest.toml does not parse: {e}")],
    };
    let profiles = doc.get("profile").and_then(toml::Value::as_table);
    let default = profiles.and_then(|p| p.get("default"));
    if !default
        .and_then(|d| d.get("retries"))
        .is_some_and(retries_is_zero)
    {
        problems.push("[profile.default] has no `retries = 0`".to_owned());
    }
    for (name, profile) in profiles.into_iter().flatten() {
        if name != "default"
            && let Some(r) = profile.get("retries")
            && !retries_is_zero(r)
        {
            problems.push(format!("[profile.{name}] retries is not 0"));
        }
        let overrides = profile.get("overrides").and_then(toml::Value::as_array);
        for o in overrides.into_iter().flatten() {
            if let Some(r) = o.get("retries")
                && !retries_is_zero(r)
            {
                problems.push(format!("[[profile.{name}.overrides]] retries is not 0"));
            }
        }
    }
    let flag = concat!("--", "retries");
    let var = concat!("NEXTEST", "_RETRIES");
    for (path, text) in scripts {
        for needle in [flag, var] {
            if text.contains(needle) {
                problems.push(format!("{path}: `{needle}` in a script"));
            }
        }
    }
    problems
}
