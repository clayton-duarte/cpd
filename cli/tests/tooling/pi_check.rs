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

/// A fake tmux body that appends `<TMUX_TMPDIR> <socket>` (the word after `-L`) per call to `record`, then `extra`.
fn recording_tmux(record: &Path, extra: &str) -> String {
    format!(
        "sock=; prev=; for a in \"$@\"; do [ \"$prev\" = -L ] && sock=$a; prev=$a; done\n\
         echo \"${{TMUX_TMPDIR-unset}} $sock\" >> '{}'\n{extra}\n\
         case \"$*\" in *has-session*) exit 1;; esac\nexit 0",
        record.display()
    )
}

fn uid() -> String {
    let out = std::process::Command::new("id").arg("-u").output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// Runs the check with a TMPDIR of at least 70 characters; returns the recorded `(TMUX_TMPDIR, socket)` pairs.
fn run_with_long_tmpdir(extra: &str) -> Vec<(String, String)> {
    let env = TestEnv::new();
    let mut tmp = env.root().to_path_buf();
    while tmp.to_str().unwrap().len() < 70 {
        tmp = tmp.join("a-rather-long-directory-name");
    }
    fs::create_dir_all(&tmp).unwrap();
    let record = env.root().join("tmux-calls");
    env.fake("pi", "echo 1.0.0");
    env.fake("tmux", &recording_tmux(&record, extra));
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/pi-check.sh");

    let out = common::output(env.command("sh").arg(&script).arg(env.bin().join("pi")).env("TMPDIR", &tmp)).unwrap();

    let recorded = fs::read_to_string(&record).unwrap_or_default();
    let calls: Vec<(String, String)> = recorded
        .lines()
        .map(|l| {
            let (d, s) = l.split_once(' ').unwrap();
            (d.to_string(), s.to_string())
        })
        .collect();
    assert!(!calls.is_empty(), "the check made no tmux call: {}", String::from_utf8_lossy(&out.stderr));
    calls
}

#[test]
fn a_long_tmpdir_still_gives_the_tmux_server_a_socket_path_of_at_most_100_characters() {
    let uid = uid();
    for (tmux_tmpdir, socket) in run_with_long_tmpdir("") {
        let path = format!("{tmux_tmpdir}/tmux-{uid}/{socket}");
        assert!(path.len() <= 100, "the tmux socket path {path:?} is {} characters, more than 100", path.len());
    }
}

#[test]
fn a_long_tmpdir_makes_the_check_use_tmp_for_its_tmux_dir() {
    for (tmux_tmpdir, _) in run_with_long_tmpdir("") {
        assert_eq!(tmux_tmpdir, "/tmp", "a tmux call ran with TMUX_TMPDIR {tmux_tmpdir:?}, not /tmp");
    }
}

#[test]
fn a_long_tmpdir_leaves_no_socket_file_behind() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let uid = uid();
    let make = "case \"$*\" in *new-session*) mkdir -p \"$TMUX_TMPDIR/tmux-$(id -u)\"; : > \"$TMUX_TMPDIR/tmux-$(id -u)/$sock\";; esac";
    let calls = run_with_long_tmpdir(make);
    let _guards: Vec<Cleanup> =
        calls.iter().map(|(_, socket)| Cleanup(Path::new("/tmp").join(format!("tmux-{uid}")).join(socket))).collect();
    for (_, socket) in &calls {
        let file = Path::new("/tmp").join(format!("tmux-{uid}")).join(socket);
        assert!(!file.exists(), "the socket file {} was left behind", file.display());
    }
}
