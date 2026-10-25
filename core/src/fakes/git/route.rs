//! `GitRoute`: the `Handler` of the program `git`, the argv of git over a [`Universe`].
//!
//! Every subcommand parses its flags strictly: one it does not list is `fake-git: unsupported: <subcommand>
//! <flag>` on stderr, exit 1, and the hit is recorded for [`GitRoute::assert_supported`]. The route never
//! sleeps and starts no process.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::repo::{file_blob, work_files};
use super::rev::{self, RevError, Selection, dwim_ref};
use super::universe::{Head, Oid, RepoId, Universe, is_oid, norm_key};
use crate::host::runner::{Call, Handler, Reply};

pub struct GitRoute {
    universe: Arc<Mutex<Universe>>,
    unsupported: Mutex<Vec<String>>,
}

impl GitRoute {
    pub fn new(universe: Arc<Mutex<Universe>>) -> GitRoute {
        GitRoute { universe, unsupported: Mutex::new(Vec::new()) }
    }
    /// Every call the route refused as `fake-git: unsupported: ...`, as the text after that prefix
    /// (`status --bogus`, `nonsense`), in order.
    pub fn unsupported(&self) -> Vec<String> {
        self.unsupported.lock().expect("the record is never held across a panic").clone()
    }
    /// The drop hook of the test kit (`Fakes`/`Fx`): panics when the route refused a call, so a test cannot
    /// read exit 1 as an answer. Does nothing while the thread already panics.
    pub fn assert_supported(&self) {
        let refused = self.unsupported();
        if !refused.is_empty() && !std::thread::panicking() {
            panic!("fake-git refused calls it does not model: {refused:?}");
        }
    }
}

/// What a call answers.
struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Out {
    fn ok(stdout: impl Into<String>) -> Out {
        Out { code: 0, stdout: stdout.into(), stderr: String::new() }
    }
    fn silent(code: i32) -> Out {
        Out { code, stdout: String::new(), stderr: String::new() }
    }
    fn fail(code: i32, stderr: impl Into<String>) -> Out {
        Out { code, stdout: String::new(), stderr: stderr.into() }
    }
}

/// `Err` is the text of an unsupported call.
type Run = Result<Out, String>;

const NOT_A_REPO: &str = "fatal: not a git repository (or any of the parent directories): .git\n";
const VERSION: &str = "git version 2.43.0\n";
const GLOBAL_KEYS: [&str; 5] =
    ["core.editor", "sequence.editor", "core.quotepath", "core.filesreflocktimeout", "core.packedrefstimeout"];

fn ambiguous(spec: &str) -> Out {
    Out::fail(
        128,
        format!(
            "fatal: ambiguous argument '{spec}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'\n"
        ),
    )
}

/// What one call works on: the Universe, the directory and the config the call added.
struct Ctx<'a> {
    u: &'a mut Universe,
    cwd: PathBuf,
    extra: Vec<(String, String)>,
}

impl Ctx<'_> {
    fn repo(&self) -> Option<RepoId> {
        self.u.repo_of(&self.cwd)
    }
    /// The root of the worktree of `cwd`, or the answer of a call that needs one.
    fn worktree(&self) -> Result<PathBuf, Out> {
        let repo = self.repo().ok_or_else(|| Out::fail(128, NOT_A_REPO))?;
        if self.u.is_bare(repo) {
            return Err(Out::fail(128, "fatal: this operation must be run in a work tree\n"));
        }
        Ok(self.u.worktree_of(&self.cwd).expect("a repo's path has a worktree"))
    }
    /// A revision of this call's worktree.
    fn parse(&self, spec: &str) -> Result<Oid, RevError> {
        rev::parse(self.u, &self.cwd, spec)
    }
    fn tree_of(&self, spec: &str) -> Result<BTreeMap<String, Oid>, RevError> {
        let tree = self.parse(&format!("{spec}^{{tree}}"))?;
        Ok(self.u.tree_entries(&tree).unwrap_or_default().into_iter().collect())
    }
}

enum Tok {
    Flag(String),
    Pos(String),
}

