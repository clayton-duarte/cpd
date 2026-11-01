//! The fixtures `merge_tests` and `rebase_tests` share: a `World` (a Universe, the `GitRoute` and a
//! `FakeRunner` over it, a manual clock, a temp dir), histories built from file lists (`snap`, `by`), and the
//! calls of the product's form (`git -C <dir> <args>`). No test of the two files runs git or any process.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use super::{Commit, GitRoute, Oid, Repo, Universe};
use crate::clock::ManualClock;
use crate::host::deadline::Failed;
use crate::host::runner::{FakeRunner, Runner};

pub(super) struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub(super) struct World {
    tmp: TempDir,
    pub universe: Arc<Mutex<Universe>>,
    pub route: Arc<GitRoute>,
    pub runner: FakeRunner,
    dirs: usize,
    worktrees: Vec<(PathBuf, PathBuf)>,  // (worktree_path, repo_dir)
}

pub(super) fn world() -> World {
    let clock = ManualClock::new();
    let mut universe = Universe::new(Arc::new(clock.clone()));
    universe.set_global("user.name", "cpd test");
    universe.set_global("user.email", "test@cpd.invalid");
    let universe = Arc::new(Mutex::new(universe));
    let route = Arc::new(GitRoute::new(universe.clone()));
    let runner = FakeRunner::new(clock.clone()).with_git(&route);
    World { tmp: tempfile::tempdir().unwrap(), universe, route, runner, dirs: 0, worktrees: Vec::new() }
}

pub(super) fn p(path: &Path) -> &str {
    path.to_str().expect("a temp path is UTF-8")
}

/// The first seven hex digits of a sha: how git abbreviates one in a small repo.
pub(super) fn short(sha: impl std::fmt::Display) -> String {
    sha.to_string()[..7].to_owned()
}

fn run(runner: &dyn Runner, cmd: &mut Command) -> Out {
    match runner.output(cmd, "git", Duration::from_secs(60)) {
        Ok(o) => Out {
            code: std::os::unix::process::ExitStatusExt::into_raw(o.status) >> 8,
            stdout: String::from_utf8(o.stdout).unwrap(),
            stderr: String::from_utf8(o.stderr).unwrap(),
        },
        Err(Failed::Io(e)) => panic!("git could not start: {e}"),
        Err(Failed::Timeout(e)) => panic!("git timed out: {e:?}"),
    }
}

