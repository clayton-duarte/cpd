//! `scripts/tests.sh`, the commands of cpd's `[tests]` table (contract §7.7): it builds, lists and runs named
//! Rust and pi tests. Every test runs it in a scratch copy: a one-crate workspace with two Rust tests, a pi
//! test file, and the repo's own `node_modules` for `tsc`.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{output, TestEnv};

const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// The cargo that built these tests, with the `cargo-nextest` beside it or on `PATH`.
const CARGO: &str = env!("CARGO");

const PI_TESTS: &str = r#"import { test } from "node:test";
import assert from "node:assert/strict";

test("passes", () => {});
test("fails", () => { assert.fail("red"); });
test("a.c (1+1) [x]", () => {});
test("abc", () => { assert.fail("the regex characters matched too much"); });
test("a.c (1+1) [x] more", () => { assert.fail("a prefix matched"); });
"#;

const RUST_TESTS: &str = "#[test]\nfn green_one() {}\n#[test]\nfn red_one() { panic!(\"red\"); }\n";

const PI: &str = "pi/test/sample.test.ts";

struct Scratch {
    env: TestEnv,
    dir: PathBuf,
}

struct Out {
    exit: i32,
    stdout: String,
    stderr: String,
}

fn write(dir: &Path, path: &str, text: &str) {
    let file = dir.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

impl Scratch {
    fn new() -> Scratch {
        let env = TestEnv::empty();
        let dir = env.root().join("scratch");
        let repo = Path::new(REPO);
        for script in ["tests.sh", "cargo.sh"] {
            fs::create_dir_all(dir.join("scripts")).unwrap();
            fs::copy(repo.join("scripts").join(script), dir.join("scripts").join(script)).unwrap();
        }
        write(&dir, "Cargo.toml", "[workspace]\nresolver = \"3\"\nmembers = [\"crate1\"]\n");
        write(&dir, "crate1/Cargo.toml", "[package]\nname = \"crate1\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
        write(&dir, "crate1/src/lib.rs", "");
        write(&dir, "crate1/tests/sample.rs", RUST_TESTS);
        write(&dir, PI, PI_TESTS);
        write(&dir, "package.json", "{\"private\": true, \"type\": \"module\", \"scripts\": {\"typecheck\": \"tsc --noEmit -p .\"}}\n");
        write(
            &dir,
            "tsconfig.json",
            "{\"compilerOptions\": {\"strict\": true, \"noEmit\": true, \"erasableSyntaxOnly\": true, \"module\": \"nodenext\", \"moduleResolution\": \"nodenext\", \"allowImportingTsExtensions\": true, \"target\": \"ES2023\", \"skipLibCheck\": true, \"types\": [\"node\"]}, \"include\": [\"pi\"]}\n",
        );
        std::os::unix::fs::symlink(repo.join("node_modules"), dir.join("node_modules")).unwrap();
        Scratch { env, dir }
    }

    /// `scripts/tests.sh <args>` in the scratch copy, started from a directory that is not its root.
    fn run(&self, args: &[&str]) -> Out {
        let cargo_dir = Path::new(CARGO).parent().expect("cargo's directory");
        let path = std::env::join_paths(
            std::iter::once(cargo_dir.to_path_buf()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
        )
        .unwrap();
        let mut cmd = self.env.command("sh");
        cmd.arg(self.dir.join("scripts/tests.sh")).args(args).current_dir(self.dir.join("pi")).env("CARGO", CARGO).env("PATH", path);
        // An outer nextest's variables would make the inner one believe it is a child of that run.
        for (key, _) in std::env::vars_os().filter_map(|(k, v)| k.to_str().map(|k| (k.to_string(), v))) {
            if key.starts_with("NEXTEST") || key.starts_with("CARGO_TARGET") || key == "CARGO_BUILD_TARGET_DIR" {
                cmd.env_remove(key);
            }
        }
        let out = output(&mut cmd).unwrap();
        Out {
            exit: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

fn pi(title: &str) -> String {
    format!("{PI}::{title}")
}

#[test]
fn a_rust_name_and_a_pi_name_each_run_and_pass() {
    let s = Scratch::new();
    let rust = s.run(&["run", "green_one"]);
    assert_eq!(rust.exit, 0, "{}{}", rust.stdout, rust.stderr);
    let both = s.run(&["run", "green_one", &pi("passes")]);
    assert_eq!(both.exit, 0, "{}{}", both.stdout, both.stderr);
    assert!(both.stdout.contains("passes"), "{}", both.stdout);
    assert!(s.run(&["list", "green_one"]).exit == 0);
    assert!(s.run(&["list", &pi("passes")]).exit == 0);
}

#[test]
fn a_failing_test_makes_run_exit_non_zero() {
    let s = Scratch::new();
    let out = s.run(&["run", &pi("fails")]);
    assert_ne!(out.exit, 0, "{}{}", out.stdout, out.stderr);
    let mixed = s.run(&["run", &pi("passes"), &pi("fails")]);
    assert_ne!(mixed.exit, 0, "{}{}", mixed.stdout, mixed.stderr);
    let rust = s.run(&["run", "red_one"]);
    assert_ne!(rust.exit, 0, "{}{}", rust.stdout, rust.stderr);
    let both = s.run(&["run", "green_one", &pi("fails")]);
    assert_ne!(both.exit, 0, "{}{}", both.stdout, both.stderr);
}

#[test]
fn a_misspelled_name_makes_list_and_run_exit_non_zero() {
    let s = Scratch::new();
    assert_ne!(s.run(&["list", &pi("pases")]).exit, 0);
    assert_ne!(s.run(&["list", "pi/test/missing.test.ts::passes"]).exit, 0);
    assert_ne!(s.run(&["list", "gren_one"]).exit, 0);
    // A title that is only a prefix of a test's title is no test either.
    assert_ne!(s.run(&["list", &pi("pass")]).exit, 0);
    let out = s.run(&["run", &pi("pases")]);
    assert_ne!(out.exit, 0, "a title that matches no test must not read as a pass: {}{}", out.stdout, out.stderr);
}

#[test]
fn a_pi_title_with_regex_characters_runs_exactly_that_test() {
    let s = Scratch::new();
    let out = s.run(&["run", &pi("a.c (1+1) [x]")]);
    assert_eq!(out.exit, 0, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.contains("a.c (1+1) [x]"), "{}", out.stdout);
    assert!(!out.stdout.contains("abc"), "{}", out.stdout);
    assert!(!out.stdout.contains("more"), "{}", out.stdout);
    assert_eq!(s.run(&["list", &pi("a.c (1+1) [x]")]).exit, 0);
}

#[test]
fn build_passes_when_the_typescript_typechecks_and_fails_when_it_does_not() {
    let s = Scratch::new();
    let ok = s.run(&["build"]);
    assert_eq!(ok.exit, 0, "{}{}", ok.stdout, ok.stderr);
    write(&s.dir, "pi/bad.ts", "export const x: number = \"not a number\";\n");
    let bad = s.run(&["build"]);
    assert_ne!(bad.exit, 0, "{}{}", bad.stdout, bad.stderr);
    assert!(bad.stdout.contains("TS2322") || bad.stderr.contains("TS2322"), "{}{}", bad.stdout, bad.stderr);
}
