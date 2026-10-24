//! The clayton-duarte fallback (contract §8.3; commands.md `context new`, `context owner`, `policy show`, `pr add`):
//! an unknown owner is `owner_unconfirmed` until `--confirm-fallback` links its policy file to clayton-duarte's, and a
//! repo with no `origin` is clayton-duarte's. The built binary against a temporary `$CPD_HOME`.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{refused, Run, TestEnv};
use serde_json::Value;

const FALLBACK: &str = "clayton-duarte";

fn env() -> TestEnv {
    let env = TestEnv::new();
    env.write_config("version = 1\nmachine = \"t\"\n");
    fs::write(owners(&env).join("clayton-duarte.toml"), "[tests]\ntext = \"x\"\n").unwrap();
    env
}

fn owners(env: &TestEnv) -> PathBuf {
    env.cpd_home().join("policy/owners")
}

fn link(env: &TestEnv, owner: &str) -> PathBuf {
    owners(env).join(format!("{owner}.toml"))
}

fn new_ctx(env: &TestEnv, id: &str, owner: &str, flag: bool) -> Run {
    let mut args = vec!["context", "new", id, "--title", "T", "--owner", owner];
    if flag {
        args.push("--confirm-fallback");
    }
    env.cpd(&args)
}

fn contexts(env: &TestEnv) -> Value {
    env.cpd(&["context", "list"]).ok()["contexts"].clone()
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false)
}

fn show(env: &TestEnv, owner: &str) -> Run {
    env.cpd(&["policy", "show", "--owner", owner])
}

/// A refusal's code and exit, `details` returned.
fn unconfirmed(run: &Run) -> Value {
    refused(run, "owner_unconfirmed", 1)
}

#[test]
fn f1_new_with_an_unknown_owner_is_owner_unconfirmed_and_writes_nothing() {
    let env = env();
    let error = unconfirmed(&new_ctx(&env, "c1", "stranger", false));
    let d = &error["details"];
    assert_eq!(d["owner"], "stranger", "{error}");
    for key in ["file", "fallback", "flag"] {
        assert!(d[key].is_string(), "details.{key}: {error}");
    }
    assert_eq!(contexts(&env), serde_json::json!([]));
    assert!(fs::symlink_metadata(link(&env, "stranger")).is_err(), "no file was created");
}

#[test]
fn f2_confirm_fallback_links_the_owner_to_clayton_duarte() {
    let env = env();
    new_ctx(&env, "c1", "stranger", true).ok();
    assert_eq!(contexts(&env)[0]["owner"], "stranger");
    let file = link(&env, "stranger");
    assert!(is_symlink(&file));
    assert_eq!(fs::read_link(&file).unwrap(), PathBuf::from("clayton-duarte.toml"));
    let real = fs::canonicalize(link(&env, FALLBACK)).unwrap().display().to_string();
    let shown = show(&env, "stranger").ok();
    assert_eq!(shown["files"].as_array().unwrap().iter().filter(|f| **f == real.as_str()).count(), 1, "{shown}");
    assert_eq!(shown["policy"]["tests"]["source"], format!("owner:{real}"), "{shown}");
}

#[test]
fn f3_confirming_again_is_idempotent() {
    let env = env();
    new_ctx(&env, "c1", "stranger", true).ok();
    let before = fs::read_link(link(&env, "stranger")).unwrap();
    new_ctx(&env, "c2", "stranger", true).ok();
    new_ctx(&env, "c3", "stranger", false).ok();
    assert_eq!(fs::read_link(link(&env, "stranger")).unwrap(), before);
    assert_eq!(contexts(&env).as_array().unwrap().len(), 3);
}

