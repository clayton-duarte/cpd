//! config.toml is replaced by rename: no reader ever sees a truncated file.

use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::home::CpdHome;

const OLD: &str = "version = 1\nmachine = \"hs\"\ntest_concurrency = 2\n";
const NEW: &str = "version = 1\nmachine = \"hs\"\n";

#[test]
fn a_config_migration_replaces_config_toml_by_rename() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    std::fs::write(home.config(), OLD).unwrap();
    let before = std::fs::metadata(home.config()).unwrap().ino();
    let mut reader = std::fs::File::open(home.config()).unwrap();

    super::migrate(&home).unwrap();

    let after = std::fs::metadata(home.config()).unwrap().ino();
    assert_ne!(
        before, after,
        "config.toml must be a new inode (rename into place)"
    );
    let mut old = String::new();
    reader.read_to_string(&mut old).unwrap();
    assert_eq!(
        old, OLD,
        "a reader opened before sees the complete old text"
    );
    assert_eq!(std::fs::read_to_string(home.config()).unwrap(), NEW);
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["config.toml"], "no leftover temporary file");

    super::migrate(&home).unwrap();
    assert_eq!(
        std::fs::metadata(home.config()).unwrap().ino(),
        after,
        "nothing changed, nothing written"
    );
    assert_eq!(std::fs::read_to_string(home.config()).unwrap(), NEW);
}

/// Test files that a `#[cfg(test)] #[path = ...]` declares and that carry no `_tests` suffix.
const BY_HAND: &[(&str, &str)] = &[
    (
        "tui/src/merge/action_d550.rs",
        "test module declared with #[cfg(test)] #[path]",
    ),
    (
        "tui/src/merge/marker_d550.rs",
        "test module declared with #[cfg(test)] #[path]",
    ),
];
fn walk(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "tests" && name != "fakes" {
                walk(&path, root, out);
            }
        } else if name.ends_with(".rs") && !name.ends_with("_tests.rs") && name != "tests.rs" {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if !BY_HAND.iter().any(|(f, _)| *f == rel) {
                out.push(path);
            }
        }
    }
}

/// The text before an inline `#[cfg(test)]` module (`mod name {`); `mod name;` declarations are kept.
fn non_test_text(text: &str) -> &str {
    let mut from = 0;
    while let Some(i) = text[from..].find("#[cfg(test)]") {
        let at = from + i;
        let rest = text[at + "#[cfg(test)]".len()..].trim_start();
        if let Some(decl) = rest.strip_prefix("mod ") {
            let head = decl.split(['{', ';']).next().unwrap_or("");
            if decl[head.len()..].starts_with('{') {
                return &text[..at];
            }
        }
        from = at + 1;
    }
    text
}

#[test]
fn no_source_file_writes_config_toml_with_fs_write() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for krate in ["core", "engine", "cli", "tui"] {
        walk(&root.join(krate).join("src"), root, &mut files);
    }
    let mut offenders: Vec<String> = files
        .iter()
        .filter(|p| {
            let text = std::fs::read_to_string(p).unwrap();
            let text = non_test_text(&text);
            text.contains(".config()")
                && (text.contains("fs::write(") || text.contains("File::create("))
        })
        .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
        .collect();
    offenders.sort();
    assert!(
        offenders.is_empty(),
        "these files write config.toml with fs::write; use cpd_core::atomic::write_atomic: {offenders:?}"
    );
}

/// `cpd preset apply` rewrites host.toml's `[model]` table whole and atomically (commands.md §14.6 step 6): no source
/// file writes the machine policy with `fs::write` or `File::create`.
#[test]
fn no_source_file_writes_host_toml_with_fs_write() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for krate in ["core", "engine", "cli", "tui"] {
        walk(&root.join(krate).join("src"), root, &mut files);
    }
    let mut offenders: Vec<String> = files
        .iter()
        .filter(|p| {
            let text = std::fs::read_to_string(p).unwrap();
            let text = non_test_text(&text);
            text.contains("host.toml")
                && (text.contains("fs::write(") || text.contains("File::create("))
        })
        .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
        .collect();
    offenders.sort();
    assert!(
        offenders.is_empty(),
        "these files write host.toml with fs::write; use cpd_core::atomic::write_atomic: {offenders:?}"
    );
}

// ---------------------------------------------------------------- `cpd config concurrency set` (commands.md §14.2c; contract §8.2)

use clap::Parser;

use crate::error::ErrorCode;
use crate::output::Output;

const BASE: &str = "version = 1\nmachine = \"hs\"\n";

/// The `cpd config` argument tree, to parse the new subcommand's argv in this process.
#[derive(Parser)]
struct ConfigCli {
    #[command(flatten)]
    args: super::ConfigArgs,
}

/// Runs `cpd config <argv>` in this process, with `$CPD_HOME` pointed at `home`.
fn run(home: &CpdHome, argv: &[&str]) -> Result<Output, crate::error::CpdError> {
    // SAFETY: nextest runs one test per process, so no other test reads `CPD_HOME` meanwhile.
    unsafe { std::env::set_var("CPD_HOME", home.root()) };
    let args = ConfigCli::try_parse_from(std::iter::once("cpd").chain(argv.iter().copied()))
        .expect("`cpd config` parses")
        .args;
    super::config(args)
}

fn home_with(text: &str) -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    std::fs::write(home.config(), text).unwrap();
    (dir, home)
}

#[test]
fn concurrency_set_refuses_a_value_outside_1_to_16() {
    let (_dir, home) = home_with(BASE);
    for n in ["0", "17"] {
        let err = run(&home, &["concurrency", "set", n]).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid, "set {n}");
        assert_eq!(
            err.details()["key"],
            serde_json::json!("concurrency"),
            "set {n}"
        );
        assert_eq!(
            std::fs::read_to_string(home.config()).unwrap(),
            BASE,
            "set {n} leaves config.toml byte-identical"
        );
    }
}

#[test]
fn concurrency_set_writes_the_value_and_the_config_reports_it() {
    let (_dir, home) = home_with(BASE);
    let before = std::fs::metadata(home.config()).unwrap().ino();

    let Output::Success(fields) = run(&home, &["concurrency", "set", "7"]).expect("set 7") else {
        panic!("a success")
    };
    assert_eq!(fields["changed"], serde_json::json!(["concurrency"]));
    assert_eq!(fields["already"], serde_json::json!(false));

    let written = std::fs::read_to_string(home.config()).unwrap();
    assert!(written.contains("concurrency = 7"), "{written}");
    assert_ne!(
        std::fs::metadata(home.config()).unwrap().ino(),
        before,
        "the write is a rename into place (write_atomic)"
    );

    let Output::Success(fields) = run(&home, &[]).expect("cpd config") else {
        panic!("a success")
    };
    assert_eq!(fields["config"]["concurrency"], serde_json::json!(7));
    assert_eq!(fields["sources"]["concurrency"], serde_json::json!("file"));
}
