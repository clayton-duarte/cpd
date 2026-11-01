//! The `workflow.jira_required` policy key (contract §8.3): default, scopes and the loader's refusal,
//! through `cpd policy show` against a temporary `$CPD_HOME`.

use super::policy_post::{keys, real, show, world, write};
use crate::common::refused;
use serde_json::json;

#[test]
fn jira_required_is_false_by_default() {
    let fx = world("");
    let out = show(&fx, false).ok();
    assert_eq!(
        out["policy"]["workflow"]["keys"]["workflow.jira_required"],
        json!({"value": false, "source": "default"}),
        "{out}"
    );
}

#[test]
fn an_owner_file_sets_jira_required_and_a_repo_file_overrides_it() {
    let fx = world("[workflow]\njira_required = true\n");
    let out = show(&fx, false).ok();
    let owner = format!("owner:{}", real(&fx, "acme/owner.toml"));
    assert_eq!(
        out["policy"]["workflow"]["keys"]["workflow.jira_required"],
        json!({"value": true, "source": owner}),
        "{out}"
    );

    write(&fx, "acme/r.toml", "[workflow]\njira_required = false\n");
    let out = show(&fx, true).ok();
    let repo = format!("repo:{}", real(&fx, "acme/r.toml"));
    assert_eq!(
        out["policy"]["workflow"]["keys"]["workflow.jira_required"],
        json!({"value": false, "source": repo}),
        "{out}"
    );
}

#[test]
fn jira_required_refuses_a_value_that_is_not_a_boolean() {
    let ok = world("[workflow]\njira_required = true\n");
    show(&ok, false).ok();
    drop(ok);
    let fx = world("[workflow]\njira_required = \"yes\"\n");
    let error = refused(&show(&fx, false), "policy_invalid", 1);
    assert_eq!(error["details"]["key"], "workflow.jira_required", "{error}");
}