/// Splits `args` into flags and positionals, refusing a flag that is neither in `exact` nor starts with one
/// of `prefixes`. `--end-of-options`, when listed, makes everything after it positional.
fn scan(sub: &str, args: &[String], exact: &[&str], prefixes: &[&str]) -> Result<Vec<Tok>, String> {
    let mut toks = Vec::new();
    let mut flags_over = false;
    for arg in args {
        if flags_over || !arg.starts_with('-') || arg == "-" {
            toks.push(Tok::Pos(arg.clone()));
        } else if exact.contains(&arg.as_str()) || prefixes.iter().any(|p| arg.starts_with(p)) {
            flags_over = arg == "--end-of-options";
            toks.push(Tok::Flag(arg.clone()));
        } else {
            return Err(format!("{sub} {arg}"));
        }
    }
    Ok(toks)
}

fn flags(toks: &[Tok]) -> Vec<&str> {
    toks.iter().filter_map(|t| if let Tok::Flag(f) = t { Some(f.as_str()) } else { None }).collect()
}

fn positionals(toks: &[Tok]) -> Vec<&str> {
    toks.iter().filter_map(|t| if let Tok::Pos(p) = t { Some(p.as_str()) } else { None }).collect()
}

impl GitRoute {
    fn run(&self, call: &Call) -> Run {
        let mut cwd = call.cwd.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
        let mut extra = env_config(call)?;
        let args = &call.args;
        let mut i = 0;
        while i < args.len() {
            let arg = args[i].as_str();
            match arg {
                "-C" => {
                    let dir = args.get(i + 1).ok_or("-C")?;
                    cwd = cwd.join(dir);
                    if !cwd.is_dir() {
                        return Ok(Out::fail(128, format!("fatal: cannot change to '{dir}': No such file or directory\n")));
                    }
                    i += 2;
                }
                "-c" => {
                    let setting = args.get(i + 1).ok_or("-c")?;
                    let (key, value) = setting.split_once('=').unwrap_or((setting, "true"));
                    let lower = key.to_lowercase();
                    if !GLOBAL_KEYS.contains(&lower.as_str()) && !lower.starts_with("user.") {
                        return Err(format!("-c {key}"));
                    }
                    extra.push((key.to_owned(), value.to_owned()));
                    i += 2;
                }
                "--no-optional-locks" => i += 1,
                "--version" => return Ok(Out::ok(VERSION)),
                flag if flag.starts_with('-') => return Err(flag.to_owned()),
                _ => break,
            }
        }
        let sub = args.get(i).ok_or("(no subcommand)")?.as_str();
        let rest = &args[i + 1..];
        let mut guard = self.universe.lock().expect("the Universe's lock is never held across a panic");
        let mut ctx = Ctx { u: &mut guard, cwd, extra };
        match sub {
            "rev-parse" => rev_parse(&mut ctx, rest),
            "symbolic-ref" => symbolic_ref(&mut ctx, rest),
            "config" => config(&mut ctx, rest),
            "remote" => remote(&mut ctx, rest),
            "cat-file" => cat_file(&mut ctx, rest),
            "merge-base" => merge_base(&mut ctx, rest),
            "rev-list" => rev_list(&mut ctx, rest),
            "ls-tree" => ls_tree(&mut ctx, rest),
            "log" => log(&mut ctx, rest),
            "status" => status(&mut ctx, rest),
            "diff" => diff(&mut ctx, rest),
            "ls-remote" => ls_remote(&mut ctx, rest),
            other => Err(other.to_owned()),
        }
    }
}

impl Handler for GitRoute {
    fn handle(&self, call: &Call) -> Reply {
        match self.run(call) {
            Ok(out) => Reply::exit(out.code, &out.stdout, &out.stderr),
            Err(text) => {
                self.unsupported.lock().expect("the record is never held across a panic").push(text.clone());
                Reply::exit(1, "", &format!("fake-git: unsupported: {text}\n"))
            }
        }
    }
}

