//! `pr push` (commands.md §9.9, contract §7.6): `cpd pr push` in-process on `run_with` over the fake ports (the git
//! route with a bare `origin`, a `sh` handler for the Gate's hooks) and a Plan written straight into the database. It
//! takes no lock (hs-d243): its Gate and a Job's merge into the PR run at once; the Gate runs `gate.pr` (the repo's
//! `pre-merge`) in the Lead worktree at G (hs-d679).

use crate::common;

use std::fs;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

use common::Run;
use common::fx::Fx as Kit;
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::fakes::git::{Head, Universe};
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Ctx, End};
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::{Value, json};

const URL: &str = "https://github.com/acme/pay/pull/812";

/// A repo with a bare `origin`, the Lead worktree of Plan `pay` (branch `cpd/pay`) pushed to `origin`, and PR `repo` of
/// the Plan with the given status. It derefs to the [`Kit`] (`env`, `git`, `calls`, ...).
struct Fx {
    kit: Kit,
    home: CpdHome,
    repo: PathBuf,
    origin: PathBuf,
    lead: PathBuf,
}

impl Deref for Fx {
    type Target = Kit;

    fn deref(&self) -> &Kit {
        &self.kit
    }
}

fn fx(status: &str) -> Fx {
    let kit = Kit::with_ports();
    kit.env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    // The PR is of the repo `acme/pay` (its `name`): its policy file is required, empty unless a test sets hooks.
    let repo = kit.keyed_repo("pay").canonicalize().unwrap();
    let origin = kit.env.root().join("repos/origin.git");
    kit.git(kit.env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    kit.local_origin(&repo, &origin);
    kit.git(&repo, &["push", "-q", "origin", "main"]);
    let setup = Setup { timeout: Duration::from_secs(30), log: kit.env.root().join("lead.log") };
    let lead = kit.scoped(|| create_lead_worktree(&repo, &PlanId::parse("pay").unwrap(), "main", &setup)).unwrap().path;
    kit.git(&lead, &["config", "user.name", "cpd test"]);
    kit.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
    kit.git(&lead, &["push", "-q", "-u", "origin", "cpd/pay"]);

    kit.env.write_config(&format!("version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n", kit.env.tmux_socket()));
    let home = CpdHome::resolve(Some(kit.env.cpd_home().into()), None).unwrap();
    // What `pr open` writes for a PR that has a pull request: its row, number and url.
    let (phase, state, columns, values, done) = match status {
        "dispatch" => ("dispatch", "", "", String::new(), "NULL"),
        "open" => ("deploy", "open", ", pr_number, pr_url, pr_opened_at, pull_request", format!(", 812, '{URL}', 't', (SELECT id FROM pull_requests)"), "NULL"),
        "merged" => ("done", "merged", ", pr_number, pr_url, pr_opened_at, pull_request, merged_at", format!(", 812, '{URL}', 't', (SELECT id FROM pull_requests), 't'"), "'t'"),
        other => panic!("no status {other}"),
    };
    let merged_at = if state == "merged" { "'t'" } else { "NULL" };
    let pull_request = if state.is_empty() {
        String::new()
    } else {
        format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
             VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, '{state}', 'none', 'none', 't', {merged_at}, 't');"
        )
    };
    let sql = format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay the bills', '{phase}', 'lead1', 't', 't', {done});
         {pull_request}
         INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at{columns})
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', '{status}', 'cpd/pay', '{lead}', cpd_txn(), 't', 't'{values});",
        repo = repo.display(),
        lead = lead.display(),
    );
    let fx = Fx { kit, home, repo, origin, lead };
    fx.store().write("fixture", |tx| Ok(tx.execute_batch(&sql)?)).unwrap();
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn push(&self) -> Run {
        self.cpd(&["pr", "push", "--plan", "pay"])
    }

    /// Commits `name` on the Lead Branch; returns the new tip.
    fn commit(&self, name: &str) -> String {
        self.commit_file(&self.lead, name, name, name);
        self.lead_sha()
    }

    /// Sets the repo file's `[hooks]` to `toml` (`pre-merge = "cmd"` lines), and commits an empty commit on the Lead Branch.
    fn hooks(&self, toml: &str) -> String {
        self.env.repo_policy("acme", "pay", &format!("[hooks]\n{toml}"));
        self.git(&self.lead, &["commit", "-q", "--allow-empty", "-m", "hooks"]);
        self.lead_sha()
    }

    /// The Gate's hooks (`sh -c <command>`) answer by `script`: what a shell would print, exit and leave in the gate
    /// worktree it runs in (the Gate is external software: nothing runs it).
    fn sh(&self, script: impl Fn(&Ran) -> Reply + Send + Sync + 'static) {
        let _ = (*self.fakes.runner).clone().route("sh", Arc::new(Sh { universe: self.universe.clone(), script: Box::new(script) }));
    }

    /// The commands the Gate's hooks were run with (`sh -c <command>`), in order.
    fn commands(&self) -> Vec<String> {
        self.calls("sh").iter().map(|call| call.args[1].clone()).collect()
    }

    fn lead_sha(&self) -> String {
        self.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay"])
    }

    fn remote_sha(&self) -> String {
        self.git(&self.origin, &["rev-parse", "refs/heads/cpd/pay"])
    }

    /// The `pre-merge` hook's output: the log without the Gate's own `gate: ...` lines at its head (the slot's, §7.6).
    fn gate_log(&self) -> String {
        let log = fs::read_to_string(self.home.plan_dir(&plan()).join("logs/pr-repo.pre-merge.log")).unwrap_or_default();
        let mut rest = log.as_str();
        while let Some(line) = rest.lines().next().filter(|l| l.starts_with("gate: ")) {
            rest = &rest[line.len() + 1..];
        }
        rest.to_owned()
    }

    /// Job `m` of the PR with Attempt `m-a1` in `merging`, as `attempt submit` leaves it: its
    /// worktree cut from the Lead Branch, with one commit writing `m.txt`.
    fn submitted(&self) -> AttemptId {
        let attempt = AttemptId::parse("m-a1").unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("attempt.log") };
        let made = self.scoped(|| create_attempt_worktree(&self.repo, &self.lead, &plan(), &attempt, &setup)).unwrap();
        self.commit_file(&made.path, "m.txt", "m\n", "m");
        let sha = self.git(&made.path, &["rev-parse", "HEAD"]);
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at)
                     VALUES ('pay', 'repo', 'm', 'M', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'running', 0, ?1, ?2, 't')",
                    params!["f".repeat(64), tx.id()],
                )?;
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                           submitted_sha, submitted_at, started_at)
                     VALUES ('pay', 'm-a1', 'm', 1, 'S2', 'ladder', 'merging', ?1, ?2, ?3, ?4, 't', 't')",
                    params![made.branch, made.path.to_str().unwrap(), made.base_sha, sha],
                )?;
                Ok(())
            })
            .unwrap();
        attempt
    }

    /// Driver step A8 for `attempt`, in this process, as `attempt run` runs it.
    fn merge(&self, attempt: &AttemptId) -> End {
        let config = Config::load(&self.home).unwrap();
        let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![0, 1, 2, 3, 4, 5] };
        let catalog = Catalog { pi_version: "1.2.0".to_owned(), o: model("p/opus"), s: model("p/sonnet"), h: model("p/haiku"), g: TierModel { model: "p/gemini".to_owned(), efforts: vec![] } };
        let ctx = Ctx { home: &self.home, config: &config, catalog: &catalog, source: "attempt run", metrics: &|_| None };
        self.scoped(|| merge::merge(&mut self.store(), &ctx, &plan(), attempt)).unwrap().end
    }

    /// Moves `cpd/pay` on `origin` by a commit the Lead Branch lacks, through a second clone.
    fn move_remote(&self) -> String {
        let clone = self.env.root().join("clone");
        self.git(self.env.root(), &["clone", "-q", self.origin.to_str().unwrap(), clone.to_str().unwrap()]);
        self.git(&clone, &["config", "user.name", "someone"]);
        self.git(&clone, &["config", "user.email", "someone@cpd.invalid"]);
        self.git(&clone, &["checkout", "-q", "cpd/pay"]);
        self.commit_file(&clone, "remote.txt", "remote\n", "remote");
        self.git(&clone, &["push", "-q", "origin", "cpd/pay"]);
        self.remote_sha()
    }
}

fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

fn code(run: &Run) -> (String, i32) {
    (run.error()["code"].as_str().unwrap().to_owned(), run.exit)
}

/// What a Gate hook's `sh` is asked: the gate worktree it runs in and that worktree's `HEAD`.
struct Ran {
    cwd: PathBuf,
    head: String,
}

/// What `git rev-parse HEAD; ls` prints in the hook's directory.
fn head_and_files(ran: &Ran) -> String {
    let mut files: Vec<String> = fs::read_dir(&ran.cwd)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    files.sort();
    format!("{}\n{}\n", ran.head, files.join("\n"))
}

/// The Gate runs in the Lead worktree, which `pr push` brought to G (rule S, hs-d679): its `HEAD` is the Lead Branch, at G.
fn ran(universe: &Mutex<Universe>, call: &Call) -> Ran {
    let cwd = call.cwd.clone().expect("a hook runs in a directory");
    let universe = universe.lock().unwrap();
    let head = match universe.head(&cwd) {
        Head::Detached(oid) => oid,
        Head::Branch(branch) => {
            let repo = universe.repo_of(&cwd).expect("a hook runs in a worktree");
            universe.resolve_ref(repo, &format!("refs/heads/{branch}")).unwrap_or_else(|| panic!("the Gate's branch {branch} has no commit"))
        }
    };
    Ran { cwd, head: head.to_string() }
}

