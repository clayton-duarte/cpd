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
fn resource_decl_a_repo_file_declares_named_resources() {
    let r = resolve_repo("[resources.e2e-port]\n\n[resources.db]\n").unwrap();
    assert_eq!(r.policy.resources.names, ["db", "e2e-port"]);
}

#[test]
fn resource_decl_a_name_outside_the_pattern_is_policy_invalid() {
    let long = "a".repeat(33);
    for name in ["E2E", "9x", long.as_str()] {
        assert_eq!(
            invalid_key(&format!("[resources.{name}]\n")),
            serde_json::json!(format!("resources.{name}"))
        );
    }
}

#[test]
fn resource_decl_a_key_inside_a_resource_is_policy_invalid() {
    assert_eq!(
        invalid_key("[resources.e2e-port]\nport = 8080\n"),
        serde_json::json!("resources.e2e-port.port")
    );
}

#[test]
fn resource_decl_a_text_key_in_resources_is_policy_invalid() {
    assert_eq!(
        invalid_key("[resources]\ntext = \"x\"\n"),
        serde_json::json!("resources.text")
    );
}

#[test]
fn resource_decl_the_default_policy_declares_no_resources() {
    assert!(default_policy().resources.names.is_empty());
}

fn owner_key() -> PolicyKey {
    PolicyKey::Owner(OwnerLogin::parse("acme").unwrap())
}

/// The policy files of a home: the host file, the owner file and the repo file `acme/cpd` (`None`: the file is absent).
fn resolve_levels(host: &str, owner: &str, repo: Option<&str>) -> Result<Resolved, CpdError> {
    let (_d, home) = home();
    write(&home, "host.toml", host);
    write(&home, "acme/owner.toml", owner);
    match repo {
        Some(text) => {
            write(&home, "acme/cpd.toml", text);
            resolve(&home, &repo_key())
        }
        None => resolve(&home, &owner_key()),
    }
}

