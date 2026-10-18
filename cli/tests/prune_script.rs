//! `scripts/prune-worktrees.sh`: the one-time cleanup of leftover worktrees whose work is on
//! origin/main (hs-d138, hs-d143).

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use common::TestEnv;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/prune-worktrees.sh")
}

/// A repo with a bare `origin`, one worktree per case, and a fake `gh` listing the merged PRs.
struct Fixture {
    env: TestEnv,
    repo: PathBuf,
    origin: PathBuf,
}

impl Fixture {
    /// Adds worktree `wt-<dir>` on new branch `branch` from `start`, with one commit in
    /// `file`; returns its path.
    fn worktree(&self, dir: &str, branch: &str, start: &str, file: Option<&str>) -> PathBuf {
        let wt = self.env.root().canonicalize().unwrap().join(format!("wt-{dir}"));
        self.env.git(&self.repo, &["worktree", "add", "-q", "-b", branch, wt.to_str().unwrap(), start]);
        if let Some(file) = file {
            self.commit(&wt, file);
        }
        wt
    }

    fn commit(&self, wt: &Path, file: &str) -> String {
        fs::write(wt.join(file), format!("{file}\n")).unwrap();
        self.env.git(wt, &["add", "-A"]);
        self.env.git(wt, &["commit", "-q", "-m", file]);
        self.env.git(wt, &["rev-parse", "HEAD"])
    }

    fn head(&self, wt: &Path) -> String {
        self.env.git(wt, &["rev-parse", "HEAD"])
    }

    fn push(&self, wt: &Path, branch: &str) {
        self.env.git(wt, &["push", "-q", "origin", &format!("{branch}:refs/heads/{branch}")]);
    }

    fn run(&self, apply: bool) -> (i32, String, String) {
        let mut cmd = self.env.command("sh");
        cmd.arg(script());
        if apply {
            cmd.arg("--apply");
        }
        let out = cmd.arg(&self.repo).output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    }

    fn worktrees(&self) -> String {
        self.env.git(&self.repo, &["worktree", "list"])
    }

    fn branches(&self) -> String {
        self.env.git(&self.repo, &["branch", "--format=%(refname:short)"])
    }

    fn remote_branches(&self) -> String {
        self.env.git(&self.origin, &["branch", "--format=%(refname:short) %(objectname)"])
    }
}

fn lines(text: &str) -> BTreeSet<String> {
    text.lines().map(str::to_owned).collect()
}

