//! The `pr.jira` status-name policy keys (contract §8.3): default, scopes and the loader's
//! refusals, through `cpd policy show` against a temporary `$CPD_HOME`.

use super::policy_post::{keys, real, show, world, write};
use crate::common::refused;
use serde_json::json;

const NAMES: [&str; 3] = ["in_progress", "in_review", "done"];

#[test]
fn the_jira_status_names_are_empty_by_default() {
    let fx = world("");
    let out = show(&fx, false).ok();
    for name in NAMES {
        assert_eq!(keys(&out)[format!("pr.jira.{name}")], json!({"value": "", "source": "default"}), "{name}: {out}");
    }
}

#[test]
fn an_owner_file_sets_the_jira_status_names_and_a_repo_file_overrides_one() {
    let fx = world("[pr.jira]\nin_progress = \"In Progress\"\nin_review = \"In Review\"\ndone = \"Done\"\n");
    let out = show(&fx, false).ok();
    let owner = format!("owner:{}", real(&fx, "acme/owner.toml"));
    assert_eq!(keys(&out)["pr.jira.in_progress"], json!({"value": "In Progress", "source": owner}), "{out}");
    assert_eq!(keys(&out)["pr.jira.in_review"], json!({"value": "In Review", "source": owner}), "{out}");
    assert_eq!(keys(&out)["pr.jira.done"], json!({"value": "Done", "source": owner}), "{out}");

    write(&fx, "acme/r.toml", "[pr.jira]\nin_review = \"Code Review\"\n");
    let out = show(&fx, true).ok();
    let repo = format!("repo:{}", real(&fx, "acme/r.toml"));
    assert_eq!(keys(&out)["pr.jira.in_review"], json!({"value": "Code Review", "source": repo}), "{out}");
    let left_out = format!("{repo} (key default)");
    assert_eq!(keys(&out)["pr.jira.in_progress"], json!({"value": "", "source": left_out}), "{out}");
    assert_eq!(keys(&out)["pr.jira.done"], json!({"value": "", "source": left_out}), "{out}");
}

#[test]
fn a_machine_file_sets_the_jira_status_names() {
    let fx = world("");
    write(&fx, "host.toml", "[pr.jira]\nin_progress = \"In Progress\"\nin_review = \"In Review\"\ndone = \"Done\"\n");
    let out = show(&fx, false).ok();
    let machine = format!("machine:{}", real(&fx, "host.toml"));
    assert_eq!(keys(&out)["pr.jira.in_progress"], json!({"value": "In Progress", "source": machine}), "{out}");
    assert_eq!(keys(&out)["pr.jira.in_review"], json!({"value": "In Review", "source": machine}), "{out}");
    assert_eq!(keys(&out)["pr.jira.done"], json!({"value": "Done", "source": machine}), "{out}");
}

/// The owner file sets `[pr.jira] <key> = <value>` (TOML text); the refusal names that key.
fn invalid(key: &str, value: &str) {
    let fx = world(&format!("[pr.jira]\n{key} = {value}\n"));
    let error = refused(&show(&fx, false), "policy_invalid", 1);
    assert_eq!(error["details"]["key"], format!("pr.jira.{key}"), "{value}: {error}");
}

#[test]
fn a_jira_status_name_that_is_over_80_characters_padded_or_holds_a_control_character_is_policy_invalid() {
    let long = format!("\"{}\"", "a".repeat(81));
    for key in NAMES {
        invalid(key, &long);
        invalid(key, "\" Done\"");
        invalid(key, "\"Done \"");
        invalid(key, "\"In\\tReview\"");
        invalid(key, "7");
    }
    let edge = "a".repeat(80);
    let fx = world(&format!("[pr.jira]\nin_progress = \"\"\nin_review = \"{edge}\"\n"));
    let out = show(&fx, false).ok();
    assert_eq!(keys(&out)["pr.jira.in_review"]["value"], edge, "{out}");
    assert_eq!(keys(&out)["pr.jira.in_progress"]["value"], "", "{out}");
}
