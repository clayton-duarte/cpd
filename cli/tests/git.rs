//! The git layer (contract §7.1–§7.3, §7.5) against temporary repos.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::TestEnv;
use cpd_core::error::ErrorCode;
use cpd_core::git::hooks::{HookError, HookOutcome, load, run_hook};
use cpd_core::git::{
    Operation, Setup, attempt_branch, attempt_worktree_path, create_attempt_worktree, create_lead_worktree,
    discard_worktree, lead_branch, lead_worktree_path, protected_list, remove_merged_attempt, tamper_check,
    touched_protected,
};
use cpd_core::home::Hook;
use cpd_core::ids::{AttemptId, PlanId};

fn plan() -> PlanId {
    PlanId::parse("pay-4521").unwrap()
}

fn attempt() -> AttemptId {
    AttemptId::parse("api-a1").unwrap()
}

fn setup(env: &TestEnv, name: &str) -> Setup {
    Setup { timeout: Duration::from_secs(30), log: env.root().join("logs").join(name) }
}

/// Writes `path` in `repo`'s checkout and commits it.
fn commit_file(env: &TestEnv, repo: &Path, path: &str, content: &str) {
    let full = repo.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(&full, content).unwrap();
    env.git(repo, &["add", "-A"]);
    env.git(repo, &["commit", "-q", "-m", &format!("edit {path}")]);
}

fn wt_toml(env: &TestEnv, repo: &Path, text: &str) {
    commit_file(env, repo, ".config/wt.toml", text);
}

// ------------------------------------------------------------ worktrees

#[test]
fn lead_worktree_is_a_sibling_on_cpd_plan_cut_from_the_base() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    wt_toml(&env, &repo, "[pre-start]\nmark = \"echo {{ branch }} {{ base }} > setup-ran\"\n");
    let main_sha = env.git(&repo, &["rev-parse", "main"]);

    let made = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "lead.log")).unwrap();

    assert_eq!(made.path, repo.parent().unwrap().join("pay.pay-4521"));
    assert_eq!(made.path, lead_worktree_path(&repo, &plan()).unwrap());
    assert_eq!(made.branch, "cpd/pay-4521");
    assert_eq!(made.branch, lead_branch(&plan()));
    assert_eq!(made.base_sha, main_sha);
    assert_eq!(env.git(&made.path, &["rev-parse", "--abbrev-ref", "HEAD"]), "cpd/pay-4521");
    assert_eq!(env.git(&made.path, &["rev-parse", "HEAD"]), main_sha);
    // pre-start ran in it, with its variables.
    assert_eq!(fs::read_to_string(made.path.join("setup-ran")).unwrap(), "cpd/pay-4521 main\n");
    // The main checkout is untouched.
    assert_eq!(env.git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(env.git(&repo, &["status", "--porcelain"]), "");
}

#[test]
fn lead_worktree_is_cut_from_a_freshly_fetched_origin_base() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let origin = env.root().join("origins").join("pay.git");
    fs::create_dir_all(&origin).unwrap();
    env.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    env.git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    env.git(&repo, &["push", "-q", "origin", "main"]);
    // Someone else advances origin/main; the repo's own refs are stale.
    let other = env.root().join("other");
    env.run_ok(env.root(), "git", &["clone", "-q", origin.to_str().unwrap(), other.to_str().unwrap()]);
    commit_file(&env, &other, "new.txt", "new\n");
    env.git(&other, &["push", "-q", "origin", "main"]);
    let pushed = env.git(&other, &["rev-parse", "HEAD"]);

    let made = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "lead.log")).unwrap();

    assert_eq!(made.base_sha, pushed);
    assert!(made.path.join("new.txt").exists());
    assert_ne!(env.git(&repo, &["rev-parse", "main"]), pushed, "the local base is not moved");
}

