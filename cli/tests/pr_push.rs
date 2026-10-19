//! `pr push` (commands.md §9.9, contract §7.6): the built binary against a temporary
//! repo with a bare `origin` and a Plan written straight into the database. It takes
//! no lock (hs-d243): its Gate runs in a gate worktree of its own, and a Job merges
//! into the PR while it runs.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::{CPD, Run, TestEnv};
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Ctx, End};
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::{Value, json};

const URL: &str = "https://github.com/acme/pay/pull/812";

/// A repo with a bare `origin`, the Lead worktree of Plan `pay` (branch `cpd/pay`)
/// pushed to `origin`, and PR `repo` of the Plan with the given status.
struct Fx {
    env: TestEnv,
    home: CpdHome,
    repo: PathBuf,
    origin: PathBuf,
    lead: PathBuf,
}

fn fx(status: &str) -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    let origin = env.root().join("repos/origin.git");
    env.git(env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    env.git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    env.git(&repo, &["push", "-q", "origin", "main"]);
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("lead.log") };
    let lead = create_lead_worktree(&repo, &PlanId::parse("pay").unwrap(), "main", &setup).unwrap().path;
    env.git(&lead, &["config", "user.name", "cpd test"]);
    env.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
    env.git(&lead, &["push", "-q", "-u", "origin", "cpd/pay"]);

    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), format!("version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket())).unwrap();
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
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay the bills', '{phase}', 'lead1', 't', 't', {done});
         {pull_request}
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at{columns})
           VALUES ('pay', 'repo', '{repo}', 'main', '{status}', 'cpd/pay', '{lead}', cpd_txn(), 't', 't'{values});",
        repo = repo.display(),
        lead = lead.display(),
    );
    let fx = Fx { env, home, repo, origin, lead };
    fx.store().write("fixture", |tx| Ok(tx.execute_batch(&sql)?)).unwrap();
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn git(&self, dir: &std::path::Path, args: &[&str]) -> String {
        self.env.git(dir, args)
    }

    fn push(&self) -> Run {
        self.env.cpd(&["pr", "push", "--plan", "pay"])
    }

    /// Commits `name` on the Lead Branch; returns the new tip.
    fn commit(&self, name: &str) -> String {
        fs::write(self.lead.join(name), name).unwrap();
        self.git(&self.lead, &["add", "-A"]);
        self.git(&self.lead, &["commit", "-q", "-m", name]);
        self.lead_sha()
    }

    /// Commits the Lead worktree's `.config/wt.toml`.
    fn hooks(&self, toml: &str) -> String {
        fs::create_dir_all(self.lead.join(".config")).unwrap();
        fs::write(self.lead.join(".config/wt.toml"), toml).unwrap();
        self.git(&self.lead, &["add", "-A"]);
        self.git(&self.lead, &["commit", "-q", "-m", "hooks"]);
        self.lead_sha()
    }

    fn lead_sha(&self) -> String {
        self.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay"])
    }

    fn remote_sha(&self) -> String {
        self.git(&self.origin, &["rev-parse", "refs/heads/cpd/pay"])
    }

    fn gate_log(&self) -> String {
        fs::read_to_string(self.home.plan_dir(&plan()).join("logs/pr-repo.pre-merge.log")).unwrap_or_default()
    }

    /// `cpd pr push --plan pay`, started in the background.
    fn spawn_push(&self) -> Child {
        Child(Some(self.env.command(CPD).args(["pr", "push", "--plan", "pay"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()))
    }

    /// Job `m` of the PR with Attempt `m-a1` in `merging`, as `attempt submit` leaves it: its
    /// worktree cut from the Lead Branch, with one commit writing `m.txt`.
    fn submitted(&self) -> AttemptId {
        let attempt = AttemptId::parse("m-a1").unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("attempt.log") };
        let made = create_attempt_worktree(&self.repo, &self.lead, &plan(), &attempt, &setup).unwrap();
        fs::write(made.path.join("m.txt"), "m\n").unwrap();
        self.git(&made.path, &["add", "-A"]);
        self.git(&made.path, &["commit", "-q", "-m", "m"]);
        let sha = self.git(&made.path, &["rev-parse", "HEAD"]);
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at)
                     VALUES ('pay', 'repo', 'm', 'M', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, ?1, ?2, 't')",
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
        let catalog = Catalog { pi_version: "1.2.0".to_owned(), o: model("p/opus"), s: model("p/sonnet"), h: model("p/haiku") };
        let ctx = Ctx { home: &self.home, config: &config, catalog: &catalog, source: "attempt run", metrics: &|_| None };
        merge::merge(&mut self.store(), &ctx, &plan(), attempt).unwrap().end
    }

    /// Moves `cpd/pay` on `origin` by a commit the Lead Branch lacks, through a second clone.
    fn move_remote(&self) -> String {
        let clone = self.env.root().join("clone");
        self.git(self.env.root(), &["clone", "-q", self.origin.to_str().unwrap(), clone.to_str().unwrap()]);
        self.git(&clone, &["config", "user.name", "someone"]);
        self.git(&clone, &["config", "user.email", "someone@cpd.invalid"]);
        self.git(&clone, &["checkout", "-q", "cpd/pay"]);
        fs::write(clone.join("remote.txt"), "remote\n").unwrap();
        self.git(&clone, &["add", "-A"]);
        self.git(&clone, &["commit", "-q", "-m", "remote"]);
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

/// A child process, killed when dropped: a test that panics leaves none behind.
struct Child(Option<std::process::Child>);

impl Child {
    /// Its stdout as JSON, once it has exited.
    fn finish(mut self) -> Value {
        let out = self.0.take().unwrap().wait_with_output().unwrap();
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)))
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A `pre-merge` paused mid-run: it prints the gated commit and its files, creates
/// `<root>/gate-started`, waits for `<root>/gate-go` (at most 60 s), then prints
/// them again.
fn paused_gate(root: &Path) -> String {
    let (started, go) = (root.join("gate-started"), root.join("gate-go"));
    format!(
        "pre-merge = \"git rev-parse HEAD; ls; touch {}; i=0; while [ ! -e {} ] && [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done; git rev-parse HEAD; ls\"\n",
        started.display(),
        go.display()
    )
}

