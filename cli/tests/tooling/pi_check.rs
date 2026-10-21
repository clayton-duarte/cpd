//! `scripts/pi-check.sh`: where its tmux server's socket lives (hygiene.md bug 4).

use crate::common;

use std::fs;
use std::path::Path;

use common::TestEnv;

#[test]
fn every_tmux_call_runs_with_a_tmux_tmpdir_inside_the_checks_own_tmp_dir() {
    let env = TestEnv::new();
    let tmp = env.root().join("tmp");
    fs::create_dir_all(&tmp).unwrap();
    let record = env.root().join("tmux-tmpdirs");
    // A pi that prints a version and exits 0, and a tmux that records the TMUX_TMPDIR each call sees; its `has-session`
    // fails, so step 3 ends at once.
    env.fake("pi", "echo 1.0.0");
    env.fake("tmux", &format!("echo \"${{TMUX_TMPDIR-unset}}\" >> '{}'\ncase \"$*\" in *has-session*) exit 1;; esac\nexit 0", record.display()));
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/pi-check.sh");

    let out = common::output(env.command("sh").arg(&script).arg(env.bin().join("pi")).env("TMPDIR", &tmp)).unwrap();

    let recorded = fs::read_to_string(&record).unwrap_or_default();
    let tmpdirs: Vec<&str> = recorded.lines().collect();
    assert!(!tmpdirs.is_empty(), "the check made no tmux call: {}", String::from_utf8_lossy(&out.stderr));
    for value in tmpdirs {
        assert!(
            Path::new(value).starts_with(&tmp) && value != tmp.to_str().unwrap(),
            "a tmux call ran with TMUX_TMPDIR {value:?}, not a directory inside the check's own tmp dir under {}",
            tmp.display()
        );
    }
}
