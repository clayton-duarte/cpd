//! The portability check `scripts/check-portable.sh` (hs-d355, hs-d463), which guards the shell
//! that stays, `scripts/install-cpd.sh` among it. The install itself is `cpd install`
//! (core/src/install/), tested in process.

use crate::common;

use std::fs;
use std::path::Path;
use std::process::Command;

// ---- The portability check (hs-d355; docs/implementing.md "The portability check") ----
//
// `scripts/check-portable.sh [<file>...]` refuses Linux-only calls in the scripts Macs run; the Gate runs it.

const SCRIPTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts");

struct Check {
    exit: i32,
    stdout: String,
    stderr: String,
}

impl Check {
    /// Both streams: the docs name the `ok` line without a stream.
    fn all(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

/// Runs `script` with `args` in `cwd`.
fn run_check(script: &Path, cwd: &Path, args: &[&str]) -> Check {
    let out = common::output(Command::new(script).current_dir(cwd).args(args)).unwrap();
    Check {
        exit: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Checks a fixture whose second line is `line` (the first is a shebang, a full-line comment).
fn check_line(line: &str) -> Check {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("fx.sh"), format!("#!/bin/sh\n{line}\n")).unwrap();
    run_check(
        &Path::new(SCRIPTS).join("check-portable.sh"),
        dir.path(),
        &["fx.sh"],
    )
}

/// One Linux-only habit per entry (docs: the table of refused patterns): each is refused with `fx.sh:2:`.
const REFUSED: &[&str] = &[
    "python3 -c 'import ctypes; ctypes.CDLL(None).renameat2' a b",
    "RENAME_EXCHANGE=2",
    "cat /proc/sys/kernel/random/uuid",
    "cat /sys/class/net/eth0/address",
    "dir=$(readlink -f \"$0\")",
    "dir=$(readlink -e \"$0\")",
    "dir=$(readlink -m \"$0\")",
    "dir=$(realpath \"$0\")",
    "stat -c %Y file",
    "stat --format=%Y file",
    "date -d yesterday",
    "date --date=yesterday",
    "date +%s.%N",
    "date -r 1700000000",
    "sed -i 's/a/b/' file",
    "sed --in-place 's/a/b/' file",
    "sed -r 's/(a)/\\1/' file",
    "flock -n \"$lock\" true",
    "while ! flock -n \"$lock\" true; do sleep 1; done",
    "timeout 5 sleep 1",
    "setsid sleep 1 &",
    "ls | xargs -r rm",
    "ls | xargs --no-run-if-empty rm",
    "ls | xargs -d '\\n' rm",
    "grep -P 'a\\d' file",
    "grep --perl-regexp 'a' file",
    "sha256sum file",
    "sha1sum file",
    "md5sum file",
    "jobs=$(nproc)",
    "mv -T a b",
    "ln -T a b",
    "cp --reflink=auto a b",
    "cp --parents a b",
    "find . -printf '%p\\n'",
    "mktemp --tmpdir x.XXXXXX",
    "touch -d yesterday file",
    "base64 -w0 file",
    "sort -V file",
    "du -b file",
    "df --output=avail /",
    // a trailing comment is not a full-line comment
    "echo hi # see /proc/self",
];

/// The portable form of the same: each passes.
const ALLOWED: &[&str] = &[
    "python3 -c 'import os,sys; os.replace(sys.argv[1], sys.argv[2])' a b",
    "node -p 'crypto.randomUUID()'",
    "ps -o etime= -p \"$pid\"",
    "uname -s",
    "target=$(readlink \"$link\")",
    "dir=$(cd \"$dir\" && pwd -P)",
    "python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' \"$0\"",
    "python3 -c 'import os,sys; print(os.stat(sys.argv[1]).st_mtime)' file",
    "[ a -nt b ] && echo newer",
    "date -u +%Y%m%dT%H%M%S",
    "date +%s",
    "sed -i.bak 's/a/b/' file",
    "sed -E 's/(a)/\\1/' file",
    "sed -n 's/a/b/p' file",
    "python3 -c 'import fcntl,sys; fcntl.flock(open(sys.argv[1], \"a\"), fcntl.LOCK_EX | fcntl.LOCK_NB)' \"$lock\"",
    "sleep 1 &",
    "kill \"$pid\"",
    "python3 -c 'import subprocess; subprocess.run([\"true\"], timeout=5)'",
    "ls | while read -r f; do rm \"$f\"; done",
    "[ -n \"$list\" ] && rm $list",
    "grep -E 'a[0-9]' file",
    "shasum -a 256 file",
    "shasum -a 1 file",
    "jobs=$(getconf _NPROCESSORS_ONLN)",
    "mv a b",
    "ln -s a b",
    "cp a b",
    "find . -name x",
    "mktemp -d",
    "touch file",
    "base64 file",
    "sort -n file",
    "du -k file",
    "df -k /",
    "echo hi",
];

#[test]
fn every_refused_pattern_fails_with_the_file_and_line_and_an_alternative() {
    let mut wrong = Vec::new();
    for line in REFUSED {
        let run = check_line(line);
        let named = run
            .stderr
            .lines()
            .any(|l| l.starts_with("fx.sh:2: ") && l.contains(": use "));
        if run.exit != 1
            || !named
            || !run
                .stderr
                .trim_end()
                .ends_with("check-portable: FAIL (1 hits)")
        {
            wrong.push(format!(
                "`{line}`: exit {}, stderr {:?}",
                run.exit, run.stderr
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "not refused as `<file>:<line>: <pattern>: use <alternative>` and `check-portable: FAIL (1 hits)`:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn the_portable_alternative_of_every_pattern_passes() {
    let mut wrong = Vec::new();
    for line in ALLOWED {
        let run = check_line(line);
        if run.exit != 0 || !run.all().contains("check-portable: ok (1 files, 0 exempt)") {
            wrong.push(format!(
                "`{line}`: exit {}, output {:?}",
                run.exit,
                run.all()
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "refused, or no `check-portable: ok (1 files, 0 exempt)`:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn a_full_line_comment_may_name_a_pattern() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("fx.sh"), "#!/bin/sh\n# renameat2 and /proc/ and readlink -f are Linux-only\n    # sed -i without a suffix\n\t# flock\necho hi\n").unwrap();
    let run = run_check(
        &Path::new(SCRIPTS).join("check-portable.sh"),
        dir.path(),
        &["fx.sh"],
    );
    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(
        run.all().contains("check-portable: ok (1 files, 0 exempt)"),
        "{}",
        run.all()
    );
}

#[test]
fn an_exempt_line_with_a_reason_passes_and_is_counted_and_one_without_a_reason_fails() {
    let ok =
        check_line("readlink -f \"$0\" # portable: a uname guard routes a Mac around this line");
    assert_eq!(ok.exit, 0, "{}", ok.stderr);
    assert!(
        ok.all().contains("check-portable: ok (1 files, 1 exempt)"),
        "{}",
        ok.all()
    );

    for line in [
        "readlink -f \"$0\" # portable:",
        "readlink -f \"$0\" # portable:   ",
        "echo hi # portable:",
    ] {
        let run = check_line(line);
        assert_eq!(run.exit, 1, "`{line}`: {}", run.stderr);
        assert!(
            run.stderr.contains("fx.sh:2: "),
            "`{line}`: a `# portable:` with no reason is a hit: {}",
            run.stderr
        );
        assert!(
            run.stderr
                .trim_end()
                .ends_with("check-portable: FAIL (1 hits)"),
            "`{line}`: {}",
            run.stderr
        );
    }
}

#[test]
fn every_hit_is_listed_with_its_own_line_and_counted_and_only_the_failing_file_is_named() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.sh"), "#!/bin/sh\necho fine\n").unwrap();
    fs::write(
        dir.path().join("b.sh"),
        "#!/bin/sh\nnproc\necho fine\ncat /proc/cpuinfo\n",
    )
    .unwrap();
    let script = Path::new(SCRIPTS).join("check-portable.sh");

    let run = run_check(&script, dir.path(), &["a.sh", "b.sh"]);

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr.contains("b.sh:2: ") && run.stderr.contains("b.sh:4: "),
        "{}",
        run.stderr
    );
    assert!(!run.stderr.contains("a.sh:"), "{}", run.stderr);
    assert!(
        run.stderr
            .trim_end()
            .ends_with("check-portable: FAIL (2 hits)"),
        "{}",
        run.stderr
    );

    let both = run_check(&script, dir.path(), &["a.sh", "a.sh"]);
    assert_eq!(both.exit, 0, "{}", both.stderr);
    assert!(
        both.all()
            .contains("check-portable: ok (2 files, 0 exempt)"),
        "{}",
        both.all()
    );
}

/// A copy of the repository's `scripts/` in a temporary directory: what the check classifies when run without arguments.
fn copied_scripts() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(Path::new(SCRIPTS), &dir.path().join("scripts"));
    dir
}

/// Copies the files of `from` and of its subdirectories (`scripts/tripwire/`, a covered script) to the new directory `to`.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else if path.is_file() {
            fs::copy(&path, &target).unwrap();
        }
    }
}

#[test]
fn the_covered_scripts_of_the_repository_pass() {
    // The Gate's run: no argument checks the covered list (sixteen scripts; the Linux-only ones are listed with a reason each).
    let root = Path::new(SCRIPTS).join("..");
    let run = run_check(&Path::new(SCRIPTS).join("check-portable.sh"), &root, &[]);
    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(
        run.all().contains("check-portable: ok (16 files, "),
        "{}",
        run.all()
    );
}

#[test]
fn a_script_in_neither_list_fails_so_a_new_script_cannot_skip_the_decision() {
    let dir = copied_scripts();
    let script = dir.path().join("scripts/check-portable.sh");
    let before = run_check(&script, dir.path(), &[]);
    assert_eq!(
        before.exit, 0,
        "the copy passes before the new script: {}",
        before.stderr
    );
    assert!(
        before.all().contains("check-portable: ok (16 files, "),
        "{}",
        before.all()
    );

    fs::write(
        dir.path().join("scripts/brand-new.sh"),
        "#!/bin/sh\nexit 0\n",
    )
    .unwrap();
    let run = run_check(&script, dir.path(), &[]);

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr
            .contains("brand-new.sh: not classified: add it to the covered or the Linux-only list"),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr
            .trim_end()
            .ends_with("check-portable: FAIL (1 hits)"),
        "{}",
        run.stderr
    );
}

#[test]
fn a_linux_only_script_is_not_covered_and_a_covered_one_is_checked_by_the_default_run() {
    let dir = copied_scripts();
    let script = dir.path().join("scripts/check-portable.sh");
    // test-time.sh measures the Gate on hs (`date +%s.%N`): listed as Linux-only, not checked.
    let mut text = fs::read_to_string(dir.path().join("scripts/test-time.sh")).unwrap();
    text.push_str("\ndate +%s.%N\n");
    fs::write(dir.path().join("scripts/test-time.sh"), text).unwrap();
    let run = run_check(&script, dir.path(), &[]);
    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(
        run.all().contains("check-portable: ok (16 files, "),
        "{}",
        run.all()
    );

    let mut text = fs::read_to_string(dir.path().join("scripts/pnpm-fresh.sh")).unwrap();
    text.push_str("\nreadlink -f \"$0\"\n");
    fs::write(dir.path().join("scripts/pnpm-fresh.sh"), text).unwrap();
    let run = run_check(&script, dir.path(), &[]);
    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(
        run.stderr.contains("scripts/pnpm-fresh.sh:"),
        "{}",
        run.stderr
    );
}

// ---- The guard that a new tmux call cannot skip `-u` (hs-d463; docs/implementing.md "The row that guards tmux") ----

/// `tmux` in command position whose next word is not `-u`: each is a hit, `fx.sh:2: bare tmux: use tmux -u, ...`.
const BARE_TMUX: &[&str] = &[
    "tmux -L s list-windows",
    "env -u TMUX tmux -L s kill-window",
    "env -u TMUX -u TMUX_PANE tmux -L \"$socket\" -f /dev/null list-windows",
    "tmux -V",
    "echo | tmux load-buffer -",
    "tmux -f /dev/null new-session",
    "x=$(tmux -L s list-panes)",
    "! tmux -L s has-session",
    "TMUX_TMPDIR=/tmp tmux -L s kill-server",
];

/// Not a call of tmux without `-u`: each passes.
const TMUX_OK: &[&str] = &[
    "tmux -u -L s list-windows",
    "env -u TMUX -u TMUX_PANE tmux -u -L \"$socket\" -f /dev/null \"$@\"",
    "tmx() { cleanenv tmux -u -L \"$socket\" \"$@\"; }",
    "command -v tmux >/dev/null",
    "for tool in tmux node; do :; done",
    "# tmux -L s list-windows is what a bare call looks like",
    "echo \"needs tmux\"",
];

#[test]
fn a_bare_tmux_in_a_script_is_refused_and_tmux_u_is_not() {
    let mut wrong = Vec::new();
    for line in BARE_TMUX {
        let run = check_line(line);
        let named = run
            .stderr
            .lines()
            .any(|l| l.starts_with("fx.sh:2: bare tmux: use tmux -u"));
        if run.exit != 1
            || !named
            || !run
                .stderr
                .trim_end()
                .ends_with("check-portable: FAIL (1 hits)")
        {
            wrong.push(format!("`{line}` is not refused as `fx.sh:2: bare tmux: use tmux -u ...`: exit {}, stderr {:?}", run.exit, run.stderr));
        }
    }
    for line in TMUX_OK {
        let run = check_line(line);
        if run.exit != 0 || !run.all().contains("check-portable: ok (1 files, 0 exempt)") {
            wrong.push(format!(
                "`{line}` is refused: exit {}, output {:?}",
                run.exit,
                run.all()
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_bare_tmux_in_a_message_may_be_exempted_with_a_reason() {
    let ok = check_line("echo tmux -L s # portable: a message, not a call");
    assert_eq!(ok.exit, 0, "{}", ok.stderr);
    assert!(
        ok.all().contains("check-portable: ok (1 files, 1 exempt)"),
        "{}",
        ok.all()
    );
}
