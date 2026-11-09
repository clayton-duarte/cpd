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

mod remote;
use remote::{fetch, ls_remote, push};
mod merge;
use merge::{merge_tree, rebase};
mod fixture;

const ZEROS: &str = "0000000000000000000000000000000000000000";
use crate::host::runner::{Call, Handler, Reply};

pub struct GitRoute {
    universe: Arc<Mutex<Universe>>,
    unsupported: Mutex<Vec<String>>,
}

impl GitRoute {
    pub fn new(universe: Arc<Mutex<Universe>>) -> GitRoute {
        GitRoute {
            universe,
            unsupported: Mutex::new(Vec::new()),
        }
    }
    /// Every call the route refused as `fake-git: unsupported: ...`, as the text after that prefix
    /// (`status --bogus`, `nonsense`), in order.
    pub fn unsupported(&self) -> Vec<String> {
        self.unsupported
            .lock()
            .expect("the record is never held across a panic")
            .clone()
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
        Out {
            code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }
    fn silent(code: i32) -> Out {
        Out {
            code,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
    fn fail(code: i32, stderr: impl Into<String>) -> Out {
        Out {
            code,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }
}

/// `Err` is the text of an unsupported call.
type Run = Result<Out, String>;

const NOT_A_REPO: &str = "fatal: not a git repository (or any of the parent directories): .git\n";
const VERSION: &str = "git version 2.43.0\n";
const GLOBAL_KEYS: [&str; 6] = [
    "core.editor",
    "sequence.editor",
    "core.quotepath",
    "core.filesreflocktimeout",
    "core.packedrefstimeout",
    "commit.gpgsign",
];

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
            return Err(Out::fail(
                128,
                "fatal: this operation must be run in a work tree\n",
            ));
        }
        Ok(self
            .u
            .worktree_of(&self.cwd)
            .expect("a repo's path has a worktree"))
    }
    /// A revision of this call's worktree.
    fn parse(&self, spec: &str) -> Result<Oid, RevError> {
        rev::parse(self.u, &self.cwd, spec)
    }
    fn tree_of(&self, spec: &str) -> Result<BTreeMap<String, Oid>, RevError> {
        let tree = self.parse(&format!("{spec}^{{tree}}"))?;
        Ok(self
            .u
            .tree_entries(&tree)
            .unwrap_or_default()
            .into_iter()
            .collect())
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
    toks.iter()
        .filter_map(|t| {
            if let Tok::Flag(f) = t {
                Some(f.as_str())
            } else {
                None
            }
        })
        .collect()
}

fn positionals(toks: &[Tok]) -> Vec<&str> {
    toks.iter()
        .filter_map(|t| {
            if let Tok::Pos(p) = t {
                Some(p.as_str())
            } else {
                None
            }
        })
        .collect()
}

impl GitRoute {
    fn run(&self, call: &Call) -> Run {
        let mut cwd = call
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
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
                        return Ok(Out::fail(
                            128,
                            format!("fatal: cannot change to '{dir}': No such file or directory\n"),
                        ));
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
        let mut guard = self
            .universe
            .lock()
            .expect("the Universe's lock is never held across a panic");
        let mut ctx = Ctx {
            u: &mut guard,
            cwd,
            extra,
        };
        match sub {
            "rev-parse" => rev_parse(&mut ctx, rest),
            "symbolic-ref" => symbolic_ref(&mut ctx, rest),
            "config" => config(&mut ctx, rest),
            "remote" => remote(&mut ctx, rest),
            "cat-file" => cat_file(&mut ctx, rest),
            "merge-base" => merge_base(&mut ctx, rest),
            "merge-tree" => merge_tree(&mut ctx, rest),
            "commit-tree" => merge::commit_tree_cmd(&mut ctx, rest),
            "rebase" => rebase(&mut ctx, rest),
            "rev-list" => rev_list(&mut ctx, rest),
            "ls-tree" => ls_tree(&mut ctx, rest),
            "log" => log(&mut ctx, rest),
            "status" => status(&mut ctx, rest),
            "ls-files" => ls_files(&mut ctx, rest),
            "diff" => diff(&mut ctx, rest),
            "ls-remote" => ls_remote(&mut ctx, rest),
            "fetch" => fetch(&mut ctx, rest),
            "push" => push(&mut ctx, rest),
            "update-ref" => update_ref(&mut ctx, rest),
            "branch" => branch(&mut ctx, rest),
            "worktree" => worktree(&mut ctx, rest),
            "write-tree" => write_tree(&mut ctx, rest),
            "read-tree" => read_tree(&mut ctx, rest),
            "init" => fixture::init(&mut ctx, rest),
            "clone" => fixture::clone(&mut ctx, rest),
            "add" => fixture::add(&mut ctx, rest),
            "commit" => fixture::commit(&mut ctx, rest),
            "switch" | "checkout" => fixture::switch(&mut ctx, sub, rest),
            "reset" => fixture::reset(&mut ctx, rest),
            "clean" => fixture::clean(&mut ctx, rest),
            "merge" => fixture::merge(&mut ctx, rest),
            "tag" => fixture::tag(&mut ctx, rest),
            "mv" => fixture::mv(&mut ctx, rest),
            "rm" => fixture::rm(&mut ctx, rest),
            "show" => fixture::show(&mut ctx, rest),
            "for-each-ref" => fixture::for_each_ref(&mut ctx, rest),
            "grep" => fixture::grep(&mut ctx, rest),
            "diff-index" => fixture::diff_index(&mut ctx, rest),
            "diff-files" => fixture::diff_files(&mut ctx, rest),
            "update-index" => fixture::update_index(rest),
            other => Err(other.to_owned()),
        }
    }
}

impl Handler for GitRoute {
    fn handle(&self, call: &Call) -> Reply {
        match self.run(call) {
            Ok(out) => Reply::exit(out.code, &out.stdout, &out.stderr),
            Err(text) => {
                self.unsupported
                    .lock()
                    .expect("the record is never held across a panic")
                    .push(text.clone());
                Reply::exit(1, "", &format!("fake-git: unsupported: {text}\n"))
            }
        }
    }
}

/// The config a call carries in `GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_<n>` and `GIT_CONFIG_VALUE_<n>`.
fn env_config(call: &Call) -> Result<Vec<(String, String)>, String> {
    let env = |name: &str| {
        call.envs
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| v.clone())
    };
    let Some(count) = env("GIT_CONFIG_COUNT") else {
        return Ok(Vec::new());
    };
    let count: usize = count.parse().map_err(|_| "GIT_CONFIG_COUNT".to_owned())?;
    (0..count)
        .map(|n| {
            match (
                env(&format!("GIT_CONFIG_KEY_{n}")),
                env(&format!("GIT_CONFIG_VALUE_{n}")),
            ) {
                (Some(k), Some(v)) => Ok((k, v)),
                _ => Err(format!(
                    "GIT_CONFIG_COUNT {count} without key and value {n}"
                )),
            }
        })
        .collect()
}

// ---- rev-parse, symbolic-ref ---------------------------------------------------------------------

fn rev_parse(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan(
        "rev-parse",
        args,
        &[
            "--verify",
            "--quiet",
            "-q",
            "--end-of-options",
            "--short",
            "--git-common-dir",
            "--git-path",
        ],
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
            short = Some(
                n.parse::<usize>()
                    .map_err(|_| format!("rev-parse {flag}"))?
                    .clamp(4, 40),
            );
        } else if let Some(format) = flag.strip_prefix("--path-format=") {
            if format != "absolute" {
                return Err(format!("rev-parse {flag}"));
            }
            absolute = true;
        }
    }
    let pos = positionals(&toks);
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let show = |oid: &Oid| match short {
        Some(n) => oid.as_str()[..n].to_owned(),
        None => oid.to_string(),
    };

