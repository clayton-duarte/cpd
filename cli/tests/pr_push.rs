//! `pr push` (commands.md §9.9, contract §7.6): the built binary against a temporary
//! repo with a bare `origin` and a Plan written straight into the database.

mod common;

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use common::{Run, TestEnv};
use cpd_core::git::{Setup, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::store::Store;
use serde_json::json;

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
    env.local_origin(&repo, &origin);
    env.git(&repo, &["push", "-q", "origin", "main"]);
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("lead.log") };
    let lead = create_lead_worktree(&repo, &PlanId::parse("pay").unwrap(), "main", &setup).unwrap().path;
    env.git(&lead, &["config", "user.name", "cpd test"]);
    env.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
    env.git(&lead, &["push", "-q", "-u", "origin", "cpd/pay"]);

    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), format!("version = 1\nmachine = \"t\"\n[timeouts]\nlock_wait_seconds = 1\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket())).unwrap();
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
        fs::read_to_string(self.home.plan_dir(&PlanId::parse("pay").unwrap()).join("logs/pr-repo.pre-merge.log")).unwrap_or_default()
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

fn code(run: &Run) -> (String, i32) {
    (run.error()["code"].as_str().unwrap().to_owned(), run.exit)
}

#[test]
fn push_gates_then_pushes_the_lead_branch() {
    let fx = fx("open");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    let tip = fx.commit("more.txt");
    assert_ne!(fx.remote_sha(), tip);

    let out = fx.push().ok();

    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "pushed": true, "sha": tip}));
    assert_eq!(fx.remote_sha(), tip);
    assert_eq!(fx.gate_log().trim(), tip, "the Gate ran on the tip");
    assert_eq!(fx.lead_sha(), tip, "no rebase");
}

#[test]
fn push_again_runs_the_gate_and_pushes_nothing() {
    let fx = fx("open");
    fx.hooks("pre-merge = \"echo gated >> \\\"$(git rev-parse --git-dir)/gated\\\"\"\n");
    let tip = fx.lead_sha();
    fx.push().ok();

    let out = fx.push().ok();

    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "pushed": false, "sha": tip}));
    let gated = fx.git(&fx.lead, &["rev-parse", "--git-dir"]);
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
fn push_refuses_a_dirty_lead_worktree() {
    let fx = fx("open");
    fs::write(fx.lead.join("scratch.txt"), "x").unwrap();

    let run = fx.push();

    assert_eq!(code(&run), ("dirty_target".into(), 3));
}
