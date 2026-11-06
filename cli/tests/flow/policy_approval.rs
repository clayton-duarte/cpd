//! The `pr.feedback.reply` and `pr.post.require_approval` policy keys, through `cpd policy show`
//! against a temporary `$CPD_HOME`.

use crate::common::refused;
use serde_json::json;

use super::policy_post::{keys, real, show, world, write};

const BOTH: &str = "[pr.feedback]\nreply = \"bots_only\"\n[pr.post]\nenabled = true\nchannel = \"C02JU6C2Z1D\"\nrequire_approval = true\n";

#[test]
fn the_two_keys_default_to_all_and_false() {
    let fx = world("");
    let out = show(&fx, false).ok();
    let k = keys(&out);
    assert_eq!(
        k["pr.feedback.reply"],
        json!({"value": "all", "source": "default"}),
        "pr.feedback.reply: {out}"
    );
    assert_eq!(
        k["pr.post.require_approval"],
        json!({"value": false, "source": "default"}),
        "pr.post.require_approval: {out}"
    );
}

#[test]
fn an_owner_sets_both_keys() {
    let fx = world(BOTH);
    let out = show(&fx, false).ok();
    let owner = format!("owner:{}", real(&fx, "acme/owner.toml"));
    let k = keys(&out);
    assert_eq!(
        k["pr.feedback.reply"],
        json!({"value": "bots_only", "source": owner}),
        "pr.feedback.reply: {out}"
    );
    assert_eq!(
        k["pr.post.require_approval"],
        json!({"value": true, "source": owner}),
        "pr.post.require_approval: {out}"
    );
}

#[test]
fn a_repo_file_overrides_the_owner() {
    let fx = world(BOTH);
    write(
        &fx,
        "acme/r.toml",
        "[pr.feedback]\nreply = \"all\"\n[pr.post]\nrequire_approval = false\n",
    );
    let out = show(&fx, true).ok();
    let repo = format!("repo:{}", real(&fx, "acme/r.toml"));
    let k = keys(&out);
    assert_eq!(
        k["pr.feedback.reply"],
        json!({"value": "all", "source": repo}),
        "pr.feedback.reply: {out}"
    );
    assert_eq!(
        k["pr.post.require_approval"],
        json!({"value": false, "source": repo}),
        "pr.post.require_approval: {out}"
    );
}

#[test]
fn an_unknown_reply_value_is_policy_invalid() {
    for good in ["all", "bots_only"] {
        let fx = world(&format!("[pr.feedback]\nreply = \"{good}\"\n"));
        let out = show(&fx, false).ok();
        assert_eq!(
            keys(&out)["pr.feedback.reply"]["value"],
            good,
            "pr.feedback.reply: {out}"
        );
    }
    for bad in ["botsonly", "none"] {
        let fx = world(&format!("[pr.feedback]\nreply = \"{bad}\"\n"));
        let error = refused(&show(&fx, false), "policy_invalid", 1);
        assert_eq!(
            error["details"]["key"], "pr.feedback.reply",
            "{bad}: {error}"
        );
    }
}

#[test]
fn a_non_boolean_require_approval_is_policy_invalid() {
    let ok = world("[pr.post]\nrequire_approval = true\n");
    let out = show(&ok, false).ok();
    assert_eq!(
        keys(&out)["pr.post.require_approval"]["value"],
        true,
        "pr.post.require_approval: {out}"
    );
    let fx = world("[pr.post]\nrequire_approval = \"yes\"\n");
    let error = refused(&show(&fx, false), "policy_invalid", 1);
    assert_eq!(
        error["details"]["key"], "pr.post.require_approval",
        "{error}"
    );
}
