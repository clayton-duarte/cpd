//! The `pr.human_gated_contexts` policy key (contract §8.3): default, scopes and the loader's
//! refusal, through `cpd policy show` against a temporary `$CPD_HOME`.

use super::policy_post::{keys, real, show, world, write};
use crate::common::refused;
use serde_json::json;

const KEY: &str = "pr.human_gated_contexts";

#[test]
fn human_gated_contexts_default_is_the_chromatic_pair() {
    let fx = world("");
    let out = show(&fx, false).ok();
    assert_eq!(
        keys(&out)[KEY],
        json!({"value": ["UI Review", "UI Tests"], "source": "default"}),
        "{out}"
    );
}

#[test]
fn human_gated_contexts_is_set_by_an_owner_file_and_overridden_by_a_repo_file() {
    let fx = world("[pr]\nhuman_gated_contexts = [\"ci/review\"]\n");
    let out = show(&fx, false).ok();
    let owner = format!("owner:{}", real(&fx, "acme/owner.toml"));
    assert_eq!(
        keys(&out)[KEY],
        json!({"value": ["ci/review"], "source": owner}),
        "{out}"
    );

    write(
        &fx,
        "acme/r.toml",
        "[pr]\nhuman_gated_contexts = [\"a\", \"b\"]\n",
    );
    let out = show(&fx, true).ok();
    let repo = format!("repo:{}", real(&fx, "acme/r.toml"));
    assert_eq!(
        keys(&out)[KEY],
        json!({"value": ["a", "b"], "source": repo}),
        "{out}"
    );
}

#[test]
fn human_gated_contexts_empty_list_turns_it_off() {
    let fx = world("[pr]\nhuman_gated_contexts = []\n");
    let out = show(&fx, false).ok();
    let owner = format!("owner:{}", real(&fx, "acme/owner.toml"));
    assert_eq!(
        keys(&out)[KEY],
        json!({"value": [], "source": owner}),
        "{out}"
    );
}

#[test]
fn human_gated_contexts_refuses_a_value_that_is_not_a_list_of_strings() {
    let ok = world("[pr]\nhuman_gated_contexts = [\"a\"]\n");
    show(&ok, false).ok();
    drop(ok);
    for value in ["\"UI Review\"", "[1]", "[\"a\", 2]"] {
        let fx = world(&format!("[pr]\nhuman_gated_contexts = {value}\n"));
        let error = refused(&show(&fx, false), "policy_invalid", 1);
        assert_eq!(error["details"]["key"], KEY, "{value}: {error}");
        drop(fx);
    }
}

#[test]
fn human_gated_contexts_left_out_of_a_set_pr_table_takes_cpds_default() {
    let fx = world("");
    write(&fx, "acme/r.toml", "[pr]\njira_required = true\n");
    let out = show(&fx, true).ok();
    let repo = format!("repo:{}", real(&fx, "acme/r.toml"));
    assert_eq!(
        keys(&out)[KEY],
        json!({"value": ["UI Review", "UI Tests"], "source": format!("{repo} (key default)")}),
        "{out}"
    );
}