/// The config a call carries in `GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_<n>` and `GIT_CONFIG_VALUE_<n>`.
fn env_config(call: &Call) -> Result<Vec<(String, String)>, String> {
    let env = |name: &str| call.envs.iter().rev().find(|(k, _)| k == name).and_then(|(_, v)| v.clone());
    let Some(count) = env("GIT_CONFIG_COUNT") else { return Ok(Vec::new()) };
    let count: usize = count.parse().map_err(|_| "GIT_CONFIG_COUNT".to_owned())?;
    (0..count)
        .map(|n| match (env(&format!("GIT_CONFIG_KEY_{n}")), env(&format!("GIT_CONFIG_VALUE_{n}"))) {
            (Some(k), Some(v)) => Ok((k, v)),
            _ => Err(format!("GIT_CONFIG_COUNT {count} without key and value {n}")),
        })
        .collect()
}

// ---- rev-parse, symbolic-ref ---------------------------------------------------------------------

fn rev_parse(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan(
        "rev-parse",
        args,
        &["--verify", "--quiet", "-q", "--end-of-options", "--short", "--git-common-dir", "--git-path"],
        &["--short=", "--path-format="],
    )?;
    let flags = flags(&toks);
    let has = |f: &str| flags.contains(&f);
    let mut short = None;
    let mut absolute = false;
    for flag in &flags {
        if *flag == "--short" {
            short = Some(7);
        } else if let Some(n) = flag.strip_prefix("--short=") {
            short = Some(n.parse::<usize>().map_err(|_| format!("rev-parse {flag}"))?.clamp(4, 40));
        } else if let Some(format) = flag.strip_prefix("--path-format=") {
            if format != "absolute" {
                return Err(format!("rev-parse {flag}"));
            }
            absolute = true;
        }
    }
    let pos = positionals(&toks);
    let Some(repo) = c.repo() else { return Ok(Out::fail(128, NOT_A_REPO)) };
    let show = |oid: &Oid| match short {
        Some(n) => oid.as_str()[..n].to_owned(),
        None => oid.to_string(),
    };

    if has("--git-common-dir") {
        let git_dir = c.u.git_dir(repo);
        let at_root = c.u.worktree_of(&c.cwd).is_some_and(|wt| c.cwd.canonicalize().ok().as_deref() == Some(wt.as_path()));
        let main_root = !c.u.is_bare(repo) && at_root && c.u.worktrees(repo)[0].path == c.cwd.canonicalize().unwrap_or_default();
        return Ok(Out::ok(if c.u.is_bare(repo) && at_root {
            ".\n".to_owned()
        } else if main_root {
            ".git\n".to_owned()
        } else {
            format!("{}\n", git_dir.display())
        }));
    }
    if has("--git-path") {
        if !absolute {
            return Err("rev-parse --git-path (without --path-format=absolute)".to_owned());
        }
        let per_worktree = ["index", "rebase-merge", "rebase-apply", "HEAD", "ORIG_HEAD", "MERGE_HEAD"];
        let mut out = String::new();
        for name in pos {
            let base = if per_worktree.iter().any(|p| name == *p || name.starts_with(&format!("{p}/"))) {
                match c.u.worktree_of(&c.cwd) {
                    Some(wt) => c.u.admin_dir(&wt),
                    None => c.u.git_dir(repo),
                }
            } else {
                c.u.git_dir(repo)
            };
            out.push_str(&format!("{}\n", base.join(name).display()));
        }
        return Ok(Out::ok(out));
    }
    if has("--verify") {
        let quiet = has("--quiet") || has("-q");
        let resolved = match pos.as_slice() {
            [spec] => c.parse(spec).ok(),
            _ => None,
        };
        return Ok(match resolved {
            Some(oid) => Out::ok(format!("{}\n", show(&oid))),
            None if quiet && pos.len() == 1 => Out::silent(1),
            None => Out::fail(128, "fatal: Needed a single revision\n"),
        });
    }
    let mut out = String::new();
    for spec in pos {
        match c.parse(spec) {
            Ok(oid) => out.push_str(&format!("{}\n", show(&oid))),
            Err(_) => {
                out.push_str(&format!("{spec}\n"));
                return Ok(Out { code: 128, stdout: out, stderr: ambiguous(spec).stderr });
            }
        }
    }
    Ok(Out::ok(out))
}

/// `refs/heads/x` as `x`, `refs/remotes/o/x` as `o/x`.
fn shorten(name: &str) -> &str {
    ["refs/heads/", "refs/tags/", "refs/remotes/"].iter().find_map(|p| name.strip_prefix(p)).unwrap_or(name)
}

