//! The source guards of the test harness: scans of the test files of `cli/tests/<area>/` that fail a test which
//! waits, sleeps, mutates the process environment or redefines a shared helper by itself, and the ratchet on
//! `TestEnv::new` in the `store` area (docs/implementing.md, Tests). They read text only: no process, no tmux.

/// The `.rs` files of the area test binaries (`cli/tests/<area>/`, not `common/`). The guards below scan these: a
/// wrong directory would scan nothing and pass, so this asserts it found more than 50.
pub(crate) fn test_files() -> Vec<std::path::PathBuf> {
    let dir = env!("CARGO_MANIFEST_DIR").to_owned() + "/tests";
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|area| area.is_dir() && area.file_name().is_some_and(|n| n != "common"))
        .flat_map(|area| {
            std::fs::read_dir(area)
                .unwrap()
                .map(|entry| entry.unwrap().path())
        })
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .collect();
    assert!(
        files.len() > 50,
        "the harness guards saw {} files in {dir}: they scan the wrong directory",
        files.len()
    );
    files
}

/// The text of every test file but `harness.rs` and this one, as (path, line number, line).
pub(crate) fn test_source_lines() -> Vec<(String, usize, String)> {
    let mut lines = Vec::new();
    for path in test_files() {
        if path
            .file_name()
            .is_some_and(|n| n == "harness.rs" || n == "guards.rs")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            lines.push((path.display().to_string(), n + 1, line.to_owned()));
        }
    }
    lines
}

/// The lines of the test files containing any of `needles`, as `path:line: text`.
pub(crate) fn test_lines_with(needles: &[&str]) -> Vec<String> {
    test_source_lines()
        .into_iter()
        .filter(|(_, _, line)| needles.iter().any(|n| line.contains(n)))
        .map(|(path, n, line)| format!("{path}:{n}: {}", line.trim()))
        .collect()
}

#[test]
fn no_test_mutates_the_process_environment() {
    // Test threads share one environment: `set_var` there races every `getenv` (tests-build-G3).
    // A test sets a variable on the `Command` it spawns.
    let found = test_lines_with(&["set_var(", "remove_var("]);
    assert!(
        found.is_empty(),
        "use Command::env, not the process environment:\n{}",
        found.join("\n")
    );
}

/// The milliseconds of the literal duration a `sleep(` on `line` waits (`from_millis(n)`, `from_secs(n)`), if it has one.
fn literal_sleep_ms(line: &str) -> Option<u64> {
    let after = &line[line.find("sleep(")? + 6..];
    let number = |prefix: &str| -> Option<u64> {
        let rest = &after[after.find(prefix)? + prefix.len()..];
        rest.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()
    };
    number("from_millis(").or_else(|| number("from_secs(").map(|s| s * 1000))
}