/// The Gate's `sh`, answering by `script`.
struct Sh {
    universe: Arc<Mutex<Universe>>,
    script: Box<dyn Fn(&Ran) -> Reply + Send + Sync>,
}

impl Handler for Sh {
    fn handle(&self, call: &Call) -> Reply {
        (self.script)(&ran(&self.universe, call))
    }
}

/// A `pre-merge` paused mid-run: `sh -c <paused-gate>` prints the gated commit and its files, reaches `started`,
/// waits at `go` (two barriers, no sleep), then prints them again.
struct Paused {
    universe: Arc<Mutex<Universe>>,
    started: Barrier,
    go: Barrier,
}

const PAUSED_GATE: &str = "paused-gate";

impl Handler for Paused {
    fn handle(&self, call: &Call) -> Reply {
        assert_eq!(call.args[1], PAUSED_GATE, "the only hook of the test");
        // A failed read still reaches both barriers, so the test's thread is never left waiting for a Gate that died.
        let first = std::panic::catch_unwind(|| head_and_files(&ran(&self.universe, call)));
        self.started.wait();
        self.go.wait();
        let first = first.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        Reply::ok(&format!("{first}{}", head_and_files(&ran(&self.universe, call))))
    }
}

/// Lets a paused Gate go when it drops, so a failed assertion does not leave the `pr push` thread blocked for ever.
struct Release<'a>(Option<&'a Paused>);

impl Release<'_> {
    fn release(&mut self) {
        if let Some(gate) = self.0.take() {
            gate.go.wait();
        }
    }
}

impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.release();
    }
}

impl Fx {
    /// Sets the Gate's `pre-merge` to the paused gate, commits it on the Lead Branch and returns that commit and the gate.
    fn paused_gate(&self) -> (String, Arc<Paused>) {
        let gated = self.hooks(&format!("pre-merge = \"{PAUSED_GATE}\"\n"));
        let gate = Arc::new(Paused { universe: self.universe.clone(), started: Barrier::new(2), go: Barrier::new(2) });
        let _ = (*self.fakes.runner).clone().route("sh", gate.clone());
        (gated, gate)
    }

    /// `cpd pr push --plan pay` on a thread, until its Gate is paused; runs `during` then, lets the Gate finish and
    /// returns the push's stdout as JSON and what `during` returned.
    fn push_paused<R>(&self, gate: &Paused, during: impl FnOnce() -> R) -> (Value, R) {
        let ports = self.fakes.ports.clone();
        std::thread::scope(|s| {
            let push = s.spawn(move || {
                let argv = ["cpd", "pr", "push", "--plan", "pay"].map(std::ffi::OsString::from);
                let mut stdout = Vec::new();
                cpd::run_with(&ports, argv, &mut stdout);
                String::from_utf8(stdout).unwrap()
            });
            let mut release = Release(Some(gate));
            gate.started.wait();
            let during = during();
            release.release();
            let stdout = push.join().unwrap();
            (serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{e}: {stdout}")), during)
        })
    }
}

#[test]
fn push_gates_then_pushes_the_lead_branch() {
    let fx = fx("open");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    fx.sh(|ran| Reply::ok(&format!("{}\n", ran.head)));
    let tip = fx.commit("more.txt");
    assert_ne!(fx.remote_sha(), tip);

    let out = fx.push().ok();

    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "pushed": true, "sha": tip, "remote": tip}));
    assert_eq!(fx.remote_sha(), tip);
    assert_eq!(fx.gate_log().trim(), tip, "the Gate ran on the tip");
    assert_eq!(fx.lead_sha(), tip, "no rebase");
}