#[test]
fn a_failing_pre_start_removes_the_worktree_and_branch() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    wt_toml(&env, &repo, "pre-start = \"echo no luck; exit 3\"\n");
    let setup = setup(&env, "lead.log");

    let err = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap_err();

    assert_eq!(err.code(), ErrorCode::SetupFailed);
    assert_eq!(err.details()["tail"], "no luck");
    assert_eq!(err.details()["log"], setup.log.to_str().unwrap());
    assert!(!lead_worktree_path(&repo, &plan()).unwrap().exists());
    assert_eq!(env.git(&repo, &["branch", "--list", "cpd/*"]), "");
    assert_eq!(fs::read_to_string(&setup.log).unwrap(), "no luck\n");
}

#[test]
fn an_invalid_pre_start_is_setup_failed_too() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    wt_toml(&env, &repo, "pre-start = \"echo {{ nope }}\"\n");
    let err = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "lead.log")).unwrap_err();
    assert_eq!(err.code(), ErrorCode::SetupFailed);
    assert!(!lead_worktree_path(&repo, &plan()).unwrap().exists());
    assert_eq!(env.git(&repo, &["branch", "--list", "cpd/*"]), "");
}

#[test]
fn lead_worktree_refuses_what_is_in_the_way() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let path = lead_worktree_path(&repo, &plan()).unwrap();

    fs::create_dir_all(&path).unwrap();
    let err = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "l.log")).unwrap_err();
    assert_eq!(err.code(), ErrorCode::WorktreeExists);
    fs::remove_dir(&path).unwrap();

    env.git(&repo, &["branch", "cpd/pay-4521"]);
    let err = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "l.log")).unwrap_err();
    assert_eq!(err.code(), ErrorCode::BranchExists);
    assert!(!path.exists());
}

#[test]
fn lead_worktree_replaces_the_leftover_of_an_interrupted_lock() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let path = lead_worktree_path(&repo, &plan()).unwrap();
    env.git(&repo, &["worktree", "add", "-q", "-b", "cpd/pay-4521", path.to_str().unwrap(), "main"]);
    fs::write(path.join("junk"), "left behind").unwrap();

    let made = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "l.log")).unwrap();

    assert_eq!(made.path, path);
    assert!(!path.join("junk").exists());
    assert_eq!(env.git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]), "cpd/pay-4521");
}

#[test]
fn attempt_worktree_is_cut_from_the_lead_branch_head() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    wt_toml(
        &env,
        &repo,
        "[pre-start]\nmark = \"echo {{ branch }} {{ base }} {{ base_worktree_path }} > setup-ran\"\n",
    );
    let setup = setup(&env, "attempt.log");
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap();
    // The Lead Branch has moved on from main.
    commit_file(&env, &lead.path, "lead.txt", "lead\n");
    let lead_sha = env.git(&lead.path, &["rev-parse", "HEAD"]);

    let made = create_attempt_worktree(&repo, &lead.path, &plan(), &attempt(), &setup).unwrap();

    assert_eq!(made.path, repo.parent().unwrap().join("pay.pay-4521--api-a1"));
    assert_eq!(made.path, attempt_worktree_path(&repo, &plan(), &attempt()).unwrap());
    assert_eq!(made.branch, "cpd/pay-4521--api-a1");
    assert_eq!(made.branch, attempt_branch(&plan(), &attempt()));
    assert_eq!(made.base_sha, lead_sha);
    assert_eq!(env.git(&made.path, &["rev-parse", "HEAD"]), lead_sha);
    assert_eq!(env.git(&made.path, &["rev-parse", "--abbrev-ref", "HEAD"]), made.branch);
    let ran = fs::read_to_string(made.path.join("setup-ran")).unwrap();
    assert_eq!(ran.trim(), format!("cpd/pay-4521--api-a1 cpd/pay-4521 {}", lead.path.canonicalize().unwrap().display()));
}