    if has("--git-common-dir") {
        let git_dir = c.u.git_dir(repo);
        let at_root =
            c.u.worktree_of(&c.cwd)
                .is_some_and(|wt| c.cwd.canonicalize().ok().as_deref() == Some(wt.as_path()));
        let main_root = !c.u.is_bare(repo)
            && at_root
            && c.u.worktrees(repo)[0].path == c.cwd.canonicalize().unwrap_or_default();
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
        let per_worktree = [
            "index",
            "rebase-merge",
            "rebase-apply",
            "HEAD",
            "ORIG_HEAD",
            "MERGE_HEAD",
        ];
        let mut out = String::new();
        for name in pos {
            let base = if per_worktree
                .iter()
                .any(|p| name == *p || name.starts_with(&format!("{p}/")))
            {
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
                return Ok(Out {
                    code: 128,
                    stdout: out,
                    stderr: ambiguous(spec).stderr,
                });
            }
        }
    }
    Ok(Out::ok(out))
}

/// `refs/heads/x` as `x`, `refs/remotes/o/x` as `o/x`.
fn shorten(name: &str) -> &str {
    ["refs/heads/", "refs/tags/", "refs/remotes/"]
        .iter()
        .find_map(|p| name.strip_prefix(p))
        .unwrap_or(name)
}

fn symbolic_ref(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("symbolic-ref", args, &["-q", "--quiet", "--short"], &[])?;
    let flags = flags(&toks);
    let (quiet, short) = (
        flags.contains(&"-q") || flags.contains(&"--quiet"),
        flags.contains(&"--short"),
    );
    let [name] = positionals(&toks)[..] else {
        return Err("symbolic-ref (arguments)".to_owned());
    };
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
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
        let end = chars.last() == Some(&'$') && chars.len() >= 2 && chars[chars.len() - 2] != '\\'
            || chars == ['$'];
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
        starts
            .into_iter()
            .any(|s| self.at(&self.atoms, &text[s.min(text.len())..]))
    }
    fn at(&self, atoms: &[(Option<char>, bool)], text: &[char]) -> bool {
        let Some(((want, star), rest)) = atoms.split_first() else {
            return !self.end || text.is_empty();
        };
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
    let toks = scan("config", args, &["--get", "--get-regexp", "--global"], &[])?;
    let flags = flags(&toks);
    let pos = positionals(&toks);
    let global = flags.contains(&"--global");
    let reading = flags.contains(&"--get") || flags.contains(&"--get-regexp");
    let entries = if global {
        c.u.global_entries()
    } else {
        let Some(repo) = c.repo() else {
            return Ok(Out::fail(128, NOT_A_REPO));
        };
        c.u.config_entries(repo, &c.extra)
    };
    if let (false, false, [key, value]) = (global, reading, &pos[..]) {
        let repo = c.repo().expect("the entries came from a repo");
        c.u.config_set(repo, key, value);
        return Ok(Out::silent(0));
    }
    let [arg] = pos[..] else {
        return Err("config (arguments)".to_owned());
    };
    if flags.contains(&"--get") {
        let key = norm_key(arg);
        return Ok(match entries.iter().rev().find(|(k, _)| *k == key) {
            Some((_, value)) => Out::ok(format!("{value}\n")),
            None => Out::silent(1),
        });
    }
    if flags.contains(&"--get-regexp") {
        let pattern = Pattern::parse(arg).ok_or_else(|| format!("config --get-regexp {arg}"))?;
        let lines: String = entries
            .iter()
            .filter(|(k, _)| pattern.is_match(k))
            .map(|(k, v)| format!("{k} {v}\n"))
            .collect();
        return Ok(if lines.is_empty() {
            Out::silent(1)
        } else {
            Out::ok(lines)
        });
    }
    Err(format!("config {arg}"))
}

