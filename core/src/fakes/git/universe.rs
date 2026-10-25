//! The model: one arena of objects for all repos, and per repo its refs, reflogs, config, worktrees and
//! the objects it holds. Pure data and deterministic; the only time is the injected `Clock`.
//!
//! STUB: every body is `todo!()`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::clock::Clock;

/// An object id: 40 lowercase hex characters, the sha1 of the object's canonical text (git's, so
/// `blob "hello\n"` is `ce01362…`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Oid(String);

impl Oid {
    /// Panics unless `hex` is 40 lowercase hex characters.
    pub fn new(_hex: &str) -> Oid {
        todo!("Oid::new")
    }
    pub fn as_str(&self) -> &str {
        todo!("Oid::as_str")
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("Oid display")
    }
}

/// A repo of the Universe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RepoId(usize);

/// What a worktree's `HEAD` is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    /// Symbolic: the short name of a branch (`main` is `refs/heads/main`), born or not.
    Branch(String),
    Detached(Oid),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub tree: Oid,
    pub parents: Vec<Oid>,
    /// `<user.name> <<user.email>>`.
    pub author: String,
    /// As given, without the trailing newline the canonical text adds.
    pub message: String,
    /// Seconds since the Unix epoch: [`Universe::EPOCH`] plus the virtual time that has passed on the clock
    /// since the Universe was made.
    pub time: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflogEntry {
    /// `None` for the entry that created the ref.
    pub old: Option<Oid>,
    pub new: Oid,
    /// The tree of the commit `new` (what `log -g --format=%T` prints).
    pub tree: Oid,
    pub message: String,
}

/// A worktree of a repo: the main one or a linked one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// `None` for the main worktree; else the name of `<git dir>/worktrees/<id>`.
    pub id: Option<String>,
}

/// An object that the repo asked about is not held by it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownObject(pub Oid);

pub struct Universe {
    _stub: (),
}

impl Universe {
    /// The commit time of the Universe's start: 2023-11-14T22:13:20Z.
    pub const EPOCH: u64 = 1_700_000_000;

    /// A Universe with no repo and no config. Time is `clock.now()` at this call; no wall time is read.
    pub fn new(_clock: Arc<dyn Clock>) -> Universe {
        todo!("Universe::new")
    }

    // ---- objects: ids from the canonical text, held per repo ----

    /// Stores a blob and adds it to what `repo` holds.
    pub fn blob(&mut self, _repo: RepoId, _bytes: &[u8]) -> Oid {
        todo!("blob")
    }
    /// Stores a FLAT tree (every path is a blob entry, mode 100644, `a/b.txt` is one entry) and adds it
    /// to what `repo` holds. Entry order does not matter.
    pub fn tree(&mut self, _repo: RepoId, _entries: &[(&str, &Oid)]) -> Oid {
        todo!("tree")
    }
    /// Stores a commit (author and committer from `user.*`, time from the clock) and adds it to what `repo`
    /// holds. Panics when no `user.name` and `user.email` is configured. It moves no ref.
    pub fn commit(&mut self, _repo: RepoId, _tree: &Oid, _parents: &[Oid], _message: &str) -> Oid {
        todo!("commit")
    }
    pub fn blob_bytes(&self, _oid: &Oid) -> Option<Vec<u8>> {
        todo!("blob_bytes")
    }
    /// The entries of a flat tree sorted by path.
    pub fn tree_entries(&self, _oid: &Oid) -> Option<Vec<(String, Oid)>> {
        todo!("tree_entries")
    }
    pub fn commit_data(&self, _oid: &Oid) -> Option<Commit> {
        todo!("commit_data")
    }
    /// Whether `repo` holds the object.
    pub fn has(&self, _repo: RepoId, _oid: &Oid) -> bool {
        todo!("has")
    }
    /// Adds `oid` and everything it reaches (a commit: its tree, blobs and ancestors) to what `to` holds.
    pub fn copy(&mut self, _from: RepoId, _to: RepoId, _oid: &Oid) {
        todo!("copy")
    }

    // ---- repos and worktrees ----

    /// Makes `dir` (and parents) a repo: `<dir>/.git/` a real directory, or for `bare` `dir` itself is the
    /// git dir. `HEAD` is the unborn branch `main`.
    pub fn init(&mut self, _dir: &Path, _bare: bool) -> RepoId {
        todo!("init")
    }
    /// The repo whose worktree (or bare git dir) is the nearest ancestor-or-self of `path`.
    pub fn repo_of(&self, _path: &Path) -> Option<RepoId> {
        todo!("repo_of")
    }
    pub fn is_bare(&self, _repo: RepoId) -> bool {
        todo!("is_bare")
    }
    /// The common git dir: `<main worktree>/.git`, or the bare repo's directory.
    pub fn git_dir(&self, _repo: RepoId) -> PathBuf {
        todo!("git_dir")
    }
    /// The main worktree first, then the linked ones in the order they were added. A bare repo has none.
    pub fn worktrees(&self, _repo: RepoId) -> Vec<Worktree> {
        todo!("worktrees")
    }
    /// Makes `path` a real directory with a `.git` FILE `gitdir: <git dir>/worktrees/<id>` and makes that
    /// admin directory; `<id>` is the last component of `path`, with the first free number appended when
    /// taken (`wt`, `wt1`, ...). Its `HEAD` is `head`.
    pub fn add_worktree(&mut self, _repo: RepoId, _path: &Path, _head: Head) -> Worktree {
        todo!("add_worktree")
    }
    /// The git dir of a worktree: the common one for the main worktree, `<git dir>/worktrees/<id>` for a
    /// linked one. Panics when `wt` is not a worktree path.
    pub fn admin_dir(&self, _wt: &Path) -> PathBuf {
        todo!("admin_dir")
    }