#[test]
fn a_failing_attempt_pre_start_removes_the_worktree_and_branch() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let setup = setup(&env, "attempt.log");
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap();
    wt_toml(&env, &lead.path, "pre-start = \"exit 1\"\n");

    let err = create_attempt_worktree(&repo, &lead.path, &plan(), &attempt(), &setup).unwrap_err();

    assert_eq!(err.code(), ErrorCode::SetupFailed);
    assert!(!attempt_worktree_path(&repo, &plan(), &attempt()).unwrap().exists());
    assert_eq!(env.git(&repo, &["branch", "--list", "cpd/pay-4521--*"]), "");
    assert_eq!(env.git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
}

#[test]
fn a_merged_attempt_is_removed_with_branch_d_and_an_unmerged_one_is_refused() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let setup = setup(&env, "attempt.log");
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap();
    let made = create_attempt_worktree(&repo, &lead.path, &plan(), &attempt(), &setup).unwrap();
    commit_file(&env, &made.path, "work.txt", "work\n");

    // Unmerged: git's own guard refuses the branch deletion.
    let err = remove_merged_attempt(&lead.path, &made.path, &made.branch).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
    assert_eq!(err.details()["argv"][3], "branch");
    assert_eq!(env.git(&repo, &["branch", "--list", &made.branch]).trim(), made.branch);

    env.git(&lead.path, &["merge", "-q", "--ff-only", &made.branch]);
    remove_merged_attempt(&lead.path, &made.path, &made.branch).unwrap();
    assert!(!made.path.exists());
    assert_eq!(env.git(&repo, &["branch", "--list", &made.branch]), "");
    // Rerunning is safe.
    remove_merged_attempt(&lead.path, &made.path, &made.branch).unwrap();
}

#[test]
fn discard_removes_whatever_state_and_is_idempotent() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let setup = setup(&env, "attempt.log");
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap();
    let made = create_attempt_worktree(&repo, &lead.path, &plan(), &attempt(), &setup).unwrap();
    commit_file(&env, &made.path, "work.txt", "unmerged\n");
    fs::write(made.path.join("dirty.txt"), "dirty").unwrap();

    discard_worktree(&lead.path, &made.path, &made.branch).unwrap();
    discard_worktree(&lead.path, &made.path, &made.branch).unwrap();

    assert!(!made.path.exists());
    assert_eq!(env.git(&repo, &["branch", "--list", &made.branch]), "");
}

// ---------------------------------------------------------------- hooks

struct Hooks {
    env: TestEnv,
    repo: PathBuf,
    log: PathBuf,
}

fn hooks(wt: &str) -> Hooks {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    fs::create_dir_all(repo.join(".config")).unwrap();
    fs::write(repo.join(".config/wt.toml"), wt).unwrap();
    let log = env.root().join("logs").join("h.log");
    Hooks { env, repo, log }
}

impl Hooks {
    fn run(&self, hook: Hook) -> cpd_core::git::HookResult {
        self.run_for(hook, Duration::from_secs(30))
    }

    fn run_for(&self, hook: Hook, timeout: Duration) -> cpd_core::git::HookResult {
        let op = Operation { branch: "main" };
        run_hook(hook, &self.repo, Some(&op), timeout, &self.log).unwrap()
    }

    fn file(&self, name: &str) -> String {
        fs::read_to_string(self.repo.join(name)).unwrap()
    }
}

#[test]
fn no_file_and_no_table_mean_no_commands_and_the_hook_passes() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let log = env.root().join("h.log");
    let run = |repo: &Path| run_hook(Hook::PreCommit, repo, None, Duration::from_secs(5), &log).unwrap();

    let result = run(&repo);
    assert_eq!(result.outcome, HookOutcome::Passed);
    assert_eq!(result.tail, None);

    fs::create_dir_all(repo.join(".config")).unwrap();
    fs::write(repo.join(".config/wt.toml"), "[pre-merge]\ngate = \"exit 1\"\n").unwrap();
    // R20: no pre-commit table means no quick check; pre-merge does not run instead.
    let result = run(&repo);
    assert_eq!(result.outcome, HookOutcome::Passed);
    assert_eq!(result.tail, None);
    assert!(!log.exists());
}

