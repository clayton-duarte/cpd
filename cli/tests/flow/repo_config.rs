//! The repo's policy file as its configuration (contract §8.3, commands.md §22.1): the
//! optional file (a repo without one inherits its owner) and the repo-only domains `hooks`, `test-commands` and `protected`,
//! through the built binary against a temporary `$CPD_HOME`.

use crate::common;

use std::fs;

use common::{refused, TestEnv};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::{json, Value};

fn env() -> TestEnv {
    let env = TestEnv::new();
    fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    env
}

fn show(env: &TestEnv) -> Value {
    env.cpd(&["policy", "show", "--owner", "acme", "--repo", "shop"]).ok()
}

fn real(env: &TestEnv, rel: &str) -> String {
    fs::canonicalize(env.cpd_home().join("policy").join(rel)).unwrap().display().to_string()
}

#[test]
fn policy_show_of_a_repo_without_a_file_shows_the_owner_policy() {
    let env = env();
    env.owner_policy("acme", "[tests]\ntext = \"Owner tests.\"\n");
    let out = show(&env);
    let policy = &out["policy"];
    assert_eq!(policy["tests"]["source"], format!("owner:{}", real(&env, "owners/acme.toml")), "{out}");
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
    assert_eq!(out["files"], json!([real(&env, "owners/acme.toml")]), "the owner file and no repo file");
}

#[test]
fn pr_add_of_a_repo_without_a_file_succeeds() {
    let env = env();
    env.write_tmux_config("");
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    env.cpd(&["plan", "new", "pl", "--context", "ctx", "--title", "Plan"]).ok();
    let repo = env.unconfigured_repo("shop");
    let out = env.cpd(&["pr", "add", "pay", "--repo", repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pl"]).ok();
    assert_eq!(out["pr"]["name"], "shop", "{out}");
}

#[test]
fn a_repo_without_a_file_shows_the_same_defaults_as_an_empty_file() {
    let env = env();
    let without = show(&env);
    env.repo_policy("acme", "shop", "");
    let empty = show(&env);
    assert_eq!(without["policy"], empty["policy"]);
}

#[test]
fn lock_starts_a_pr_of_a_repo_without_a_file() {
    let env = env();
    env.write_tmux_config("");
    env.fake_cpd_panes(common::WATCH_OK);
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    let repo = env.keyed_repo("pay");
    env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Plan pay"]).ok();
    env.cpd(&["pr", "add", "pay", "--repo", repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pay"]).ok();
    env.cpd(&["job", "cancel", "pay-config", "--reason", "a stub of the template", "--plan", "pay"]).ok();
    env.drop_unreachable_origin(&repo);
    env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    let mut store = Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, created_txn, created_at)
                 VALUES ('pay', (SELECT key FROM prs WHERE plan = 'pay'), 'a', 'Job a', 'S2', 'Goal of a.', 'Context of a.', '[\"src/a.rs\"]',
                         '[\"cargo test\"]', '[]', 'draft', 0, {id}, 't');",
                id = tx.id()
            ))?;
            Ok(())
        })
        .unwrap();
    env.outline_for_review("pay");
    env.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]).ok();
    env.cpd(&["plan", "decide", "r1", "--approve", "--plan", "pay"]).ok();

    // The repo has no policy file at the lock.
    fs::remove_file(env.cpd_home().join("policy/repos/acme/pay.toml")).unwrap();
    let out = env.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    let pr = &out["prs"][0];
    assert_eq!((&pr["key"], &pr["status"]), (&json!("pay"), &json!("dispatch")), "{out}");
    let worktree = pr["lead_worktree"].as_str().unwrap_or_else(|| panic!("no Lead worktree: {out}"));
    assert!(std::path::Path::new(worktree).is_dir(), "{out}");
    let events = env.cpd(&["events", "list", "--kind", "error"]).ok();
    let failed: Vec<&Value> = events["events"].as_array().unwrap().iter().filter(|e| e.to_string().contains("pr_start_failed")).collect();
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
fn an_empty_repo_file_is_valid_and_shows_the_default_hooks_test_commands_and_protected() {
    let env = env();
    env.repo_policy("acme", "shop", "");
    let out = show(&env);
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
    let env = env();
    env.repo_policy(
        "acme",
        "shop",
        "[hooks]\npre-start = \"npm ci\"\npre-commit = \"npm run typecheck || exit 64\"\npre-merge = \"npm test\"\n\n\
         [test-commands]\nbuild = \"make build\"\nrun = \"make test {{ test }}\"\nlist = \"make list {{ test }}\"\n\n\
         [protected]\npaths = [\"verify.sh\", \"scripts/ci/\"]\n",
    );
    let out = show(&env);
    let src = format!("repo:{}", real(&env, "repos/acme/shop.toml"));
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
    let env = env();
    env.repo_policy(
        "acme",
        "shop",
        "[hooks]\npre-start = \"npm ci\"\n\n\
         [hooks.pre-commit]\nzulu = \"echo z\"\nalpha = \"echo a\"\n\n\
         [[hooks.pre-merge]]\nfirst = \"echo 1\"\n\n\
         [[hooks.pre-merge]]\nsecond = \"echo 2\"\n",
    );
    let out = show(&env);
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
    let env = env();
    env.repo_policy("acme", "shop", "");
    let tables = [
        ("hooks", "[hooks]\npre-start = \"npm ci\"\n"),
        ("test-commands", "[test-commands]\nbuild = \"b\"\nrun = \"r\"\n"),
        ("protected", "[protected]\npaths = [\"verify.sh\"]\n"),
    ];
    for (domain, toml) in tables {
        env.owner_policy("acme", toml);
        let run = env.cpd(&["policy", "show", "--owner", "acme"]);
        let error = refused(&run, "policy_invalid", 1);
        assert!(error["details"]["file"].as_str().unwrap().ends_with("policy/owners/acme.toml"), "{domain}: {run:?}");
        assert_eq!(error["details"]["key"], domain, "{run:?}");
        assert!(error["message"].as_str().unwrap().contains("exposed to repo only"), "{domain}: {run:?}");
    }
}

#[test]
fn an_invalid_hooks_or_test_commands_table_is_policy_invalid() {
    let env = env();
    // The same tables, valid, are accepted: only their invalid forms are refused.
    env.repo_policy("acme", "shop", "[hooks]\npre-merge = \"x\"\n\n[test-commands]\nbuild = \"b\"\nrun = \"r\"\nlist = \"l {{ test }}\"\n\n[protected]\npaths = []\n");
    show(&env);
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
        env.repo_policy("acme", "shop", toml);
        let run = env.cpd(&["policy", "show", "--owner", "acme", "--repo", "shop"]);
        let error = refused(&run, "policy_invalid", 1);
        assert!(error["details"]["file"].as_str().unwrap().ends_with("policy/repos/acme/shop.toml"), "{toml}: {run:?}");
        assert!(error["details"]["key"].as_str().unwrap().starts_with(domain), "{toml}: {run:?}");
    }
}
