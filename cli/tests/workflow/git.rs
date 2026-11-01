//! The git layer (contract §7.1–§7.3, §7.5) against temporary repos.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

use common::TestEnv;
use common::fx::Fx;
use cpd_core::error::ErrorCode;
use cpd_core::fakes::git::{GitRoute, Repo};
use cpd_core::git::hooks::{HookError, HookOutcome, Pipeline, run_hook, run_hook_with};
use cpd_core::git::{
    Operation, Push, Setup, Synced, attempt_branch, attempt_worktree_path, create_attempt_worktree,
    create_attempt_worktree_with, create_lead_worktree, create_lead_worktree_with,
    discard_worktree, lead_worktree_path, legacy_lead_branch, push_commit, remove_merged_attempt,
    sync_lead, tamper_check, touched_protected,
};
use cpd_core::home::Hook;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::ports::Ports;

fn plan() -> PlanId {
    PlanId::parse("pay-4521").unwrap()
}

fn attempt() -> AttemptId {
    AttemptId::parse("api-a1").unwrap()
}

/// The pipeline of `hook` in `wt`, written as a `.config/wt.toml` was (`pre-start = "cmd"`, `[pre-commit]`,
/// `[[pre-merge]]`): read as the `[hooks]` table of a repo policy file is. `Err` is the loader's `policy_invalid`.
fn try_pipeline(wt: &str, hook: Hook) -> Result<Pipeline, cpd_core::error::CpdError> {
    let scope = cpd_core::policy::parse_scope(
        Path::new("pay.toml"),
        &format!("[hooks]\n{wt}")
            .replace("\n[", "\n[hooks.")
            .replace("[hooks.[", "[[hooks."),
    )?;
    let hooks = scope.hooks.unwrap_or_default();
    Ok(match hook {
        Hook::PreStart => hooks.pre_start,
        Hook::PreCommit => hooks.pre_commit,
        Hook::PreMerge => hooks.pre_merge,
    }
    .unwrap_or_default())
}

fn pipeline(wt: &str, hook: Hook) -> Pipeline {
    try_pipeline(wt, hook).unwrap()
}

// ------------------------------------------------------------ worktrees

// In-process: the repos are those of the fake git of an `Fx` (a `Universe`), the library is called on its ports
// (`Fx::scoped`), and the `sh -c` of a `pre-start` is a script on the `FakeRunner`: what a test reads of a hook is the
// `Call` it got (its command, rendered, and its directory), not a file a shell wrote.

/// The `pre-start` hooks answer `reply`, whatever their command.
fn pre_start_replies(fx: &Fx, reply: Reply) {
    fx.fake("sh", reply);
}

/// The `Call`s of the hooks' `sh`: `(command, directory)`, in order.
fn sh_calls(fx: &Fx) -> Vec<(String, Option<PathBuf>)> {
    fx.calls("sh")
        .into_iter()
        .map(|call| (call.args[1].clone(), call.cwd))
        .collect()
}

#[test]
fn lead_worktree_is_a_sibling_on_cpd_plan_cut_from_the_base() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let pre_start = pipeline(
        "[pre-start]\nmark = \"echo {{ branch }} {{ base }} > setup-ran\"\n",
        Hook::PreStart,
    );
    let main_sha = fx.git(&repo, &["rev-parse", "main"]);
    pre_start_replies(&fx, Reply::ok(""));

    let made = fx
        .scoped(|| {
            create_lead_worktree_with(
                &repo,
                &plan(),
                "main",
                &setup_in(&fx, "lead.log"),
                &pre_start,
            )
        })
        .unwrap();

    assert_eq!(made.path, repo.parent().unwrap().join("pay.pay-4521"));
    assert_eq!(made.path, lead_worktree_path(&repo, &plan()).unwrap());
    assert_eq!(made.branch, "cpd/pay-4521");
    assert_eq!(made.branch, legacy_lead_branch(&plan()));
    assert_eq!(made.base_sha, main_sha);
    assert_eq!(
        fx.git(&made.path, &["symbolic-ref", "--short", "HEAD"]),
        "cpd/pay-4521"
    );
    assert_eq!(fx.git(&made.path, &["rev-parse", "HEAD"]), main_sha);
    // pre-start ran in it, with its variables: the command `sh` got is rendered, in the new worktree.
    assert_eq!(
        sh_calls(&fx),
        [(
            "echo cpd/pay-4521 main > setup-ran".to_owned(),
            Some(made.path.canonicalize().unwrap())
        )]
    );
    // The main checkout is untouched.
    assert_eq!(fx.git(&repo, &["symbolic-ref", "--short", "HEAD"]), "main");
    assert_eq!(fx.git(&repo, &["status", "--porcelain"]), "");
}

#[test]
fn lead_worktree_is_cut_from_a_freshly_fetched_origin_base() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let origin = fx.env.root().join("origins").join("pay.git");
    fs::create_dir_all(&origin).unwrap();
    fx.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    fx.git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    fx.git(&repo, &["push", "-q", "origin", "main"]);
    // Someone else advances origin/main; the repo's own refs are stale.
    let other = fx.env.root().join("other");
    fx.git(
        fx.env.root(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    fx.commit_file(&other, "new.txt", "new\n", "edit new.txt");
    fx.git(&other, &["push", "-q", "origin", "main"]);
    let pushed = fx.git(&other, &["rev-parse", "HEAD"]);

    let made = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup_in(&fx, "lead.log")))
        .unwrap();

    assert_eq!(made.base_sha, pushed);
    assert!(made.path.join("new.txt").exists());
    assert_ne!(
        fx.git(&repo, &["rev-parse", "main"]),
        pushed,
        "the local base is not moved"
    );
}

#[test]
fn a_failing_pre_start_removes_the_worktree_and_branch() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let pre_start = pipeline("pre-start = \"echo no luck; exit 3\"\n", Hook::PreStart);
    let setup = setup_in(&fx, "lead.log");
    pre_start_replies(&fx, Reply::exit(3, "no luck\n", ""));

    let err = fx
        .scoped(|| create_lead_worktree_with(&repo, &plan(), "main", &setup, &pre_start))
        .unwrap_err();

    assert_eq!(err.code(), ErrorCode::SetupFailed);
    assert_eq!(err.details()["tail"], "no luck");
    assert_eq!(err.details()["log"], setup.log.to_str().unwrap());
    assert!(!lead_worktree_path(&repo, &plan()).unwrap().exists());
    assert_eq!(fx.git(&repo, &["branch", "--list", "cpd/*"]), "");
    assert_eq!(fs::read_to_string(&setup.log).unwrap(), "no luck\n");
}

#[test]
fn an_invalid_pre_start_is_setup_failed_too() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let pre_start = pipeline("pre-start = \"echo {{ nope }}\"\n", Hook::PreStart);
    let err = fx
        .scoped(|| {
            create_lead_worktree_with(
                &repo,
                &plan(),
                "main",
                &setup_in(&fx, "lead.log"),
                &pre_start,
            )
        })
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::SetupFailed);
    assert!(!lead_worktree_path(&repo, &plan()).unwrap().exists());
    assert_eq!(fx.git(&repo, &["branch", "--list", "cpd/*"]), "");
    assert!(
        fx.calls("sh").is_empty(),
        "the command did not render: nothing ran"
    );
}

#[test]
fn lead_worktree_refuses_what_is_in_the_way() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let path = lead_worktree_path(&repo, &plan()).unwrap();

    fs::create_dir_all(&path).unwrap();
    let err = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup_in(&fx, "l.log")))
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::WorktreeExists);
    fs::remove_dir(&path).unwrap();

    fx.git(&repo, &["branch", "cpd/pay-4521"]);
    let err = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup_in(&fx, "l.log")))
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::BranchExists);
    assert!(!path.exists());
}

#[test]
fn lead_worktree_replaces_the_leftover_of_an_interrupted_lock() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let path = lead_worktree_path(&repo, &plan()).unwrap();
    fx.git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "cpd/pay-4521",
            path.to_str().unwrap(),
            "main",
        ],
    );
    fs::write(path.join("junk"), "left behind").unwrap();

    let made = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup_in(&fx, "l.log")))
        .unwrap();

    assert_eq!(made.path, path);
    assert!(!path.join("junk").exists());
    assert_eq!(
        fx.git(&path, &["symbolic-ref", "--short", "HEAD"]),
        "cpd/pay-4521"
    );
}

#[test]
fn attempt_worktree_is_cut_from_the_lead_branch_head() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let pre_start = pipeline(
        "[pre-start]\nmark = \"echo {{ branch }} {{ base }} {{ base_worktree_path }} > setup-ran\"\n",
        Hook::PreStart,
    );
    let setup = setup_in(&fx, "attempt.log");
    let lead = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup))
        .unwrap();
    // The Lead Branch has moved on from main.
    fx.commit_file(&lead.path, "lead.txt", "lead\n", "edit lead.txt");
    let lead_sha = fx.git(&lead.path, &["rev-parse", "HEAD"]);
    pre_start_replies(&fx, Reply::ok(""));

    let made = fx
        .scoped(|| {
            create_attempt_worktree_with(&repo, &lead.path, &plan(), &attempt(), &setup, &pre_start)
        })
        .unwrap();

    assert_eq!(
        made.path,
        repo.parent().unwrap().join("pay.pay-4521--api-a1")
    );
    assert_eq!(
        made.path,
        attempt_worktree_path(&repo, &plan(), &attempt()).unwrap()
    );
    assert_eq!(made.branch, "cpd/pay-4521--api-a1");
    assert_eq!(made.branch, attempt_branch(&plan(), &attempt()));
    assert_eq!(made.base_sha, lead_sha);
    assert_eq!(fx.git(&made.path, &["rev-parse", "HEAD"]), lead_sha);
    assert_eq!(
        fx.git(&made.path, &["symbolic-ref", "--short", "HEAD"]),
        made.branch
    );
    // The command `sh` got is rendered with the variables, in the new worktree.
    let command = format!(
        "echo cpd/pay-4521--api-a1 cpd/pay-4521 {} > setup-ran",
        lead.path.canonicalize().unwrap().display()
    );
    assert_eq!(
        sh_calls(&fx),
        [(command, Some(made.path.canonicalize().unwrap()))]
    );
}