#[test]
fn a_passing_hook_has_its_output_logged_and_returned() {
    let h = hooks("pre-commit = \"echo out; echo err >&2\"\n");
    let result = h.run(Hook::PreCommit);
    assert_eq!(result.outcome, HookOutcome::Passed);
    assert_eq!(result.tail.as_deref(), Some("out\nerr"));
    assert_eq!(fs::read_to_string(&h.log).unwrap(), "out\nerr\n");
}

#[test]
fn a_failing_hook_reports_its_exit_status_and_output() {
    let h = hooks("pre-merge = \"echo broken; exit 7\"\n");
    let result = h.run(Hook::PreMerge);
    assert_eq!(result.outcome, HookOutcome::Failed { exit: 7 });
    assert_eq!(result.tail.as_deref(), Some("broken"));
}

#[test]
fn only_the_last_60_lines_are_returned_and_the_log_has_all() {
    let h = hooks("pre-merge = \"seq 1 100\"\n");
    let result = h.run(Hook::PreMerge);
    let tail = result.tail.unwrap();
    assert_eq!(tail.lines().count(), 60);
    assert_eq!(tail.lines().next(), Some("41"));
    assert_eq!(tail.lines().last(), Some("100"));
    assert_eq!(fs::read_to_string(&h.log).unwrap().lines().count(), 100);
}

