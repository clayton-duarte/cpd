//! The git layer (contract §7.1–§7.3, §7.5) against temporary repos.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::TestEnv;
use cpd_core::error::ErrorCode;
use cpd_core::git::hooks::{HookError, HookOutcome, load, run_hook};
use cpd_core::git::{
    GateHooks, GateWorktree, Operation, Push, Setup, Synced, gate, gate_worktree_path, sweep_gate_worktrees, sync_lead, attempt_branch, attempt_worktree_path, create_attempt_worktree, create_lead_worktree,
    discard_worktree, push_commit, lead_branch, lead_worktree_path, protected_list, remove_merged_attempt, tamper_check,
    touched_protected,
};
use cpd_core::home::Hook;
use cpd_core::ids::{AttemptId, PlanId, PrKey};

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
    env.local_origin(&repo, &origin);
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

// ------------------------------------------------- shared git state (hs-d243)

/// `repo` gets a bare `origin` holding `main`; returns the origin's path.
fn with_origin(env: &TestEnv, repo: &Path) -> PathBuf {
    let origin = env.root().join("origins").join("pay.git");
    fs::create_dir_all(&origin).unwrap();
    env.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    env.git(repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    env.git(repo, &["push", "-q", "origin", "main"]);
    origin
}

/// The `branch.*` entries of `repo`'s `.git/config`, one per line.
fn branch_config(env: &TestEnv, repo: &Path) -> String {
    let out = common::output(env.command("git").args(["-C", repo.to_str().unwrap(), "config", "--get-regexp", r"^branch\."])).unwrap();
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn a_lead_worktree_from_origin_has_no_upstream_and_its_push_writes_no_config() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    with_origin(&env, &repo);

    let made = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "lead.log")).unwrap();

    assert!(made.push_failed.is_none());
    assert_eq!(env.git(&repo, &["ls-remote", "--heads", "origin", &made.branch]), format!("{}\trefs/heads/{}", made.base_sha, made.branch));
    assert_eq!(branch_config(&env, &repo), "", "no upstream: nothing in .git/config");
}

#[test]
fn git_calls_wait_for_a_ref_lock_held_for_one_second() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let origin = with_origin(&env, &repo);
    env.git(&repo, &["fetch", "-q", "origin"]);
    // The remote moves; another git process holds the lock of the repo's tracking ref.
    let other = env.root().join("other");
    env.run_ok(env.root(), "git", &["clone", "-q", origin.to_str().unwrap(), other.to_str().unwrap()]);
    commit_file(&env, &other, "new.txt", "new\n");
    env.git(&other, &["push", "-q", "origin", "main"]);
    let moved = env.git(&other, &["rev-parse", "HEAD"]);
    let lock = repo.join(".git/refs/remotes/origin/main.lock");
    fs::write(&lock, "").unwrap();

    let started = Instant::now();
    let fetched = std::thread::scope(|s| {
        s.spawn(|| {
            // time-claim: git waits out a ref lock held for a second; no per-poll signal from git.
            std::thread::sleep(Duration::from_secs(1));
            fs::remove_file(&lock).unwrap();
        });
        cpd_core::git::run::ok(&repo, &["fetch", "origin", "main"])
    });

    fetched.unwrap();
    assert!(started.elapsed() >= Duration::from_secs(1), "it waited for the lock");
    assert_eq!(env.git(&repo, &["rev-parse", "origin/main"]), moved);
}

