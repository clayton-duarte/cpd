//! The test-only subcommands: the ones the product never runs and the tests of `cli/tests` use to build their
//! repos (`init add commit clone switch checkout reset merge tag mv rm show for-each-ref grep diff-index
//! diff-files update-index`), and the writing forms of `config`, `remote` and `branch`. Served by the same table
//! as the product's, with the same strict argument parsers.
//!
//! Not modelled (a call that needs it is `fake-git: unsupported`): `MERGE_HEAD` (a conflicted `merge` leaves the
//! stages and the markers, and a later `commit` makes a commit of one parent), the diffstat lines of a merge,
//! rename detection, an editor, `.gitignore` patterns other than literal names and `dir/`.

use super::super::merge3::line_changes;
use super::merge::{checkout, commit_tree, head_commit, merge_trees, sha7, tree_of};
use super::*;

// ---- helpers --------------------------------------------------------------------------------------------

/// The last value of `key` among the config entries of the call.
fn last(entries: &[(String, String)], key: &str) -> Option<String> {
    entries
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

/// `<user.name> <<user.email>>` of the call's config, or the refusal git gives.
fn identity(c: &Ctx, repo: RepoId) -> Result<String, Out> {
    let entries = c.u.config_entries(repo, &c.extra);
    match (last(&entries, "user.name"), last(&entries, "user.email")) {
        (Some(name), Some(email)) => Ok(format!("{name} <{email}>")),
        _ => Err(Out::fail(
            128,
            "fatal: unable to auto-detect email address\n",
        )),
    }
}

/// An fnmatch of `text` against `pattern`, where `*` stands for any run of characters and `?` for one.
fn glob(pattern: &str, text: &str) -> bool {
    fn at(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => (0..=t.len()).any(|n| at(rest, &t[n..])),
            Some(('?', rest)) => !t.is_empty() && at(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && at(rest, &t[1..]),
        }
    }
    at(
        &pattern.chars().collect::<Vec<_>>(),
        &text.chars().collect::<Vec<_>>(),
    )
}

/// The first line of a commit's message.
fn subject(u: &Universe, commit: &Oid) -> String {
    u.commit_data(commit)
        .map(|c| c.message.lines().next().unwrap_or("").to_owned())
        .unwrap_or_default()
}

/// The stage 0 entries of the index of `wt` as a map.
fn staged(u: &Universe, wt: &Path) -> BTreeMap<String, Oid> {
    u.effective_index(wt)
        .into_iter()
        .filter(|(_, _, s)| *s == 0)
        .map(|(p, o, _)| (p, o))
        .collect()
}

fn from_tree(tree: &BTreeMap<String, Oid>) -> Vec<(String, Oid, u8)> {
    tree.iter()
        .map(|(p, o)| (p.clone(), o.clone(), 0))
        .collect()
}

/// `spec` as a path relative to the worktree root (`""` is the root itself), `None` when it leaves the worktree.
fn relative(c: &Ctx, wt: &Path, spec: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let given = Path::new(spec);
    let (base, rest): (PathBuf, &Path) = if given.is_absolute() {
        (
            given.canonicalize().unwrap_or_else(|_| given.to_owned()),
            Path::new(""),
        )
    } else {
        (
            c.cwd.canonicalize().unwrap_or_else(|_| c.cwd.clone()),
            given,
        )
    };
    for part in base.strip_prefix(wt).ok()?.components() {
        parts.push(part.as_os_str().to_string_lossy().into_owned());
    }
    for part in rest.components() {
        match part {
            std::path::Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            std::path::Component::ParentDir => {
                parts.pop()?;
            }
            _ => {}
        }
    }
    Some(parts.join("/"))
}

/// Whether `path` is `dir` or lies under it (`dir` is `""` for the root).
fn under(path: &str, dir: &str) -> bool {
    dir.is_empty() || path == dir || path.starts_with(&format!("{dir}/"))
}

// ---- .gitignore: literal names and `dir/` only -----------------------------------------------------------

struct Rule {
    name: String,
    /// Names a path from the root (it has a slash), not a name at any depth.
    anchored: bool,
    dir_only: bool,
}

pub(super) struct Ignore(Vec<Rule>);

impl Ignore {
    /// The rules of the `.gitignore` at the root of `wt`; `Err` is what the call cannot model.
    fn load(wt: &Path, files: &BTreeMap<String, PathBuf>) -> Result<Ignore, String> {
        if let Some(nested) = files.keys().find(|p| p.ends_with("/.gitignore")) {
            return Err(format!("a nested .gitignore ({nested})"));
        }
        let Ok(text) = std::fs::read_to_string(wt.join(".gitignore")) else {
            return Ok(Ignore(Vec::new()));
        };
        let mut rules = Vec::new();
        for line in text
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            if line.contains(['*', '?', '[', '\\', '!']) {
                return Err(format!(".gitignore line {line}"));
            }
            let (body, dir_only) = line.strip_suffix('/').map_or((line, false), |b| (b, true));
            rules.push(Rule {
                name: body.trim_start_matches('/').to_owned(),
                anchored: body.contains('/'),
                dir_only,
            });
        }
        Ok(Ignore(rules))
    }
    /// Whether `path` (a file, or with `is_dir` a directory) or one of the directories above it is ignored.
    pub(super) fn ignores(&self, path: &str, is_dir: bool) -> bool {
        let parts: Vec<&str> = path.split('/').collect();
        self.0.iter().any(|rule| {
            (0..parts.len()).any(|i| {
                let dir = i + 1 < parts.len() || is_dir;
                if rule.dir_only && !dir {
                    return false;
                }
                if rule.anchored {
                    parts[..=i].join("/") == rule.name
                } else {
                    parts[i] == rule.name
                }
            })
        })
    }
}

/// The `Ignore` of a call, or its `Err` as the text of an unsupported call.
pub(super) fn ignore_of(
    wt: &Path,
    files: &BTreeMap<String, PathBuf>,
    sub: &str,
) -> Result<Ignore, String> {
    Ignore::load(wt, files).map_err(|what| format!("{sub} ({what})"))
}

// ---- init, clone ------------------------------------------------------------------------------------------

