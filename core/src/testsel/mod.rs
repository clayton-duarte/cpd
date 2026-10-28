//! `cpd test changed` and `cpd test run` (commands.md §26): the logic of `scripts/test-changed` and of
//! `scripts/tests.sh`, over [`Ports`]. The selection and the classification of a test name are pure over the text
//! of `git diff` and `cargo metadata`, which the [`Runner`](crate::host::runner::Runner) returns; the commands start
//! `cargo` through `scripts/cargo.sh`, and `node` for pi tests.
//!
//! A process's output is read when it ends and then forwarded: the `Runner` returns it whole.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Output;

use clap::{Args, Subcommand};
use serde_json::Value;

use crate::error::{CpdError, ErrorCode};
use crate::host::runner::{Call, NO_DEADLINE};
use crate::ports::Ports;

/// `cpd test` (commands.md §26).
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the Rust tests the changed files can reach (§26.1)
    Changed(ChangedArgs),
    /// Run the named tests (§26.2)
    Run(RunArgs),
    /// Exit 0 only when that one test exists (§26.3)
    List(ListArgs),
    /// Compile the Rust tests and typecheck the TypeScript (§26.3)
    Build,
}

/// `cpd test changed [--dry-run] [<base>]` (commands.md §26.1).
#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct ChangedArgs {
    /// Print the one command it would run, and run nothing
    #[arg(long)]
    pub dry_run: bool,
    /// The git revision the changes are counted from (default `origin/main`)
    pub base: Option<String>,
}

/// `cpd test list <name>` (commands.md §26.3).
#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct ListArgs {
    /// A Rust name as nextest filters it, or a pi name `pi/test/<file>.test.ts::<title>`
    #[arg(value_name = "NAME")]
    pub name: String,
}

/// `cpd test run <name>...` (commands.md §26.2).
#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct RunArgs {
    /// A Rust name as nextest filters it, or a pi name `pi/test/<file>.test.ts::<title>`
    #[arg(required = true, value_name = "NAME")]
    pub names: Vec<String>,
}

/// The bootstrap that finds cargo (and the test slot), run from the repo root.
const CARGO: &str = "scripts/cargo.sh";

/// `cpd test <command>` on the real standard streams, from the current directory; the exit status of the process.
pub fn exec(ports: &Ports, command: Command) -> Result<u8, CpdError> {
    let cwd = std::env::current_dir().map_err(|e| internal(format!("cpd test: cannot read the current directory: {e}.")))?;
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    match command {
        Command::Changed(args) => changed(ports, &cwd, &args, &mut out, &mut err),
        Command::Run(args) => run(ports, &cwd, &args.names, &mut out, &mut err),
        Command::List(args) => list(ports, &cwd, &args.name, &mut err),
        Command::Build => build(ports, &cwd, &mut out, &mut err),
    }
}

