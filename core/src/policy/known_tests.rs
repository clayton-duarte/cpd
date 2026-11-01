use super::known_owners;
use crate::ids::OwnerLogin;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
}

fn names(dir: &Path) -> Vec<String> {
    known_owners(dir)
        .iter()
        .map(|o| o.as_str().to_owned())
        .collect()
}

#[test]
fn a_directory_with_owner_toml_is_listed_sorted() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("zeta/owner.toml"));
    touch(&dir.path().join("acme/owner.toml"));
    touch(&dir.path().join("mid/owner.toml"));
    assert_eq!(names(dir.path()), ["acme", "mid", "zeta"]);
    assert_eq!(
        known_owners(dir.path())[0],
        OwnerLogin::parse("acme").unwrap()
    );
}

#[test]
fn a_directory_with_only_repo_files_is_not_listed() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("acme/r.toml"));
    touch(&dir.path().join("beta/owner.toml"));
    assert_eq!(names(dir.path()), ["beta"]);
}

#[test]
fn a_symlink_owner_toml_that_resolves_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("clayton-duarte/owner.toml"));
    fs::create_dir(dir.path().join("stranger")).unwrap();
    symlink(
        "../clayton-duarte/owner.toml",
        dir.path().join("stranger/owner.toml"),
    )
    .unwrap();
    assert_eq!(names(dir.path()), ["clayton-duarte", "stranger"]);
}

#[test]
fn a_dangling_symlink_owner_toml_is_not_listed() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("stranger")).unwrap();
    symlink(
        "../clayton-duarte/owner.toml",
        dir.path().join("stranger/owner.toml"),
    )
    .unwrap();
    assert_eq!(names(dir.path()), Vec::<String>::new());
}

#[test]
fn host_toml_and_root_files_are_not_listed() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("host.toml"));
    touch(&dir.path().join("acme.toml"));
    touch(&dir.path().join("owner.toml"));
    assert_eq!(names(dir.path()), Vec::<String>::new());
}

#[test]
fn names_starting_with_a_dot_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join(".hidden/owner.toml"));
    touch(&dir.path().join(".DS_Store"));
    touch(&dir.path().join("acme/owner.toml"));
    assert_eq!(names(dir.path()), ["acme"]);
}

#[test]
fn a_missing_directory_is_the_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(names(&dir.path().join("absent")), Vec::<String>::new());
}
