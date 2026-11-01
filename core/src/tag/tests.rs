//! `cpd tag`, ported from `cli/tests/tooling/tag_script.rs` (the exec tests of `scripts/tag-version.sh`) and extended
//! with every `tag-version:` line of docs/commands.md §26.1. Git is `Origin`: a scripted, stateful `git` on a `FakeRunner`
//! route, in-process; it answers exactly the calls §26.1 lists and refuses the rest.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::tag;
use crate::host::runner::{Call, Handler, Reply};
use crate::ports::Fakes;

fn cargo_toml(version: &str) -> String {
    format!("[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"{version}\"\npublish = false\n\n[workspace.dependencies]\nclap = {{ version = \"4.6\" }}\n")
}

fn version_of(toml: &str) -> Option<String> {
    let after = toml.split("[workspace.package]").nth(1)?;
    let line = after.lines().find(|l| l.starts_with("version = \""))?;
    Some(line["version = \"".len()..].split('"').next()?.to_owned())
}

/// `(annotated, commit)`.
type Tag = (bool, String);

#[derive(Default)]
struct State {
    /// The first-parent history of origin/main, oldest first: `(sha, Cargo.toml or None when absent)`.
    commits: Vec<(String, Option<String>)>,
    origin_tags: BTreeMap<String, Tag>,
    local_tags: BTreeMap<String, Tag>,
    /// The Cargo.toml of the checkout's HEAD, which no correct run reads.
    checkout_toml: Option<String>,
    /// The subcommands that fail (`fetch`, `ls-remote`, `log`, `tag`, `push`, `show`).
    failing: Vec<&'static str>,
    /// `log` answers this instead of the real commit (`Some("")` is an empty answer).
    log_says: Option<String>,
    /// A concurrent pusher: after the first `ls-remote` the remote has the annotated tag on this commit.
    racing: Option<(usize, String)>,
    /// Calls the model does not know.
    refused: Vec<Vec<String>>,
}

struct Origin(Mutex<State>);

fn fail(stderr: &str) -> Reply {
    Reply::exit(128, "", stderr)
}

fn oid_of(tag: &Tag) -> String {
    if tag.0 { format!("a{}", &tag.1[1..]) } else { tag.1.clone() }
}

