//! `merge-tree --write-tree --name-only` and `rebase` (with `--continue` and `--abort`): the per-path three-way
//! merge they share, over the text merge of `merge3`. Not modelled: rename detection, a criss-cross merge
//! (the first best base is taken), patch-id dedup, dropping a replayed commit that became empty, and
//! `--rebase-merges`.

use super::super::merge3::merge3;
use super::super::universe::RebaseState;
use super::*;

/// A path the merge could not settle: the blobs of the base and of each side (`None`: absent there).
pub(super) struct Conflict {
    pub(super) path: String,
    pub(super) base: Option<Oid>,
    pub(super) ours: Option<Oid>,
    pub(super) theirs: Option<Oid>,
}

pub(super) struct Merged {
    /// Every path of the result; a conflicted one holds its markers, or the version left in the tree.
    pub(super) entries: BTreeMap<String, Oid>,
    /// Sorted by path.
    pub(super) conflicts: Vec<Conflict>,
    /// What git prints about the paths it merged or could not, in path order.
    pub(super) messages: Vec<String>,
}

pub(super) fn tree_map(u: &Universe, tree: &Oid) -> BTreeMap<String, Oid> {
    u.tree_entries(tree)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// The paths of the tree of the commit `commit`.
pub(super) fn commit_tree(u: &Universe, commit: &Oid) -> BTreeMap<String, Oid> {
    tree_map(u, &u.commit_data(commit).expect("a commit has data").tree)
}

/// The three-way merge of two flat trees over a base: git's trivial cases per path, the text merge for the
/// rest. `label_ours` and `label_theirs` name the sides in markers and messages.
pub(super) fn merge_trees(
    u: &mut Universe,
    repo: RepoId,
    base: &BTreeMap<String, Oid>,
    ours: &BTreeMap<String, Oid>,
    theirs: &BTreeMap<String, Oid>,
    (label_ours, label_theirs): (&str, &str),
    favor_ours: bool,
) -> Merged {
    let paths: BTreeSet<&String> = base
        .keys()
        .chain(ours.keys())
        .chain(theirs.keys())
        .collect();
    let mut merged = Merged {
        entries: BTreeMap::new(),
        conflicts: Vec::new(),
        messages: Vec::new(),
    };
    for path in paths {
        let (b, o, t) = (base.get(path), ours.get(path), theirs.get(path));
        let settled = if o == t || b == t {
            Some(o)
        } else if b == o {
            Some(t)
        } else {
            None
        };
        if let Some(blob) = settled {
            if let Some(blob) = blob {
                merged.entries.insert(path.clone(), blob.clone());
            }
            continue;
        }
        let conflict = || Conflict {
            path: path.clone(),
            base: b.cloned(),
            ours: o.cloned(),
            theirs: t.cloned(),
        };
        match (o, t) {
            (Some(ours_blob), Some(theirs_blob)) => {
                let text =
                    |blob: Option<&Oid>| blob.and_then(|b| u.blob_bytes(b)).unwrap_or_default();
                let (base_text, ours_text, theirs_text) =
                    (text(b), text(Some(ours_blob)), text(Some(theirs_blob)));
                merged.messages.push(format!("Auto-merging {path}"));
                let (bytes, conflicted) = merge3(
                    &base_text,
                    &ours_text,
                    &theirs_text,
                    label_ours,
                    label_theirs,
                    favor_ours,
                );
                merged.entries.insert(path.clone(), u.blob(repo, &bytes));
                if conflicted {
                    let kind = if b.is_none() { "add/add" } else { "content" };
                    merged
                        .messages
                        .push(format!("CONFLICT ({kind}): Merge conflict in {path}"));
                    merged.conflicts.push(conflict());
                }
            }
            (None, Some(blob)) => {
                merged.messages.push(format!(
                    "CONFLICT (modify/delete): {path} deleted in {label_ours} and modified in {label_theirs}.  Version {label_theirs} of {path} left in tree."
                ));
                merged.entries.insert(path.clone(), blob.clone());
                merged.conflicts.push(conflict());
            }
            (Some(blob), None) => {
                merged.messages.push(format!(
                    "CONFLICT (modify/delete): {path} deleted in {label_theirs} and modified in {label_ours}.  Version {label_ours} of {path} left in tree."
                ));
                merged.entries.insert(path.clone(), blob.clone());
                merged.conflicts.push(conflict());
            }
            (None, None) => unreachable!("both sides deleted the path, so they agree"),
        }
    }
    merged
}

// ---- merge-tree ------------------------------------------------------------------------------------

/// `merge-tree --write-tree --name-only <a> <b>`: the tree id, and on a conflict the paths, a blank line and
/// the messages (exit 1).
pub(super) fn merge_tree(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("merge-tree", args, &["--write-tree", "--name-only"], &[])?;
    let flags = flags(&toks);
    if !(flags.contains(&"--write-tree") && flags.contains(&"--name-only")) {
        return Err("merge-tree (without --write-tree and --name-only)".to_owned());
    }
    let [a, b] = positionals(&toks)[..] else {
        return Err("merge-tree (arguments)".to_owned());
    };
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let mut sides = Vec::new();
    for spec in [a, b] {
        match c.parse(&format!("{spec}^{{commit}}")) {
            Ok(oid) => sides.push(oid),
            Err(_) => {
                return Ok(Out::fail(
                    1,
                    format!("merge-tree: {spec} - not something we can merge\n"),
                ));
            }
        }
    }
    let Some(base) = c.u.merge_base(repo, &sides[0], &sides[1]) else {
        return Ok(Out::fail(
            128,
            "fatal: refusing to merge unrelated histories\n",
        ));
    };
    let trees = [&base, &sides[0], &sides[1]].map(|commit| commit_tree(c.u, commit));
    let merged = merge_trees(c.u, repo, &trees[0], &trees[1], &trees[2], (a, b), false);
    let tree = tree_of(c.u, repo, &merged.entries);
    if merged.conflicts.is_empty() {
        return Ok(Out::ok(format!("{tree}\n")));
    }
    let names: String = merged
        .conflicts
        .iter()
        .map(|k| format!("{}\n", k.path))
        .collect();
    let messages: String = merged.messages.iter().map(|m| format!("{m}\n")).collect();
    Ok(Out {
        code: 1,
        stdout: format!("{tree}\n{names}\n{messages}"),
        stderr: String::new(),
    })
}

pub(super) fn tree_of(u: &mut Universe, repo: RepoId, entries: &BTreeMap<String, Oid>) -> Oid {
    let refs: Vec<(&str, &Oid)> = entries.iter().map(|(p, o)| (p.as_str(), o)).collect();
    u.tree(repo, &refs)
}

// ---- rebase ----------------------------------------------------------------------------------------

pub(super) fn sha7(oid: &Oid) -> &str {
    &oid.as_str()[..7]
}

/// The commit `HEAD` of the worktree names.
pub(super) fn head_commit(u: &Universe, repo: RepoId, wt: &Path) -> Option<Oid> {
    match u.head(wt) {
        Head::Branch(b) => u.ref_oid(repo, &format!("refs/heads/{b}")),
        Head::Detached(oid) => Some(oid),
    }
}

/// Makes the working tree and the index of `wt` what `files` and `index` say: every file of `files` written,
/// every file the old index or `HEAD` tracked and `files` lacks deleted; untracked files stay.
pub(super) fn checkout(
    c: &mut Ctx,
    wt: &Path,
    files: &BTreeMap<String, Oid>,
    index: Vec<(String, Oid, u8)>,
) {
    let mut tracked: BTreeSet<String> =
        c.u.effective_index(wt)
            .into_iter()
            .map(|(p, _, _)| p)
            .collect();
    tracked.extend(c.u.head_tree(wt).into_iter().map(|(p, _)| p));
    for path in tracked.iter().filter(|p| !files.contains_key(*p)) {
        remove_file(wt, path);
    }
    for (path, blob) in files {
        write_blob(c.u, wt, path, blob);
    }
    c.u.set_index(wt, index);
}

fn rebase_dir(u: &Universe, wt: &Path) -> PathBuf {
    u.admin_dir(wt).join("rebase-merge")
}

/// Ends the rebase of `wt`: the state and `rebase-merge/` are gone.
fn clear(c: &mut Ctx, wt: &Path) {
    c.u.set_rebase_state(wt, None);
    let _ = std::fs::remove_dir_all(rebase_dir(c.u, wt));
}

pub(super) fn rebase(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("rebase", args, &["--continue", "--abort"], &[])?;
    let (flags, pos) = (flags(&toks), positionals(&toks));
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    let repo = c.repo().expect("a worktree has a repo");
    match (flags.as_slice(), pos.as_slice()) {
        ([], [onto]) => start(c, repo, &wt, onto),
        (["--continue"], []) => resume(c, repo, &wt),
        (["--abort"], []) => abort(c, &wt),
        _ => Err("rebase (arguments)".to_owned()),
    }
}

/// `rebase <onto>`: up to date, a fast-forward, or a replay of the non-merge commits of `onto..HEAD`.
fn start(c: &mut Ctx, repo: RepoId, wt: &Path, onto: &str) -> Run {
    let dir = rebase_dir(c.u, wt);
    if c.u.rebase_state(wt).is_some() || dir.exists() {
        return Ok(Out::fail(
            128,
            format!(
                "fatal: It seems that there is already a rebase-merge directory, and\nI wonder if you are in the middle of another rebase.  If that is the\ncase, please try\n\tgit rebase (--continue | --abort | --skip)\nIf that is not the case, please\n\trm -fr \"{}\"\nand run me again.  I am stopping in case you still have something\nvaluable there.\n",
                dir.display()
            ),
        ));
    }
    let Ok(onto_oid) = c.parse(&format!("{onto}^{{commit}}")) else {
        return Ok(Out::fail(
            128,
            format!("fatal: invalid upstream '{onto}'\n"),
        ));
    };
    let Some(head) = head_commit(c.u, repo, wt) else {
        return Err("rebase (an unborn HEAD)".to_owned());
    };
    let files = work_files(wt);
    let unstaged = c.u.effective_index(wt).iter().any(|(path, blob, stage)| {
        *stage == 0 && files.get(path).and_then(|f| file_blob(f)).as_ref() != Some(blob)
    });
    if unstaged {
        return Ok(Out::fail(
            1,
            "error: cannot rebase: You have unstaged changes.\nerror: Please commit or stash them.\n",
        ));
    }
    let branch = match c.u.head(wt) {
        Head::Branch(b) => Some(b),
        Head::Detached(_) => None,
    };
    let contains =
        |u: &Universe, outer: &Oid, inner: &Oid| u.is_ancestor(repo, inner, outer).unwrap_or(false);
    if contains(c.u, &head, &onto_oid) {
        return Ok(Out::ok(match &branch {
            Some(b) => format!("Current branch {b} is up to date.\n"),
            None => "HEAD is up to date.\n".to_owned(),
        }));
    }
    let mut state = RebaseState {
        orig_head: head.clone(),
        branch,
        onto: onto_oid.clone(),
        stopped: head.clone(),
        todo: Vec::new(),
    };
    if contains(c.u, &onto_oid, &head) {
        return Ok(finish(c, repo, wt, &state, &onto_oid));
    }
    let selection = Selection {
        include: vec![head],
        exclude: vec![onto_oid.clone()],
    };
    state.todo = c.u.rev_list(repo, &selection, true);
    state.todo.reverse();
    Ok(replay(c, repo, wt, state, onto_oid))
}

/// Replays what is left to do of `state` over `current`: the commits one by one, to the end or to the first
/// conflict.
fn replay(c: &mut Ctx, repo: RepoId, wt: &Path, mut state: RebaseState, mut current: Oid) -> Out {
    while !state.todo.is_empty() {
        let commit = state.todo.remove(0);
        let data = c.u.commit_data(&commit).expect("a commit to replay");
        let base = data
            .parents
            .first()
            .map(|p| commit_tree(c.u, p))
            .unwrap_or_default();
        let ours = commit_tree(c.u, &current);
        let theirs = tree_map(c.u, &data.tree);
        let subject = data.message.lines().next().unwrap_or("");
        let label = format!("{} ({subject})", sha7(&commit));
        let merged = merge_trees(c.u, repo, &base, &ours, &theirs, ("HEAD", &label), false);
        if merged.conflicts.is_empty() {
            let tree = tree_of(c.u, repo, &merged.entries);
            current = c.u.commit_as(
                repo,
                &data.author,
                &tree,
                std::slice::from_ref(&current),
                &data.message,
            );
            continue;
        }
        let mut index: Vec<(String, Oid, u8)> = merged
            .entries
            .iter()
            .filter(|(p, _)| !merged.conflicts.iter().any(|k| &k.path == *p))
            .map(|(p, o)| (p.clone(), o.clone(), 0))
            .collect();
        for k in &merged.conflicts {
            let stages = [(1, &k.base), (2, &k.ours), (3, &k.theirs)];
            index.extend(
                stages
                    .into_iter()
                    .filter_map(|(stage, blob)| Some((k.path.clone(), blob.clone()?, stage))),
            );
        }
        checkout(c, wt, &merged.entries, index);
        c.u.set_head(wt, Head::Detached(current));
        let dir = rebase_dir(c.u, wt);
        std::fs::create_dir_all(&dir).expect("the rebase directory can be made");
        std::fs::write(dir.join("orig-head"), format!("{}\n", state.orig_head))
            .expect("orig-head can be written");
        state.stopped = commit.clone();
        c.u.set_rebase_state(wt, Some(state));
        return Out {
            code: 1,
            stdout: merged.messages.iter().map(|m| format!("{m}\n")).collect(),
            stderr: format!(
                "error: could not apply {sha}... {subject}\nhint: Resolve all conflicts manually, mark them as resolved with\nhint: \"git add/rm <conflicted_files>\", then run \"git rebase --continue\".\nhint: To abort and get back to the state before \"git rebase\", run \"git rebase --abort\".\nCould not apply {sha}... {subject}\n",
                sha = sha7(&commit)
            ),
        };
    }
    finish(c, repo, wt, &state, &current)
}

/// Ends a rebase at `tip`: the working tree and the index are its tree, the branch (or the detached `HEAD`)
/// is on it.
fn finish(c: &mut Ctx, repo: RepoId, wt: &Path, state: &RebaseState, tip: &Oid) -> Out {
    let files = commit_tree(c.u, tip);
    let index = files
        .iter()
        .map(|(p, o)| (p.clone(), o.clone(), 0))
        .collect();
    checkout(c, wt, &files, index);
    let what = match &state.branch {
        Some(b) => {
            let name = format!("refs/heads/{b}");
            c.u.set_ref(
                repo,
                &name,
                tip,
                &format!("rebase (finish): {name} onto {}", state.onto),
            );
            c.u.set_head(wt, Head::Branch(b.clone()));
            name
        }
        None => {
            c.u.set_head(wt, Head::Detached(tip.clone()));
            "detached HEAD".to_owned()
        }
    };
    clear(c, wt);
    Out {
        code: 0,
        stdout: String::new(),
        stderr: format!("Successfully rebased and updated {what}.\n"),
    }
}

/// `rebase --continue`: commits the index as the stopped commit (its author and message), then goes on.
fn resume(c: &mut Ctx, repo: RepoId, wt: &Path) -> Run {
    let Some(state) = c.u.rebase_state(wt) else {
        return Ok(Out::fail(128, "fatal: no rebase in progress\n"));
    };
    let index = c.u.effective_index(wt);
    let unmerged: BTreeSet<&str> = index
        .iter()
        .filter(|(_, _, s)| *s > 0)
        .map(|(p, _, _)| p.as_str())
        .collect();
    if !unmerged.is_empty() {
        let mut stdout: String = unmerged
            .iter()
            .map(|p| format!("{p}: needs merge\n"))
            .collect();
        stdout.push_str(
            "You must edit all merge conflicts and then\nmark them as resolved using git add\n",
        );
        return Ok(Out {
            code: 1,
            stdout,
            stderr: String::new(),
        });
    }
    let entries: Vec<(&str, &Oid)> = index.iter().map(|(p, o, _)| (p.as_str(), o)).collect();
    let tree = c.u.tree(repo, &entries);
    let Head::Detached(current) = c.u.head(wt) else {
        return Err("rebase --continue (a HEAD on a branch)".to_owned());
    };
    let data =
        c.u.commit_data(&state.stopped)
            .expect("the stopped commit has data");
    let (before, after) = (commit_tree(c.u, &current), tree_map(c.u, &tree));
    let new =
        c.u.commit_as(repo, &data.author, &tree, &[current], &data.message);
    let subject = data.message.lines().next().unwrap_or("");
    let committed = format!(
        "[detached HEAD {}] {subject}\n{}",
        sha7(&new),
        super::fixture::diffstat(c.u, &before, &after)
    );
    let mut out = replay(c, repo, wt, state, new);
    out.stdout.insert_str(0, &committed);
    Ok(out)
}

/// `rebase --abort`: a `reset --hard` to `orig-head`, `HEAD` back where it was.
fn abort(c: &mut Ctx, wt: &Path) -> Run {
    let Some(state) = c.u.rebase_state(wt) else {
        return Ok(Out::fail(128, "fatal: no rebase in progress\n"));
    };
    let files = commit_tree(c.u, &state.orig_head);
    let index = files
        .iter()
        .map(|(p, o)| (p.clone(), o.clone(), 0))
        .collect();
    checkout(c, wt, &files, index);
    c.u.set_head(
        wt,
        match &state.branch {
            Some(b) => Head::Branch(b.clone()),
            None => Head::Detached(state.orig_head.clone()),
        },
    );
    clear(c, wt);
    Ok(Out::ok(""))
}
