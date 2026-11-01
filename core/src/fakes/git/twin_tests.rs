//! The TWIN scenarios (research `tests-mock-git-route.md` §5): eleven `fn scenario(r: &dyn Runner) -> Transcript`
//! that drive `git` through a [`Runner`] and write down what they see (argv, exit code, stdout, the first line
//! of stderr), with shas and temp paths replaced by placeholders. Each runs here on `FakeRunner` + `GitRoute`
//! and is compared to a LITERAL, `EXPECTED_*`, below. `contract::git` (a later Job, the only place that may
//! run real git) runs the same scenarios over `SystemRunner` and compares them to the same literals: when real
//! git and the route disagree, the route changes. A behaviour of the route that no scenario pins is not
//! allowed to exist.
//!
//! The literals were recorded ONCE, by hand, from real git in a scratch dir, by running the scenario over
//! `SystemRunner` (a manual recording step, never a test, never regenerated in the Gate). Recorded with
//! git version 2.54.0 (Apple Git-157), `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`; the Spec asked
//! for git 2.43, which was not installed (unverified on 2.43). Where a message is known to change between
//! git versions (the wording of a refusal), the scenario records only the CLASS of the first stderr line
//! (`! error`, `! fatal`), not its text.
//!
//! Transcript format, one block per observed call:
//! ```text
//! $ git <args>       the arguments, `<root>` for the temp dir, `<S1>`.. for shas in order of first sight
//! = <exit code>
//! | <a line of stdout>      (`|` alone for an empty line, `\0` for a NUL byte, `\ no newline` when the last has none)
//! ! <the first line of stderr, or its class>
//! # <a note the scenario made: a file read, a path tested>
//! ```
//! Set-up calls are not in the transcript but must exit 0 (a failure panics with git's stderr).
//! No test here runs git (or any process), reads the wall clock or sleeps.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use super::{GitRoute, Universe};
use crate::clock::ManualClock;
use crate::host::deadline::Failed;
use crate::host::runner::{FakeRunner, Runner};

pub(crate) type Transcript = String;

const UNKNOWN: &str = "1111111111111111111111111111111111111111";

struct Raw {
    code: i32,
    stdout: String,
    stderr: String,
}

/// How much of git's stderr a transcript keeps.
#[derive(Clone, Copy, PartialEq)]
enum Show {
    /// The first line.
    Line,
    /// Only what comes before its first `:` (`error`, `fatal`, `hint`): for a text that varies by git version.
    Class,
}

/// A scratch dir, a `Runner`, and the transcript being written.
struct Sh<'a> {
    r: &'a dyn Runner,
    _tmp: TempDir,
    root: PathBuf,
    log: String,
    shas: Vec<String>,
}

fn s(path: &Path) -> &str {
    path.to_str().expect("a temp path is UTF-8")
}

/// The maximal runs of lowercase hex digits of `text`, with where they start.
fn hex_runs(text: &str) -> Vec<(usize, &str)> {
    let bytes = text.as_bytes();
    let (mut runs, mut i) = (Vec::new(), 0);
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() || (b'a'..=b'f').contains(&bytes[i]) {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit() || (b'a'..=b'f').contains(&bytes[i]))
            {
                i += 1;
            }
            runs.push((start, &text[start..i]));
        } else {
            i += 1;
        }
    }
    runs
}