#[test]
fn a_fetch_outlasts_a_worktree_add_in_progress() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let origin = with_origin(&env, &repo);
    let other = env.root().join("other");
    env.run_ok(env.root(), "git", &["clone", "-q", origin.to_str().unwrap(), other.to_str().unwrap()]);
    commit_file(&env, &other, "new.txt", "new\n");
    env.git(&other, &["push", "-q", "origin", "main"]);
    // What `git worktree add` leaves for a moment: an entry whose HEAD is the all-zero placeholder.
    let adding = repo.join(".git/worktrees/adding");
    fs::create_dir_all(&adding).unwrap();
    fs::write(adding.join("HEAD"), format!("{}\n", "0".repeat(40))).unwrap();
    fs::write(adding.join("commondir"), "../..\n").unwrap();
    fs::write(adding.join("gitdir"), format!("{}\n", env.root().join("adding/.git").display())).unwrap();
    let broken = common::output(env.command("git").args(["-C", repo.to_str().unwrap(), "fetch", "origin", "main"])).unwrap();
    assert!(String::from_utf8_lossy(&broken.stderr).contains("bad object worktrees/adding/HEAD"), "{broken:?}");

    let fetched = std::thread::scope(|s| {
        s.spawn(|| {
            // time-claim: the fetch outlasts an entry half-created for a second; no per-poll signal from git.
            std::thread::sleep(Duration::from_secs(1));
            fs::remove_dir_all(&adding).unwrap();
        });
        cpd_core::git::run::fetch(&repo, &["origin", "main"])
    });

    fetched.unwrap();
    assert_eq!(env.git(&repo, &["rev-parse", "origin/main"]), env.git(&other, &["rev-parse", "HEAD"]));
    // Any other failure is not retried.
    let started = Instant::now();
    let err = cpd_core::git::run::fetch(&repo, &["origin", "no-such-branch"]).unwrap_err();
    assert_eq!((err.code(), &err.details()["argv"][3]), (ErrorCode::GitFailed, &serde_json::json!("fetch")));
    // time-claim: a failure that is not the race returns at once (20 retries would take over 1.9 s); no per-poll signal from git.
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn rule_p_pushes_the_named_commit_not_the_branch_tip() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let origin = with_origin(&env, &repo);
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "lead.log")).unwrap();
    commit_file(&env, &lead.path, "one.txt", "one\n");
    let one = env.git(&lead.path, &["rev-parse", "HEAD"]);
    commit_file(&env, &lead.path, "two.txt", "two\n");
    let two = env.git(&lead.path, &["rev-parse", "HEAD"]);

    assert_eq!(push_commit(&lead.path, &lead.branch, &one).unwrap(), Push::Pushed { new: true });
    assert_eq!(env.git(&origin, &["rev-parse", &lead.branch]), one, "the named commit, not the tip");
    assert_eq!(push_commit(&lead.path, &lead.branch, &one).unwrap(), Push::Pushed { new: false });
    assert_eq!(push_commit(&lead.path, &lead.branch, &two).unwrap(), Push::Pushed { new: true });
    assert_eq!(env.git(&origin, &["rev-parse", &lead.branch]), two);
    assert_eq!(branch_config(&env, &repo), "", "no -u");
}

#[test]
fn rule_p_an_ancestor_pushed_after_its_descendant_is_a_success_that_pushed_nothing() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let origin = with_origin(&env, &repo);
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "lead.log")).unwrap();
    commit_file(&env, &lead.path, "a.txt", "a\n");
    let a = env.git(&lead.path, &["rev-parse", "HEAD"]);
    commit_file(&env, &lead.path, "b.txt", "b\n");
    let b = env.git(&lead.path, &["rev-parse", "HEAD"]);
    // Merge B pushed first; merge A's push of its own `new` comes late.
    assert_eq!(push_commit(&lead.path, &lead.branch, &b).unwrap(), Push::Pushed { new: true });

    assert_eq!(push_commit(&lead.path, &lead.branch, &a).unwrap(), Push::Contained { remote: b.clone() });
    assert_eq!(env.git(&origin, &["rev-parse", &lead.branch]), b, "the remote is not moved back");
}

#[test]
fn rule_p_a_rejection_whose_remote_tip_lacks_the_commit_stands() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let origin = with_origin(&env, &repo);
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "lead.log")).unwrap();
    // A known commit that does not descend from ours, pushed to the remote branch.
    commit_file(&env, &lead.path, "theirs.txt", "theirs\n");
    let theirs = env.git(&lead.path, &["rev-parse", "HEAD"]);
    env.git(&lead.path, &["push", "-q", "origin", &format!("{theirs}:refs/heads/{}", lead.branch)]);
    env.git(&lead.path, &["reset", "-q", "--hard", &lead.base_sha]);
    commit_file(&env, &lead.path, "ours.txt", "ours\n");
    let ours = env.git(&lead.path, &["rev-parse", "HEAD"]);

    let Push::Rejected { stderr } = push_commit(&lead.path, &lead.branch, &ours).unwrap() else { panic!("not rejected") };
    assert!(stderr.contains("rejected"), "{stderr}");
    assert_eq!(env.git(&origin, &["rev-parse", &lead.branch]), theirs);

    // A remote tip not in the local object store is a rejection too.
    let other = env.root().join("other");
    env.run_ok(env.root(), "git", &["clone", "-q", "-b", &lead.branch, origin.to_str().unwrap(), other.to_str().unwrap()]);
    commit_file(&env, &other, "unknown.txt", "unknown\n");
    env.git(&other, &["push", "-q", "origin", &lead.branch]);
    assert!(matches!(push_commit(&lead.path, &lead.branch, &theirs).unwrap(), Push::Rejected { .. }));
}