#[test]
fn a_failing_attempt_pre_start_removes_the_worktree_and_branch() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let setup = setup_in(&fx, "attempt.log");
    let lead = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup))
        .unwrap();
    let pre_start = pipeline("pre-start = \"exit 1\"\n", Hook::PreStart);
    pre_start_replies(&fx, Reply::exit(1, "", ""));

    let err = fx
        .scoped(|| {
            create_attempt_worktree_with(&repo, &lead.path, &plan(), &attempt(), &setup, &pre_start)
        })
        .unwrap_err();

    assert_eq!(err.code(), ErrorCode::SetupFailed);
    assert!(
        !attempt_worktree_path(&repo, &plan(), &attempt())
            .unwrap()
            .exists()
    );
    assert_eq!(fx.git(&repo, &["branch", "--list", "cpd/pay-4521--*"]), "");
    assert_eq!(fx.git(&repo, &["symbolic-ref", "--short", "HEAD"]), "main");
}

#[test]
fn discard_removes_whatever_state_and_is_idempotent() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let setup = setup_in(&fx, "attempt.log");
    let lead = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup))
        .unwrap();
    let made = fx
        .scoped(|| create_attempt_worktree(&repo, &lead.path, &plan(), &attempt(), &setup))
        .unwrap();
    fx.commit_file(&made.path, "work.txt", "unmerged\n", "edit work.txt");
    fs::write(made.path.join("dirty.txt"), "dirty").unwrap();

    fx.scoped(|| discard_worktree(&lead.path, &made.path, &made.branch))
        .unwrap();
    fx.scoped(|| discard_worktree(&lead.path, &made.path, &made.branch))
        .unwrap();

    assert!(!made.path.exists());
    assert_eq!(fx.git(&repo, &["branch", "--list", &made.branch]), "");
}

// ------------------------------------------------- shared git state (hs-d243)

/// `repo` gets a bare `origin` holding `main`; returns the origin's path.
fn with_origin(fx: &Fx, repo: &Path) -> PathBuf {
    let origin = fx.env.root().join("origins").join("pay.git");
    fs::create_dir_all(&origin).unwrap();
    fx.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    fx.git(repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    fx.git(repo, &["push", "-q", "origin", "main"]);
    origin
}

#[test]
fn a_failed_lead_branch_push_names_the_first_stderr_line_in_its_message() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    with_origin(&fx, &repo);
    let stderr = "Connection closed by 140.82.112.4 port 22\nfatal: Could not read from remote repository.\n";
    let _ = (*fx.fakes.runner).clone().once(
        "git",
        |c| c.args.iter().any(|a| a == "push"),
        Reply::exit(128, "", stderr),
    );

    let made = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup_in(&fx, "lead.log")))
        .unwrap();

    let e = made.push_failed.expect("the push failed");
    assert_eq!(e.code(), ErrorCode::PushFailed);
    assert!(
        e.message()
            .contains("Connection closed by 140.82.112.4 port 22"),
        "{}",
        e.message()
    );
    assert_eq!(e.details()["stderr"].as_str().unwrap(), stderr.trim_end());
}

/// The `branch.*` entries of `repo`'s `.git/config`, one per line (exit 1 and no output when there is none).
fn branch_config(fx: &Fx, repo: &Path) -> String {
    String::from_utf8(
        fx.git_output(repo, &["config", "--get-regexp", r"^branch\."])
            .stdout,
    )
    .unwrap()
}

#[test]
fn a_lead_worktree_from_origin_has_no_upstream_and_its_push_writes_no_config() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    with_origin(&fx, &repo);

    let made = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup_in(&fx, "lead.log")))
        .unwrap();

    assert!(made.push_failed.is_none());
    assert_eq!(
        fx.git(&repo, &["ls-remote", "--heads", "origin", &made.branch]),
        format!("{}\trefs/heads/{}", made.base_sha, made.branch)
    );
    assert_eq!(
        branch_config(&fx, &repo),
        "",
        "no upstream: nothing in .git/config"
    );
}

/// Scripts the next `n` calls of git that `is` accepts: each exits 128 with `stderr`.
fn fail_git(fx: &Fx, n: usize, is: fn(&Call) -> bool, stderr: &str) {
    let mut runner = (*fx.fakes.runner).clone();
    for _ in 0..n {
        runner = runner.once("git", is, Reply::exit(128, "", stderr));
    }
}

fn is_fetch(call: &Call) -> bool {
    call.args.iter().any(|a| a == "fetch")
}

fn is_worktree_list(call: &Call) -> bool {
    call.args.windows(2).any(|w| w == ["worktree", "list"])
}

/// The calls of git `is` accepts, in order.
fn git_calls(fx: &Fx, is: fn(&Call) -> bool) -> Vec<Call> {
    fx.calls("git").into_iter().filter(is).collect()
}

/// Every nap of `fx`'s clock is one retry round's pause: 100-500 ms (contract §7).
fn assert_retry_naps(fx: &Fx, rounds: usize) {
    let naps = fx.fakes.clock.naps();
    assert_eq!(naps.len(), rounds, "{naps:?}");
    assert!(
        naps.iter()
            .all(|n| (Duration::from_millis(100)..=Duration::from_millis(500)).contains(n)),
        "{naps:?}"
    );
}

#[test]
fn git_calls_wait_for_a_ref_lock_held_for_one_second() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    // Another git process holds the lock of the repo's tracking ref: git waits it out inside the one call (the wait
    // is git's, bounded by the `core.filesRefLockTimeout` that every call of ours carries), so the call takes a second.
    let _ = (*fx.fakes.runner).clone().once(
        "git",
        is_fetch,
        Reply::ok("").takes(Duration::from_secs(1)),
    );

    let fetched = fx.scoped(|| cpd_core::git::run::ok(&repo, &["fetch", "origin", "main"]));

    fetched.unwrap();
    assert!(
        fx.fakes.clock.elapsed() >= Duration::from_secs(1),
        "it waited for the lock"
    );
    // One call: a held lock is not a race to retry, and the call is allowed the ten seconds of the lock timeouts.
    let fetches = git_calls(&fx, is_fetch);
    assert_eq!(fetches.len(), 1);
    assert_eq!(
        fetches[0].args,
        ["-C", repo.to_str().unwrap(), "fetch", "origin", "main"]
    );
    assert_eq!(
        fx.fakes.clock.naps(),
        [Duration::from_secs(1)],
        "the one wait is git's own, no retry pause"
    );
    let has = |k: &str, v: &str| {
        fetches[0]
            .envs
            .contains(&(k.to_owned(), Some(v.to_owned())))
    };
    assert!(has("GIT_CONFIG_COUNT", "2"));
    assert!(
        has("GIT_CONFIG_KEY_0", "core.filesRefLockTimeout") && has("GIT_CONFIG_VALUE_0", "10000")
    );
    assert!(
        has("GIT_CONFIG_KEY_1", "core.packedRefsTimeout") && has("GIT_CONFIG_VALUE_1", "10000")
    );
}

#[test]
fn a_fetch_outlasts_a_worktree_add_in_progress() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let origin = fx.env.root().join("origins").join("pay.git");
    fs::create_dir_all(&origin).unwrap();
    fx.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    fx.git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    fx.git(&repo, &["push", "-q", "origin", "main"]);
    let other = fx.env.root().join("other");
    fx.git(
        fx.env.root(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    fx.commit_file(&other, "new.txt", "new\n", "edit new.txt");
    fx.git(&other, &["push", "-q", "origin", "main"]);
    // What `git worktree add` leaves for a moment: an entry whose HEAD is the all-zero placeholder, on which the
    // fetch's connectivity check dies, until the entry is whole.
    fail_git(
        &fx,
        3,
        is_fetch,
        "fatal: bad object worktrees/adding/HEAD\nerror: https://example.invalid did not send all necessary objects\n",
    );

    let fetched = fx.scoped(|| cpd_core::git::run::fetch(&repo, &["origin", "main"]));

    fetched.unwrap();
    assert_eq!(
        git_calls(&fx, is_fetch).len(),
        4,
        "three rounds on the race, the fourth fetched"
    );
    assert_retry_naps(&fx, 3);
    assert_eq!(
        fx.git(&repo, &["rev-parse", "origin/main"]),
        fx.git(&other, &["rev-parse", "HEAD"])
    );
    // Any other failure is not retried.
    let before = (git_calls(&fx, is_fetch).len(), fx.fakes.clock.naps());
    let err = fx
        .scoped(|| cpd_core::git::run::fetch(&repo, &["origin", "no-such-branch"]))
        .unwrap_err();
    assert_eq!(
        (err.code(), &err.details()["argv"][3]),
        (ErrorCode::GitFailed, &serde_json::json!("fetch"))
    );
    // time-claim replaced: a failure that is not the race returns at once, with no nap (20 retries would nap 19 times).
    assert_eq!(
        (git_calls(&fx, is_fetch).len(), fx.fakes.clock.naps()),
        (before.0 + 1, before.1)
    );
}

#[test]
fn rule_p_pushes_the_named_commit_not_the_branch_tip() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let origin = with_origin(&fx, &repo);
    let lead = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup_in(&fx, "lead.log")))
        .unwrap();
    fx.commit_file(&lead.path, "one.txt", "one\n", "edit one.txt");
    let one = fx.git(&lead.path, &["rev-parse", "HEAD"]);
    fx.commit_file(&lead.path, "two.txt", "two\n", "edit two.txt");
    let two = fx.git(&lead.path, &["rev-parse", "HEAD"]);

    assert_eq!(
        fx.scoped(|| push_commit(&lead.path, &lead.branch, &one))
            .unwrap(),
        Push::Pushed { new: true }
    );
    assert_eq!(
        fx.git(&origin, &["rev-parse", &lead.branch]),
        one,
        "the named commit, not the tip"
    );
    assert_eq!(
        fx.scoped(|| push_commit(&lead.path, &lead.branch, &one))
            .unwrap(),
        Push::Pushed { new: false }
    );
    assert_eq!(
        fx.scoped(|| push_commit(&lead.path, &lead.branch, &two))
            .unwrap(),
        Push::Pushed { new: true }
    );
    assert_eq!(fx.git(&origin, &["rev-parse", &lead.branch]), two);
    assert_eq!(branch_config(&fx, &repo), "", "no -u");
}

