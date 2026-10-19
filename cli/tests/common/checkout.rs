//! A temporary checkout of cpd's install files with an `origin/main` of its own, for tests of
//! `scripts/install-cpd.sh`: whether a migration file is on `origin/main` is decided by the test,
//! never by the real repository. Include it with `#[path = "common/checkout.rs"] mod checkout;`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::common::TestEnv;

const SCRIPTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts");

/// A clone of an origin whose `main` has the install script, `scripts/cargo.sh`, a one-file
/// adapter and the migration files `on_main` (each with its own name as text).
pub struct Checkout {
    /// The clone: the tree the script builds from.
    pub root: PathBuf,
}

impl Checkout {
    pub fn new(env: &TestEnv, on_main: &[&str]) -> Checkout {
        let origin = env.git_repo("install-origin");
        fs::create_dir_all(origin.join("scripts")).unwrap();
        for script in ["install-cpd.sh", "cargo.sh"] {
            fs::copy(Path::new(SCRIPTS).join(script), origin.join("scripts").join(script)).unwrap();
        }
        fs::create_dir_all(origin.join("pi")).unwrap();
        fs::write(origin.join("pi/package.json"), "{}\n").unwrap();
        let migrations = origin.join("core/src/store/migrations");
        fs::create_dir_all(&migrations).unwrap();
        for name in on_main {
            fs::write(migrations.join(name), format!("{name}\n")).unwrap();
        }
        env.git(&origin, &["add", "-A"]);
        env.git(&origin, &["commit", "-q", "-m", "install files"]);
        let root = env.root().join("repos/install-clone");
        env.git(env.root(), &["clone", "-q", origin.to_str().unwrap(), root.to_str().unwrap()]);
        Checkout { root }
    }

    /// Writes (adds or changes) a migration file of the working tree, uncommitted.
    pub fn write_migration(&self, name: &str, text: &str) {
        fs::write(self.root.join("core/src/store/migrations").join(name), text).unwrap();
    }

    pub fn script(&self) -> PathBuf {
        self.root.join("scripts/install-cpd.sh")
    }
}