#[test]
fn a_table_runs_together_waits_for_all_and_fails_with_the_first_in_file_order() {
    // Keys are not in alphabetical order: file order decides, not the table's sort.
    let h = hooks("[pre-merge]\nzeta = \"exit 5\"\nalpha = \"exit 3\"\nslow = \"sleep 0.3; echo slow-done\"\n");
    let started = Instant::now();
    let result = h.run(Hook::PreMerge);
    assert_eq!(result.outcome, HookOutcome::Failed { exit: 5 });
    // The slow command was not abandoned when the others failed, and ran alongside them.
    assert!(result.tail.unwrap().contains("slow │ slow-done"));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn commands_of_a_table_run_concurrently() {
    let h = hooks("[pre-merge]\na = \"sleep 0.6\"\nb = \"sleep 0.6\"\nc = \"sleep 0.6\"\n");
    let started = Instant::now();
    assert_eq!(h.run(Hook::PreMerge).outcome, HookOutcome::Passed);
    assert!(started.elapsed() < Duration::from_millis(1500), "{:?}", started.elapsed());
}

#[test]
fn a_pipeline_runs_its_blocks_in_order_and_a_failing_block_stops_it() {
    let h = hooks(
        "[[pre-merge]]\none = \"echo one > one.txt\"\n\n[[pre-merge]]\ntwo = \"test -f one.txt && echo two > two.txt\"\nfail = \"exit 4\"\n\n[[pre-merge]]\nthree = \"echo three > three.txt\"\n",
    );
    let result = h.run(Hook::PreMerge);
    assert_eq!(result.outcome, HookOutcome::Failed { exit: 4 });
    assert_eq!(h.file("two.txt"), "two\n");
    assert!(!h.repo.join("three.txt").exists());
}

#[test]
fn output_of_concurrent_commands_is_prefixed_with_their_names() {
    let h = hooks("[pre-merge]\nlint = \"echo lint-ok\"\ntest = \"echo test-ok\"\n");
    let tail = h.run(Hook::PreMerge).tail.unwrap();
    let mut lines: Vec<&str> = tail.lines().collect();
    lines.sort();
    assert_eq!(lines, vec!["lint │ lint-ok", "test │ test-ok"]);
}

#[test]
fn commands_get_the_variables_as_a_template_and_as_json_on_stdin() {
    let h = hooks(
        "[pre-merge]\necho = \"echo {{ hook_type }} {{ hook_name }} {{ branch }} {{ target }} {{ target_worktree_path }} > echo.txt\"\njson = \"cat > ctx.json\"\n",
    );
    h.env.git(&h.repo, &["add", "-A"]);
    h.env.git(&h.repo, &["commit", "-q", "-m", "hooks"]);
    let sha = h.env.git(&h.repo, &["rev-parse", "HEAD"]);
    assert_eq!(h.run(Hook::PreMerge).outcome, HookOutcome::Passed);

    let repo = h.repo.canonicalize().unwrap();
    assert_eq!(h.file("echo.txt"), format!("pre-merge echo main main {}\n", repo.display()));
    let ctx: serde_json::Value = serde_json::from_str(&h.file("ctx.json")).unwrap();
    assert_eq!(ctx["branch"], "main");
    assert_eq!(ctx["hook_type"], "pre-merge");
    assert_eq!(ctx["hook_name"], "json");
    assert_eq!(ctx["commit"], sha);
    assert_eq!(ctx["short_commit"], &sha[..7]);
    assert_eq!(ctx["worktree_path"], repo.to_str().unwrap());
    assert_eq!(ctx["cwd"], repo.to_str().unwrap());
    assert_eq!(ctx["worktree_name"], "pay");
    assert_eq!(ctx["repo"], "pay");
    assert_eq!(ctx["repo_path"], repo.to_str().unwrap());
    assert_eq!(ctx["primary_worktree_path"], repo.to_str().unwrap());
    assert_eq!(ctx["default_branch"], "main");
    assert_eq!(ctx["target"], "main");
    assert_eq!(ctx["target_worktree_path"], repo.to_str().unwrap());
    assert_eq!(ctx["args"], serde_json::json!([]));
    assert!(ctx.get("base").is_none(), "base is for pre-start only");
}

#[test]
fn the_command_runs_with_sh_in_the_worktree_root() {
    let h = hooks("pre-commit = \"pwd > pwd.txt; echo $0 > shell.txt\"\n");
    assert_eq!(h.run(Hook::PreCommit).outcome, HookOutcome::Passed);
    assert_eq!(h.file("pwd.txt").trim(), h.repo.canonicalize().unwrap().to_str().unwrap());
    assert_eq!(h.file("shell.txt").trim(), "sh");
}

#[test]
fn an_undefined_variable_or_worktrunk_filter_fails_before_anything_runs() {
    for bad in ["{{ nope }}", "{{ branch | sanitize }}", "{{ vars.x }}", "{{ target_worktree_path_of('x') }}"] {
        let h = hooks(&format!("[pre-merge]\nfirst = \"touch ran.txt\"\nsecond = \"echo {bad}\"\n"));
        let result = h.run(Hook::PreMerge);
        assert!(matches!(result.outcome, HookOutcome::Error(HookError::Invalid(_))), "{bad}: {result:?}");
        assert_eq!(result.tail, None);
        assert!(!h.repo.join("ran.txt").exists(), "{bad}");
    }
}

#[test]
fn target_worktree_path_is_undefined_when_no_worktree_has_the_target() {
    let h = hooks("pre-merge = \"echo {{ target_worktree_path }}\"\n");
    let op = Operation { branch: "no-such-branch" };
    let result = run_hook(Hook::PreMerge, &h.repo, Some(&op), Duration::from_secs(5), &h.log).unwrap();
    assert!(matches!(result.outcome, HookOutcome::Error(HookError::Invalid(_))));
}

#[test]
fn a_malformed_wt_toml_is_hook_invalid() {
    for text in ["pre-merge = [", "pre-merge = 3\n", "[pre-merge]\nx = 1\n"] {
        let h = hooks(text);
        let result = h.run(Hook::PreMerge);
        assert!(matches!(result.outcome, HookOutcome::Error(HookError::Invalid(_))), "{text}: {result:?}");
    }
}

#[test]
fn the_file_is_read_each_time_from_the_worktree_that_runs_it() {
    let h = hooks("pre-commit = \"echo first\"\n");
    assert_eq!(h.run(Hook::PreCommit).tail.as_deref(), Some("first"));
    fs::write(h.repo.join(".config/wt.toml"), "pre-commit = \"echo second\"\n").unwrap();
    assert_eq!(h.run(Hook::PreCommit).tail.as_deref(), Some("second"));
}

#[test]
fn other_tables_are_ignored() {
    let h = hooks("post-start = \"touch post.txt\"\npre-remove = \"touch rm.txt\"\n[list]\nfull = true\n[aliases]\nx = \"touch alias.txt\"\n");
    for hook in [Hook::PreStart, Hook::PreCommit, Hook::PreMerge] {
        assert_eq!(h.run(hook).outcome, HookOutcome::Passed);
    }
    for f in ["post.txt", "rm.txt", "alias.txt"] {
        assert!(!h.repo.join(f).exists());
    }
}

#[test]
fn a_hook_is_killed_with_its_process_group_at_the_timeout() {
    let h = hooks("pre-merge = \"(sleep 1; touch survivor.txt) & wait\"\n");
    let started = Instant::now();
    let result = h.run_for(Hook::PreMerge, Duration::from_millis(300));
    assert_eq!(result.outcome, HookOutcome::Error(HookError::Timeout));
    assert!(started.elapsed() < Duration::from_millis(900), "{:?}", started.elapsed());
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!h.repo.join("survivor.txt").exists(), "the background child outlived the kill");
}