impl<'a> Sh<'a> {
    fn new(r: &'a dyn Runner) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        Sh {
            r,
            _tmp: tmp,
            root,
            log: String::new(),
            shas: Vec::new(),
        }
    }

    fn at(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// Runs `git args` (in `cwd` when given) under the environment of the contract, and notes the shas it printed.
    fn spawn(&mut self, cwd: Option<&Path>, env: &[(&str, &str)], args: &[&str]) -> Raw {
        let mut cmd = Command::new("git");
        cmd.args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0");
        for (k, v) in env {
            cmd.env(k, v);
        }
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }
        cmd.stdin(std::process::Stdio::null());
        let out = match self.r.output(&mut cmd, "git", Duration::from_secs(60)) {
            Ok(o) => o,
            Err(Failed::Io(e)) => panic!("git {args:?} could not start: {e}"),
            Err(Failed::Timeout(e)) => panic!("git {args:?} timed out: {e:?}"),
        };
        let raw = Raw {
            code: out.status.code().expect("git exited by a signal"),
            stdout: String::from_utf8(out.stdout).expect("git printed UTF-8"),
            stderr: String::from_utf8(out.stderr).expect("git printed UTF-8"),
        };
        for text in [&raw.stdout, &raw.stderr] {
            for (_, run) in hex_runs(text) {
                if run.len() == 40 && !self.shas.iter().any(|known| known == run) {
                    self.shas.push(run.to_owned());
                }
            }
        }
        raw
    }

    fn record(&mut self, shown: &str, raw: &Raw, show: Show) {
        self.log
            .push_str(&format!("$ git {shown}\n= {}\n", raw.code));
        let stdout = raw.stdout.replace('\0', "\\0");
        for line in stdout.split_inclusive('\n') {
            let (text, ended) = match line.strip_suffix('\n') {
                Some(text) => (text, true),
                None => (line, false),
            };
            self.log.push_str(&format!(
                "|{}{text}\n",
                if text.is_empty() { "" } else { " " }
            ));
            if !ended {
                self.log.push_str("\\ no newline\n");
            }
        }
        // A progress meter ends with `\r` and the next text overwrites it: the first line is what is left on screen.
        if let Some(first) = raw
            .stderr
            .lines()
            .next()
            .and_then(|line| line.rsplit('\r').next())
        {
            let kept = match show {
                Show::Line => first,
                Show::Class => first.split(':').next().unwrap_or(first),
            };
            self.log.push_str(&format!("! {kept}\n"));
        }
    }

    /// An observed call: `git -C <dir> <args>`.
    fn obs(&mut self, dir: &Path, args: &[&str]) -> Raw {
        self.observe(dir, &[], args, Show::Line)
    }

    /// An observed call whose stderr text varies by git version: only its class is kept.
    fn obs_class(&mut self, dir: &Path, args: &[&str]) -> Raw {
        self.observe(dir, &[], args, Show::Class)
    }

    fn obs_env(&mut self, dir: &Path, env: &[(&str, &str)], args: &[&str]) -> Raw {
        self.observe(dir, env, args, Show::Line)
    }

    fn observe(&mut self, dir: &Path, env: &[(&str, &str)], args: &[&str], show: Show) -> Raw {
        let mut full = vec!["-C", s(dir)];
        full.extend_from_slice(args);
        let raw = self.spawn(None, env, &full);
        let tag = if env.is_empty() {
            String::new()
        } else {
            format!(
                "  [env {}]",
                env.iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        self.record(&format!("{}{tag}", full.join(" ")), &raw, show);
        raw
    }

    /// An observed call with the arguments exactly as given and `cwd` as the working directory of the process.
    fn obs_raw(&mut self, cwd: Option<&Path>, args: &[&str]) -> Raw {
        let raw = self.spawn(cwd, &[], args);
        let place = if cwd.is_some() { "  [cwd]" } else { "" };
        self.record(&format!("{}{place}", args.join(" ")), &raw, Show::Line);
        raw
    }

    /// A set-up call: not in the transcript, must exit 0.
    fn set(&mut self, dir: &Path, args: &[&str]) -> String {
        let mut full = vec!["-C", s(dir)];
        full.extend_from_slice(args);
        let raw = self.spawn(None, &[], &full);
        assert_eq!(raw.code, 0, "set-up `git {full:?}` said: {}", raw.stderr);
        raw.stdout
    }

    fn rev(&mut self, dir: &Path, rev: &str) -> String {
        self.set(dir, &["rev-parse", "--verify", rev])
            .trim()
            .to_owned()
    }

    fn note(&mut self, text: &str) {
        for line in text.lines() {
            self.log.push_str(&format!("# {line}\n"));
        }
    }

    fn write(&self, dir: &Path, path: &str, text: &str) {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    fn read(&self, path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// A repo `name` on `main` with an identity, whose first commit (`base`) holds `files`.
    fn repo(&mut self, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = self.at(name);
        let root = self.root.clone();
        self.set(&root, &["init", "-q", "-b", "main", s(&dir)]);
        self.ident(&dir);
        if !files.is_empty() {
            self.commit(&dir, files, "base");
        }
        dir
    }

    fn ident(&mut self, dir: &Path) {
        self.set(dir, &["config", "user.name", "cpd test"]);
        self.set(dir, &["config", "user.email", "test@cpd.invalid"]);
    }

    /// A bare repo `name` on `main`.
    fn bare(&mut self, name: &str) -> PathBuf {
        let dir = self.at(name);
        let root = self.root.clone();
        self.set(&root, &["init", "-q", "--bare", "-b", "main", s(&dir)]);
        dir
    }

    /// A clone of `origin` as `name`, with an identity.
    fn clone_of(&mut self, origin: &Path, name: &str) -> PathBuf {
        let dir = self.at(name);
        let root = self.root.clone();
        self.set(&root, &["clone", "-q", s(origin), s(&dir)]);
        self.ident(&dir);
        dir
    }

    /// Writes `files`, stages everything, commits `msg`, and returns the sha of the new HEAD.
    fn commit(&mut self, dir: &Path, files: &[(&str, &str)], msg: &str) -> String {
        for (path, text) in files {
            self.write(dir, path, text);
        }
        self.set(dir, &["add", "-A"]);
        self.set(dir, &["commit", "-q", "-m", msg]);
        self.rev(dir, "HEAD")
    }

    /// The transcript, with the temp dir and the shas replaced by placeholders.
    fn finish(self) -> Transcript {
        let text = self.log.replace(s(&self.root), "<root>");
        let mut out = String::new();
        let mut at = 0;
        for (start, run) in hex_runs(&text) {
            let known = if run.len() == 40 {
                self.shas.iter().position(|sha| sha == run)
            } else if run.len() >= 7 {
                self.shas.iter().position(|sha| sha.starts_with(run))
            } else {
                None
            };
            if let Some(n) = known {
                out.push_str(&text[at..start]);
                out.push_str(&format!("<S{}>", n + 1));
                at = start + run.len();
            }
        }
        out.push_str(&text[at..]);
        out
    }
}

// ---- 1. run env and exit mapping ---------------------------------------------------------------------------

/// `-C` (alone, twice, and `current_dir`), the lock timeouts the product passes in `GIT_CONFIG_COUNT`, the exit
/// code kept as git gave it, and the messages of a directory that is missing or is no repo.
pub(crate) fn run_env_and_exit_mapping(r: &dyn Runner) -> Transcript {
    let mut sh = Sh::new(r);
    let repo = sh.repo("repo", &[("README.md", "one\n")]);
    let root = sh.root.clone();
    let env = [
        ("GIT_CONFIG_COUNT", "2"),
        ("GIT_CONFIG_KEY_0", "core.filesRefLockTimeout"),
        ("GIT_CONFIG_VALUE_0", "10000"),
        ("GIT_CONFIG_KEY_1", "core.packedRefsTimeout"),
        ("GIT_CONFIG_VALUE_1", "10000"),
    ];
    sh.obs_env(&repo, &env, &["rev-parse", "--verify", "HEAD"]);
    sh.obs_env(
        &repo,
        &env,
        &["config", "--get", "core.filesRefLockTimeout"],
    );
    sh.obs_env(&repo, &env, &["config", "--get", "core.packedRefsTimeout"]);
    sh.obs(&repo, &["config", "--get", "core.filesRefLockTimeout"]);
    sh.obs_raw(Some(&repo), &["rev-parse", "--verify", "HEAD"]);
    sh.obs_raw(
        None,
        &[
            "-C",
            s(&root),
            "-C",
            "repo",
            "rev-parse",
            "--verify",
            "HEAD",
        ],
    );
    sh.obs(
        &repo,
        &[
            "-c",
            "core.editor=true",
            "-c",
            "sequence.editor=true",
            "rev-parse",
            "--verify",
            "HEAD",
        ],
    );
    sh.obs(&repo, &["--no-optional-locks", "status", "--porcelain"]);
    sh.write(&repo, "README.md", "two\n");
    sh.obs(&repo, &["diff", "--quiet"]);
    sh.obs(&repo, &["status", "--porcelain"]);
    sh.obs(&repo, &["rev-parse", "--verify", "nothing"]);
    let missing = root.join("missing");
    sh.obs_raw(None, &["-C", s(&missing), "status", "--porcelain"]);
    sh.obs(&root, &["status", "--porcelain"]);
    sh.finish()
}

// ---- 2. the exit-code table ---------------------------------------------------------------------------------

/// What the product reads off exit codes: `rev-parse --verify` 128 and `--quiet` 1, `ls-remote --exit-code` 2,
/// `merge-base --is-ancestor` 0/1/128, `symbolic-ref -q` 1, `cat-file -e` 1 and 128.
pub(crate) fn exit_code_table(r: &dyn Runner) -> Transcript {
    let mut sh = Sh::new(r);
    let origin = sh.bare("origin.git");
    let repo = sh.repo("repo", &[("a.txt", "a\n")]);
    let c1 = sh.rev(&repo, "HEAD");
    let c2 = sh.commit(&repo, &[("b.txt", "b\n")], "two");
    sh.set(&repo, &["remote", "add", "origin", s(&origin)]);
    sh.set(&repo, &["push", "origin", "main"]);
    sh.obs(&repo, &["rev-parse", "--verify", "--quiet", "nothing"]);
    sh.obs(&repo, &["rev-parse", "--verify", "nothing"]);
    sh.obs(
        &repo,
        &["rev-parse", "--verify", &format!("{c1}^{{commit}}")],
    );
    sh.obs(
        &repo,
        &["ls-remote", "--exit-code", "--heads", "origin", "nothing"],
    );
    sh.obs(
        &repo,
        &["ls-remote", "--exit-code", "--heads", "origin", "main"],
    );
    sh.obs(&repo, &["merge-base", "--is-ancestor", &c1, &c2]);
    sh.obs(&repo, &["merge-base", "--is-ancestor", &c2, &c1]);
    sh.obs(&repo, &["merge-base", "--is-ancestor", UNKNOWN, &c2]);
    sh.obs(&repo, &["symbolic-ref", "-q", "HEAD"]);
    sh.obs(&repo, &["symbolic-ref", "--short", "-q", "HEAD"]);
    sh.obs(&repo, &["cat-file", "-e", &format!("{c1}^{{commit}}")]);
    sh.obs(&repo, &["cat-file", "-e", &format!("{UNKNOWN}^{{commit}}")]);
    sh.obs(&repo, &["cat-file", "-e", UNKNOWN]);
    sh.set(&repo, &["checkout", "-q", "--detach"]);
    sh.obs(&repo, &["symbolic-ref", "-q", "HEAD"]);
    sh.obs(&repo, &["symbolic-ref", "HEAD"]);
    sh.finish()
}

// ---- 3. a worktree round trip -------------------------------------------------------------------------------

/// `worktree add` (new branch, detached and locked), `list --porcelain`, the `.git` file, `remove` refused on a
/// locked or a dirty worktree, `remove --force --force`, and `prune` of a worktree whose directory is gone.
pub(crate) fn worktree_round_trip(r: &dyn Runner) -> Transcript {
    let mut sh = Sh::new(r);
    let repo = sh.repo("repo", &[("a.txt", "a\n")]);
    let c1 = sh.rev(&repo, "HEAD");
    let c2 = sh.commit(&repo, &[("b.txt", "b\n")], "two");
    let (wt, gate, gone) = (sh.at("wt1"), sh.at("gate"), sh.at("gone"));
    sh.obs(
        &repo,
        &["worktree", "add", "--no-track", "-b", "att", s(&wt), &c1],
    );
    let dotgit = sh.read(&wt.join(".git"));
    sh.note(&format!(".git of wt1: {}", dotgit.trim()));
    sh.obs(&repo, &["worktree", "list", "--porcelain"]);
    sh.obs(
        &repo,
        &[
            "worktree",
            "add",
            "--detach",
            "--lock",
            "--reason",
            "cpd gate 4242",
            s(&gate),
            &c2,
        ],
    );
    sh.obs(&repo, &["worktree", "list", "--porcelain"]);
    sh.obs(&repo, &["worktree", "remove", s(&gate)]);
    sh.obs(&repo, &["worktree", "remove", "--force", s(&gate)]);
    sh.write(&wt, "dirty.txt", "x\n");
    sh.obs(&repo, &["worktree", "remove", s(&wt)]);
    sh.obs(
        &repo,
        &["worktree", "remove", "--force", "--force", s(&gate)],
    );
    sh.obs(&repo, &["worktree", "remove", "--force", s(&wt)]);
    sh.obs(&repo, &["worktree", "list", "--porcelain"]);
    sh.obs(&repo, &["worktree", "add", "--detach", s(&gone), &c1]);
    std::fs::remove_dir_all(&gone).unwrap();
    sh.obs(&repo, &["worktree", "list", "--porcelain"]);
    sh.obs(&repo, &["worktree", "prune"]);
    sh.obs(&repo, &["worktree", "list", "--porcelain"]);
    sh.finish()
}

// ---- 4. the shape of merge-tree's conflicts -----------------------------------------------------------------

/// `merge-tree --write-tree --name-only`: exit 1 on a conflict (content, add/add, modify/delete), the tree id,
/// the names, a blank line and the messages; exit 0 and the tree id alone when clean; the sides swapped.
pub(crate) fn merge_tree_conflict_shape(r: &dyn Runner) -> Transcript {
    let mut sh = Sh::new(r);
    let repo = sh.repo(
        "repo",
        &[("a.txt", "1\n2\n3\n"), ("d.txt", "d\n"), ("m.txt", "m\n")],
    );
    sh.set(&repo, &["checkout", "-q", "-b", "ours"]);
    sh.commit(
        &repo,
        &[
            ("a.txt", "1\nours\n3\n"),
            ("add.txt", "ours\n"),
            ("d.txt", "d ours\n"),
        ],
        "ours",
    );
    sh.set(&repo, &["checkout", "-q", "main"]);
    sh.set(&repo, &["checkout", "-q", "-b", "theirs"]);
    std::fs::remove_file(repo.join("d.txt")).unwrap();
    sh.commit(
        &repo,
        &[("a.txt", "1\ntheirs\n3\n"), ("add.txt", "theirs\n")],
        "theirs",
    );
    sh.set(&repo, &["checkout", "-q", "main"]);
    sh.set(&repo, &["checkout", "-q", "-b", "clean"]);
    sh.commit(&repo, &[("m.txt", "m clean\n")], "clean");
    sh.obs(
        &repo,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "ours",
            "theirs",
        ],
    );
    sh.obs(
        &repo,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "theirs",
            "ours",
        ],
    );
    sh.obs(
        &repo,
        &["merge-tree", "--write-tree", "--name-only", "ours", "clean"],
    );
    sh.obs(
        &repo,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "ours",
            "nothing",
        ],
    );
    sh.finish()
}

