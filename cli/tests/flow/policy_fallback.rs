//! The clayton-duarte fallback (contract §8.3; commands.md `context new`, `context owner`, `policy show`, `pr add`):
//! an unknown owner is `owner_unconfirmed` until `--confirm-fallback` links its policy file to clayton-duarte's, and a
//! repo with no `origin` is clayton-duarte's. Run in-process (`Fx::with_ports`) against a temporary `$CPD_HOME`; the repos are repos of the fake git.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::fx::Fx;
use common::{Run, refused};
use serde_json::Value;

const FALLBACK: &str = "clayton-duarte";

fn base() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_config("version = 1\nmachine = \"t\"\n");
    fs::create_dir_all(link(&fx, FALLBACK).parent().unwrap()).unwrap();
    fs::write(link(&fx, FALLBACK), "[tests]\ntext = \"x\"\n").unwrap();
    fx
}

fn owners(fx: &Fx) -> PathBuf {
    fx.env.cpd_home().join("policy")
}

fn link(fx: &Fx, owner: &str) -> PathBuf {
    owners(fx).join(owner).join("owner.toml")
}

fn new_ctx(fx: &Fx, id: &str, owner: &str, flag: bool) -> Run {
    let mut args = vec!["context", "new", id, "--title", "T", "--owner", owner];
    if flag {
        args.push("--confirm-fallback");
    }
    fx.cpd(&args)
}