#[test]
fn prunes_exactly_the_worktrees_whose_work_is_on_main() {
    let env = TestEnv::new();
    let repo = env.git_repo("work");
    let origin = env.root().join("repos/origin.git");
    env.git(env.root(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    env.git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    env.git(&repo, &["push", "-q", "origin", "main"]);
    let fx = Fixture { env, repo, origin };

    // Fast-forwarded into origin/main.
    let ff = fx.worktree("ff", "cpd/ff", "main", Some("ff.txt"));
    fx.push(&ff, "cpd/ff");
    // A Lead branch squash-merged into main (PR #1), and an Attempt branch inside its head.
    let x = fx.worktree("x", "cpd/x", "main", Some("x1.txt"));
    let attempt = fx.worktree("attempt", "cpd/x--j-a1", "cpd/x", None);
    let x_head = fx.commit(&x, "x2.txt");
    fx.push(&x, "cpd/x");
    // A PR (#2) merged into cpd/y (#3), which merged into main.
    let chain = fx.worktree("chain", "cpd/chain", "main", Some("chain.txt"));
    let chain_head = fx.head(&chain);
    // A commit after its PR's head (#4).
    let late = fx.worktree("late", "cpd/late", "main", Some("late1.txt"));
    let late_pr_head = fx.head(&late);
    fx.commit(&late, "late2.txt");
    // Dirty (its HEAD is on main) and unmerged.
    let dirty = fx.worktree("dirty", "cpd/dirty", "main", None);
    fs::write(dirty.join("scratch.txt"), "wip\n").unwrap();
    let wip = fx.worktree("wip", "cpd/wip", "main", Some("wip.txt"));
    fx.push(&wip, "cpd/wip");

    // Main moves on: the fast-forward, and the squash merges.
    fx.env.git(&fx.repo, &["push", "-q", "origin", "cpd/ff:refs/heads/main"]);
    fx.env.git(&fx.repo, &["fetch", "-q", "origin"]);
    for squash in ["squash-x.txt", "squash-chain.txt", "squash-late.txt"] {
        let main = fx.env.root().join("main-wt");
        if !main.exists() {
            fx.env.git(&fx.repo, &["worktree", "add", "-q", "--detach", main.to_str().unwrap(), "origin/main"]);
        }
        fx.commit(&main, squash);
        fx.env.git(&main, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    }
    fx.env.git(&fx.repo, &["worktree", "remove", "--force", fx.env.root().join("main-wt").to_str().unwrap()]);

    let prs = format!(
        r#"[{{"number":1,"headRefName":"cpd/x","baseRefName":"main","headRefOid":"{x_head}"}},
{{"number":2,"headRefName":"cpd/chain","baseRefName":"cpd/y","headRefOid":"{chain_head}"}},
{{"number":3,"headRefName":"cpd/y","baseRefName":"main","headRefOid":"{x_head}"}},
{{"number":4,"headRefName":"cpd/late","baseRefName":"main","headRefOid":"{late_pr_head}"}}]"#
    );
    fx.env.fake_gh(&format!("cat <<'EOF'\n{prs}\nEOF"));

    let removable = [&ff, &x, &attempt, &chain];
    let wt = |p: &PathBuf, b: &str| format!("{} ({b})", p.display());
    let expected: BTreeSet<String> = [
        format!("remove {}", wt(&ff, "cpd/ff")),
        format!("remove {}", wt(&x, "cpd/x")),
        format!("remove {}", wt(&attempt, "cpd/x--j-a1")),
        format!("remove {}", wt(&chain, "cpd/chain")),
        format!("keep {}: dirty", wt(&dirty, "cpd/dirty")),
        format!("keep {}: not merged", wt(&wip, "cpd/wip")),
        format!("keep {}: not merged", wt(&late, "cpd/late")),
        format!("{}: 4 remove, 3 keep", fx.repo.display()),
    ]
    .into_iter()
    .collect();

    let worktrees = fx.worktrees();
    let branches = fx.branches();
    let remotes = fx.remote_branches();

    // A dry run changes nothing.
    let (exit, stdout, stderr) = fx.run(false);
    assert_eq!((exit, stderr.as_str()), (0, ""), "{stdout}");
    assert_eq!(lines(&stdout), expected, "{stdout}");
    assert_eq!(fx.worktrees(), worktrees);
    assert_eq!(fx.branches(), branches);
    assert_eq!(fx.remote_branches(), remotes);
    assert!(removable.iter().all(|p| p.exists()));

    // gh was asked once, for the merged PRs of origin.
    let calls = fx.env.calls("gh");
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(&calls[0][..2], ["pr", "list"]);
    assert!(calls[0].windows(2).any(|w| w == ["--repo", "repos/origin"]), "{calls:?}");
    assert!(calls[0].windows(2).any(|w| w == ["--state", "merged"]), "{calls:?}");

    // --apply removes exactly the removable worktrees and their local branches.
    let (exit, stdout, stderr) = fx.run(true);
    assert_eq!((exit, stderr.as_str()), (0, ""), "{stdout}");
    assert_eq!(lines(&stdout), expected, "{stdout}");
    assert!(removable.iter().all(|p| !p.exists()));
    assert!([&dirty, &wip, &late].iter().all(|p| p.exists()));
    assert!(dirty.join("scratch.txt").exists());
    assert_eq!(
        fx.branches().lines().collect::<BTreeSet<_>>(),
        ["cpd/dirty", "cpd/late", "cpd/wip", "main"].into_iter().collect()
    );
    let listed = fx.worktrees();
    assert!(removable.iter().all(|p| !listed.contains(p.to_str().unwrap())), "{listed}");
    assert_eq!(listed.lines().count(), 4, "{listed}");
    // No remote branch is touched.
    assert_eq!(fx.remote_branches(), remotes);

    // A second run finds nothing to remove.
    let (exit, stdout, stderr) = fx.run(true);
    assert_eq!((exit, stderr.as_str()), (0, ""), "{stdout}");
    assert!(!stdout.lines().any(|l| l.starts_with("remove ")), "{stdout}");
    assert!(stdout.contains(&format!("{}: 0 remove, 3 keep", fx.repo.display())), "{stdout}");
    assert_eq!(fx.remote_branches(), remotes);
}

#[test]
fn a_failed_command_is_reported_and_exits_non_zero() {
    let env = TestEnv::new();
    let repo = env.git_repo("work");
    let origin = env.root().join("repos/origin.git");
    env.git(env.root(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    env.git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    env.git(&repo, &["push", "-q", "origin", "main"]);
    env.fake_gh("echo 'gh: not logged in' >&2; exit 1");

    let out = env.command("sh").arg(script()).arg(&repo).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not logged in"));
}