// ---- 5. a rebase that stops, continues and aborts -----------------------------------------------------------

/// `rebase` of two commits onto a main that conflicts with both: the stop (exit 1, `rebase-merge/orig-head`,
/// stages, markers, `diff --diff-filter=U`), `--continue` with the conflict unresolved and resolved, the
/// second stop, the finish; and `--abort` of a second attempt.
pub(crate) fn rebase_stop_continue_abort(r: &dyn Runner) -> Transcript {
    let mut sh = Sh::new(r);
    let repo = sh.repo("repo", &[("README.md", "one\n")]);
    sh.set(&repo, &["checkout", "-q", "-b", "feat"]);
    sh.commit(&repo, &[("README.md", "feat1\n")], "feat1");
    let orig = sh.commit(&repo, &[("README.md", "feat2\n")], "feat2");
    sh.set(&repo, &["checkout", "-q", "main"]);
    sh.commit(&repo, &[("README.md", "main\n")], "main");
    sh.set(&repo, &["checkout", "-q", "feat"]);
    let editors = ["-c", "core.editor=true", "-c", "sequence.editor=true"];
    let with = |rest: &[&str]| -> Vec<String> {
        editors
            .iter()
            .chain(rest)
            .map(|a| (*a).to_owned())
            .collect()
    };
    let run = |sh: &mut Sh, rest: &[&str], show: Show| {
        let args = with(rest);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        sh.observe(&repo, &[], &args, show)
    };
    run(&mut sh, &["rebase", "main"], Show::Class);
    let orig_head = sh.obs(
        &repo,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "rebase-merge/orig-head",
        ],
    );
    let orig_head = PathBuf::from(orig_head.stdout.trim());
    let saved = sh.read(&orig_head);
    sh.note(&format!("orig-head holds {}", saved.trim()));
    sh.note(&format!("orig-head is feat: {}", saved.trim() == orig));
    sh.obs(&repo, &["diff", "--name-only", "--diff-filter=U"]);
    sh.obs(&repo, &["status", "--porcelain"]);
    let markers = sh.read(&repo.join("README.md"));
    sh.note(&format!("README.md:\n{markers}"));
    run(&mut sh, &["rebase", "--continue"], Show::Line);
    sh.write(&repo, "README.md", "resolved1\n");
    sh.set(&repo, &["add", "README.md"]);
    run(&mut sh, &["rebase", "--continue"], Show::Class);
    sh.rev(&repo, "HEAD");
    sh.obs(&repo, &["diff", "--name-only", "--diff-filter=U"]);
    sh.write(&repo, "README.md", "resolved2\n");
    sh.set(&repo, &["add", "README.md"]);
    run(&mut sh, &["rebase", "--continue"], Show::Line);
    sh.rev(&repo, "HEAD");
    sh.obs(&repo, &["status", "--porcelain"]);
    sh.obs(&repo, &["symbolic-ref", "--short", "HEAD"]);
    sh.obs(&repo, &["rev-list", "--count", "main..feat"]);
    run(&mut sh, &["rebase", "main"], Show::Line);
    sh.note(&format!(
        "rebase-merge is gone: {}",
        !repo.join(".git/rebase-merge").exists()
    ));
    // The second attempt: back to the first state of feat, stop, abort.
    sh.set(&repo, &["reset", "-q", "--hard", &orig]);
    run(&mut sh, &["rebase", "main"], Show::Class);
    sh.note(&format!(
        "rebase-merge exists: {}",
        repo.join(".git/rebase-merge").exists()
    ));
    run(&mut sh, &["rebase", "--abort"], Show::Line);
    sh.obs(&repo, &["rev-parse", "HEAD"]);
    sh.obs(&repo, &["symbolic-ref", "--short", "HEAD"]);
    sh.obs(&repo, &["status", "--porcelain"]);
    sh.note(&format!(
        "rebase-merge exists: {}",
        repo.join(".git/rebase-merge").exists()
    ));
    run(&mut sh, &["rebase", "--abort"], Show::Class);
    sh.finish()
}

// ---- 6. the shapes of `push --porcelain` --------------------------------------------------------------------