#[test]
fn rule_p_an_ancestor_pushed_after_its_descendant_is_a_success_that_pushed_nothing() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let origin = with_origin(&fx, &repo);
    let lead = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup_in(&fx, "lead.log")))
        .unwrap();
    fx.commit_file(&lead.path, "a.txt", "a\n", "edit a.txt");
    let a = fx.git(&lead.path, &["rev-parse", "HEAD"]);
    fx.commit_file(&lead.path, "b.txt", "b\n", "edit b.txt");
    let b = fx.git(&lead.path, &["rev-parse", "HEAD"]);
    // Merge B pushed first; merge A's push of its own `new` comes late.
    assert_eq!(
        fx.scoped(|| push_commit(&lead.path, &lead.branch, &b))
            .unwrap(),
        Push::Pushed { new: true }
    );

    assert_eq!(
        fx.scoped(|| push_commit(&lead.path, &lead.branch, &a))
            .unwrap(),
        Push::Contained { remote: b.clone() }
    );
    assert_eq!(
        fx.git(&origin, &["rev-parse", &lead.branch]),
        b,
        "the remote is not moved back"
    );
}

#[test]
fn rule_p_a_rejection_whose_remote_tip_lacks_the_commit_stands() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let origin = with_origin(&fx, &repo);
    let lead = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup_in(&fx, "lead.log")))
        .unwrap();
    // A known commit that does not descend from ours, pushed to the remote branch.
    fx.commit_file(&lead.path, "theirs.txt", "theirs\n", "edit theirs.txt");
    let theirs = fx.git(&lead.path, &["rev-parse", "HEAD"]);
    fx.git(
        &lead.path,
        &[
            "push",
            "-q",
            "origin",
            &format!("{theirs}:refs/heads/{}", lead.branch),
        ],
    );
    fx.git(&lead.path, &["reset", "-q", "--hard", &lead.base_sha]);
    fx.commit_file(&lead.path, "ours.txt", "ours\n", "edit ours.txt");
    let ours = fx.git(&lead.path, &["rev-parse", "HEAD"]);

    let Push::Rejected { stderr } = fx
        .scoped(|| push_commit(&lead.path, &lead.branch, &ours))
        .unwrap()
    else {
        panic!("not rejected")
    };
    assert!(stderr.contains("rejected"), "{stderr}");
    assert_eq!(fx.git(&origin, &["rev-parse", &lead.branch]), theirs);

    // A remote tip not in the local object store is a rejection too.
    let other = fx.env.root().join("other");
    fx.git(
        fx.env.root(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    fx.git(
        &other,
        &[
            "switch",
            "-q",
            "-c",
            &lead.branch,
            &format!("origin/{}", lead.branch),
        ],
    );
    fx.commit_file(&other, "unknown.txt", "unknown\n", "edit unknown.txt");
    fx.git(&other, &["push", "-q", "origin", &lead.branch]);
    assert!(matches!(
        fx.scoped(|| push_commit(&lead.path, &lead.branch, &theirs))
            .unwrap(),
        Push::Rejected { .. }
    ));
}

#[test]
fn rule_p_without_origin_is_git_failed() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let lead = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup_in(&fx, "lead.log")))
        .unwrap();
    let err = fx
        .scoped(|| push_commit(&lead.path, &lead.branch, &lead.base_sha))
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
    assert_eq!(err.details()["argv"][3], "push");
}

// ---------------------------------------------------------------- hooks

struct Hooks {
    /// Holds the temporary directory of `repo`.
    _env: TestEnv,
    repo: PathBuf,
    log: PathBuf,
    /// The hooks, written as a `.config/wt.toml` was and read as the `[hooks]` of a repo policy file.
    wt: String,
}

fn hooks(wt: &str) -> Hooks {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    let log = env.root().join("logs").join("h.log");
    Hooks {
        _env: env,
        repo,
        log,
        wt: wt.to_owned(),
    }
}

impl Hooks {
    fn run(&self, hook: Hook) -> cpd_core::git::HookResult {
        self.run_for(hook, Duration::from_secs(30))
    }

    fn run_for(&self, hook: Hook, timeout: Duration) -> cpd_core::git::HookResult {
        let op = Operation { branch: "main" };
        run_hook(
            hook,
            &pipeline(&self.wt, hook),
            &self.repo,
            Some(&op),
            timeout,
            &self.log,
        )
        .unwrap()
    }

    fn file(&self, name: &str) -> String {
        fs::read_to_string(self.repo.join(name)).unwrap()
    }
}

// The hooks below run in this process: `run_hook_with` on the `Ports` of an `Fx`, the git calls that build the
// variables answered by the fake git (a `Repo` of a `Universe`), the hooks' `sh -c` by a script on the `FakeRunner`.
// No file a hook would write is written: what a test reads is the `Call`s of `sh` and what the script answered.

/// What a script of `sh` answers, by the command (`sh -c <command>`).
struct Sh(Box<dyn Fn(&str) -> Reply + Send + Sync>);

impl Handler for Sh {
    fn handle(&self, call: &Call) -> Reply {
        (self.0)(&call.args[1])
    }
}

struct Hooked {
    fx: Fx,
    repo: Repo,
    route: Arc<GitRoute>,
    log: PathBuf,
    /// The hooks, written as a `.config/wt.toml` was and read as the `[hooks]` of a repo policy file.
    wt: String,
}

/// A repo `pay` on `main` with one commit, in the fake git of an `Fx` on fake ports.
fn hooked(wt: &str) -> Hooked {
    let fx = Fx::with_ports();
    let route = fx.git_route.clone();
    let root = fx.env.root().canonicalize().unwrap();
    let repo = Repo::init(&fx.universe, &root.join("pay"));
    repo.write("README.md", "pay\n");
    repo.commit("init");
    let log = root.join("logs").join("h.log");
    Hooked {
        fx,
        repo,
        route,
        log,
        wt: wt.to_owned(),
    }
}

impl Drop for Hooked {
    fn drop(&mut self) {
        self.route.assert_supported();
    }
}

impl Hooked {
    /// The hooks answer `reply`, whatever their command.
    fn sh(&self, reply: Reply) {
        self.fx.fake("sh", reply);
    }

    /// The hooks answer by their command: `script(command)`.
    fn script(&self, script: impl Fn(&str) -> Reply + Send + Sync + 'static) {
        let _ = (*self.fx.fakes.runner)
            .clone()
            .route("sh", Arc::new(Sh(Box::new(script))));
    }

    /// The commands the hooks were run with (`sh -c <command>`), in the order they were called.
    fn commands(&self) -> Vec<String> {
        self.fx
            .calls("sh")
            .iter()
            .map(|call| call.args[1].clone())
            .collect()
    }

    fn run(&self, hook: Hook) -> cpd_core::git::HookResult {
        self.run_for(hook, Duration::from_secs(30))
    }

    fn run_for(&self, hook: Hook, timeout: Duration) -> cpd_core::git::HookResult {
        self.run_table(&self.wt, hook, timeout)
    }

    /// The hook `hook` of the `[hooks]` text `wt`, in the repo.
    fn run_table(&self, wt: &str, hook: Hook, timeout: Duration) -> cpd_core::git::HookResult {
        let operation = Operation { branch: "main" };
        let ports = &self.fx.fakes.ports;
        run_hook_with(
            ports,
            hook,
            &pipeline(wt, hook),
            self.repo.dir(),
            Some(&operation),
            timeout,
            &self.log,
        )
        .unwrap()
    }
}

#[test]
fn no_file_and_no_table_mean_no_commands_and_the_hook_passes() {
    let h = hooked("");
    let run = |wt: &str| {
        let ports = &h.fx.fakes.ports;
        run_hook_with(
            ports,
            Hook::PreCommit,
            &pipeline(wt, Hook::PreCommit),
            h.repo.dir(),
            None,
            Duration::from_secs(5),
            &h.log,
        )
        .unwrap()
    };

    let result = run("");
    assert_eq!(result.outcome, HookOutcome::Passed);
    assert_eq!(result.tail, None);

    // R20: no pre-commit table means no quick check; pre-merge does not run instead.
    let result = run("[pre-merge]\ngate = \"exit 1\"\n");
    assert_eq!(result.outcome, HookOutcome::Passed);
    assert_eq!(result.tail, None);
    assert!(!h.log.exists());
    assert!(
        h.fx.fakes.runner.calls().is_empty(),
        "nothing was run, not even a git call for the variables"
    );
}

#[test]
fn a_passing_hook_has_its_output_logged_and_returned() {
    let h = hooked("pre-commit = \"echo out; echo err >&2\"\n");
    h.sh(Reply::exit(0, "out\n", "err\n"));
    let result = h.run(Hook::PreCommit);
    assert_eq!(result.outcome, HookOutcome::Passed);
    assert_eq!(result.tail.as_deref(), Some("out\nerr"));
    assert_eq!(fs::read_to_string(&h.log).unwrap(), "out\nerr\n");
    assert_eq!(h.commands(), ["echo out; echo err >&2"]);
}

#[test]
fn a_failing_hook_reports_its_exit_status_and_output() {
    let h = hooked("pre-merge = \"echo broken; exit 7\"\n");
    h.sh(Reply::exit(7, "broken\n", ""));
    let result = h.run(Hook::PreMerge);
    assert_eq!(
        result.outcome,
        HookOutcome::Failed {
            exit: 7,
            command: "echo broken; exit 7".to_owned()
        }
    );
    assert_eq!(result.tail.as_deref(), Some("broken"));
}

#[test]
fn a_failing_hook_reports_its_rendered_command_not_the_template() {
    let h = hooked("pre-merge = \"echo {{ branch }}; exit 4\"\n");
    h.sh(Reply::exit(4, "", ""));
    let HookOutcome::Failed { exit, command } = h.run(Hook::PreMerge).outcome else {
        panic!("the hook fails")
    };
    assert_eq!(exit, 4);
    assert_eq!(command, h.commands()[0]);
    assert!(!command.contains("{{"), "{command}");
}

#[test]
fn only_the_last_60_lines_are_returned_and_the_log_has_all() {
    let h = hooked("pre-merge = \"seq 1 100\"\n");
    h.sh(Reply::ok(
        &(1..=100).map(|n| format!("{n}\n")).collect::<String>(),
    ));
    let result = h.run(Hook::PreMerge);
    let tail = result.tail.unwrap();
    assert_eq!(tail.lines().count(), 60);
    assert_eq!(tail.lines().next(), Some("41"));
    assert_eq!(tail.lines().last(), Some("100"));
    assert_eq!(fs::read_to_string(&h.log).unwrap().lines().count(), 100);
}

