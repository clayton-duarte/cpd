//! The one policy tree (contract §1.4, §8.2, §8.3, §2.1; commands.md `policy show`, `context new`, `pr add`):
//! `policy/host.toml`, `policy/<owner>/owner.toml`, `policy/<owner>/<repo>.toml`; no `host` key, no repo named
//! `owner`, every entry of `policy/` checked. The built binary against a temporary `$CPD_HOME`.

use crate::common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use common::fx::Fx;
use common::{Run, refused};
use serde_json::Value;

struct Layout {
    fx: Fx,
}

impl Layout {
    /// An env whose `policy/` is empty (the harness's `acme/owner.toml` is removed).
    fn bare() -> Layout {
        let fx = Fx::with_ports();
        fx.env.write_config("version = 1\nmachine = \"pro\"\n");
        fs::remove_dir_all(fx.env.cpd_home().join("policy")).unwrap();
        fs::create_dir_all(fx.env.cpd_home().join("policy")).unwrap();
        Layout { fx }
    }

    /// `acme/owner.toml` and `acme/r.toml`, both empty.
    fn new() -> Layout {
        let fx = Layout::bare();
        fx.write("acme/owner.toml", "");
        fx.write("acme/r.toml", "");
        fx
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.fx.env.cpd_home().join("policy").join(rel)
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn real(&self, rel: &str) -> String {
        fs::canonicalize(self.path(rel))
            .unwrap()
            .display()
            .to_string()
    }

    fn show(&self, args: &[&str]) -> Run {
        let mut all = vec!["policy", "show"];
        all.extend(args);
        self.fx.cpd(&all)
    }

    fn owner(&self) -> Value {
        self.show(&["--owner", "acme"]).ok()
    }

    fn repo(&self) -> Value {
        self.show(&["--owner", "acme", "--repo", "r"]).ok()
    }
}

fn tests_of(out: &Value) -> &Value {
    &out["policy"]["tests"]
}

// ------------------------------------------------------------------ l1: machine scope

#[test]
fn l1_host_toml_is_the_machine_scope() {
    let fx = Layout::new();
    fx.write("host.toml", "[tests]\ntext = \"Machine tests.\"\n");
    let out = fx.owner();
    assert_eq!(
        tests_of(&out)["source"],
        format!("machine:{}", fx.real("host.toml")),
        "{out}"
    );
    assert_eq!(tests_of(&out)["text"], "Machine tests.", "{out}");
}

#[test]
fn l1_without_host_toml_the_domain_is_the_default() {
    let fx = Layout::new();
    assert_eq!(tests_of(&fx.owner())["source"], "default");
}

#[test]
fn l1_machines_directory_is_not_read() {
    let fx = Layout::new();
    fx.write("machines/x.toml", "[tests]\ntext = \"Old.\"\n");
    // The old layout is refused whole (l8), never read.
    refused(&fx.show(&["--owner", "acme"]), "policy_invalid", 1);
}

// ------------------------------------------------------------------ l2: owner scope

#[test]
fn l2_owner_toml_is_the_owner_file() {
    let fx = Layout::new();
    fx.write("acme/owner.toml", "[tests]\ntext = \"Owner tests.\"\n");
    let out = fx.owner();
    assert_eq!(
        tests_of(&out)["source"],
        format!("owner:{}", fx.real("acme/owner.toml")),
        "{out}"
    );
    assert_eq!(tests_of(&out)["text"], "Owner tests.", "{out}");
}

#[test]
fn l2_the_old_owners_file_is_not_read() {
    let fx = Layout::bare();
    fx.write("owners/acme.toml", "");
    // acme is not known by `owners/acme.toml`: with the old layout out of the way it is unconfirmed.
    fs::remove_dir_all(fx.path("owners")).unwrap();
    let error = refused(&fx.show(&["--owner", "acme"]), "owner_unconfirmed", 1);
    assert_eq!(
        error["details"]["file"],
        fx.path("acme/owner.toml").display().to_string(),
        "{error}"
    );
}

// ------------------------------------------------------------------ l3: repo scope

#[test]
fn l3_the_repo_file_resolves_with_source_repo() {
    let fx = Layout::new();
    fx.write("acme/r.toml", "[tests]\ntext = \"Repo tests.\"\n");
    let out = fx.repo();
    assert_eq!(
        tests_of(&out)["source"],
        format!("repo:{}", fx.real("acme/r.toml")),
        "{out}"
    );
}

#[test]
fn l3_the_repo_wins_over_the_owner_and_the_machine() {
    let fx = Layout::new();
    fx.write("host.toml", "[tests]\ntext = \"Machine.\"\n");
    assert_eq!(tests_of(&fx.owner())["text"], "Machine.");
    fx.write("acme/owner.toml", "[tests]\ntext = \"Owner.\"\n");
    assert_eq!(tests_of(&fx.owner())["text"], "Owner.");
    fx.write("acme/r.toml", "[tests]\ntext = \"Repo.\"\n");
    assert_eq!(tests_of(&fx.repo())["text"], "Repo.");
}

#[test]
fn l3_a_missing_repo_file_inherits_the_owner() {
    let fx = Layout::new();
    fx.write("acme/owner.toml", "[tests]\ntext = \"Owner.\"\n");
    fs::remove_file(fx.path("acme/r.toml")).unwrap();
    let out = fx.repo();
    assert_eq!(tests_of(&out)["text"], "Owner.", "{out}");
    assert_eq!(
        out["files"],
        serde_json::json!([fx.real("acme/owner.toml")]),
        "{out}"
    );
}

#[test]
fn l3_a_missing_owner_file_is_owner_unconfirmed_naming_the_file() {
    let fx = Layout::new();
    fs::remove_file(fx.path("acme/owner.toml")).unwrap();
    fs::remove_file(fx.path("acme/r.toml")).unwrap();
    let error = refused(&fx.show(&["--owner", "acme"]), "owner_unconfirmed", 1);
    assert_eq!(
        error["details"]["file"],
        fx.path("acme/owner.toml").display().to_string(),
        "{error}"
    );
}

// ------------------------------------------------------------------ l4: files

#[test]
fn l4_files_lists_host_owner_repo_realpaths() {
    let fx = Layout::new();
    fx.write("host.toml", "");
    let out = fx.repo();
    assert_eq!(
        out["files"],
        serde_json::json!([
            fx.real("host.toml"),
            fx.real("acme/owner.toml"),
            fx.real("acme/r.toml")
        ]),
        "{out}"
    );
}

#[test]
fn l4_absent_files_are_left_out() {
    let fx = Layout::new();
    let out = fx.repo();
    assert_eq!(
        out["files"],
        serde_json::json!([fx.real("acme/owner.toml"), fx.real("acme/r.toml")]),
        "{out}"
    );
    assert_eq!(
        fx.owner()["files"],
        serde_json::json!([fx.real("acme/owner.toml")])
    );
}

// ------------------------------------------------------------------ l5: no `host` key

#[test]
fn l5_the_host_key_is_config_invalid() {
    let fx = Fx::new();
    fx.env
        .write_config("version = 1\nmachine = \"pro\"\nhost = \"x\"\n");
    let error = refused(&fx.cpd(&["config"]), "config_invalid", 1);
    assert_eq!(error["details"]["key"], "host", "{error}");
}

#[test]
fn l5_config_prints_no_host() {
    let fx = Fx::new();
    fx.env.write_config("version = 1\nmachine = \"pro\"\n");
    let out = fx.cpd(&["config"]).ok();
    assert!(out["config"].get("host").is_none(), "{out}");
    assert!(out["sources"].get("host").is_none(), "{out}");
}

// ------------------------------------------------------------------ l6: a repo named `owner`

#[test]
fn l6_show_refuses_a_repo_named_owner() {
    let fx = Layout::new();
    fx.write("acme/owner.toml", "");
    refused(
        &fx.show(&["--owner", "acme", "--repo", "owner"]),
        "usage",
        2,
    );
}

#[test]
fn l6_pr_add_of_a_repo_whose_origin_is_acme_owner_is_repo_unkeyed() {
    let fx = Layout::new();
    let fx = &fx.fx;
    let repo = fx.git_repo("owner");
    fx.git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    fx.git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/owner.git",
        ],
    );
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay-1', 'ctx', 'Pay', 'grounding', 'lead1', 't');",
    );
    let run = fx.cpd(&[
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
    ]);
    refused(&run, "repo_unkeyed", 1);
}