#[test]
fn rule_p_without_origin_is_git_failed() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "lead.log")).unwrap();
    let err = push_commit(&lead.path, &lead.branch, &lead.base_sha).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
    assert_eq!(err.details()["argv"][3], "push");
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
fn the_log_has_a_hooks_line_before_the_hook_ends() {
    let h = hooks("pre-merge = \"echo first; sleep 3; echo last\"\n");
    let (repo, log) = (h.repo.clone(), h.log.clone());
    let runner = std::thread::spawn(move || {
        let op = Operation { branch: "main" };
        run_hook(Hook::PreMerge, &repo, Some(&op), Duration::from_secs(30), &log).unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while fs::read_to_string(&h.log).unwrap_or_default() != "first\n" && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let seen = fs::read_to_string(&h.log).unwrap_or_default();
    let ended = runner.is_finished();
    let result = runner.join().unwrap();
    assert_eq!(seen, "first\n", "the log before the hook ended");
    assert!(!ended, "the hook ended before the log was read");
    assert_eq!(result.tail.as_deref(), Some("first\nlast"));
    assert_eq!(fs::read_to_string(&h.log).unwrap(), "first\nlast\n");
}

#[test]
fn a_table_runs_together_waits_for_all_and_fails_with_the_first_in_file_order() {
    // Keys are not in alphabetical order: file order decides, not the table's sort. `zeta`
    // (first in the file) only fails once `slow` (last) has started, so the commands run
    // together; `slow` ends after the others have failed, so it is waited for, not abandoned.
    let h = hooks(
        "[pre-merge]\nzeta = \"until [ -e slow.started ]; do sleep 0.05; done; touch zeta.failed; exit 5\"\nalpha = \"exit 3\"\nslow = \"touch slow.started; until [ -e zeta.failed ]; do sleep 0.05; done; sleep 0.3; echo slow-done\"\n",
    );
    let result = h.run(Hook::PreMerge);
    assert_eq!(result.outcome, HookOutcome::Failed { exit: 5 });
    assert!(result.tail.unwrap().contains("slow │ slow-done"));
}

#[test]
fn commands_of_a_table_run_concurrently() {
    // Each command waits until all three have started: run one after another, the first
    // never sees the others and the hook times out.
    let rendezvous = |me: &str| {
        format!(
            "touch {me}.started; until [ -e a.started ] && [ -e b.started ] && [ -e c.started ]; do sleep 0.05; done"
        )
    };
    let h = hooks(&format!(
        "[pre-merge]\na = \"{}\"\nb = \"{}\"\nc = \"{}\"\n",
        rendezvous("a"),
        rendezvous("b"),
        rendezvous("c")
    ));
    assert_eq!(h.run_for(Hook::PreMerge, Duration::from_secs(20)).outcome, HookOutcome::Passed);
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

/// Dead means no such process or state Z. The hook's sh is gone, so a background child is
/// an orphan: after SIGKILL it is a zombie (`kill -0` still succeeds) until init reaps it,
/// and on a loaded runner that window is visible.
fn is_dead(pid: &str) -> bool {
    let out = std::process::Command::new("ps").args(["-o", "stat=", "-p", pid]).output().unwrap();
    let state = String::from_utf8(out.stdout).unwrap();
    state.trim().is_empty() || state.trim().starts_with('Z')
}

#[test]
fn a_hook_is_killed_with_its_process_group_at_the_timeout() {
    // The background child would touch survivor.txt after 10 s; the kill must end it long
    // before, and the test waits for its death rather than for the clock.
    let h = hooks("pre-merge = \"(sleep 10; touch survivor.txt) & echo $! > pid; wait\"\n");
    let result = h.run_for(Hook::PreMerge, Duration::from_secs(2));
    assert_eq!(result.outcome, HookOutcome::Error(HookError::Timeout));
    let pid = h.file("pid").trim().to_owned();
    common::wait_until("the background child to die with the hook's process group", || is_dead(&pid));
    assert!(!h.repo.join("survivor.txt").exists(), "the background child outlived the kill");
}

#[test]
fn a_background_child_of_a_finished_hook_is_killed_when_the_hook_returns() {
    let h = hooks("pre-merge = \"sleep 30 & echo $! > pid\"\n");
    assert_eq!(h.run(Hook::PreMerge).outcome, HookOutcome::Passed);
    let pid = h.file("pid").trim().to_owned();
    // The contract is that the child is dead, not that its pid is already free: wait a
    // bounded time for the kill to land.
    common::wait_until("the background child to die when the hook returns", || is_dead(&pid));
}

#[test]
fn a_command_killed_by_a_signal_is_an_error_not_a_failure() {
    let h = hooks("pre-merge = \"echo started; kill -9 $$\"\n");
    let result = h.run(Hook::PreMerge);
    assert_eq!(result.outcome, HookOutcome::Error(HookError::Signal(9)));
    assert_eq!(result.tail.as_deref(), Some("started"));
}

/// Set (by the test below, on the child's `Command`) in the one process whose environment holds
/// the cockpit's variables; the helper does nothing without it.
const HOOK_ENV_GATE: &str = "CPD_GITTEST_HOOK_ENV_HELPER";

/// Not a test of its own: the child process of the test below, whose environment is the cockpit's.
#[test]
fn hook_env_helper() {
    if std::env::var_os(HOOK_ENV_GATE).is_none() {
        return;
    }
    let h = hooks("pre-merge = \"env > env.txt\"\n");
    assert_eq!(h.run(Hook::PreMerge).outcome, HookOutcome::Passed);
    let env = h.file("env.txt");
    for gone in ["TMUX=", "TMUX_PANE=", "PI_GITTEST=", "CPD_GITTEST="] {
        assert!(!env.lines().any(|l| l.starts_with(gone)), "{gone} leaked");
    }
    assert!(env.lines().any(|l| l == "GITTEST_KEEP=kept"));
}

#[test]
fn cockpit_pi_and_cpd_variables_do_not_reach_hooks() {
    // The variables are on a child's `Command`, never in this multi-threaded process's environment.
    let out = common::output(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "git::hook_env_helper", "--test-threads=1"])
            .env(HOOK_ENV_GATE, "x")
            .env("TMUX", "/tmp/fake,1,0")
            .env("TMUX_PANE", "%9")
            .env("PI_GITTEST", "x")
            .env("CPD_GITTEST", "x")
            .env("GITTEST_KEEP", "kept"),
    )
    .unwrap();
    assert!(
        out.status.success() && String::from_utf8_lossy(&out.stdout).contains("1 passed"),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
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
    touched_protected(repo, "cpd/pay-4521", "main", "cpd/pay-4521", "att").unwrap()
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
        let err = tamper_check(&repo, "cpd/pay-4521", "main", "cpd/pay-4521", "att").unwrap_err();
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
    tamper_check(&repo, "cpd/pay-4521", "main", "cpd/pay-4521", "att").unwrap();
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
    assert_eq!(touched_protected(&repo, "cpd/pay-4521", "main", &base, "att").unwrap(), vec!["secret.txt"]);
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
        let err = touched_protected(&repo, "cpd/pay-4521", "main", "cpd/pay-4521", "att").unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalid, "{text}");
    }
}