impl Handler for Origin {
    fn handle(&self, call: &Call) -> Reply {
        let mut s = self.0.lock().unwrap();
        // `git -C <dir> ...` and `cwd` both name the repository: there is only one.
        let mut args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        if args.first() == Some(&"-C") {
            args.drain(..2);
        }
        let sub = args.first().copied().unwrap_or("");
        if sub == "ls-remote"
            && let Some((asked, sha)) = s.racing.clone() {
                s.racing = Some((asked + 1, sha.clone()));
                if asked >= 1 {
                    let name = args.last().unwrap().trim_end_matches("^{}").trim_start_matches("refs/tags/").to_owned();
                    s.origin_tags.insert(name, (true, sha));
                }
            }
        let failing = s.failing.contains(&sub);
        
        match (sub, &args[1..]) {
            _ if failing => fail("fatal: scripted failure\n"),
            ("fetch", ["--quiet", "origin", "main"]) => Reply::ok(""),
            ("show", [spec]) => match spec.split_once(':') {
                Some(("origin/main", "Cargo.toml")) => match s.commits.last().and_then(|c| c.1.clone()) {
                    Some(text) => Reply::ok(&text),
                    None => fail("fatal: path 'Cargo.toml' does not exist in 'origin/main'\n"),
                },
                Some(("HEAD", "Cargo.toml")) => Reply::ok(&s.checkout_toml.clone().unwrap_or_default()),
                Some((sha, "Cargo.toml")) => match s.commits.iter().find(|c| c.0 == sha).and_then(|c| c.1.clone()) {
                    Some(text) => Reply::ok(&text),
                    None => fail("fatal: bad object\n"),
                },
                _ => self.refuse(&mut s, call),
            },
            ("ls-remote", ["--tags", "origin", spec]) => {
                let (name, peeled) = match spec.strip_suffix("^{}") {
                    Some(n) => (n, true),
                    None => (*spec, false),
                };
                match s.origin_tags.get(name.trim_start_matches("refs/tags/")) {
                    Some(t) if peeled && t.0 => Reply::ok(&format!("{}\t{spec}\n", t.1)),
                    Some(t) if !peeled => Reply::ok(&format!("{}\t{spec}\n", oid_of(t))),
                    _ => Reply::ok(""),
                }
            }
            ("log", ["-1", "--first-parent", g, "--format=%H", "origin/main", "--", "Cargo.toml"]) if g.starts_with("-G") => {
                match s.log_says.clone() {
                    Some(sha) => Reply::ok(&if sha.is_empty() { sha } else { format!("{sha}\n") }),
                    None => {
                        // The newest commit whose Cargo.toml version differs from its parent's.
                        let versions: Vec<Option<String>> = s.commits.iter().map(|c| c.1.as_deref().and_then(version_of)).collect();
                        let hit = (0..versions.len()).rev().find(|&i| versions[i].is_some() && (i == 0 || versions[i - 1] != versions[i]));
                        Reply::ok(&hit.map(|i| format!("{}\n", s.commits[i].0)).unwrap_or_default())
                    }
                }
            }
            ("rev-parse", ["-q", "--verify", name]) => match s.local_tags.get(name.trim_start_matches("refs/tags/")) {
                Some(t) => Reply::ok(&format!("{}\n", oid_of(t))),
                None => Reply::exit(1, "", ""),
            },
            ("rev-parse", [spec]) if spec.ends_with("^{commit}") => {
                let name = spec.trim_end_matches("^{commit}").trim_start_matches("refs/tags/");
                match s.local_tags.get(name) {
                    Some(t) => Reply::ok(&format!("{}\n", t.1)),
                    None => fail("fatal: bad revision\n"),
                }
            }
            ("tag", ["-a", name, sha, "-m", message]) if name == message => {
                s.local_tags.insert((*name).to_owned(), (true, (*sha).to_owned()));
                Reply::ok("")
            }
            ("push", ["--quiet", "origin", spec]) => match s.local_tags.get(spec.trim_start_matches("refs/tags/")).cloned() {
                Some(t) => {
                    s.origin_tags.insert(spec.trim_start_matches("refs/tags/").to_owned(), t);
                    Reply::ok("")
                }
                None => fail("error: src refspec does not match any\n"),
            },
            _ => self.refuse(&mut s, call),
        }
    }
}

impl Origin {
    fn refuse(&self, s: &mut State, call: &Call) -> Reply {
        s.refused.push(call.args.clone());
        Reply::exit(129, "", "fake git: not modelled\n")
    }
}

struct Fixture {
    fakes: Fakes,
    origin: Arc<Origin>,
    root: std::path::PathBuf,
    /// c1 (0.0.5), c2 (0.13.45), c3 (unrelated).
    c1: String,
    c2: String,
    c3: String,
}

fn sha(n: usize) -> String {
    format!("c{n:039x}")
}

impl Fixture {
    fn new() -> Fixture {
        let fakes = Fakes::new();
        let origin = Arc::new(Origin(Mutex::new(State::default())));
        let _ = (*fakes.runner).clone().route("git", origin.clone());
        let fx = Fixture { fakes, origin, root: Path::new("/repo").to_owned(), c1: sha(1), c2: sha(2), c3: sha(3) };
        fx.push_commit(&fx.c1, Some(&cargo_toml("0.0.5")));
        fx.push_commit(&fx.c2, Some(&cargo_toml("0.13.45")));
        fx.push_commit(&fx.c3, Some(&cargo_toml("0.13.45")));
        fx
    }

