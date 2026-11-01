//! `cpd prune` (commands.md §26), in process on `Fakes`: the five tests of the old exec file
//! `cli/tests/tooling/prune_script.rs`, by name. The git route of the `FakeRunner` is a `Universe` of repos
//! and worktrees, `gh pr list` is a reply on the runner, the protected set is a store fixture in a temp dir.
//! No test starts a process, runs real git or `gh`, sleeps or reads the wall clock.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use super::{Args, Outcome, run};
use crate::fakes::git::{GitRoute, Repo, Universe};
use crate::home::CpdHome;
use crate::host::runner::Reply;
use crate::ports::Fakes;
use crate::store::Store;

/// A repo `work` on `main` whose `origin` is the GitHub repo `acme/work` (a bare repo of the Universe behind
/// an `insteadOf`), both at the same commit; an empty store; a `gh` that lists no merged PR.
pub(super) struct Fixture {
    pub(super) tmp: TempDir,
    pub(super) fakes: Fakes,
    pub(super) universe: Arc<Mutex<Universe>>,
    pub(super) route: Arc<GitRoute>,
    pub(super) home: CpdHome,
    pub(super) repo: Repo,
    pub(super) origin: Repo,
    pub(super) worktrees: usize,
}

pub(super) const NO_PRS: &str = "[]";

impl Fixture {
    pub(super) fn new() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let fakes = Fakes::new();
        let mut universe = Universe::new(fakes.clock.clone());
        universe.set_global("user.name", "cpd test");
        universe.set_global("user.email", "test@cpd.invalid");
        let universe = Arc::new(Mutex::new(universe));
        let route = Arc::new(GitRoute::new(universe.clone()));
        let _ = (*fakes.runner).clone().with_git(&route);

        let repo = Repo::init(&universe, &root.join("work"));
        repo.write("README.md", "one\n");
        repo.commit("base");
        let origin = repo.bare_origin(&root.join("origin.git"));
        {
            let mut u = universe.lock().unwrap();
            let url = "https://github.com/acme/work.git";
            u.config_set(repo.id(), "remote.origin.url", url);
            let bare = u.git_dir(origin.id());
            u.config_add(repo.id(), &format!("url.{}.insteadOf", bare.display()), url);
        }
        repo.push();

