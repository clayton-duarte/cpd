//! Revisions: `HEAD`, a full sha, refs in git's DWIM order, `^{commit}`, `^{tree}`, `<rev>:<path>`,
//! `a..b` and `a...b`.

use std::path::Path;

use super::universe::{Head, Kind, Oid, RepoId, Universe, is_oid};

/// Why a revision did not resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevError {
    /// Names nothing: no such ref, an unborn `HEAD`, an object the repo does not hold, a path not in the tree.
    Unknown(String),
    /// Names an object of the wrong type for `^{commit}` / `^{tree}`.
    WrongType(String),
}

/// What `rev-list` selects: reachable from `include`, not from `exclude`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub include: Vec<Oid>,
    pub exclude: Vec<Oid>,
}

fn repo_of(u: &Universe, wt: &Path, spec: &str) -> Result<RepoId, RevError> {
    u.repo_of(wt)
        .ok_or_else(|| RevError::Unknown(spec.to_owned()))
}

/// The full ref name a name reaches in git's DWIM order: `<x>`, `refs/<x>`, `refs/tags/<x>`,
/// `refs/heads/<x>`, `refs/remotes/<x>`, `refs/remotes/<x>/HEAD`; the first that exists wins.
pub fn dwim_ref(u: &Universe, repo: RepoId, name: &str) -> Option<String> {
    [
        name.to_owned(),
        format!("refs/{name}"),
        format!("refs/tags/{name}"),
        format!("refs/heads/{name}"),
        format!("refs/remotes/{name}"),
        format!("refs/remotes/{name}/HEAD"),
    ]
    .into_iter()
    .find(|candidate| u.resolve_ref(repo, candidate).is_some())
}

/// The commit or tree an object-ish name stands for, before any `^{}` or `:path`.
fn atom(u: &Universe, wt: &Path, spec: &str) -> Result<Oid, RevError> {
    let unknown = || RevError::Unknown(spec.to_owned());
    let repo = repo_of(u, wt, spec)?;
    if is_oid(spec) {
        let oid = Oid::new(spec);
        return if u.has(repo, &oid) {
            Ok(oid)
        } else {
            Err(unknown())
        };
    }
    if spec == "HEAD" {
        return match u.head(wt) {
            Head::Branch(b) => u
                .resolve_ref(repo, &format!("refs/heads/{b}"))
                .ok_or_else(unknown),
            Head::Detached(oid) => Ok(oid),
        };
    }
    dwim_ref(u, repo, spec)
        .and_then(|name| u.resolve_ref(repo, &name))
        .ok_or_else(unknown)
}

/// Resolves one revision in the repo of the worktree `wt` (the `HEAD` it reads is that worktree's).
/// DWIM order for a name `<x>`: `<x>`, `refs/<x>`, `refs/tags/<x>`, `refs/heads/<x>`, `refs/remotes/<x>`,
/// `refs/remotes/<x>/HEAD`; the first that exists wins.
pub fn parse(u: &Universe, wt: &Path, spec: &str) -> Result<Oid, RevError> {
    for (suffix, want) in [("^{commit}", Kind::Commit), ("^{tree}", Kind::Tree)] {
        if let Some(inner) = spec.strip_suffix(suffix) {
            let mut oid = parse(u, wt, inner)?;
            while let Some(target) = u.tag_target(&oid) {
                oid = target;
            }
            let wrong = || RevError::WrongType(spec.to_owned());
            return match (u.kind(&oid), want) {
                (Some(Kind::Commit), Kind::Commit) => Ok(oid),
                (Some(Kind::Commit), Kind::Tree) => {
                    Ok(u.commit_data(&oid).expect("a commit has data").tree)
                }
                (Some(Kind::Tree), Kind::Tree) => Ok(oid),
                _ => Err(wrong()),
            };
        }
    }
    if let Some(path) = spec.strip_prefix(':').filter(|p| !p.starts_with('/')) {
        // `:<path>`: the blob the index holds at stage 0.
        let wt_root = u
            .worktree_of(wt)
            .ok_or_else(|| RevError::Unknown(spec.to_owned()))?;
        return u
            .effective_index(&wt_root)
            .into_iter()
            .find(|(p, _, stage)| p == path && *stage == 0)
            .map(|(_, blob, _)| blob)
            .ok_or_else(|| RevError::Unknown(spec.to_owned()));
    }
    if let Some((rev, path)) = spec.split_once(':') {
        let tree = parse(u, wt, &format!("{rev}^{{tree}}"))?;
        return u
            .tree_entries(&tree)
            .and_then(|entries| entries.into_iter().find(|(p, _)| p == path))
            .map(|(_, blob)| blob)
            .ok_or_else(|| RevError::Unknown(spec.to_owned()));
    }
    if let Some((inner, steps)) = spec.rsplit_once('~') {
        // `<rev>~<n>`: the n-th first parent (`~` alone is `~1`).
        if let Ok(n) = if steps.is_empty() {
            Ok(1)
        } else {
            steps.parse::<usize>()
        } {
            let mut oid = parse(u, wt, &format!("{inner}^{{commit}}"))?;
            for _ in 0..n {
                let parent = u.commit_data(&oid).and_then(|c| c.parents.first().cloned());
                oid = parent.ok_or_else(|| RevError::Unknown(spec.to_owned()))?;
            }
            return Ok(oid);
        }
    }
    atom(u, wt, spec)
}

/// `parse`, which must name a commit.
fn commit(u: &Universe, wt: &Path, spec: &str) -> Result<Oid, RevError> {
    let oid = atom(u, wt, spec)?;
    match u.kind(&oid) {
        Some(Kind::Commit) => Ok(oid),
        _ => Err(RevError::WrongType(spec.to_owned())),
    }
}

/// Resolves `a..b` (include `b`, exclude `a`) or `a...b` (include both, exclude their merge base); a side
/// left empty is `HEAD`.
pub fn range(u: &Universe, wt: &Path, spec: &str) -> Result<Selection, RevError> {
    let (a, b, symmetric) = match spec.split_once("...") {
        Some((a, b)) => (a, b, true),
        None => match spec.split_once("..") {
            Some((a, b)) => (a, b, false),
            None => return Err(RevError::Unknown(spec.to_owned())),
        },
    };
    let side = |s: &str| commit(u, wt, if s.is_empty() { "HEAD" } else { s });
    let (a, b) = (side(a)?, side(b)?);
    if symmetric {
        let repo = repo_of(u, wt, spec)?;
        let exclude = u.merge_base(repo, &a, &b).into_iter().collect();
        Ok(Selection {
            include: vec![a, b],
            exclude,
        })
    } else {
        Ok(Selection {
            include: vec![b],
            exclude: vec![a],
        })
    }
}