impl World {
    /// The temp root, canonical (what real git prints for a path under it).
    pub fn root(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }
    pub fn u<R>(&self, f: impl FnOnce(&mut Universe) -> R) -> R {
        f(&mut self.universe.lock().unwrap())
    }
    /// A new path under the temp root that exists nowhere yet.
    pub fn fresh(&mut self, stem: &str) -> PathBuf {
        self.dirs += 1;
        self.root().join(format!("{stem}{}", self.dirs))
    }
    /// A repo on `main` whose one commit, `base`, holds `files`.
    pub fn repo(&mut self, files: &[(&str, &str)]) -> Repo {
        let dir = self.fresh("repo");
        let repo = Repo::init(&self.universe, &dir);
        for (path, text) in files {
            repo.write(path, text);
        }
        repo.commit("base");
        repo
    }
    fn build(&self, repo: &Repo, parents: &[&Oid], files: &[(&str, &str)], message: &str) -> Oid {
        self.u(|u| {
            let blobs: Vec<(&str, Oid)> = files.iter().map(|(p, t)| (*p, u.blob(repo.id(), t.as_bytes()))).collect();
            let entries: Vec<(&str, &Oid)> = blobs.iter().map(|(p, b)| (*p, b)).collect();
            let tree = u.tree(repo.id(), &entries);
            let parents: Vec<Oid> = parents.iter().map(|p| (*p).clone()).collect();
            u.commit(repo.id(), &tree, &parents, message)
        })
    }
    /// A commit of exactly `files` over `parent` (or a root commit), authored by the default user, moving no ref.
    pub fn snap(&self, repo: &Repo, parent: Option<&Oid>, files: &[(&str, &str)], message: &str) -> Oid {
        self.build(repo, &parent.into_iter().collect::<Vec<_>>(), files, message)
    }
    /// [`World::snap`] authored by `<name> <<name>@x.invalid>`.
    pub fn by(&self, repo: &Repo, name: &str, parent: Option<&Oid>, files: &[(&str, &str)], message: &str) -> Oid {
        self.u(|u| {
            u.config_set(repo.id(), "user.name", name);
            u.config_set(repo.id(), "user.email", &format!("{name}@x.invalid"));
        });
        let commit = self.snap(repo, parent, files, message);
        self.u(|u| {
            u.config_set(repo.id(), "user.name", "cpd test");
            u.config_set(repo.id(), "user.email", "test@cpd.invalid");
        });
        commit
    }
    /// A commit with these parents (a merge when there are two), authored by the default user, moving no ref.
    pub fn join(&self, repo: &Repo, parents: &[&Oid], files: &[(&str, &str)], message: &str) -> Oid {
        self.build(repo, parents, files, message)
    }
    /// `refs/heads/<name>` at `oid`.
    pub fn branch(&self, repo: &Repo, name: &str, oid: &Oid) {
        self.u(|u| u.set_ref(repo.id(), &format!("refs/heads/{name}"), oid, "branch: test"));
    }
    /// `refs/remotes/origin/<name>` at `oid`.
    pub fn tracking(&self, repo: &Repo, name: &str, oid: &Oid) {
        self.u(|u| u.set_ref(repo.id(), &format!("refs/remotes/origin/{name}"), oid, "fetch"));
    }
    /// `git -C <dir> <args...>`, the form of the product.
    pub fn git(&self, dir: &Path, args: &[&str]) -> Out {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        run(&self.runner, &mut cmd)
    }
    /// [`World::git`] with the options the product puts before a rebase: `-c core.editor=true -c sequence.editor=true`.
    pub fn git_no_editor(&self, dir: &Path, args: &[&str]) -> Out {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(["-c", "core.editor=true", "-c", "sequence.editor=true"]).args(args);
        run(&self.runner, &mut cmd)
    }
    /// [`World::git`] that must exit 0; returns stdout.
    pub fn ok(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git(dir, args);
        assert_eq!(out.code, 0, "git {args:?} said: {}", out.stderr);
        out.stdout
    }
    /// What `git rev-parse --verify <rev>` prints in `dir`.
    pub fn rev(&self, dir: &Path, rev: &str) -> Oid {
        Oid::new(self.ok(dir, &["rev-parse", "--verify", rev]).trim())
    }
    /// A linked worktree on the new branch `branch` at `at`, made by `worktree add`.
    pub fn worktree(&mut self, repo: &Repo, branch: &str, at: &Oid) -> PathBuf {
        let path = self.fresh("wt");
        self.ok(repo.dir(), &["worktree", "add", "--no-track", "-b", branch, p(&path), at.as_str()]);
        self.worktrees.push((path.clone(), repo.dir().to_path_buf()));
        path
    }
    /// A linked worktree with a detached `HEAD` at `at` (the gate worktree of the product).
    pub fn detached(&mut self, repo: &Repo, at: &Oid) -> PathBuf {
        let path = self.fresh("gate");
        self.ok(repo.dir(), &["worktree", "add", "--detach", p(&path), at.as_str()]);
        self.worktrees.push((path.clone(), repo.dir().to_path_buf()));
        path
    }

    // ---- what a test reads ----

