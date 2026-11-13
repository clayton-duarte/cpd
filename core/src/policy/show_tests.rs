//! Tests of `policy show`'s typed keys of the `workflow` domain.

use super::*;
use crate::home::CpdHome;
use crate::ids::RepoName;
use crate::policy::{Resolved, resolve};

fn resolve_owner(owner_toml: &str) -> Resolved {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let path = home.root().join("policy/acme/owner.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, owner_toml).unwrap();
    let key = PolicyKey::Repo(
        OwnerLogin::parse("acme").unwrap(),
        RepoName::parse("cpd").unwrap(),
    );
    resolve(&home, &key).unwrap()
}

#[test]
fn workflow_keys_list_local_docs_with_value_and_source() {
    let r = resolve_owner("[workflow]\nlocal_docs = false\n");
    let keys = typed_keys(Domain::Workflow, &r, r.sources.get(Domain::Workflow));
    assert_eq!(keys["workflow.local_docs"]["value"], json!(false), "{keys}");
    assert!(
        keys["workflow.local_docs"]["source"]
            .as_str()
            .unwrap()
            .ends_with("owner.toml"),
        "{keys}"
    );
    let r = resolve_owner("");
    let keys = typed_keys(Domain::Workflow, &r, r.sources.get(Domain::Workflow));
    assert_eq!(keys["workflow.local_docs"]["value"], json!(true), "{keys}");
}