#[test]
fn a_background_child_of_a_finished_hook_is_killed_when_the_hook_returns() {
    let h = hooks("pre-merge = \"sleep 30 & echo $! > pid\"\n");
    assert_eq!(h.run(Hook::PreMerge).outcome, HookOutcome::Passed);
    let pid = h.file("pid").trim().to_owned();
    // The contract is that the child is dead, not that its pid is already free. The
    // hook's sh is gone, so the child is an orphan: after SIGKILL it is a zombie
    // (`kill -0` still succeeds) until init reaps it, and on a loaded runner that
    // window is visible. Dead means no such process or state Z; wait a bounded time
    // for the kill to land.
    let state = || {
        let out = std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid]).output().unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !(state().is_empty() || state().starts_with('Z')) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let last = state();
    assert!(last.is_empty() || last.starts_with('Z'), "the background child {pid} outlived the hook (state {last:?})");
}

#[test]
fn a_command_killed_by_a_signal_is_an_error_not_a_failure() {
    let h = hooks("pre-merge = \"echo started; kill -9 $$\"\n");
    let result = h.run(Hook::PreMerge);
    assert_eq!(result.outcome, HookOutcome::Error(HookError::Signal(9)));
    assert_eq!(result.tail.as_deref(), Some("started"));
}

#[test]
fn cockpit_pi_and_cpd_variables_do_not_reach_hooks() {
    let h = hooks("pre-merge = \"env > env.txt\"\n");
    // Names no other test in this file reads.
    unsafe {
        std::env::set_var("TMUX", "/tmp/fake,1,0");
        std::env::set_var("TMUX_PANE", "%9");
        std::env::set_var("PI_GITTEST", "x");
        std::env::set_var("CPD_GITTEST", "x");
        std::env::set_var("GITTEST_KEEP", "kept");
    }
    assert_eq!(h.run(Hook::PreMerge).outcome, HookOutcome::Passed);
    let env = h.file("env.txt");
    for gone in ["TMUX=", "TMUX_PANE=", "PI_GITTEST=", "CPD_GITTEST="] {
        assert!(!env.lines().any(|l| l.starts_with(gone)), "{gone} leaked");
    }
    assert!(env.lines().any(|l| l == "GITTEST_KEEP=kept"));
}

