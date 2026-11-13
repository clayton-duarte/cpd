//! The thin test bootstraps must execute under their declared shell and delegate to the built Cargo artifact.

use crate::common::{output, write_executable};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let scripts = dir.path().join("scripts");
    fs::create_dir(&scripts).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts");
    for name in ["cargo.sh", "test-changed", "tests.sh"] {
        fs::copy(source.join(name), scripts.join(name)).unwrap();
    }
    write_executable(
        &dir.path().join("cargo"),
        r#"#!/bin/sh
set -eu
[ "${QA_CARGO_EXIT:-0}" = 0 ] || exit "$QA_CARGO_EXIT"
mode=$1
bin=${CARGO_TARGET_DIR:-target}/debug/cpd
mkdir -p "${bin%/*}"
cat > "$bin" <<'PROGRAM'
#!/bin/sh
printf 'current\000'
printf '%s\000' "$@"
exit "${QA_CHILD_EXIT:-0}"
PROGRAM
chmod +x "$bin"
if [ "$mode" = run ]; then
  while [ "$1" != -- ]; do shift; done
  shift
  exec "$bin" "$@"
fi
"#,
    );
    dir
}

fn run(root: &Path, script: &str, args: &[&str], custom: bool, extra: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(root.join("scripts").join(script))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("CARGO", root.join("cargo"))
        .env("RUSTC_WRAPPER", "")
        .env("CPD_CARGO_RESOURCE_HELD", "1");
    if custom {
        cmd.env("CARGO_TARGET_DIR", root.join("separate target"));
    }
    for (key, value) in extra {
        cmd.env(key, value);
    }
    output(&mut cmd).unwrap()
}

#[test]
fn both_bootstraps_use_cargos_custom_target_artifact() {
    let dir = fixture();
    for (script, args, expected) in [
        (
            "test-changed",
            vec!["--dry-run", "base with space"],
            "current\0test\0changed\0--dry-run\0base with space\0",
        ),
        ("tests.sh", vec!["build"], "current\0test\0build\0"),
    ] {
        let out = run(dir.path(), script, &args, true, &[]);
        assert!(
            out.status.success(),
            "{script}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected.as_bytes(), "{script}");
    }
}

#[test]
fn a_newer_stale_release_cannot_replace_the_built_binary() {
    let dir = fixture();
    let release = dir.path().join("target/release/cpd");
    fs::create_dir_all(release.parent().unwrap()).unwrap();
    write_executable(&release, "#!/bin/sh\necho stale\n");
    fs::File::open(&release)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000))
        .unwrap();
    let out = run(dir.path(), "test-changed", &["--dry-run"], false, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"current\0test\0changed\0--dry-run\0");
}

#[test]
fn posix_shell_preserves_normalized_literal_arguments_and_child_failure() {
    let dir = fixture();
    let out = run(
        dir.path(),
        "tests.sh",
        &[
            "run",
            "engine::mod::test",
            "core::mod::test",
            "web/a.test.ts::literal '.*' ; $(nope)",
            "",
        ],
        true,
        &[("QA_CHILD_EXIT", "23")],
    );
    assert_eq!(
        out.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout,
        b"current\0test\0run\0mod::test\0mod::test\0web/a.test.ts::literal '.*' ; $(nope)\0\0"
    );
}

#[test]
fn both_bootstraps_propagate_cargo_failure_without_running_an_old_binary() {
    let dir = fixture();
    for script in ["test-changed", "tests.sh"] {
        let out = run(
            dir.path(),
            script,
            &["build"],
            false,
            &[("QA_CARGO_EXIT", "19")],
        );
        assert_eq!(out.status.code(), Some(19), "{script}");
        assert!(out.stdout.is_empty(), "{script}");
    }
}
