//! Red tests of the `deploys` domain (contract §8.3 "The `deploys` domain").

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
    PolicyKey::Repo(
        OwnerLogin::parse("acme").unwrap(),
        RepoName::parse("cpd").unwrap(),
    )
}

/// Resolves the repo `acme/cpd` with the repo file `text` (an empty owner file).
fn resolve_repo(text: &str) -> Result<Resolved, CpdError> {
    let (_d, home) = home();
    write(&home, "acme/owner.toml", "");
    write(&home, "acme/cpd.toml", text);
    resolve(&home, &repo_key())
}

fn workflows(r: &Resolved) -> Vec<String> {
    r.policy.deploys.workflows.clone()
}

/// The `policy_invalid` of the repo file `text`: its `details.key`.
fn invalid_key(text: &str) -> serde_json::Value {
    let err = resolve_repo(text).expect_err(&format!("`{text}` is policy_invalid"));
    assert_eq!(err.code(), ErrorCode::PolicyInvalid, "{text}");
    assert!(
        err.details()["file"]
            .as_str()
            .unwrap()
            .ends_with("acme/cpd.toml")
    );
    err.details()["key"].clone()
}

#[test]
fn repo_file_deploys_workflows_resolve_sorted() {
    let r = resolve_repo("[deploys]\nworkflows = [\"main.yml\", \"build.yaml\"]\n").unwrap();
    assert_eq!(workflows(&r), ["build.yaml", "main.yml"]);
    assert!(matches!(
        r.sources.get(Domain::Deploys).source,
        Source::Repo(_)
    ));
    // No table: the default, `[]`.
    let none = resolve_repo("").unwrap();
    assert!(workflows(&none).is_empty());
    assert!(matches!(
        none.sources.get(Domain::Deploys).source,
        Source::Default
    ));
}

#[test]
fn owner_file_deploys_table_is_policy_invalid() {
    let text = "[deploys]\nworkflows = [\"main.yml\"]\n";
    let file = std::path::Path::new("/h/policy/acme/owner.toml");
    let scope = parse_scope(file, text).unwrap();
    let err = exposure::check_exposed(file, exposure::ScopeKind::Owner, &scope)
        .expect_err("deploys is repo only");
    assert_eq!(err.code(), ErrorCode::PolicyInvalid);
    assert_eq!(err.details()["key"], "deploys");
    // The same table in the repo file is read.
    assert_eq!(workflows(&resolve_repo(text).unwrap()), ["main.yml"]);
}

#[test]
fn machine_file_deploys_table_is_policy_invalid() {
    let text = "[deploys]\nworkflows = [\"main.yml\"]\n";
    let file = std::path::Path::new("/h/policy/host.toml");
    let scope = parse_scope(file, text).unwrap();
    let err = exposure::check_exposed(file, exposure::ScopeKind::Machine, &scope)
        .expect_err("deploys is repo only");
    assert_eq!(err.code(), ErrorCode::PolicyInvalid);
    assert_eq!(err.details()["key"], "deploys");
    assert_eq!(workflows(&resolve_repo(text).unwrap()), ["main.yml"]);
}

#[test]
fn invalid_workflow_name_is_policy_invalid() {
    for name in [
        "main",
        "main.json",
        ".hidden.yml",
        "dir/main.yml",
        "-x.yml",
        "a b.yml",
        "",
    ] {
        assert_eq!(
            invalid_key(&format!("[deploys]\nworkflows = [\"{name}\"]\n")),
            serde_json::json!("deploys.workflows"),
            "{name}"
        );
    }
    assert_eq!(
        workflows(&resolve_repo("[deploys]\nworkflows = [\"a.b-c_d.yml\"]\n").unwrap()),
        ["a.b-c_d.yml"]
    );
}

#[test]
fn duplicate_workflow_is_policy_invalid() {
    assert_eq!(
        invalid_key("[deploys]\nworkflows = [\"main.yml\", \"main.yml\"]\n"),
        serde_json::json!("deploys.workflows")
    );
    assert_eq!(
        workflows(&resolve_repo("[deploys]\nworkflows = [\"main.yml\", \"main.yaml\"]\n").unwrap()),
        ["main.yaml", "main.yml"]
    );
}

#[test]
fn policy_show_prints_the_deploys_workflows() {
    let r = resolve_repo("[deploys]\nworkflows = [\"main.yml\", \"build.yaml\"]\n").unwrap();
    let source = r.sources.get(Domain::Deploys);
    let keys = show::typed_keys(Domain::Deploys, &r, source);
    assert_eq!(
        keys["workflows"]["value"],
        serde_json::json!(["build.yaml", "main.yml"])
    );
    assert!(
        keys["workflows"]["source"]
            .as_str()
            .unwrap()
            .ends_with("acme/cpd.toml"),
        "{keys}"
    );
    assert_eq!(Domain::Deploys.name(), "deploys");
}
