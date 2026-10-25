//! The Distiller's pi launch (contract §6.5 "Distiller", hs-d611): no
//! `--no-context-files`, so pi loads the global AGENTS.md, and every other
//! flag and the environment stay as they were.

use std::collections::BTreeMap;

use cpd_core::host::pi::distiller;

const MODEL: &str = "p/sonnet:medium";
const FILES: &[&str] = &["prompt.md", "batch.md", "known.md", "docs.md"];

#[test]
fn distiller_launch_does_not_pass_no_context_files() {
    let argv = distiller(MODEL, FILES).argv;
    assert!(
        !argv.iter().any(|arg| arg == "--no-context-files"),
        "the Distiller must load context files (hs-d611): {argv:?}"
    );
    for flag in ["--no-session", "--no-extensions", "--no-skills", "--no-tools"] {
        assert!(argv.iter().any(|arg| arg == flag), "{flag} in {argv:?}");
    }
    assert!(
        argv.windows(2).any(|pair| pair == ["--mode", "json"]),
        "--mode json in {argv:?}"
    );
    let model = argv.windows(2).find(|pair| pair[0] == "--model").map(|pair| pair[1].as_str());
    assert_eq!(model, Some(MODEL), "model in {argv:?}");

    let separator = argv.iter().position(|arg| arg == "--").unwrap_or_else(|| panic!("no `--` in {argv:?}"));
    let expected: Vec<String> = FILES.iter().map(|file| format!("@{file}")).collect();
    assert_eq!(&argv[separator + 1..], expected.as_slice(), "file arguments after `--` in {argv:?}");
}

#[test]
fn distiller_launch_env_is_only_skip_version_check() {
    let env = distiller(MODEL, FILES).env;
    assert_eq!(env, BTreeMap::from([("PI_SKIP_VERSION_CHECK".to_owned(), "1".to_owned())]));
}
