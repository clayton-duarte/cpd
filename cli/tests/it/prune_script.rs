//! `scripts/prune-worktrees.sh`: the one-time cleanup of leftover worktrees whose work is on
//! origin/main (hs-d138, hs-d143).

use crate::common;

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
    /// A repo with a bare `origin` (both on `main`) and a fake `cpd` that knows no Plan.
    fn new() -> Fixture {
        let env = TestEnv::new();
        let repo = env.git_repo("work");
        let origin = env.root().join("repos/origin.git");
        env.git(env.root(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        env.git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        env.git(&repo, &["push", "-q", "origin", "main"]);
        let fx = Fixture { env, repo, origin };
        fx.fake_cpd(&[]);
        fx
    }

    /// The fake `cpd` behind `CPD_BIN`: `plan list` prints one Plan per `(id, phase, show)` and
    /// `plan show --plan <id>` prints `show`; any other call fails.
    fn fake_cpd(&self, plans: &[(&str, &str, &str)]) {
        let list: Vec<String> = plans.iter().map(|(id, phase, _)| format!(r#"{{"id":"{id}","phase":"{phase}"}}"#)).collect();
        let mut body = format!(
            "case \"$1 $2\" in\n\"plan list\") cat <<'EOF'\n{{\"ok\":true,\"plans\":[{}]}}\nEOF\n;;\n\"plan show\") case \"$4\" in\n",
            list.join(",")
        );
        for (id, _, show) in plans {
            body += &format!("{id}) cat <<'EOF'\n{show}\nEOF\n;;\n");
        }
        body += "*) exit 1 ;; esac ;;\n*) exit 1 ;; esac";
        self.env.fake("cpd-fake", &body);
    }

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
        self.run_with(if apply { &["--apply"] } else { &[] })
    }

    fn run_with(&self, args: &[&str]) -> (i32, String, String) {
        let mut cmd = self.env.command("sh");
        cmd.arg(script()).env("CPD_BIN", self.env.bin().join("cpd-fake"));
        let out = cmd.args(args).arg(&self.repo).output().unwrap();
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
    let fx = Fixture::new();

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
    let fx = Fixture::new();
    fx.env.fake_gh("echo 'gh: not logged in' >&2; exit 1");

    let (exit, _, stderr) = fx.run(false);
    assert_eq!(exit, 1);
    assert!(stderr.contains("not logged in"), "{stderr}");
}

const NO_PRS: &str = "echo '[]'";

/// The `plan show` of Plan `q`: PR `followups` in dispatch (Job `j`, Attempt `j-a1`) and PR `cpd`
/// merged (Job `old`, Attempt `old-a1`).
const SHOW_Q: &str = r#"{"ok":true,"prs":[
{"key":"followups","status":"dispatch","lead_branch":"cpd/q/followups","jobs":["j"]},
{"key":"cpd","status":"merged","lead_branch":"cpd/q/cpd","jobs":["old"]}],
"jobs":[{"key":"j","attempts":[{"id":"j-a1"}]},{"key":"old","attempts":[{"id":"old-a1"}]}]}"#;

#[test]
fn keeps_what_cpd_still_runs() {
    let fx = Fixture::new();
    // Plan `r` is done: asking for its show would fail the fake.
    fx.fake_cpd(&[("q", "dispatch", SHOW_Q), ("r", "done", "")]);
    fx.env.fake_gh(NO_PRS);
    // All at origin/main's commit, so every one of them was removable before.
    let lead = fx.worktree("lead", "cpd/q/followups", "main", None);
    let attempt = fx.worktree("attempt", "cpd/q--j-a1", "main", None);
    let merged_lead = fx.worktree("merged-lead", "cpd/q/cpd", "main", None);
    let merged_attempt = fx.worktree("merged-attempt", "cpd/q--old-a1", "main", None);
    let wt = |p: &PathBuf, b: &str| format!("{} ({b})", p.display());
    let expected: BTreeSet<String> = [
        format!("keep {}: active", wt(&lead, "cpd/q/followups")),
        format!("keep {}: active", wt(&attempt, "cpd/q--j-a1")),
        format!("remove {}", wt(&merged_lead, "cpd/q/cpd")),
        format!("remove {}", wt(&merged_attempt, "cpd/q--old-a1")),
        format!("{}: 2 remove, 2 keep", fx.repo.display()),
    ]
    .into_iter()
    .collect();

    let (exit, stdout, stderr) = fx.run(false);
    assert_eq!((exit, stderr.as_str()), (0, ""), "{stdout}");
    assert_eq!(lines(&stdout), expected, "{stdout}");

    // Also with --apply and --drop of their Lead Branches.
    let (exit, stdout, stderr) = fx.run_with(&["--apply", "--drop", "cpd/q/followups", "--drop", "cpd/q"]);
    assert_eq!((exit, stderr.as_str()), (0, ""), "{stdout}");
    assert!(lead.exists() && attempt.exists());
    assert!(!merged_lead.exists() && !merged_attempt.exists());
    assert_eq!(
        fx.branches().lines().collect::<BTreeSet<_>>(),
        ["cpd/q--j-a1", "cpd/q/followups", "main"].into_iter().collect()
    );
}

#[test]
fn matches_a_legacy_lead_branch_renamed_locally() {
    let fx = Fixture::new();
    // Local `cpd/p/k`, whose PR #1 had head `cpd/p` and was squash-merged into main.
    let renamed = fx.worktree("renamed", "cpd/p/k", "main", Some("k.txt"));
    let head = fx.head(&renamed);
    let other = fx.worktree("other", "cpd/o/k", "main", Some("o.txt"));
    fx.env.fake_gh(&format!(
        r#"echo '[{{"number":1,"headRefName":"cpd/p","baseRefName":"main","headRefOid":"{head}"}}]'"#
    ));

    let (exit, stdout, stderr) = fx.run(true);
    assert_eq!((exit, stderr.as_str()), (0, ""), "{stdout}");
    let expected: BTreeSet<String> = [
        format!("remove {} (cpd/p/k)", renamed.display()),
        format!("keep {} (cpd/o/k): not merged", other.display()),
        format!("{}: 1 remove, 1 keep", fx.repo.display()),
    ]
    .into_iter()
    .collect();
    assert_eq!(lines(&stdout), expected, "{stdout}");
    assert!(!renamed.exists() && other.exists());
    assert_eq!(fx.branches().lines().collect::<BTreeSet<_>>(), ["cpd/o/k", "main"].into_iter().collect());
}

#[test]
fn drop_removes_clean_unmerged_worktrees_of_the_named_lead_branch() {
    let fx = Fixture::new();
    fx.env.fake_gh(NO_PRS);
    let old = fx.worktree("old", "cpd/old", "main", Some("old.txt"));
    let a1 = fx.worktree("a1", "cpd/old--j-a1", "main", Some("a1.txt"));
    let a2 = fx.worktree("a2", "cpd/old--j-a2", "main", Some("a2.txt"));
    fs::write(a2.join("scratch.txt"), "wip\n").unwrap();
    let other = fx.worktree("other", "cpd/other", "main", Some("other.txt"));
    let remotes = fx.remote_branches();

    let expected: BTreeSet<String> = [
        format!("remove {} (cpd/old): dropped", old.display()),
        format!("remove {} (cpd/old--j-a1): dropped", a1.display()),
        format!("keep {} (cpd/old--j-a2): dirty", a2.display()),
        format!("keep {} (cpd/other): not merged", other.display()),
        format!("{}: 2 remove, 2 keep", fx.repo.display()),
    ]
    .into_iter()
    .collect();

    // Without --drop all of them are kept.
    let (exit, stdout, stderr) = fx.run(true);
    assert_eq!((exit, stderr.as_str()), (0, ""), "{stdout}");
    assert!(!stdout.lines().any(|l| l.starts_with("remove ")), "{stdout}");

    let (exit, stdout, stderr) = fx.run_with(&["--drop", "cpd/old"]);
    assert_eq!((exit, stderr.as_str()), (0, ""), "{stdout}");
    assert_eq!(lines(&stdout), expected, "{stdout}");
    assert!(old.exists() && a1.exists());

    let (exit, stdout, stderr) = fx.run_with(&["--apply", "--drop", "cpd/old"]);
    assert_eq!((exit, stderr.as_str()), (0, ""), "{stdout}");
    assert_eq!(lines(&stdout), expected, "{stdout}");
    assert!(!old.exists() && !a1.exists() && a2.exists() && other.exists());
    assert!(a2.join("scratch.txt").exists());
    assert_eq!(
        fx.branches().lines().collect::<BTreeSet<_>>(),
        ["cpd/old--j-a2", "cpd/other", "main"].into_iter().collect()
    );
    assert_eq!(fx.remote_branches(), remotes);
}

#[test]
fn a_failing_cpd_exits_2_and_removes_nothing() {
    let fx = Fixture::new();
    fx.env.fake_gh(NO_PRS);
    let ff = fx.worktree("ff", "cpd/ff", "main", None);
    fx.env.fake("cpd-fake", "echo 'cpd: broken' >&2; exit 1");
    let worktrees = fx.worktrees();
    let branches = fx.branches();

    let (exit, stdout, stderr) = fx.run_with(&["--apply", "--drop", "cpd/ff"]);
    assert_eq!(exit, 2, "{stdout}{stderr}");
    assert_eq!(stdout, "");
    assert!(ff.exists());
    assert_eq!(fx.worktrees(), worktrees);
    assert_eq!(fx.branches(), branches);
    assert!(fx.env.calls("gh").is_empty());

    // The same when cpd is not there at all.
    let out = fx
        .env
        .command("sh")
        .arg(script())
        .arg("--apply")
        .arg(&fx.repo)
        .env("CPD_BIN", fx.env.root().join("no-such-cpd"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(ff.exists());
}