// ------------------------------------------------------------------ l7: the fallback

fn confirm(fx: &Layout, id: &str) -> Run {
    fx.fx.cpd(&[
        "context",
        "new",
        id,
        "--title",
        "T",
        "--owner",
        "stranger",
        "--confirm-fallback",
    ])
}

#[test]
fn l7_confirming_creates_the_directory_and_a_relative_symlink() {
    let fx = Layout::bare();
    fx.write(
        "clayton-duarte/owner.toml",
        "[tests]\ntext = \"Fallback.\"\n",
    );
    confirm(&fx, "c1").ok();
    let link = fx.path("stranger/owner.toml");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_link(&link).unwrap(),
        PathBuf::from("../clayton-duarte/owner.toml")
    );
    let out = fx.show(&["--owner", "stranger"]).ok();
    assert_eq!(
        tests_of(&out)["source"],
        format!("owner:{}", fx.real("clayton-duarte/owner.toml")),
        "{out}"
    );
}

#[test]
fn l7_an_existing_owner_directory_gets_the_link_inside() {
    let fx = Layout::bare();
    fx.write("clayton-duarte/owner.toml", "");
    fx.write("stranger/r.toml", "");
    confirm(&fx, "c1").ok();
    assert_eq!(
        fs::read_link(fx.path("stranger/owner.toml")).unwrap(),
        PathBuf::from("../clayton-duarte/owner.toml")
    );
    assert!(fx.path("stranger/r.toml").is_file());
}