pub(super) fn init(c: &mut Ctx, args: &[String]) -> Run {
    let (mut quiet, mut bare, mut branch): (bool, bool, Option<String>) = (false, false, None);
    let mut pos: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-q" => quiet = true,
            "--bare" => bare = true,
            "-b" | "--initial-branch" => {
                branch = Some(it.next().ok_or_else(|| format!("init {arg}"))?.clone())
            }
            flag if flag.starts_with("--initial-branch=") => {
                branch = Some(flag["--initial-branch=".len()..].to_owned())
            }
            flag if flag.starts_with('-') => return Err(format!("init {flag}")),
            dir => pos.push(dir),
        }
    }
    let dir = match pos[..] {
        [] => c.cwd.clone(),
        [dir] => c.cwd.join(dir),
        _ => return Err("init (arguments)".to_owned()),
    };
    let existing = c.u.repo_at(&dir.to_string_lossy());
    let (id, shown) = match existing {
        Some(id) => (id, "Reinitialized existing"),
        None => {
            let id = c.u.init(&dir, bare);
            let name = branch
                .or_else(|| last(&c.u.global_entries(), "init.defaultbranch"))
                .unwrap_or_else(|| "master".to_owned());
            let path = c.u.main_path(id);
            c.u.set_head(&path, Head::Branch(name));
            (id, "Initialized empty")
        }
    };
    let place = format!("{}/", c.u.git_dir(id).display());
    Ok(Out::ok(if quiet {
        String::new()
    } else {
        format!("{shown} Git repository in {place}\n")
    }))
}

pub(super) fn clone(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("clone", args, &["-q"], &[])?;
    let quiet = flags(&toks).contains(&"-q");
    let [src, dest] = positionals(&toks)[..] else {
        return Err("clone (arguments)".to_owned());
    };
    let dest_path = c.cwd.join(dest);
    if dest_path.is_file()
        || std::fs::read_dir(&dest_path).is_ok_and(|mut entries| entries.next().is_some())
    {
        return Ok(Out::fail(
            128,
            format!(
                "fatal: destination path '{dest}' already exists and is not an empty directory.\n"
            ),
        ));
    }
    let src_path = c.cwd.join(src);
    let Some(from) = c.u.repo_at(&src_path.to_string_lossy()) else {
        return Ok(Out::fail(
            128,
            format!("fatal: repository '{src}' does not exist\n"),
        ));
    };
    let url = src_path
        .canonicalize()
        .unwrap_or(src_path)
        .to_string_lossy()
        .into_owned();
    let Head::Branch(branch) = c.u.head(&c.u.main_path(from)) else {
        return Err("clone (a source with a detached HEAD)".to_owned());
    };
    let id = c.u.init(&dest_path, false);
    let wt = c.u.main_path(id);
    let heads = c.u.refs(from, "refs/heads/");
    let tags = c.u.refs(from, "refs/tags/");
    for (_, oid) in heads.iter().chain(&tags) {
        c.u.copy(from, id, oid);
    }
    for (name, oid) in &heads {
        let short = name
            .strip_prefix("refs/heads/")
            .expect("a head has its prefix");
        c.u.set_ref(
            id,
            &format!("refs/remotes/origin/{short}"),
            oid,
            &format!("clone: from {url}"),
        );
    }
    for (name, oid) in &tags {
        c.u.set_ref(id, name, oid, &format!("clone: from {url}"));
    }
    c.u.config_add(id, "remote.origin.url", &url);
    c.u.config_add(
        id,
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    );
    c.u.set_head(&wt, Head::Branch(branch.clone()));
    let own = format!("refs/heads/{branch}");
    let empty = match heads.iter().find(|(n, _)| *n == own) {
        Some((_, tip)) => {
            c.u.set_ref(id, &own, tip, &format!("clone: from {url}"));
            c.u.set_symbolic_ref(
                id,
                "refs/remotes/origin/HEAD",
                &format!("refs/remotes/origin/{branch}"),
            );
            c.u.config_add(id, &format!("branch.{branch}.remote"), "origin");
            c.u.config_add(id, &format!("branch.{branch}.merge"), &own);
            let files = commit_tree(c.u, tip);
            for (path, blob) in &files {
                write_blob(c.u, &wt, path, blob);
            }
            c.u.set_index(&wt, from_tree(&files));
            false
        }
        None => true,
    };
    let mut stderr = String::new();
    if !quiet {
        stderr.push_str(&format!("Cloning into '{dest}'...\n"));
        if empty {
            stderr.push_str("warning: You appear to have cloned an empty repository.\n");
        }
    }
    Ok(Out {
        code: 0,
        stdout: String::new(),
        stderr,
    })
}

// ---- add, commit ------------------------------------------------------------------------------------------

pub(super) fn add(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("add", args, &["-A", "--all"], &[])?;
    let all = !flags(&toks).is_empty();
    let mut specs: Vec<&str> = positionals(&toks);
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    if specs.is_empty() {
        if !all {
            return Ok(Out::fail(0, "Nothing specified, nothing added.\n"));
        }
        specs.push(".");
    }
    let files = work_files(&wt);
    let ignore = ignore_of(&wt, &files, "add")?;
    let mut index = c.u.effective_index(&wt);
    let tracked = |index: &[(String, Oid, u8)], path: &str| index.iter().any(|(p, _, _)| p == path);
    let mut ignored: Vec<String> = Vec::new();
    let mut staged_now: BTreeMap<String, Oid> = BTreeMap::new();
    let mut gone: BTreeSet<String> = BTreeSet::new();
    let repo = c.repo().expect("a worktree has a repo");
    for spec in specs {
        let Some(rel) = relative(c, &wt, spec) else {
            return Ok(Out::fail(
                128,
                format!("fatal: {spec}: '{spec}' is outside repository\n"),
            ));
        };
        let is_dir = wt.join(&rel).is_dir();
        let matching: Vec<&String> = files.keys().filter(|p| under(p, &rel)).collect();
        let tracked_here: Vec<String> = index
            .iter()
            .filter(|(p, _, _)| under(p, &rel))
            .map(|(p, _, _)| p.clone())
            .collect();
        if matching.is_empty() && tracked_here.is_empty() && !(rel.is_empty() || is_dir) {
            return Ok(Out::fail(
                128,
                format!("fatal: pathspec '{spec}' did not match any files\n"),
            ));
        }
        if !rel.is_empty() && !tracked(&index, &rel) && ignore.ignores(&rel, is_dir) {
            ignored.push(rel.clone());
            continue;
        }
        for path in matching {
            if tracked(&index, path) || !ignore.ignores(path, false) {
                let bytes =
                    std::fs::read(&files[path]).map_err(|e| format!("add ({path}: {e})"))?;
                staged_now.insert(path.clone(), c.u.blob(repo, &bytes));
            }
        }
        gone.extend(tracked_here.into_iter().filter(|p| !files.contains_key(p)));
    }
    if !ignored.is_empty() {
        let list: String = ignored.iter().map(|p| format!("{p}\n")).collect();
        return Ok(Out::fail(
            1,
            format!(
                "The following paths are ignored by one of your .gitignore files:\n{list}hint: Use -f if you really want to add them.\nhint: Turn this message off by running\nhint: \"git config advice.addIgnoredFile false\"\n"
            ),
        ));
    }
    index.retain(|(p, _, _)| !staged_now.contains_key(p) && !gone.contains(p));
    index.extend(staged_now.into_iter().map(|(p, o)| (p, o, 0)));
    c.u.set_index(&wt, index);
    Ok(Out::ok(""))
}