#[test]
fn a_table_runs_together_waits_for_all_and_fails_with_the_first_in_file_order() {
    // Keys are not in alphabetical order: file order decides, not the table's sort. `zeta` (first in the file) only
    // fails once `slow` (last) has started, so the commands run together: run one after another, the first would wait
    // for the second for ever. `slow` ends after the others have failed, so it is waited for, not abandoned.
    let h = hooked("[pre-merge]\nzeta = \"zeta\"\nalpha = \"alpha\"\nslow = \"slow\"\n");
    let (slow_started, zeta_failed) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
    h.script({
        let (slow_started, zeta_failed) = (slow_started.clone(), zeta_failed.clone());
        move |command| match command {
            "zeta" => {
                slow_started.wait();
                zeta_failed.wait();
                Reply::exit(5, "", "")
            }
            "alpha" => Reply::exit(3, "", ""),
            _ => {
                slow_started.wait();
                zeta_failed.wait();
                Reply::ok("slow-done\n")
            }
        }
    });
    let result = h.run(Hook::PreMerge);
    assert_eq!(
        result.outcome,
        HookOutcome::Failed {
            exit: 5,
            command: "zeta".to_owned()
        }
    );
    assert!(result.tail.unwrap().contains("slow │ slow-done"));
}

#[test]
fn commands_of_a_table_run_concurrently() {
    // Each command waits until all three have started: run one after another, the first never sees the others.
    let h = hooked("[pre-merge]\na = \"a\"\nb = \"b\"\nc = \"c\"\n");
    let started = Arc::new(Barrier::new(3));
    h.script(move |_| {
        started.wait();
        Reply::ok("")
    });
    assert_eq!(
        h.run_for(Hook::PreMerge, Duration::from_secs(20)).outcome,
        HookOutcome::Passed
    );
    assert_eq!(h.fx.calls("sh").len(), 3);
}

#[test]
fn a_pipeline_runs_its_blocks_in_order_and_a_failing_block_stops_it() {
    let h = hooked(
        "[[pre-merge]]\none = \"echo one > one.txt\"\n\n[[pre-merge]]\ntwo = \"test -f one.txt && echo two > two.txt\"\nfail = \"exit 4\"\n\n[[pre-merge]]\nthree = \"echo three > three.txt\"\n",
    );
    h.script(|command| {
        if command == "exit 4" {
            Reply::exit(4, "", "")
        } else {
            Reply::ok("")
        }
    });
    let result = h.run(Hook::PreMerge);
    assert_eq!(
        result.outcome,
        HookOutcome::Failed {
            exit: 4,
            command: "exit 4".to_owned()
        }
    );
    let commands = h.commands();
    // The second block started after the first ended (`two` reads the file `one` writes), and ran whole.
    assert_eq!(commands[0], "echo one > one.txt");
    let mut second = commands[1..].to_vec();
    second.sort();
    assert_eq!(second, ["exit 4", "test -f one.txt && echo two > two.txt"]);
    assert_eq!(
        commands.len(),
        3,
        "the third block did not run: {commands:?}"
    );
}

#[test]
fn output_of_concurrent_commands_is_prefixed_with_their_names() {
    let h = hooked("[pre-merge]\nlint = \"echo lint-ok\"\ntest = \"echo test-ok\"\n");
    h.script(|command| Reply::ok(&format!("{}\n", command.trim_start_matches("echo "))));
    let tail = h.run(Hook::PreMerge).tail.unwrap();
    let mut lines: Vec<&str> = tail.lines().collect();
    lines.sort();
    assert_eq!(lines, vec!["lint │ lint-ok", "test │ test-ok"]);
}

#[test]
fn an_undefined_variable_or_worktrunk_filter_fails_before_anything_runs() {
    let h = hooked("");
    for bad in [
        "{{ nope }}",
        "{{ branch | sanitize }}",
        "{{ vars.x }}",
        "{{ target_worktree_path_of('x') }}",
    ] {
        let wt = format!("[pre-merge]\nfirst = \"touch ran.txt\"\nsecond = \"echo {bad}\"\n");
        let result = h.run_table(&wt, Hook::PreMerge, Duration::from_secs(30));
        assert!(
            matches!(result.outcome, HookOutcome::Error(HookError::Invalid(_))),
            "{bad}: {result:?}"
        );
        assert_eq!(result.tail, None);
        assert!(h.fx.calls("sh").is_empty(), "{bad}: a command ran");
    }
}

#[test]
fn a_malformed_hooks_table_is_policy_invalid() {
    for text in ["pre-merge = [", "pre-merge = 3\n", "[pre-merge]\nx = 1\n"] {
        let err = try_pipeline(text, Hook::PreMerge).unwrap_err();
        assert_eq!(err.code(), ErrorCode::PolicyInvalid, "{text}");
    }
}

#[test]
fn a_wt_toml_in_the_tree_is_not_read() {
    let h = hooked("pre-commit = \"echo from the policy\"\n");
    h.repo
        .write(".config/wt.toml", "pre-commit = \"echo from the tree\"\n");
    h.script(|command| Reply::ok(&format!("{}\n", command.trim_start_matches("echo "))));
    assert_eq!(
        h.run(Hook::PreCommit).tail.as_deref(),
        Some("from the policy")
    );
    assert_eq!(h.commands(), ["echo from the policy"]);
}

#[test]
fn a_command_killed_by_a_signal_is_an_error_not_a_failure() {
    let h = hooked("pre-merge = \"echo started; kill -9 $$\"\n");
    h.sh(Reply::signal(9, "started\n", ""));
    let result = h.run(Hook::PreMerge);
    assert_eq!(result.outcome, HookOutcome::Error(HookError::Signal(9)));
    assert_eq!(result.tail.as_deref(), Some("started"));
}

#[test]
fn cockpit_pi_and_cpd_variables_do_not_reach_hooks() {
    // The variables are in this process's environment, which `Fx` puts back; the hook's call removes each of them
    // (`Call.envs`: `None`), and leaves the others to be inherited (`GITTEST_KEEP` is not named).
    let h = hooked("pre-merge = \"env > env.txt\"\n");
    for (key, value) in [
        ("TMUX", "/tmp/fake,1,0"),
        ("TMUX_PANE", "%9"),
        ("PI_GITTEST", "x"),
        ("CPD_GITTEST", "x"),
        ("GITTEST_KEEP", "kept"),
    ] {
        h.fx.export(key, value);
    }
    h.sh(Reply::ok(""));
    assert_eq!(h.run(Hook::PreMerge).outcome, HookOutcome::Passed);
    let envs = h.fx.calls("sh")[0].envs.clone();
    for gone in ["TMUX", "TMUX_PANE", "PI_GITTEST", "CPD_GITTEST"] {
        assert!(
            envs.contains(&(gone.to_owned(), None)),
            "{gone} leaked: {envs:?}"
        );
    }
    assert!(
        envs.iter().all(|(key, _)| key != "GITTEST_KEEP"),
        "GITTEST_KEEP is the caller's to inherit: {envs:?}"
    );
}

#[test]
fn the_commands_of_a_hook_can_be_listed_in_file_order() {
    let wt = "[[pre-commit]]\nz = \"first {{ branch }}\"\n\n[[pre-commit]]\nb = \"second\"\na = \"third\"\n";
    let commands: Vec<_> = pipeline(wt, Hook::PreCommit)
        .commands()
        .map(|c| (c.name.clone().unwrap(), c.template.clone()))
        .collect();
    assert_eq!(
        commands,
        vec![
            ("z".to_owned(), "first {{ branch }}".to_owned()),
            ("b".to_owned(), "second".to_owned()),
            ("a".to_owned(), "third".to_owned())
        ]
    );
}

#[test]
fn commands_get_the_variables_as_a_template_and_as_json_on_stdin() {
    let h = hooked(
        "[pre-merge]\necho = \"echo {{ hook_type }} {{ hook_name }} {{ branch }} {{ target }} {{ target_worktree_path }} > echo.txt\"\njson = \"cat > ctx.json\"\n",
    );
    h.sh(Reply::ok(""));
    let sha = h.fx.git(h.repo.dir(), &["rev-parse", "HEAD"]);
    assert_eq!(h.run(Hook::PreMerge).outcome, HookOutcome::Passed);

    let repo = h.repo.dir().canonicalize().unwrap();
    let calls = h.fx.calls("sh");
    let by = |command: &str| {
        calls
            .iter()
            .find(|call| call.args[1] == command)
            .unwrap_or_else(|| panic!("no call of {command:?}: {calls:?}"))
    };
    // The template is rendered with the variables before `sh` gets it.
    assert_eq!(
        by(&format!(
            "echo pre-merge echo main main {} > echo.txt",
            repo.display()
        ))
        .args[0],
        "-c"
    );
    // The same variables, as JSON, on the stdin of every command.
    let stdin = by("cat > ctx.json")
        .stdin
        .clone()
        .expect("the command has the context on its stdin");
    let ctx: serde_json::Value = serde_json::from_slice(&stdin).unwrap();
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
    let h = hooked("pre-commit = \"pwd > pwd.txt; echo $0 > shell.txt\"\n");
    h.sh(Reply::ok(""));
    assert_eq!(h.run(Hook::PreCommit).outcome, HookOutcome::Passed);
    let calls = h.fx.calls("sh");
    assert_eq!(calls.len(), 1);
    // `sh -c <command>` in the worktree root.
    assert_eq!(calls[0].program.rsplit('/').next(), Some("sh"));
    assert_eq!(calls[0].args, ["-c", "pwd > pwd.txt; echo $0 > shell.txt"]);
    assert_eq!(
        calls[0].cwd.as_deref(),
        Some(h.repo.dir().canonicalize().unwrap().as_path())
    );
}

#[test]
fn target_worktree_path_is_undefined_when_no_worktree_has_the_target() {
    let h = hooked("pre-merge = \"echo {{ target_worktree_path }}\"\n");
    let operation = Operation {
        branch: "no-such-branch",
    };
    let ports = &h.fx.fakes.ports;
    let result = run_hook_with(
        ports,
        Hook::PreMerge,
        &pipeline(&h.wt, Hook::PreMerge),
        h.repo.dir(),
        Some(&operation),
        Duration::from_secs(5),
        &h.log,
    )
    .unwrap();
    assert!(matches!(
        result.outcome,
        HookOutcome::Error(HookError::Invalid(_))
    ));
    assert!(
        h.fx.calls("sh").is_empty(),
        "nothing ran: the command did not render"
    );
}

