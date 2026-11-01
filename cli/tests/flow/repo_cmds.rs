//! The test commands (`[test-commands]`) and the protected paths (`[protected]`) come from the repo's policy file
//! (contract §7.5, §7.7, §8.3), never from `.config/cpd.toml`: the command tree in this process (`Fx::with_ports`) against
//! a temporary `$CPD_HOME`, the repos and worktrees repos of the fake git. The repo file's commands are not run through
//! `sh` (an external): the `sh` hook route of the `FakeRunner` answers each call by the command and the worktree it names.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use common::fx::Fx;
use common::{refused, Run};
use cpd_core::git::{create_attempt_worktree, create_lead_worktree, Setup};
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::ports::Ports;
use serde_json::json;

/// `build` fails while `BROKEN` exists; `run` fails unless `GREEN` exists.
const BUILD: &str = "test ! -e BROKEN";
const COMMANDS: &str = "[test-commands]\nbuild = \"test ! -e BROKEN\"\nrun = \"echo running; echo {{ tests }} > ../ran.txt; test -e GREEN\"\n";

/// The answer of `sh -c <command>` for the two commands of [`COMMANDS`], read off the worktree the call names (its `cwd`),
/// as the shell would: `build` is `test ! -e BROKEN`; `run` prints `running` and is `test -e GREEN`. Any other
/// command is not scripted: it answers exit 99 and the test fails on its call log.
struct Sh;

impl Handler for Sh {
    fn handle(&self, call: &Call) -> Reply {
        let cwd = call.cwd.as_deref().expect("a hook runs in a worktree");
        let command = match call.args.as_slice() {
            [flag, command] if flag == "-c" => command.as_str(),
            other => panic!("sh was not called as `sh -c <command>`: {other:?}"),
        };
        if command == BUILD {
            return Reply::exit(i32::from(cwd.join("BROKEN").exists()), "", "");
        }
        if command.starts_with("echo running; echo ") && command.ends_with("> ../ran.txt; test -e GREEN") {
            return Reply::exit(i32::from(!cwd.join("GREEN").exists()), "running\n", "");
        }
        Reply::exit(99, "", &format!("no reply scripted for {command:?}\n"))
    }
}

// ---------- pr add ----------

const PLAN: &str = "pay";

/// A repo `shop` of the fake git keyed `acme/shop`, with its commits on `main` and `origin/main`, and Plan `pl`
/// (`grounding`) of Context `ctx` owned by `acme`, ready for `pr add`.
fn pr_fx() -> (Fx, PathBuf) {
    let fx = Fx::with_ports();
    fx.env.write_config("version = 1\nmachine = \"m1\"\n");
    let repo = fx.keyed_repo("shop").canonicalize().unwrap();
    fx.git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Payments', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pl', 'ctx', 'Plan', 'grounding', 'lead1', 't');",
    );
    (fx, repo)
}

fn add_tdd(fx: &Fx, repo: &Path) -> Run {
    fx.cpd(&["pr", "add", "pay", "--repo", repo.to_str().unwrap(), "--title", "Change", "--shape", "tdd", "--plan", "pl"])
}

#[test]
fn pr_add_of_a_tdd_pr_takes_the_test_commands_from_the_repo_file() {
    let (fx, repo) = pr_fx();
    assert!(!repo.join(".config/cpd.toml").exists());
    fx.env.repo_policy("acme", "shop", "[test-commands]\nbuild = \"true\"\nrun = \"true\"\n");
    add_tdd(&fx, &repo).ok();
}

#[test]
fn a_cpd_toml_in_the_repo_tree_does_not_configure_the_test_commands() {
    let (fx, repo) = pr_fx();
    fx.commit_file(&repo, ".config/cpd.toml", "[tests]\nbuild = \"true\"\nrun = \"true\"\n", "tests table");
    fx.git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    fx.env.repo_policy("acme", "shop", "");
    let run = add_tdd(&fx, &repo);
    let error = refused(&run, "tests_unconfigured", 3);
    assert!(error["message"].as_str().unwrap().contains("policy/acme/shop.toml"), "{run:?}");
}

// ---------- attempt submit ----------

/// What the fixture made: the Fx and the Attempt `api-a1`'s worktree.
struct Sub {
    fx: Fx,
    work: PathBuf,
    /// The Attempt's `sh` calls the route answered, in order.
    sh: Arc<Mutex<Vec<Call>>>,
}

/// Records the `sh` calls and answers them as [`Sh`].
struct Recorded(Arc<Mutex<Vec<Call>>>);

impl Handler for Recorded {
    fn handle(&self, call: &Call) -> Reply {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(call.clone());
        Sh.handle(call)
    }
}