fn symbolic_ref(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("symbolic-ref", args, &["-q", "--quiet", "--short"], &[])?;
    let flags = flags(&toks);
    let (quiet, short) = (flags.contains(&"-q") || flags.contains(&"--quiet"), flags.contains(&"--short"));
    let [name] = positionals(&toks)[..] else { return Err("symbolic-ref (arguments)".to_owned()) };
    let Some(repo) = c.repo() else { return Ok(Out::fail(128, NOT_A_REPO)) };
    let target = if name == "HEAD" {
        match c.u.head(&c.cwd) {
            Head::Branch(b) => Some(format!("refs/heads/{b}")),
            Head::Detached(_) => None,
        }
    } else {
        c.u.symbolic_ref(repo, name)
    };
    Ok(match target {
        Some(t) => Out::ok(format!("{}\n", if short { shorten(&t) } else { &t })),
        None if quiet => Out::silent(1),
        None => Out::fail(128, format!("fatal: ref {name} is not a symbolic ref\n")),
    })
}

// ---- config and remote ----------------------------------------------------------------------------

/// The part of git's key patterns the product uses: `^`, `$`, `.`, `\<c>`, a literal and `*` after an atom.
struct Pattern {
    start: bool,
    end: bool,
    atoms: Vec<(Option<char>, bool)>,
}

impl Pattern {
    fn parse(text: &str) -> Option<Pattern> {
        let mut chars: Vec<char> = text.chars().collect();
        let start = chars.first() == Some(&'^');
        if start {
            chars.remove(0);
        }
        let end = chars.last() == Some(&'$') && chars.len() >= 2 && chars[chars.len() - 2] != '\\' || chars == ['$'];
        if end {
            chars.pop();
        }
        let mut atoms: Vec<(Option<char>, bool)> = Vec::new();
        let mut it = chars.into_iter();
        while let Some(c) = it.next() {
            match c {
                '.' => atoms.push((None, false)),
                '\\' => atoms.push((Some(it.next()?), false)),
                '*' => atoms.last_mut()?.1 = true,
                '[' | ']' | '(' | ')' | '|' | '+' | '?' | '{' | '}' | '^' | '$' => return None,
                lit => atoms.push((Some(lit), false)),
            }
        }
        Some(Pattern { start, end, atoms })
    }
    fn is_match(&self, text: &str) -> bool {
        let text: Vec<char> = text.chars().collect();
        let starts = if self.start { 0..1 } else { 0..text.len() + 1 };
        starts.into_iter().any(|s| self.at(&self.atoms, &text[s.min(text.len())..]))
    }
    fn at(&self, atoms: &[(Option<char>, bool)], text: &[char]) -> bool {
        let Some(((want, star), rest)) = atoms.split_first() else { return !self.end || text.is_empty() };
        let fits = |c: &char| want.is_none_or(|w| w == *c);
        if *star {
            let run = text.iter().take_while(|c| fits(c)).count();
            (0..=run).rev().any(|n| self.at(rest, &text[n..]))
        } else {
            text.first().is_some_and(fits) && self.at(rest, &text[1..])
        }
    }
}

fn config(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("config", args, &["--get", "--get-regexp"], &[])?;
    let flags = flags(&toks);
    let pos = positionals(&toks);
    let [arg] = pos[..] else { return Err("config (arguments)".to_owned()) };
    let Some(repo) = c.repo() else { return Ok(Out::fail(128, NOT_A_REPO)) };
    let entries = c.u.config_entries(repo, &c.extra);
    if flags.contains(&"--get") {
        let key = norm_key(arg);
        return Ok(match entries.iter().rev().find(|(k, _)| *k == key) {
            Some((_, value)) => Out::ok(format!("{value}\n")),
            None => Out::silent(1),
        });
    }
    if flags.contains(&"--get-regexp") {
        let pattern = Pattern::parse(arg).ok_or_else(|| format!("config --get-regexp {arg}"))?;
        let lines: String =
            entries.iter().filter(|(k, _)| pattern.is_match(k)).map(|(k, v)| format!("{k} {v}\n")).collect();
        return Ok(if lines.is_empty() { Out::silent(1) } else { Out::ok(lines) });
    }
    Err(format!("config {arg}"))
}