/// ` 1 file changed, 2 insertions(+), 1 deletion(-)` and the `create mode` / `delete mode` lines of the change
/// from `old` to `new`; empty when nothing changed.
pub(super) fn diffstat(
    u: &Universe,
    old: &BTreeMap<String, Oid>,
    new: &BTreeMap<String, Oid>,
) -> String {
    let paths: BTreeSet<&String> = old
        .keys()
        .chain(new.keys())
        .filter(|p| old.get(*p) != new.get(*p))
        .collect();
    if paths.is_empty() {
        return String::new();
    }
    let (mut ins, mut del, mut modes) = (0, 0, String::new());
    for path in &paths {
        let bytes = |tree: &BTreeMap<String, Oid>| {
            tree.get(*path)
                .and_then(|b| u.blob_bytes(b))
                .unwrap_or_default()
        };
        let (i, d) = line_changes(&bytes(old), &bytes(new));
        (ins, del) = (ins + i, del + d);
        match (old.contains_key(*path), new.contains_key(*path)) {
            (false, true) => modes.push_str(&format!(" create mode 100644 {path}\n")),
            (true, false) => modes.push_str(&format!(" delete mode 100644 {path}\n")),
            _ => {}
        }
    }
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut line = format!(" {}", plural(paths.len(), "file changed", "files changed"));
    if ins > 0 || del == 0 {
        line.push_str(&format!(", {}(+)", plural(ins, "insertion", "insertions")));
    }
    if del > 0 || ins == 0 {
        line.push_str(&format!(", {}(-)", plural(del, "deletion", "deletions")));
    }
    format!("{line}\n{modes}")
}

pub(super) fn commit(c: &mut Ctx, args: &[String]) -> Run {
    let (mut quiet, mut allow_empty, mut message): (bool, bool, Option<&str>) =
        (false, false, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-q" => quiet = true,
            "--allow-empty" => allow_empty = true,
            "-m" => message = Some(it.next().ok_or("commit -m")?.as_str()),
            flag => return Err(format!("commit {flag}")),
        }
    }
    let Some(message) = message else {
        return Err("commit (without -m: an editor would open)".to_owned());
    };
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    let repo = c.repo().expect("a worktree has a repo");
    let index = c.u.effective_index(&wt);
    if index.iter().any(|(_, _, s)| *s > 0) {
        return Ok(Out::fail(
            128,
            "error: Committing is not possible because you have unmerged files.\nhint: Fix them up in the work tree, and then use 'git add/rm <file>'\nhint: as appropriate to mark resolution and make a commit.\nfatal: Exiting because of an unresolved conflict.\n",
        ));
    }
    let head = c.u.head(&wt);
    let parent = head_commit(c.u, repo, &wt);
    let old: BTreeMap<String, Oid> = c.u.head_tree(&wt).into_iter().collect();
    let new: BTreeMap<String, Oid> = index
        .iter()
        .map(|(p, o, _)| (p.clone(), o.clone()))
        .collect();
    if old == new && !allow_empty {
        let place = match &head {
            Head::Branch(b) => format!("On branch {b}"),
            Head::Detached(oid) => format!("HEAD detached at {}", sha7(oid)),
        };
        let clean = status(c, &["--porcelain".to_owned()])?.stdout.is_empty();
        if clean && parent.is_some() {
            return Ok(Out {
                code: 1,
                stdout: format!("{place}\nnothing to commit, working tree clean\n"),
                stderr: String::new(),
            });
        }
        return Err(
            "commit (nothing staged, and a tree that is not clean or has no commit)".to_owned(),
        );
    }
    let author = match identity(c, repo) {
        Ok(author) => author,
        Err(out) => return Ok(out),
    };
    let tree = tree_of(c.u, repo, &new);
    let commit = c.u.commit_as(
        repo,
        &author,
        &tree,
        &parent.iter().cloned().collect::<Vec<_>>(),
        message,
    );
    let initial = if parent.is_none() { " (initial)" } else { "" };
    let label = match &head {
        Head::Branch(b) => {
            c.u.set_ref(
                repo,
                &format!("refs/heads/{b}"),
                &commit,
                &format!("commit{initial}: {message}"),
            );
            b.clone()
        }
        Head::Detached(_) => {
            c.u.set_head(&wt, Head::Detached(commit.clone()));
            "detached HEAD".to_owned()
        }
    };
    if quiet {
        return Ok(Out::ok(""));
    }
    let root = if parent.is_none() {
        " (root-commit)"
    } else {
        ""
    };
    let first = message.lines().next().unwrap_or("");
    Ok(Out::ok(format!(
        "[{label}{root} {}] {first}\n{}",
        sha7(&commit),
        diffstat(c.u, &old, &new)
    )))
}

// ---- moving the working tree: switch, checkout, reset, merge ---------------------------------------------