/// Dead means no such process or state Z. The hook's sh is gone, so a background child is
/// an orphan: after SIGKILL it is a zombie (`kill -0` still succeeds) until init reaps it,
/// and on a loaded runner that window is visible.
fn is_dead(pid: &str) -> bool {
    let out = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .unwrap();
    let state = String::from_utf8(out.stdout).unwrap();
    state.trim().is_empty() || state.trim().starts_with('Z')
}

#[test]
fn a_background_child_of_a_finished_hook_is_killed_when_the_hook_returns() {
    let h = hooks("pre-merge = \"sleep 30 & echo $! > pid\"\n");
    assert_eq!(h.run(Hook::PreMerge).outcome, HookOutcome::Passed);
    let pid = h.file("pid").trim().to_owned();
    // The contract is that the child is dead, not that its pid is already free: wait a
    // bounded time for the kill to land.
    common::wait_until("the background child to die when the hook returns", || {
        is_dead(&pid)
    });
}

// --------------------------------------------------------------- tamper

// In-process: the repos are those of the fake git of an `Fx`, the library is called on its ports (`Fx::scoped`).

/// A repo on `main` with `verify.sh` and `scripts/ci/run.sh` committed (the protected paths of [`list`]),
/// and the branches `cpd/pay-4521` (the Lead Branch) and `att`, both at `main`.
fn tamper_repo() -> (Fx, PathBuf) {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    fx.commit_file(&repo, "verify.sh", "#!/bin/sh\n", "edit verify.sh");
    fx.commit_file(&repo, "scripts/ci/run.sh", "ci\n", "edit scripts/ci/run.sh");
    fx.commit_file(&repo, "src/a.rs", "a\n", "edit src/a.rs");
    fx.git(&repo, &["branch", "cpd/pay-4521"]);
    fx.git(&repo, &["branch", "att"]);
    (fx, repo)
}

/// The PR's `protected.paths`, from its policy.
fn list() -> Vec<String> {
    vec!["verify.sh".to_owned(), "scripts/ci/".to_owned()]
}

/// Commits `edit` on branch `att` (changing the tree), then returns to `main`.
fn on_att(fx: &Fx, repo: &Path, edit: impl FnOnce(&Path)) {
    fx.git(repo, &["switch", "-q", "att"]);
    edit(repo);
    fx.git(repo, &["add", "-A"]);
    fx.git(repo, &["commit", "-q", "-m", "attempt"]);
    fx.git(repo, &["switch", "-q", "main"]);
}

fn touched(fx: &Fx, repo: &Path) -> Vec<String> {
    fx.scoped(|| {
        touched_protected(repo, "cpd/pay-4521", &list(), "main", "cpd/pay-4521", "att").unwrap()
    })
}

#[test]
fn deleting_a_protected_path_is_refused() {
    let (fx, repo) = tamper_repo();
    on_att(&fx, &repo, |r| {
        fs::remove_file(r.join("verify.sh")).unwrap()
    });
    assert_eq!(touched(&fx, &repo), vec!["verify.sh"]);
}

#[test]
fn renaming_a_protected_path_away_is_refused() {
    let (fx, repo) = tamper_repo();
    fx.git(&repo, &["switch", "-q", "att"]);
    fx.git(&repo, &["mv", "verify.sh", "src/not-protected.sh"]);
    fx.git(&repo, &["mv", "scripts/ci/run.sh", "scripts/run.sh"]);
    fx.git(&repo, &["commit", "-q", "-m", "rename away"]);
    fx.git(&repo, &["switch", "-q", "main"]);
    // The new names are not protected; the old paths still show (--no-renames).
    assert_eq!(touched(&fx, &repo), vec!["scripts/ci/run.sh", "verify.sh"]);
}

#[test]
fn each_protected_path_is_refused() {
    let paths = [
        ".cpd/anything",
        ".cpd/deep/er/file",
        "verify.sh",
        "scripts/ci/run.sh",
        "scripts/ci/new/nested.sh",
    ];
    // One repo: each case starts `att` again at the Lead tip, so it holds that case's change only.
    let (fx, repo) = tamper_repo();
    for path in paths {
        fx.git(&repo, &["update-ref", "refs/heads/att", "cpd/pay-4521"]);
        on_att(&fx, &repo, |r| {
            let full = r.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, "changed\n").unwrap();
        });
        assert_eq!(touched(&fx, &repo), vec![path.to_owned()], "{path}");
        let err = fx
            .scoped(|| {
                tamper_check(
                    &repo,
                    "cpd/pay-4521",
                    &list(),
                    "main",
                    "cpd/pay-4521",
                    "att",
                )
            })
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::GateTampered, "{path}");
        assert_eq!(err.details()["paths"], serde_json::json!([path]));
    }
}

#[test]
fn unprotected_paths_pass() {
    let (fx, repo) = tamper_repo();
    on_att(&fx, &repo, |r| {
        for p in [
            "src/a.rs",
            "scripts/cix.sh",
            "verify.sh.bak",
            ".cpdx/y",
            ".config/other.toml",
        ] {
            let full = r.join(p);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, "x\n").unwrap();
        }
    });
    assert_eq!(touched(&fx, &repo), Vec::<String>::new());
    fx.scoped(|| {
        tamper_check(
            &repo,
            "cpd/pay-4521",
            &list(),
            "main",
            "cpd/pay-4521",
            "att",
        )
    })
    .unwrap();
}

#[test]
fn the_list_is_the_policys_never_the_attempts() {
    let (fx, repo) = tamper_repo();
    // The Attempt edits verify.sh and also "protects" its own file by writing a list in its tree.
    on_att(&fx, &repo, |r| {
        fs::create_dir_all(r.join(".config")).unwrap();
        fs::write(r.join(".config/cpd.toml"), "protected = [\"mine.txt\"]\n").unwrap();
        fs::write(r.join("verify.sh"), "weakened\n").unwrap();
        fs::write(r.join("mine.txt"), "mine\n").unwrap();
    });
    assert_eq!(touched(&fx, &repo), vec!["verify.sh"]);
}

#[test]
fn no_list_means_only_the_cpd_directory() {
    let (fx, repo) = tamper_repo();
    on_att(&fx, &repo, |r| {
        fs::write(r.join("verify.sh"), "changed\n").unwrap();
        fs::create_dir_all(r.join(".config")).unwrap();
        fs::write(r.join(".config/wt.toml"), "x = 1\n").unwrap();
        fs::write(r.join(".config/cpd.toml"), "x = 1\n").unwrap();
        fs::create_dir_all(r.join(".cpd")).unwrap();
        fs::write(r.join(".cpd/x"), "x\n").unwrap();
    });
    let found = fx.scoped(|| {
        touched_protected(&repo, "cpd/pay-4521", &[], "main", "cpd/pay-4521", "att").unwrap()
    });
    assert_eq!(found, vec![".cpd/x"]);
}

// A base merge (§7.5): a protected path whose version on the Attempt is the Lead tip's or the
// base's is allowed.

/// `tamper_repo` plus a commit on `main` writing `path` = `content` (`None`: removing it),
/// which `att` merges; `cpd/pay-4521` stays where it was, so the base is ahead of the Lead tip.
fn base_merge_repo(path: &str, content: Option<&str>) -> (Fx, PathBuf) {
    let (fx, repo) = tamper_repo();
    match content {
        Some(content) => fx.commit_file(&repo, path, content, &format!("edit {path}")),
        None => {
            fx.git(&repo, &["rm", "-q", path]);
            fx.git(&repo, &["commit", "-q", "-m", "remove"]);
        }
    }
    fx.git(&repo, &["switch", "-q", "att"]);
    fx.git(
        &repo,
        &["merge", "-q", "--no-ff", "-m", "merge the base", "main"],
    );
    fx.git(&repo, &["switch", "-q", "main"]);
    (fx, repo)
}

#[test]
fn a_protected_path_the_attempt_has_as_the_base_has_it_is_allowed() {
    for path in [".cpd/x", "verify.sh", "scripts/ci/run.sh"] {
        let (fx, repo) = base_merge_repo(path, Some("from the base\n"));
        assert_eq!(touched(&fx, &repo), Vec::<String>::new(), "{path}");
        fx.scoped(|| {
            tamper_check(
                &repo,
                "cpd/pay-4521",
                &list(),
                "main",
                "cpd/pay-4521",
                "att",
            )
        })
        .unwrap();
    }
}

#[test]
fn a_protected_path_changed_to_anything_but_the_bases_or_the_lead_tips_is_tampered() {
    let (fx, repo) = base_merge_repo(".cpd/x", Some("from the base\n"));
    fx.git(&repo, &["switch", "-q", "att"]);
    fx.commit_file(&repo, ".cpd/x", "mine\n", "edit .cpd/x");
    fx.git(&repo, &["switch", "-q", "main"]);
    assert_eq!(touched(&fx, &repo), vec![".cpd/x"]);
}

#[test]
fn a_protected_path_that_matches_the_lead_tip_is_allowed() {
    // The Lead tip gained the file and the Attempt, cut earlier, has the same version.
    let (fx, repo) = tamper_repo();
    fx.git(&repo, &["switch", "-q", "cpd/pay-4521"]);
    fx.commit_file(&repo, ".cpd/x", "lead\n", "edit .cpd/x");
    fx.git(&repo, &["switch", "-q", "att"]);
    fx.git(
        &repo,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "merge the lead",
            "cpd/pay-4521",
        ],
    );
    fx.git(&repo, &["switch", "-q", "main"]);
    let base = fx.git(&repo, &["merge-base", "main", "att"]);
    let found = fx.scoped(|| {
        touched_protected(&repo, "cpd/pay-4521", &list(), "main", &base, "att").unwrap()
    });
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn a_protected_file_deleted_on_the_base_and_in_the_attempt_is_allowed() {
    for path in ["verify.sh", "scripts/ci/run.sh"] {
        let (fx, repo) = base_merge_repo(path, None);
        assert_eq!(touched(&fx, &repo), Vec::<String>::new(), "{path}");
    }
}

#[test]
fn a_protected_file_deleted_in_the_attempt_alone_is_tampered() {
    let (fx, repo) = tamper_repo();
    on_att(&fx, &repo, |r| {
        fs::remove_file(r.join("verify.sh")).unwrap()
    });
    assert_eq!(touched(&fx, &repo), vec!["verify.sh"]);
}

