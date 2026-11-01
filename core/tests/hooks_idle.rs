//! Hook priority (contract, "Priority (hs-d528)"): a hook command runs at nice 19 and I/O class idle (Linux),
//! and the caller, the engine or the test process, keeps its own nice.
//!
//! The subject is cpd's: the argv the `SystemRunner` starts for a hook (`system_command`), which is `idle_prefix()` and
//! then `sh -c <hook>`. That the kernel then runs the child at that priority is `nice(1)`'s and `ionice(1)`'s, not
//! ours (research tests-mock-mig-rest.md §5 row 10), so no process is started here. That the Git hooks' calls carry
//! `HOOK_TOOL` is pinned in `core/src/git/hooks/exec_runner_tests.rs`.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use cpd_core::host::priority::idle_prefix;
use cpd_core::host::runner::{Call, HOOK_TOOL, system_command};

fn call(tool: &str) -> Call {
    Call {
        tool: tool.to_owned(),
        program: "sh".to_owned(),
        args: vec!["-c".to_owned(), "cargo test".to_owned()],
        envs: vec![
            ("KEPT".to_owned(), Some("yes".to_owned())),
            ("TMUX".to_owned(), None),
        ],
        cwd: Some(PathBuf::from("/work/tree")),
        deadline: Duration::from_secs(60),
        stdin: Some(b"{}".to_vec()),
        stderr_to: None,
    }
}

fn argv(cmd: &Command) -> Vec<String> {
    std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

mod hooks_idle {
    use super::*;

    #[test]
    fn a_hook_command_runs_at_nice_19_and_io_class_idle_and_the_caller_keeps_its_nice() {
        let hook = system_command(&call(HOOK_TOOL));
        let prefix: Vec<String> = idle_prefix().iter().map(|s| (*s).to_owned()).collect();
        // The hook's own argv follows the idle prefix, whole.
        let expected: Vec<String> = prefix
            .iter()
            .cloned()
            .chain(["sh", "-c", "cargo test"].map(str::to_owned))
            .collect();
        assert_eq!(argv(&hook), expected, "nice, ionice, then the hook's shell");
        // Linux: nice 19 and the idle I/O class (`ionice -c 3`); macOS has no ionice, only nice 19 (hs-d528).
        #[cfg(target_os = "linux")]
        assert_eq!(&prefix[..], ["nice", "-n", "19", "ionice", "-c", "3"]);
        #[cfg(target_os = "macos")]
        assert_eq!(&prefix[..], ["nice", "-n", "19"]);
        // The hook keeps its directory and its environment.
        assert_eq!(
            hook.get_current_dir(),
            Some(std::path::Path::new("/work/tree"))
        );
        let envs: Vec<(&OsStr, Option<&OsStr>)> = hook.get_envs().collect();
        assert_eq!(
            envs,
            [
                (OsStr::new("KEPT"), Some(OsStr::new("yes"))),
                (OsStr::new("TMUX"), None)
            ]
        );
        // The caller keeps its nice: only a hook is wrapped, every other call is its own argv.
        assert_eq!(
            argv(&system_command(&call("git"))),
            ["sh", "-c", "cargo test"]
        );
    }
}