/// Moves the index and the working tree of `wt` from the tree of `HEAD` to `to`, the way `switch` does: a path
/// that is the same in both stays as it is (its local edits too); a path that differs and that has local
/// changes, or an untracked file in the way, refuses the move and nothing is touched. With `force` every
/// tracked path becomes what `to` says.
fn move_to(c: &mut Ctx, wt: &Path, to: &BTreeMap<String, Oid>, force: bool) -> Result<(), Out> {
    if force {
        checkout(c, wt, to, from_tree(to));
        return Ok(());
    }
    let from: BTreeMap<String, Oid> = c.u.head_tree(wt).into_iter().collect();
    let index = c.u.effective_index(wt);
    if index.iter().any(|(_, _, s)| *s > 0) {
        return Err(Out::fail(
            1,
            "error: you need to resolve your current index first\n",
        ));
    }
    let current: BTreeMap<String, Oid> = index.into_iter().map(|(p, o, _)| (p, o)).collect();
    let files = work_files(wt);
    let changed: Vec<&String> = from
        .keys()
        .chain(to.keys())
        .filter(|p| from.get(*p) != to.get(*p))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let (mut dirty, mut in_the_way) = (String::new(), String::new());
    for path in &changed {
        let (in_index, work) = (
            current.get(*path),
            files.get(*path).and_then(|f| file_blob(f)),
        );
        if in_index != from.get(*path) || (in_index.is_some() && work.as_ref() != in_index) {
            dirty.push_str(&format!("\t{path}\n"));
        } else if in_index.is_none() && work.is_some() && to.contains_key(*path) {
            in_the_way.push_str(&format!("\t{path}\n"));
        }
    }
    if !in_the_way.is_empty() {
        return Err(Out::fail(
            1,
            format!(
                "error: The following untracked working tree files would be overwritten by checkout:\n{in_the_way}Please move or remove them before you switch branches.\nAborting\n"
            ),
        ));
    }
    if !dirty.is_empty() {
        return Err(Out::fail(
            1,
            format!(
                "error: Your local changes to the following files would be overwritten by checkout:\n{dirty}Please commit your changes or stash them before you switch branches.\nAborting\n"
            ),
        ));
    }
    if changed.is_empty() {
        return Ok(());
    }
    let mut next = current.clone();
    for path in changed {
        match to.get(path) {
            Some(blob) => {
                write_blob(c.u, wt, path, blob);
                next.insert(path.clone(), blob.clone());
            }
            None => {
                remove_file(wt, path);
                next.remove(path);
            }
        }
    }
    c.u.set_index(wt, from_tree(&next));
    Ok(())
}

/// Sets `branch` to track the remote-tracking branch `start` names, when it is one: the config git writes and
/// the line it says.
fn track(c: &mut Ctx, repo: RepoId, branch: &str, start: &str) -> String {
    let Some(full) = dwim_ref(c.u, repo, start) else {
        return String::new();
    };
    let Some((remote, name)) = full
        .strip_prefix("refs/remotes/")
        .and_then(|short| short.split_once('/'))
    else {
        return String::new();
    };
    if name == "HEAD" {
        return String::new();
    }
    c.u.config_set(repo, &format!("branch.{branch}.remote"), remote);
    c.u.config_set(
        repo,
        &format!("branch.{branch}.merge"),
        &format!("refs/heads/{name}"),
    );
    format!("branch '{branch}' set up to track '{remote}/{name}'.\n")
}

/// `checkout` and `switch`, which differ in the name of the flags that make a branch (`-b`/`-B`, `-c`/`-C`) and
/// in what they do with a name that is no branch.
pub(super) fn switch(c: &mut Ctx, sub: &str, args: &[String]) -> Run {
    let (make, remake) = if sub == "checkout" {
        ("-b", "-B")
    } else {
        ("-c", "-C")
    };
    let (mut quiet, mut force, mut detach): (bool, bool, bool) = (false, false, false);
    let mut create: Option<(&str, bool)> = None;
    let mut pos: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-q" => quiet = true,
            "--force" | "-f" => force = true,
            "--detach" => detach = true,
            flag if flag == make || flag == remake => {
                create = Some((
                    it.next().ok_or_else(|| format!("{sub} {flag}"))?.as_str(),
                    flag == remake,
                ))
            }
            flag if flag.starts_with('-') => return Err(format!("{sub} {flag}")),
            name => pos.push(name),
        }
    }
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    let repo = c.repo().expect("a worktree has a repo");
    let said = |stdout: String, stderr: String| {
        if quiet {
            Out::ok("")
        } else {
            Out {
                code: 0,
                stdout,
                stderr,
            }
        }
    };
    let commit_of = |c: &Ctx, rev: &str| c.parse(&format!("{rev}^{{commit}}")).ok();

    if let Some((name, reset)) = create {
        let start = match pos[..] {
            [] => "HEAD",
            [start] => start,
            _ => return Err(format!("{sub} {make} (arguments)")),
        };
        let full = format!("refs/heads/{name}");
        let exists = c.u.ref_oid(repo, &full).is_some();
        if exists && !reset {
            return Ok(Out::fail(
                128,
                format!("fatal: a branch named '{name}' already exists\n"),
            ));
        }
        let unborn = start == "HEAD" && head_commit(c.u, repo, &wt).is_none();
        if unborn {
            c.u.set_head(&wt, Head::Branch(name.to_owned()));
            return Ok(said(
                String::new(),
                format!("Switched to a new branch '{name}'\n"),
            ));
        }
        let Some(commit) = commit_of(c, start) else {
            return Ok(Out::fail(
                128,
                format!("fatal: invalid reference: {start}\n"),
            ));
        };
        let tree = commit_tree(c.u, &commit);
        if let Err(refused) = move_to(c, &wt, &tree, force) {
            return Ok(refused);
        }
        let why = if exists { "Reset to" } else { "Created from" };
        c.u.set_ref(repo, &full, &commit, &format!("branch: {why} {start}"));
        let tracking = track(c, repo, name, start);
        c.u.set_head(&wt, Head::Branch(name.to_owned()));
        let what = if exists {
            "Switched to and reset branch"
        } else {
            "Switched to a new branch"
        };
        return Ok(said(tracking, format!("{what} '{name}'\n")));
    }
    if detach {
        let rev = match pos[..] {
            [] => "HEAD",
            [rev] => rev,
            _ => return Err(format!("{sub} --detach (arguments)")),
        };
        let Some(commit) = commit_of(c, rev) else {
            return Ok(Out::fail(128, format!("fatal: invalid reference: {rev}\n")));
        };
        let tree = commit_tree(c.u, &commit);
        if let Err(refused) = move_to(c, &wt, &tree, force) {
            return Ok(refused);
        }
        c.u.set_head(&wt, Head::Detached(commit.clone()));
        return Ok(said(
            String::new(),
            format!(
                "HEAD is now at {} {}\n",
                sha7(&commit),
                subject(c.u, &commit)
            ),
        ));
    }
    let [name] = pos[..] else {
        return Err(format!("{sub} (arguments)"));
    };
    let full = format!("refs/heads/{name}");
    if let Some(tip) = c.u.ref_oid(repo, &full) {
        if c.u.head(&wt) == Head::Branch(name.to_owned()) {
            return Ok(Out {
                code: 0,
                stdout: String::new(),
                stderr: format!("Already on '{name}'\n"),
            });
        }
        let tree = commit_tree(c.u, &tip);
        if let Err(refused) = move_to(c, &wt, &tree, force) {
            return Ok(refused);
        }
        c.u.set_head(&wt, Head::Branch(name.to_owned()));
        return Ok(said(
            String::new(),
            format!("Switched to branch '{name}'\n"),
        ));
    }
    // A name only a remote has: a branch of that name that tracks it.
    let remotes: Vec<String> = remote_urls(&c.u.config_entries(repo, &c.extra))
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let tracked: Vec<String> = remotes
        .into_iter()
        .map(|r| format!("{r}/{name}"))
        .filter(|t| c.u.ref_oid(repo, &format!("refs/remotes/{t}")).is_some())
        .collect();
    match &tracked[..] {
        [one] => {
            let tip =
                c.u.ref_oid(repo, &format!("refs/remotes/{one}"))
                    .expect("the tracking branch exists");
            let tree = commit_tree(c.u, &tip);
            if let Err(refused) = move_to(c, &wt, &tree, force) {
                return Ok(refused);
            }
            c.u.set_ref(repo, &full, &tip, &format!("branch: Created from {one}"));
            let tracking = track(c, repo, name, one);
            c.u.set_head(&wt, Head::Branch(name.to_owned()));
            Ok(said(
                tracking,
                format!("Switched to a new branch '{name}'\n"),
            ))
        }
        [] if sub == "switch" => Ok(Out::fail(
            128,
            format!("fatal: invalid reference: {name}\n"),
        )),
        [] if commit_of(c, name).is_some() => {
            Err(format!("checkout {name} (a revision that is no branch)"))
        }
        [] => Ok(Out::fail(
            1,
            format!("error: pathspec '{name}' did not match any file(s) known to git\n"),
        )),
        _ => Err(format!("{sub} {name} (a name that several remotes have)")),
    }
}