#[test]
fn the_commands_of_a_hook_can_be_listed_in_file_order() {
    let h = hooks("[[pre-commit]]\nz = \"first {{ branch }}\"\n\n[[pre-commit]]\nb = \"second\"\na = \"third\"\n");
    let commands: Vec<_> =
        load(&h.repo, Hook::PreCommit).unwrap().commands().map(|c| (c.name.clone().unwrap(), c.template.clone())).collect();
    assert_eq!(
        commands,
        vec![
            ("z".to_owned(), "first {{ branch }}".to_owned()),
            ("b".to_owned(), "second".to_owned()),
            ("a".to_owned(), "third".to_owned())
        ]
    );
}

// --------------------------------------------------------------- tamper

const LIST: &str = ".config/cpd.toml";

/// A repo on `main` with the repo's protected list, `verify.sh` and `scripts/ci/run.sh`
/// committed, and the branches `cpd/pay-4521` (the Lead Branch) and `att`, both at `main`.
fn tamper_repo(list: Option<&str>) -> (TestEnv, PathBuf) {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    if let Some(list) = list {
        commit_file(&env, &repo, LIST, list);
    }
    commit_file(&env, &repo, "verify.sh", "#!/bin/sh\n");
    commit_file(&env, &repo, "scripts/ci/run.sh", "ci\n");
    commit_file(&env, &repo, "src/a.rs", "a\n");
    env.git(&repo, &["branch", "cpd/pay-4521"]);
    env.git(&repo, &["branch", "att"]);
    (env, repo)
}

const DEFAULT_LIST: &str = "protected = [\"verify.sh\", \"scripts/ci/\"]\n";

/// Commits `edit` on branch `att` (changing the tree), then returns to `main`.
fn on_att(env: &TestEnv, repo: &Path, edit: impl FnOnce(&Path)) {
    env.git(repo, &["switch", "-q", "att"]);
    edit(repo);
    env.git(repo, &["add", "-A"]);
    env.git(repo, &["commit", "-q", "-m", "attempt"]);
    env.git(repo, &["switch", "-q", "main"]);
}

fn touched(repo: &Path) -> Vec<String> {
    touched_protected(repo, "cpd/pay-4521", "cpd/pay-4521", "att").unwrap()
}

#[test]
fn each_protected_path_is_refused() {
    for path in [
        ".config/wt.toml",
        ".config/cpd.toml",
        ".cpd/anything",
        ".cpd/deep/er/file",
        "verify.sh",
        "scripts/ci/run.sh",
        "scripts/ci/new/nested.sh",
    ] {
        let (env, repo) = tamper_repo(Some(DEFAULT_LIST));
        on_att(&env, &repo, |r| {
            let full = r.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, "changed\n").unwrap();
        });
        assert_eq!(touched(&repo), vec![path.to_owned()], "{path}");
        let err = tamper_check(&repo, "cpd/pay-4521", "cpd/pay-4521", "att").unwrap_err();
        assert_eq!(err.code(), ErrorCode::GateTampered, "{path}");
        assert_eq!(err.details()["paths"], serde_json::json!([path]));
    }
}

#[test]
fn deleting_a_protected_path_is_refused() {
    let (env, repo) = tamper_repo(Some(DEFAULT_LIST));
    on_att(&env, &repo, |r| fs::remove_file(r.join("verify.sh")).unwrap());
    assert_eq!(touched(&repo), vec!["verify.sh"]);
}

#[test]
fn renaming_a_protected_path_away_is_refused() {
    let (env, repo) = tamper_repo(Some(DEFAULT_LIST));
    env.git(&repo, &["switch", "-q", "att"]);
    env.git(&repo, &["mv", "verify.sh", "src/not-protected.sh"]);
    env.git(&repo, &["mv", "scripts/ci/run.sh", "scripts/run.sh"]);
    env.git(&repo, &["commit", "-q", "-m", "rename away"]);
    env.git(&repo, &["switch", "-q", "main"]);
    // The new names are not protected; the old paths still show (--no-renames).
    assert_eq!(touched(&repo), vec!["scripts/ci/run.sh", "verify.sh"]);
}