/// The remotes of a call's config: `remote.<name>.url` entries, in order.
fn remote_urls(entries: &[(String, String)]) -> Vec<(String, String)> {
    entries
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix("remote.")?.strip_suffix(".url")?.to_owned(), v.clone())))
        .collect()
}

fn remote(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("remote", args, &[], &[])?;
    let pos = positionals(&toks);
    let Some(repo) = c.repo() else { return Ok(Out::fail(128, NOT_A_REPO)) };
    let remotes = remote_urls(&c.u.config_entries(repo, &c.extra));
    match pos[..] {
        [] => {
            let names: BTreeSet<&str> = remotes.iter().map(|(n, _)| n.as_str()).collect();
            Ok(Out::ok(names.into_iter().map(|n| format!("{n}\n")).collect::<String>()))
        }
        ["get-url", name] => Ok(match remotes.iter().find(|(n, _)| n == name) {
            Some((_, url)) => Out::ok(format!("{}\n", c.u.resolve_url_with(repo, url, &c.extra))),
            None => Out::fail(2, format!("error: No such remote '{name}'\n")),
        }),
        _ => Err(format!("remote {}", pos.join(" "))),
    }
}

// ---- cat-file, merge-base, rev-list, ls-tree, log -------------------------------------------------

fn cat_file(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("cat-file", args, &["-e"], &[])?;
    let (true, [spec]) = (flags(&toks).contains(&"-e"), &positionals(&toks)[..]) else {
        return Err("cat-file (arguments)".to_owned());
    };
    let Some(repo) = c.repo() else { return Ok(Out::fail(128, NOT_A_REPO)) };
    if is_oid(spec) {
        return Ok(Out::silent(if c.u.has(repo, &Oid::new(spec)) { 0 } else { 1 }));
    }
    Ok(match c.parse(spec) {
        Ok(_) => Out::silent(0),
        Err(_) => Out::fail(128, format!("fatal: Not a valid object name {spec}\n")),
    })
}

fn merge_base(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("merge-base", args, &["--is-ancestor"], &[])?;
    let (true, [a, b]) = (flags(&toks).contains(&"--is-ancestor"), &positionals(&toks)[..]) else {
        return Err("merge-base (arguments)".to_owned());
    };
    let Some(repo) = c.repo() else { return Ok(Out::fail(128, NOT_A_REPO)) };
    let mut ids = Vec::new();
    for spec in [a, b] {
        match c.parse(spec) {
            Ok(oid) => ids.push(oid),
            Err(_) if is_oid(spec) => return Ok(Out::fail(128, format!("fatal: Not a valid commit name {spec}\n"))),
            Err(_) => return Ok(Out::fail(128, format!("fatal: Not a valid object name {spec}\n"))),
        }
    }
    Ok(match c.u.is_ancestor(repo, &ids[0], &ids[1]) {
        Ok(yes) => Out::silent(if yes { 0 } else { 1 }),
        Err(unknown) => Out::fail(128, format!("fatal: Not a valid commit name {}\n", unknown.0)),
    })
}

fn rev_list(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("rev-list", args, &["--count", "--no-merges", "--not"], &["--remotes="])?;
    let Some(repo) = c.repo() else { return Ok(Out::fail(128, NOT_A_REPO)) };
    let mut negate = false;
    let (mut count, mut no_merges, mut any) = (false, false, false);
    let mut sel = Selection::default();
    for tok in &toks {
        match tok {
            Tok::Flag(f) if f == "--count" => count = true,
            Tok::Flag(f) if f == "--no-merges" => no_merges = true,
            Tok::Flag(f) if f == "--not" => negate = !negate,
            Tok::Flag(f) => {
                let tips = c.u.remote_tips(repo, f.strip_prefix("--remotes=").expect("only --remotes= is left"));
                any = true;
                if negate { sel.exclude.extend(tips) } else { sel.include.extend(tips) }
            }
            Tok::Pos(spec) => {
                any = true;
                let one = if spec.contains("..") {
                    rev::range(c.u, &c.cwd, spec)
                } else {
                    c.parse(&format!("{spec}^{{commit}}")).map(|oid| Selection { include: vec![oid], exclude: vec![] })
                };
                let Ok(one) = one else { return Ok(ambiguous(spec)) };
                let (inc, exc) = if negate { (one.exclude, one.include) } else { (one.include, one.exclude) };
                sel.include.extend(inc);
                sel.exclude.extend(exc);
            }
        }
    }
    if !any {
        return Ok(Out::fail(129, "usage: git rev-list [<options>] <commit>... [--] [<path>...]\n"));
    }
    let list = c.u.rev_list(repo, &sel, no_merges);
    Ok(Out::ok(if count { format!("{}\n", list.len()) } else { list.iter().map(|o| format!("{o}\n")).collect() }))
}