/// `cpd test changed` (§26.1) from `cwd`: the command goes to `out` (a dry run) or runs, its output forwarded to `out`
/// and `err`; `test-changed: nothing to run` goes to `err`. The exit status: 0 for a dry run and for nothing selected,
/// nextest's otherwise; a refusal is an `Err` whose exit code is the status.
pub fn changed(ports: &Ports, cwd: &Path, args: &ChangedArgs, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, CpdError> {
    let root = repo_root(ports, cwd)?;
    let base = args.base.as_deref().unwrap_or("origin/main");
    let merge_base = process(ports, "git", "git", &["merge-base", base, "HEAD"], &root)?;
    if !merge_base.status.success() {
        return Err(CpdError::new(ErrorCode::Usage, format!("test-changed: no merge base between {base} and HEAD")));
    }
    let merge_base = String::from_utf8_lossy(&merge_base.stdout).trim().to_owned();

    let mut files = BTreeSet::new();
    let diff = ["-c", "core.quotePath=false", "diff", "--name-only", "--no-renames", merge_base.as_str(), "--"];
    let others = ["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"];
    for git_args in [&diff[..], &others[..]] {
        let listed = process(ports, "git", "git", git_args, &root)?;
        if !listed.status.success() {
            return Err(CpdError::new(ErrorCode::GitFailed, format!("test-changed: git {} failed: {}", git_args[2], text(&listed.stderr).trim())));
        }
        files.extend(text(&listed.stdout).lines().filter(|l| !l.is_empty()).map(str::to_owned));
    }

    let selected = if files.iter().any(|f| is_root_input(f)) { vec!["--workspace".to_owned()] } else { select(ports, &root, &files)? };
    if selected.is_empty() {
        write(err, "test-changed: nothing to run\n")?;
        return Ok(0);
    }
    if args.dry_run {
        let words = [CARGO, "nextest", "run"].map(str::to_owned).into_iter().chain(selected);
        write(out, &format!("{}\n", words.map(|w| quote(&w)).collect::<Vec<_>>().join(" ")))?;
        return Ok(0);
    }
    let output = cargo(ports, &root, &[&["nextest", "run"], selected.iter().map(String::as_str).collect::<Vec<_>>().as_slice()].concat())?;
    forward(out, err, &output)?;
    Ok(status_of(&output))
}

/// `cpd test run <name>...` (§26.2) from `cwd`, run from the repo root: the output of the tests goes to `out` and
/// `err`. Non-zero when any test failed or did not run; no name is a `usage` error.
pub fn run(ports: &Ports, cwd: &Path, names: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, CpdError> {
    if names.is_empty() {
        return Err(CpdError::new(ErrorCode::Usage, "usage: cpd test run <name>..."));
    }
    let root = repo_root(ports, cwd)?;
    let mut status = 0;
    let mut rust: Vec<&str> = Vec::new();
    let mut pi: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for name in names {
        match pi_name(name) {
            Some((file, title)) => pi.entry(file).or_default().push(title),
            None => rust.push(name),
        }
    }
    if !rust.is_empty() {
        let output = cargo(ports, &root, &[&["nextest", "run", "--workspace", "--no-fail-fast"], rust.as_slice()].concat())?;
        forward(out, err, &output)?;
        if !output.status.success() {
            status = 1;
        }
    }
    for (file, titles) in pi {
        let mut node = vec!["--test".to_owned()];
        for title in titles {
            if pi_has(&root, file, title) {
                node.push(format!("--test-name-pattern={}", pi_pattern(title)));
            } else {
                write(err, &no_title(file, title))?;
                status = 1;
            }
        }
        if node.len() == 1 {
            continue;
        }
        node.push(file.to_owned());
        let args: Vec<&str> = node.iter().map(String::as_str).collect();
        let output = process(ports, "node", "node", &args, &root)?;
        forward(out, err, &output)?;
        if !output.status.success() {
            status = 1;
        }
    }
    Ok(status)
}

/// `scripts/tests.sh build`: `nextest run --workspace --no-run`, then `pnpm run typecheck`; stops at the first failure.
pub fn build(ports: &Ports, cwd: &Path, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, CpdError> {
    let root = repo_root(ports, cwd)?;
    let compiled = cargo(ports, &root, &["nextest", "run", "--workspace", "--no-run"])?;
    forward(out, err, &compiled)?;
    if !compiled.status.success() {
        return Ok(status_of(&compiled));
    }
    let typechecked = process(ports, "pnpm", "pnpm", &["run", "typecheck"], &root)?;
    forward(out, err, &typechecked)?;
    Ok(status_of(&typechecked))
}

/// `scripts/tests.sh list <name>`: 0 only when that one test exists.
pub fn list(ports: &Ports, cwd: &Path, name: &str, err: &mut dyn Write) -> Result<u8, CpdError> {
    let root = repo_root(ports, cwd)?;
    if let Some((file, title)) = pi_name(name) {
        if pi_has(&root, file, title) {
            return Ok(0);
        }
        write(err, &no_title(file, title))?;
        return Ok(1);
    }
    let listed = cargo(ports, &root, &["nextest", "list", "--workspace", "--message-format", "oneline", name])?;
    if listed.status.success() && text(&listed.stdout).lines().any(|l| !l.is_empty()) {
        return Ok(0);
    }
    write(err, &text(&listed.stderr))?;
    write(err, &format!("cpd test: no test named '{name}'\n"))?;
    Ok(1)
}

fn no_title(file: &str, title: &str) -> String {
    format!("cpd test: no test titled '{title}' in {file}\n")
}

/// A pi name: `pi/test/<file>.test.ts::<title>` is its file and its title; `None` for a Rust name.
fn pi_name(name: &str) -> Option<(&str, &str)> {
    let (file, title) = name.split_once("::")?;
    let is_pi = file.starts_with("pi/test/") && file.ends_with(".test.ts") && !title.is_empty();
    is_pi.then_some((file, title))
}

/// Whether `title` is in a `test(` or `it(` call of the file, quoted `'`, `"` or `` ` ``.
fn pi_has(root: &Path, file: &str, title: &str) -> bool {
    let Ok(source) = fs::read_to_string(root.join(file)) else { return false };
    ["test(", "it("].iter().any(|call| ['\'', '"', '`'].iter().any(|q| source.contains(&format!("{call}{q}{title}{q}"))))
}

/// The title as an anchored regular expression that matches that title only.
fn pi_pattern(title: &str) -> String {
    let mut pattern = String::from("^");
    for c in title.chars() {
        if "[]\\.*+?^$(){}|".contains(c) {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('$');
    pattern
}

/// The root-level inputs that no crate's selection can bound: the full suite.
fn is_root_input(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    file == "Cargo.toml"
        || file == "Cargo.lock"
        || file.starts_with("rust-toolchain")
        || name == "build.rs"
        || file.starts_with(".config/")
        || file.starts_with("core/src/store/migrations/")
        || file == "scripts/cargo.sh"
}

/// A workspace crate: its directory (real), and the crates that depend on it by path.
struct Member {
    name: String,
    dir: PathBuf,
    deps: Vec<PathBuf>,
}

/// What of a crate's integration test binary the changed files reach.
enum Area {
    Whole,
    Modules(Vec<String>),
}

/// The nextest arguments the files reach through the workspace's crates; empty when they reach no Rust test.
fn select(ports: &Ports, root: &Path, files: &BTreeSet<String>) -> Result<Vec<String>, CpdError> {
    let metadata = cargo(ports, root, &["metadata", "--format-version", "1", "--no-deps"])?;
    if !metadata.status.success() {
        return Err(internal(format!("test-changed: cargo metadata failed: {}", text(&metadata.stderr).trim())));
    }
    let members = members(&metadata.stdout)?;
    // Reverse dependencies: a member depends on another through a path dependency to its directory.
    let mut users: Vec<Vec<usize>> = vec![Vec::new(); members.len()];
    for (i, member) in members.iter().enumerate() {
        for dep in &member.deps {
            if let Some(j) = members.iter().position(|m| m.dir == *dep) {
                users[j].push(i);
            }
        }
    }
    let mut whole: Vec<usize> = Vec::new();
    let mut areas: Vec<(usize, Vec<(String, Area)>)> = Vec::new();
    for file in files {
        let abs = root.join(file);
        let Some((owner, member)) =
            members.iter().enumerate().filter(|(_, m)| abs.starts_with(&m.dir)).max_by_key(|(_, m)| m.dir.as_os_str().len())
        else {
            continue;
        };
        let rel = abs.strip_prefix(&member.dir).expect("the owner's directory is a prefix").to_string_lossy().into_owned();
        // tests/<area>/..., a directory with a main.rs is one test binary: its module files select the module, any
        // other file of it the binary. A directory without one (tests/common/) is shared by every binary.
        let binary = rel
            .strip_prefix("tests/")
            .and_then(|r| r.split_once('/'))
            .filter(|(area, rest)| !rest.is_empty() && member.dir.join("tests").join(area).join("main.rs").exists());
        match binary {
            Some((area, rest)) => {
                let module = rest.strip_suffix(".rs").filter(|m| *m != "main" && is_identifier(m));
                reach_area(&mut areas, owner, area, module);
            }
            None => reach(&mut whole, &users, owner),
        }
    }
    areas.retain(|(crate_, _)| !whole.contains(crate_));

    let mut args: Vec<String> = Vec::new();
    for &i in &whole {
        args.extend(["-p".to_owned(), members[i].name.clone()]);
    }
    for (i, _) in &areas {
        args.extend(["-p".to_owned(), members[*i].name.clone()]);
    }
    // One crate that is not whole: `--test` builds and links only its binaries. Otherwise cargo cannot narrow one
    // crate and keep another whole, so the narrowing is by filter alone.
    let single = whole.is_empty() && areas.len() == 1;
    if single {
        for (area, _) in &areas[0].1 {
            args.extend(["--test".to_owned(), area.clone()]);
        }
    }
    // The union: every test of the whole crates, and the binaries and named modules of the others.
    if !areas.is_empty() {
        for &i in &whole {
            args.extend(["-E".to_owned(), format!("package(={})", members[i].name)]);
        }
    }
    for (i, binaries) in &areas {
        let prefix = if single { String::new() } else { format!("package(={}) & ", members[*i].name) };
        for (area, selected) in binaries {
            let binary = if single && binaries.len() == 1 { String::new() } else { format!("binary({area}) & ") };
            match selected {
                Area::Whole => {
                    if !(prefix.is_empty() && binary.is_empty()) {
                        args.extend(["-E".to_owned(), format!("{prefix}binary({area})")]);
                    }
                }
                Area::Modules(modules) => {
                    for module in modules {
                        args.extend(["-E".to_owned(), format!("{prefix}{binary}test(/^{module}::/)")]);
                    }
                }
            }
        }
    }
    Ok(args)
}

/// The crate `i` and, transitively, its users are whole.
fn reach(whole: &mut Vec<usize>, users: &[Vec<usize>], i: usize) {
    if whole.contains(&i) {
        return;
    }
    whole.push(i);
    for &user in &users[i] {
        reach(whole, users, user);
    }
}

/// `module` of the test binary `area` of crate `i` is reached; `None` is the whole binary.
fn reach_area(areas: &mut Vec<(usize, Vec<(String, Area)>)>, i: usize, area: &str, module: Option<&str>) {
    let at = areas.iter().position(|(c, _)| *c == i).unwrap_or_else(|| {
        areas.push((i, Vec::new()));
        areas.len() - 1
    });
    let binaries = &mut areas[at].1;
    let slot = match binaries.iter().position(|(a, _)| a == area) {
        Some(p) => &mut binaries[p].1,
        None => {
            binaries.push((area.to_owned(), Area::Modules(Vec::new())));
            &mut binaries.last_mut().expect("just pushed").1
        }
    };
    match (module, &mut *slot) {
        (None, _) => *slot = Area::Whole,
        (Some(_), Area::Whole) => {}
        (Some(m), Area::Modules(modules)) => {
            if !modules.iter().any(|x| x == m) {
                modules.push(m.to_owned());
            }
        }
    }
}

fn is_identifier(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The workspace members of `cargo metadata --no-deps` output.
fn members(json: &[u8]) -> Result<Vec<Member>, CpdError> {
    let bad = |what: &str| internal(format!("test-changed: cargo metadata is not what was expected: {what}."));
    let meta: Value = serde_json::from_slice(json).map_err(|e| bad(&e.to_string()))?;
    let ids: BTreeSet<&str> = meta["workspace_members"].as_array().ok_or_else(|| bad("no workspace_members"))?.iter().filter_map(Value::as_str).collect();
    let real = |path: &str| fs::canonicalize(path).map_err(|e| internal(format!("test-changed: cannot resolve {path}: {e}.")));
    let mut members = Vec::new();
    for package in meta["packages"].as_array().ok_or_else(|| bad("no packages"))? {
        if !package["id"].as_str().is_some_and(|id| ids.contains(id)) {
            continue;
        }
        let name = package["name"].as_str().ok_or_else(|| bad("a package has no name"))?;
        let manifest = package["manifest_path"].as_str().ok_or_else(|| bad("a package has no manifest_path"))?;
        let dir = real(manifest)?.parent().ok_or_else(|| bad("a manifest has no directory"))?.to_path_buf();
        let mut deps = Vec::new();
        for dep in package["dependencies"].as_array().into_iter().flatten() {
            if let Some(path) = dep["path"].as_str() {
                deps.push(real(path)?);
            }
        }
        members.push(Member { name: name.to_owned(), dir, deps });
    }
    Ok(members)
}

/// `word` as one shell word: as is when it has only `A-Za-z0-9_=-`, else in single quotes.
fn quote(word: &str) -> String {
    if !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || "_=-/.".contains(c)) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// The repo's top, from `cwd`.
fn repo_root(ports: &Ports, cwd: &Path) -> Result<PathBuf, CpdError> {
    let top = process(ports, "git", "git", &["rev-parse", "--show-toplevel"], cwd)?;
    if !top.status.success() {
        return Err(CpdError::new(ErrorCode::NotAGitRepo, format!("cpd test: {} is not in a git repository.", cwd.display())));
    }
    Ok(PathBuf::from(text(&top.stdout).trim()))
}

/// `scripts/cargo.sh <args>` from `root`.
fn cargo(ports: &Ports, root: &Path, args: &[&str]) -> Result<Output, CpdError> {
    process(ports, "cargo", &root.join(CARGO).to_string_lossy(), args, root)
}

/// One process through the runner, from `cwd`, with no deadline: its output once it ends.
fn process(ports: &Ports, tool: &str, program: &str, args: &[&str], cwd: &Path) -> Result<Output, CpdError> {
    let call = Call {
        tool: tool.to_owned(),
        program: program.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        envs: Vec::new(),
        cwd: Some(cwd.to_path_buf()),
        deadline: NO_DEADLINE,
        stdin: None,
        stderr_to: None,
    };
    ports.runner.run(&call).map_err(|failed| match failed {
        crate::host::deadline::Failed::Io(e) => internal(format!("cpd test: cannot run {program}: {e}.")),
        crate::host::deadline::Failed::Timeout(e) => e,
    })
}

/// The exit status of a process, as the shell reports it: `128 + n` for a signal `n`.
fn status_of(output: &Output) -> u8 {
    output.status.code().map_or_else(|| 128 + output.status.signal().unwrap_or(0), |c| c) as u8
}

fn forward(out: &mut dyn Write, err: &mut dyn Write, output: &Output) -> Result<(), CpdError> {
    out.write_all(&output.stdout).map_err(|e| internal(format!("cpd test: cannot write stdout: {e}.")))?;
    err.write_all(&output.stderr).map_err(|e| internal(format!("cpd test: cannot write stderr: {e}.")))
}

fn write(to: &mut dyn Write, text: &str) -> Result<(), CpdError> {
    to.write_all(text.as_bytes()).map_err(|e| internal(format!("cpd test: cannot write: {e}.")))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn internal(message: String) -> CpdError {
    CpdError::new(ErrorCode::Internal, message)
}

#[cfg(test)]
mod tests;
