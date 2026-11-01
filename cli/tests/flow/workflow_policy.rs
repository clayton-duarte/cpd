//! The `workflow` policy domain (contract §8.3; map R10 of `docs/research/glossary-audit-rename.md`):
//! `workflow.jira_required` is accepted, the retired `pr.jira_required` is refused, and the exposure
//! table lists the new name, through `cpd policy show` against a temporary `$CPD_HOME`.

use super::policy_post::{real, show, world, write};
use crate::common::refused;
use serde_json::{json, Value};

/// The `policy show` output's `workflow` domain keys.
fn keys(out: &Value) -> &Value {
    &out["policy"]["workflow"]["keys"]
}

#[test]
fn domain_and_exposure() {
    // The `workflow` domain is a domain of the resolved policy: the exposure table lists the new
    // name, and `workflow.jira_required` takes cpd's default of false.
    let fx = world("");
    let out = show(&fx, false).ok();
    let mut domains: Vec<&str> = out["policy"].as_object().unwrap().keys().map(String::as_str).collect();
    domains.sort_unstable();
    assert!(domains.contains(&"workflow"), "the workflow domain is in the exposure table: {out}");
    assert!(domains.contains(&"pr"), "the pr domain stays: {out}");
    assert_eq!(keys(&out)["workflow.jira_required"], json!({"value": false, "source": "default"}), "{out}");

    // The retired `pr.jira_required` is no longer a key of the `pr` domain.
    assert!(out["policy"]["pr"]["keys"].get("pr.jira_required").is_none(), "{out}");

    drop(fx);
    // An owner file sets `workflow.jira_required`; a repo file overrides it.
    let fx = world("[workflow]\njira_required = true\n");
    let out = show(&fx, false).ok();
    let owner = format!("owner:{}", real(&fx, "acme/owner.toml"));
    assert_eq!(keys(&out)["workflow.jira_required"], json!({"value": true, "source": owner}), "{out}");
    write(&fx, "acme/r.toml", "[workflow]\njira_required = false\n");
    let out = show(&fx, true).ok();
    let repo = format!("repo:{}", real(&fx, "acme/r.toml"));
    assert_eq!(keys(&out)["workflow.jira_required"], json!({"value": false, "source": repo}), "{out}");

    drop(fx);
    // The retired `pr.jira_required` is `policy_invalid`.
    let fx = world("[pr]\njira_required = true\n");
    let error = refused(&show(&fx, false), "policy_invalid", 1);
    assert_eq!(error["details"]["key"], "pr.jira_required", "{error}");

    // The exposure table keeps the moved key's exposure: `workflow` is set in a machine, an owner
    // and a repo file.
    drop(fx);
    for rel in ["host.toml", "acme/owner.toml", "acme/r.toml"] {
        let fx = world("");
        write(&fx, rel, "[workflow]\njira_required = true\n");
        show(&fx, rel == "acme/r.toml").ok();
    }
}