fn contexts(fx: &Fx) -> Value {
    fx.cpd(&["context", "list"]).ok()["contexts"].clone()
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

fn show(fx: &Fx, owner: &str) -> Run {
    fx.cpd(&["policy", "show", "--owner", owner])
}

/// A refusal's code and exit, `details` returned.
fn unconfirmed(run: &Run) -> Value {
    refused(run, "owner_unconfirmed", 1)
}

#[test]
fn f1_new_with_an_unknown_owner_is_owner_unconfirmed_and_writes_nothing() {
    let fx = base();
    let error = unconfirmed(&new_ctx(&fx, "c1", "stranger", false));
    let d = &error["details"];
    assert_eq!(d["owner"], "stranger", "{error}");
    for key in ["file", "fallback", "flag"] {
        assert!(d[key].is_string(), "details.{key}: {error}");
    }
    assert_eq!(contexts(&fx), serde_json::json!([]));
    assert!(
        fs::symlink_metadata(link(&fx, "stranger")).is_err(),
        "no file was created"
    );
}

#[test]
fn f2_confirm_fallback_links_the_owner_to_clayton_duarte() {
    let fx = base();
    new_ctx(&fx, "c1", "stranger", true).ok();
    assert_eq!(contexts(&fx)[0]["owner"], "stranger");
    let file = link(&fx, "stranger");
    assert!(is_symlink(&file));
    assert_eq!(
        fs::read_link(&file).unwrap(),
        PathBuf::from("../clayton-duarte/owner.toml")
    );
    let real = fs::canonicalize(link(&fx, FALLBACK))
        .unwrap()
        .display()
        .to_string();
    let shown = show(&fx, "stranger").ok();
    assert_eq!(
        shown["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| **f == real.as_str())
            .count(),
        1,
        "{shown}"
    );
    assert_eq!(
        shown["policy"]["tests"]["source"],
        format!("owner:{real}"),
        "{shown}"
    );
}

#[test]
fn f3_confirming_again_is_idempotent() {
    let fx = base();
    new_ctx(&fx, "c1", "stranger", true).ok();
    let before = fs::read_link(link(&fx, "stranger")).unwrap();
    new_ctx(&fx, "c2", "stranger", true).ok();
    new_ctx(&fx, "c3", "stranger", false).ok();
    assert_eq!(fs::read_link(link(&fx, "stranger")).unwrap(), before);
    assert_eq!(contexts(&fx).as_array().unwrap().len(), 3);
}

#[test]
fn f4_the_flag_with_a_known_owner_creates_nothing() {
    let fx = base();
    new_ctx(&fx, "c1", "acme", true).ok();
    assert!(!is_symlink(&link(&fx, "acme")));
    assert_eq!(fs::read_to_string(link(&fx, "acme")).unwrap(), "");
    let names: Vec<_> = fs::read_dir(owners(&fx))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn f5_without_the_fallbacks_file_the_flag_is_fallback_missing() {
    let fx = base();
    fs::remove_file(link(&fx, FALLBACK)).unwrap();
    let run = new_ctx(&fx, "c1", "stranger", true);
    let error = refused(&run, "fallback_missing", 1);
    assert_eq!(
        error["details"]["file"],
        link(&fx, FALLBACK).display().to_string(),
        "{error}"
    );
    assert!(fs::symlink_metadata(link(&fx, "stranger")).is_err());
    assert_eq!(contexts(&fx), serde_json::json!([]));
}

#[test]
fn f6_context_owner_needs_the_flag_for_an_unknown_owner() {
    let fx = base();
    new_ctx(&fx, "c0", "acme", false).ok();
    fx.env
        .sql("UPDATE contexts SET owner = NULL WHERE id = 'c0'");
    let error = unconfirmed(&fx.cpd(&["context", "owner", "stranger", "--context", "c0"]));
    assert_eq!(error["details"]["owner"], "stranger");
    assert!(fs::symlink_metadata(link(&fx, "stranger")).is_err());
    fx.cpd(&[
        "context",
        "owner",
        "stranger",
        "--context",
        "c0",
        "--confirm-fallback",
    ])
    .ok();
    assert_eq!(contexts(&fx)[0]["owner"], "stranger");
    assert!(is_symlink(&link(&fx, "stranger")));
}

#[test]
fn f6_an_owner_already_set_keeps_its_refusals_first() {
    let fx = base();
    new_ctx(&fx, "c0", "acme", false).ok();
    // A Context with an owner and a PR row is `owner_in_use`; whatever it is, it is not `owner_unconfirmed`.
    fx.env.sql(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-p', 'lead', 'c0', 'p1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('p1', 'c0', 'P', 'grounding', 'lead-p', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('p1', 'r', '/r', 'main', 'waiting', 'cpd/p1', cpd_txn(), 't');",
    );
    let run = fx.cpd(&[
        "context",
        "owner",
        "stranger",
        "--context",
        "c0",
        "--confirm-fallback",
    ]);
    refused(&run, "owner_in_use", 3);
    assert!(
        fs::symlink_metadata(link(&fx, "stranger")).is_err(),
        "no link when refused first"
    );
}

#[test]
fn f7_removing_the_link_revokes_the_owner() {
    let fx = base();
    new_ctx(&fx, "c1", "stranger", true).ok();
    show(&fx, "stranger").ok();
    fs::remove_file(link(&fx, "stranger")).unwrap();
    unconfirmed(&show(&fx, "stranger"));
}

#[test]
fn f8_a_dangling_symlink_is_unknown_and_the_flag_replaces_it() {
    let fx = base();
    let file = link(&fx, "x");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("../missing/owner.toml", &file).unwrap();
    unconfirmed(&new_ctx(&fx, "c1", "x", false));
    new_ctx(&fx, "c1", "x", true).ok();
    assert_eq!(
        fs::read_link(&file).unwrap(),
        PathBuf::from("../clayton-duarte/owner.toml")
    );
    show(&fx, "x").ok();
}

#[test]
fn f9_an_invalid_login_with_the_flag_stays_usage_and_creates_nothing() {
    let fx = base();
    let before = fs::read_dir(owners(&fx)).unwrap().count();
    let run = new_ctx(&fx, "c1", "-bad-", true);
    refused(&run, "usage", 2);
    assert_eq!(fs::read_dir(owners(&fx)).unwrap().count(), before);
    assert_eq!(contexts(&fx), serde_json::json!([]));
}

/// A Plan `pay-1` in Context `ctx` of `owner`, and a git repo `name` with no `origin`.
fn pr_fx(owner: &str, name: &str) -> (Fx, PathBuf) {
    let fx = base();
    let repo = fx.git_repo(name);
    fx.git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', '{owner}', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay-1', 'ctx', 'Pay', 'grounding', 'lead1', 't');"
    ));
    (fx, repo)
}

fn add(fx: &Fx, repo: &Path) -> Run {
    fx.cpd(&[
        "workflow",
        "add",
        "api",
        "--repo",
        repo.to_str().unwrap(),
        "--title",
        "Change api",
        "--shape",
        "config",
        "--plan",
        "pay-1",
    ])
}

fn repo_file(fx: &Fx, owner: &str, name: &str) {
    let dir = fx.env.cpd_home().join("policy").join(owner);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{name}.toml")), "").unwrap();
}

#[test]
fn f10_a_repo_with_no_origin_is_clayton_duartes() {
    let (fx, repo) = pr_fx(FALLBACK, "pay");
    repo_file(&fx, FALLBACK, "pay");
    let out = add(&fx, &repo).ok();
    assert_eq!(out["workflow"]["name"], "pay", "{out}");
    assert_eq!(
        fx.env
            .query::<String>("SELECT name FROM workflows WHERE plan = 'pay-1' AND key = 'api'"),
        "pay"
    );
}

#[test]
fn f10_a_repo_with_no_origin_of_another_owners_context_is_owner_mismatch() {
    let (fx, repo) = pr_fx("acme", "pay");
    repo_file(&fx, FALLBACK, "pay");
    refused(&add(&fx, &repo), "owner_mismatch", 3);
}

#[test]
fn f10_a_directory_that_is_not_a_repo_name_is_repo_unkeyed() {
    let (fx, repo) = pr_fx(FALLBACK, "bad name");
    refused(&add(&fx, &repo), "repo_unkeyed", 1);
}

#[test]
fn f10_an_origin_that_does_not_parse_is_repo_unkeyed() {
    let (fx, repo) = pr_fx(FALLBACK, "pay");
    fx.git(&repo, &["remote", "add", "origin", "https://example.com/x"]);
    repo_file(&fx, FALLBACK, "pay");
    refused(&add(&fx, &repo), "repo_unkeyed", 1);
}

#[test]
fn f11_owner_unconfirmed_replaces_the_old_refusal() {
    {
        // One `Fx` at a time (the process-wide lock): this one is dropped before the next.
        let fx = base();
        unconfirmed(&show(&fx, "nobody"));
        new_ctx(&fx, "c0", "acme", false).ok();
        fx.env
            .sql("UPDATE contexts SET owner = NULL WHERE id = 'c0'");
        unconfirmed(&fx.cpd(&["context", "owner", "nobody", "--context", "c0"]));
    }

    let (fx, repo) = pr_fx("acme", "pay");
    fx.git(
        &repo,
        &["remote", "add", "origin", "https://github.com/acme/pay"],
    );
    fs::remove_file(link(&fx, "acme")).unwrap();
    unconfirmed(&add(&fx, &repo));
}