#[test]
fn l7_without_the_fallbacks_file_it_is_fallback_missing_and_nothing_is_created() {
    let fx = Layout::bare();
    let error = refused(&confirm(&fx, "c1"), "fallback_missing", 1);
    assert_eq!(
        error["details"]["file"],
        fx.path("clayton-duarte/owner.toml").display().to_string(),
        "{error}"
    );
    assert!(fs::symlink_metadata(fx.path("stranger")).is_err());
}

#[test]
fn l7_revoking_the_link_makes_the_owner_unconfirmed_again() {
    let fx = Layout::bare();
    fx.write("clayton-duarte/owner.toml", "");
    confirm(&fx, "c1").ok();
    fx.show(&["--owner", "stranger"]).ok();
    fs::remove_file(fx.path("stranger/owner.toml")).unwrap();
    let error = refused(&fx.show(&["--owner", "stranger"]), "owner_unconfirmed", 1);
    assert_eq!(
        error["details"]["file"],
        fx.path("stranger/owner.toml").display().to_string(),
        "{error}"
    );
}

#[test]
fn l7_a_dangling_link_is_not_a_known_owner() {
    let fx = Layout::bare();
    fs::create_dir(fx.path("stranger")).unwrap();
    symlink(
        "../clayton-duarte/owner.toml",
        fx.path("stranger/owner.toml"),
    )
    .unwrap();
    let error = refused(&fx.show(&["--owner", "stranger"]), "owner_unconfirmed", 1);
    assert_eq!(
        error["details"]["file"],
        fx.path("stranger/owner.toml").display().to_string(),
        "{error}"
    );
}

// ------------------------------------------------------------------ l8: strict entries

/// Every form of `policy show` is refused `policy_invalid`, `details.file` the entry.
fn invalid(fx: &Layout, entry: &str) -> Value {
    let mut last = Value::Null;
    for args in [
        &["--owner", "acme"][..],
        &["--owner", "acme", "--repo", "r"][..],
    ] {
        let error = refused(&fx.show(args), "policy_invalid", 1);
        assert_eq!(
            error["details"]["file"],
            fx.path(entry).display().to_string(),
            "{args:?}: {error}"
        );
        last = error;
    }
    last
}