/// A repo keyed `acme/pay` with `repo_file` as its policy file and `files` committed on `main`; Job `api` of `kind`
/// (a `test` Job names `t::one` in `tests/t.rs`; an `impl` Job needs the merged test Job naming `t::one`).
fn fixture(repo_file: &str, files: &[(&str, &str)], kind: &str) -> Sub {
    let fx = Fx::with_ports();
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    let bare = fx.env.root().join("pay-origin.git");
    fx.git(fx.env.root(), &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()]);
    fx.local_origin(&repo, &bare);
    fx.git(&repo, &["push", "-q", "origin", "main"]);
    fx.env.repo_policy("acme", "pay", repo_file);
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        fx.git(&repo, &["config", key, value]);
    }
    for (path, content) in files {
        let full = repo.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, content).unwrap();
    }
    fx.git(&repo, &["add", "-A"]);
    fx.git(&repo, &["commit", "-q", "-m", "setup", "--allow-empty"]);
    fx.env.write_config("version = 1\nmachine = \"t\"\n");
    let plan = PlanId::parse(PLAN).unwrap();
    let attempt = AttemptId::parse("api-a1").unwrap();
    let setup = |log: &str| Setup { timeout: Duration::from_secs(30), log: fx.env.root().join(log) };
    // The worktrees are made by the product's own functions, on the ports of the `Fx`: git is the fake git.
    let (lead, made) = Ports::scope(&fx.fakes.ports, || {
        let lead = create_lead_worktree(&repo, &plan, "main", &setup("setup.log")).unwrap().path;
        let made = create_attempt_worktree(&repo, &lead, &plan, &attempt, &setup("attempt.log")).unwrap();
        (lead, made)
    });
    fx.env.sql(&format!(
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
    let (tests, test_files) = if kind == "test" { ("'[\"t::one\"]'", "'[\"tests/t.rs\"]'") } else { ("NULL", "NULL") };
    let fingerprint = "f".repeat(64);
    // One transaction, as the Jobs of a locked Plan are written: a Job and its Dependency of another transaction are `job_locked`.
    let mut jobs = format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                           status, appended, fingerprint, created_txn, created_at, kind, tests, test_files)
         VALUES ('pay', 'repo', 'api', 'T', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'running', 0, '{fingerprint}', cpd_txn(), 't', '{kind}', {tests}, {test_files});"
    );
    if kind == "impl" {
        jobs.push_str(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               created_txn, created_at, kind, tests, test_files, merged_sha)
               VALUES ('pay', 'repo', 't', 'T', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'merged', 0, printf('%064d', 1),
                       cpd_txn(), 't', 'test', '[\"t::one\"]', '[\"tests/\"]', 'c0ffee');
             INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'api', 't');",
        );
    }
    fx.env.sql(&format!(
        "{jobs}
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
         VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', '{}', '{}', '{}', 't');",
        made.branch,
        made.path.display(),
        made.base_sha
    ));
    let sh = Arc::new(Mutex::new(Vec::new()));
    let _ = (*fx.fakes.runner).clone().route("sh", Arc::new(Recorded(sh.clone())));
    Sub { fx, work: made.path, sh }
}

impl Sub {
    fn commit(&self, path: &str, content: &str) {
        self.fx.commit_file(&self.work, path, content, &format!("edit {path}"));
    }

    fn submit(&self) -> Run {
        self.fx.cpd_in(&self.work, &["attempt", "submit", "api-a1", "--plan", PLAN])
    }

    fn status(&self) -> String {
        self.fx.env.query("SELECT status FROM attempts WHERE id = 'api-a1'")
    }

    /// The commands `sh -c` was asked to run, in order, each with its cwd.
    fn sh_calls(&self) -> Vec<(String, PathBuf)> {
        let calls = self.sh.lock().unwrap_or_else(PoisonError::into_inner);
        calls.iter().map(|call| (call.args[1].clone(), call.cwd.clone().unwrap())).collect()
    }
}

#[test]
fn a_test_job_submit_builds_and_runs_with_the_test_commands_of_the_repo_file() {
    let fx = fixture(COMMANDS, &[], "test");
    let work = fx.work.canonicalize().unwrap();
    // A test that passes is not red.
    fx.commit("tests/t.rs", "// passes\n");
    fx.commit("GREEN", "x\n");
    let error = refused(&fx.submit(), "tests_not_red", 3);
    assert_eq!(error["details"]["tests"], json!(["t::one"]));
    assert!(error["details"]["tail"].as_str().unwrap().contains("running"), "{error}");
    // The commands that ran are the repo file's, `build` then `run`, in the Attempt's worktree.
    assert_eq!(
        fx.sh_calls().iter().map(|(command, cwd)| (command.as_str(), cwd.canonicalize().unwrap())).collect::<Vec<_>>(),
        [(BUILD, work.clone()), ("echo running; echo t::one > ../ran.txt; test -e GREEN", work.clone())]
    );
    // A red one is accepted.
    fx.fx.git(&fx.work, &["rm", "-q", "GREEN"]);
    fx.fx.git(&fx.work, &["commit", "-q", "-m", "red"]);
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
    // `run` is the repo file's, given the named tests (the `test` Job's), after `build`.
    let commands: Vec<String> = fx.sh_calls().into_iter().map(|(command, _)| command).collect();
    assert_eq!(commands.len(), 4, "{commands:?}");
    assert_eq!(commands[1], "echo running; echo t::one > ../ran.txt; test -e GREEN");
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