/// The remotes of a call's config: `remote.<name>.url` entries, in order.
fn remote_urls(entries: &[(String, String)]) -> Vec<(String, String)> {
    entries
        .iter()
        .filter_map(|(k, v)| {
            Some((
                k.strip_prefix("remote.")?.strip_suffix(".url")?.to_owned(),
                v.clone(),
            ))
        })
        .collect()
}

fn remote(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("remote", args, &[], &[])?;
    let pos = positionals(&toks);
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let remotes = remote_urls(&c.u.config_entries(repo, &c.extra));
    match pos[..] {
        [] => {
            let names: BTreeSet<&str> = remotes.iter().map(|(n, _)| n.as_str()).collect();
            Ok(Out::ok(
                names
                    .into_iter()
                    .map(|n| format!("{n}\n"))
                    .collect::<String>(),
            ))
        }
        ["get-url", name] => Ok(match remotes.iter().find(|(n, _)| n == name) {
            Some((_, url)) => Out::ok(format!("{}\n", c.u.resolve_url_with(repo, url, &c.extra))),
            None => Out::fail(2, format!("error: No such remote '{name}'\n")),
        }),
        ["add", name, _] if remotes.iter().any(|(n, _)| n == name) => Ok(Out::fail(
            3,
            format!("error: remote {name} already exists.\n"),
        )),
        ["add", name, url] => {
            c.u.config_add(repo, &format!("remote.{name}.url"), url);
            c.u.config_add(
                repo,
                &format!("remote.{name}.fetch"),
                &format!("+refs/heads/*:refs/remotes/{name}/*"),
            );
            Ok(Out::ok(""))
        }
        ["set-url", name, url] if remotes.iter().any(|(n, _)| n == name) => {
            c.u.config_set(repo, &format!("remote.{name}.url"), url);
            Ok(Out::ok(""))
        }
        ["remove" | "rm", name] if remotes.iter().any(|(n, _)| n == name) => {
            let entries = c.u.config_entries(repo, &[]);
            let tracking: BTreeSet<String> = entries
                .iter()
                .filter(|(k, v)| k.starts_with("branch.") && k.ends_with(".remote") && v == name)
                .map(|(k, _)| k["branch.".len()..k.len() - ".remote".len()].to_owned())
                .collect();
            let own = format!("remote.{name}.");
            c.u.config_drop(repo, |k, _| {
                k.starts_with(&own)
                    || tracking.iter().any(|b| {
                        k == format!("branch.{b}.remote") || k == format!("branch.{b}.merge")
                    })
            });
            let prefix = format!("refs/remotes/{name}/");
            for (reference, _) in c.u.refs(repo, &prefix) {
                c.u.delete_ref(repo, &reference);
            }
            for (reference, _) in c.u.symbolic_refs(repo, &prefix) {
                c.u.delete_symbolic_ref(repo, &reference);
            }
            Ok(Out::ok(""))
        }
        ["set-url", name, _] => Ok(Out::fail(2, format!("error: No such remote '{name}'\n"))),
        ["remove" | "rm", name] => Ok(Out::fail(2, format!("error: No such remote: '{name}'\n"))),
        _ => Err(format!("remote {}", pos.join(" "))),
    }
}

// ---- cat-file, merge-base, rev-list, ls-tree, log -------------------------------------------------

fn cat_file(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("cat-file", args, &["-e"], &[])?;
    let (true, [spec]) = (flags(&toks).contains(&"-e"), &positionals(&toks)[..]) else {
        return Err("cat-file (arguments)".to_owned());
    };
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    if is_oid(spec) {
        return Ok(Out::silent(if c.u.has(repo, &Oid::new(spec)) {
            0
        } else {
            1
        }));
    }
    Ok(match c.parse(spec) {
        Ok(_) => Out::silent(0),
        Err(_) => Out::fail(128, format!("fatal: Not a valid object name {spec}\n")),
    })
}

fn merge_base(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("merge-base", args, &["--is-ancestor"], &[])?;
    let is_ancestor = flags(&toks).contains(&"--is-ancestor");
    let [a, b] = positionals(&toks)[..] else {
        return Err("merge-base (arguments)".to_owned());
    };
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let mut ids = Vec::new();
    for spec in [a, b] {
        match c.parse(spec) {
            Ok(oid) => ids.push(oid),
            Err(_) if is_oid(spec) => {
                return Ok(Out::fail(
                    128,
                    format!("fatal: Not a valid commit name {spec}\n"),
                ));
            }
            Err(_) => {
                return Ok(Out::fail(
                    128,
                    format!("fatal: Not a valid object name {spec}\n"),
                ));
            }
        }
    }
    if !is_ancestor {
        return Ok(match c.u.merge_base(repo, &ids[0], &ids[1]) {
            Some(base) => Out::ok(format!("{base}\n")),
            None => Out::silent(1),
        });
    }
    Ok(match c.u.is_ancestor(repo, &ids[0], &ids[1]) {
        Ok(yes) => Out::silent(if yes { 0 } else { 1 }),
        Err(unknown) => Out::fail(
            128,
            format!("fatal: Not a valid commit name {}\n", unknown.0),
        ),
    })
}

fn rev_list(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan(
        "rev-list",
        args,
        &["--count", "--no-merges", "--not"],
        &["--remotes="],
    )?;
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let mut negate = false;
    let (mut count, mut no_merges, mut any) = (false, false, false);
    let mut sel = Selection::default();
    for tok in &toks {
        match tok {
            Tok::Flag(f) if f == "--count" => count = true,
            Tok::Flag(f) if f == "--no-merges" => no_merges = true,
            Tok::Flag(f) if f == "--not" => negate = !negate,
            Tok::Flag(f) => {
                let tips = c.u.remote_tips(
                    repo,
                    f.strip_prefix("--remotes=")
                        .expect("only --remotes= is left"),
                );
                any = true;
                if negate {
                    sel.exclude.extend(tips)
                } else {
                    sel.include.extend(tips)
                }
            }
            Tok::Pos(spec) => {
                any = true;
                let one = if spec.contains("..") {
                    rev::range(c.u, &c.cwd, spec)
                } else {
                    c.parse(&format!("{spec}^{{commit}}")).map(|oid| Selection {
                        include: vec![oid],
                        exclude: vec![],
                    })
                };
                let Ok(one) = one else {
                    return Ok(ambiguous(spec));
                };
                let (inc, exc) = if negate {
                    (one.exclude, one.include)
                } else {
                    (one.include, one.exclude)
                };
                sel.include.extend(inc);
                sel.exclude.extend(exc);
            }
        }
    }
    if !any {
        return Ok(Out::fail(
            129,
            "usage: git rev-list [<options>] <commit>... [--] [<path>...]\n",
        ));
    }
    let list = c.u.rev_list(repo, &sel, no_merges);
    Ok(Out::ok(if count {
        format!("{}\n", list.len())
    } else {
        list.iter().map(|o| format!("{o}\n")).collect()
    }))
}