/// `(name, scope, slots, the file's name)` of each declared resource, in order.
fn declared(r: &Resolved) -> Vec<(String, ResourceScope, u8, String)> {
    r.policy
        .resources
        .declared
        .iter()
        .map(|d| {
            (
                d.name.clone(),
                d.scope,
                d.slots,
                std::path::Path::new(&d.source)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .collect()
}

fn entry(
    name: &str,
    scope: ResourceScope,
    slots: u8,
    file: &str,
) -> (String, ResourceScope, u8, String) {
    (name.to_owned(), scope, slots, file.to_owned())
}

#[test]
fn resource_slots_default_to_one_slot() {
    let r = resolve_repo("[resources.x]\n\n[resources.y]\nslots = 1\n").unwrap();

    assert_eq!(
        declared(&r),
        [
            entry("x", ResourceScope::Repo, 1, "cpd.toml"),
            entry("y", ResourceScope::Repo, 1, "cpd.toml")
        ]
    );
}

#[test]
fn resource_slots_are_declared_in_host_owner_and_repo_files() {
    let r = resolve_levels(
        "[resources.h]\nslots = 4\n",
        "[resources.o]\nslots = 64\n",
        Some("[resources.r]\nslots = 2\n"),
    )
    .expect("host, owner and repo files may declare resources");

    assert_eq!(
        declared(&r),
        [
            entry("h", ResourceScope::Host, 4, "host.toml"),
            entry("o", ResourceScope::Owner, 64, "owner.toml"),
            entry("r", ResourceScope::Repo, 2, "cpd.toml")
        ]
    );
    let source = &r.policy.resources.declared[0].source;
    assert!(
        std::path::Path::new(source).is_absolute() && source.ends_with("policy/host.toml"),
        "{source}"
    );
}

#[test]
fn resource_slots_outside_one_to_sixty_four_are_policy_invalid() {
    for bad in ["0", "65", "-1", "1.5", "\"2\"", "true"] {
        let text = format!("[resources.x]\nslots = {bad}\n");
        let key = std::panic::catch_unwind(|| invalid_key(&text))
            .unwrap_or_else(|_| panic!("slots = {bad} is policy_invalid"));
        assert_eq!(key, serde_json::json!("resources.x.slots"), "slots = {bad}");
    }
    for ok in ["1", "64"] {
        assert!(
            resolve_repo(&format!("[resources.x]\nslots = {ok}\n")).is_ok(),
            "slots = {ok}"
        );
    }
}

#[test]
fn resource_unknown_key_in_a_declaration_is_policy_invalid() {
    // Each of the three files takes a declaration with `slots` and refuses one with any other key.
    for (file, valid, invalid) in [
        (
            "host.toml",
            "[resources.x]\nslots = 2\n",
            "[resources.x]\nslots = 2\nport = 1\n",
        ),
        (
            "owner.toml",
            "[resources.x]\nslots = 2\n",
            "[resources.x]\nslots = 2\nport = 1\n",
        ),
        (
            "cpd.toml",
            "[resources.x]\nslots = 2\n",
            "[resources.x]\nslots = 2\nport = 1\n",
        ),
    ] {
        let levels = |text: &str| match file {
            "host.toml" => resolve_levels(text, "", Some("")),
            "owner.toml" => resolve_levels("", text, Some("")),
            _ => resolve_levels("", "", Some(text)),
        };
        levels(valid).unwrap_or_else(|e| panic!("{file} takes `slots`: {e:?}"));
        let err =
            levels(invalid).expect_err(&format!("an unknown key in {file} is policy_invalid"));
        assert_eq!(err.code(), ErrorCode::PolicyInvalid, "{file}");
        assert!(
            err.details()["file"].as_str().unwrap().ends_with(file),
            "{file}: {:?}",
            err.details()
        );
        assert_eq!(err.details()["key"], "resources.x.port", "{file}");
    }
}

#[test]
fn resource_resolution_is_the_union_of_the_three_levels() {
    let r = resolve_levels(
        "[resources.h]\n",
        "[resources.o]\n",
        Some("[resources.r]\n"),
    )
    .unwrap();

    assert_eq!(r.policy.resources.names, ["h", "o", "r"]);
    assert_eq!(
        declared(&r).iter().map(|d| d.1).collect::<Vec<_>>(),
        [
            ResourceScope::Host,
            ResourceScope::Owner,
            ResourceScope::Repo
        ]
    );
    // An owner key sees the host's and its own; a repo file with a `[resources]` table replaces nothing wider.
    let owner = resolve_levels("[resources.h]\n", "[resources.o]\n", None).unwrap();
    assert_eq!(owner.policy.resources.names, ["h", "o"]);
    let empty_table =
        resolve_levels("[resources.h]\n", "[resources.o]\n", Some("[resources]\n")).unwrap();
    assert_eq!(empty_table.policy.resources.names, ["h", "o"]);
}

#[test]
fn resource_resolution_narrower_declaration_wins_per_name() {
    // The worked example of §8.3: acme/web declares `staging` with 3 slots; acme/api has no file.
    let host = "[resources.shared-port]\nslots = 2\n";
    let owner = "[resources.staging]\n";
    let web = resolve_levels(
        host,
        owner,
        Some("[resources.staging]\nslots = 3\n\n[resources.okta-port]\n"),
    )
    .unwrap();
    assert_eq!(
        declared(&web),
        [
            entry("okta-port", ResourceScope::Repo, 1, "cpd.toml"),
            entry("shared-port", ResourceScope::Host, 2, "host.toml"),
            entry("staging", ResourceScope::Repo, 3, "cpd.toml"),
        ]
    );
    assert_eq!(
        web.policy.resources.names,
        ["okta-port", "shared-port", "staging"]
    );

    let api = resolve_levels(host, owner, None).unwrap();
    assert_eq!(
        declared(&api),
        [
            entry("shared-port", ResourceScope::Host, 2, "host.toml"),
            entry("staging", ResourceScope::Owner, 1, "owner.toml")
        ]
    );

    // Owner over host, with the owner's own slots: nothing of the host's declaration is merged.
    let both = resolve_levels("[resources.x]\nslots = 5\n", "[resources.x]\n", None).unwrap();
    assert_eq!(
        declared(&both),
        [entry("x", ResourceScope::Owner, 1, "owner.toml")]
    );
}

#[test]
fn workers_name_in_a_policy_file_is_policy_invalid() {
    // `workers` is the built-in resource (contract §8.3): reserved at every level, `details.key` `resources.workers`.
    assert_eq!(
        invalid_key("[resources.workers]\n"),
        serde_json::json!("resources.workers")
    );
    assert_eq!(
        invalid_key("[resources.workers]\nslots = 2\n"),
        serde_json::json!("resources.workers")
    );
    for (host, owner) in [("[resources.workers]\n", ""), ("", "[resources.workers]\n")] {
        let err = resolve_levels(host, owner, Some(""))
            .expect_err("a host or owner file may not declare `workers` either");
        assert_eq!(err.code(), ErrorCode::PolicyInvalid);
        assert_eq!(err.details()["key"], "resources.workers");
    }
}
