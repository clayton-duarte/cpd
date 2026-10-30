//! The `pr.excused_labels` policy key (contract §8.3): default, scopes, the `[pr]` replace rule and the loader's
//! refusal, through `cpd policy show` against a temporary `$CPD_HOME`.

use super::policy_post::{keys, real, show, world, write};
use crate::common::refused;
use serde_json::json;

const KEY: &str = "pr.excused_labels";

#[test]
fn excused_labels_default_is_empty() {
    let fx = world("");
    let out = show(&fx, false).ok();
    assert_eq!(keys(&out)[KEY], json!({"value": [], "source": "default"}), "{out}");
}

#[test]
fn excused_labels_is_set_by_an_owner_file_and_overridden_by_a_repo_file() {
    let fx = world("[pr]\nexcused_labels = [\"No Merges on Friday\"]\n");
    let out = show(&fx, false).ok();
    let owner = format!("owner:{}", real(&fx, "acme/owner.toml"));
    assert_eq!(keys(&out)[KEY], json!({"value": ["No Merges on Friday"], "source": owner}), "{out}");

    write(&fx, "acme/r.toml", "[pr]\nexcused_labels = [\"a\", \"b\"]\n");
    let out = show(&fx, true).ok();
    let repo = format!("repo:{}", real(&fx, "acme/r.toml"));
    assert_eq!(keys(&out)[KEY], json!({"value": ["a", "b"], "source": repo}), "{out}");
}

#[test]
fn a_repo_pr_table_without_excused_labels_resets_it_to_empty() {
    let fx = world("[pr]\nexcused_labels = [\"dont merge\"]\n");
    write(&fx, "acme/r.toml", "[pr]\njira_required = true\n");
    let out = show(&fx, true).ok();
    let repo = format!("repo:{}", real(&fx, "acme/r.toml"));
    assert_eq!(keys(&out)[KEY], json!({"value": [], "source": format!("{repo} (key default)")}), "{out}");
}

#[test]
fn excused_labels_that_is_not_a_list_of_strings_is_policy_invalid() {
    let ok = world("[pr]\nexcused_labels = [\"a\"]\n");
    show(&ok, false).ok();
    drop(ok);
    for value in ["\"dont merge\"", "[1]", "[\"a\", 2]"] {
        let fx = world(&format!("[pr]\nexcused_labels = {value}\n"));
        let error = refused(&show(&fx, false), "policy_invalid", 1);
        assert_eq!(error["details"]["key"], KEY, "{value}: {error}");
        drop(fx);
    }
}