fn ls_tree(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("ls-tree", args, &["-r", "--name-only", "-z"], &[])?;
    let flags = flags(&toks);
    let [spec] = positionals(&toks)[..] else {
        return Err("ls-tree (arguments)".to_owned());
    };
    if !flags.contains(&"-r") {
        return Err("ls-tree (without -r)".to_owned());
    }
    if c.repo().is_none() {
        return Ok(Out::fail(128, NOT_A_REPO));
    }
    let Ok(entries) = c.tree_of(spec) else {
        return Ok(Out::fail(
            128,
            format!("fatal: Not a valid object name {spec}\n"),
        ));
    };
    let end = if flags.contains(&"-z") { '\0' } else { '\n' };
    Ok(Out::ok(
        entries
            .iter()
            .map(|(path, oid)| {
                if flags.contains(&"--name-only") {
                    format!("{path}{end}")
                } else {
                    format!("100644 blob {oid}\t{path}{end}")
                }
            })
            .collect::<String>(),
    ))
}

/// `git log [-1] --format=%s [<rev>]`: the subject of each commit reachable from the rev (`HEAD` by default), newest first.
fn log_subjects(c: &mut Ctx, toks: &[Tok]) -> Run {
    let flags = flags(toks);
    if flags.iter().find_map(|f| f.strip_prefix("--format=")) != Some("%s") {
        return Err("log (arguments)".to_owned());
    }
    let spec = match positionals(toks)[..] {
        [] => "HEAD",
        [spec] => spec,
        _ => return Err("log (arguments)".to_owned()),
    };
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let Ok(tip) = c.parse(&format!("{spec}^{{commit}}")) else {
        return Ok(ambiguous(spec));
    };
    let mut commits = c.u.rev_list(
        repo,
        &Selection {
            include: vec![tip],
            exclude: vec![],
        },
        false,
    );
    if flags.contains(&"-1") {
        commits.truncate(1);
    }
    let subjects = commits
        .iter()
        .filter_map(|oid| c.u.commit_data(oid))
        .map(|commit| format!("{}\n", commit.message.lines().next().unwrap_or("")));
    Ok(Out::ok(subjects.collect::<String>()))
}

fn log(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("log", args, &["-g", "-1"], &["--format="])?;
    let flags = flags(&toks);
    if !flags.contains(&"-g") {
        return log_subjects(c, &toks);
    }
    if let Some(format) = flags.iter().find_map(|f| f.strip_prefix("--format="))
        && format != "%T"
    {
        return Err(format!("log --format={format}"));
    }
    let ([spec], true, true) = (
        &positionals(&toks)[..],
        flags.contains(&"-g"),
        flags.contains(&"--format=%T"),
    ) else {
        return Err("log (arguments)".to_owned());
    };
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let name = if *spec == "HEAD" {
        match c.u.head(&c.cwd) {
            Head::Branch(b) => Some(format!("refs/heads/{b}")),
            Head::Detached(_) => None,
        }
    } else {
        dwim_ref(c.u, repo, spec)
    };
    let Some(name) = name else {
        return Ok(ambiguous(spec));
    };
    Ok(Out::ok(
        c.u.reflog(repo, &name)
            .iter()
            .map(|e| format!("{}\n", e.tree))
            .collect::<String>(),
    ))
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

/// What `status` and `ls-files --others` show of the files no index entry tracks: a directory with nothing tracked
/// in it once, as `dir/`, and every other file by its path; `.gitignore` rules (see `fixture::Ignore`) apply.
fn untracked_entries(
    wt: &Path,
    files: &BTreeMap<String, PathBuf>,
    known: &[&str],
    sub: &str,
) -> Result<BTreeSet<String>, String> {
    let ignore = fixture::ignore_of(wt, files, sub)?;
    let mut found = BTreeSet::new();
    for path in files
        .keys()
        .filter(|p| !known.contains(&p.as_str()) && !ignore.ignores(p, false))
    {
        let parts: Vec<&str> = path.split('/').collect();
        let shown = (1..parts.len())
            .map(|k| format!("{}/", parts[..k].join("/")))
            .find(|dir| !known.iter().any(|t| t.starts_with(dir.as_str())))
            .unwrap_or_else(|| path.clone());
        found.insert(shown);
    }
    Ok(found)
}

/// `ls-files --others --exclude-standard --directory [-z]`: the untracked entries `status` shows, one per line or
/// NUL-terminated, unquoted. Any other form is unsupported.
fn ls_files(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan(
        "ls-files",
        args,
        &["--others", "--exclude-standard", "--directory", "-z"],
        &[],
    )?;
    let flags = flags(&toks);
    for required in ["--others", "--exclude-standard", "--directory"] {
        if !flags.contains(&required) {
            return Err(format!("ls-files (without {required})"));
        }
    }
    if !positionals(&toks).is_empty() {
        return Err("ls-files (arguments)".to_owned());
    }
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    let index = c.u.effective_index(&wt);
    let known: Vec<&str> = index.iter().map(|(p, _, _)| p.as_str()).collect();
    let end = if flags.contains(&"-z") { '\0' } else { '\n' };
    let files = work_files(&wt);
    Ok(Out::ok(
        untracked_entries(&wt, &files, &known, "ls-files")?
            .into_iter()
            .map(|e| format!("{e}{end}"))
            .collect::<String>(),
    ))
}

fn status(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan(
        "status",
        args,
        &["--porcelain", "-z"],
        &["--untracked-files="],
    )?;
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
    let merged: BTreeMap<&str, &Oid> = index
        .iter()
        .filter(|(_, _, s)| *s == 0)
        .map(|(p, o, _)| (p.as_str(), o))
        .collect();
    let conflicts = unmerged(&index);
    let files = work_files(&wt);

    let mut paths: BTreeSet<&str> = head.keys().map(String::as_str).collect();
    paths.extend(merged.keys().copied());
    paths.extend(conflicts.keys().map(String::as_str));
    let mut lines: Vec<(String, String)> = Vec::new();
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
            lines.push((code.to_owned(), path.to_owned()));
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
            lines.push((format!("{x}{y}"), path.to_owned()));
        }
    }
    if untracked {
        let known: Vec<&str> = merged
            .keys()
            .copied()
            .chain(conflicts.keys().map(String::as_str))
            .collect();
        lines.extend(
            untracked_entries(&wt, &files, &known, "status")?
                .into_iter()
                .map(|l| ("??".to_owned(), l)),
        );
    }
    let z = flags.contains(&"-z");
    Ok(Out::ok(
        lines
            .into_iter()
            .map(|(code, path)| {
                if z {
                    format!("{code} {path}\0")
                } else {
                    format!("{code} {}\n", quoted(&path))
                }
            })
            .collect::<String>(),
    ))
}

