use super::{
    PolicyKey, SelectionCapabilities, check_file_names, resolve, resolve_owner_role_selection,
};
use crate::home::CpdHome;
use crate::ids::{OwnerLogin, RepoName};

const ROLES: &str = r#"
[role_selection]
provider = "bifrost"
[role_selection.manager]
model = "bifrost/manager"
effort = 2
[role_selection.lead]
model = "bifrost/lead"
effort = 1
[role_selection.worker]
model = "bifrost/worker"
effort = 0
"#;
struct FakeCapabilities;
impl SelectionCapabilities for FakeCapabilities {
    fn supports(&self, _: &str, _: u8) -> bool {
        true
    }
}
fn setup() -> (tempfile::TempDir, CpdHome, PolicyKey) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    std::fs::create_dir_all(home.root().join("policy/missionlane")).unwrap();
    let key = PolicyKey::Repo(
        OwnerLogin::parse("missionlane").unwrap(),
        RepoName::parse("app").unwrap(),
    );
    (dir, home, key)
}
#[test]
fn owner_roles_coexist_with_legacy_resolution() {
    let (_dir, home, key) = setup();
    std::fs::write(
        home.root().join("policy/missionlane/owner.toml"),
        format!("[pr]\n{}", ROLES),
    )
    .unwrap();
    resolve_owner_role_selection(&home, &key, &FakeCapabilities).unwrap();
    check_file_names(&home).unwrap();
    resolve(&home, &key).unwrap();
}
#[test]
fn shared_default_is_not_a_scope_file() {
    let (_dir, home, key) = setup();
    std::fs::write(home.root().join("policy/missionlane/owner.toml"), "[pr]\n").unwrap();
    std::fs::write(home.root().join("policy/default.toml"), ROLES).unwrap();
    check_file_names(&home).unwrap();
    resolve(&home, &key).unwrap();
}