pub(super) fn reset(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("reset", args, &["-q", "--hard"], &[])?;
    let flags = flags(&toks);
    if !flags.contains(&"--hard") {
        return Err("reset (without --hard)".to_owned());
    }
    let rev = match positionals(&toks)[..] {
        [] => "HEAD",
        [rev] => rev,
        _ => return Err("reset (arguments)".to_owned()),
    };
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    let repo = c.repo().expect("a worktree has a repo");
    let Ok(commit) = c.parse(&format!("{rev}^{{commit}}")) else {
        return Ok(ambiguous(rev));
    };
    let tree = commit_tree(c.u, &commit);
    checkout(c, &wt, &tree, from_tree(&tree));
    match c.u.head(&wt) {
        Head::Branch(b) => c.u.set_ref(
            repo,
            &format!("refs/heads/{b}"),
            &commit,
            &format!("reset: moving to {rev}"),
        ),
        Head::Detached(_) => c.u.set_head(&wt, Head::Detached(commit.clone())),
    }
    Ok(Out::ok(if flags.contains(&"-q") {
        String::new()
    } else {
        format!(
            "HEAD is now at {} {}\n",
            sha7(&commit),
            subject(c.u, &commit)
        )
    }))
}

/// How git names what a merge merged in its message: `branch 'x'`, `remote-tracking branch 'x'`, `tag 'x'`.
fn merged_name(c: &Ctx, repo: RepoId, name: &str) -> String {
    match dwim_ref(c.u, repo, name).as_deref() {
        Some(full) if full.starts_with("refs/heads/") => format!("branch '{name}'"),
        Some(full) if full.starts_with("refs/remotes/") => {
            format!("remote-tracking branch '{name}'")
        }
        Some(full) if full.starts_with("refs/tags/") => format!("tag '{name}'"),
        _ => format!("commit '{name}'"),
    }
}

pub(super) fn merge(c: &mut Ctx, args: &[String]) -> Run {
    let (mut quiet, mut no_ff, mut ff_only, mut ours): (bool, bool, bool, bool) =
        (false, false, false, false);
    let mut message: Option<&str> = None;
    let mut pos: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let strategy = |value: &str| {
            if value == "ours" {
                Ok(true)
            } else {
                Err(format!("merge -X {value}"))
            }
        };
        match arg.as_str() {
            "-q" => quiet = true,
            "--no-edit" => {}
            "--no-ff" => no_ff = true,
            "--ff-only" => ff_only = true,
            "-m" => message = Some(it.next().ok_or("merge -m")?.as_str()),
            "-X" => ours = strategy(it.next().ok_or("merge -X")?.as_str())?,
            flag if flag.starts_with("-X") => ours = strategy(&flag[2..])?,
            flag if flag.starts_with('-') => return Err(format!("merge {flag}")),
            name => pos.push(name),
        }
    }
    let [name] = pos[..] else {
        return Err("merge (arguments)".to_owned());
    };
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    let repo = c.repo().expect("a worktree has a repo");
    let Some(head) = head_commit(c.u, repo, &wt) else {
        return Err("merge (an unborn HEAD)".to_owned());
    };
    let Ok(theirs) = c.parse(&format!("{name}^{{commit}}")) else {
        return Ok(Out::fail(
            1,
            format!("merge: {name} - not something we can merge\n"),
        ));
    };
    let contains =
        |u: &Universe, outer: &Oid, inner: &Oid| u.is_ancestor(repo, inner, outer).unwrap_or(false);
    if contains(c.u, &head, &theirs) {
        return Ok(Out::ok(if quiet { "" } else { "Already up to date.\n" }));
    }
    let branch = c.u.head(&wt);
    let advance = |c: &mut Ctx, to: &Oid, why: &str| match &branch {
        Head::Branch(b) => c.u.set_ref(repo, &format!("refs/heads/{b}"), to, why),
        Head::Detached(_) => c.u.set_head(&wt, Head::Detached(to.clone())),
    };
    if !no_ff && contains(c.u, &theirs, &head) {
        if !quiet {
            return Err(
                "merge (a fast-forward without -q: the diffstat is not modelled)".to_owned(),
            );
        }
        let tree = commit_tree(c.u, &theirs);
        if let Err(refused) = move_to(c, &wt, &tree, false) {
            return Ok(refused);
        }
        advance(c, &theirs, &format!("merge {name}: Fast-forward"));
        return Ok(Out::ok(""));
    }
    if ff_only {
        return Ok(Out::fail(
            128,
            "fatal: Not possible to fast-forward, aborting.\n",
        ));
    }
    let Some(base) = c.u.merge_base(repo, &head, &theirs) else {
        return Ok(Out::fail(
            128,
            "fatal: refusing to merge unrelated histories\n",
        ));
    };
    let [base_tree, ours_tree, theirs_tree] =
        [&base, &head, &theirs].map(|commit| commit_tree(c.u, commit));
    let merged = merge_trees(
        c.u,
        repo,
        &base_tree,
        &ours_tree,
        &theirs_tree,
        ("HEAD", name),
        ours,
    );
    if !merged.conflicts.is_empty() {
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
        checkout(c, &wt, &merged.entries, index);
        let mut stdout: String = merged.messages.iter().map(|m| format!("{m}\n")).collect();
        stdout.push_str("Automatic merge failed; fix conflicts and then commit the result.\n");
        return Ok(Out {
            code: 1,
            stdout,
            stderr: String::new(),
        });
    }
    if let Err(refused) = move_to(c, &wt, &merged.entries, false) {
        return Ok(refused);
    }
    let author = match identity(c, repo) {
        Ok(author) => author,
        Err(out) => return Ok(out),
    };
    let text = match (message, &branch) {
        (Some(message), _) => message.to_owned(),
        (None, Head::Branch(b)) if b == "main" || b == "master" => {
            format!("Merge {}", merged_name(c, repo, name))
        }
        (None, Head::Branch(b)) => format!("Merge {} into {b}", merged_name(c, repo, name)),
        (None, Head::Detached(_)) => format!("Merge {} into HEAD", merged_name(c, repo, name)),
    };
    let tree = tree_of(c.u, repo, &merged.entries);
    let commit = c.u.commit_as(repo, &author, &tree, &[head, theirs], &text);
    advance(
        c,
        &commit,
        &format!("merge {name}: Merge made by the 'ort' strategy."),
    );
    Ok(Out::ok(if quiet {
        ""
    } else {
        "Merge made by the 'ort' strategy.\n"
    }))
}