/// A path as `status --porcelain` shows it: in double quotes, with C escapes, when it has a space, a quote, a
/// backslash, a control character or a byte above ASCII.
fn quoted(path: &str) -> String {
    if !path
        .chars()
        .any(|c| c == ' ' || c == '"' || c == '\\' || c.is_control() || !c.is_ascii())
    {
        return path.to_owned();
    }
    let mut out = String::from("\"");
    for byte in path.bytes() {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(byte as char),
            other => out.push_str(&format!("\\{other:03o}")),
        }
    }
    out.push('"');
    out
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
            let work = if tracked.contains(p) {
                files.get(*p).and_then(|f| file_blob(f))
            } else {
                None
            };
            tree.get(*p) != work.as_ref()
        })
        .map(str::to_owned)
        .collect()
}

fn diff(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan(
        "diff",
        args,
        &["--quiet", "--name-only", "--no-renames", "-z", "--cached"],
        &["--diff-filter="],
    )?;
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
    } else if flags.contains(&"--cached") {
        if !pos.is_empty() {
            return Err("diff --cached (with a revision)".to_owned());
        }
        let head: BTreeMap<String, Oid> = c.u.head_tree(&wt).into_iter().collect();
        let staged: BTreeMap<String, Oid> = index
            .iter()
            .filter(|(_, _, s)| *s == 0)
            .map(|(p, o, _)| (p.clone(), o.clone()))
            .collect();
        changed_between(&head, &staged)
            .into_iter()
            .chain(unmerged(&index).into_keys())
            .collect()
    } else {
        match pos[..] {
            [] => {
                let files = work_files(&wt);
                let conflicts = unmerged(&index);
                let merged: BTreeMap<&str, &Oid> = index
                    .iter()
                    .filter(|(_, _, s)| *s == 0)
                    .map(|(p, o, _)| (p.as_str(), o))
                    .collect();
                conflicts
                    .into_keys()
                    .chain(
                        merged
                            .iter()
                            .filter(|(p, o)| {
                                files.get(**p).and_then(|f| file_blob(f)).as_ref() != Some(**o)
                            })
                            .map(|(p, _)| (*p).to_owned()),
                    )
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
                let side = |s: &str| {
                    if s.is_empty() {
                        "HEAD".to_owned()
                    } else {
                        s.to_owned()
                    }
                };
                let (Ok(a), Ok(b)) = (
                    c.parse(&format!("{}^{{commit}}", side(a))),
                    c.parse(&format!("{}^{{commit}}", side(b))),
                ) else {
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
                    c.u.tree_entries(&t)
                        .unwrap_or_default()
                        .into_iter()
                        .collect()
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
    Ok(Out::ok(
        names
            .into_iter()
            .map(|n| format!("{n}{end}"))
            .collect::<String>(),
    ))
}

fn changed_between(a: &BTreeMap<String, Oid>, b: &BTreeMap<String, Oid>) -> BTreeSet<String> {
    a.keys()
        .chain(b.keys())
        .filter(|p| a.get(*p) != b.get(*p))
        .cloned()
        .collect()
}

// ---- ls-remote ---------------------------------------------------------------------------------------

// ---- update-ref, branch, worktree, write-tree, read-tree -------------------------------------------

fn update_ref(c: &mut Ctx, args: &[String]) -> Run {
    let mut message = "";
    let mut pos: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == "-m" {
            message = it.next().ok_or("update-ref -m")?;
        } else if arg.starts_with('-') {
            return Err(format!("update-ref {arg}"));
        } else {
            pos.push(arg);
        }
    }
    let (name, new, old) = match pos[..] {
        [name, new] => (name, new, None),
        [name, new, old] => (name, new, Some(old)),
        _ => return Err("update-ref (arguments)".to_owned()),
    };
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    if !name.starts_with("refs/") || new == ZEROS {
        return Err(format!("update-ref {name} {new}"));
    }
    let refused = |why: String| {
        Ok(Out::fail(
            128,
            format!("fatal: update_ref failed for ref '{name}': {why}\n"),
        ))
    };
    let Ok(new_oid) = c.parse(new) else {
        return refused(format!(
            "cannot update ref '{name}': trying to write ref '{name}' with nonexistent object {new}"
        ));
    };
    let current = c.u.ref_oid(repo, name);
    if let Some(old) = old {
        let lock = |why: String| refused(format!("cannot lock ref '{name}': {why}"));
        if old == ZEROS {
            if current.is_some() {
                return lock("reference already exists".to_owned());
            }
        } else {
            let Ok(expected) = c.parse(old) else {
                return Ok(Out::fail(128, format!("fatal: {old}: not a valid SHA1\n")));
            };
            match &current {
                None => return lock(format!("unable to resolve reference '{name}'")),
                Some(at) if *at != expected => {
                    return lock(format!("is at {at} but expected {expected}"));
                }
                Some(_) => {}
            }
        }
    }
    c.u.set_ref(repo, name, &new_oid, message);
    Ok(Out::ok(""))
}

fn branch(c: &mut Ctx, args: &[String]) -> Run {
    let (mut rename, mut delete, mut force, mut list, mut remotes, mut current) =
        (false, false, false, false, false, false);
    let mut format: Option<&str> = None;
    let mut pos: Vec<&str> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-m" => rename = true,
            "-d" => delete = true,
            "-D" => (delete, force) = (true, true),
            "--list" => list = true,
            "-r" => remotes = true,
            "--show-current" => current = true,
            flag if flag.starts_with("--format=") => format = Some(&flag["--format=".len()..]),
            flag if flag.starts_with('-') => return Err(format!("branch {flag}")),
            name => pos.push(name),
        }
    }
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let modes = [rename, delete, list || remotes || format.is_some(), current];
    match (modes, &pos[..]) {
        ([true, false, false, false], [old, new]) => {
            if c.u.ref_oid(repo, &format!("refs/heads/{old}")).is_none() {
                return Ok(Out::fail(128, format!("fatal: no branch named '{old}'\n")));
            }
            if c.u.ref_oid(repo, &format!("refs/heads/{new}")).is_some() {
                return Ok(Out::fail(
                    128,
                    format!("fatal: a branch named '{new}' already exists\n"),
                ));
            }
            c.u.rename_branch(repo, old, new);
            Ok(Out::ok(""))
        }
        ([false, true, false, false], names) if !names.is_empty() => {
            let (mut stdout, mut stderr, mut code) = (String::new(), String::new(), 0);
            for name in names {
                match delete_branch(c, repo, name, force) {
                    Ok(done) => stdout.push_str(&done),
                    Err(why) => (stderr, code) = (stderr + &why, 1),
                }
            }
            Ok(Out {
                code,
                stdout,
                stderr,
            })
        }
        ([false, false, false, true], []) => {
            let wt = match c.worktree() {
                Ok(wt) => wt,
                Err(out) => return Ok(out),
            };
            Ok(Out::ok(match c.u.head(&wt) {
                Head::Branch(name) => format!("{name}\n"),
                Head::Detached(_) => String::new(),
            }))
        }
        ([false, false, true, false], patterns) | ([false, false, false, false], patterns @ []) => {
            fixture::list_branches(c, repo, remotes, format, patterns)
        }
        ([false, false, false, false], [name]) => fixture::create_branch(c, repo, name, None),
        ([false, false, false, false], [name, start]) => {
            fixture::create_branch(c, repo, name, Some(start))
        }
        _ => Err("branch (arguments)".to_owned()),
    }
}