fn ls_tree(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("ls-tree", args, &["-r", "--name-only", "-z"], &[])?;
    let flags = flags(&toks);
    let [spec] = positionals(&toks)[..] else { return Err("ls-tree (arguments)".to_owned()) };
    if !flags.contains(&"-r") {
        return Err("ls-tree (without -r)".to_owned());
    }
    if c.repo().is_none() {
        return Ok(Out::fail(128, NOT_A_REPO));
    }
    let Ok(entries) = c.tree_of(spec) else { return Ok(Out::fail(128, format!("fatal: Not a valid object name {spec}\n"))) };
    let end = if flags.contains(&"-z") { '\0' } else { '\n' };
    Ok(Out::ok(
        entries
            .iter()
            .map(|(path, oid)| if flags.contains(&"--name-only") { format!("{path}{end}") } else { format!("100644 blob {oid}\t{path}{end}") })
            .collect::<String>(),
    ))
}

fn log(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("log", args, &["-g"], &["--format="])?;
    let flags = flags(&toks);
    if let Some(format) = flags.iter().find_map(|f| f.strip_prefix("--format=")) {
        if format != "%T" {
            return Err(format!("log --format={format}"));
        }
    }
    let ([spec], true, true) = (&positionals(&toks)[..], flags.contains(&"-g"), flags.contains(&"--format=%T")) else {
        return Err("log (arguments)".to_owned());
    };
    let Some(repo) = c.repo() else { return Ok(Out::fail(128, NOT_A_REPO)) };
    let name = if *spec == "HEAD" {
        match c.u.head(&c.cwd) {
            Head::Branch(b) => Some(format!("refs/heads/{b}")),
            Head::Detached(_) => None,
        }
    } else {
        dwim_ref(c.u, repo, spec)
    };
    let Some(name) = name else { return Ok(ambiguous(spec)) };
    Ok(Out::ok(c.u.reflog(repo, &name).iter().map(|e| format!("{}\n", e.tree)).collect::<String>()))
}

// ---- status, diff ----------------------------------------------------------------------------------

/// The paths of the index with a stage above 0, with the stages each has.
fn unmerged(index: &[(String, Oid, u8)]) -> BTreeMap<String, Vec<u8>> {
    let mut out: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (path, _, stage) in index.iter().filter(|(_, _, s)| *s > 0) {
        out.entry(path.clone()).or_default().push(*stage);
    }
    out
}

