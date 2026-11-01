//! The fixture handle of a test: a repo of the Universe and its working tree on disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use super::universe::{Head, Oid, RepoId, Universe, blob_id};

/// A repo's handle; a clone shares the Universe.
#[derive(Clone)]
pub struct Repo {
    universe: Arc<Mutex<Universe>>,
    id: RepoId,
    dir: PathBuf,
}

/// Every file under `root` as `relative path -> absolute path`, skipping every entry named `.git`.
pub(super) fn work_files(root: &Path) -> BTreeMap<String, PathBuf> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            if entry.file_name() == ".git" {
                continue;
            }
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                walk(root, &path, out);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.insert(rel.to_string_lossy().into_owned(), path);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

impl Repo {
    fn u(&self) -> MutexGuard<'_, Universe> {
        self.universe.lock().expect("the Universe's lock is never held across a panic")
    }

    /// A non-bare repo at `dir` on the unborn branch `main`.
    pub fn init(universe: &Arc<Mutex<Universe>>, dir: &Path) -> Repo {
        let (id, dir) = {
            let mut u = universe.lock().expect("the Universe's lock is never held across a panic");
            let id = u.init(dir, false);
            let main = u.worktrees(id).remove(0).path;
            (id, main)
        };
        Repo { universe: universe.clone(), id, dir }
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }
    pub fn id(&self) -> RepoId {
        self.id
    }
    /// Writes a real file of the working tree (parents made).
    pub fn write(&self, path: &str, text: &str) {
        let file = self.dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a file has a parent")).expect("the parents can be made");
        std::fs::write(file, text).expect("the file can be written");
    }
    /// Stages the file as it is on disk (`git add <path>`).
    pub fn add(&self, path: &str) {
        let bytes = std::fs::read(self.dir.join(path)).expect("the file to add exists");
        let mut u = self.u();
        let blob = u.blob(self.id, &bytes);
        let mut index = u.effective_index(&self.dir);
        index.retain(|(p, _, _)| p != path);
        index.push((path.to_owned(), blob, 0));
        u.set_index(&self.dir, index);
    }
    /// Stages every file of the working tree, commits it on the branch `HEAD` names (or on `HEAD` when
    /// detached) with the reflog message `commit: <message>`, and returns the commit.
    pub fn commit(&self, message: &str) -> Oid {
        let mut u = self.u();
        let mut index = Vec::new();
        for (path, file) in work_files(&self.dir) {
            let bytes = std::fs::read(file).expect("a file of the working tree can be read");
            index.push((path, u.blob(self.id, &bytes), 0));
        }
        let entries: Vec<(&str, &Oid)> = index.iter().map(|(p, o, _)| (p.as_str(), o)).collect();
        let tree = u.tree(self.id, &entries);
        let head = u.head(&self.dir);
        let parent = match &head {
            Head::Branch(b) => u.ref_oid(self.id, &format!("refs/heads/{b}")),
            Head::Detached(oid) => Some(oid.clone()),
        };
        let commit = u.commit(self.id, &tree, &parent.into_iter().collect::<Vec<_>>(), message);
        match head {
            Head::Branch(b) => u.set_ref(self.id, &format!("refs/heads/{b}"), &commit, &format!("commit: {message}")),
            Head::Detached(_) => u.set_head(&self.dir, Head::Detached(commit.clone())),
        }
        u.set_index(&self.dir, index);
        commit
    }
    /// `refs/heads/<name>` at `HEAD`.
    pub fn branch(&self, name: &str) {
        let mut u = self.u();
        let at = match u.head(&self.dir) {
            Head::Branch(b) => u.ref_oid(self.id, &format!("refs/heads/{b}")),
            Head::Detached(oid) => Some(oid),
        }
        .expect("a branch needs a commit at HEAD");
        u.set_ref(self.id, &format!("refs/heads/{name}"), &at, "branch: Created from HEAD");
    }
    /// Appends a value to the repo's own config.
    pub fn config_add(&self, key: &str, value: &str) {
        self.u().config_add(self.id, key, value);
    }
    /// Makes `dir` a bare repo of the Universe and the remote `origin` of this one (its url is `dir`).
    pub fn bare_origin(&self, dir: &Path) -> Repo {
        let mut u = self.u();
        let id = u.init(dir, true);
        let dir = u.git_dir(id);
        u.config_add(self.id, "remote.origin.url", &dir.to_string_lossy());
        Repo { universe: self.universe.clone(), id, dir }
    }
    /// The remote `origin` is `https://github.com/acme/<name>.git`, which resolves to no repo, and
    /// `refs/remotes/origin/<branch>` is the current commit of the branch `HEAD` names.
    pub fn keyed(&self, name: &str) {
        let mut u = self.u();
        u.config_add(self.id, "remote.origin.url", &format!("https://github.com/acme/{name}.git"));
        if let Head::Branch(b) = u.head(&self.dir)
            && let Some(at) = u.ref_oid(self.id, &format!("refs/heads/{b}")) {
                u.set_ref(self.id, &format!("refs/remotes/origin/{b}"), &at, "fetch");
            }
    }
    /// Pushes the branch `HEAD` names to `origin` (its url resolved through `insteadOf` to a repo of the
    /// Universe): the objects, `refs/heads/<b>` there and `refs/remotes/origin/<b>` here.
    pub fn push(&self) {
        let mut u = self.u();
        let Head::Branch(b) = u.head(&self.dir) else { panic!("push needs a branch at HEAD") };
        let commit = u.ref_oid(self.id, &format!("refs/heads/{b}")).expect("the branch to push has a commit");
        let url = u.config_values(self.id, "remote.origin.url").pop().expect("push needs a remote origin");
        let url = u.resolve_url(self.id, &url);
        let there = u.repo_at(&url).unwrap_or_else(|| panic!("the url {url} is no repo of the Universe"));
        u.copy(self.id, there, &commit);
        u.set_ref(there, &format!("refs/heads/{b}"), &commit, "push");
        u.set_ref(self.id, &format!("refs/remotes/origin/{b}"), &commit, "update by push");
    }
}

/// The blob id of a file on disk, `None` when it is not a readable file.
pub(super) fn file_blob(path: &Path) -> Option<Oid> {
    std::fs::read(path).ok().map(|bytes| blob_id(&bytes))
}