/// `branch -d` / `-D` of one branch: what git prints, or its `error:` line.
fn delete_branch(c: &mut Ctx, repo: RepoId, name: &str, force: bool) -> Result<String, String> {
    let full = format!("refs/heads/{name}");
    let Some(tip) = c.u.ref_oid(repo, &full) else {
        return Err(format!("error: branch '{name}' not found.\n"));
    };
    let wts = c.u.worktrees(repo);
    if let Some(wt) = wts
        .iter()
        .find(|w| c.u.head(&w.path) == Head::Branch(name.to_owned()))
    {
        return Err(format!(
            "error: Cannot delete branch '{name}' checked out at '{}'\n",
            wt.path.display()
        ));
    }
    if !force {
        let head = c.parse("HEAD").ok();
        if !head.is_some_and(|h| c.u.is_ancestor(repo, &tip, &h).unwrap_or(false)) {
            return Err(format!(
                "error: the branch '{name}' is not fully merged.\nIf you are sure you want to delete it, run 'git branch -D {name}'.\n"
            ));
        }
    }
    c.u.delete_ref(repo, &full);
    Ok(format!(
        "Deleted branch {name} (was {}).\n",
        &tip.as_str()[..7]
    ))
}

fn worktree(c: &mut Ctx, args: &[String]) -> Run {
    let Some((sub, rest)) = args.split_first() else {
        return Err("worktree".to_owned());
    };
    match sub.as_str() {
        "add" => worktree_add(c, rest),
        "list" => worktree_list(c, rest),
        "remove" => worktree_remove(c, rest),
        "prune" => {
            scan("worktree prune", rest, &[], &[])?;
            worktree_prune(c)
        }
        other => Err(format!("worktree {other}")),
    }
}

/// `path` as the call gave it, made absolute against the call's directory.
fn given(c: &Ctx, path: &str) -> PathBuf {
    c.cwd.join(path)
}