#[test]
fn no_test_sleeps_for_a_result_or_bounds_the_time_a_result_takes() {
    // A fixed sleep before an assertion proves nothing on a loaded machine, and an upper bound on elapsed time fails on
    // one (tests-build-W4): wait for the result (`wait_until`, a whole engine tick, a rendezvous). A sleep of 250 ms or
    // more, or an upper bound, stands only for a claim about time itself, and says so in a `// time-claim: <what it
    // asserts>; no per-poll signal from <X>` comment on its line or the five above it.
    let lines = test_source_lines();
    let mut found = Vec::new();
    for (i, (path, n, line)) in lines.iter().enumerate() {
        let code = line.trim();
        if code.starts_with("//") {
            continue;
        }
        let long_sleep = literal_sleep_ms(code).is_some_and(|ms| ms >= 250);
        let upper_bound =
            code.contains("assert") && code.contains("elapsed()") && code.contains(" < ");
        if !(long_sleep || upper_bound) {
            continue;
        }
        let claimed = lines[i.saturating_sub(5)..=i]
            .iter()
            .any(|(p, _, l)| p == path && l.contains("time-claim:"));
        if !claimed {
            found.push(format!("{path}:{n}: {code}"));
        }
    }
    assert!(
        found.is_empty(),
        "wait for the result, or give the `// time-claim:` it asserts:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_shared_helpers_are_defined_only_in_the_harness() {
    // One wait policy, one refusal check, one fixture writer, one store query (tests-build-G2, commands-G4): a file
    // that needs another shape calls the harness's and wraps it, it does not copy it.
    let found = test_lines_with(&[
        "fn wait_until(",
        "fn wait_until_for(",
        "fn wait_some<",
        "fn poll<",
        "fn refused(run: &Run, code: &str, exit: i32)",
        "fn sql(&self",
        "fn sql(env: &TestEnv",
        "fn sql(store: &mut Store",
        "fn query<T",
    ]);
    assert!(
        found.is_empty(),
        "use common::wait_until, wait_until_for, wait_some, poll, refused, sql and TestEnv::query:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_wait_polls_through_the_harness() {
    // One poll loop, at one period (common::poll, every 10 ms): a wait or an absence window calls wait_until,
    // wait_some, wait_screen or poll; it keeps no deadline and no sleep of its own.
    let found = test_lines_with(&[concat!("Instant::", "now() +")]);
    assert!(
        found.is_empty(),
        "wait with common::wait_until, wait_some, wait_screen or poll:\n{}",
        found.join("\n")
    );
}

// ---- the store area is in-process: no `TestEnv::new` ----

/// The calls of `TestEnv::new` and `TestEnv::empty` still in each file of `cli/tests/store/`: the debt of PR `mx-inproc`
/// (cross-process writers, exec of the binary, `--version`), which takes it to nothing and removes this table. An exact
/// match, as the other ratchets (`scripts/tripwire/allow.txt`, `pi/test/sleep-allow.txt`): a new call fails, and a call
/// that goes away fails until its number is lowered here.
const STORE_TESTENV_DEBT: &[(&str, usize)] = &[("store.rs", 8), ("store_window.rs", 1)];

/// How each `store/` file differs from `debt`, as `file: n calls (allowed m)`; empty when the counts are exact.
/// `lines` are the (path, line number, line) of the test files; a comment line calls nothing.
fn store_testenv_mismatches(
    lines: &[(String, usize, String)],
    debt: &[(&str, usize)],
) -> Vec<String> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for (path, _, line) in lines {
        let Some((_, file)) = path.rsplit_once("/tests/store/") else {
            continue;
        };
        let code = line.trim();
        if !code.starts_with("//")
            && ["TestEnv::new", "TestEnv::empty"]
                .iter()
                .any(|call| code.contains(call))
        {
            *counts.entry(file.to_owned()).or_default() += 1;
        }
    }
    for (file, _) in debt {
        counts.entry((*file).to_owned()).or_default();
    }
    counts
        .into_iter()
        .filter_map(|(file, n)| {
            let allowed = debt.iter().find(|(f, _)| *f == file).map_or(0, |(_, a)| *a);
            (n != allowed).then(|| format!("{file}: {n} calls (allowed {allowed})"))
        })
        .collect()
}

#[test]
fn no_store_test_calls_testenv_new_beyond_the_debt_of_mx_inproc() {
    let lines = test_source_lines();
    assert!(
        lines
            .iter()
            .any(|(path, ..)| path.contains("/tests/store/")),
        "the guard saw no file of cli/tests/store/"
    );
    let found = store_testenv_mismatches(&lines, STORE_TESTENV_DEBT);
    assert!(
        found.is_empty(),
        "a store test builds a temp home (cli/tests/store/home.rs: `Home::new()`/`Home::empty()`), not a `TestEnv` with its reaper, tmux and sqlite3;\nlower STORE_TESTENV_DEBT when a call goes away:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_store_scan_fails_a_store_test_that_calls_testenv_new_and_passes_the_exact_debt() {
    let line = |path: &str, text: &str| (format!("/r/cli/tests/{path}"), 1, text.to_owned());
    let lines = [
        line("store/store.rs", "    let env = TestEnv::new();"),
        line("store/store.rs", "    let env = common::TestEnv::empty();"),
        line(
            "store/store.rs",
            "    // TestEnv::new() is what a store test no longer calls",
        ),
        line("store/home.rs", "    let env = Home::new();"),
        line("flow/job.rs", "    let env = TestEnv::new();"),
    ];
    assert_eq!(
        store_testenv_mismatches(&lines, &[]),
        ["store.rs: 2 calls (allowed 0)"],
        "a call with no debt fails"
    );
    assert_eq!(
        store_testenv_mismatches(&lines, &[("store.rs", 2)]),
        Vec::<String>::new(),
        "the exact debt passes"
    );
    assert_eq!(
        store_testenv_mismatches(&lines, &[("store.rs", 3)]),
        ["store.rs: 2 calls (allowed 3)"],
        "a call gone fails until lowered"
    );
    assert_eq!(
        store_testenv_mismatches(&lines, &[("store.rs", 2), ("gone.rs", 1)]),
        ["gone.rs: 0 calls (allowed 1)"]
    );
}

/// The problems of the exception list `scripts/tripwire/shell-tests.txt` as `text` says it: a line is `<path> # <reason>`,
/// the path is a file of the repository at `root`, and no path is listed twice.
fn shell_test_list_problems(root: &std::path::Path, text: &str) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut problems = Vec::new();
    for line in text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let Some((path, reason)) = line.split_once(" # ") else {
            problems.push(format!("{line}: not `<path> # <reason>`"));
            continue;
        };
        if reason.trim().is_empty() {
            problems.push(format!("{path}: no reason"));
        }
        if !root.join(path).is_file() {
            problems.push(format!("{path}: not a file of the repository"));
        }
        if !seen.insert(path) {
            problems.push(format!("{path}: listed twice"));
        }
    }
    problems
}