#[test]
fn an_unknown_lead_branch_is_git_failed() {
    let (_env, repo) = tamper_repo(Some(DEFAULT_LIST));
    let err = protected_list(&repo, "cpd/no-such").unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
}

// A base merge (§7.5): a protected path whose version on the Attempt is the Lead tip's or the
// base's is allowed.

/// `tamper_repo` plus a commit on `main` writing `path` = `content` (`None`: removing it),
/// which `att` merges; `cpd/pay-4521` stays where it was, so the base is ahead of the Lead tip.
fn base_merge_repo(path: &str, content: Option<&str>) -> (TestEnv, PathBuf) {
    let (env, repo) = tamper_repo(Some(DEFAULT_LIST));
    match content {
        Some(content) => {
            commit_file(&env, &repo, path, content);
        }
        None => {
            env.git(&repo, &["rm", "-q", path]);
            env.git(&repo, &["commit", "-q", "-m", "remove"]);
        }
    }
    env.git(&repo, &["switch", "-q", "att"]);
    env.git(&repo, &["merge", "-q", "--no-ff", "-m", "merge the base", "main"]);
    env.git(&repo, &["switch", "-q", "main"]);
    (env, repo)
}

#[test]
fn a_protected_path_the_attempt_has_as_the_base_has_it_is_allowed() {
    for path in [".config/wt.toml", ".cpd/x", "verify.sh", "scripts/ci/run.sh"] {
        let (_env, repo) = base_merge_repo(path, Some("from the base\n"));
        assert_eq!(touched_protected(&repo, "cpd/pay-4521", "main", "cpd/pay-4521", "att").unwrap(), Vec::<String>::new(), "{path}");
        tamper_check(&repo, "cpd/pay-4521", "main", "cpd/pay-4521", "att").unwrap();
    }
}

#[test]
fn the_repos_own_list_file_is_handled_like_the_others() {
    let (env, repo) = base_merge_repo(LIST, Some("protected = [\"verify.sh\", \"x/\"]\n"));
    assert_eq!(touched_protected(&repo, "cpd/pay-4521", "main", "cpd/pay-4521", "att").unwrap(), Vec::<String>::new());
    // Changed again to anything else, it is tampered.
    env.git(&repo, &["switch", "-q", "att"]);
    commit_file(&env, &repo, LIST, "protected = []\n");
    env.git(&repo, &["switch", "-q", "main"]);
    assert_eq!(touched(&repo), vec![LIST]);
    assert_eq!(touched_protected(&repo, "cpd/pay-4521", "main", "cpd/pay-4521", "att").unwrap(), vec![LIST]);
}

#[test]
fn a_protected_path_changed_to_anything_but_the_bases_or_the_lead_tips_is_tampered() {
    let (env, repo) = base_merge_repo(".config/wt.toml", Some("from the base\n"));
    env.git(&repo, &["switch", "-q", "att"]);
    commit_file(&env, &repo, ".config/wt.toml", "mine\n");
    env.git(&repo, &["switch", "-q", "main"]);
    assert_eq!(touched(&repo), vec![".config/wt.toml"]);
}