fn worktree_add(c: &mut Ctx, args: &[String]) -> Run {
    let (mut branch, mut reason): (Option<&str>, Option<&str>) = (None, None);
    let (mut detach, mut lock, mut quiet) = (false, false, false);
    let mut pos: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--no-track" => {}
            "-q" => quiet = true,
            "--detach" => detach = true,
            "--lock" => lock = true,
            "-b" => branch = Some(it.next().ok_or("worktree add -b")?),
            "--reason" => reason = Some(it.next().ok_or("worktree add --reason")?),
            flag if flag.starts_with('-') => return Err(format!("worktree add {flag}")),
            other => pos.push(other),
        }
    }
    if branch.is_some() == detach {
        return Err("worktree add (without exactly one of -b and --detach)".to_owned());
    }
    let (path, start) = match pos[..] {
        [path] => (path, "HEAD"),
        [path, start] => (path, start),
        _ => return Err("worktree add (arguments)".to_owned()),
    };
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let dir = given(c, path);
    if std::fs::read_dir(&dir).is_ok_and(|mut entries| entries.next().is_some()) || dir.is_file() {
        return Ok(Out::fail(
            128,
            format!("fatal: '{}' already exists\n", dir.display()),
        ));
    }
    if let Some(name) = branch
        && c.u.ref_oid(repo, &format!("refs/heads/{name}")).is_some()
    {
        return Ok(Out::fail(
            128,
            format!("fatal: a branch named '{name}' already exists\n"),
        ));
    }
    let Ok(commit) = c.parse(&format!("{start}^{{commit}}")) else {
        return Ok(Out::fail(
            128,
            format!("fatal: invalid reference: {start}\n"),
        ));
    };
    let data = c.u.commit_data(&commit).expect("a parsed commit has data");
    let entries = c.u.tree_entries(&data.tree).unwrap_or_default();
    let head = match branch {
        Some(name) => {
            c.u.set_ref(
                repo,
                &format!("refs/heads/{name}"),
                &commit,
                &format!("branch: Created from {start}"),
            );
            Head::Branch(name.to_owned())
        }
        None => Head::Detached(commit.clone()),
    };
    let wt = c.u.add_worktree(repo, &dir, head);
    write_files(c.u, &wt.path, &entries);
    c.u.set_index(
        &wt.path,
        entries.into_iter().map(|(p, o)| (p, o, 0)).collect(),
    );
    if lock {
        c.u.lock_worktree(&wt.path, reason.unwrap_or(""));
    }
    let what = match branch {
        Some(name) => format!("new branch '{name}'"),
        None => format!("detached HEAD {}", &commit.as_str()[..7]),
    };
    let first = data.message.lines().next().unwrap_or("");
    if quiet {
        return Ok(Out::ok(""));
    }
    Ok(Out {
        code: 0,
        stdout: format!("HEAD is now at {} {first}\n", &commit.as_str()[..7]),
        stderr: format!("Preparing worktree ({what})\n"),
    })
}

/// Writes the blobs of `entries` as files under `root` (parents made).
fn write_files(u: &Universe, root: &Path, entries: &[(String, Oid)]) {
    for (path, blob) in entries {
        write_blob(u, root, path, blob);
    }
}

fn write_blob(u: &Universe, root: &Path, path: &str, blob: &Oid) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().expect("a file has a parent"))
        .expect("the parents can be made");
    std::fs::write(file, u.blob_bytes(blob).unwrap_or_default()).expect("the file can be written");
}

fn worktree_list(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("worktree list", args, &["--porcelain"], &[])?;
    if !flags(&toks).contains(&"--porcelain") || !positionals(&toks).is_empty() {
        return Err("worktree list (arguments)".to_owned());
    }
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    if c.u.is_bare(repo) {
        return Ok(Out::ok(format!(
            "worktree {}\nbare\n\n",
            c.u.git_dir(repo).display()
        )));
    }
    let mut out = String::new();
    let mut wts = c.u.worktrees(repo);
    // The main worktree first, then the linked ones by the name of their admin directory.
    if let Some((_, linked)) = wts.split_first_mut() {
        linked.sort_by(|a, b| a.id.cmp(&b.id));
    }
    for (i, wt) in wts.iter().enumerate() {
        let head = c.u.head(&wt.path);
        let oid = match &head {
            Head::Branch(b) => c.u.ref_oid(repo, &format!("refs/heads/{b}")),
            Head::Detached(oid) => Some(oid.clone()),
        };
        out.push_str(&format!(
            "worktree {}\nHEAD {}\n",
            wt.path.display(),
            oid.map_or_else(|| ZEROS.to_owned(), |o| o.to_string())
        ));
        match head {
            Head::Branch(b) => out.push_str(&format!("branch refs/heads/{b}\n")),
            Head::Detached(_) => out.push_str("detached\n"),
        }
        match c.u.lock_reason(&wt.path) {
            Some(reason) if reason.is_empty() => out.push_str("locked\n"),
            Some(reason) => out.push_str(&format!("locked {reason}\n")),
            None if i > 0 && !wt.path.exists() => {
                out.push_str("prunable gitdir file points to non-existent location\n")
            }
            None => {}
        }
        out.push('\n');
    }
    Ok(Out::ok(out))
}