// ---- branch: list and create ----------------------------------------------------------------------------------

/// `branch [--list] [-r] [--format=<f>] [<pattern>...]`.
pub(super) fn list_branches(
    c: &mut Ctx,
    repo: RepoId,
    remotes: bool,
    format: Option<&str>,
    patterns: &[&str],
) -> Run {
    let prefix = if remotes {
        "refs/remotes/"
    } else {
        "refs/heads/"
    };
    let mut refs = c.u.refs(repo, prefix);
    if remotes {
        let heads = c.u.symbolic_refs(repo, prefix);
        refs.extend(
            heads
                .iter()
                .filter_map(|(name, _)| Some((name.clone(), c.u.resolve_ref(repo, name)?))),
        );
        refs.sort();
    }
    let here = c.worktree().ok().map(|wt| c.u.head(&wt));
    let elsewhere: Vec<Head> =
        c.u.worktrees(repo)
            .iter()
            .map(|w| c.u.head(&w.path))
            .collect();
    let mut out = String::new();
    if format.is_none()
        && !remotes
        && let Some(Head::Detached(oid)) = &here
    {
        out.push_str(&format!("* (HEAD detached at {})\n", sha7(oid)));
    }
    for (name, oid) in refs {
        let short = &name[prefix.len()..];
        if !patterns.is_empty() && !patterns.iter().any(|p| glob(p, short)) {
            continue;
        }
        match format {
            Some("%(refname:short) %(objectname)") => out.push_str(&format!("{short} {oid}\n")),
            Some(other) => return Err(format!("branch --format={other}")),
            None if remotes => match c.u.symbolic_ref(repo, &name) {
                Some(target) => {
                    out.push_str(&format!("  {short} -> {}\n", &target[prefix.len()..]))
                }
                None => out.push_str(&format!("  {short}\n")),
            },
            None => {
                let this = Head::Branch(short.to_owned());
                let mark = if here.as_ref() == Some(&this) {
                    '*'
                } else if elsewhere.contains(&this) {
                    '+'
                } else {
                    ' '
                };
                out.push_str(&format!("{mark} {short}\n"));
            }
        }
    }
    Ok(Out::ok(out))
}

/// `branch <name> [<start>]`.
pub(super) fn create_branch(c: &mut Ctx, repo: RepoId, name: &str, start: Option<&str>) -> Run {
    let full = format!("refs/heads/{name}");
    if c.u.ref_oid(repo, &full).is_some() {
        return Ok(Out::fail(
            128,
            format!("fatal: a branch named '{name}' already exists\n"),
        ));
    }
    let tip = match start {
        Some(start) => c
            .parse(&format!("{start}^{{commit}}"))
            .map_err(|_| Out::fail(128, format!("fatal: not a valid object name: '{start}'\n"))),
        None => {
            let wt = c.u.worktree_of(&c.cwd).unwrap_or_else(|| c.cwd.clone());
            head_commit(c.u, repo, &wt).ok_or_else(|| {
                let on = match c.u.head(&wt) {
                    Head::Branch(b) => b,
                    Head::Detached(_) => "HEAD".to_owned(),
                };
                Out::fail(128, format!("fatal: not a valid object name: '{on}'\n"))
            })
        }
    };
    let tip = match tip {
        Ok(tip) => tip,
        Err(out) => return Ok(out),
    };
    let from = start.unwrap_or("HEAD");
    c.u.set_ref(repo, &full, &tip, &format!("branch: Created from {from}"));
    Ok(Out::ok(
        start.map_or_else(String::new, |s| track(c, repo, name, s)),
    ))
}

// ---- tag ----------------------------------------------------------------------------------------------------