#[test]
fn a_protected_path_that_matches_the_lead_tip_is_allowed() {
    // The Lead tip gained the file and the Attempt, cut earlier, has the same version.
    let (env, repo) = tamper_repo(Some(DEFAULT_LIST));
    env.git(&repo, &["switch", "-q", "cpd/pay-4521"]);
    commit_file(&env, &repo, ".config/wt.toml", "lead\n");
    env.git(&repo, &["switch", "-q", "att"]);
    env.git(&repo, &["merge", "-q", "--no-ff", "-m", "merge the lead", "cpd/pay-4521"]);
    env.git(&repo, &["switch", "-q", "main"]);
    let base = env.git(&repo, &["merge-base", "main", "att"]);
    assert_eq!(touched_protected(&repo, "cpd/pay-4521", "main", &base, "att").unwrap(), Vec::<String>::new());
}

#[test]
fn a_protected_file_deleted_on_the_base_and_in_the_attempt_is_allowed() {
    for path in ["verify.sh", ".config/cpd.toml"] {
        let (_env, repo) = base_merge_repo(path, None);
        assert_eq!(touched_protected(&repo, "cpd/pay-4521", "main", "cpd/pay-4521", "att").unwrap(), Vec::<String>::new(), "{path}");
    }
}

#[test]
fn a_protected_file_deleted_in_the_attempt_alone_is_tampered() {
    let (env, repo) = tamper_repo(Some(DEFAULT_LIST));
    on_att(&env, &repo, |r| fs::remove_file(r.join("verify.sh")).unwrap());
    assert_eq!(touched(&repo), vec!["verify.sh"]);
}

#[test]
fn base_ref_is_origin_slash_base_only_with_an_origin() {
    let (env, repo) = tamper_repo(None);
    assert_eq!(cpd_core::git::tamper::base_ref(&repo, "main").unwrap(), "main");
    env.git(&repo, &["remote", "add", "origin", "https://example.invalid/pay.git"]);
    assert_eq!(cpd_core::git::tamper::base_ref(&repo, "main").unwrap(), "origin/main");
}

// ---------------------------------------------------------------- rule S

/// A Lead worktree on `cpd/pay-4521` and a scratch worktree that makes the
/// commits a merge would fast-forward the Lead Branch to.
struct Moving {
    env: TestEnv,
    repo: PathBuf,
    lead: PathBuf,
    branch: String,
    scratch: PathBuf,
}

fn moving() -> Moving {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let made = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "lead.log")).unwrap();
    let scratch = env.root().join("scratch");
    env.git(&repo, &["worktree", "add", "-q", "--detach", scratch.to_str().unwrap(), &made.branch]);
    Moving { env, repo, lead: made.path, branch: made.branch, scratch }
}

impl Moving {
    /// A merge's step 8: a commit that writes `path` on top of the Lead Branch,
    /// then the compare-and-swap of the ref. The Lead worktree is not touched.
    fn merge(&self, path: &str, content: &str) -> String {
        let reference = format!("refs/heads/{}", self.branch);
        let old = self.env.git(&self.repo, &["rev-parse", &reference]);
        self.env.git(&self.scratch, &["checkout", "-q", "--detach", &old]);
        commit_file(&self.env, &self.scratch, path, content);
        let new = self.env.git(&self.scratch, &["rev-parse", "HEAD"]);
        self.env.git(&self.repo, &["update-ref", "-m", "cpd: merge test", &reference, &new, &old]);
        new
    }

    fn assert_at_tip_and_clean(&self, tip: &str) {
        assert_eq!(self.env.git(&self.lead, &["rev-parse", "HEAD"]), tip);
        assert_eq!(self.env.git(&self.lead, &["status", "--porcelain"]), "", "the Lead worktree holds the tip's tree");
    }
}

#[test]
fn rule_s_two_merges_synced_in_the_reverse_order_end_at_the_tip_and_clean() {
    let m = moving();
    let _first = m.merge("a.txt", "a\n");
    let second = m.merge("b.txt", "b\n");

    // The second merge's sync runs first, then the first merge's.
    assert_eq!(sync_lead(&m.lead, &m.branch).unwrap(), Synced::Done);
    assert_eq!(sync_lead(&m.lead, &m.branch).unwrap(), Synced::Done);

    m.assert_at_tip_and_clean(&second);
    assert_eq!(fs::read_to_string(m.lead.join("a.txt")).unwrap(), "a\n");
    assert_eq!(fs::read_to_string(m.lead.join("b.txt")).unwrap(), "b\n");
}

#[test]
fn rule_s_two_syncs_at_once_both_end_with_the_lead_worktree_at_the_tip() {
    let m = moving();
    m.merge("a.txt", "a\n");
    let tip = m.merge("b.txt", "b\n");

    let results = std::thread::scope(|s| {
        let one = s.spawn(|| sync_lead(&m.lead, &m.branch));
        let two = s.spawn(|| sync_lead(&m.lead, &m.branch));
        [one.join().unwrap(), two.join().unwrap()]
    });

    for result in results {
        assert_eq!(result.unwrap(), Synced::Done);
    }
    m.assert_at_tip_and_clean(&tip);
}