        let home = CpdHome::resolve(Some(root.join("cpd").into_os_string()), None).unwrap();
        Fixture {
            tmp,
            fakes,
            universe,
            route,
            home,
            repo,
            origin,
            worktrees: 0,
        }
    }

    /// `gh pr list` answers `json` (exit 0).
    pub(super) fn merged_prs(&self, json: &str) {
        let _ = (*self.fakes.runner).clone().on(
            "gh",
            |c| c.args.starts_with(&["pr".to_owned(), "list".to_owned()]),
            Reply::ok(json),
        );
    }

    /// Every `gh` call fails with `stderr`.
    fn gh_fails(&self, stderr: &str) {
        let _ = (*self.fakes.runner)
            .clone()
            .on("gh", |_| true, Reply::exit(1, "", stderr));
    }

    /// The store of the home with `sql` run on it.
    pub(super) fn store(&self, sql: &str) {
        let mut store = Store::open(&self.home, 5000).unwrap();
        store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    /// `git -C <dir> <args>` through the runner; must exit 0; stdout.
    pub(super) fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        let out = self
            .fakes
            .ports
            .runner
            .output(&mut cmd, "git", Duration::from_secs(60))
            .expect("the fake git answers");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    /// A worktree `wt-<dir>` of the repo on the new branch `branch` from `start`, with one commit in `file`.
    pub(super) fn worktree(
        &mut self,
        dir: &str,
        branch: &str,
        start: &str,
        file: Option<&str>,
    ) -> PathBuf {
        self.worktrees += 1;
        let wt = self
            .tmp
            .path()
            .canonicalize()
            .unwrap()
            .join(format!("wt-{dir}"));
        self.git(
            self.repo.dir(),
            &[
                "worktree",
                "add",
                "-q",
                "--no-track",
                "-b",
                branch,
                wt.to_str().unwrap(),
                start,
            ],
        );
        if let Some(file) = file {
            std::fs::write(wt.join(file), format!("{file}\n")).unwrap();
            self.git(&wt, &["add", "-A"]);
            self.git(&wt, &["commit", "-q", "-m", file]);
        }
        wt
    }

    pub(super) fn head(&self, wt: &Path) -> String {
        self.git(wt, &["rev-parse", "HEAD"])
    }

    pub(super) fn prune(&self, flags: &[&str]) -> Outcome {
        let mut args = Args {
            apply: false,
            drop: Vec::new(),
            repos: vec![self.repo.dir().to_path_buf()],
        };
        let mut it = flags.iter();
        while let Some(flag) = it.next() {
            match *flag {
                "--apply" => args.apply = true,
                "--drop" => args.drop.push((*it.next().unwrap()).to_owned()),
                other => panic!("the test passes no {other}"),
            }
        }
        // The default `gh` answer goes last: the first matching rule answers, so a test's own rule wins.
        self.merged_prs(NO_PRS);
        let outcome = run(&self.fakes.ports, &self.home, &args);
        self.route.assert_supported();
        outcome
    }

    pub(super) fn run(&self, apply: bool) -> Outcome {
        self.prune(if apply { &["--apply"] } else { &[] })
    }

    /// The worktrees the Universe knows for the repo, the main one first.
    fn worktrees_listed(&self) -> Vec<PathBuf> {
        self.universe
            .lock()
            .unwrap()
            .worktrees(self.repo.id())
            .into_iter()
            .map(|w| w.path)
            .collect()
    }

    /// The local branches of the repo.
    pub(super) fn branches(&self) -> BTreeSet<String> {
        let u = self.universe.lock().unwrap();
        u.refs(self.repo.id(), "refs/heads/")
            .into_iter()
            .map(|(name, _)| name.trim_start_matches("refs/heads/").to_owned())
            .collect()
    }

    /// The branches of `origin` and their commits.
    fn remote_branches(&self) -> Vec<String> {
        let u = self.universe.lock().unwrap();
        u.refs(self.origin.id(), "refs/heads/")
            .into_iter()
            .map(|(name, oid)| format!("{name} {oid}"))
            .collect()
    }

    fn removals_and_deletions(&self) -> Vec<Vec<String>> {
        self.fakes
            .runner
            .calls()
            .into_iter()
            .filter(|c| c.program.ends_with("git"))
            .map(|c| c.args)
            .filter(|a| a.iter().any(|w| w == "remove" || w == "-D" || w == "prune"))
            .collect()
    }
}

pub(super) fn lines(text: &str) -> BTreeSet<String> {
    text.lines().map(str::to_owned).collect()
}

pub(super) fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// Plan `qq` (dispatch): PR `followups` in dispatch (Job `j`, Attempt `j-a1`) and PR `cpd` merged (Job `old`,
/// Attempt `old-a1`); Plan `rr` is done, with one PR that is not merged.
const STORE_Q: &str = "
INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
  VALUES ('lead1', 'lead', 'ctx', 'qq', 'Lead', '/', 'O3', 'working', 't', 't');
INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
  VALUES ('qq', 'ctx', 'Q', 'dispatch', 'lead1', 't', 't');
INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
  VALUES ('rr', 'ctx', 'R', 'done', 'lead1', 't', 't', 't');
INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
  VALUES ('qq', 'followups', '/r', 'main', 'dispatch', 'cpd/qq/followups', '/w1', 't', cpd_txn(), 't');
INSERT INTO pull_requests (id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, mergeable, merge_state, authored, review_requested)
  VALUES (5, 'acme/work', 5, 'https://github.com/acme/work/pull/5', 'Cpd', 0, 'merged', 'none', 'none', 't', 't', 't', 'unknown', 'unknown', 0, 0);
INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, pr_number, pr_url, pr_opened_at, pull_request, merged_at, created_txn, created_at)
  VALUES ('qq', 'cpd', '/r', 'main', 'merged', 'cpd/qq/cpd', '/w2', 't', 5, 'https://github.com/acme/work/pull/5', 't', 5, 't', cpd_txn(), 't');
INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
  VALUES ('rr', 'late', '/r', 'main', 'waiting', 'cpd/rr/late', cpd_txn(), 't');
INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
  VALUES ('qq', 'followups', 'j', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff', cpd_txn(), 't');
INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
  VALUES ('qq', 'j-a1', 'j', 1, 'S2', 'ladder', 'running', 'cpd/qq--j-a1', '/w', 't');
INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, merged_sha, created_txn, created_at)
  VALUES ('qq', 'cpd', 'old', 'Old', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff', 'abc', cpd_txn(), 't');
INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, outcome, cause, merged_sha, ended_at)
  VALUES ('qq', 'old-a1', 'old', 1, 'S2', 'ladder', 'ended', 'cpd/qq--old-a1', '/w', 't', 'merged', 'merged', 'abc', 't');
";

#[test]
fn a_failed_command_is_reported_and_exits_non_zero() {
    let fx = Fixture::new();
    fx.gh_fails("gh: not logged in\n");

    let out = fx.run(false);
    assert_eq!(out.exit, 1);
    assert!(out.stderr.contains("not logged in"), "{}", out.stderr);
}

#[test]
fn keeps_what_cpd_still_runs() {
    let mut fx = Fixture::new();
    // Plan `rr` is done: its PR is not protected.
    fx.store(STORE_Q);
    fx.merged_prs(NO_PRS);
    // All at origin/main's commit, so every one of them was removable before.
    let lead = fx.worktree("lead", "cpd/qq/followups", "main", None);
    let attempt = fx.worktree("attempt", "cpd/qq--j-a1", "main", None);
    let merged_lead = fx.worktree("merged-lead", "cpd/qq/cpd", "main", None);
    let merged_attempt = fx.worktree("merged-attempt", "cpd/qq--old-a1", "main", None);
    let wt = |p: &PathBuf, b: &str| format!("{} ({b})", p.display());
    let expected: BTreeSet<String> = [
        format!("keep {}: active", wt(&lead, "cpd/qq/followups")),
        format!("keep {}: active", wt(&attempt, "cpd/qq--j-a1")),
        format!("remove {}", wt(&merged_lead, "cpd/qq/cpd")),
        format!("remove {}", wt(&merged_attempt, "cpd/qq--old-a1")),
        format!("{}: 2 remove, 2 keep", fx.repo.dir().display()),
    ]
    .into_iter()
    .collect();

    let out = fx.run(false);
    assert_eq!((out.exit, out.stderr.as_str()), (0, ""), "{}", out.stdout);
    assert_eq!(lines(&out.stdout), expected, "{}", out.stdout);

    // Also with --apply and --drop of their Lead Branches.
    let out = fx.prune(&["--apply", "--drop", "cpd/qq/followups", "--drop", "cpd/qq"]);
    assert_eq!((out.exit, out.stderr.as_str()), (0, ""), "{}", out.stdout);
    assert!(lead.exists() && attempt.exists());
    assert!(!merged_lead.exists() && !merged_attempt.exists());
    assert_eq!(
        fx.branches(),
        set(&["cpd/qq--j-a1", "cpd/qq/followups", "main"])
    );
}

#[test]
fn matches_a_legacy_lead_branch_renamed_locally() {
    let mut fx = Fixture::new();
    // Local `cpd/p/k`, whose PR #1 had head `cpd/p` and was squash-merged into main.
    let renamed = fx.worktree("renamed", "cpd/p/k", "main", Some("k.txt"));
    let head = fx.head(&renamed);
    let other = fx.worktree("other", "cpd/o/k", "main", Some("o.txt"));
    fx.merged_prs(&format!(
        r#"[{{"number":1,"headRefName":"cpd/p","baseRefName":"main","headRefOid":"{head}"}}]"#
    ));

    let out = fx.run(true);
    assert_eq!((out.exit, out.stderr.as_str()), (0, ""), "{}", out.stdout);
    let expected: BTreeSet<String> = [
        format!("remove {} (cpd/p/k)", renamed.display()),
        format!("keep {} (cpd/o/k): not merged", other.display()),
        format!("{}: 1 remove, 1 keep", fx.repo.dir().display()),
    ]
    .into_iter()
    .collect();
    assert_eq!(lines(&out.stdout), expected, "{}", out.stdout);
    assert!(!renamed.exists() && other.exists());
    assert_eq!(fx.branches(), set(&["cpd/o/k", "main"]));
}

