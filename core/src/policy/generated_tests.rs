//! Red tests of the `generated` domain (contract §8.3, §7.5 "The Job's touches"; hs-n641): the repo-only domain whose typed
//! key `paths` lists the paths the touches check lets change without an approval.
//!
//! No test runs git, reads the wall clock or sleeps: the policy files are a temp `$CPD_HOME` fixture.

use super::*;
use crate::error::ErrorCode;
use crate::home::CpdHome;

fn home() -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    (dir, home)
}

fn write(home: &CpdHome, rel: &str, text: &str) {
    let path = home.root().join("policy").join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn repo_key() -> PolicyKey {
    PolicyKey::Repo(OwnerLogin::parse("acme").unwrap(), RepoName::parse("cpd").unwrap())
}

/// Resolves the repo `acme/cpd` with the repo file `text` (an empty owner file).
fn resolve_repo(text: &str) -> Result<Resolved, CpdError> {
    let (_d, home) = home();
    write(&home, "acme/owner.toml", "");
    write(&home, "acme/cpd.toml", text);
    resolve(&home, &repo_key())
}

/// Resolves the owner `acme` with the owner file `text`.
fn resolve_owner(text: &str) -> Result<Resolved, CpdError> {
    let (_d, home) = home();
    write(&home, "acme/owner.toml", text);
    resolve(&home, &PolicyKey::Owner(OwnerLogin::parse("acme").unwrap()))
}

/// The `policy_invalid` of the repo file `text`: its `details.key`.
fn invalid_key(text: &str) -> serde_json::Value {
    let err = resolve_repo(text).expect_err(&format!("`{text}` is policy_invalid"));
    assert_eq!(err.code(), ErrorCode::PolicyInvalid, "{text}");
    assert!(err.details()["file"].as_str().unwrap().ends_with("acme/cpd.toml"), "{text}: {:?}", err.details());
    err.details()["key"].clone()
}

#[test]
fn the_generated_domain_defaults_empty_and_is_repo_only() {
    // cpd's compiled default: no generated path.
    assert!(default_policy().generated.paths.is_empty(), "the default [generated] paths are []");
    // A repo file without the table takes the default, from the default.
    let none = resolve_repo("").unwrap();
    assert!(none.policy.generated.paths.is_empty(), "{:?}", none.policy.generated);
    assert!(matches!(none.sources.get(Domain::Generated).source, Source::Default));

    // `paths` is the typed key: a repo file's `[generated] paths = []` is read (the domain is set, nothing is left out).
    let empty = resolve_repo("[generated]\npaths = []\n").unwrap();
    assert!(empty.policy.generated.paths.is_empty(), "{:?}", empty.policy.generated);
    assert!(matches!(empty.sources.get(Domain::Generated).source, Source::Repo(_)));
    assert!(!empty.sources.get(Domain::Generated).key_default(), "an empty table sets every typed key");

    // A repo-only domain, like `[protected]`: a table in an owner or a machine file is `policy_invalid`, and it has no `text`.
    let err = resolve_owner("[generated]\npaths = [\"pnpm-lock.yaml\"]\n").expect_err("an owner file may not set [generated]");
    assert_eq!(err.code(), ErrorCode::PolicyInvalid);
    assert_eq!(err.details()["key"], "generated");
    assert!(err.message().contains("exposed to repo only"), "{err:?}");
    assert!(audience(Domain::Generated).is_empty(), "a repo-only domain reaches no agent");
    assert_eq!(Domain::from_name("generated"), Some(Domain::Generated), "the domain is in Domain::ALL");
    assert_eq!(invalid_key("[generated]\ntext = \"x\"\n"), serde_json::json!("generated.text"), "no `text` in a repo-only domain");
}

#[test]
fn the_repo_file_declares_generated_paths() {
    let r = resolve_repo("[generated]\npaths = [\"pnpm-lock.yaml\", \"dist/\"]\n").unwrap();
    assert_eq!(r.policy.generated.paths, ["pnpm-lock.yaml", "dist/"], "the repo file's [generated] paths reach the resolved policy");
    assert!(matches!(r.sources.get(Domain::Generated).source, Source::Repo(_)));
    assert!(!r.sources.get(Domain::Generated).key_default(), "`paths` is the typed key of the domain");
    assert!(r.sources.get(Domain::Generated).left_out.is_empty(), "the file sets the typed key");

    // The key holds a list of paths: anything else is `policy_invalid` at the key.
    assert_eq!(invalid_key("[generated]\npaths = \"pnpm-lock.yaml\"\n"), serde_json::json!("generated.paths"));
    assert_eq!(invalid_key("[generated]\nbogus = []\n"), serde_json::json!("generated.bogus"));
    // An empty table is cpd's default, with the key left out.
    let empty = resolve_repo("[generated]\n").unwrap();
    assert!(empty.policy.generated.paths.is_empty());
    assert!(empty.sources.get(Domain::Generated).key_default());
}