/// `=`, `*`, ` `, `+` and `!` lines (`(fetch first)`, `(non-fast-forward)`, `(stale info)`), `--force-with-lease`
/// stale and current, `--delete`, against a local bare origin.
pub(crate) fn push_porcelain_shapes(r: &dyn Runner) -> Transcript {
    let mut sh = Sh::new(r);
    let origin = sh.bare("origin.git");
    let a = sh.repo("a", &[("f.txt", "1\n")]);
    sh.set(&a, &["remote", "add", "origin", s(&origin)]);
    let c1 = sh.rev(&a, "HEAD");
    sh.obs(
        &a,
        &[
            "push",
            "--porcelain",
            "origin",
            &format!("{c1}:refs/heads/main"),
        ],
    );
    sh.obs(
        &a,
        &[
            "push",
            "--porcelain",
            "origin",
            &format!("{c1}:refs/heads/main"),
        ],
    );
    let c2 = sh.commit(&a, &[("f.txt", "2\n")], "two");
    sh.obs(
        &a,
        &[
            "push",
            "--porcelain",
            "origin",
            &format!("{c2}:refs/heads/main"),
        ],
    );
    // B pushes a commit A has never seen; A then holds a commit that is not on it.
    let b = sh.clone_of(&origin, "b");
    let b1 = sh.commit(&b, &[("g.txt", "b\n")], "b1");
    sh.set(&b, &["push", "-q", "origin", "main"]);
    let c3 = sh.commit(&a, &[("f.txt", "3\n")], "three");
    sh.obs(
        &a,
        &[
            "push",
            "--porcelain",
            "origin",
            &format!("{c3}:refs/heads/main"),
        ],
    );
    sh.set(&a, &["fetch", "-q", "origin"]);
    sh.obs(
        &a,
        &[
            "push",
            "--porcelain",
            "origin",
            &format!("{c3}:refs/heads/main"),
        ],
    );
    sh.obs(
        &a,
        &[
            "push",
            "--porcelain",
            &format!("--force-with-lease=refs/heads/main:{c2}"),
            "origin",
            &format!("{c3}:refs/heads/main"),
        ],
    );
    sh.obs(
        &a,
        &[
            "push",
            "--porcelain",
            &format!("--force-with-lease=refs/heads/main:{b1}"),
            "origin",
            &format!("{c3}:refs/heads/main"),
        ],
    );
    sh.obs(
        &a,
        &[
            "push",
            "--porcelain",
            &format!("--force-with-lease=refs/heads/main:{c3}"),
            "origin",
            &format!("{c3}:refs/heads/main"),
        ],
    );
    sh.set(
        &a,
        &["push", "-q", "origin", &format!("{c3}:refs/heads/cpd/x")],
    );
    sh.obs(&a, &["push", "origin", "--delete", "cpd/x"]);
    sh.obs(&a, &["push", "origin", "--delete", "cpd/x"]);
    sh.obs(&a, &["push", "--porcelain", "origin", "--delete", "cpd/y"]);
    sh.obs(&a, &["push", "origin", "main"]);
    sh.finish()
}

// ---- 7. fetch and ls-remote ---------------------------------------------------------------------------------

/// `ls-remote` (`<sha>\t<ref>`, a missing ref: exit 0 and nothing, `--exit-code` 2, a repo that is not there
/// 128), `fetch origin <ref>` and the remote-tracking ref it moves, a ref that is not there 128.
pub(crate) fn fetch_and_ls_remote_shapes(r: &dyn Runner) -> Transcript {
    let mut sh = Sh::new(r);
    let origin = sh.bare("origin.git");
    let seed = sh.repo("seed", &[("f.txt", "1\n")]);
    sh.set(&seed, &["remote", "add", "origin", s(&origin)]);
    sh.set(&seed, &["push", "-q", "origin", "main"]);
    sh.set(&seed, &["push", "-q", "origin", "main:refs/heads/other"]);
    let a = sh.clone_of(&origin, "a");
    sh.obs(&a, &["ls-remote", "--heads", "origin", "refs/heads/main"]);
    sh.obs(&a, &["ls-remote", "origin", "refs/heads/nothing"]);
    sh.obs(
        &a,
        &["ls-remote", "--exit-code", "origin", "refs/heads/nothing"],
    );
    sh.obs(&a, &["ls-remote", "--heads", "origin"]);
    sh.obs(&a, &["ls-remote", "origin"]);
    sh.obs(&a, &["fetch", "origin", "main"]);
    sh.commit(&seed, &[("f.txt", "2\n")], "two");
    sh.set(&seed, &["push", "-q", "origin", "main"]);
    sh.obs(&a, &["rev-parse", "refs/remotes/origin/main"]);
    sh.obs(&a, &["fetch", "origin", "main"]);
    sh.obs(&a, &["rev-parse", "refs/remotes/origin/main"]);
    sh.obs(&a, &["rev-parse", "--verify", "refs/remotes/origin/other"]);
    sh.obs(&a, &["fetch", "origin", "nothing"]);
    let nowhere = sh.at("nowhere");
    sh.obs(&a, &["ls-remote", s(&nowhere)]);
    sh.obs(&a, &["fetch", "nowhere"]);
    sh.finish()
}

// ---- 8. rule S's plumbing -----------------------------------------------------------------------------------

/// What `git/lead.rs` runs in a linked worktree whose branch moved under it: `--git-path index`,
/// `diff --quiet`, `write-tree`, `log -g --format=%T`, `read-tree -m -u`, and `index.lock` held.
pub(crate) fn rule_s_plumbing(r: &dyn Runner) -> Transcript {
    let mut sh = Sh::new(r);
    let repo = sh.repo("repo", &[("a.txt", "one\n")]);
    let c1 = sh.rev(&repo, "HEAD");
    let lead = sh.at("lead");
    sh.set(
        &repo,
        &["worktree", "add", "-q", "-b", "lead", s(&lead), &c1],
    );
    sh.set(&repo, &["checkout", "-q", "-b", "side"]);
    let c2 = sh.commit(&repo, &[("a.txt", "two\n"), ("b.txt", "b\n")], "two");
    sh.set(&repo, &["checkout", "-q", "main"]);
    // The branch `lead` moves to c2 behind the back of the worktree.
    sh.set(&repo, &["update-ref", "refs/heads/lead", &c2, &c1]);
    sh.obs(
        &lead,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    sh.obs(
        &repo,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    sh.obs(&lead, &["rev-parse", "--verify", "HEAD"]);
    sh.obs(&lead, &["--no-optional-locks", "diff", "--quiet"]);
    sh.obs(&lead, &["status", "--porcelain"]);
    let tree = sh.obs(&lead, &["write-tree"]).stdout.trim().to_owned();
    sh.obs(&lead, &["rev-parse", "--verify", &format!("{c2}^{{tree}}")]);
    sh.obs(&lead, &["log", "-g", "--format=%T", "refs/heads/lead"]);
    let lock = repo.join(".git/worktrees/lead/index.lock");
    std::fs::write(&lock, "").unwrap();
    sh.obs(&lead, &["read-tree", "-m", "-u", &tree, &c2]);
    sh.obs(&lead, &["write-tree"]);
    std::fs::remove_file(&lock).unwrap();
    sh.obs(&lead, &["read-tree", "-m", "-u", &tree, &c2]);
    sh.obs(&lead, &["status", "--porcelain"]);
    sh.obs(&lead, &["write-tree"]);
    sh.obs(&lead, &["rev-parse", "--verify", &format!("{c2}^{{tree}}")]);
    let moved = sh.read(&lead.join("a.txt"));
    sh.note(&format!("a.txt in the worktree: {}", moved.trim()));
    sh.note(&format!(
        "b.txt in the worktree: {}",
        lead.join("b.txt").exists()
    ));
    sh.finish()
}

// ---- 9. status and diff -------------------------------------------------------------------------------------

/// `status --porcelain` (`M`, `A`, `D`, `??`, the collapse of an untracked directory, a quoted name), with
/// `--untracked-files=no` and `-z`; `diff` (`--name-only`, `-z`, `--cached`, `--quiet`, `a..b`, `a...b`).
pub(crate) fn status_and_diff_shapes(r: &dyn Runner) -> Transcript {
    let mut sh = Sh::new(r);
    let repo = sh.repo(
        "repo",
        &[("a.txt", "a\n"), ("b.txt", "b\n"), ("sub/c.txt", "c\n")],
    );
    let c1 = sh.rev(&repo, "HEAD");
    sh.obs(&repo, &["status", "--porcelain"]);
    sh.write(&repo, "a.txt", "a2\n");
    sh.write(&repo, "n.txt", "n\n");
    sh.set(&repo, &["add", "n.txt"]);
    std::fs::remove_file(repo.join("b.txt")).unwrap();
    sh.write(&repo, "sub/c.txt", "c2\n");
    sh.set(&repo, &["add", "sub/c.txt"]);
    sh.write(&repo, "sub/c.txt", "c3\n");
    sh.write(&repo, "u.txt", "u\n");
    sh.write(&repo, "newdir/x.txt", "x\n");
    sh.write(&repo, "newdir/y.txt", "y\n");
    sh.write(&repo, "sp ace.txt", "s\n");
    sh.obs(&repo, &["status", "--porcelain"]);
    sh.obs(&repo, &["status", "--porcelain", "--untracked-files=no"]);
    sh.obs(&repo, &["status", "--porcelain", "-z"]);
    sh.obs(&repo, &["diff", "--name-only"]);
    sh.obs(&repo, &["diff", "--name-only", "--cached"]);
    sh.obs(&repo, &["diff", "--name-only", "-z", "HEAD"]);
    sh.obs(&repo, &["diff", "--quiet"]);
    sh.obs(&repo, &["diff", "--quiet", "--cached"]);
    sh.obs(&repo, &["diff", "--name-only", "--diff-filter=U"]);
    let c2 = sh.commit(&repo, &[], "two");
    sh.set(&repo, &["checkout", "-q", "-b", "side", &c1]);
    let c3 = sh.commit(&repo, &[("side.txt", "s\n")], "side");
    sh.obs(&repo, &["status", "--porcelain"]);
    sh.obs(
        &repo,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            &format!("{c1}..{c2}"),
        ],
    );
    sh.obs(
        &repo,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            &format!("{c1}...{c2}"),
        ],
    );
    sh.obs(
        &repo,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            &format!("{c2}...{c3}"),
        ],
    );
    sh.obs(
        &repo,
        &["diff", "--name-only", "--no-renames", "-z", &c1, &c2],
    );
    sh.obs(&repo, &["diff", "--quiet", &c1, &c1]);
    sh.obs(&repo, &["diff", "--quiet", &c1, &c2]);
    sh.finish()
}

