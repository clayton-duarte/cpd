//! `scripts/tripwire/tmux`: the stand-in the Gate puts first on PATH (docs/implementing.md, Tests). It execs the real
//! `tmux` only for a `NEXTEST_BINARY_ID` + test name pair of `allow.txt`; any other call is denied (exit 127) and logged.
//! The tests copy the directory (they own `allow.txt`) and put a shell stand-in behind it: no real tmux runs.

use crate::common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use common::Run;

const TOOLS: &[&str] = &[
    "sh", "dirname", "basename", "grep", "sed", "awk", "cat", "tr", "printf", "echo", "mkdir",
    "env", "cut", "head", "tee", "expr", "read", "test", "[", "date", "id",
];

struct Fx {
    dir: tempfile::TempDir,
}

impl Fx {
    /// A copy of scripts/tripwire/ with `allow` as its allow.txt, a stand-in `tmux` (records its argv to `argv`,
    /// prints `stand-in`) in `behind/`, and `tools/` with the basic commands only (so no real tmux is on PATH).
    fn new(allow: &str) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let trip = root.join("tripwire");
        fs::create_dir_all(&trip).unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/tripwire");
        for entry in fs::read_dir(&src).unwrap() {
            let entry = entry.unwrap();
            let to = trip.join(entry.file_name());
            if entry.file_name() == "allow.txt" {
                continue;
            }
            fs::copy(entry.path(), &to).unwrap();
        }
        fs::write(trip.join("allow.txt"), allow).unwrap();
        let behind = root.join("behind");
        fs::create_dir_all(&behind).unwrap();
        common::write_executable(
            &behind.join("tmux"),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\necho stand-in\n",
                root.join("argv").display()
            ),
        );
        let tools = root.join("tools");
        fs::create_dir_all(&tools).unwrap();
        for tool in TOOLS {
            let found = std::process::Command::new("sh")
                .args(["-c", &format!("command -v '{tool}'")])
                .output()
                .unwrap();
            let path = String::from_utf8_lossy(&found.stdout).trim().to_string();
            if path.starts_with('/') {
                symlink(path, tools.join(tool)).unwrap();
            }
        }
        fs::create_dir_all(root.join("tmp")).unwrap();
        Fx { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn log(&self) -> String {
        fs::read_to_string(self.path("tmp/tripwire.log")).unwrap_or_default()
    }

    fn standin_ran(&self) -> bool {
        self.path("argv").exists()
    }

    /// Runs the tripwire as test `name` of binary `id` (None: no NEXTEST_* variables); PATH is the tripwire's own
    /// directory, `behind` when `with_tmux`, then `tools`.
    fn run(&self, ids: Option<(&str, &str)>, with_tmux: bool, args: &[&str]) -> Run {
        let mut path = self.path("tripwire").display().to_string();
        if with_tmux {
            path = format!("{path}:{}", self.path("behind").display());
        }
        path = format!("{path}:{}", self.path("tools").display());
        let mut cmd = std::process::Command::new(self.path("tripwire/tmux"));
        cmd.args(args)
            .env_clear()
            .env("PATH", path)
            .env("TMPDIR", self.path("tmp"));
        if let Some((id, name)) = ids {
            cmd.env("NEXTEST_BINARY_ID", id)
                .env("NEXTEST_TEST_NAME", name);
        }
        let out = common::output(&mut cmd).unwrap();
        Run {
            exit: out
                .status
                .code()
                .expect("the tripwire was killed by a signal"),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

#[test]
fn an_allowed_pair_runs_the_tmux_behind_it_with_the_same_argv() {
    let fx = Fx::new("cpd::tooling bare::  # contract: x\n");

    let run = fx.run(
        Some(("cpd::tooling", "bare::x")),
        true,
        &["-L", "s", "new-session"],
    );

    assert_eq!(
        (run.exit, run.stdout.as_str()),
        (0, "stand-in\n"),
        "{run:?}"
    );
    assert_eq!(
        fs::read_to_string(fx.path("argv")).unwrap(),
        "-L\ns\nnew-session\n"
    );
}

#[test]
fn the_prefix_matches_the_start_of_the_test_name() {
    let fx = Fx::new("cpd::tooling bare::  # contract: x\n");

    let allowed = fx.run(Some(("cpd::tooling", "bare::x")), true, &["ls"]);
    let denied = fx.run(Some(("cpd::tooling", "barefoot::x")), true, &["ls"]);

    assert_eq!(allowed.exit, 0, "{allowed:?}");
    assert_eq!(denied.exit, 127, "{denied:?}");
}

#[test]
fn the_binary_id_must_be_equal() {
    let fx = Fx::new("cpd::tooling bare::  # contract: x\n");

    let run = fx.run(Some(("cpd::tooling2", "bare::x")), true, &["ls"]);

    assert_eq!(run.exit, 127, "{run:?}");
    assert!(!fx.standin_ran());
}

#[test]
fn comment_and_blank_lines_of_the_list_are_ignored() {
    let fx = Fx::new("# cpd::tooling bare::\n\n   \ncpd::tooling other::  # contract: x\n");

    let run = fx.run(Some(("cpd::tooling", "bare::x")), true, &["ls"]);

    assert_eq!(run.exit, 127, "{run:?}");
}

#[test]
fn a_pair_not_in_the_list_exits_127_and_is_logged_without_running_tmux() {
    let fx = Fx::new("cpd::tooling bare::  # contract: x\n");

    let run = fx.run(
        Some(("cpd::engine", "watch::t")),
        true,
        &["kill-server", "-t", "x"],
    );

    let line = "tripwire: real tmux called by cpd::engine watch::t: tmux kill-server -t x";
    assert_eq!(run.exit, 127, "{run:?}");
    assert!(run.stderr.contains(line), "{run:?}");
    assert!(fx.log().contains(line), "{}", fx.log());
    assert!(!fx.standin_ran(), "the stand-in ran");
}

#[test]
fn without_nextest_variables_the_call_is_denied() {
    let fx = Fx::new("cpd::tooling bare::  # contract: x\n");

    let run = fx.run(None, true, &["ls"]);

    assert_eq!(run.exit, 127, "{run:?}");
    assert!(!fx.standin_ran());
}

#[test]
fn an_allowed_call_with_no_tmux_behind_the_tripwire_exits_127() {
    let fx = Fx::new("cpd::tooling bare::  # contract: x\n");

    let run = fx.run(Some(("cpd::tooling", "bare::x")), false, &["ls"]);

    assert_eq!(run.exit, 127, "{run:?}");
    assert!(run.stderr.contains("tripwire: no real tmux"), "{run:?}");
}

#[test]
fn the_tripwires_own_directory_is_left_off_the_path_before_the_lookup() {
    let fx = Fx::new("cpd::tooling bare::  # contract: x\n");

    let run = fx.run(Some(("cpd::tooling", "bare::x")), true, &["ls"]);

    // Looking itself up again would loop or exit 99 (the stub); the stand-in is what answers.
    assert_eq!(
        (run.exit, run.stdout.as_str()),
        (0, "stand-in\n"),
        "{run:?}"
    );
}

#[test]
fn a_prefix_with_glob_characters_matches_literally() {
    let fx = Fx::new("cpd::tooling a*b::  # contract: x\n");

    let literal = fx.run(Some(("cpd::tooling", "a*b::t")), true, &["ls"]);
    let glob = fx.run(Some(("cpd::tooling", "axxb::t")), true, &["ls"]);

    assert_eq!(literal.exit, 0, "{literal:?}");
    assert_eq!(glob.exit, 127, "{glob:?}");
}

#[test]
fn the_portability_check_covers_the_tripwire_and_passes() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/check-portable.sh");
    let out =
        common::output(std::process::Command::new(script).current_dir(env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{all}");
    let src = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/check-portable.sh"),
    )
    .unwrap();
    assert!(
        src.lines()
            .any(|l| l.starts_with("covered=") && l.contains("tripwire/tmux")),
        "tripwire/tmux is not in the covered list"
    );
}