fn status(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("status", args, &["--porcelain"], &["--untracked-files="])?;
    let flags = flags(&toks);
    let mut untracked = true;
    for flag in &flags {
        match flag.strip_prefix("--untracked-files=") {
            Some("no") => untracked = false,
            Some("normal") => {}
            Some(_) => return Err(format!("status {flag}")),
            None => {}
        }
    }
    if !flags.contains(&"--porcelain") || !positionals(&toks).is_empty() {
        return Err("status (arguments)".to_owned());
    }
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    let head: BTreeMap<String, Oid> = c.u.head_tree(&wt).into_iter().collect();
    let index = c.u.effective_index(&wt);
    let merged: BTreeMap<&str, &Oid> = index.iter().filter(|(_, _, s)| *s == 0).map(|(p, o, _)| (p.as_str(), o)).collect();
    let conflicts = unmerged(&index);
    let files = work_files(&wt);

    let mut paths: BTreeSet<&str> = head.keys().map(String::as_str).collect();
    paths.extend(merged.keys().copied());
    paths.extend(conflicts.keys().map(String::as_str));
    let mut out = String::new();
    for path in paths {
        if let Some(stages) = conflicts.get(path) {
            let code = match stages[..] {
                [1, 2, 3] => "UU",
                [2, 3] => "AA",
                [1, 2] => "UD",
                [1, 3] => "DU",
                [2] => "AU",
                _ => "UA",
            };
            out.push_str(&format!("{code} {path}\n"));
            continue;
        }
        let x = match (head.get(path), merged.get(path)) {
            (Some(h), Some(i)) if h != *i => 'M',
            (None, Some(_)) => 'A',
            (Some(_), None) => 'D',
            _ => ' ',
        };
        let y = match (merged.get(path), files.get(path)) {
            (Some(_), None) => 'D',
            (Some(i), Some(file)) if file_blob(file).as_ref() != Some(*i) => 'M',
            _ => ' ',
        };
        if (x, y) != (' ', ' ') {
            out.push_str(&format!("{x}{y} {path}\n"));
        }
    }
    if untracked {
        let known: Vec<&str> = merged.keys().copied().chain(conflicts.keys().map(String::as_str)).collect();
        let mut lines = BTreeSet::new();
        for path in files.keys().filter(|p| !known.contains(&p.as_str())) {
            let parts: Vec<&str> = path.split('/').collect();
            let shown = (1..parts.len())
                .map(|k| format!("{}/", parts[..k].join("/")))
                .find(|dir| !known.iter().any(|t| t.starts_with(dir.as_str())))
                .unwrap_or_else(|| path.clone());
            lines.insert(shown);
        }
        out.extend(lines.into_iter().map(|l| format!("?? {l}\n")));
    }
    Ok(Out::ok(out))
}

/// The paths where the working tree differs from `tree`, among the paths of the tree and the index.
fn work_against(u: &Universe, wt: &Path, tree: &BTreeMap<String, Oid>) -> BTreeSet<String> {
    let index = u.effective_index(wt);
    let files = work_files(wt);
    let tracked: BTreeSet<&str> = index.iter().map(|(p, _, _)| p.as_str()).collect();
    let mut paths: BTreeSet<&str> = tree.keys().map(String::as_str).collect();
    paths.extend(tracked.iter().copied());
    paths
        .into_iter()
        .filter(|p| {
            let work = if tracked.contains(p) { files.get(*p).and_then(|f| file_blob(f)) } else { None };
            tree.get(*p) != work.as_ref()
        })
        .map(str::to_owned)
        .collect()
}