#[test]
fn rule_s_keeps_a_staged_edit_and_reports_it_stale() {
    let m = moving();
    fs::write(m.lead.join("README.md"), "staged by the Lead\n").unwrap();
    m.env.git(&m.lead, &["add", "README.md"]);
    let tip = m.merge("a.txt", "a\n");

    let err = sync_lead(&m.lead, &m.branch).unwrap_err();

    assert_eq!(err.code(), ErrorCode::LeadWorktreeStale);
    assert_eq!(err.details()["worktree"], m.lead.display().to_string());
    // Nothing was overwritten: the edit is still staged and on disk, and the tip's file is not checked out.
    assert_eq!(m.env.git(&m.lead, &["diff", "--cached", "--name-only"]), "README.md\na.txt");
    assert_eq!(m.env.git(&m.lead, &["show", ":README.md"]), "staged by the Lead");
    assert_eq!(fs::read_to_string(m.lead.join("README.md")).unwrap(), "staged by the Lead\n");
    assert!(!m.lead.join("a.txt").exists());
    assert_eq!(m.env.git(&m.lead, &["rev-parse", "HEAD"]), tip, "the ref move stands");
}

#[test]
fn rule_s_keeps_an_unstaged_edit_and_reports_it_stale() {
    let m = moving();
    fs::write(m.lead.join("README.md"), "edited by the Lead\n").unwrap();
    m.merge("a.txt", "a\n");

    let err = sync_lead(&m.lead, &m.branch).unwrap_err();

    assert_eq!(err.code(), ErrorCode::LeadWorktreeStale);
    assert_eq!(fs::read_to_string(m.lead.join("README.md")).unwrap(), "edited by the Lead\n");
    assert!(!m.lead.join("a.txt").exists());
}

#[test]
fn rule_s_waits_out_an_index_lock_held_for_one_second() {
    let m = moving();
    let tip = m.merge("a.txt", "a\n");
    let lock = PathBuf::from(m.env.git(&m.lead, &["rev-parse", "--path-format=absolute", "--git-path", "index.lock"]));
    fs::write(&lock, "").unwrap();

    let started = Instant::now();
    let synced = std::thread::scope(|s| {
        s.spawn(|| {
            // time-claim: rule S waits out an index lock held for a second; no per-poll signal from git.
            std::thread::sleep(Duration::from_secs(1));
            fs::remove_file(&lock).unwrap();
        });
        sync_lead(&m.lead, &m.branch)
    });

    assert_eq!(synced.unwrap(), Synced::Done);
    assert!(started.elapsed() >= Duration::from_secs(1), "it waited for the lock");
    m.assert_at_tip_and_clean(&tip);
}

#[test]
fn rule_s_on_an_up_to_date_lead_worktree_is_done_and_touches_nothing() {
    let m = moving();
    let tip = m.env.git(&m.lead, &["rev-parse", "HEAD"]);
    assert_eq!(sync_lead(&m.lead, &m.branch).unwrap(), Synced::Done);
    m.assert_at_tip_and_clean(&tip);
}

// ------------------------------------------------------- gate worktree

fn key() -> PrKey {
    PrKey::parse("api").unwrap()
}

fn gate_hooks<'a>(env: &TestEnv, lead_branch: &'a str) -> GateHooks<'a> {
    GateHooks {
        lead_branch,
        base: "main",
        pre_start: setup(env, "pr-api.pre-start.log"),
        pre_merge: setup(env, "pr-api.pre-merge.log"),
    }
}

/// A Lead worktree whose tip `G` carries `wt` as `.config/wt.toml`, and a later
/// tip whose `.config/wt.toml` would fail every hook. Returns `(repo, lead, G)`.
fn gated(env: &TestEnv, wt: &str) -> (PathBuf, cpd_core::git::Created, String) {
    let repo = env.git_repo("pay");
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup(&env, "lead.log")).unwrap();
    commit_file(env, &lead.path, ".config/wt.toml", wt);
    let g = env.git(&lead.path, &["rev-parse", "HEAD"]);
    commit_file(env, &lead.path, ".config/wt.toml", "pre-start = \"exit 9\"\npre-merge = \"exit 9\"\n");
    (repo, lead, g)
}

fn worktrees(env: &TestEnv, repo: &Path) -> String {
    env.git(repo, &["worktree", "list", "--porcelain"])
}