#[test]
fn l8_the_old_owners_directory_names_the_old_layout() {
    let fx = Layout::new();
    fx.write("owners/clayton-duarte.toml", "");
    let error = invalid(&fx, "owners");
    let message = error["message"].as_str().unwrap();
    for old in ["owners/", "repos/", "machines/"] {
        assert!(message.contains(old), "{message}");
    }
}

#[test]
fn l8_the_old_repos_directory_is_policy_invalid() {
    let fx = Layout::new();
    fx.write("repos/acme/r.toml", "");
    let error = invalid(&fx, "repos");
    assert!(
        error["message"].as_str().unwrap().contains("repos/"),
        "{error}"
    );
}

#[test]
fn l8_a_stray_file_at_the_root_is_policy_invalid() {
    for name in ["x.txt", "stray.toml"] {
        let fx = Layout::new();
        fx.write(name, "");
        invalid(&fx, name);
    }
}

#[test]
fn l8_an_invalid_owner_directory_name_is_policy_invalid() {
    for name in ["a_b", "-x"] {
        let fx = Layout::new();
        fx.write(&format!("{name}/owner.toml"), "");
        invalid(&fx, name);
    }
}

#[test]
fn l8_an_invalid_repo_file_name_is_policy_invalid() {
    let fx = Layout::new();
    fx.write("acme/Bad Name.toml", "");
    invalid(&fx, "acme/Bad Name.toml");
}

#[test]
fn l8_a_directory_with_repo_files_and_no_owner_toml_is_policy_invalid() {
    let fx = Layout::new();
    fx.write("beta/r.toml", "");
    let mut entries = vec![];
    for args in [
        &["--owner", "acme"][..],
        &["--owner", "acme", "--repo", "r"][..],
    ] {
        entries.push(refused(&fx.show(args), "policy_invalid", 1));
    }
    for error in entries {
        assert_eq!(
            error["details"]["file"],
            fx.path("beta").display().to_string(),
            "{error}"
        );
    }
}

#[test]
fn l8_dot_names_are_ignored() {
    let fx = Layout::new();
    fx.write(".DS_Store", "");
    fx.write("acme/.DS_Store", "");
    fx.repo();
}

#[test]
fn l8_a_tree_with_only_host_toml_is_valid() {
    let fx = Layout::bare();
    fx.write("host.toml", "");
    let error = refused(&fx.show(&["--owner", "acme"]), "owner_unconfirmed", 1);
    assert_eq!(
        error["details"]["file"],
        fx.path("acme/owner.toml").display().to_string(),
        "{error}"
    );
}

// ------------------------------------------------------------------ l9: context owner

#[test]
fn l9_context_new_accepts_an_owner_with_owner_toml() {
    let fx = Layout::new();
    fx.fx
        .cpd(&["context", "new", "c1", "--title", "T", "--owner", "acme"])
        .ok();
}

#[test]
fn l9_context_new_without_owner_toml_is_owner_unconfirmed() {
    let fx = Layout::bare();
    fx.write("clayton-duarte/owner.toml", "");
    let error = refused(
        &fx.fx
            .cpd(&["context", "new", "c1", "--title", "T", "--owner", "acme"]),
        "owner_unconfirmed",
        1,
    );
    assert_eq!(
        error["details"]["file"],
        fx.path("acme/owner.toml").display().to_string(),
        "{error}"
    );
}

#[test]
fn l9_context_owner_accepts_with_and_refuses_without_owner_toml() {
    let fx = Layout::new();
    fx.fx
        .cpd(&["context", "new", "c0", "--title", "T", "--owner", "acme"])
        .ok();
    fx.fx
        .env
        .sql("UPDATE contexts SET owner = NULL WHERE id = 'c0'");
    fx.fx
        .cpd(&["context", "owner", "acme", "--context", "c0"])
        .ok();
    fx.fx
        .env
        .sql("UPDATE contexts SET owner = NULL WHERE id = 'c0'");
    fs::remove_file(fx.path("acme/owner.toml")).unwrap();
    let error = refused(
        &fx.fx.cpd(&["context", "owner", "acme", "--context", "c0"]),
        "owner_unconfirmed",
        1,
    );
    assert_eq!(
        error["details"]["file"],
        fx.path("acme/owner.toml").display().to_string(),
        "{error}"
    );
}
