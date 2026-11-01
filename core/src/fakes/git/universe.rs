//! The model: one arena of objects for all repos, and per repo its refs, reflogs, config, worktrees and
//! the objects it holds. Pure data and deterministic; the only time is the injected `Clock`.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use super::rev::Selection;
use super::sha1::sha1;
use crate::clock::Clock;

/// An object id: 40 lowercase hex characters, the sha1 of the object's canonical text (git's, so
/// `blob "hello\n"` is `ce01362…`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Oid(String);

impl Oid {
    /// Panics unless `hex` is 40 lowercase hex characters.
    pub fn new(hex: &str) -> Oid {
        assert!(is_oid(hex), "not an object id: {hex:?}");
        Oid(hex.to_owned())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// The 20 raw bytes a tree entry holds.
    fn raw(&self) -> Vec<u8> {
        (0..20).map(|i| u8::from_str_radix(&self.0[i * 2..i * 2 + 2], 16).expect("an Oid is hex")).collect()
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether `text` is 40 lowercase hex characters.
pub fn is_oid(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The id of an object: the sha1 of `<kind> <length>\0<body>`.
fn object_id(kind: &str, body: &[u8]) -> Oid {
    let mut text = format!("{kind} {}\0", body.len()).into_bytes();
    text.extend_from_slice(body);
    Oid(sha1(&text).iter().map(|b| format!("{b:02x}")).collect())
}

/// The id git gives a blob of `bytes`, without storing it.
pub fn blob_id(bytes: &[u8]) -> Oid {
    object_id("blob", bytes)
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

/// What an object is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Blob,
    Tree,
    Commit,
    /// An annotated tag: it names an object and has a message.
    Tag,
}

enum Object {
    Blob(Vec<u8>),
    /// Flat, sorted by path.
    Tree(Vec<(String, Oid)>),
    Commit(Commit),
    /// An annotated tag: the object it names.
    Tag(Oid),
}

struct Stored {
    object: Object,
    /// Order of creation: the tie-break of equal commit times.
    seq: usize,
}

struct WtData {
    path: PathBuf,
    id: Option<String>,
    head: Head,
    /// `Some(reason)` while locked (`worktree add --lock`); an empty reason is a lock without one.
    locked: Option<String>,
    /// `None` until something is staged; then every entry, sorted by path then stage.
    index: Option<Vec<(String, Oid, u8)>>,
    /// The rebase stopped in this worktree (`rebase-merge/` on disk), if any.
    rebase: Option<RebaseState>,
}

/// What a stopped `rebase` remembers: where it started, where it is and what is left to replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RebaseState {
    /// The tip the rebase started from (`rebase-merge/orig-head`).
    pub orig_head: Oid,
    /// The short name of the branch it moves when it ends; `None` for a detached `HEAD`.
    pub branch: Option<String>,
    pub onto: Oid,
    /// The commit whose replay stopped.
    pub stopped: Oid,
    /// The commits still to replay after `stopped`, oldest first.
    pub todo: Vec<Oid>,
}

struct RepoData {
    git_dir: PathBuf,
    bare: bool,
    /// The main worktree first (for a bare repo, a placeholder at the git dir that holds the `HEAD`).
    wts: Vec<WtData>,
    refs: BTreeMap<String, Oid>,
    symrefs: BTreeMap<String, String>,
    /// Oldest first.
    reflogs: HashMap<String, Vec<ReflogEntry>>,
    /// Keys normalized, in the order added.
    config: Vec<(String, String)>,
    have: HashSet<Oid>,
}

pub struct Universe {
    clock: Arc<dyn Clock>,
    start: Instant,
    global: Vec<(String, String)>,
    objects: HashMap<Oid, Stored>,
    repos: Vec<RepoData>,
}

/// Section and variable names are case-insensitive, the subsection (`url.<x>.`) is not.
pub(super) fn norm_key(key: &str) -> String {
    match (key.find('.'), key.rfind('.')) {
        (Some(first), Some(last)) if first != last => {
            format!("{}.{}.{}", key[..first].to_lowercase(), &key[first + 1..last], key[last + 1..].to_lowercase())
        }
        _ => key.to_lowercase(),
    }
}

impl Universe {
    /// The commit time of the Universe's start: 2023-11-14T22:13:20Z.
    pub const EPOCH: u64 = 1_700_000_000;

    /// A Universe with no repo and no config. Time is `clock.now()` at this call; no wall time is read.
    pub fn new(clock: Arc<dyn Clock>) -> Universe {
        let start = clock.now();
        Universe { clock, start, global: Vec::new(), objects: HashMap::new(), repos: Vec::new() }
    }

    fn repo_data(&self, repo: RepoId) -> &RepoData {
        &self.repos[repo.0]
    }
    fn repo_mut(&mut self, repo: RepoId) -> &mut RepoData {
        &mut self.repos[repo.0]
    }
    fn store(&mut self, repo: RepoId, id: Oid, object: Object) -> Oid {
        let seq = self.objects.len();
        self.objects.entry(id.clone()).or_insert(Stored { object, seq });
        self.repos[repo.0].have.insert(id.clone());
        id
    }

    // ---- objects: ids from the canonical text, held per repo ----

    /// Stores a blob and adds it to what `repo` holds.
    pub fn blob(&mut self, repo: RepoId, bytes: &[u8]) -> Oid {
        self.store(repo, blob_id(bytes), Object::Blob(bytes.to_vec()))
    }
    /// Stores a FLAT tree (every path is a blob entry, mode 100644, `a/b.txt` is one entry) and adds it
    /// to what `repo` holds. Entry order does not matter; a repeated path keeps the last entry, as staging
    /// it again would (a real tree has one entry per name).
    pub fn tree(&mut self, repo: RepoId, entries: &[(&str, &Oid)]) -> Oid {
        let mut by_path: std::collections::BTreeMap<String, Oid> = std::collections::BTreeMap::new();
        for (path, oid) in entries {
            by_path.insert((*path).to_owned(), (*oid).clone());
        }
        let sorted: Vec<(String, Oid)> = by_path.into_iter().collect();
        let mut body = Vec::new();
        for (path, oid) in &sorted {
            body.extend_from_slice(format!("100644 {path}\0").as_bytes());
            body.extend_from_slice(&oid.raw());
        }
        let id = object_id("tree", &body);
        self.store(repo, id, Object::Tree(sorted))
    }
    /// Stores a commit (author and committer from `user.*`, time from the clock) and adds it to what `repo`
    /// holds. Panics when no `user.name` and `user.email` is configured. It moves no ref.
    pub fn commit(&mut self, repo: RepoId, tree: &Oid, parents: &[Oid], message: &str) -> Oid {
        let user = |key: &str| {
            self.config_values(repo, key).pop().unwrap_or_else(|| panic!("a commit needs {key} configured"))
        };
        let author = format!("{} <{}>", user("user.name"), user("user.email"));
        self.commit_as(repo, &author, tree, parents, message)
    }
    /// [`Universe::commit`] by `author` (`<name> <<email>>`): what a rebase does to the commits it replays.
    pub(super) fn commit_as(&mut self, repo: RepoId, author: &str, tree: &Oid, parents: &[Oid], message: &str) -> Oid {
        let author = author.to_owned();
        let time = Self::EPOCH + self.clock.now().saturating_duration_since(self.start).as_secs();
        let mut body = format!("tree {tree}\n");
        for parent in parents {
            body.push_str(&format!("parent {parent}\n"));
        }
        body.push_str(&format!("author {author} {time} +0000\ncommitter {author} {time} +0000\n\n{message}\n"));
        let id = object_id("commit", body.as_bytes());
        let commit = Commit { tree: tree.clone(), parents: parents.to_vec(), author, message: message.to_owned(), time };
        self.store(repo, id, Object::Commit(commit))
    }
    /// Stores an annotated tag `name` of `target` by `tagger` (`<name> <<email>>`) and adds it to what `repo` holds.
    pub(super) fn tag_object(&mut self, repo: RepoId, target: &Oid, name: &str, tagger: &str, message: &str) -> Oid {
        let kind = match self.kind(target) {
            Some(Kind::Tree) => "tree",
            Some(Kind::Blob) => "blob",
            Some(Kind::Tag) => "tag",
            _ => "commit",
        };
        let time = Self::EPOCH + self.clock.now().saturating_duration_since(self.start).as_secs();
        let body = format!("object {target}\ntype {kind}\ntag {name}\ntagger {tagger} {time} +0000\n\n{message}\n");
        let id = object_id("tag", body.as_bytes());
        self.store(repo, id, Object::Tag(target.clone()))
    }
    /// The object an annotated tag names.
    pub(super) fn tag_target(&self, oid: &Oid) -> Option<Oid> {
        match &self.objects.get(oid)?.object {
            Object::Tag(target) => Some(target.clone()),
            _ => None,
        }
    }
    pub fn blob_bytes(&self, oid: &Oid) -> Option<Vec<u8>> {
        match &self.objects.get(oid)?.object {
            Object::Blob(bytes) => Some(bytes.clone()),
            _ => None,
        }
    }
    /// The entries of a flat tree sorted by path.
    pub fn tree_entries(&self, oid: &Oid) -> Option<Vec<(String, Oid)>> {
        match &self.objects.get(oid)?.object {
            Object::Tree(entries) => Some(entries.clone()),
            _ => None,
        }
    }
    pub fn commit_data(&self, oid: &Oid) -> Option<Commit> {
        match &self.objects.get(oid)?.object {
            Object::Commit(commit) => Some(commit.clone()),
            _ => None,
        }
    }
    /// What the object is, `None` when nobody made it.
    pub fn kind(&self, oid: &Oid) -> Option<Kind> {
        Some(match self.objects.get(oid)?.object {
            Object::Blob(_) => Kind::Blob,
            Object::Tree(_) => Kind::Tree,
            Object::Commit(_) => Kind::Commit,
            Object::Tag(_) => Kind::Tag,
        })
    }
    /// Whether `repo` holds the object.
    pub fn has(&self, repo: RepoId, oid: &Oid) -> bool {
        self.repo_data(repo).have.contains(oid)
    }
    /// Adds `oid` and everything it reaches (a commit: its tree, blobs and ancestors) to what `to` holds.
    pub fn copy(&mut self, _from: RepoId, to: RepoId, oid: &Oid) {
        let mut stack = vec![oid.clone()];
        while let Some(next) = stack.pop() {
            if self.repos[to.0].have.contains(&next) {
                continue;
            }
            match self.objects.get(&next).map(|s| &s.object) {
                Some(Object::Commit(c)) => {
                    stack.push(c.tree.clone());
                    stack.extend(c.parents.iter().cloned());
                }
                Some(Object::Tree(entries)) => stack.extend(entries.iter().map(|(_, o)| o.clone())),
                Some(Object::Tag(target)) => stack.push(target.clone()),
                Some(Object::Blob(_)) => {}
                None => continue,
            }
            self.repos[to.0].have.insert(next);
        }
    }

    // ---- repos and worktrees ----

    /// Makes `dir` (and parents) a repo: `<dir>/.git/` a real directory, or for `bare` `dir` itself is the
    /// git dir. `HEAD` is the unborn branch `main`.
    pub fn init(&mut self, dir: &Path, bare: bool) -> RepoId {
        let git_dir_of = |dir: &Path| if bare { dir.to_owned() } else { dir.join(".git") };
        std::fs::create_dir_all(git_dir_of(dir)).expect("the repo's directory can be made");
        let dir = dir.canonicalize().expect("the repo's directory exists");
        let main = WtData { path: dir.clone(), id: None, head: Head::Branch("main".to_owned()), locked: None, index: None, rebase: None };
        self.repos.push(RepoData {
            git_dir: git_dir_of(&dir),
            bare,
            wts: vec![main],
            refs: BTreeMap::new(),
            symrefs: BTreeMap::new(),
            reflogs: HashMap::new(),
            config: Vec::new(),
            have: HashSet::new(),
        });
        RepoId(self.repos.len() - 1)
    }
    /// The repo and worktree whose path is the nearest ancestor-or-self of `path`.
    fn locate(&self, path: &Path) -> Option<(usize, usize)> {
        let search = |path: &Path| {
            let mut best: Option<(usize, usize, usize)> = None;
            for (r, repo) in self.repos.iter().enumerate() {
                for (w, wt) in repo.wts.iter().enumerate() {
                    if path.starts_with(&wt.path) && best.is_none_or(|(_, _, len)| wt.path.as_os_str().len() > len) {
                        best = Some((r, w, wt.path.as_os_str().len()));
                    }
                }
            }
            best.map(|(r, w, _)| (r, w))
        };
        search(path).or_else(|| path.canonicalize().ok().and_then(|c| search(&c)))
    }
    fn place(&self, wt: &Path) -> (usize, usize) {
        self.locate(wt).unwrap_or_else(|| panic!("not a worktree path: {}", wt.display()))
    }
    /// The repo whose worktree (or bare git dir) is the nearest ancestor-or-self of `path`.
    pub fn repo_of(&self, path: &Path) -> Option<RepoId> {
        self.locate(path).map(|(r, _)| RepoId(r))
    }
    /// The worktree (main or linked) whose path is the nearest ancestor-or-self of `path`.
    pub fn worktree_of(&self, path: &Path) -> Option<PathBuf> {
        self.locate(path).map(|(r, w)| self.repos[r].wts[w].path.clone())
    }
    /// The repo a remote url names: its worktree, its git dir, or a bare repo's directory, exactly.
    pub fn repo_at(&self, url: &str) -> Option<RepoId> {
        let url = Path::new(url);
        let canonical = url.canonicalize().ok();
        self.repos.iter().position(|r| {
            let own = [r.git_dir.as_path(), r.wts[0].path.as_path()];
            own.iter().any(|p| *p == url || Some(*p) == canonical.as_deref())
        }).map(RepoId)
    }
    pub fn is_bare(&self, repo: RepoId) -> bool {
        self.repo_data(repo).bare
    }
    /// The common git dir: `<main worktree>/.git`, or the bare repo's directory.
    pub fn git_dir(&self, repo: RepoId) -> PathBuf {
        self.repo_data(repo).git_dir.clone()
    }
    /// The directory that holds the main `HEAD` of the repo: its main worktree, or a bare repo's own directory.
    pub(super) fn main_path(&self, repo: RepoId) -> PathBuf {
        self.repo_data(repo).wts[0].path.clone()
    }
    /// The main worktree first, then the linked ones in the order they were added. A bare repo has none.
    pub fn worktrees(&self, repo: RepoId) -> Vec<Worktree> {
        let data = self.repo_data(repo);
        if data.bare {
            return Vec::new();
        }
        data.wts.iter().map(|w| Worktree { path: w.path.clone(), id: w.id.clone() }).collect()
    }
    /// Makes `path` a real directory with a `.git` FILE `gitdir: <git dir>/worktrees/<id>` and makes that
    /// admin directory; `<id>` is the last component of `path`, with the first free number appended when
    /// taken (`wt`, `wt1`, ...). Its `HEAD` is `head`.
    pub fn add_worktree(&mut self, repo: RepoId, path: &Path, head: Head) -> Worktree {
        std::fs::create_dir_all(path).expect("the worktree's directory can be made");
        let path = path.canonicalize().expect("the worktree's directory exists");
        let base = path.file_name().expect("a worktree path has a name").to_string_lossy().into_owned();
        let data = self.repo_mut(repo);
        let taken = |id: &str| data.wts.iter().any(|w| w.id.as_deref() == Some(id));
        let id = if !taken(&base) {
            base
        } else {
            (1..).map(|n| format!("{base}{n}")).find(|id| !taken(id)).expect("a free number exists")
        };
        let admin = data.git_dir.join("worktrees").join(&id);
        std::fs::create_dir_all(&admin).expect("the admin directory can be made");
        std::fs::write(path.join(".git"), format!("gitdir: {}\n", admin.display())).expect("the .git file can be written");
        data.wts.push(WtData { path: path.clone(), id: Some(id.clone()), head, locked: None, index: None, rebase: None });
        Worktree { path, id: Some(id) }
    }
    /// The git dir of a worktree: the common one for the main worktree, `<git dir>/worktrees/<id>` for a
    /// linked one. Panics when `wt` is not a worktree path.
    pub fn admin_dir(&self, wt: &Path) -> PathBuf {
        let (r, w) = self.place(wt);
        let data = &self.repos[r];
        match &data.wts[w].id {
            None => data.git_dir.clone(),
            Some(id) => data.git_dir.join("worktrees").join(id),
        }
    }

    /// Locks the worktree at `wt` with `reason` (empty: no reason).
    pub fn lock_worktree(&mut self, wt: &Path, reason: &str) {
        let (r, w) = self.place(wt);
        self.repos[r].wts[w].locked = Some(reason.to_owned());
    }
    /// Unlocks the worktree at `wt` (`git worktree unlock`).
    pub fn unlock_worktree(&mut self, wt: &Path) {
        let (r, w) = self.place(wt);
        self.repos[r].wts[w].locked = None;
    }
    /// `Some(reason)` while the worktree at `wt` is locked (empty: no reason given).
    pub fn lock_reason(&self, wt: &Path) -> Option<String> {
        let (r, w) = self.place(wt);
        self.repos[r].wts[w].locked.clone()
    }
    /// Forgets the linked worktree at `wt` and deletes its admin directory; its directory and the branch
    /// stay. Panics when `wt` is not a linked worktree path.
    pub fn forget_worktree(&mut self, wt: &Path) {
        let (r, w) = self.place(wt);
        let data = &mut self.repos[r];
        let id = data.wts[w].id.clone().expect("only a linked worktree is forgotten");
        data.wts.remove(w);
        let _ = std::fs::remove_dir_all(data.git_dir.join("worktrees").join(id));
    }

    // ---- refs, HEAD, reflogs ----

    /// Points `name` (a full ref name) at `new`, and appends a reflog entry with `message`.
    pub fn set_ref(&mut self, repo: RepoId, name: &str, new: &Oid, message: &str) {
        let tree = self.commit_data(new).map_or_else(|| new.clone(), |c| c.tree);
        let data = self.repo_mut(repo);
        let old = data.refs.insert(name.to_owned(), new.clone());
        data.reflogs.entry(name.to_owned()).or_default().push(ReflogEntry { old, new: new.clone(), tree, message: message.to_owned() });
    }
    pub fn ref_oid(&self, repo: RepoId, name: &str) -> Option<Oid> {
        self.repo_data(repo).refs.get(name).cloned()
    }
    pub fn delete_ref(&mut self, repo: RepoId, name: &str) {
        let data = self.repo_mut(repo);
        data.refs.remove(name);
        data.reflogs.remove(name);
    }
    /// Renames the branch `old` to `new` (short names), with its reflog, and moves the `HEAD` of every
    /// worktree that has it checked out. Panics when `old` does not exist.
    pub fn rename_branch(&mut self, repo: RepoId, old: &str, new: &str) {
        let data = self.repo_mut(repo);
        let (from, to) = (format!("refs/heads/{old}"), format!("refs/heads/{new}"));
        let oid = data.refs.remove(&from).expect("the branch to rename exists");
        data.refs.insert(to.clone(), oid);
        if let Some(log) = data.reflogs.remove(&from) {
            data.reflogs.insert(to, log);
        }
        for wt in &mut data.wts {
            if wt.head == Head::Branch(old.to_owned()) {
                wt.head = Head::Branch(new.to_owned());
            }
        }
    }
    /// The direct refs whose full name starts with `prefix`, sorted by name.
    pub fn refs(&self, repo: RepoId, prefix: &str) -> Vec<(String, Oid)> {
        self.repo_data(repo).refs.range(prefix.to_owned()..).take_while(|(n, _)| n.starts_with(prefix)).map(|(n, o)| (n.clone(), o.clone())).collect()
    }
    /// Makes `name` (for example `refs/remotes/origin/HEAD`) a symbolic ref to the full ref `target`.
    pub fn set_symbolic_ref(&mut self, repo: RepoId, name: &str, target: &str) {
        self.repo_mut(repo).symrefs.insert(name.to_owned(), target.to_owned());
    }
    /// The symbolic refs whose full name starts with `prefix`, as `(name, target)`, sorted by name.
    pub(super) fn symbolic_refs(&self, repo: RepoId, prefix: &str) -> Vec<(String, String)> {
        self.repo_data(repo).symrefs.iter().filter(|(n, _)| n.starts_with(prefix)).map(|(n, t)| (n.clone(), t.clone())).collect()
    }
    pub(super) fn delete_symbolic_ref(&mut self, repo: RepoId, name: &str) {
        self.repo_mut(repo).symrefs.remove(name);
    }
    pub fn symbolic_ref(&self, repo: RepoId, name: &str) -> Option<String> {
        self.repo_data(repo).symrefs.get(name).cloned()
    }
    /// The commit a full ref name reaches, following symbolic refs.
    pub fn resolve_ref(&self, repo: RepoId, name: &str) -> Option<Oid> {
        let mut name = name.to_owned();
        for _ in 0..5 {
            if let Some(oid) = self.ref_oid(repo, &name) {
                return Some(oid);
            }
            name = self.symbolic_ref(repo, &name)?;
        }
        None
    }
    /// The `HEAD` of the worktree at `wt`; each worktree has its own, the refs are shared.
    pub fn head(&self, wt: &Path) -> Head {
        let (r, w) = self.place(wt);
        self.repos[r].wts[w].head.clone()
    }
    pub fn set_head(&mut self, wt: &Path, head: Head) {
        let (r, w) = self.place(wt);
        self.repos[r].wts[w].head = head;
    }
    /// The reflog of `name`, newest first; empty for a ref that never moved.
    pub fn reflog(&self, repo: RepoId, name: &str) -> Vec<ReflogEntry> {
        self.repo_data(repo).reflogs.get(name).map(|log| log.iter().rev().cloned().collect()).unwrap_or_default()
    }
    /// The tips of `refs/remotes/<remote>/*` (what `--remotes=<remote>` selects), sorted by ref name.
    pub fn remote_tips(&self, repo: RepoId, remote: &str) -> Vec<Oid> {
        self.refs(repo, &format!("refs/remotes/{remote}/")).into_iter().map(|(_, o)| o).collect()
    }

    // ---- config: multi-valued; the global one comes first ----

    /// Sets a global (`GIT_CONFIG_GLOBAL`) value, replacing the global values of the key.
    pub fn set_global(&mut self, key: &str, value: &str) {
        let key = norm_key(key);
        self.global.retain(|(k, _)| *k != key);
        self.global.push((key, value.to_owned()));
    }
    /// Appends a value of the repo's own config.
    pub fn config_add(&mut self, repo: RepoId, key: &str, value: &str) {
        self.repo_mut(repo).config.push((norm_key(key), value.to_owned()));
    }
    /// Replaces every value of the key in the repo's own config.
    pub fn config_set(&mut self, repo: RepoId, key: &str, value: &str) {
        let key = norm_key(key);
        let config = &mut self.repo_mut(repo).config;
        config.retain(|(k, _)| *k != key);
        config.push((key, value.to_owned()));
    }
    /// Drops the entries of the repo's own config for which `drop(key, value)` is true (keys normalized).
    pub(super) fn config_drop(&mut self, repo: RepoId, drop: impl Fn(&str, &str) -> bool) {
        self.repo_mut(repo).config.retain(|(k, v)| !drop(k, v));
    }
    /// The global entries, keys normalized.
    pub(super) fn global_entries(&self) -> Vec<(String, String)> {
        self.global.clone()
    }
    /// Every entry: the global ones, then the repo's, then `extra` (a call's `-c` and `GIT_CONFIG_*`), keys normalized.
    pub fn config_entries(&self, repo: RepoId, extra: &[(String, String)]) -> Vec<(String, String)> {
        let extra = extra.iter().map(|(k, v)| (norm_key(k), v.clone()));
        self.global.iter().chain(&self.repo_data(repo).config).cloned().chain(extra).collect()
    }
    /// Every value of the key: the global ones, then the repo's, each in the order added. Section and
    /// variable names are case-insensitive, the subsection (`url.<x>.`) is not.
    pub fn config_values(&self, repo: RepoId, key: &str) -> Vec<String> {
        let key = norm_key(key);
        self.config_entries(repo, &[]).into_iter().filter(|(k, _)| *k == key).map(|(_, v)| v).collect()
    }
    /// `url` rewritten by the longest matching `url.<base>.insteadOf` (git's `remote get-url`).
    pub fn resolve_url(&self, repo: RepoId, url: &str) -> String {
        self.resolve_url_with(repo, url, &[])
    }
    /// [`Universe::resolve_url`] with the config of one call added.
    pub fn resolve_url_with(&self, repo: RepoId, url: &str, extra: &[(String, String)]) -> String {
        let mut best: Option<(usize, String)> = None;
        for (key, prefix) in self.config_entries(repo, extra) {
            let Some(base) = key.strip_prefix("url.").and_then(|k| k.strip_suffix(".insteadof")) else { continue };
            if url.starts_with(&prefix) && best.as_ref().is_none_or(|(len, _)| prefix.len() > *len) {
                best = Some((prefix.len(), format!("{base}{}", &url[prefix.len()..])));
            }
        }
        best.map_or_else(|| url.to_owned(), |(_, rewritten)| rewritten)
    }

    // ---- the index of a worktree: a real file, rewritten by rename ----

    fn write_index(&self, wt: &Path) {
        let (r, w) = self.place(wt);
        let admin = self.admin_dir(wt);
        let text: String = self.repos[r].wts[w].index.iter().flatten().map(|(p, o, s)| format!("{s} {o}\t{p}\n")).collect();
        let tmp = admin.join("index.new");
        std::fs::write(&tmp, text).expect("the new index can be written");
        std::fs::rename(&tmp, admin.join("index")).expect("the index can be renamed into place");
    }
    /// Records `path` at `stage` (0 merged, 1 base, 2 ours, 3 theirs) with `blob`, replacing that stage of the
    /// path, and rewrites the worktree's `index` file by rename: a new inode at every call.
    pub fn stage(&mut self, wt: &Path, path: &str, blob: &Oid, stage: u8) {
        let (r, w) = self.place(wt);
        let entries = self.repos[r].wts[w].index.get_or_insert_with(Vec::new);
        entries.retain(|(p, _, s)| !(p == path && *s == stage));
        entries.push((path.to_owned(), blob.clone(), stage));
        entries.sort_by(|a, b| (&a.0, a.2).cmp(&(&b.0, b.2)));
        self.write_index(wt);
    }
    /// Replaces the whole index (entries in any order).
    pub(super) fn set_index(&mut self, wt: &Path, mut entries: Vec<(String, Oid, u8)>) {
        let (r, w) = self.place(wt);
        entries.sort_by(|a, b| (&a.0, a.2).cmp(&(&b.0, b.2)));
        self.repos[r].wts[w].index = Some(entries);
        self.write_index(wt);
    }
    /// The entries sorted by path, then stage.
    pub fn index(&self, wt: &Path) -> Vec<(String, Oid, u8)> {
        let (r, w) = self.place(wt);
        self.repos[r].wts[w].index.clone().unwrap_or_default()
    }
    /// What git sees as the index: the stored one, or while nothing was staged the tree of `HEAD`.
    pub(super) fn effective_index(&self, wt: &Path) -> Vec<(String, Oid, u8)> {
        let (r, w) = self.place(wt);
        if let Some(entries) = &self.repos[r].wts[w].index {
            return entries.clone();
        }
        self.head_tree(wt).into_iter().map(|(p, o)| (p, o, 0)).collect()
    }
    /// The entries of the tree of the commit `HEAD` names; empty when it is unborn.
    pub(super) fn head_tree(&self, wt: &Path) -> Vec<(String, Oid)> {
        let (r, _) = self.place(wt);
        let commit = match self.head(wt) {
            Head::Branch(b) => self.ref_oid(RepoId(r), &format!("refs/heads/{b}")),
            Head::Detached(oid) => Some(oid),
        };
        commit.and_then(|c| self.commit_data(&c)).and_then(|c| self.tree_entries(&c.tree)).unwrap_or_default()
    }
    /// The rebase stopped in the worktree at `wt`.
    pub(super) fn rebase_state(&self, wt: &Path) -> Option<RebaseState> {
        let (r, w) = self.place(wt);
        self.repos[r].wts[w].rebase.clone()
    }
    pub(super) fn set_rebase_state(&mut self, wt: &Path, state: Option<RebaseState>) {
        let (r, w) = self.place(wt);
        self.repos[r].wts[w].rebase = state;
    }
    /// `<admin dir of wt>/index`.
    pub fn index_path(&self, wt: &Path) -> PathBuf {
        self.admin_dir(wt).join("index")
    }

    // ---- ancestry ----

    /// `start` and every commit they reach through parents.
    fn ancestors(&self, start: &[Oid]) -> HashSet<Oid> {
        let mut seen = HashSet::new();
        let mut stack: Vec<Oid> = start.to_vec();
        while let Some(next) = stack.pop() {
            if let Some(commit) = self.commit_data(&next)
                && seen.insert(next) {
                    stack.extend(commit.parents);
                }
        }
        seen
    }
    fn order_key(&self, oid: &Oid) -> (u64, usize) {
        let time = self.commit_data(oid).map_or(0, |c| c.time);
        (time, self.objects.get(oid).map_or(0, |s| s.seq))
    }
    /// The best common ancestor of two commits (on a criss-cross, the first); `None` for unrelated histories.
    pub fn merge_base(&self, _repo: RepoId, a: &Oid, b: &Oid) -> Option<Oid> {
        let from_b = self.ancestors(std::slice::from_ref(b));
        let common: Vec<Oid> = self.ancestors(std::slice::from_ref(a)).into_iter().filter(|c| from_b.contains(c)).collect();
        let parents: Vec<Oid> = common.iter().filter_map(|c| self.commit_data(c)).flat_map(|c| c.parents).collect();
        let dominated = self.ancestors(&parents);
        common.into_iter().filter(|c| !dominated.contains(c)).max_by_key(|c| self.order_key(c))
    }
    /// Whether `a` is an ancestor of `b` (a commit is its own ancestor); the first object `repo` does not
    /// hold is the error.
    pub fn is_ancestor(&self, repo: RepoId, a: &Oid, b: &Oid) -> Result<bool, UnknownObject> {
        for oid in [a, b] {
            if !self.has(repo, oid) {
                return Err(UnknownObject(oid.clone()));
            }
        }
        Ok(self.ancestors(std::slice::from_ref(b)).contains(a))
    }
    /// The commits reachable from `include` and not from `exclude`, newest first (by commit time; a child
    /// always before its parents); `no_merges` drops the commits with two parents or more.
    pub fn rev_list(&self, _repo: RepoId, sel: &Selection, no_merges: bool) -> Vec<Oid> {
        let excluded = self.ancestors(&sel.exclude);
        let selected: HashSet<Oid> = self.ancestors(&sel.include).into_iter().filter(|c| !excluded.contains(c)).collect();
        let mut pending: HashMap<&Oid, usize> = selected.iter().map(|c| (c, 0)).collect();
        for commit in &selected {
            for parent in self.commit_data(commit).into_iter().flat_map(|c| c.parents).filter_map(|p| selected.get(&p)) {
                *pending.get_mut(parent).expect("a selected parent is pending") += 1;
            }
        }
        let mut ready: BinaryHeap<((u64, usize), Reverse<Oid>)> =
            pending.iter().filter(|(_, n)| **n == 0).map(|(c, _)| (self.order_key(c), Reverse((*c).clone()))).collect();
        let mut out = Vec::new();
        while let Some((_, Reverse(next))) = ready.pop() {
            let data = self.commit_data(&next).expect("a selected oid is a commit");
            for parent in data.parents.iter().filter(|p| selected.contains(*p)) {
                let n = pending.get_mut(parent).expect("a selected parent is pending");
                *n -= 1;
                if *n == 0 {
                    ready.push((self.order_key(parent), Reverse(parent.clone())));
                }
            }
            if !(no_merges && data.parents.len() > 1) {
                out.push(next);
            }
        }
        out
    }
}
