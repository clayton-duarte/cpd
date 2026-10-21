//! Hook priority (contract, "Priority (hs-d528)"): a hook command runs at nice 19 and I/O class idle (Linux),
//! and the caller, the engine or the test process, keeps its own nice.

#![cfg(target_os = "linux")]

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use cpd_core::git::hooks::{HookOutcome, Log, Operation, run_command};

/// The nice of process `pid`: field 19 of `/proc/<pid>/stat`, counted after the `)` that closes the comm field.
const NICE_OF_SHELL: &str = "sed 's/.*) //' /proc/$$/stat | cut -d' ' -f17";

fn own_nice() -> i64 {
    let stat = fs::read_to_string("/proc/self/stat").unwrap();
    let after = &stat[stat.rfind(')').unwrap() + 2..];
    // After ')' the first field is field 3, so field 19 is the 17th.
    after.split(' ').nth(16).unwrap().parse().unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

mod hooks_idle {
    use super::*;

    #[test]
    fn a_hook_command_runs_at_nice_19_and_io_class_idle_and_the_caller_keeps_its_nice() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t.invalid", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "init"]);
        let out = dir.path().join("out.txt");
        let log = dir.path().join("hook.log");

        let before = own_nice();
        let command = format!("{{ {NICE_OF_SHELL}; ionice -p $$; }} > '{}' 2>&1", out.display());
        let result = run_command(&command, &[], &repo, &Operation { branch: "main" }, Duration::from_secs(60), &log, Log::Truncate).unwrap();
        assert_eq!(result.outcome, HookOutcome::Passed, "{:?}", result.tail);

        let text = fs::read_to_string(&out).unwrap();
        let mut lines = text.lines();
        assert_eq!(lines.next().map(str::trim), Some("19"), "nice of the hook's shell; output: {text:?}");
        assert!(text.contains("idle"), "ionice of the hook's shell: {text:?}");
        assert_eq!(own_nice(), before, "the caller keeps its nice");
    }
}