#[test]
fn push_again_runs_the_gate_and_pushes_nothing() {
    let fx = fx("open");
    // The gate worktree is removed after each run: the record goes to the repo's common dir.
    let gate = "echo gated >> \"$(git rev-parse --path-format=absolute --git-common-dir)/gated\"";
    fx.hooks(&format!("pre-merge = {gate:?}\n"));
    fx.sh(|_| Reply::ok(""));
    let tip = fx.lead_sha();
    fx.push().ok();

    let out = fx.push().ok();

    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "pushed": false, "sha": tip, "remote": tip}));
    assert_eq!(fx.commands(), [gate, gate], "the Gate ran both times");
}

#[test]
fn push_with_a_failed_gate_pushes_nothing() {
    let fx = fx("open");
    let before = fx.remote_sha();
    fx.hooks("pre-merge = \"echo the suite is red; exit 3\"\n");
    fx.sh(|_| Reply::exit(3, "the suite is red\n", ""));

    let run = fx.push();

    assert_eq!(code(&run), ("gate_failed".into(), 3));
    assert!(run.error()["details"]["tail"].as_str().unwrap().contains("the suite is red"));
    assert!(run.error()["details"]["log"].as_str().unwrap().ends_with("logs/pr-repo.pre-merge.log"));
    assert_eq!(fx.remote_sha(), before);
}

#[test]
fn push_with_gate_ci_runs_no_gate_and_pushes() {
    let fx = fx("open");
    fx.env.owner_policy("acme", "[pr]\ngate = \"ci\"\n");
    fx.hooks("pre-merge = \"touch hook-ran; exit 3\"\n");
    fx.sh(|_| Reply::exit(3, "", ""));

    let out = fx.push().ok();

    assert_eq!(out["pushed"], true);
    assert_eq!(fx.commands(), Vec::<String>::new(), "the pre-merge hook ran under pr.gate ci");
    assert_eq!(fx.remote_sha(), fx.lead_sha());
}

#[test]
fn push_refuses_a_remote_that_moved_and_forces_nothing() {
    let fx = fx("open");
    let moved = fx.move_remote();
    let tip = fx.commit("mine.txt");

    let run = fx.push();

    assert_eq!(code(&run), ("push_rejected".into(), 3));
    assert!(!run.error()["details"]["stderr"].as_str().unwrap().is_empty());
    assert_eq!(fx.remote_sha(), moved, "the remote is untouched");
    assert_eq!(fx.lead_sha(), tip);
}

/// `main` moves on `origin` through a second clone, writing `path`; the repo has not fetched it. Returns the new tip.
fn move_base(fx: &Fx, path: &str, content: &str) -> String {
    let clone = fx.env.root().join("base-clone");
    fx.git(fx.env.root(), &["clone", "-q", fx.origin.to_str().unwrap(), clone.to_str().unwrap()]);
    fx.git(&clone, &["config", "user.name", "someone"]);
    fx.git(&clone, &["config", "user.email", "someone@cpd.invalid"]);
    fx.commit_file(&clone, path, content, "base moved");
    fx.git(&clone, &["push", "-q", "origin", "main"]);
    fx.git(&clone, &["rev-parse", "HEAD"])
}

#[test]
fn push_checks_nothing_about_the_base_whether_it_moved_ahead_or_conflicts() {
    for (path, content) in [("base.txt", "base\n"), ("more.txt", "the base's more\n")] {
        let fx = fx("open");
        let seen = fx.git(&fx.repo, &["rev-parse", "origin/main"]);
        let moved = move_base(&fx, path, content);
        // The Lead Branch lacks the base, and in the second round conflicts with it.
        let tip = fx.commit("more.txt");

        let out = fx.push().ok();

        assert_eq!((out["pushed"].as_bool(), out["sha"].as_str()), (Some(true), Some(tip.as_str())), "{path}");
        assert_eq!(fx.remote_sha(), tip);
        assert_eq!(fx.lead_sha(), tip, "no merge of the base");
        assert_eq!(fx.git(&fx.repo, &["rev-parse", "origin/main"]), seen, "the base was not fetched");
        assert_ne!(moved, seen);
    }
}