    fn state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.origin.0.lock().unwrap())
    }

    fn push_commit(&self, sha: &str, toml: Option<&str>) {
        self.state(|s| s.commits.push((sha.to_owned(), toml.map(str::to_owned))));
    }

    fn run(&self) -> Result<String, String> {
        tag(&self.fakes.ports, &self.root)
    }

    fn origin_tags(&self) -> Vec<(String, Tag)> {
        self.state(|s| s.origin_tags.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    fn origin_tag(&self, name: &str) -> Option<Tag> {
        self.state(|s| s.origin_tags.get(name).cloned())
    }

    fn local_tags(&self) -> Vec<String> {
        self.state(|s| s.local_tags.keys().cloned().collect())
    }

    fn fails(&self, sub: &'static str) {
        self.state(|s| s.failing.push(sub));
    }

    fn subcommands(&self) -> Vec<String> {
        self.fakes
            .runner
            .calls()
            .iter()
            .map(|c| {
                let a: Vec<&String> = c.args.iter().skip(if c.args.first().map(String::as_str) == Some("-C") { 2 } else { 0 }).collect();
                a.first().map(|s| s.to_string()).unwrap_or_default()
            })
            .collect()
    }
}

#[test]
fn tags_the_commit_that_last_changed_the_version() {
    let fx = Fixture::new();

    let line = fx.run().unwrap_or_else(|e| panic!("{e}"));

    let (annotated, target) = fx.origin_tag("v0.13.45").unwrap_or_else(|| panic!("origin has no v0.13.45: {:?}", fx.origin_tags()));
    assert!(annotated, "the tag is annotated");
    assert_eq!(target, fx.c2, "v0.13.45 is on c2, not on c3 ({})", fx.c3);
    assert_eq!(line, format!("tag-version: tagged v0.13.45 on {}", fx.c2));
    assert!(fx.state(|s| s.refused.is_empty()), "{:?}", fx.state(|s| s.refused.clone()));
}

#[test]
fn a_second_run_changes_nothing() {
    let fx = Fixture::new();
    fx.run().unwrap();
    let before = fx.origin_tags();
    assert!(!before.is_empty(), "the first run tagged nothing");
    let calls = fx.fakes.runner.calls().len();

    let line = fx.run().unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(line, "tag-version: v0.13.45 exists; nothing to do");
    assert_eq!(fx.origin_tags(), before);
    let second: Vec<String> = fx.subcommands().split_off(calls);
    assert!(!second.iter().any(|s| s == "tag" || s == "push"), "the second run wrote: {second:?}");
}

#[test]
fn an_existing_remote_tag_is_left_alone() {
    let fx = Fixture::new();
    fx.state(|s| s.origin_tags.insert("v0.13.45".to_owned(), (true, sha(1))));

    let line = fx.run().unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(fx.origin_tag("v0.13.45").map(|t| t.1), Some(fx.c1.clone()), "the tag still points at c1");
    assert_eq!(line, "tag-version: v0.13.45 exists; nothing to do");
    assert!(fx.local_tags().is_empty(), "no local tag");
}

#[test]
fn a_new_version_gets_its_own_tag() {
    let fx = Fixture::new();
    fx.run().unwrap();
    let c4 = sha(4);
    fx.push_commit(&c4, Some(&cargo_toml("0.13.46")));
    fx.push_commit(&sha(5), Some(&cargo_toml("0.13.46")));

    let line = fx.run().unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(line, format!("tag-version: tagged v0.13.46 on {c4}"));
    assert_eq!(fx.origin_tag("v0.13.46").map(|t| t.1), Some(c4), "v0.13.46 is on c4");
    assert_eq!(fx.origin_tag("v0.13.45").map(|t| t.1), Some(fx.c2.clone()), "v0.13.45 stays on c2");
}

#[test]
fn it_reads_origin_main_not_the_checkout() {
    let fx = Fixture::new();
    fx.state(|s| s.checkout_toml = Some(cargo_toml("0.99.9")));

    let line = fx.run().unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(line, format!("tag-version: tagged v0.13.45 on {}", fx.c2));
    assert_eq!(fx.origin_tag("v0.13.45").map(|t| t.1), Some(fx.c2.clone()), "{:?}", fx.origin_tags());
    assert!(fx.origin_tag("v0.99.9").is_none(), "{:?}", fx.origin_tags());
    assert!(!fx.local_tags().contains(&"v0.99.9".to_owned()), "no local v0.99.9");
    let subs = fx.subcommands();
    assert!(!subs.iter().any(|s| ["checkout", "switch", "reset", "merge", "pull"].contains(&s.as_str())), "no checkout: {subs:?}");
}

#[test]
fn an_unreachable_origin_fails_loud_and_tags_nothing() {
    let fx = Fixture::new();
    fx.fails("fetch");

    let err = fx.run().unwrap_err();

    assert_eq!(err, "tag-version: cannot fetch origin/main");
    assert!(fx.local_tags().is_empty(), "a local tag exists");
    assert!(fx.origin_tags().is_empty());
}

#[test]
fn a_local_tag_whose_push_failed_is_pushed_by_the_next_run() {
    let fx = Fixture::new();
    fx.state(|s| s.local_tags.insert("v0.13.45".to_owned(), (true, sha(2))));
    assert!(fx.origin_tag("v0.13.45").is_none());

    let line = fx.run().unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(line, format!("tag-version: tagged v0.13.45 on {}", fx.c2));
    assert_eq!(fx.origin_tag("v0.13.45").map(|t| t.1), Some(fx.c2.clone()), "{:?}", fx.origin_tags());
    assert!(!fx.subcommands().iter().any(|s| s == "tag"), "the local tag is not created again");
}

// ---- every other `tag-version:` line of commands.md §26.1 ----

#[test]
fn no_cargo_toml_on_origin_main_fails_loud() {
    let fx = Fixture::new();
    fx.push_commit(&sha(4), None);

    assert_eq!(fx.run(), Err("tag-version: cannot read Cargo.toml on origin/main".to_owned()));
    assert!(fx.origin_tags().is_empty());
}

#[test]
fn no_workspace_version_fails_loud() {
    let fx = Fixture::new();
    fx.push_commit(&sha(4), Some("[workspace]\nmembers = []\n\n[workspace.dependencies]\nversion = \"1\"\n"));

    assert_eq!(fx.run(), Err("tag-version: no workspace version in Cargo.toml on origin/main".to_owned()));
    assert!(fx.origin_tags().is_empty());
}

#[test]
fn a_failing_ls_remote_fails_loud_and_tags_nothing() {
    let fx = Fixture::new();
    fx.fails("ls-remote");

    assert_eq!(fx.run(), Err("tag-version: cannot list the tags of origin".to_owned()));
    assert!(fx.local_tags().is_empty() && fx.origin_tags().is_empty());
}

#[test]
fn a_failing_log_fails_loud_and_tags_nothing() {
    let fx = Fixture::new();
    fx.fails("log");

    assert_eq!(fx.run(), Err("tag-version: cannot find the commit that set v0.13.45".to_owned()));
    assert!(fx.local_tags().is_empty() && fx.origin_tags().is_empty());
}

#[test]
fn an_empty_log_fails_loud_and_tags_nothing() {
    let fx = Fixture::new();
    fx.state(|s| s.log_says = Some(String::new()));

    assert_eq!(fx.run(), Err("tag-version: no commit on origin/main changed the version".to_owned()));
    assert!(fx.local_tags().is_empty() && fx.origin_tags().is_empty());
}

#[test]
fn a_commit_with_another_version_fails_loud_and_tags_nothing() {
    let fx = Fixture::new();
    fx.state(|s| s.log_says = Some(sha(1)));

    assert_eq!(fx.run(), Err(format!("tag-version: commit {} has version '0.0.5', not 0.13.45", fx.c1)));
    assert!(fx.local_tags().is_empty() && fx.origin_tags().is_empty());
}

#[test]
fn a_local_tag_that_points_elsewhere_fails_loud_and_pushes_nothing() {
    let fx = Fixture::new();
    fx.state(|s| s.local_tags.insert("v0.13.45".to_owned(), (true, sha(1))));

    assert_eq!(fx.run(), Err(format!("tag-version: local tag v0.13.45 points at {}, not {}", fx.c1, fx.c2)));
    assert!(fx.origin_tags().is_empty());
    assert!(!fx.subcommands().iter().any(|s| s == "push"), "nothing is pushed");
}

#[test]
fn a_failing_tag_fails_loud_and_pushes_nothing() {
    let fx = Fixture::new();
    fx.fails("tag");

    assert_eq!(fx.run(), Err("tag-version: cannot create v0.13.45".to_owned()));
    assert!(fx.origin_tags().is_empty());
    assert!(!fx.subcommands().iter().any(|s| s == "push"), "nothing is pushed");
}

#[test]
fn a_failing_push_that_left_the_remote_without_the_commit_fails_loud() {
    let fx = Fixture::new();
    fx.fails("push");

    assert_eq!(fx.run(), Err("tag-version: push of v0.13.45 failed".to_owned()));
    assert!(fx.origin_tags().is_empty());
}

#[test]
fn a_failing_push_is_accepted_when_the_remote_already_has_the_commit() {
    let fx = Fixture::new();
    fx.fails("push");
    // A concurrent run pushed the tag between our `ls-remote` and our `push`: the first `ls-remote` is empty.
    fx.state(|s| s.racing = Some((0, fx.c2.clone())));

    let line = fx.run().unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(line, format!("tag-version: tagged v0.13.45 on {}", fx.c2));
}