/// Waits until the [`paused_gate`] has started.
fn await_gate(root: &Path) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !root.join("gate-started").exists() {
        assert!(Instant::now() < deadline, "the Gate never started");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Lets the [`paused_gate`] finish.
fn release_gate(root: &Path) {
    fs::write(root.join("gate-go"), "").unwrap();
}

#[test]
fn push_gates_then_pushes_the_lead_branch() {
    let fx = fx("open");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
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
    fx.hooks("pre-merge = \"echo gated >> \\\"$(git rev-parse --path-format=absolute --git-common-dir)/gated\\\"\"\n");
    let tip = fx.lead_sha();
    fx.push().ok();

    let out = fx.push().ok();

    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "pushed": false, "sha": tip, "remote": tip}));
    let gated = fx.git(&fx.lead, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let gated = fs::read_to_string(PathBuf::from(gated).join("gated")).unwrap();
    assert_eq!(gated.lines().count(), 2, "the Gate ran both times");
}

#[test]
fn push_with_a_failed_gate_pushes_nothing() {
    let fx = fx("open");
    let before = fx.remote_sha();
    fx.hooks("pre-merge = \"echo the suite is red; exit 3\"\n");

    let run = fx.push();

    assert_eq!(code(&run), ("gate_failed".into(), 3));
    assert!(run.error()["details"]["tail"].as_str().unwrap().contains("the suite is red"));
    assert!(run.error()["details"]["log"].as_str().unwrap().ends_with("logs/pr-repo.pre-merge.log"));
    assert_eq!(fx.remote_sha(), before);
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
    let gated = fx.hooks(&paused_gate(fx.env.root()));
    let attempt = fx.submitted();

    let child = fx.spawn_push();
    await_gate(fx.env.root());
    // The merge runs to its end while the Gate is paused: no lock to wait for.
    let end = fx.merge(&attempt);
    let End::Merged { sha: merged } = end else { panic!("{end:?}") };
    release_gate(fx.env.root());
    let out = child.finish();

    // The Gate saw G, twice the same: the merge did not change its files.
    let log = fx.gate_log();
    let (first, second) = log.split_at(log.find(&format!("\n{gated}\n")).unwrap_or_else(|| panic!("{log}")) + 1);
    assert!(first.starts_with(&format!("{gated}\n")) && !first.contains("m.txt"), "{log}");
    assert_eq!(first, second, "{log}");
    // The merge pushed its own commit (rule P), which contains G: `pr push` pushed nothing.
    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "pushed": false, "sha": gated, "remote": merged}));
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
    fx.hooks(&paused_gate(fx.env.root()));

    let child = fx.spawn_push();
    await_gate(fx.env.root());
    fx.store()
        .write("pr poll", |tx| {
            tx.execute_batch("UPDATE prs SET status = 'merged', merged_at = 't' WHERE plan = 'pay'")?;
            Ok(())
        })
        .unwrap();
    release_gate(fx.env.root());
    let out = child.finish();

    assert_eq!((out["error"]["code"].as_str(), out["error"]["details"]["from"].as_str()), (Some("illegal_transition"), Some("merged")), "{out}");
    assert_eq!(fx.remote_sha(), before, "nothing pushed");
}
