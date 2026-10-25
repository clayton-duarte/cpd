//! The test commands (`[test-commands]`) and the protected paths (`[protected]`) come from the repo's policy file
//! (contract §7.5, §7.7, §8.3), never from `.config/cpd.toml`: through the built binary against a temporary `$CPD_HOME`.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{refused, Run, TestEnv};
use cpd_core::git::{create_attempt_worktree, create_lead_worktree, Setup};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::json;

/// `build` fails while `BROKEN` exists; `run` fails unless `GREEN` exists.
const COMMANDS: &str = "[test-commands]\nbuild = \"test ! -e BROKEN\"\nrun = \"echo running; echo {{ tests }} > ../ran.txt; test -e GREEN\"\n";

// ---------- pr add ----------

fn pr_env() -> TestEnv {
    let env = TestEnv::new();
    env.fake_tmux();
    fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    env.write_tmux_config("");
    env.fake_tmux();
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    env.cpd(&["plan", "new", "pl", "--context", "ctx", "--title", "Plan"]).ok();
    env
}

fn add_tdd(env: &TestEnv, repo: &Path) -> Run {
    env.cpd(&["pr", "add", "pay", "--repo", repo.to_str().unwrap(), "--title", "Change", "--shape", "tdd", "--plan", "pl"])
}

#[test]
fn pr_add_of_a_tdd_pr_takes_the_test_commands_from_the_repo_file() {
    let env = pr_env();
    let repo = env.keyed_repo("shop").canonicalize().unwrap();
    assert!(!repo.join(".config/cpd.toml").exists());
    env.repo_policy("acme", "shop", "[test-commands]\nbuild = \"true\"\nrun = \"true\"\n");
    add_tdd(&env, &repo).ok();
}

#[test]
fn a_cpd_toml_in_the_repo_tree_does_not_configure_the_test_commands() {
    let env = pr_env();
    let repo = env.keyed_repo("shop").canonicalize().unwrap();
    fs::create_dir_all(repo.join(".config")).unwrap();
    fs::write(repo.join(".config/cpd.toml"), "[tests]\nbuild = \"true\"\nrun = \"true\"\n").unwrap();
    env.git(&repo, &["add", "-A"]);
    env.git(&repo, &["commit", "-q", "-m", "tests table"]);
    env.git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    env.repo_policy("acme", "shop", "");
    let run = add_tdd(&env, &repo);
    let error = refused(&run, "tests_unconfigured", 3);
    assert!(error["message"].as_str().unwrap().contains("policy/repos/acme/shop.toml"), "{run:?}");
}

// ---------- attempt submit ----------

const PLAN: &str = "pay";

struct Fx {
    env: TestEnv,
    work: PathBuf,
}

/// A repo keyed `acme/pay` with `repo_file` as its policy file and `files` committed on `main`; Job `api` of `kind`
/// (a `test` Job names `t::one` in `tests/t.rs`; an `impl` Job needs the merged test Job naming `t::one`).
fn fixture(repo_file: &str, files: &[(&str, &str)], kind: &str) -> Fx {
    let env = TestEnv::new();
    let repo = env.keyed_repo("pay").canonicalize().unwrap();
    let bare = env.root().join("pay-origin.git");
    env.git(&repo, &["clone", "-q", "--bare", repo.to_str().unwrap(), bare.to_str().unwrap()]);
    env.local_origin(&repo, &bare);
    env.repo_policy("acme", "pay", repo_file);
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(&repo, &["config", key, value]);
    }
    for (path, content) in files {
        let full = repo.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, content).unwrap();
    }
    env.git(&repo, &["add", "-A"]);
    env.git(&repo, &["commit", "-q", "-m", "setup", "--allow-empty"]);
    let plan = PlanId::parse(PLAN).unwrap();
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("setup.log") };
    let lead = create_lead_worktree(&repo, &plan, "main", &setup).unwrap().path;
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{}', 'pay', 'main', 'dispatch', 'cpd/pay', '{}', cpd_txn(), 't', 't');",
        repo.display(),
        lead.display()
    ));
    let attempt = AttemptId::parse("api-a1").unwrap();
    let made = create_attempt_worktree(&repo, &lead, &plan, &attempt, &Setup { timeout: Duration::from_secs(30), log: env.root().join("attempt.log") }).unwrap();
    let (tests, files) = if kind == "test" { (Some(r#"["t::one"]"#), Some(r#"["tests/t.rs"]"#)) } else { (None, None) };
    Store::open(&home, 5000)
        .unwrap()
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at, kind, tests, test_files)
                 VALUES ('pay', 'repo', 'api', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, ?1, ?2, 't', ?3, ?4, ?5)",
                params!["f".repeat(64), tx.id(), kind, tests, files],
            )?;
            if kind == "impl" {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                                       created_txn, created_at, kind, tests, test_files, merged_sha)
                       VALUES ('pay', 'repo', 't', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, printf('%064d', 1),
                               ?1, 't', 'test', '[\"t::one\"]', '[\"tests/\"]', 'c0ffee')",
                    params![tx.id()],
                )?;
                tx.execute("INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'api', 't')", [])?;
            }
            tx.execute(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
                 VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', ?1, ?2, ?3, 't')",
                params![made.branch, made.path.to_str().unwrap(), made.base_sha],
            )?;
            Ok(())
        })
        .unwrap();
    Fx { env, work: made.path }
}