#[test]
fn unprotected_paths_pass() {
    let (env, repo) = tamper_repo(Some(DEFAULT_LIST));
    on_att(&env, &repo, |r| {
        for p in ["src/a.rs", "scripts/cix.sh", "verify.sh.bak", ".cpdx/y", ".config/other.toml"] {
            let full = r.join(p);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, "x\n").unwrap();
        }
    });
    assert_eq!(touched(&repo), Vec::<String>::new());
    tamper_check(&repo, "cpd/pay-4521", "cpd/pay-4521", "att").unwrap();
}

#[test]
fn the_list_comes_from_the_lead_branch_never_from_the_attempt() {
    let (env, repo) = tamper_repo(Some(DEFAULT_LIST));
    // The Attempt empties the list and edits verify.sh, and also "protects" its own file.
    on_att(&env, &repo, |r| {
        fs::write(r.join(LIST), "protected = [\"mine.txt\"]\n").unwrap();
        fs::write(r.join("verify.sh"), "weakened\n").unwrap();
        fs::write(r.join("mine.txt"), "mine\n").unwrap();
    });
    let paths = touched(&repo);
    assert!(paths.contains(&"verify.sh".to_owned()), "{paths:?}");
    assert!(paths.contains(&LIST.to_owned()), "{paths:?}");
    assert!(!paths.contains(&"mine.txt".to_owned()), "{paths:?}");
}

#[test]
fn the_list_is_read_at_the_lead_branchs_tip_not_the_bases() {
    let (env, repo) = tamper_repo(Some(DEFAULT_LIST));
    on_att(&env, &repo, |r| fs::write(r.join("secret.txt"), "x\n").unwrap());
    assert_eq!(touched(&repo), Vec::<String>::new());
    // The Lead Branch later protects secret.txt; the diff is still base...att.
    env.git(&repo, &["switch", "-q", "cpd/pay-4521"]);
    commit_file(&env, &repo, LIST, "protected = [\"secret.txt\"]\n");
    env.git(&repo, &["switch", "-q", "main"]);
    let base = env.git(&repo, &["merge-base", "cpd/pay-4521", "att"]);
    assert_eq!(touched_protected(&repo, "cpd/pay-4521", &base, "att").unwrap(), vec!["secret.txt"]);
}

#[test]
fn no_list_file_means_only_the_fixed_protected_paths() {
    let (env, repo) = tamper_repo(None);
    assert_eq!(protected_list(&repo, "cpd/pay-4521").unwrap(), Vec::<String>::new());
    on_att(&env, &repo, |r| {
        fs::write(r.join("verify.sh"), "changed\n").unwrap();
        fs::create_dir_all(r.join(".config")).unwrap();
        fs::write(r.join(".config/wt.toml"), "x = 1\n").unwrap();
    });
    assert_eq!(touched(&repo), vec![".config/wt.toml"]);
}

#[test]
fn a_bad_list_file_is_config_invalid() {
    for text in [
        "protected = \"verify.sh\"\n",
        "protected = [1]\n",
        "protect = [\"verify.sh\"]\n",
        "protected = [\"\"]\n",
        "protected = [\"/abs\"]\n",
        "protected = [\n",
    ] {
        let (_env, repo) = tamper_repo(Some(text));
        let err = protected_list(&repo, "cpd/pay-4521").unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid, "{text}");
        let err = touched_protected(&repo, "cpd/pay-4521", "cpd/pay-4521", "att").unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid, "{text}");
    }
}

#[test]
fn an_unknown_lead_branch_is_git_failed() {
    let (_env, repo) = tamper_repo(Some(DEFAULT_LIST));
    let err = protected_list(&repo, "cpd/no-such").unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
}
