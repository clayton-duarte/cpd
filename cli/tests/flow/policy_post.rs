//! The `pr.post` policy keys (contract §8.3): defaults, scopes and the loader's refusals, through
//! `cpd policy show` against a temporary `$CPD_HOME`.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::{refused, Run};
use serde_json::{json, Value};

const ALL: &str = "[pr.post]\nenabled = true\nchannel = \"C02JU6C2Z1D\"\ntemplate = \":git-pr-open: {{ title }}:\\n{{ url }}\"\nmerged_reaction = \"git-merge\"\n";

fn world(owner: &str) -> Fx {
    let fx = Fx::new();
    fx.env.write_config("version = 1\nmachine = \"pro\"\n");
    write(&fx, "acme/owner.toml", owner);
    write(&fx, "acme/r.toml", "");
    fx
}

fn write(fx: &Fx, rel: &str, text: &str) {
    let path = fx.env.cpd_home().join("policy").join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn real(fx: &Fx, rel: &str) -> String {
    fs::canonicalize(fx.env.cpd_home().join("policy").join(rel)).unwrap().display().to_string()
}

fn show(fx: &Fx, repo: bool) -> Run {
    let mut args = vec!["policy", "show", "--owner", "acme"];
    if repo {
        args.extend(["--repo", "r"]);
    }
    fx.cpd(&args)
}

/// The owner file holds `[pr.post]` with one `key = value` line; the refusal names that key.
fn invalid(key: &str, value: &str) {
    let fx = world(&format!("[pr.post]\n{key} = {value}\n"));
    let error = refused(&show(&fx, false), "policy_invalid", 1);
    assert_eq!(error["details"]["key"], format!("pr.post.{key}"), "{value}: {error}");
}

fn keys(out: &Value) -> &Value {
    &out["policy"]["pr"]["keys"]
}

#[test]
fn the_default_post_is_disabled_with_no_channel_and_the_title_and_url_template() {
    let fx = world("");
    let out = show(&fx, false).ok();
    let k = keys(&out);
    assert_eq!(k["pr.post.enabled"], json!({"value": false, "source": "default"}), "{out}");
    assert_eq!(k["pr.post.channel"], json!({"value": "", "source": "default"}), "{out}");
    assert_eq!(k["pr.post.template"], json!({"value": "{{ title }}\n{{ url }}", "source": "default"}), "{out}");
    assert_eq!(k["pr.post.merged_reaction"], json!({"value": "", "source": "default"}), "{out}");
}

#[test]
fn an_owner_sets_every_post_key() {
    let fx = world(ALL);
    let out = show(&fx, false).ok();
    let owner = format!("owner:{}", real(&fx, "acme/owner.toml"));
    let k = keys(&out);
    assert_eq!(k["pr.post.enabled"], json!({"value": true, "source": owner}), "{out}");
    assert_eq!(k["pr.post.channel"], json!({"value": "C02JU6C2Z1D", "source": owner}), "{out}");
    assert_eq!(k["pr.post.template"], json!({"value": ":git-pr-open: {{ title }}:\n{{ url }}", "source": owner}), "{out}");
    assert_eq!(k["pr.post.merged_reaction"], json!({"value": "git-merge", "source": owner}), "{out}");
}

#[test]
fn a_left_out_post_key_takes_cpds_default_not_the_owners() {
    let fx = world(ALL);
    write(&fx, "acme/r.toml", "[pr.post]\ntemplate = \"{{ url }} {{ title }}\"\n");
    let out = show(&fx, true).ok();
    let repo = format!("repo:{}", real(&fx, "acme/r.toml"));
    let dflt = format!("{repo} (key default)");
    let k = keys(&out);
    assert_eq!(k["pr.post.template"], json!({"value": "{{ url }} {{ title }}", "source": repo}), "{out}");
    assert_eq!(k["pr.post.enabled"], json!({"value": false, "source": dflt}), "{out}");
    assert_eq!(k["pr.post.channel"], json!({"value": "", "source": dflt}), "{out}");
    assert_eq!(k["pr.post.merged_reaction"], json!({"value": "", "source": dflt}), "{out}");
}

#[test]
fn a_channel_that_is_not_a_slack_id_is_policy_invalid() {
    invalid("channel", "\"c02ju6c2z1d\"");
    invalid("channel", "\"general\"");
}

#[test]
fn a_template_without_url_is_policy_invalid() {
    invalid("template", "\"{{ title }}\"");
}

#[test]
fn a_template_with_an_unknown_variable_is_policy_invalid() {
    invalid("template", "\"{{ title }} {{ author }} {{ url }}\"");
}

#[test]
fn a_template_that_does_not_parse_is_policy_invalid() {
    invalid("template", "\"{{ title\"");
}

#[test]
fn a_template_over_1000_characters_is_policy_invalid() {
    invalid("template", &format!("\"{{{{ url }}}}{}\"", "x".repeat(1000)));
}

#[test]
fn a_reaction_that_is_not_an_emoji_name_is_policy_invalid() {
    for bad in [":git-merge:".to_string(), "Git-Merge".into(), "git merge".into(), "a".repeat(101)] {
        invalid("merged_reaction", &format!("\"{bad}\""));
    }
    for good in ["git-merge", "+1"] {
        let fx = world(&format!("[pr.post]\nmerged_reaction = \"{good}\"\n"));
        let out = show(&fx, false).ok();
        assert_eq!(keys(&out)["pr.post.merged_reaction"]["value"], good, "{out}");
    }
}

#[test]
fn enabled_that_is_not_a_boolean_is_policy_invalid() {
    invalid("enabled", "\"yes\"");
}

#[test]
fn a_machine_file_with_a_post_table_is_policy_invalid() {
    let fx = world("");
    write(&fx, "host.toml", "[pr.post]\nenabled = true\n");
    let error = refused(&show(&fx, false), "policy_invalid", 1);
    assert_eq!(error["details"]["key"], "pr.post", "{error}");
    assert_eq!(error["details"]["file"], real(&fx, "host.toml"), "{error}");
}