pub(super) fn tag(c: &mut Ctx, args: &[String]) -> Run {
    let (mut annotate, mut delete, mut list): (bool, bool, bool) = (false, false, false);
    let mut message: Option<&str> = None;
    let mut pos: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-a" => annotate = true,
            "-d" => delete = true,
            "-l" | "--list" => list = true,
            "-m" => message = Some(it.next().ok_or("tag -m")?.as_str()),
            flag if flag.starts_with('-') => return Err(format!("tag {flag}")),
            name => pos.push(name),
        }
    }
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    if list || (pos.is_empty() && !delete && !annotate) {
        let names: Vec<String> =
            c.u.refs(repo, "refs/tags/")
                .into_iter()
                .map(|(n, _)| n["refs/tags/".len()..].to_owned())
                .collect();
        let shown: String = names
            .iter()
            .filter(|n| pos.is_empty() || pos.iter().any(|p| glob(p, n)))
            .map(|n| format!("{n}\n"))
            .collect();
        return Ok(Out::ok(shown));
    }
    if delete {
        let (mut stdout, mut stderr, mut code) = (String::new(), String::new(), 0);
        for name in pos {
            match c.u.ref_oid(repo, &format!("refs/tags/{name}")) {
                Some(oid) => {
                    c.u.delete_ref(repo, &format!("refs/tags/{name}"));
                    stdout.push_str(&format!("Deleted tag '{name}' (was {})\n", sha7(&oid)));
                }
                None => (stderr, code) = (stderr + &format!("error: tag '{name}' not found.\n"), 1),
            }
        }
        return Ok(Out {
            code,
            stdout,
            stderr,
        });
    }
    let (name, rev) = match pos[..] {
        [name] => (name, "HEAD"),
        [name, rev] => (name, rev),
        _ => return Err("tag (arguments)".to_owned()),
    };
    let full = format!("refs/tags/{name}");
    if c.u.ref_oid(repo, &full).is_some() {
        return Ok(Out::fail(
            128,
            format!("fatal: tag '{name}' already exists\n"),
        ));
    }
    let Ok(target) = c.parse(&format!("{rev}^{{commit}}")) else {
        return Ok(Out::fail(
            128,
            format!("fatal: Failed to resolve '{rev}' as a valid ref.\n"),
        ));
    };
    let oid = if annotate || message.is_some() {
        let Some(message) = message else {
            return Err("tag -a (without -m: an editor would open)".to_owned());
        };
        let tagger = match identity(c, repo) {
            Ok(tagger) => tagger,
            Err(out) => return Ok(out),
        };
        c.u.tag_object(repo, &target, name, &tagger, message)
    } else {
        target
    };
    c.u.set_ref(repo, &full, &oid, "tag");
    Ok(Out::ok(""))
}

// ---- mv, rm, show -------------------------------------------------------------------------------------------

pub(super) fn mv(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("mv", args, &[], &[])?;
    let [src, dst] = positionals(&toks)[..] else {
        return Err("mv (arguments)".to_owned());
    };
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    let bad = |why: &str| {
        Ok(Out::fail(
            128,
            format!("fatal: {why}, source={src}, destination={dst}\n"),
        ))
    };
    let (Some(from), Some(to)) = (relative(c, &wt, src), relative(c, &wt, dst)) else {
        return bad("can not move outside the repository");
    };
    let index = c.u.effective_index(&wt);
    if !wt.join(&from).exists() {
        return bad("bad source");
    }
    let Some((_, blob, _)) = index.iter().find(|(p, _, s)| *p == from && *s == 0) else {
        return bad("not under version control");
    };
    let to = if wt.join(&to).is_dir() {
        format!("{to}/{}", from.rsplit('/').next().unwrap_or(&from))
    } else {
        to
    };
    if wt.join(&to).exists() {
        return bad("destination exists");
    }
    if std::fs::rename(wt.join(&from), wt.join(&to)).is_err() {
        return Ok(Out::fail(
            128,
            format!("fatal: renaming '{src}' failed: No such file or directory\n"),
        ));
    }
    let blob = blob.clone();
    let mut next = staged(c.u, &wt);
    next.remove(&from);
    next.insert(to, blob);
    c.u.set_index(&wt, from_tree(&next));
    Ok(Out::ok(""))
}

pub(super) fn rm(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("rm", args, &["-q"], &[])?;
    let quiet = !flags(&toks).is_empty();
    let specs = positionals(&toks);
    if specs.is_empty() {
        return Err("rm (arguments)".to_owned());
    }
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    let head: BTreeMap<String, Oid> = c.u.head_tree(&wt).into_iter().collect();
    let mut next = staged(c.u, &wt);
    let files = work_files(&wt);
    let (mut staged_changes, mut local, mut both) = (String::new(), String::new(), String::new());
    let mut removed = Vec::new();
    for spec in specs {
        let Some(path) = relative(c, &wt, spec).filter(|p| next.contains_key(p)) else {
            return Ok(Out::fail(
                128,
                format!("fatal: pathspec '{spec}' did not match any files\n"),
            ));
        };
        let in_index = &next[&path];
        let work = files.get(&path).and_then(|f| file_blob(f));
        let (staged_diff, work_diff) = (
            head.get(&path) != Some(in_index),
            work.as_ref().is_some_and(|w| w != in_index),
        );
        match (staged_diff, work_diff) {
            (true, true) => both.push_str(&format!("    {path}\n")),
            (true, false) => staged_changes.push_str(&format!("    {path}\n")),
            (false, true) => local.push_str(&format!("    {path}\n")),
            (false, false) => removed.push(path),
        }
    }
    let one = |list: &str| {
        if list.lines().count() == 1 {
            "file has"
        } else {
            "files have"
        }
    };
    for (list, what, hint) in [
        (
            &staged_changes,
            "changes staged in the index",
            "(use --cached to keep the file, or -f to force removal)",
        ),
        (
            &local,
            "local modifications",
            "(use --cached to keep the file, or -f to force removal)",
        ),
        (
            &both,
            "staged content different from both the\nfile and the HEAD",
            "(use -f to force removal)",
        ),
    ] {
        if !list.is_empty() {
            return Ok(Out::fail(
                1,
                format!("error: the following {} {what}:\n{list}{hint}\n", one(list)),
            ));
        }
    }
    let mut stdout = String::new();
    for path in removed {
        remove_file(&wt, &path);
        next.remove(&path);
        stdout.push_str(&format!("rm '{path}'\n"));
    }
    c.u.set_index(&wt, from_tree(&next));
    Ok(Out::ok(if quiet { String::new() } else { stdout }))
}