#[test]
fn f4_the_flag_with_a_known_owner_creates_nothing() {
    let env = env();
    new_ctx(&env, "c1", "acme", true).ok();
    assert!(!is_symlink(&link(&env, "acme")));
    assert_eq!(fs::read_to_string(link(&env, "acme")).unwrap(), "");
    let names: Vec<_> = fs::read_dir(owners(&env)).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn f5_without_the_fallbacks_file_the_flag_is_fallback_missing() {
    let env = env();
    fs::remove_file(link(&env, FALLBACK)).unwrap();
    let run = new_ctx(&env, "c1", "stranger", true);
    let error = refused(&run, "fallback_missing", 1);
    assert_eq!(error["details"]["file"], link(&env, FALLBACK).display().to_string(), "{error}");
    assert!(fs::symlink_metadata(link(&env, "stranger")).is_err());
    assert_eq!(contexts(&env), serde_json::json!([]));
}

#[test]
fn f6_context_owner_needs_the_flag_for_an_unknown_owner() {
    let env = env();
    new_ctx(&env, "c0", "acme", false).ok();
    env.sql("UPDATE contexts SET owner = NULL WHERE id = 'c0'");
    let error = unconfirmed(&env.cpd(&["context", "owner", "stranger", "--context", "c0"]));
    assert_eq!(error["details"]["owner"], "stranger");
    assert!(fs::symlink_metadata(link(&env, "stranger")).is_err());
    env.cpd(&["context", "owner", "stranger", "--context", "c0", "--confirm-fallback"]).ok();
    assert_eq!(contexts(&env)[0]["owner"], "stranger");
    assert!(is_symlink(&link(&env, "stranger")));
}

#[test]
fn f6_an_owner_already_set_keeps_its_refusals_first() {
    let env = env();
    new_ctx(&env, "c0", "acme", false).ok();
    // A Context with an owner and a PR row is `owner_in_use`; whatever it is, it is not `owner_unconfirmed`.
    env.sql(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-p', 'lead', 'c0', 'p1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('p1', 'c0', 'P', 'grounding', 'lead-p', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('p1', 'r', '/r', 'main', 'waiting', 'cpd/p1', cpd_txn(), 't');",
    );
    let run = env.cpd(&["context", "owner", "stranger", "--context", "c0", "--confirm-fallback"]);
    refused(&run, "owner_in_use", 3);
    assert!(fs::symlink_metadata(link(&env, "stranger")).is_err(), "no link when refused first");
}

#[test]
fn f7_removing_the_link_revokes_the_owner() {
    let env = env();
    new_ctx(&env, "c1", "stranger", true).ok();
    show(&env, "stranger").ok();
    fs::remove_file(link(&env, "stranger")).unwrap();
    unconfirmed(&show(&env, "stranger"));
}

#[test]
fn f8_a_dangling_symlink_is_unknown_and_the_flag_replaces_it() {
    let env = env();
    let file = link(&env, "x");
    std::os::unix::fs::symlink("missing.toml", &file).unwrap();
    unconfirmed(&new_ctx(&env, "c1", "x", false));
    new_ctx(&env, "c1", "x", true).ok();
    assert_eq!(fs::read_link(&file).unwrap(), PathBuf::from("clayton-duarte.toml"));
    show(&env, "x").ok();
}

#[test]
fn f9_an_invalid_login_with_the_flag_stays_usage_and_creates_nothing() {
    let env = env();
    let before = fs::read_dir(owners(&env)).unwrap().count();
    let run = new_ctx(&env, "c1", "-bad-", true);
    refused(&run, "usage", 2);
    assert_eq!(fs::read_dir(owners(&env)).unwrap().count(), before);
    assert_eq!(contexts(&env), serde_json::json!([]));
}

/// A Plan `pay-1` in Context `ctx` of `owner`, and a git repo `name` with no `origin`.
fn pr_fx(owner: &str, name: &str) -> (TestEnv, PathBuf) {
    let env = env();
    let repo = env.git_repo(name);
    env.git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', '{owner}', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay-1', 'ctx', 'Pay', 'grounding', 'lead1', 't');"
    ));
    (env, repo)
}

fn add(env: &TestEnv, repo: &Path) -> Run {
    env.cpd(&["pr", "add", "api", "--repo", repo.to_str().unwrap(), "--title", "Change api", "--shape", "config", "--plan", "pay-1"])
}

fn repo_file(env: &TestEnv, owner: &str, name: &str) {
    let dir = env.cpd_home().join("policy/repos").join(owner);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{name}.toml")), "").unwrap();
}

#[test]
fn f10_a_repo_with_no_origin_is_clayton_duartes() {
    let (env, repo) = pr_fx(FALLBACK, "pay");
    repo_file(&env, FALLBACK, "pay");
    let out = add(&env, &repo).ok();
    assert_eq!(out["pr"]["name"], "pay", "{out}");
    assert_eq!(env.query::<String>("SELECT name FROM prs WHERE plan = 'pay-1' AND key = 'api'"), "pay");
}

#[test]
fn f10_a_repo_with_no_origin_of_another_owners_context_is_owner_mismatch() {
    let (env, repo) = pr_fx("acme", "pay");
    repo_file(&env, FALLBACK, "pay");
    refused(&add(&env, &repo), "owner_mismatch", 3);
}

#[test]
fn f10_a_directory_that_is_not_a_repo_name_is_repo_unkeyed() {
    let (env, repo) = pr_fx(FALLBACK, "bad name");
    refused(&add(&env, &repo), "repo_unkeyed", 1);
}

#[test]
fn f10_an_origin_that_does_not_parse_is_repo_unkeyed() {
    let (env, repo) = pr_fx(FALLBACK, "pay");
    env.git(&repo, &["remote", "add", "origin", "https://example.com/x"]);
    repo_file(&env, FALLBACK, "pay");
    refused(&add(&env, &repo), "repo_unkeyed", 1);
}

#[test]
fn f11_owner_unknown_is_gone() {
    let env = env();
    unconfirmed(&show(&env, "nobody"));
    new_ctx(&env, "c0", "acme", false).ok();
    env.sql("UPDATE contexts SET owner = NULL WHERE id = 'c0'");
    unconfirmed(&env.cpd(&["context", "owner", "nobody", "--context", "c0"]));

    let (env, repo) = pr_fx("acme", "pay");
    env.git(&repo, &["remote", "add", "origin", "https://github.com/acme/pay"]);
    fs::remove_file(link(&env, "acme")).unwrap();
    unconfirmed(&add(&env, &repo));
}