// ---- 10. rev-list and ls-tree -------------------------------------------------------------------------------

/// `rev-list --count a..HEAD`, `--no-merges HEAD --not --remotes=origin` (a list, and a count with and without
/// `--no-merges` around a merge commit), `ls-tree -r --name-only` with and without `-z`.
pub(crate) fn rev_list_ls_tree_shapes(r: &dyn Runner) -> Transcript {
    let mut sh = Sh::new(r);
    let origin = sh.bare("origin.git");
    let repo = sh.repo(
        "repo",
        &[
            ("a.txt", "a\n"),
            ("a/b.txt", "ab\n"),
            ("sp ace.txt", "s\n"),
            ("sub/c.txt", "c\n"),
        ],
    );
    sh.set(&repo, &["remote", "add", "origin", s(&origin)]);
    sh.set(&repo, &["push", "-q", "origin", "main"]);
    let c1 = sh.rev(&repo, "HEAD");
    sh.obs(&repo, &["rev-list", "--count", "HEAD"]);
    sh.obs(&repo, &["rev-list", "--count", &format!("{c1}..HEAD")]);
    sh.obs(
        &repo,
        &[
            "rev-list",
            "--no-merges",
            "HEAD",
            "--not",
            "--remotes=origin",
        ],
    );
    sh.set(&repo, &["checkout", "-q", "-b", "feat"]);
    sh.commit(&repo, &[("f.txt", "f\n")], "f1");
    sh.set(&repo, &["checkout", "-q", "main"]);
    let m1 = sh.commit(&repo, &[("m.txt", "m\n")], "m1");
    sh.obs(
        &repo,
        &[
            "rev-list",
            "--no-merges",
            "HEAD",
            "--not",
            "--remotes=origin",
        ],
    );
    sh.set(
        &repo,
        &["merge", "-q", "--no-ff", "-m", "merge feat", "feat"],
    );
    sh.obs(&repo, &["rev-list", "--count", &format!("{c1}..HEAD")]);
    sh.obs(
        &repo,
        &[
            "rev-list",
            "--count",
            "--no-merges",
            "HEAD",
            "--not",
            "--remotes=origin",
        ],
    );
    sh.obs(
        &repo,
        &["rev-list", "--count", "HEAD", "--not", "--remotes=origin"],
    );
    sh.obs(&repo, &["rev-list", "--count", &format!("HEAD..{m1}")]);
    sh.obs(&repo, &["rev-list", "--count", &format!("{m1}..HEAD")]);
    sh.set(&repo, &["push", "-q", "origin", "main"]);
    sh.obs(
        &repo,
        &[
            "rev-list",
            "--no-merges",
            "HEAD",
            "--not",
            "--remotes=origin",
        ],
    );
    sh.obs(
        &repo,
        &["rev-list", "--count", "HEAD", "--not", "--remotes=origin"],
    );
    sh.obs(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    sh.obs(&repo, &["ls-tree", "-r", "--name-only", "-z", &c1]);
    sh.obs(
        &repo,
        &[
            "ls-tree",
            "-r",
            "--name-only",
            "-z",
            &format!("{c1}^{{tree}}"),
        ],
    );
    sh.obs(&repo, &["ls-tree", "-r", "--name-only", "-z", "nothing"]);
    sh.finish()
}

// ---- 11. branch -d and -m -----------------------------------------------------------------------------------

/// `branch -d` of a merged branch, an unmerged one, one checked out in another worktree and one that is not
/// there; `branch -D`; `branch -m` (a rename, onto a name that is taken, of a branch that is not there).
pub(crate) fn branch_d_m_refusals(r: &dyn Runner) -> Transcript {
    let mut sh = Sh::new(r);
    let repo = sh.repo("repo", &[("a.txt", "a\n")]);
    sh.set(&repo, &["branch", "merged"]);
    sh.set(&repo, &["checkout", "-q", "-b", "unmerged"]);
    sh.commit(&repo, &[("u.txt", "u\n")], "unmerged");
    sh.set(&repo, &["checkout", "-q", "main"]);
    let wt = sh.at("wt");
    sh.set(
        &repo,
        &["worktree", "add", "-q", "-b", "att", s(&wt), "main"],
    );
    sh.obs(&repo, &["branch", "-d", "merged"]);
    sh.obs_class(&repo, &["branch", "-d", "unmerged"]);
    sh.obs_class(&repo, &["branch", "-d", "att"]);
    sh.obs_class(&repo, &["branch", "-d", "main"]);
    sh.obs_class(&repo, &["branch", "-d", "nothing"]);
    sh.obs(&repo, &["branch", "-D", "unmerged"]);
    sh.set(&repo, &["branch", "old"]);
    sh.set(&repo, &["branch", "taken"]);
    sh.obs(&repo, &["branch", "-m", "old", "new"]);
    sh.obs(&repo, &["branch", "-m", "new", "taken"]);
    sh.obs_class(&repo, &["branch", "-m", "nothing", "other"]);
    sh.obs(&repo, &["branch", "-m", "att", "att2"]);
    sh.obs(&repo, &["branch", "--list"]);
    sh.obs(&repo, &["branch", "x", "nothing"]);
    sh.finish()
}

// ---- the twins ----------------------------------------------------------------------------------------------

/// The route over a fresh Universe, behind a `FakeRunner`.
fn fake() -> (FakeRunner, Arc<GitRoute>) {
    let clock = ManualClock::new();
    let universe = Arc::new(Mutex::new(Universe::new(Arc::new(clock.clone()))));
    let route = Arc::new(GitRoute::new(universe));
    (FakeRunner::new(clock).with_git(&route), route)
}

/// Runs `scenario` over the route; every call it made was one the route supports.
fn on_the_route(scenario: fn(&dyn Runner) -> Transcript) -> Transcript {
    let (runner, route) = fake();
    let transcript = scenario(&runner);
    assert_eq!(route.unsupported(), Vec::<String>::new());
    transcript
}

#[test]
fn run_env_and_exit_mapping_matches_real_git() {
    assert_eq!(
        on_the_route(run_env_and_exit_mapping),
        EXPECTED_RUN_ENV_AND_EXIT_MAPPING
    );
}

#[test]
fn exit_code_table_matches_real_git() {
    assert_eq!(on_the_route(exit_code_table), EXPECTED_EXIT_CODE_TABLE);
}

#[test]
fn worktree_round_trip_matches_real_git() {
    assert_eq!(
        on_the_route(worktree_round_trip),
        EXPECTED_WORKTREE_ROUND_TRIP
    );
}

#[test]
fn merge_tree_conflict_shape_matches_real_git() {
    assert_eq!(
        on_the_route(merge_tree_conflict_shape),
        EXPECTED_MERGE_TREE_CONFLICT_SHAPE
    );
}

#[test]
fn rebase_stop_continue_abort_matches_real_git() {
    assert_eq!(
        on_the_route(rebase_stop_continue_abort),
        EXPECTED_REBASE_STOP_CONTINUE_ABORT
    );
}

#[test]
fn push_porcelain_shapes_match_real_git() {
    assert_eq!(
        on_the_route(push_porcelain_shapes),
        EXPECTED_PUSH_PORCELAIN_SHAPES
    );
}

#[test]
fn fetch_and_ls_remote_shapes_match_real_git() {
    assert_eq!(
        on_the_route(fetch_and_ls_remote_shapes),
        EXPECTED_FETCH_AND_LS_REMOTE_SHAPES
    );
}

#[test]
fn rule_s_plumbing_matches_real_git() {
    assert_eq!(on_the_route(rule_s_plumbing), EXPECTED_RULE_S_PLUMBING);
}

#[test]
fn status_and_diff_shapes_match_real_git() {
    assert_eq!(
        on_the_route(status_and_diff_shapes),
        EXPECTED_STATUS_AND_DIFF_SHAPES
    );
}

#[test]
fn rev_list_ls_tree_shapes_match_real_git() {
    assert_eq!(
        on_the_route(rev_list_ls_tree_shapes),
        EXPECTED_REV_LIST_LS_TREE_SHAPES
    );
}

#[test]
fn branch_d_m_refusals_match_real_git() {
    assert_eq!(
        on_the_route(branch_d_m_refusals),
        EXPECTED_BRANCH_D_M_REFUSALS
    );
}

// ---- the literals, recorded from real git (see the top of the file) -----------------------------------------

const EXPECTED_RUN_ENV_AND_EXIT_MAPPING: &str = r##"$ git -C <root>/repo rev-parse --verify HEAD  [env GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=core.filesRefLockTimeout GIT_CONFIG_VALUE_0=10000 GIT_CONFIG_KEY_1=core.packedRefsTimeout GIT_CONFIG_VALUE_1=10000]
= 0
| <S1>
$ git -C <root>/repo config --get core.filesRefLockTimeout  [env GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=core.filesRefLockTimeout GIT_CONFIG_VALUE_0=10000 GIT_CONFIG_KEY_1=core.packedRefsTimeout GIT_CONFIG_VALUE_1=10000]
= 0
| 10000
$ git -C <root>/repo config --get core.packedRefsTimeout  [env GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=core.filesRefLockTimeout GIT_CONFIG_VALUE_0=10000 GIT_CONFIG_KEY_1=core.packedRefsTimeout GIT_CONFIG_VALUE_1=10000]
= 0
| 10000
$ git -C <root>/repo config --get core.filesRefLockTimeout
= 1
$ git rev-parse --verify HEAD  [cwd]
= 0
| <S1>
$ git -C <root> -C repo rev-parse --verify HEAD
= 0
| <S1>
$ git -C <root>/repo -c core.editor=true -c sequence.editor=true rev-parse --verify HEAD
= 0
| <S1>
$ git -C <root>/repo --no-optional-locks status --porcelain
= 0
$ git -C <root>/repo diff --quiet
= 1
$ git -C <root>/repo status --porcelain
= 0
|  M README.md
$ git -C <root>/repo rev-parse --verify nothing
= 128
! fatal: Needed a single revision
$ git -C <root>/missing status --porcelain
= 128
! fatal: cannot change to '<root>/missing': No such file or directory
$ git -C <root> status --porcelain
= 128
! fatal: not a git repository (or any of the parent directories): .git
"##;
const EXPECTED_EXIT_CODE_TABLE: &str = r##"$ git -C <root>/repo rev-parse --verify --quiet nothing
= 1
$ git -C <root>/repo rev-parse --verify nothing
= 128
! fatal: Needed a single revision
$ git -C <root>/repo rev-parse --verify <S1>^{commit}
= 0
| <S1>
$ git -C <root>/repo ls-remote --exit-code --heads origin nothing
= 2
$ git -C <root>/repo ls-remote --exit-code --heads origin main
= 0
| <S2>	refs/heads/main
$ git -C <root>/repo merge-base --is-ancestor <S1> <S2>
= 0
$ git -C <root>/repo merge-base --is-ancestor <S2> <S1>
= 1
$ git -C <root>/repo merge-base --is-ancestor <S3> <S2>
= 128
! fatal: Not a valid commit name <S3>
$ git -C <root>/repo symbolic-ref -q HEAD
= 0
| refs/heads/main
$ git -C <root>/repo symbolic-ref --short -q HEAD
= 0
| main
$ git -C <root>/repo cat-file -e <S1>^{commit}
= 0
$ git -C <root>/repo cat-file -e <S3>^{commit}
= 128
! fatal: Not a valid object name <S3>^{commit}
$ git -C <root>/repo cat-file -e <S3>
= 1
$ git -C <root>/repo symbolic-ref -q HEAD
= 1
$ git -C <root>/repo symbolic-ref HEAD
= 128
! fatal: ref HEAD is not a symbolic ref
"##;
const EXPECTED_WORKTREE_ROUND_TRIP: &str = r##"$ git -C <root>/repo worktree add --no-track -b att <root>/wt1 <S1>
= 0
| HEAD is now at <S1> base
! Preparing worktree (new branch 'att')
# .git of wt1: gitdir: <root>/repo/.git/worktrees/wt1
$ git -C <root>/repo worktree list --porcelain
= 0
| worktree <root>/repo
| HEAD <S2>
| branch refs/heads/main
|
| worktree <root>/wt1
| HEAD <S1>
| branch refs/heads/att
|
$ git -C <root>/repo worktree add --detach --lock --reason cpd gate 4242 <root>/gate <S2>
= 0
| HEAD is now at <S2> two
! Preparing worktree (detached HEAD <S2>)
$ git -C <root>/repo worktree list --porcelain
= 0
| worktree <root>/repo
| HEAD <S2>
| branch refs/heads/main
|
| worktree <root>/gate
| HEAD <S2>
| detached
| locked cpd gate 4242
|
| worktree <root>/wt1
| HEAD <S1>
| branch refs/heads/att
|
$ git -C <root>/repo worktree remove <root>/gate
= 128
! fatal: cannot remove a locked working tree, lock reason: cpd gate 4242
$ git -C <root>/repo worktree remove --force <root>/gate
= 128
! fatal: cannot remove a locked working tree, lock reason: cpd gate 4242
$ git -C <root>/repo worktree remove <root>/wt1
= 128
! fatal: '<root>/wt1' contains modified or untracked files, use --force to delete it
$ git -C <root>/repo worktree remove --force --force <root>/gate
= 0
$ git -C <root>/repo worktree remove --force <root>/wt1
= 0
$ git -C <root>/repo worktree list --porcelain
= 0
| worktree <root>/repo
| HEAD <S2>
| branch refs/heads/main
|
$ git -C <root>/repo worktree add --detach <root>/gone <S1>
= 0
| HEAD is now at <S1> base
! Preparing worktree (detached HEAD <S1>)
$ git -C <root>/repo worktree list --porcelain
= 0
| worktree <root>/repo
| HEAD <S2>
| branch refs/heads/main
|
| worktree <root>/gone
| HEAD <S1>
| detached
| prunable gitdir file points to non-existent location
|
$ git -C <root>/repo worktree prune
= 0
$ git -C <root>/repo worktree list --porcelain
= 0
| worktree <root>/repo
| HEAD <S2>
| branch refs/heads/main
|
"##;
const EXPECTED_MERGE_TREE_CONFLICT_SHAPE: &str = r##"$ git -C <root>/repo merge-tree --write-tree --name-only ours theirs
= 1
| <S5>
| a.txt
| add.txt
| d.txt
|
| Auto-merging a.txt
| CONFLICT (content): Merge conflict in a.txt
| Auto-merging add.txt
| CONFLICT (add/add): Merge conflict in add.txt
| CONFLICT (modify/delete): d.txt deleted in theirs and modified in ours.  Version ours of d.txt left in tree.
$ git -C <root>/repo merge-tree --write-tree --name-only theirs ours
= 1
| <S6>
| a.txt
| add.txt
| d.txt
|
| Auto-merging a.txt
| CONFLICT (content): Merge conflict in a.txt
| Auto-merging add.txt
| CONFLICT (add/add): Merge conflict in add.txt
| CONFLICT (modify/delete): d.txt deleted in theirs and modified in ours.  Version ours of d.txt left in tree.
$ git -C <root>/repo merge-tree --write-tree --name-only ours clean
= 0
| <S7>
$ git -C <root>/repo merge-tree --write-tree --name-only ours nothing
= 1
! merge-tree: nothing - not something we can merge
"##;
const EXPECTED_REBASE_STOP_CONTINUE_ABORT: &str = r##"$ git -C <root>/repo -c core.editor=true -c sequence.editor=true rebase main
= 1
| Auto-merging README.md
| CONFLICT (content): Merge conflict in README.md
! error
$ git -C <root>/repo rev-parse --path-format=absolute --git-path rebase-merge/orig-head
= 0
| <root>/repo/.git/rebase-merge/orig-head
# orig-head holds <S3>
# orig-head is feat: true
$ git -C <root>/repo diff --name-only --diff-filter=U
= 0
| README.md
$ git -C <root>/repo status --porcelain
= 0
| UU README.md
# README.md:
# <<<<<<< HEAD
# main
# =======
# feat1
# >>>>>>> <S2> (feat1)
$ git -C <root>/repo -c core.editor=true -c sequence.editor=true rebase --continue
= 1
| README.md: needs merge
| You must edit all merge conflicts and then
| mark them as resolved using git add
$ git -C <root>/repo -c core.editor=true -c sequence.editor=true rebase --continue
= 1
| [detached HEAD <S5>] feat1
|  1 file changed, 1 insertion(+), 1 deletion(-)
| Auto-merging README.md
| CONFLICT (content): Merge conflict in README.md
! error
$ git -C <root>/repo diff --name-only --diff-filter=U
= 0
| README.md
$ git -C <root>/repo -c core.editor=true -c sequence.editor=true rebase --continue
= 0
| [detached HEAD <S6>] feat2
|  1 file changed, 1 insertion(+), 1 deletion(-)
! Successfully rebased and updated refs/heads/feat.
$ git -C <root>/repo status --porcelain
= 0
$ git -C <root>/repo symbolic-ref --short HEAD
= 0
| feat
$ git -C <root>/repo rev-list --count main..feat
= 0
| 2
$ git -C <root>/repo -c core.editor=true -c sequence.editor=true rebase main
= 0
| Current branch feat is up to date.
# rebase-merge is gone: true
$ git -C <root>/repo -c core.editor=true -c sequence.editor=true rebase main
= 1
| Auto-merging README.md
| CONFLICT (content): Merge conflict in README.md
! error
# rebase-merge exists: true
$ git -C <root>/repo -c core.editor=true -c sequence.editor=true rebase --abort
= 0
$ git -C <root>/repo rev-parse HEAD
= 0
| <S3>
$ git -C <root>/repo symbolic-ref --short HEAD
= 0
| feat
$ git -C <root>/repo status --porcelain
= 0
# rebase-merge exists: false
$ git -C <root>/repo -c core.editor=true -c sequence.editor=true rebase --abort
= 128
! fatal
"##;
const EXPECTED_PUSH_PORCELAIN_SHAPES: &str = r##"$ git -C <root>/a push --porcelain origin <S1>:refs/heads/main
= 0
| To <root>/origin.git
| *	<S1>:refs/heads/main	[new branch]
| Done
$ git -C <root>/a push --porcelain origin <S1>:refs/heads/main
= 0
| To <root>/origin.git
| =	<S1>:refs/heads/main	[up to date]
| Done
$ git -C <root>/a push --porcelain origin <S2>:refs/heads/main
= 0
| To <root>/origin.git
|  	<S2>:refs/heads/main	<S1>..<S2>
| Done
$ git -C <root>/a push --porcelain origin <S4>:refs/heads/main
= 1
| To <root>/origin.git
| !	<S4>:refs/heads/main	[rejected] (fetch first)
| Done
! error: failed to push some refs to '<root>/origin.git'
$ git -C <root>/a push --porcelain origin <S4>:refs/heads/main
= 1
| To <root>/origin.git
| !	<S4>:refs/heads/main	[rejected] (non-fast-forward)
| Done
! error: failed to push some refs to '<root>/origin.git'
$ git -C <root>/a push --porcelain --force-with-lease=refs/heads/main:<S2> origin <S4>:refs/heads/main
= 1
| To <root>/origin.git
| !	<S4>:refs/heads/main	[rejected] (stale info)
| Done
! error: failed to push some refs to '<root>/origin.git'
$ git -C <root>/a push --porcelain --force-with-lease=refs/heads/main:<S3> origin <S4>:refs/heads/main
= 0
| To <root>/origin.git
| +	<S4>:refs/heads/main	<S3>...<S4> (forced update)
| Done
$ git -C <root>/a push --porcelain --force-with-lease=refs/heads/main:<S4> origin <S4>:refs/heads/main
= 0
| To <root>/origin.git
| =	<S4>:refs/heads/main	[up to date]
| Done
$ git -C <root>/a push origin --delete cpd/x
= 0
! To <root>/origin.git
$ git -C <root>/a push origin --delete cpd/x
= 1
! error: unable to delete 'cpd/x': remote ref does not exist
$ git -C <root>/a push --porcelain origin --delete cpd/y
= 1
! error: unable to delete 'cpd/y': remote ref does not exist
$ git -C <root>/a push origin main
= 0
! Everything up-to-date
"##;
const EXPECTED_FETCH_AND_LS_REMOTE_SHAPES: &str = r##"$ git -C <root>/a ls-remote --heads origin refs/heads/main
= 0
| <S1>	refs/heads/main
$ git -C <root>/a ls-remote origin refs/heads/nothing
= 0
$ git -C <root>/a ls-remote --exit-code origin refs/heads/nothing
= 2
$ git -C <root>/a ls-remote --heads origin
= 0
| <S1>	refs/heads/main
| <S1>	refs/heads/other
$ git -C <root>/a ls-remote origin
= 0
| <S1>	HEAD
| <S1>	refs/heads/main
| <S1>	refs/heads/other
$ git -C <root>/a fetch origin main
= 0
! From <root>/origin
$ git -C <root>/a rev-parse refs/remotes/origin/main
= 0
| <S1>
$ git -C <root>/a fetch origin main
= 0
! From <root>/origin
$ git -C <root>/a rev-parse refs/remotes/origin/main
= 0
| <S2>
$ git -C <root>/a rev-parse --verify refs/remotes/origin/other
= 0
| <S1>
$ git -C <root>/a fetch origin nothing
= 128
! fatal: couldn't find remote ref nothing
$ git -C <root>/a ls-remote <root>/nowhere
= 128
! fatal: '<root>/nowhere' does not appear to be a git repository
$ git -C <root>/a fetch nowhere
= 128
! fatal: 'nowhere' does not appear to be a git repository
"##;
const EXPECTED_RULE_S_PLUMBING: &str = r##"$ git -C <root>/lead rev-parse --path-format=absolute --git-path index
= 0
| <root>/repo/.git/worktrees/lead/index
$ git -C <root>/repo rev-parse --path-format=absolute --git-path index
= 0
| <root>/repo/.git/index
$ git -C <root>/lead rev-parse --verify HEAD
= 0
| <S2>
$ git -C <root>/lead --no-optional-locks diff --quiet
= 0
$ git -C <root>/lead status --porcelain
= 0
| M  a.txt
| D  b.txt
$ git -C <root>/lead write-tree
= 0
| <S3>
$ git -C <root>/lead rev-parse --verify <S2>^{tree}
= 0
| <S4>
$ git -C <root>/lead log -g --format=%T refs/heads/lead
= 0
| <S4>
| <S3>
$ git -C <root>/lead read-tree -m -u <S3> <S2>
= 128
! fatal: Unable to create '<root>/repo/.git/worktrees/lead/index.lock': File exists.
$ git -C <root>/lead write-tree
= 128
! fatal: Unable to create '<root>/repo/.git/worktrees/lead/index.lock': File exists.
$ git -C <root>/lead read-tree -m -u <S3> <S2>
= 0
$ git -C <root>/lead status --porcelain
= 0
$ git -C <root>/lead write-tree
= 0
| <S4>
$ git -C <root>/lead rev-parse --verify <S2>^{tree}
= 0
| <S4>
# a.txt in the worktree: two
# b.txt in the worktree: true
"##;
const EXPECTED_STATUS_AND_DIFF_SHAPES: &str = r##"$ git -C <root>/repo status --porcelain
= 0
$ git -C <root>/repo status --porcelain
= 0
|  M a.txt
|  D b.txt
| A  n.txt
| MM sub/c.txt
| ?? newdir/
| ?? "sp ace.txt"
| ?? u.txt
$ git -C <root>/repo status --porcelain --untracked-files=no
= 0
|  M a.txt
|  D b.txt
| A  n.txt
| MM sub/c.txt
$ git -C <root>/repo status --porcelain -z
= 0
|  M a.txt\0 D b.txt\0A  n.txt\0MM sub/c.txt\0?? newdir/\0?? sp ace.txt\0?? u.txt\0
\ no newline
$ git -C <root>/repo diff --name-only
= 0
| a.txt
| b.txt
| sub/c.txt
$ git -C <root>/repo diff --name-only --cached
= 0
| n.txt
| sub/c.txt
$ git -C <root>/repo diff --name-only -z HEAD
= 0
| a.txt\0b.txt\0n.txt\0sub/c.txt\0
\ no newline
$ git -C <root>/repo diff --quiet
= 1
$ git -C <root>/repo diff --quiet --cached
= 1
$ git -C <root>/repo diff --name-only --diff-filter=U
= 0
$ git -C <root>/repo status --porcelain
= 0
$ git -C <root>/repo diff --name-only --no-renames <S1>..<S2>
= 0
| a.txt
| b.txt
| n.txt
| newdir/x.txt
| newdir/y.txt
| sp ace.txt
| sub/c.txt
| u.txt
$ git -C <root>/repo diff --name-only --no-renames <S1>...<S2>
= 0
| a.txt
| b.txt
| n.txt
| newdir/x.txt
| newdir/y.txt
| sp ace.txt
| sub/c.txt
| u.txt
$ git -C <root>/repo diff --name-only --no-renames <S2>...<S3>
= 0
| side.txt
$ git -C <root>/repo diff --name-only --no-renames -z <S1> <S2>
= 0
| a.txt\0b.txt\0n.txt\0newdir/x.txt\0newdir/y.txt\0sp ace.txt\0sub/c.txt\0u.txt\0
\ no newline
$ git -C <root>/repo diff --quiet <S1> <S1>
= 0
$ git -C <root>/repo diff --quiet <S1> <S2>
= 1
"##;
const EXPECTED_REV_LIST_LS_TREE_SHAPES: &str = r##"$ git -C <root>/repo rev-list --count HEAD
= 0
| 1
$ git -C <root>/repo rev-list --count <S1>..HEAD
= 0
| 0
$ git -C <root>/repo rev-list --no-merges HEAD --not --remotes=origin
= 0
$ git -C <root>/repo rev-list --no-merges HEAD --not --remotes=origin
= 0
| <S3>
$ git -C <root>/repo rev-list --count <S1>..HEAD
= 0
| 3
$ git -C <root>/repo rev-list --count --no-merges HEAD --not --remotes=origin
= 0
| 2
$ git -C <root>/repo rev-list --count HEAD --not --remotes=origin
= 0
| 3
$ git -C <root>/repo rev-list --count HEAD..<S3>
= 0
| 0
$ git -C <root>/repo rev-list --count <S3>..HEAD
= 0
| 2
$ git -C <root>/repo rev-list --no-merges HEAD --not --remotes=origin
= 0
$ git -C <root>/repo rev-list --count HEAD --not --remotes=origin
= 0
| 0
$ git -C <root>/repo ls-tree -r --name-only HEAD
= 0
| a.txt
| a/b.txt
| f.txt
| m.txt
| sp ace.txt
| sub/c.txt
$ git -C <root>/repo ls-tree -r --name-only -z <S1>
= 0
| a.txt\0a/b.txt\0sp ace.txt\0sub/c.txt\0
\ no newline
$ git -C <root>/repo ls-tree -r --name-only -z <S1>^{tree}
= 0
| a.txt\0a/b.txt\0sp ace.txt\0sub/c.txt\0
\ no newline
$ git -C <root>/repo ls-tree -r --name-only -z nothing
= 128
! fatal: Not a valid object name nothing
"##;
const EXPECTED_BRANCH_D_M_REFUSALS: &str = r##"$ git -C <root>/repo branch -d merged
= 0
| Deleted branch merged (was <S1>).
$ git -C <root>/repo branch -d unmerged
= 1
! error
$ git -C <root>/repo branch -d att
= 1
! error
$ git -C <root>/repo branch -d main
= 1
! error
$ git -C <root>/repo branch -d nothing
= 1
! error
$ git -C <root>/repo branch -D unmerged
= 0
| Deleted branch unmerged (was <S2>).
$ git -C <root>/repo branch -m old new
= 0
$ git -C <root>/repo branch -m new taken
= 128
! fatal: a branch named 'taken' already exists
$ git -C <root>/repo branch -m nothing other
= 128
! fatal
$ git -C <root>/repo branch -m att att2
= 0
$ git -C <root>/repo branch --list
= 0
| + att2
| * main
|   new
|   taken
$ git -C <root>/repo branch x nothing
= 128
! fatal: not a valid object name: 'nothing'
"##;