#[test]
fn base_ref_is_origin_slash_base_only_with_an_origin() {
    let (fx, repo) = tamper_repo();
    assert_eq!(
        fx.scoped(|| cpd_core::git::tamper::base_ref(&repo, "main"))
            .unwrap(),
        "main"
    );
    fx.git(
        &repo,
        &["remote", "add", "origin", "https://example.invalid/pay.git"],
    );
    assert_eq!(
        fx.scoped(|| cpd_core::git::tamper::base_ref(&repo, "main"))
            .unwrap(),
        "origin/main"
    );
}

// ---------------------------------------------------------------- rule S

// In-process: the git calls are the fake git's, the nap between two rounds is a nap of the `ManualClock`.

/// A Lead worktree on `cpd/pay-4521` and a scratch worktree that makes the
/// commits a merge would fast-forward the Lead Branch to.
struct Moving {
    fx: Fx,
    repo: PathBuf,
    lead: PathBuf,
    branch: String,
    scratch: PathBuf,
}

/// The `Setup` of a worktree's hooks, its log `name` under the `Fx`'s root.
fn setup_in(fx: &Fx, name: &str) -> Setup {
    Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("logs").join(name),
    }
}

fn moving() -> Moving {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let made = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup_in(&fx, "lead.log")))
        .unwrap();
    let scratch = fx.env.root().join("scratch");
    fx.git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            scratch.to_str().unwrap(),
            &made.branch,
        ],
    );
    Moving {
        fx,
        repo,
        lead: made.path,
        branch: made.branch,
        scratch,
    }
}

impl Moving {
    /// A merge's step 8: a commit that writes `path` on top of the Lead Branch,
    /// then the compare-and-swap of the ref. The Lead worktree is not touched.
    fn merge(&self, path: &str, content: &str) -> String {
        let reference = format!("refs/heads/{}", self.branch);
        let old = self.fx.git(&self.repo, &["rev-parse", &reference]);
        self.fx
            .git(&self.scratch, &["checkout", "-q", "--detach", &old]);
        self.fx
            .commit_file(&self.scratch, path, content, &format!("edit {path}"));
        let new = self.fx.git(&self.scratch, &["rev-parse", "HEAD"]);
        self.fx.git(
            &self.repo,
            &[
                "update-ref",
                "-m",
                "cpd: merge test",
                &reference,
                &new,
                &old,
            ],
        );
        new
    }

    fn sync(&self) -> Result<Synced, cpd_core::error::CpdError> {
        self.fx.scoped(|| sync_lead(&self.lead, &self.branch))
    }

    fn assert_at_tip_and_clean(&self, tip: &str) {
        assert_eq!(self.fx.git(&self.lead, &["rev-parse", "HEAD"]), tip);
        assert_eq!(
            self.fx.git(&self.lead, &["status", "--porcelain"]),
            "",
            "the Lead worktree holds the tip's tree"
        );
    }
}

#[test]
fn rule_s_two_merges_synced_in_the_reverse_order_end_at_the_tip_and_clean() {
    let m = moving();
    let _first = m.merge("a.txt", "a\n");
    let second = m.merge("b.txt", "b\n");

    // The second merge's sync runs first, then the first merge's.
    assert_eq!(m.sync().unwrap(), Synced::Done);
    assert_eq!(m.sync().unwrap(), Synced::Done);

    m.assert_at_tip_and_clean(&second);
    assert_eq!(fs::read_to_string(m.lead.join("a.txt")).unwrap(), "a\n");
    assert_eq!(fs::read_to_string(m.lead.join("b.txt")).unwrap(), "b\n");
}

#[test]
fn rule_s_two_syncs_at_once_both_end_with_the_lead_worktree_at_the_tip() {
    let m = moving();
    m.merge("a.txt", "a\n");
    let tip = m.merge("b.txt", "b\n");
    // Two threads on the route's mutex: a thread does not see the scope of this one, so it scopes the ports itself.
    let ports = m.fx.fakes.ports.clone();

    let results = std::thread::scope(|s| {
        let sync = || Ports::scope(&ports, || sync_lead(&m.lead, &m.branch));
        let one = s.spawn(sync);
        let two = s.spawn(sync);
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
    m.fx.git(&m.lead, &["add", "README.md"]);
    let tip = m.merge("a.txt", "a\n");

    let err = m.sync().unwrap_err();

    assert_eq!(err.code(), ErrorCode::LeadWorktreeStale);
    assert_eq!(err.details()["worktree"], m.lead.display().to_string());
    // Nothing was overwritten: the edit is still staged and on disk, and the tip's file is not checked out.
    assert_eq!(
        m.fx.git(&m.lead, &["diff", "--cached", "--name-only"]),
        "README.md\na.txt"
    );
    assert_eq!(
        m.fx.git(&m.lead, &["show", ":README.md"]),
        "staged by the Lead"
    );
    assert_eq!(
        fs::read_to_string(m.lead.join("README.md")).unwrap(),
        "staged by the Lead\n"
    );
    assert!(!m.lead.join("a.txt").exists());
    assert_eq!(
        m.fx.git(&m.lead, &["rev-parse", "HEAD"]),
        tip,
        "the ref move stands"
    );
}

#[test]
fn rule_s_keeps_an_unstaged_edit_and_reports_it_stale() {
    let m = moving();
    fs::write(m.lead.join("README.md"), "edited by the Lead\n").unwrap();
    m.merge("a.txt", "a\n");

    let err = m.sync().unwrap_err();

    assert_eq!(err.code(), ErrorCode::LeadWorktreeStale);
    assert_eq!(
        fs::read_to_string(m.lead.join("README.md")).unwrap(),
        "edited by the Lead\n"
    );
    assert!(!m.lead.join("a.txt").exists());
}

#[test]
fn rule_s_waits_out_an_index_lock_held_for_one_second() {
    let m = moving();
    let tip = m.merge("a.txt", "a\n");
    // Another git process holds the index lock for one second of virtual time: `write-tree` finds it held until then.
    let lock = PathBuf::from(m.fx.git(
        &m.lead,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "index.lock",
        ],
    ));
    let held = format!(
        "fatal: Unable to create '{}': File exists.\n",
        lock.display()
    );
    let clock = m.fx.fakes.clock.clone();
    let _ = (*m.fx.fakes.runner).clone().on(
        "git",
        move |call| {
            call.args.iter().any(|a| a == "write-tree") && clock.elapsed() < Duration::from_secs(1)
        },
        Reply::exit(128, "", &held),
    );

    let synced = m.sync();

    assert_eq!(synced.unwrap(), Synced::Done);
    let naps = m.fx.fakes.clock.naps();
    assert!(
        m.fx.fakes.clock.elapsed() >= Duration::from_secs(1),
        "it waited for the lock"
    );
    assert!(
        naps.len() >= 2 && naps.len() < 20,
        "a nap per round that found the lock: {naps:?}"
    );
    assert!(
        naps.iter()
            .all(|n| (Duration::from_millis(100)..=Duration::from_millis(500)).contains(n)),
        "{naps:?}"
    );
    m.assert_at_tip_and_clean(&tip);
}

#[test]
fn rule_s_on_an_up_to_date_lead_worktree_is_done_and_touches_nothing() {
    let m = moving();
    let tip = m.fx.git(&m.lead, &["rev-parse", "HEAD"]);
    assert_eq!(m.sync().unwrap(), Synced::Done);
    m.assert_at_tip_and_clean(&tip);
}

// ------------------------------------------------------------ deadline (contract §1.2, §7)

/// strerror(0): "Success" on Linux, "Undefined error: 0" on macOS.
fn strerror_zero() -> &'static str {
    if cfg!(target_os = "macos") {
        "Undefined error: 0"
    } else {
        "Success"
    }
}

#[test]
fn a_merged_attempt_is_removed_with_branch_d_and_an_unmerged_one_is_refused() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let setup = setup_in(&fx, "attempt.log");
    let lead = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup))
        .unwrap();
    let made = fx
        .scoped(|| create_attempt_worktree(&repo, &lead.path, &plan(), &attempt(), &setup))
        .unwrap();
    fx.commit_file(&made.path, "work.txt", "work\n", "edit work.txt");

    // Unmerged: git's own guard refuses the branch deletion.
    let err = fx
        .scoped(|| remove_merged_attempt(&lead.path, &made.path, &made.branch))
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitFailed);
    assert_eq!(err.details()["argv"][3], "branch");
    assert_eq!(
        fx.git(&repo, &["branch", "--list", &made.branch]).trim(),
        made.branch
    );

    fx.git(&lead.path, &["merge", "-q", "--ff-only", &made.branch]);
    fx.scoped(|| remove_merged_attempt(&lead.path, &made.path, &made.branch))
        .unwrap();
    assert!(!made.path.exists());
    assert_eq!(fx.git(&repo, &["branch", "--list", &made.branch]), "");
    // Rerunning is safe.
    fx.scoped(|| remove_merged_attempt(&lead.path, &made.path, &made.branch))
        .unwrap();
}

// ------------------------------------------------ `worktree::list` over a half-written admin dir (contract §7)
//
// `git worktree list --porcelain` reads `.git/worktrees/<id>/commondir` of every linked worktree. Another cpd operation
// in the same repo adding a worktree (git creates `commondir` empty, then writes it) or removing one (git deletes its
// files one after another) leaves an admin dir half-written for a moment, and git dies on it: exit 128, `fatal: failed
// to read .git/worktrees/<id>/commondir: Success` (reproduced by hand, git 2.43). Each test below scripts that
// answer to the first `worktree list` call(s) of the route, so the rounds of `worktree::list` and the pauses between
// them are ours and on the virtual clock.

/// A repo with one linked worktree, `held` on branch `held`: returns the repo and the worktree.
fn repo_with_a_linked_worktree(fx: &Fx) -> (PathBuf, PathBuf) {
    let repo = fx.git_repo("pay");
    let worktree = fx.env.root().join("pay.held");
    fx.git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "held",
            worktree.to_str().unwrap(),
        ],
    );
    (repo, worktree)
}

/// The paths `worktree::list` found, symlinks resolved.
fn listed(fx: &Fx, repo: &Path) -> Result<Vec<PathBuf>, cpd_core::error::CpdError> {
    fx.scoped(|| {
        Ok(cpd_core::git::worktree::list(repo)?
            .into_iter()
            .map(|w| w.path.canonicalize().unwrap_or(w.path))
            .collect())
    })
}

