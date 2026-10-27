//! Red tests of the `resources` domain (contract §8.3 "The `resources` domain").

use super::*;
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

/// The `policy_invalid` of the repo file `text`: its `details.key`.
fn invalid_key(text: &str) -> serde_json::Value {
    let err = resolve_repo(text).expect_err(&format!("`{text}` is policy_invalid"));
    assert_eq!(err.code(), ErrorCode::PolicyInvalid, "{text}");
    assert!(err.details()["file"].as_str().unwrap().ends_with("acme/cpd.toml"));
    err.details()["key"].clone()
}

#[test]
fn resource_decl_a_repo_file_declares_named_resources() {
    let r = resolve_repo("[resources.e2e-port]\n\n[resources.db]\n").unwrap();
    assert_eq!(r.policy.resources.names, ["db", "e2e-port"]);
}

#[test]
fn resource_decl_a_name_outside_the_pattern_is_policy_invalid() {
    let long = "a".repeat(33);
    for name in ["E2E", "9x", long.as_str()] {
        assert_eq!(invalid_key(&format!("[resources.{name}]\n")), serde_json::json!(format!("resources.{name}")));
    }
}

#[test]
fn resource_decl_a_key_inside_a_resource_is_policy_invalid() {
    assert_eq!(invalid_key("[resources.e2e-port]\nport = 8080\n"), serde_json::json!("resources.e2e-port.port"));
}

#[test]
fn resource_decl_a_text_key_in_resources_is_policy_invalid() {
    assert_eq!(invalid_key("[resources]\ntext = \"x\"\n"), serde_json::json!("resources.text"));
}

#[test]
fn resource_decl_resources_in_an_owner_or_machine_file_are_policy_invalid() {
    let file = std::path::Path::new("/h/policy/acme/owner.toml");
    let scope = parse_scope(file, "[resources.e2e-port]\n").unwrap();
    for kind in [exposure::ScopeKind::Owner, exposure::ScopeKind::Machine] {
        let err = exposure::check_exposed(file, kind, &scope).expect_err("resources is repo only");
        assert_eq!(err.code(), ErrorCode::PolicyInvalid, "{kind:?}");
        assert_eq!(err.details()["key"], "resources", "{kind:?}");
    }
}

#[test]
fn resource_decl_the_default_policy_declares_no_resources() {
    assert!(default_policy().resources.names.is_empty());
}
