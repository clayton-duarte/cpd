//! The repo's policy file as its configuration (contract §8.3, commands.md §22.1): the
//! optional file (a repo without one inherits its owner) and the repo-only domains `hooks`, `test-commands` and `protected`,
//! the command tree, in this process (`Fx::with_ports`), against a temporary `$CPD_HOME`; the repo of `pr add` is a repo of the fake git.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::refused;
use serde_json::{json, Value};

fn fx() -> Fx {
    let fx = Fx::with_ports();
    fs::write(fx.env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    fx
}

fn show(fx: &Fx) -> Value {
    fx.cpd(&["policy", "show", "--owner", "acme", "--repo", "shop"]).ok()
}

fn real(fx: &Fx, rel: &str) -> String {
    fs::canonicalize(fx.env.cpd_home().join("policy").join(rel)).unwrap().display().to_string()
}

#[test]
fn policy_show_of_a_repo_without_a_file_shows_the_owner_policy() {
    let fx = fx();
    fx.env.owner_policy("acme", "[tests]\ntext = \"Owner tests.\"\n");
    let out = show(&fx);
    let policy = &out["policy"];
    assert_eq!(policy["tests"]["source"], format!("owner:{}", real(&fx, "acme/owner.toml")), "{out}");
    assert_eq!(policy["tests"]["text"], "Owner tests.", "{out}");
    assert_eq!(
        policy["hooks"],
        json!({"source": "default", "audience": [], "text": "", "keys": {
            "pre-start": {"value": [], "source": "default"},
            "pre-commit": {"value": [], "source": "default"},
            "pre-merge": {"value": [], "source": "default"}}})
    );
    assert_eq!(
        policy["test-commands"],
        json!({"source": "default", "audience": [], "text": "", "keys": {
            "build": {"value": null, "source": "default"},
            "run": {"value": null, "source": "default"},
            "list": {"value": null, "source": "default"}}})
    );
    assert_eq!(
        policy["protected"],
        json!({"source": "default", "audience": [], "text": "", "keys": {"paths": {"value": [], "source": "default"}}})
    );
    assert_eq!(out["files"], json!([real(&fx, "acme/owner.toml")]), "the owner file and no repo file");
}

#[test]
fn pr_add_of_a_repo_without_a_file_succeeds() {
    let fx = fx();
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Payments', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pl', 'ctx', 'Plan', 'grounding', 'lead1', 't');",
    );
    let repo = fx.keyed_repo("shop");
    let out = fx.cpd(&["workflow", "add", "pay", "--repo", repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pl"]).ok();
    assert_eq!(out["pr"]["name"], "shop", "{out}");
}

#[test]
fn a_repo_without_a_file_shows_the_same_defaults_as_an_empty_file() {
    let fx = fx();
    let without = show(&fx);
    fx.env.repo_policy("acme", "shop", "");
    let empty = show(&fx);
    assert_eq!(without["policy"], empty["policy"]);
}

#[test]
fn an_empty_repo_file_is_valid_and_shows_the_default_hooks_test_commands_and_protected() {
    let fx = fx();
    fx.env.repo_policy("acme", "shop", "");
    let out = show(&fx);
    let policy = &out["policy"];
    assert_eq!(
        policy["hooks"],
        json!({"source": "default", "audience": [], "text": "", "keys": {
            "pre-start": {"value": [], "source": "default"},
            "pre-commit": {"value": [], "source": "default"},
            "pre-merge": {"value": [], "source": "default"}}})
    );
    assert_eq!(
        policy["test-commands"],
        json!({"source": "default", "audience": [], "text": "", "keys": {
            "build": {"value": null, "source": "default"},
            "run": {"value": null, "source": "default"},
            "list": {"value": null, "source": "default"}}})
    );
    assert_eq!(
        policy["protected"],
        json!({"source": "default", "audience": [], "text": "", "keys": {"paths": {"value": [], "source": "default"}}})
    );
}

#[test]
fn policy_show_prints_the_hooks_test_commands_and_protected_of_the_repo_file() {
    let fx = fx();
    fx.env.repo_policy(
        "acme",
        "shop",
        "[hooks]\npre-start = \"npm ci\"\npre-commit = \"npm run typecheck || exit 64\"\npre-merge = \"npm test\"\n\n\
         [test-commands]\nbuild = \"make build\"\nrun = \"make test {{ test }}\"\nlist = \"make list {{ test }}\"\n\n\
         [protected]\npaths = [\"verify.sh\", \"scripts/ci/\"]\n",
    );
    let out = show(&fx);
    let src = format!("repo:{}", real(&fx, "acme/shop.toml"));
    let policy = &out["policy"];
    assert_eq!(
        policy["hooks"],
        json!({"source": src, "audience": [], "text": "", "keys": {
            "pre-start": {"value": [[{"name": null, "command": "npm ci"}]], "source": src},
            "pre-commit": {"value": [[{"name": null, "command": "npm run typecheck || exit 64"}]], "source": src},
            "pre-merge": {"value": [[{"name": null, "command": "npm test"}]], "source": src}}})
    );
    assert_eq!(
        policy["test-commands"],
        json!({"source": src, "audience": [], "text": "", "keys": {
            "build": {"value": "make build", "source": src},
            "run": {"value": "make test {{ test }}", "source": src},
            "list": {"value": "make list {{ test }}", "source": src}}})
    );
    assert_eq!(
        policy["protected"],
        json!({"source": src, "audience": [], "text": "", "keys": {"paths": {"value": ["verify.sh", "scripts/ci/"], "source": src}}})
    );
}

#[test]
fn a_hook_shows_as_blocks_of_commands_in_file_order_in_each_of_the_three_forms() {
    let fx = fx();
    fx.env.repo_policy(
        "acme",
        "shop",
        "[hooks]\npre-start = \"npm ci\"\n\n\
         [hooks.pre-commit]\nzulu = \"echo z\"\nalpha = \"echo a\"\n\n\
         [[hooks.pre-merge]]\nfirst = \"echo 1\"\n\n\
         [[hooks.pre-merge]]\nsecond = \"echo 2\"\n",
    );
    let out = show(&fx);
    let keys = &out["policy"]["hooks"]["keys"];
    assert_eq!(keys["pre-start"]["value"], json!([[{"name": null, "command": "npm ci"}]]));
    assert_eq!(keys["pre-commit"]["value"], json!([[{"name": "zulu", "command": "echo z"}, {"name": "alpha", "command": "echo a"}]]));
    assert_eq!(
        keys["pre-merge"]["value"],
        json!([[{"name": "first", "command": "echo 1"}], [{"name": "second", "command": "echo 2"}]])
    );
}

#[test]
fn hooks_test_commands_and_protected_in_an_owner_file_are_policy_invalid() {
    let fx = fx();
    fx.env.repo_policy("acme", "shop", "");
    let tables = [
        ("hooks", "[hooks]\npre-start = \"npm ci\"\n"),
        ("test-commands", "[test-commands]\nbuild = \"b\"\nrun = \"r\"\n"),
        ("protected", "[protected]\npaths = [\"verify.sh\"]\n"),
    ];
    for (domain, toml) in tables {
        fx.env.owner_policy("acme", toml);
        let run = fx.cpd(&["policy", "show", "--owner", "acme"]);
        let error = refused(&run, "policy_invalid", 1);
        assert!(error["details"]["file"].as_str().unwrap().ends_with("policy/acme/owner.toml"), "{domain}: {run:?}");
        assert_eq!(error["details"]["key"], domain, "{run:?}");
        assert!(error["message"].as_str().unwrap().contains("exposed to repo only"), "{domain}: {run:?}");
    }
}

#[test]
fn an_invalid_hooks_or_test_commands_table_is_policy_invalid() {
    let fx = fx();
    // The same tables, valid, are accepted: only their invalid forms are refused.
    fx.env.repo_policy("acme", "shop", "[hooks]\npre-merge = \"x\"\n\n[test-commands]\nbuild = \"b\"\nrun = \"r\"\nlist = \"l {{ test }}\"\n\n[protected]\npaths = []\n");
    show(&fx);
    let cases = [
        ("hooks", "[hooks]\npre-merge = 3\n"),
        ("test-commands", "[test-commands]\nbuild = \"b\"\n"),
        ("test-commands", "[test-commands]\nbuild = \"\"\nrun = \"r\"\n"),
        ("test-commands", "[test-commands]\nbuild = \"b\"\nrun = \"r\"\nlist = \"l\"\n"),
        ("hooks", "[hooks]\npost-merge = \"x\"\n"),
        ("test-commands", "[test-commands]\nbuild = \"b\"\nrun = \"r\"\nother = \"x\"\n"),
        ("protected", "[protected]\npaths = []\nother = 1\n"),
    ];
    for (domain, toml) in cases {
        fx.env.repo_policy("acme", "shop", toml);
        let run = fx.cpd(&["policy", "show", "--owner", "acme", "--repo", "shop"]);
        let error = refused(&run, "policy_invalid", 1);
        assert!(error["details"]["file"].as_str().unwrap().ends_with("policy/acme/shop.toml"), "{toml}: {run:?}");
        assert!(error["details"]["key"].as_str().unwrap().starts_with(domain), "{toml}: {run:?}");
    }
}