#[test]
fn the_gate_runs_pre_start_then_pre_merge_in_a_detached_worktree_at_g_and_removes_it() {
    let env = TestEnv::new();
    let out = env.root().join("out");
    fs::create_dir_all(&out).unwrap();
    let wt = format!(
        "pre-start = \"touch started; echo {{{{ branch }}}} {{{{ base }}}} {{{{ worktree_path }}}} $(git rev-parse HEAD) > {0}/start.txt\"\n\
         pre-merge = \"test -f started && echo {{{{ branch }}}} {{{{ target }}}} $(git rev-parse HEAD) $(git symbolic-ref -q HEAD || echo detached) > {0}/merge.txt\"\n",
        out.display()
    );
    let (repo, lead, g) = gated(&env, &wt);

    let gated = gate(&repo, &plan(), &key(), &g, &gate_hooks(&env, &lead.branch)).unwrap();

    assert_eq!(gated, Ok(()));
    let w = gate_worktree_path(&repo, &plan(), &key(), std::process::id()).unwrap();
    assert_eq!(w, repo.parent().unwrap().join(format!("pay.pay-4521.api.gate-{}", std::process::id())));
    let w_real = repo.parent().unwrap().canonicalize().unwrap().join(w.file_name().unwrap());
    assert_eq!(fs::read_to_string(out.join("start.txt")).unwrap(), format!("cpd/pay-4521 main {} {g}\n", w_real.display()));
    assert_eq!(fs::read_to_string(out.join("merge.txt")).unwrap(), format!("cpd/pay-4521 main {g} detached\n"));
    assert!(!w.exists(), "the gate worktree is removed");
    assert!(!worktrees(&env, &repo).contains(".gate-"));
    assert_eq!(branch_config(&env, &repo), "", "no .git/config write");
}

#[test]
fn a_failing_pre_start_refuses_the_gate_without_pre_merge_and_removes_the_worktree() {
    let env = TestEnv::new();
    let marker = env.root().join("pre-merge-ran");
    let wt = format!("pre-start = \"echo no setup; exit 4\"\npre-merge = \"touch {}\"\n", marker.display());
    let (repo, lead, g) = gated(&env, &wt);
    let hooks = gate_hooks(&env, &lead.branch);

    let refused = gate(&repo, &plan(), &key(), &g, &hooks).unwrap().unwrap_err();

    assert_eq!(refused.hook, Hook::PreStart);
    assert_eq!(refused.outcome, HookOutcome::Failed { exit: 4 });
    assert_eq!(refused.tail.as_deref(), Some("no setup"));
    assert_eq!(refused.log, hooks.pre_start.log);
    assert!(!marker.exists(), "pre-merge did not run");
    assert!(!worktrees(&env, &repo).contains(".gate-"));
}

#[test]
fn a_failing_pre_merge_refuses_the_gate_and_removes_the_worktree() {
    let env = TestEnv::new();
    let (repo, lead, g) = gated(&env, "pre-merge = \"echo red; exit 6\"\n");
    let hooks = gate_hooks(&env, &lead.branch);

    let refused = gate(&repo, &plan(), &key(), &g, &hooks).unwrap().unwrap_err();

    assert_eq!(refused.hook, Hook::PreMerge);
    assert_eq!(refused.outcome, HookOutcome::Failed { exit: 6 });
    assert_eq!(refused.tail.as_deref(), Some("red"));
    assert_eq!(fs::read_to_string(&hooks.pre_merge.log).unwrap(), "red\n");
    assert!(!worktrees(&env, &repo).contains(".gate-"));
}