fn worktree_remove(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("worktree remove", args, &["--force", "-f"], &[])?;
    let force = flags(&toks).len();
    let [path] = positionals(&toks)[..] else {
        return Err("worktree remove (arguments)".to_owned());
    };
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let dir = given(c, path);
    let canonical = dir.canonicalize().ok();
    let found =
        c.u.worktrees(repo)
            .into_iter()
            .find(|w| w.path == dir || Some(&w.path) == canonical.as_ref());
    let Some(wt) = found else {
        return Ok(Out::fail(
            128,
            format!("fatal: '{path}' is not a working tree\n"),
        ));
    };
    if wt.id.is_none() {
        return Ok(Out::fail(
            128,
            format!("fatal: '{path}' is a main working tree\n"),
        ));
    }
    if let Some(reason) = c.u.lock_reason(&wt.path).filter(|_| force < 2) {
        let why = if reason.is_empty() {
            ";".to_owned()
        } else {
            format!(", lock reason: {reason}")
        };
        return Ok(Out::fail(
            128,
            format!(
                "fatal: cannot remove a locked working tree{why}\nuse 'remove -f -f' to override or unlock first\n"
            ),
        ));
    }
    if force == 0 {
        let mut inside = Ctx {
            u: &mut *c.u,
            cwd: wt.path.clone(),
            extra: Vec::new(),
        };
        let dirty = status(&mut inside, &["--porcelain".to_owned()])?;
        if !dirty.stdout.is_empty() {
            return Ok(Out::fail(
                128,
                format!(
                    "fatal: '{path}' contains modified or untracked files, use --force to delete it\n"
                ),
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&wt.path);
    c.u.forget_worktree(&wt.path);
    Ok(Out::ok(""))
}

/// Forgets the unlocked linked worktrees whose directory is gone.
fn worktree_prune(c: &mut Ctx) -> Run {
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    for wt in c.u.worktrees(repo) {
        if wt.id.is_some() && !wt.path.exists() && c.u.lock_reason(&wt.path).is_none() {
            c.u.forget_worktree(&wt.path);
        }
    }
    Ok(Out::ok(""))
}

/// The refusal of a call that needs the index while `index.lock` exists.
fn index_lock(u: &Universe, wt: &Path) -> Option<Out> {
    let mut lock = u.index_path(wt).into_os_string();
    lock.push(".lock");
    Path::new(&lock).exists().then(|| {
        Out::fail(
            128,
            format!(
                "fatal: Unable to create '{}': File exists.\n\nAnother git process seems to be running in this repository, e.g.\nan editor opened by 'git commit'. Please make sure all processes\nare terminated then try again. If it still fails, a git process\nmay have crashed in this repository earlier:\nremove the file manually to continue.\n",
                Path::new(&lock).display()
            ),
        )
    })
}

fn write_tree(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("write-tree", args, &[], &[])?;
    if !positionals(&toks).is_empty() {
        return Err("write-tree (arguments)".to_owned());
    }
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    if let Some(busy) = index_lock(c.u, &wt) {
        return Ok(busy);
    }
    let repo = c.repo().expect("a worktree has a repo");
    let index = c.u.effective_index(&wt);
    let conflicts = unmerged(&index);
    if !conflicts.is_empty() {
        let mut stderr = String::new();
        for (path, oid, _) in index.iter().filter(|(_, _, s)| *s > 0) {
            stderr.push_str(&format!("error: {path}: unmerged ({oid})\n"));
        }
        stderr.push_str("fatal: git-write-tree: error building trees\n");
        return Ok(Out::fail(128, stderr));
    }
    let entries: Vec<(&str, &Oid)> = index.iter().map(|(p, o, _)| (p.as_str(), o)).collect();
    Ok(Out::ok(format!("{}\n", c.u.tree(repo, &entries))))
}

/// `read-tree -m -u <from> <to>`: the two-way merge of git's `read-tree` documentation, then the index and
/// the working tree; nothing is touched when a path cannot move.
fn read_tree(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("read-tree", args, &["-m", "-u"], &[])?;
    let flags = flags(&toks);
    let ([from, to], true, true) = (
        &positionals(&toks)[..],
        flags.contains(&"-m"),
        flags.contains(&"-u"),
    ) else {
        return Err("read-tree (arguments)".to_owned());
    };
    let wt = match c.worktree() {
        Ok(wt) => wt,
        Err(out) => return Ok(out),
    };
    if let Some(busy) = index_lock(c.u, &wt) {
        return Ok(busy);
    }
    let mut trees = Vec::new();
    for spec in [from, to] {
        match c.tree_of(spec) {
            Ok(tree) => trees.push(tree),
            Err(_) => {
                return Ok(Out::fail(
                    128,
                    format!("fatal: failed to unpack tree object {spec}\n"),
                ));
            }
        }
    }
    let index = c.u.effective_index(&wt);
    if index.iter().any(|(_, _, stage)| *stage > 0) {
        return Ok(Out::fail(
            128,
            "error: you need to resolve your current index first\n",
        ));
    }
    let current: BTreeMap<String, Oid> = index.into_iter().map(|(p, o, _)| (p, o)).collect();
    let files = work_files(&wt);
    let (head, merge) = (&trees[0], &trees[1]);
    let paths: BTreeSet<&String> = head
        .keys()
        .chain(merge.keys())
        .chain(current.keys())
        .collect();
    let mut result: BTreeMap<String, Oid> = BTreeMap::new();
    let mut moves: Vec<(&String, Option<&Oid>)> = Vec::new();
    for path in paths {
        let (h, m, i) = (head.get(path), merge.get(path), current.get(path));
        let next = if i == h {
            m
        } else if h == m || i == m {
            i
        } else {
            return Ok(Out::fail(
                128,
                format!("error: Entry '{path}' would be overwritten by merge. Cannot merge.\n"),
            ));
        };
        if next != i {
            let on_disk = files.get(path.as_str()).map(|f| file_blob(f));
            match (i, on_disk) {
                (Some(i), Some(disk)) if disk.as_ref() != Some(i) => {
                    return Ok(Out::fail(
                        128,
                        format!("error: Entry '{path}' not uptodate. Cannot merge.\n"),
                    ));
                }
                (None, Some(_)) => {
                    return Ok(Out::fail(
                        128,
                        format!(
                            "error: Untracked working tree file '{path}' would be overwritten by merge.\n"
                        ),
                    ));
                }
                _ => {}
            }
            moves.push((path, next));
        }
        if let Some(next) = next {
            result.insert(path.clone(), next.clone());
        }
    }
    for (path, next) in moves {
        match next {
            Some(blob) => write_blob(c.u, &wt, path, blob),
            None => remove_file(&wt, path),
        }
    }
    c.u.set_index(&wt, result.into_iter().map(|(p, o)| (p, o, 0)).collect());
    Ok(Out::ok(""))
}

/// Deletes the file and the directories it leaves empty, up to `root`.
fn remove_file(root: &Path, path: &str) {
    let mut file = root.join(path);
    let _ = std::fs::remove_file(&file);
    while file.pop() && file != root && std::fs::remove_dir(&file).is_ok() {}
}
