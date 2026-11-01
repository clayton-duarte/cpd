//! Red tests of the exposure rules (contract §8.3, the domain table's `exposure` column).
//! They pin only the API of `exposure.rs` and the documents.

use super::*;
use crate::error::ErrorCode;
use crate::policy::parse_scope;
use std::path::Path;

use ScopeKind::{Machine, Owner, Repo};

const ALL_THREE: [&str; 7] = ["gate", "pr", "deploy", "code", "tests", "monitor", "instructions"];
const REPO_ONLY: [&str; 4] = ["hooks", "test-commands", "protected", "generated"];

fn domain(name: &str) -> Domain {
    Domain::from_name(name).unwrap_or_else(|| panic!("the domain `{name}` is in Domain::ALL"))
}

/// A scope file that sets only `table` (a TOML text), parsed as the loader parses one.
fn scope(text: &str) -> Scope {
    parse_scope(Path::new("t.toml"), text).unwrap_or_else(|e| panic!("`{text}` is a valid scope: {e}"))
}

const SETS: [(&str, &str); 7] = [
    ("gate", "[gate]\nmerge = \"local\"\n"),
    ("pr", "[pr]\nfeedback.checks = \"none\"\n"),
    ("deploy", "[deploy]\ntext = \"d\"\n"),
    ("code", "[code]\ntext = \"c\"\n"),
    ("tests", "[tests]\ntext = \"t\"\n"),
    ("monitor", "[monitor]\ntext = \"m\"\n"),
    ("instructions", "[instructions]\ntext = \"i\"\n"),
];

fn kind_word(kind: ScopeKind) -> &'static str {
    match kind {
        Machine => "machine",
        Owner => "owner",
        Repo => "repo",
    }
}

// u1
#[test]
fn the_six_text_domains_are_exposed_to_every_scope() {
    for name in ALL_THREE {
        assert_eq!(exposure(domain(name)), [Machine, Owner, Repo], "{name}");
    }
}

#[test]
fn the_repo_only_domains_that_exist_are_exposed_to_the_repo_only() {
    for name in REPO_ONLY {
        if let Some(d) = Domain::from_name(name) {
            assert_eq!(exposure(d), [Repo], "{name}");
        }
    }
}

#[test]
fn every_domain_has_a_non_empty_exposure_in_scope_order() {
    for d in Domain::ALL {
        let scopes = exposure(*d);
        assert!(!scopes.is_empty(), "{}", d.name());
        let order: Vec<usize> = scopes.iter().map(|k| *k as usize).collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{} is listed in the order machine, owner, repo", d.name());
    }
}

// u2
#[test]
fn a_scope_that_sets_only_exposed_domains_is_accepted_in_every_kind() {
    let file = Path::new("/h/policy/x.toml");
    for kind in [Machine, Owner, Repo] {
        for (_, text) in SETS {
            check_exposed(file, kind, &scope(text)).unwrap_or_else(|e| panic!("{kind:?} `{text}`: {e}"));
        }
        check_exposed(file, kind, &scope("")).unwrap();
    }
}

fn refused(kind: ScopeKind, text: &str, key: &str, exposed: &str) {
    let file = Path::new("/h/policy/some/file.toml");
    let error = check_exposed(file, kind, &scope(text)).expect_err("an unexposed domain is refused");
    assert_eq!(error.code(), ErrorCode::PolicyInvalid, "{kind:?} {key}");
    assert_eq!(error.details()["file"], "/h/policy/some/file.toml");
    assert_eq!(error.details()["key"], key);
    let message = error.message();
    assert!(message.contains(&format!("`{key}`")) && message.contains(exposed), "{message}");
    assert!(message.contains(&format!("not a {} file", kind_word(kind))), "{message}");
}

#[test]
fn a_repo_only_domain_in_a_machine_or_owner_scope_is_policy_invalid() {
    for name in REPO_ONLY {
        // Each repo-only domain that exists on the branch is set with its own empty table.
        if Domain::from_name(name).is_none() {
            continue;
        }
        for kind in [Machine, Owner] {
            refused(kind, &format!("[{name}]\n"), name, "repo");
        }
    }
}

// u3
#[test]
fn the_contract_table_exposure_column_is_the_code() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/contract.md");
    let doc = std::fs::read_to_string(path).unwrap();
    let start = doc.find("**The domains** of the first build.").expect("domains table");
    let mut checked = 0;
    let mut in_table = false;
    for line in doc[start..].lines() {
        if !line.starts_with('|') {
            if in_table {
                break;
            }
            continue;
        }
        in_table = true;
        let line = line.replace("\\|", "\u{1}");
        let cells: Vec<String> = line.trim().trim_matches('|').split('|').map(|c| c.trim().to_owned()).collect();
        if cells[0] == "Domain" {
            assert!(cells.iter().any(|c| c == "Exposure"), "the table has an Exposure column: {cells:?}");
            continue;
        }
        if cells[0].starts_with("---") {
            continue;
        }
        let name = cells[0].trim_matches('`');
        // The Exposure cell is the one before the audience (the last).
        let cell = &cells[cells.len() - 2];
        let documented: Vec<&str> = cell.split(',').map(str::trim).collect();
        let code: Vec<&str> = exposure(domain(name)).iter().map(|k| kind_word(*k)).collect();
        assert_eq!(documented, code, "exposure of {name}");
        checked += 1;
    }
    assert!(checked >= ALL_THREE.len(), "the table has the domain rows");
}

#[test]
fn a_pr_post_table_in_a_machine_file_is_policy_invalid() {
    for text in [
        "[pr.post]\nenabled = true\n",
        "[pr]\npost.channel = \"C02JU6C2Z1D\"\n",
        "[pr.post]\n",
        "pr.post.template = \"{{ url }}\"\n",
    ] {
        refused(Machine, text, "pr.post", "owner, repo");
    }
}