/// The pid of a process that has ended.
fn dead_pid(env: &TestEnv) -> u32 {
    let mut child = common::spawn(&mut env.command("true")).unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

#[test]
fn a_gate_worktree_add_outlasts_a_concurrent_worktree_add_in_progress() {
    let env = TestEnv::new();
    let (repo, _lead, g) = gated(&env, "");
    // What a concurrent `git worktree add` leaves for a moment: an entry whose `commondir` is empty.
    let adding = repo.join(".git/worktrees/adding");
    fs::create_dir_all(&adding).unwrap();
    fs::write(adding.join("gitdir"), format!("{}\n", env.root().join("adding/.git").display())).unwrap();
    fs::write(adding.join("commondir"), "").unwrap();
    let path = gate_worktree_path(&repo, &plan(), &key(), std::process::id()).unwrap();
    let broken = common::output(env.command("git").args(["-C", repo.to_str().unwrap(), "worktree", "add", "--detach", path.to_str().unwrap(), &g])).unwrap();
    assert!(String::from_utf8_lossy(&broken.stderr).contains("worktrees/adding/commondir: Success"), "{broken:?}");

    let added = std::thread::scope(|s| {
        s.spawn(|| {
            // time-claim: the add outlasts an entry that stays half-created for a second; no per-poll signal from git.
            std::thread::sleep(Duration::from_secs(1));
            fs::remove_dir_all(&adding).unwrap();
        });
        GateWorktree::add(&repo, &plan(), &key(), &g)
    });

    let gate = added.unwrap();
    assert!(gate.path().exists());
    gate.remove().unwrap();
    // Any other failure is not retried.
    let err = GateWorktree::add(&repo, &plan(), &key(), "no-such-commit").unwrap_err();
    assert_eq!((err.code(), &err.details()["argv"][3]), (ErrorCode::GitFailed, &serde_json::json!("worktree")));
}

/// The reproduction of the Gate's race: `ROUNDS` rounds of `PARALLEL` `worktree add --detach --lock`
/// into one repo at once (before the retry 5, 6 and 15 of 1600 adds failed in three runs).
fn parallel_adds(env: &TestEnv, repo: &Path, sha: &str, parallel: usize, rounds: usize, add: impl Sync + Fn(&Path, &[&str], &Path) -> Result<String, cpd_core::error::CpdError>) -> usize {
    let mut failed = 0;
    let add = &add;
    for round in 0..rounds {
        let paths: Vec<PathBuf> = (0..parallel).map(|i| env.root().join(format!("race-{round}-{i}"))).collect();
        failed += std::thread::scope(|s| {
            let adds: Vec<_> = paths
                .iter()
                .map(|p| s.spawn(move || add(repo, &["--detach", "--lock", "--reason", "race", p.to_str().unwrap(), sha], p)))
                .collect();
            adds.into_iter().map(|a| a.join().unwrap()).filter(Result::is_err).count()
        });
        for p in &paths {
            let _ = cpd_core::git::run::run(repo, &["worktree", "remove", "--force", "--force", p.to_str().unwrap()]); // a failed add left none
        }
    }
    failed
}

#[test]
fn parallel_worktree_adds_into_one_repo_all_succeed() {
    let env = TestEnv::new();
    let (repo, _lead, g) = gated(&env, "");
    assert_eq!(parallel_adds(&env, &repo, &g, 16, 40, cpd_core::git::run::worktree_add), 0);
}

#[test]
fn the_sweep_removes_the_prs_gate_worktrees_of_dead_pids_only() {
    let env = TestEnv::new();
    let (repo, _lead, g) = gated(&env, "");
    let dead = dead_pid(&env);
    let add = |key: &PrKey, pid: u32| {
        let path = gate_worktree_path(&repo, &plan(), key, pid).unwrap();
        env.git(&repo, &["worktree", "add", "-q", "--detach", "--lock", "--reason", "cpd gate", path.to_str().unwrap(), &g]);
        path
    };
    let leftover = add(&key(), dead);
    let other_pr = add(&PrKey::parse("web").unwrap(), dead);
    let live = GateWorktree::add(&repo, &plan(), &key(), &g).unwrap();
    assert!(worktrees(&env, &repo).contains("locked cpd gate"));

    let removed = sweep_gate_worktrees(&repo, &plan(), &key()).unwrap();

    assert_eq!(removed.len(), 1, "{removed:?}");
    assert_eq!(removed[0].file_name(), leftover.file_name());
    assert!(!leftover.exists());
    assert!(other_pr.exists(), "another PR's leftover is its own sweep's");
    assert!(live.path().exists(), "a live pid's gate worktree is left alone");
    // A locked leftover whose directory is gone is swept too.
    fs::remove_dir_all(&other_pr).unwrap();
    assert_eq!(sweep_gate_worktrees(&repo, &plan(), &PrKey::parse("web").unwrap()).unwrap().len(), 1);
    live.remove().unwrap();
    assert!(!worktrees(&env, &repo).contains(".gate-"));
}

// ------------------------------------------------------------ deadline (contract §1.2, §7)

/// Whether process `pid` is running (a zombie is not).
fn running(pid: i32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.rsplit(") ").next().is_some_and(|r| !r.starts_with('Z')))
}

/// engine-W4, commands-W1: a git call that hangs is killed at its deadline with its
/// whole process group, and fails `tool_timeout`. The deadline is shortened to 1 s by
/// `CPD_TEST_DEADLINE_GIT`, which a debug build reads from its environment: this test
/// runs itself again in a child process born with it, so no other test sees it.
#[test]
fn a_git_call_past_its_deadline_is_killed_with_its_group_and_is_tool_timeout() {
    const NAME: &str = "a_git_call_past_its_deadline_is_killed_with_its_group_and_is_tool_timeout";
    if std::env::var_os("CPD_TEST_DEADLINE_GIT").is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", &format!("git::{NAME}"), "--nocapture"]).env("CPD_TEST_DEADLINE_GIT", "1");
        let out = common::output(&mut child).unwrap();
        let printed = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && printed.contains("1 passed"), "{printed}");
        return;
    }
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let pidfile = env.root().join("sleep.pid");
    // A real git whose alias runs a sleeper in git's process group, and waits for it.
    env.git(&repo, &["config", "alias.hang", &format!("!sleep {} & echo $! > '{}'; wait", common::SLEEP_SECONDS, pidfile.display())]);

    let started = std::time::Instant::now();
    let err = cpd_core::git::run::run(&repo, &["hang"]).unwrap_err();

    // time-claim: the call returns at its 1 s deadline, not when the sleeper ends; no per-poll signal from the deadline.
    assert!(started.elapsed() < Duration::from_secs(4), "took {:?}", started.elapsed());
    assert_eq!(err.code(), ErrorCode::ToolTimeout, "{err:?}");
    let d = err.details();
    assert_eq!(d["tool"], "git");
    assert_eq!(d["deadline_seconds"], 1);
    assert_eq!(d["argv"], serde_json::json!(["git", "-C", repo.display().to_string(), "hang"]));
    let pid: i32 = fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    common::wait_until("git's child to die with its process group", || !running(pid));
}