    // ---- refs, HEAD, reflogs ----

    /// Points `name` (a full ref name) at `new`, and appends a reflog entry with `message`.
    pub fn set_ref(&mut self, _repo: RepoId, _name: &str, _new: &Oid, _message: &str) {
        todo!("set_ref")
    }
    pub fn ref_oid(&self, _repo: RepoId, _name: &str) -> Option<Oid> {
        todo!("ref_oid")
    }
    pub fn delete_ref(&mut self, _repo: RepoId, _name: &str) {
        todo!("delete_ref")
    }
    /// The direct refs whose full name starts with `prefix`, sorted by name.
    pub fn refs(&self, _repo: RepoId, _prefix: &str) -> Vec<(String, Oid)> {
        todo!("refs")
    }
    /// Makes `name` (for example `refs/remotes/origin/HEAD`) a symbolic ref to the full ref `target`.
    pub fn set_symbolic_ref(&mut self, _repo: RepoId, _name: &str, _target: &str) {
        todo!("set_symbolic_ref")
    }
    pub fn symbolic_ref(&self, _repo: RepoId, _name: &str) -> Option<String> {
        todo!("symbolic_ref")
    }
    /// The `HEAD` of the worktree at `wt`; each worktree has its own, the refs are shared.
    pub fn head(&self, _wt: &Path) -> Head {
        todo!("head")
    }
    pub fn set_head(&mut self, _wt: &Path, _head: Head) {
        todo!("set_head")
    }
    /// The reflog of `name`, newest first; empty for a ref that never moved.
    pub fn reflog(&self, _repo: RepoId, _name: &str) -> Vec<ReflogEntry> {
        todo!("reflog")
    }
    /// The tips of `refs/remotes/<remote>/*` (what `--remotes=<remote>` selects), sorted by ref name.
    pub fn remote_tips(&self, _repo: RepoId, _remote: &str) -> Vec<Oid> {
        todo!("remote_tips")
    }

    // ---- config: multi-valued; the global one comes first ----

    /// Sets a global (`GIT_CONFIG_GLOBAL`) value, replacing the global values of the key.
    pub fn set_global(&mut self, _key: &str, _value: &str) {
        todo!("set_global")
    }
    /// Appends a value of the repo's own config.
    pub fn config_add(&mut self, _repo: RepoId, _key: &str, _value: &str) {
        todo!("config_add")
    }
    /// Replaces every value of the key in the repo's own config.
    pub fn config_set(&mut self, _repo: RepoId, _key: &str, _value: &str) {
        todo!("config_set")
    }
    /// Every value of the key: the global ones, then the repo's, each in the order added. Section and
    /// variable names are case-insensitive, the subsection (`url.<x>.`) is not.
    pub fn config_values(&self, _repo: RepoId, _key: &str) -> Vec<String> {
        todo!("config_values")
    }
    /// `url` rewritten by the longest matching `url.<base>.insteadOf` (git's `remote get-url`).
    pub fn resolve_url(&self, _repo: RepoId, _url: &str) -> String {
        todo!("resolve_url")
    }

    // ---- the index of a worktree: a real file, rewritten by rename ----

    /// Records `path` at `stage` (0 merged, 1 base, 2 ours, 3 theirs) with `blob`, replacing that stage of the
    /// path, and rewrites the worktree's `index` file by rename: a new inode at every call.
    pub fn stage(&mut self, _wt: &Path, _path: &str, _blob: &Oid, _stage: u8) {
        todo!("stage")
    }
    /// The entries sorted by path, then stage.
    pub fn index(&self, _wt: &Path) -> Vec<(String, Oid, u8)> {
        todo!("index")
    }
    /// `<admin dir of wt>/index`.
    pub fn index_path(&self, _wt: &Path) -> PathBuf {
        todo!("index_path")
    }

    // ---- ancestry ----

    /// The best common ancestor of two commits (on a criss-cross, the first); `None` for unrelated histories.
    pub fn merge_base(&self, _repo: RepoId, _a: &Oid, _b: &Oid) -> Option<Oid> {
        todo!("merge_base")
    }
    /// Whether `a` is an ancestor of `b` (a commit is its own ancestor); the first object `repo` does not
    /// hold is the error.
    pub fn is_ancestor(&self, _repo: RepoId, _a: &Oid, _b: &Oid) -> Result<bool, UnknownObject> {
        todo!("is_ancestor")
    }
    /// The commits reachable from `include` and not from `exclude`, newest first (by commit time; a child
    /// always before its parents); `no_merges` drops the commits with two parents or more.
    pub fn rev_list(&self, _repo: RepoId, _sel: &super::rev::Selection, _no_merges: bool) -> Vec<Oid> {
        todo!("rev_list")
    }
}
