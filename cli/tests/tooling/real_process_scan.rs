//! The real-process ratchet and the retries pin of the Gate (plan gate-lint, PR `final`). Text scans only: no process.

/// The files whose local `self`/`fx` forwards `git` to `TestEnv`, so `self.git(`, `self.git_repo(`, `fx.git(` and
/// `fx.git_repo(` are real git there and only there.
pub const REAL_GIT_SELF_FILES: &[&str] = &[];

/// The files whose subject is the real program: every file under `cli/tests/common/` and `cli/tests/tooling/` and the
/// listed sources. The scan skips them.
pub const ALLOW: &[&str] = &[];

/// The baseline: (file, program, count) of the real-process sites outside `ALLOW` at the base of PR `final`.
pub const DEBT: &[(&str, &str, usize)] = &[];

/// One line `<file>: <program> <n> sites (allowed <m>)` for every (file, program) whose count of real-process sites
/// differs from its `debt` row (exact-match ratchet). `files` are (repo-relative path, text); the files of `allow` and
/// comment lines (`//`) are skipped. `git` counts a receiver `env`, `clone` or `repo` before `.git(`/`.git_repo(`, the
/// `self`/`fx` forms in `REAL_GIT_SELF_FILES`, a `Command::new("git")` whose statement (to the next `;`) calls
/// `.output()` or `.status()`, and `.command("git")`; a command handed to a runner is a fake. `tmux`, `sh`, `python3`,
/// `node`, `ps`, `kill` and `env` count a `Command::new` or `.command` of that name under the same statement rule.
pub fn real_process_mismatches(
    _files: &[(String, String)],
    _allow: &[&str],
    _debt: &[(&str, &str, usize)],
) -> Vec<String> {
    unimplemented!("final-impl")
}

/// One line per problem: `[profile.default]` without `retries = 0`; any other `retries` of a `[profile.*]` table or a
/// `[[profile.*.overrides]]` entry that is not 0 (an integer or `{ count = N }`); any `--retries` or `NEXTEST_RETRIES`
/// in a script text (`scripts` are (path, text)).
pub fn retries_problems(_nextest_toml: &str, _scripts: &[(String, String)]) -> Vec<String> {
    unimplemented!("final-impl")
}