#[test]
fn push_refuses_a_pr_that_is_not_open() {
    for status in ["dispatch", "merged"] {
        let fx = fx(status);
        let before = fx.remote_sha();
        let run = fx.push();
        assert_eq!(code(&run), ("illegal_transition".into(), 3), "{status}");
        assert_eq!(run.error()["details"]["from"], status);
        assert_eq!(fx.remote_sha(), before);
    }
}

#[test]
fn push_gates_the_commit_and_leaves_the_lead_worktrees_local_changes_alone() {
    let fx = fx("open");
    fx.hooks("pre-merge = \"ls\"\n");
    fx.sh(|ran| Reply::ok(&head_and_files(ran).split_once('\n').unwrap().1.to_owned()));
    let tip = fx.commit("more.txt");
    fs::write(fx.lead.join("scratch.txt"), "x").unwrap();

    let out = fx.push().ok();

    // No `dirty_target` (hs-d243): the Gate ran on the commit, without the untracked file.
    assert_eq!((out["pushed"].as_bool(), out["sha"].as_str()), (Some(true), Some(tip.as_str())));
    assert!(fx.gate_log().contains("more.txt") && !fx.gate_log().contains("scratch.txt"), "{}", fx.gate_log());
    assert_eq!(fx.remote_sha(), tip);
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "?? scratch.txt");
}

#[test]
fn push_when_the_remote_already_contains_the_gated_commit_pushes_nothing() {
    let fx = fx("open");
    let gated = fx.commit("g.txt");
    let later = fx.commit("later.txt");
    // A descendant of G reached origin; the Lead Branch is back at G.
    fx.git(&fx.lead, &["push", "-q", "origin", &format!("{later}:refs/heads/cpd/pay")]);
    fx.git(&fx.lead, &["reset", "-q", "--hard", &gated]);

    let out = fx.push().ok();

    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "pushed": false, "sha": gated, "remote": later}));
    assert_eq!(fx.remote_sha(), later, "nothing forced");
}

#[test]
fn a_job_merges_into_the_pr_during_its_gate_and_neither_waits() {
    let fx = fx("open");
    let (gated, gate) = fx.paused_gate();
    let attempt = fx.submitted();

    // The merge runs to its end while the Gate is paused: no lock to wait for.
    let (out, end) = fx.push_paused(&gate, || fx.merge(&attempt));

    let End::Merged { sha: merged } = end else { panic!("{end:?}") };
    // The Gate saw G, twice the same: the merge did not change its files.
    let log = fx.gate_log();
    assert!(log.starts_with(&format!("{gated}\n")), "the Gate did not run on G: {log}");
    // The merge pushed its own commit (rule P), which contains G: `pr push` pushed nothing.
    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "pushed": false, "sha": gated, "remote": merged}));
    assert!(fx.git(&fx.repo, &["merge-base", "--is-ancestor", &gated, &merged]).is_empty(), "the remote holds G");
    assert_eq!(fx.remote_sha(), merged);
    // The Lead worktree followed the Lead Branch (rule S).
    assert_eq!(fx.lead_sha(), merged);
    assert_eq!(fs::read_to_string(fx.lead.join("m.txt")).unwrap(), "m\n");
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
}

#[test]
fn push_of_a_pr_merged_during_its_gate_pushes_nothing() {
    let fx = fx("open");
    let before = fx.remote_sha();
    let (_, gate) = fx.paused_gate();

    let (out, ()) = fx.push_paused(&gate, || {
        fx.store()
            .write("pr poll", |tx| {
                tx.execute_batch("UPDATE prs SET status = 'merged', merged_at = 't' WHERE plan = 'pay'")?;
                Ok(())
            })
            .unwrap();
    });

    assert_eq!((out["error"]["code"].as_str(), out["error"]["details"]["from"].as_str()), (Some("illegal_transition"), Some("merged")), "{out}");
    assert_eq!(fx.remote_sha(), before, "nothing pushed");
}