impl Fx {
    fn commit(&self, path: &str, content: &str) {
        let full = self.work.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, content).unwrap();
        self.env.git(&self.work, &["add", "-A"]);
        self.env.git(&self.work, &["commit", "-q", "-m", &format!("edit {path}")]);
    }

    fn submit(&self) -> Run {
        self.env.cpd_in(&self.work, &["attempt", "submit", "api-a1", "--plan", PLAN])
    }

    fn status(&self) -> String {
        self.env.query("SELECT status FROM attempts WHERE id = 'api-a1'")
    }
}

#[test]
fn a_test_job_submit_builds_and_runs_with_the_test_commands_of_the_repo_file() {
    let fx = fixture(COMMANDS, &[], "test");
    // A test that passes is not red.
    fx.commit("tests/t.rs", "// passes\n");
    fx.commit("GREEN", "x\n");
    let error = refused(&fx.submit(), "tests_not_red", 3);
    assert_eq!(error["details"]["tests"], json!(["t::one"]));
    assert!(error["details"]["tail"].as_str().unwrap().contains("running"), "{error}");
    // A red one is accepted.
    fx.env.git(&fx.work, &["rm", "-q", "GREEN"]);
    fx.env.git(&fx.work, &["commit", "-q", "-m", "red"]);
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
}

#[test]
fn an_impl_job_submit_runs_the_named_tests_with_the_run_command_of_the_repo_file() {
    let fx = fixture(COMMANDS, &[], "impl");
    fx.commit("src/lib.rs", "// not enough\n");
    let error = refused(&fx.submit(), "tests_failed", 3);
    assert_eq!(error["details"]["tests"], json!(["t::one"]));
    assert!(error["details"]["tail"].as_str().unwrap().contains("running"), "{error}");
    fx.commit("GREEN", "x\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
}

#[test]
fn a_diff_touching_a_protected_path_of_the_repo_file_is_tampering() {
    let fx = fixture("[protected]\npaths = [\"verify.sh\", \"scripts/ci/\"]\n", &[("verify.sh", "#!/bin/sh\n")], "work");
    fx.commit("src/fine.rs", "ok\n");
    fx.commit("verify.sh", "#!/bin/sh\nexit 0\n");
    fx.commit("scripts/ci/run.sh", "x\n");
    let error = refused(&fx.submit(), "gate_tampered", 3);
    let mut paths: Vec<String> = serde_json::from_value(error["details"]["paths"].clone()).unwrap();
    paths.sort();
    assert_eq!(paths, ["scripts/ci/run.sh", "verify.sh"]);
    assert_eq!(fx.status(), "running");
}

#[test]
fn a_diff_touching_wt_toml_or_cpd_toml_is_not_tampering() {
    let fx = fixture("", &[], "work");
    fx.commit(".config/wt.toml", "[pre-commit]\nnothing = \"true\"\n");
    fx.commit(".config/cpd.toml", "protected = [\"verify.sh\"]\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
}