fn diff(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("diff", args, &["--quiet", "--name-only", "--no-renames", "-z"], &["--diff-filter="])?;
    let flags = flags(&toks);
    let mut only_unmerged = false;
    for flag in &flags {
        match flag.strip_prefix("--diff-filter=") {
            Some("U") => only_unmerged = true,
            Some(_) => return Err(format!("diff {flag}")),
            None => {}
        }
    }
    let quiet = flags.contains(&"--quiet");
    if !quiet && !flags.contains(&"--name-only") {
        return Err("diff (without --name-only or --quiet)".to_owned());
    }
    let pos = positionals(&toks);
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    let index = c.u.effective_index(&wt);
    let names: BTreeSet<String> = if only_unmerged {
        unmerged(&index).into_keys().collect()
    } else {
        match pos[..] {
            [] => {
                let files = work_files(&wt);
                let conflicts = unmerged(&index);
                let merged: BTreeMap<&str, &Oid> = index.iter().filter(|(_, _, s)| *s == 0).map(|(p, o, _)| (p.as_str(), o)).collect();
                conflicts
                    .into_keys()
                    .chain(merged.iter().filter(|(p, o)| files.get(**p).and_then(|f| file_blob(f)).as_ref() != Some(**o)).map(|(p, _)| (*p).to_owned()))
                    .collect()
            }
            [spec] if spec.contains("..") => {
                let (a, b, symmetric) = match spec.split_once("...") {
                    Some((a, b)) => (a, b, true),
                    None => {
                        let (a, b) = spec.split_once("..").expect("the spec has two dots");
                        (a, b, false)
                    }
                };
                let side = |s: &str| if s.is_empty() { "HEAD".to_owned() } else { s.to_owned() };
                let (Ok(a), Ok(b)) = (c.parse(&format!("{}^{{commit}}", side(a))), c.parse(&format!("{}^{{commit}}", side(b)))) else {
                    return Ok(ambiguous(spec));
                };
                let from = if symmetric {
                    let repo = c.repo().expect("a worktree has a repo");
                    match c.u.merge_base(repo, &a, &b) {
                        Some(base) => base,
                        None => return Ok(Out::fail(128, "fatal: no merge base\n")),
                    }
                } else {
                    a
                };
                let tree = |oid: &Oid| -> BTreeMap<String, Oid> {
                    let t = c.u.commit_data(oid).expect("a commit").tree;
                    c.u.tree_entries(&t).unwrap_or_default().into_iter().collect()
                };
                changed_between(&tree(&from), &tree(&b))
            }
            [spec] => match c.tree_of(spec) {
                Ok(tree) => work_against(c.u, &wt, &tree),
                Err(_) => return Ok(ambiguous(spec)),
            },
            [a, b] => match (c.tree_of(a), c.tree_of(b)) {
                (Ok(a), Ok(b)) => changed_between(&a, &b),
                (Err(_), _) => return Ok(ambiguous(a)),
                (_, Err(_)) => return Ok(ambiguous(b)),
            },
            _ => return Err("diff (more than two revisions)".to_owned()),
        }
    };
    if quiet {
        return Ok(Out::silent(if names.is_empty() { 0 } else { 1 }));
    }
    let end = if flags.contains(&"-z") { '\0' } else { '\n' };
    Ok(Out::ok(names.into_iter().map(|n| format!("{n}{end}")).collect::<String>()))
}

fn changed_between(a: &BTreeMap<String, Oid>, b: &BTreeMap<String, Oid>) -> BTreeSet<String> {
    a.keys().chain(b.keys()).filter(|p| a.get(*p) != b.get(*p)).cloned().collect()
}

// ---- ls-remote ---------------------------------------------------------------------------------------

fn ls_remote(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("ls-remote", args, &["--heads", "--exit-code"], &[])?;
    let flags = flags(&toks);
    let pos = positionals(&toks);
    let Some((remote, patterns)) = pos.split_first() else { return Err("ls-remote (arguments)".to_owned()) };
    let Some(repo) = c.repo() else { return Ok(Out::fail(128, NOT_A_REPO)) };
    let configured = remote_urls(&c.u.config_entries(repo, &c.extra)).into_iter().find(|(n, _)| n == remote).map(|(_, u)| u);
    let url = match configured {
        Some(url) => url,
        None if remote.contains('/') => (*remote).to_owned(),
        None => {
            return Ok(Out::fail(
                128,
                format!("fatal: '{remote}' does not appear to be a git repository\nfatal: Could not read from remote repository.\n"),
            ));
        }
    };
    let url = c.u.resolve_url_with(repo, &url, &c.extra);
    let Some(there) = c.u.repo_at(&url) else {
        return Ok(Out::fail(128, format!("fatal: unable to access '{url}': Could not resolve host\n")));
    };
    let mut refs: Vec<(String, Oid)> = Vec::new();
    if !flags.contains(&"--heads") {
        let head = match c.u.head(&c.u.git_dir(there)) {
            Head::Branch(b) => c.u.ref_oid(there, &format!("refs/heads/{b}")),
            Head::Detached(oid) => Some(oid),
        };
        refs.extend(head.map(|o| ("HEAD".to_owned(), o)));
    }
    refs.extend(c.u.refs(there, "refs/heads/"));
    if !flags.contains(&"--heads") {
        refs.extend(c.u.refs(there, "refs/tags/"));
    }
    let shown: Vec<&(String, Oid)> = refs
        .iter()
        .filter(|(name, _)| patterns.is_empty() || patterns.iter().any(|p| name == p || name.ends_with(&format!("/{p}"))))
        .collect();
    if shown.is_empty() && flags.contains(&"--exit-code") {
        return Ok(Out::silent(2));
    }
    Ok(Out::ok(shown.iter().map(|(n, o)| format!("{o}\t{n}\n")).collect::<String>()))
}