#[test]
fn list_reads_again_when_the_admin_dir_it_read_half_written_is_completed() {
    let fx = Fx::with_ports();
    let (repo, worktree) = repo_with_a_linked_worktree(&fx);
    // The first read got an empty `commondir`; the writer has finished before the second.
    fail_git(
        &fx,
        1,
        is_worktree_list,
        &format!(
            "fatal: failed to read .git/worktrees/pay.held/commondir: {}\n",
            strerror_zero()
        ),
    );

    let found = listed(&fx, &repo);

    assert_eq!(
        git_calls(&fx, is_worktree_list).len(),
        2,
        "the failed read, then the second"
    );
    assert_retry_naps(&fx, 1);
    let found = found.expect("list reads again: the admin dir is complete at the second read");
    assert_eq!(
        found,
        [
            repo.canonicalize().unwrap_or(repo),
            worktree.canonicalize().unwrap_or(worktree)
        ]
    );
}

#[test]
fn list_reads_again_when_the_admin_dir_it_read_is_removed() {
    let fx = Fx::with_ports();
    let (repo, worktree) = repo_with_a_linked_worktree(&fx);
    // `git worktree remove` deleted the dir while the first read was in it: the second read lists the main one only.
    fail_git(
        &fx,
        1,
        is_worktree_list,
        "fatal: failed to read .git/worktrees/pay.held/commondir: No such file or directory\n",
    );
    fx.git(
        &repo,
        &["worktree", "remove", "--force", worktree.to_str().unwrap()],
    );

    let found = listed(&fx, &repo);

    assert_eq!(
        git_calls(&fx, is_worktree_list).len(),
        2,
        "the failed read, then the second"
    );
    assert_retry_naps(&fx, 1);
    let found = found.expect("list reads again: the admin dir is gone at the second read");
    assert_eq!(found, [repo.canonicalize().unwrap_or(repo)]);
}

#[test]
fn an_admin_dir_that_stays_half_written_is_one_git_failed_after_twenty_reads() {
    let fx = Fx::with_ports();
    let (repo, _worktree) = repo_with_a_linked_worktree(&fx);
    let stderr = format!(
        "fatal: failed to read .git/worktrees/pay.held/commondir: {}\n",
        strerror_zero()
    );
    let _ = (*fx.fakes.runner)
        .clone()
        .on("git", is_worktree_list, Reply::exit(128, "", &stderr));

    let found = listed(&fx, &repo);

    // The bound is the one of `worktree add` (§7): 20 rounds, 19 pauses between them; the 20th failure is the error.
    assert_eq!(
        git_calls(&fx, is_worktree_list).len(),
        20,
        "each round ran git once"
    );
    assert_retry_naps(&fx, 19);
    let error = found.unwrap_err();
    assert_eq!(error.code(), ErrorCode::GitFailed);
    let details = error.details();
    assert_eq!(details["exit"], 128);
    assert_eq!(
        details["argv"].as_array().unwrap()[3..],
        [
            serde_json::json!("worktree"),
            serde_json::json!("list"),
            serde_json::json!("--porcelain")
        ]
    );
    let stderr = details["stderr"].as_str().unwrap();
    assert!(
        stderr.contains("fatal: failed to read ")
            && stderr.contains(&format!(
                "worktrees/pay.held/commondir: {}",
                strerror_zero()
            )),
        "{stderr}"
    );
}

#[test]
fn any_other_worktree_list_failure_is_git_failed_at_once() {
    let fx = Fx::with_ports();
    let (repo, _worktree) = repo_with_a_linked_worktree(&fx);
    // The first read of the config finds a line git cannot parse: not the half-written admin dir. The next read
    // would succeed (the script is used up), so a retry would show.
    fail_git(
        &fx,
        1,
        is_worktree_list,
        "fatal: bad config line 1 in file .git/config\n",
    );

    let found = listed(&fx, &repo);

    assert_eq!(git_calls(&fx, is_worktree_list).len(), 1, "no second read");
    assert!(fx.fakes.clock.naps().is_empty());
    let error = found.expect_err("no second read: the failure is not the half-written admin dir");
    assert_eq!(error.code(), ErrorCode::GitFailed);
    assert_eq!(error.details()["exit"], 128);
    assert!(
        error.details()["stderr"]
            .as_str()
            .unwrap()
            .contains("bad config"),
        "{error:?}"
    );
}

// ------------------------------------------------ removal of a Lead worktree (DESIGN 4): whole or untouched
//
// `remove_merged_lead` runs on the git route of the `Fx` (real files, written by the fake git). A `Stopper` stands for
// the deadline kill: it makes the n-th git call of the removal fail, and `git worktree remove` of the Lead worktree
// first deletes about half of its files (git deletes in place, so a kill inside it leaves a partial tree).

const TRACKED: [&str; 6] = [
    "README.md",
    "a.txt",
    "b.txt",
    "dir/c.txt",
    "dir/d.txt",
    "dir/e.txt",
];

/// A [`Runner`] over `inner` that fails the `stop`-th git call it sees (exit 137, as a kill), and counts them.
struct Stopper {
    inner: Arc<dyn cpd_core::host::runner::Runner>,
    stop: usize,
    seen: Mutex<usize>,
    lead: PathBuf,
}

impl Stopper {
    fn calls(&self) -> usize {
        *self.seen.lock().unwrap()
    }

    fn kill(&self, call: &Call) -> Option<std::process::Output> {
        if call.program.rsplit('/').next() != Some("git") {
            return None;
        }
        let mut seen = self.seen.lock().unwrap();
        *seen += 1;
        if *seen != self.stop {
            return None;
        }
        let removes_lead = call.args.windows(2).any(|w| w == ["worktree", "remove"])
            && call.args.iter().any(|a| Path::new(a) == self.lead);
        if removes_lead {
            let mut files: Vec<PathBuf> = TRACKED.iter().map(|f| self.lead.join(f)).collect();
            files.truncate(TRACKED.len() / 2);
            for file in files {
                let _ = fs::remove_file(file);
            }
        }
        use std::os::unix::process::ExitStatusExt;
        Some(std::process::Output {
            status: std::process::ExitStatus::from_raw(137 << 8),
            stdout: Vec::new(),
            stderr: b"killed\n".to_vec(),
        })
    }
}

impl cpd_core::host::runner::Runner for Stopper {
    fn output(
        &self,
        cmd: &mut std::process::Command,
        tool: &str,
        deadline: Duration,
    ) -> Result<std::process::Output, cpd_core::host::deadline::Failed> {
        let call = Call {
            tool: tool.to_owned(),
            program: cmd.get_program().to_string_lossy().into_owned(),
            args: cmd
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            envs: Vec::new(),
            cwd: cmd.get_current_dir().map(Path::to_path_buf),
            deadline,
            stdin: None,
            stderr_to: None,
        };
        match self.kill(&call) {
            Some(out) => Ok(out),
            None => self.inner.output(cmd, tool, deadline),
        }
    }
    fn output_unbounded(
        &self,
        cmd: &mut std::process::Command,
        tool: &str,
    ) -> std::io::Result<std::process::Output> {
        self.inner.output_unbounded(cmd, tool)
    }
    fn run(&self, call: &Call) -> Result<std::process::Output, cpd_core::host::deadline::Failed> {
        match self.kill(call) {
            Some(out) => Ok(out),
            None => self.inner.run(call),
        }
    }
}

/// A merged PR's Lead worktree `<root>/<dir>/pay.pay-4521` of a repo `name`, clean, on its Lead Branch, with the six
/// `TRACKED` files. Returns `(repo, path, head)`; `head` is the PR's recorded head (the commits are not on any origin).
fn lead_to_remove(fx: &Fx, name: &str, dir: &str) -> (PathBuf, PathBuf, String) {
    let repo = fx.git_repo(name);
    for file in &TRACKED[1..] {
        fx.commit_file(&repo, file, &format!("{file}\n"), &format!("add {file}"));
    }
    let parent = fx.env.root().join(dir);
    fs::create_dir_all(&parent).unwrap();
    let path = parent.join("pay.pay-4521");
    fx.git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            &legacy_lead_branch(&plan()),
            path.to_str().unwrap(),
        ],
    );
    let head = fx.git(&path, &["rev-parse", "HEAD"]);
    (repo, path, head)
}

fn removing(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap().to_os_string();
    name.push(".removing");
    path.with_file_name(name)
}

/// `path` with the symlinks of its directory resolved (the file itself may be gone).
fn resolved(path: &Path) -> PathBuf {
    path.parent()
        .and_then(|p| p.canonicalize().ok())
        .map_or_else(|| path.to_path_buf(), |p| p.join(path.file_name().unwrap()))
}

fn registered(fx: &Fx, repo: &Path, path: &Path) -> bool {
    let want = resolved(path);
    fx.git(repo, &["worktree", "list", "--porcelain"])
        .lines()
        .any(|l| {
            l.strip_prefix("worktree ")
                .is_some_and(|p| resolved(Path::new(p)) == want)
        })
}

fn branch_exists(fx: &Fx, repo: &Path) -> bool {
    !fx.git(repo, &["branch", "--list", &legacy_lead_branch(&plan())])
        .is_empty()
}