#[test]
fn every_shell_test_exception_names_an_existing_file_once_with_a_reason() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let text = std::fs::read_to_string(root.join("scripts/tripwire/shell-tests.txt")).unwrap();
    let problems = shell_test_list_problems(&root, &text);
    assert!(
        problems.is_empty(),
        "scripts/tripwire/shell-tests.txt:\n{}",
        problems.join("\n")
    );
    assert!(
        text.lines().filter(|l| !l.starts_with('#')).count() > 5,
        "the list is read from the right file"
    );
}

#[test]
fn the_shell_test_list_scan_names_a_missing_file_a_missing_reason_and_a_repeat() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let text = "# a comment\ncli/tests/tooling/guards.rs # fine\ncli/tests/tooling/guards.rs # again\nnowhere.rs # gone\ncli/tests/tooling/smoke.rs #  \nno separator\n";
    assert_eq!(
        shell_test_list_problems(&root, text),
        [
            "cli/tests/tooling/guards.rs: listed twice",
            "nowhere.rs: not a file of the repository",
            "cli/tests/tooling/smoke.rs: no reason",
            "no separator: not `<path> # <reason>`",
        ]
    );
}

/// The `path:line: text` of every merge-conflict marker in `text`: a line starting with `<<<<<<< ` or `>>>>>>> `, and
/// a line equal to `=======` when the same text also has a `<<<<<<< ` line (a lone `=======` is a legal setext heading
/// or markdown table line).
fn conflict_marker_lines(path: &str, text: &str) -> Vec<String> {
    let has_open = text.lines().any(|l| l.starts_with("<<<<<<< "));
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            l.starts_with("<<<<<<< ") || l.starts_with(">>>>>>> ") || (has_open && *l == "=======")
        })
        .map(|(n, l)| format!("{path}:{}: {l}", n + 1))
        .collect()
}

/// Every file of the tree at `dir` with a source extension, skipping `target/`, `node_modules/`, `.git/` and the
/// worktrees (`.cpd/`, `.worktrees/`, `worktrees/`). std `fs` only: no git, no process.
fn source_files(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    const SKIP: [&str; 6] = [
        "target",
        "node_modules",
        ".git",
        ".cpd",
        ".worktrees",
        "worktrees",
    ];
    const EXTENSIONS: [&str; 7] = ["rs", "md", "toml", "sh", "ts", "json", "txt"];
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if !SKIP.contains(&name.as_str()) {
                source_files(&path, found);
            }
        } else if path
            .extension()
            .is_some_and(|e| EXTENSIONS.iter().any(|x| e == *x))
        {
            found.push(path);
        }
    }
}

#[test]
fn no_tracked_source_file_carries_a_merge_conflict_marker() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    source_files(&root, &mut files);
    assert!(
        files.len() > 200,
        "the scan saw {} files under {}: it scans the wrong directory",
        files.len(),
        root.display()
    );
    let marked: Vec<String> = files
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok().map(|text| (path, text)))
        .flat_map(|(path, text)| conflict_marker_lines(&path.display().to_string(), &text))
        .collect();
    assert!(
        marked.is_empty(),
        "merge-conflict markers committed:\n{}",
        marked.join("\n")
    );
}

#[test]
fn the_conflict_marker_scan_names_each_marker_and_spares_a_lone_setext_line() {
    let conflict = "a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> origin/main\n";
    assert_eq!(
        conflict_marker_lines("f.rs", conflict),
        [
            "f.rs:2: <<<<<<< HEAD",
            "f.rs:4: =======",
            "f.rs:6: >>>>>>> origin/main"
        ]
    );
    assert!(conflict_marker_lines("f.md", "Title\n=======\n\n| a |\n").is_empty());
    assert!(conflict_marker_lines("f.md", "x <<<<<<< y\n  >>>>>>> z\n").is_empty());
}