pub(super) fn show(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("show", args, &[], &[])?;
    let [spec] = positionals(&toks)[..] else {
        return Err("show (arguments)".to_owned());
    };
    let Some((rev, path)) = spec.split_once(':') else {
        return Err(format!("show {spec} (a revision without a path)"));
    };
    if c.repo().is_none() {
        return Ok(Out::fail(128, NOT_A_REPO));
    }
    let blob = if rev.is_empty() {
        c.parse(spec).map_err(|_| {
            Out::fail(
                128,
                format!("fatal: path '{path}' does not exist (neither on disk nor in the index)\n"),
            )
        })
    } else {
        match c.tree_of(rev) {
            Err(_) => return Ok(ambiguous(rev)),
            Ok(tree) => tree.get(path).cloned().ok_or_else(|| {
                Out::fail(
                    128,
                    format!("fatal: path '{path}' does not exist in '{rev}'\n"),
                )
            }),
        }
    };
    Ok(match blob {
        Ok(blob) => Out::ok(
            String::from_utf8_lossy(&c.u.blob_bytes(&blob).unwrap_or_default()).into_owned(),
        ),
        Err(out) => out,
    })
}

// ---- for-each-ref, grep -----------------------------------------------------------------------------------------

/// Every ref under `refs/` (symbolic ones resolved) as `(full name, object)`, sorted by name.
fn all_refs(u: &Universe, repo: RepoId) -> Vec<(String, Oid)> {
    let mut refs = u.refs(repo, "refs/");
    refs.extend(
        u.symbolic_refs(repo, "refs/")
            .into_iter()
            .filter_map(|(name, _)| Some((name.clone(), u.resolve_ref(repo, &name)?))),
    );
    refs.sort();
    refs
}

pub(super) fn for_each_ref(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("for-each-ref", args, &[], &["--format="])?;
    let format = flags(&toks)
        .iter()
        .find_map(|f| f.strip_prefix("--format="))
        .unwrap_or("%(objectname) %(objecttype)\t%(refname)")
        .to_owned();
    let patterns = positionals(&toks);
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let mut out = String::new();
    for (name, oid) in all_refs(c.u, repo) {
        let wanted = patterns.is_empty()
            || patterns
                .iter()
                .any(|p| name == *p || name.starts_with(&format!("{}/", p.trim_end_matches('/'))));
        if !wanted {
            continue;
        }
        let kind = if c.u.kind(&oid) == Some(super::super::universe::Kind::Tag) {
            "tag"
        } else {
            "commit"
        };
        let mut line = String::new();
        let mut rest = format.as_str();
        while let Some(at) = rest.find("%(") {
            line.push_str(&rest[..at]);
            let end = rest[at..]
                .find(')')
                .ok_or("for-each-ref (an unterminated %( in the format)")?
                + at;
            match &rest[at + 2..end] {
                "refname" => line.push_str(&name),
                "refname:short" => line.push_str(shorten(&name)),
                "objectname" => line.push_str(oid.as_str()),
                "objecttype" => line.push_str(kind),
                atom => return Err(format!("for-each-ref %({atom})")),
            }
            rest = &rest[end + 1..];
        }
        line.push_str(rest);
        out.push_str(&format!("{line}\n"));
    }
    Ok(Out::ok(out))
}

/// Whether `word` is in `text` between two characters that are not part of a word (or its ends).
fn has_word(text: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        text[..at].chars().next_back().is_none_or(|c| !is_word(c))
            && text[at + word.len()..]
                .chars()
                .next()
                .is_none_or(|c| !is_word(c))
    })
}

/// `grep -l -w <word> [--] <pathspec>...`: the tracked files whose working-tree text has the word, sorted.
/// A pathspec is a file or directory, or `*<suffix>` (the `*` crosses `/`).
pub(super) fn grep(c: &mut Ctx, args: &[String]) -> Run {
    let (mut word, mut specs, mut after): (Option<&str>, Vec<&str>, bool) =
        (None, Vec::new(), false);
    for arg in args {
        match arg.as_str() {
            "--" if !after => after = true,
            "-l" | "-w" if !after => {}
            flag if flag.starts_with('-') && !after => return Err(format!("grep {flag}")),
            text if after => specs.push(text),
            text if word.is_none() => word = Some(text),
            text => specs.push(text),
        }
    }
    let Some(word) = word else {
        return Err("grep (without a pattern)".to_owned());
    };
    if word.contains([
        '\\', '.', '[', ']', '*', '^', '$', '+', '?', '(', ')', '{', '}', '|',
    ]) {
        return Err(format!(
            "grep {word} (a pattern that is a regular expression)"
        ));
    }
    let matches = |path: &str, spec: &str| -> Result<bool, String> {
        match spec.strip_prefix('*') {
            Some(suffix) if suffix.contains(['*', '?', '[']) => Err(format!("grep {spec}")),
            Some(suffix) => Ok(path.ends_with(suffix)),
            None if spec.contains(['?', '[']) => Err(format!("grep {spec}")),
            None => Ok(under(path, spec.trim_end_matches('/'))),
        }
    };
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    let mut found = String::new();
    for (path, _) in staged(c.u, &wt) {
        let mut wanted = specs.is_empty();
        for spec in &specs {
            wanted |= matches(&path, spec)?;
        }
        let Ok(bytes) = std::fs::read(wt.join(&path)) else {
            continue;
        };
        if wanted && has_word(&String::from_utf8_lossy(&bytes), word) {
            found.push_str(&format!("{path}\n"));
        }
    }
    Ok(if found.is_empty() {
        Out::silent(1)
    } else {
        Out::ok(found)
    })
}

// ---- diff-index, diff-files, update-index -------------------------------------------------------------------

pub(super) fn diff_index(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("diff-index", args, &["--quiet", "--"], &[])?;
    let (quiet, pos) = (flags(&toks).contains(&"--quiet"), positionals(&toks));
    let ([rev], true) = (&pos[..], quiet) else {
        return Err("diff-index (arguments)".to_owned());
    };
    diff(c, &["--quiet".to_owned(), (*rev).to_owned()])
}

pub(super) fn diff_files(c: &mut Ctx, args: &[String]) -> Run {
    if args != ["--quiet"] {
        return Err("diff-files (arguments)".to_owned());
    }
    diff(c, &["--quiet".to_owned()])
}

pub(super) fn update_index(args: &[String]) -> Run {
    let mut given: Vec<&str> = args.iter().map(String::as_str).collect();
    given.sort_unstable();
    if given != ["--refresh", "-q"] {
        return Err(format!("update-index {}", args.join(" ")));
    }
    Ok(Out::ok(""))
}
