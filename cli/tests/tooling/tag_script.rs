//! `scripts/tag-version.sh`: tags the commit of `origin/main` that last changed Cargo.toml's workspace version with the annotated
//! tag `v<version>` and pushes it; idempotent. Against a temporary bare origin and a clone of it.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{output, TestEnv};

const SCRIPTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts");

struct Fixture {
    env: TestEnv,
    origin: PathBuf,
    clone: PathBuf,
    /// c1 (0.0.5), c2 (0.13.45), c3 (unrelated), all pushed to origin main.
    c1: String,
    c2: String,
    c3: String,
}

struct Run {
    exit: i32,
    stdout: String,
    stderr: String,
}

fn cargo_toml(version: &str) -> String {
    format!("[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"{version}\"\npublish = false\n\n[workspace.dependencies]\nclap = {{ version = \"4.6\" }}\n")
}

impl Fixture {
    fn new() -> Fixture {
        let env = TestEnv::new();
        let origin = env.root().join("repos/tag-origin.git");
        env.git(env.root(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        let clone = env.root().join("repos/tag-clone");
        env.git(env.root(), &["clone", "-q", origin.to_str().unwrap(), clone.to_str().unwrap()]);
        let mut fx = Fixture { env, origin, clone, c1: String::new(), c2: String::new(), c3: String::new() };
        fx.c1 = fx.commit("Cargo.toml", &cargo_toml("0.0.5"), "c1");
        fx.c2 = fx.commit("Cargo.toml", &cargo_toml("0.13.45"), "c2");
        fx.c3 = fx.commit("other.txt", "unrelated\n", "c3");
        fx.env.git(&fx.clone, &["push", "-q", "origin", "main"]);
        // The script is copied in after the history: it is no part of it.
        let dest = fx.clone.join("scripts/tag-version.sh");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        common::copy_executable(&format!("{SCRIPTS}/tag-version.sh"), &dest);
        fx
    }

    /// Writes `file`, commits it in the clone, returns the commit.
    fn commit(&self, file: &str, text: &str, message: &str) -> String {
        fs::write(self.clone.join(file), text).unwrap();
        self.env.git(&self.clone, &["add", file]);
        self.env.git(&self.clone, &["commit", "-q", "-m", message]);
        self.env.git(&self.clone, &["rev-parse", "HEAD"])
    }

    fn run(&self) -> Run {
        let script: &Path = &self.clone.join("scripts/tag-version.sh");
        let out = output(self.env.command(script).current_dir(&self.clone)).unwrap();
        Run {
            exit: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// `<tag> <object type> <commit it points at>` for every tag of origin, sorted.
    fn origin_tags(&self) -> Vec<String> {
        let names = self.env.git(&self.origin, &["tag", "--list"]);
        let mut tags: Vec<String> = names
            .lines()
            .map(|t| {
                let kind = self.env.git(&self.origin, &["cat-file", "-t", t]);
                let target = self.env.git(&self.origin, &["rev-parse", &format!("{t}^{{commit}}")]);
                format!("{t} {kind} {target}")
            })
            .collect();
        tags.sort();
        tags
    }

    fn origin_tag(&self, tag: &str) -> Option<(String, String)> {
        let line = self.origin_tags().into_iter().find(|l| l.split(' ').next() == Some(tag))?;
        let mut parts = line.split(' ').skip(1);
        Some((parts.next().unwrap().to_owned(), parts.next().unwrap().to_owned()))
    }
}

#[test]
fn tags_the_commit_that_last_changed_the_version() {
    let fx = Fixture::new();

    let run = fx.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    let (kind, target) = fx.origin_tag("v0.13.45").unwrap_or_else(|| panic!("origin has no v0.13.45: {:?}; stderr {}", fx.origin_tags(), run.stderr));
    assert_eq!(kind, "tag", "the tag is annotated");
    assert_eq!(target, fx.c2, "v0.13.45 is on c2, not on c3 ({})", fx.c3);
    assert!(run.stdout.contains(&format!("tagged v0.13.45 on {}", fx.c2)), "{}", run.stdout);
}

#[test]
fn a_second_run_changes_nothing() {
    let fx = Fixture::new();
    assert_eq!(fx.run().exit, 0);
    let before = fx.origin_tags();
    assert!(!before.is_empty(), "the first run tagged nothing");

    let run = fx.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert!(run.stdout.contains("nothing to do"), "{}", run.stdout);
    assert_eq!(fx.origin_tags(), before);
}

#[test]
fn an_existing_remote_tag_is_left_alone() {
    let fx = Fixture::new();
    fx.env.git(&fx.clone, &["tag", "v0.13.45", &fx.c1]);
    fx.env.git(&fx.clone, &["push", "-q", "origin", "v0.13.45"]);
    fx.env.git(&fx.clone, &["tag", "-d", "v0.13.45"]);

    let run = fx.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(fx.origin_tag("v0.13.45").map(|t| t.1), Some(fx.c1.clone()), "the tag still points at c1");
    assert!(run.stdout.contains("nothing to do"), "{}", run.stdout);
}

#[test]
fn a_new_version_gets_its_own_tag() {
    let fx = Fixture::new();
    assert_eq!(fx.run().exit, 0);
    let c4 = fx.commit("Cargo.toml", &cargo_toml("0.13.46"), "c4");
    fx.commit("other.txt", "unrelated again\n", "c5");
    fx.env.git(&fx.clone, &["push", "-q", "origin", "main"]);

    let run = fx.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(fx.origin_tag("v0.13.46").map(|t| t.1), Some(c4), "v0.13.46 is on c4");
    assert_eq!(fx.origin_tag("v0.13.45").map(|t| t.1), Some(fx.c2.clone()), "v0.13.45 stays on c2");
}

#[test]
fn it_reads_origin_main_not_the_checkout() {
    let fx = Fixture::new();
    fx.env.git(&fx.clone, &["checkout", "-q", "-b", "side"]);
    fx.commit("Cargo.toml", &cargo_toml("0.99.9"), "side version");

    let run = fx.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(fx.origin_tag("v0.13.45").map(|t| t.1), Some(fx.c2.clone()), "{:?}", fx.origin_tags());
    assert!(fx.origin_tag("v0.99.9").is_none(), "{:?}", fx.origin_tags());
    assert!(fx.env.git(&fx.clone, &["tag", "--list", "v0.99.9"]).is_empty(), "no local v0.99.9");
    assert_eq!(fx.env.git(&fx.clone, &["branch", "--show-current"]), "side", "no checkout");
}

#[test]
fn an_unreachable_origin_fails_loud_and_tags_nothing() {
    let fx = Fixture::new();
    fx.env.git(&fx.clone, &["remote", "set-url", "origin", fx.env.root().join("nowhere").to_str().unwrap()]);

    let run = fx.run();

    assert_eq!(run.exit, 1, "{}", run.stderr);
    assert!(run.stderr.lines().any(|l| l.starts_with("tag-version:")), "{}", run.stderr);
    assert!(fx.env.git(&fx.clone, &["tag", "--list"]).is_empty(), "a local tag exists");
}

#[test]
fn a_local_tag_whose_push_failed_is_pushed_by_the_next_run() {
    let fx = Fixture::new();
    fx.env.git(&fx.clone, &["tag", "-a", "v0.13.45", &fx.c2, "-m", "v0.13.45"]);
    assert!(fx.origin_tag("v0.13.45").is_none());

    let run = fx.run();

    assert_eq!(run.exit, 0, "{}", run.stderr);
    assert_eq!(fx.origin_tag("v0.13.45").map(|t| t.1), Some(fx.c2.clone()), "{:?}", fx.origin_tags());
}