#[test]
fn drop_removes_clean_unmerged_worktrees_of_the_named_lead_branch() {
    let mut fx = Fixture::new();
    let old = fx.worktree("old", "cpd/old", "main", Some("old.txt"));
    let a1 = fx.worktree("a1", "cpd/old--j-a1", "main", Some("a1.txt"));
    let a2 = fx.worktree("a2", "cpd/old--j-a2", "main", Some("a2.txt"));
    std::fs::write(a2.join("scratch.txt"), "wip\n").unwrap();
    let other = fx.worktree("other", "cpd/other", "main", Some("other.txt"));
    let remotes = fx.remote_branches();

    let expected: BTreeSet<String> = [
        format!("remove {} (cpd/old): dropped", old.display()),
        format!("remove {} (cpd/old--j-a1): dropped", a1.display()),
        format!("keep {} (cpd/old--j-a2): dirty", a2.display()),
        format!("keep {} (cpd/other): not merged", other.display()),
        format!("{}: 2 remove, 2 keep", fx.repo.dir().display()),
    ]
    .into_iter()
    .collect();

    // Without --drop all of them are kept.
    let out = fx.run(true);
    assert_eq!((out.exit, out.stderr.as_str()), (0, ""), "{}", out.stdout);
    assert!(
        !out.stdout.lines().any(|l| l.starts_with("remove ")),
        "{}",
        out.stdout
    );

    let out = fx.prune(&["--drop", "cpd/old"]);
    assert_eq!((out.exit, out.stderr.as_str()), (0, ""), "{}", out.stdout);
    assert_eq!(lines(&out.stdout), expected, "{}", out.stdout);
    assert!(old.exists() && a1.exists());

    let out = fx.prune(&["--apply", "--drop", "cpd/old"]);
    assert_eq!((out.exit, out.stderr.as_str()), (0, ""), "{}", out.stdout);
    assert_eq!(lines(&out.stdout), expected, "{}", out.stdout);
    assert!(!old.exists() && !a1.exists() && a2.exists() && other.exists());
    assert!(a2.join("scratch.txt").exists());
    assert_eq!(fx.branches(), set(&["cpd/old--j-a2", "cpd/other", "main"]));
    assert_eq!(fx.remote_branches(), remotes);
}

#[test]
fn a_failing_cpd_exits_2_and_removes_nothing() {
    // The old test's "cpd fails" is now: the store cannot be read.
    let mut fx = Fixture::new();
    let ff = fx.worktree("ff", "cpd/ff", "main", None);
    std::fs::create_dir_all(fx.home.root()).unwrap();
    std::fs::write(fx.home.db(), "this is not a database\n").unwrap();
    let worktrees = fx.worktrees_listed();
    let branches = fx.branches();

    let out = fx.prune(&["--apply", "--drop", "cpd/ff"]);
    assert_eq!(out.exit, 2, "{}{}", out.stdout, out.stderr);
    assert_eq!(out.stdout, "");
    assert!(ff.exists());
    assert_eq!(fx.worktrees_listed(), worktrees);
    assert_eq!(fx.branches(), branches);
    assert!(
        fx.removals_and_deletions().is_empty(),
        "{:?}",
        fx.removals_and_deletions()
    );
    assert!(
        fx.fakes
            .runner
            .calls()
            .iter()
            .all(|c| !c.program.ends_with("gh"))
    );

    // The same when the database is not even a file.
    std::fs::remove_file(fx.home.db()).unwrap();
    std::fs::create_dir(fx.home.db()).unwrap();
    let out = fx.prune(&["--apply"]);
    assert_eq!(out.exit, 2, "{}{}", out.stdout, out.stderr);
    assert!(ff.exists());
}