/// Every file under `dir` with its bytes, sorted.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                walk(&entry, out);
            } else {
                out.push((entry.clone(), fs::read(&entry).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out
}

fn remove_lead(
    fx: &Fx,
    repo: &Path,
    path: &Path,
    head: &str,
) -> Result<cpd_core::git::LeadRemoval, cpd_core::error::CpdError> {
    fx.scoped(|| {
        cpd_core::git::remove_merged_lead(repo, path, &legacy_lead_branch(&plan()), || {
            Some(head.to_owned())
        })
    })
}

#[test]
fn lead_removal_stopped_after_any_step_leaves_the_worktree_whole_or_gone() {
    let fx = Fx::with_ports();
    for n in 1.. {
        let (repo, path, head) = lead_to_remove(&fx, &format!("pay{n}"), &format!("n{n}"));
        let stopper = Arc::new(Stopper {
            inner: fx.fakes.ports.runner.clone(),
            stop: n,
            seen: Mutex::new(0),
            lead: path.clone(),
        });
        let ports = Ports {
            runner: stopper.clone(),
            ..fx.fakes.ports.clone()
        };

        let _ = Ports::scope(&ports, || {
            cpd_core::git::remove_merged_lead(&repo, &path, &legacy_lead_branch(&plan()), || {
                Some(head.clone())
            })
        });

        if stopper.calls() < n {
            assert!(n > 3, "the removal made only {} git calls", stopper.calls());
            break;
        }
        if path.exists() {
            for file in TRACKED {
                assert!(
                    path.join(file).exists(),
                    "stop at git call {n}: {file} is missing from the worktree that still exists"
                );
            }
            assert!(
                registered(&fx, &repo, &path),
                "stop at git call {n}: the worktree that still exists is not registered"
            );
            assert_eq!(
                fx.git(&path, &["branch", "--show-current"]),
                legacy_lead_branch(&plan()),
                "stop at git call {n}"
            );
        }
    }
}

#[test]
fn lead_removal_after_a_stop_is_finished_by_the_next_call() {
    let fx = Fx::with_ports();
    let (repo, path, head) = lead_to_remove(&fx, "pay", "wt");
    // The state a stop after the rename leaves: `<path>` absent, `<path>.removing` the marker, registration and branch there.
    fs::rename(&path, removing(&path)).unwrap();
    assert!(registered(&fx, &repo, &path) && branch_exists(&fx, &repo));

    remove_lead(&fx, &repo, &path, &head).unwrap();

    assert!(!path.exists());
    assert!(!removing(&path).exists(), "the marker is deleted");
    assert!(
        !registered(&fx, &repo, &path),
        "the registration is dropped"
    );
    assert!(!branch_exists(&fx, &repo), "the Lead Branch is deleted");

    let before = (
        fx.git(&repo, &["worktree", "list", "--porcelain"]),
        fx.git(&repo, &["branch", "--list"]),
    );
    let again = remove_lead(&fx, &repo, &path, &head).expect("a third call is not an error");
    assert_eq!(again, cpd_core::git::LeadRemoval::Removed);
    assert_eq!(
        before,
        (
            fx.git(&repo, &["worktree", "list", "--porcelain"]),
            fx.git(&repo, &["branch", "--list"])
        )
    );
    assert!(!path.exists() && !removing(&path).exists());
}

#[test]
fn lead_removal_keeps_a_dirty_or_unpushed_worktree_untouched_without_renaming() {
    use cpd_core::git::{Kept, LeadRemoval};
    let fx = Fx::with_ports();

    // Dirty: an uncommitted change and an untracked file.
    let (repo, path, head) = lead_to_remove(&fx, "dirty", "d");
    fs::write(path.join("a.txt"), "changed\n").unwrap();
    fs::write(path.join("new.txt"), "new\n").unwrap();
    let before = snapshot(&path);
    let kept = remove_lead(&fx, &repo, &path, &head).unwrap();
    assert_eq!(kept, LeadRemoval::Kept(Kept::Dirty));
    assert_eq!(snapshot(&path), before);
    assert!(!removing(&path).exists());
    assert!(registered(&fx, &repo, &path) && branch_exists(&fx, &repo));

    // Unpushed: a non-merge commit that is not on origin, and no recorded head that accepts it.
    let (repo, path, _) = lead_to_remove(&fx, "unpushed", "u");
    fx.commit_file(&path, "local.txt", "local\n", "local work");
    let before = snapshot(&path);
    let kept = fx
        .scoped(|| {
            cpd_core::git::remove_merged_lead(&repo, &path, &legacy_lead_branch(&plan()), || None)
        })
        .unwrap();
    assert_eq!(kept, LeadRemoval::Kept(Kept::Unpushed));
    assert_eq!(snapshot(&path), before);
    assert!(!removing(&path).exists());
    assert!(registered(&fx, &repo, &path) && branch_exists(&fx, &repo));
}

#[test]
fn lead_removal_with_a_failing_rename_is_kept_untouched() {
    use std::os::unix::fs::PermissionsExt;
    /// Gives the directory its mode back on drop, so the env removes it, also when an assertion fails.
    struct Restore(PathBuf, fs::Permissions);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, self.1.clone());
        }
    }
    let fx = Fx::with_ports();
    let (repo, path, head) = lead_to_remove(&fx, "pay", "ro");
    let parent = path.parent().unwrap().to_path_buf();
    let before = snapshot(&path);
    let mode = fs::metadata(&parent).unwrap().permissions();
    let _restore = Restore(parent.clone(), mode.clone());
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();

    let result = remove_lead(&fx, &repo, &path, &head);

    fs::set_permissions(&parent, mode).unwrap();
    match result.expect("a failing rename is kept, not an error") {
        cpd_core::git::LeadRemoval::Kept(cpd_core::git::Kept::Rename(reason)) => {
            assert!(!reason.is_empty(), "the error is the reason")
        }
        other => panic!("expected kept with the rename error, got {other:?}"),
    }
    assert_eq!(snapshot(&path), before);
    assert!(!removing(&path).exists());
    assert!(registered(&fx, &repo, &path) && branch_exists(&fx, &repo));
}

// ------------------------------------------------ removing an Attempt worktree outside git's deadline (contract §7.2)

fn is_worktree_remove(call: &Call) -> bool {
    call.args.windows(2).any(|w| w == ["worktree", "remove"])
}

/// Every `git worktree remove` takes 120 s: the fake runner turns it into `tool_timeout`.
fn worktree_remove_times_out(fx: &Fx) {
    (*fx.fakes.runner).clone().on(
        "git",
        is_worktree_remove,
        Reply::ok("").takes(Duration::from_secs(120)),
    );
}

/// A repo with a Lead worktree and a parallel Attempt worktree of `id`.
fn lead_and_attempt(
    fx: &Fx,
    id: &str,
) -> (PathBuf, cpd_core::git::Created, cpd_core::git::Created) {
    let repo = fx.git_repo("pay");
    let setup = setup_in(fx, "attempt.log");
    let lead = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup))
        .unwrap();
    let made = fx
        .scoped(|| {
            create_attempt_worktree(
                &repo,
                &lead.path,
                &plan(),
                &AttemptId::parse(id).unwrap(),
                &setup,
            )
        })
        .unwrap();
    (repo, lead, made)
}

fn has_branch(fx: &Fx, repo: &Path, branch: &str) -> bool {
    !fx.git(repo, &["branch", "--list", branch])
        .trim()
        .is_empty()
}

#[test]
fn a_merged_attempt_is_removed_though_git_worktree_remove_would_time_out() {
    let fx = Fx::with_ports();
    let (repo, lead, made) = lead_and_attempt(&fx, "api-a1");
    fx.commit_file(&made.path, "work.txt", "work\n", "edit work.txt");
    fx.git(&lead.path, &["merge", "-q", "--ff-only", &made.branch]);
    worktree_remove_times_out(&fx);

    fx.scoped(|| remove_merged_attempt(&lead.path, &made.path, &made.branch))
        .unwrap();

    assert!(!made.path.exists());
    assert!(!removing(&made.path).exists());
    assert!(!registered(&fx, &repo, &made.path));
    assert!(!has_branch(&fx, &repo, &made.branch));
    assert!(
        git_calls(&fx, is_worktree_remove).is_empty(),
        "no git worktree remove call"
    );
    fx.scoped(|| remove_merged_attempt(&lead.path, &made.path, &made.branch))
        .unwrap();
}

#[test]
fn a_merged_attempt_with_untracked_files_is_refused_by_status_and_left_whole() {
    let fx = Fx::with_ports();
    let (repo, lead, made) = lead_and_attempt(&fx, "api-a1");
    fx.commit_file(&made.path, "work.txt", "work\n", "edit work.txt");
    fs::write(made.path.join("untracked.txt"), "x").unwrap();
    fx.git(&lead.path, &["merge", "-q", "--ff-only", &made.branch]);
    worktree_remove_times_out(&fx);

    let err = fx
        .scoped(|| remove_merged_attempt(&lead.path, &made.path, &made.branch))
        .unwrap_err();

    assert_eq!(err.code(), ErrorCode::GitFailed);
    assert!(made.path.join("untracked.txt").exists());
    assert!(registered(&fx, &repo, &made.path));
    assert!(has_branch(&fx, &repo, &made.branch));
    assert!(!removing(&made.path).exists());
}

#[test]
fn discard_worktree_removes_a_dirty_unmerged_worktree_though_git_worktree_remove_would_time_out() {
    let fx = Fx::with_ports();
    let (repo, lead, made) = lead_and_attempt(&fx, "api-a1");
    fx.commit_file(&made.path, "work.txt", "unmerged\n", "edit work.txt");
    fs::write(made.path.join("dirty.txt"), "dirty").unwrap();
    worktree_remove_times_out(&fx);

    fx.scoped(|| discard_worktree(&lead.path, &made.path, &made.branch))
        .unwrap();

    assert!(!made.path.exists());
    assert!(!removing(&made.path).exists());
    assert!(!registered(&fx, &repo, &made.path));
    assert!(!has_branch(&fx, &repo, &made.branch));
    assert!(
        git_calls(&fx, is_worktree_remove).is_empty(),
        "no git worktree remove call"
    );
}

#[test]
fn a_removal_finishes_or_replaces_a_removing_aside_left_by_an_earlier_stop() {
    let fx = Fx::with_ports();
    let (repo, lead, made) = lead_and_attempt(&fx, "api-a1");
    // (i) A stop after the rename: the path is gone, the aside holds the tree, registration and branch remain.
    fs::rename(&made.path, removing(&made.path)).unwrap();
    fs::write(removing(&made.path).join("left.txt"), "x").unwrap();

    fx.scoped(|| discard_worktree(&lead.path, &made.path, &made.branch))
        .unwrap();

    assert!(!made.path.exists());
    assert!(!removing(&made.path).exists());
    assert!(!registered(&fx, &repo, &made.path));
    assert!(!has_branch(&fx, &repo, &made.branch));

    // (ii) A stale aside beside a live worktree.
    let setup = setup_in(&fx, "attempt2.log");
    let other = fx
        .scoped(|| {
            create_attempt_worktree(
                &repo,
                &lead.path,
                &plan(),
                &AttemptId::parse("web-a1").unwrap(),
                &setup,
            )
        })
        .unwrap();
    fs::create_dir_all(removing(&other.path)).unwrap();
    fs::write(removing(&other.path).join("stale.txt"), "x").unwrap();

    fx.scoped(|| discard_worktree(&lead.path, &other.path, &other.branch))
        .unwrap();

    assert!(!other.path.exists());
    assert!(!removing(&other.path).exists());
    assert!(!registered(&fx, &repo, &other.path));
    assert!(!has_branch(&fx, &repo, &other.branch));
}