    /// The text of the blob.
    pub fn text(&self, blob: &Oid) -> String {
        String::from_utf8(self.u(|u| u.blob_bytes(blob)).expect("the blob exists")).unwrap()
    }
    /// The file `path` of the tree of `rev` (a commit or a tree), as `git rev-parse <rev>:<path>` finds it.
    pub fn file_at(&self, dir: &Path, rev: &str, path: &str) -> Option<String> {
        let out = self.git(dir, &["rev-parse", "--verify", "--quiet", &format!("{rev}:{path}")]);
        (out.code == 0).then(|| self.text(&Oid::new(out.stdout.trim())))
    }
    /// Every path of the tree of `rev`, sorted.
    pub fn paths_at(&self, dir: &Path, rev: &str) -> Vec<String> {
        self.ok(dir, &["ls-tree", "-r", "--name-only", rev]).lines().map(str::to_owned).collect()
    }
    /// The commit's data.
    pub fn commit_of(&self, oid: &Oid) -> Commit {
        self.u(|u| u.commit_data(oid)).expect("a commit")
    }
    /// A file of the working tree at `dir`.
    pub fn read(&self, dir: &Path, path: &str) -> Option<String> {
        std::fs::read_to_string(dir.join(path)).ok()
    }
    /// Writes a file of the working tree at `dir`.
    pub fn write(&self, dir: &Path, path: &str, text: &str) {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    /// The index of the worktree at `dir` as `(path, stage)`, sorted.
    pub fn stages(&self, dir: &Path) -> Vec<(String, u8)> {
        self.u(|u| u.index(&u.worktree_of(dir).unwrap())).into_iter().map(|(p, _, s)| (p, s)).collect()
    }
    /// The text the index of the worktree at `dir` holds for `path` at `stage`.
    pub fn staged(&self, dir: &Path, path: &str, stage: u8) -> Option<String> {
        let blob = self.u(|u| u.index(&u.worktree_of(dir).unwrap())).into_iter().find(|(p, _, s)| p == path && *s == stage).map(|(_, b, _)| b);
        blob.map(|b| self.text(&b))
    }
    /// `git add <path>` of the working file as it is: stage 0, the other stages of the path gone.
    pub fn add(&self, dir: &Path, path: &str) {
        let bytes = std::fs::read(dir.join(path)).expect("the file to add exists");
        self.u(|u| {
            let wt = u.worktree_of(dir).unwrap();
            let repo = u.repo_of(dir).unwrap();
            let blob = u.blob(repo, &bytes);
            let mut index = u.effective_index(&wt);
            index.retain(|(p, _, _)| p != path);
            index.push((path.to_owned(), blob, 0));
            u.set_index(&wt, index);
        });
    }
    /// `git rm <path>`: the file and every stage of it gone.
    pub fn remove(&self, dir: &Path, path: &str) {
        let _ = std::fs::remove_file(dir.join(path));
        self.u(|u| {
            let wt = u.worktree_of(dir).unwrap();
            let mut index = u.effective_index(&wt);
            index.retain(|(p, _, _)| p != path);
            u.set_index(&wt, index);
        });
    }
    /// The absolute path `git rev-parse --path-format=absolute --git-path <name>` prints in `dir`.
    pub fn git_path(&self, dir: &Path, name: &str) -> PathBuf {
        PathBuf::from(self.ok(dir, &["rev-parse", "--path-format=absolute", "--git-path", name]).trim())
    }
}

impl Drop for World {
    fn drop(&mut self) {
        // Remove all worktrees to prevent leaks: git keeps administrative state in .git/worktrees/
        // that must be cleaned up with `git worktree remove`, not just by deleting the directory.
        for (wt_path, repo_dir) in &self.worktrees {
            let mut cmd = Command::new("git");
            cmd.arg("-C").arg(repo_dir).args(["worktree", "remove", "--force", "--force", p(wt_path)]);
            let _ = self.runner.output(&mut cmd, "git", Duration::from_secs(60));
        }
    }
}

/// Lines `1\n2\n...` up to `n`, with `edits` (1-based line, new text) applied.
pub(super) fn numbered(n: usize, edits: &[(usize, &str)]) -> String {
    (1..=n)
        .map(|i| match edits.iter().find(|(line, _)| *line == i) {
            Some((_, text)) => format!("{text}\n"),
            None => format!("{i}\n"),
        })
        .collect()
}
