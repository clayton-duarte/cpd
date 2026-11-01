use super::shared::is_shared;

#[test]
fn is_shared_names_the_four_lockfiles_at_any_depth() {
    for name in [
        "Cargo.lock",
        "pnpm-lock.yaml",
        "package-lock.json",
        "yarn.lock",
    ] {
        for dir in ["", "core/", "a/b/c/", ".cpd/x/"] {
            let path = format!("{dir}{name}");
            assert!(is_shared(&path), "{path}");
        }
    }
}

#[test]
fn is_shared_is_false_for_other_names() {
    for path in [
    for path in [
        "Cargo.toml",
        "Cargo.lock.bak",
        "my-Cargo.lock",
        "lock",
        "bun.lockb",
        "go.sum",
        "cargo.lock",
        "package.json",
        "",
    ] {
    ] {
        assert!(!is_shared(path), "{path:?}");
    }
}

#[test]
fn is_shared_reads_the_name_after_the_last_slash() {
    assert!(!is_shared("Cargo.lock/x.rs"));
    assert!(!is_shared("docs/"));
    assert!(!is_shared("Cargo.lock/"));
    assert!(is_shared("a/Cargo.lock/yarn.lock"));
}
