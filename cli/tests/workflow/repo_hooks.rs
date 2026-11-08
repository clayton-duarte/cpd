//! Hooks come from the repo policy file's `[hooks]` (contract §7.3 "Hooks from the repo policy file"), never from
//! `.config/wt.toml`: `pre-start` of a started PR (§7.2), the quick check at `attempt submit` (commands.md §6.1),
//! the Gate of `pr push` (§9.9, §7.6) and the Worker Spec's quick-check text (§16.1). `cpd` in-process on `run_with`
//! over the fake ports (the git route with a bare `origin`; the hooks' `sh` is a rule of the `FakeRunner`): every hook
//! proves it ran by the `sh -c <command>` call it made.

use crate::common;

use std::fs;
use std::ops::Deref;
use std::path::PathBuf;
use std::time::Duration;

use common::fx::Fx;
use common::refused;
use cpd_core::attempt::ledger;
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::driver::worker::render_spec;
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Reply};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::Ctx;
use cpd_core::store::Store;
use serde_json::json;

const URL: &str = "https://github.com/acme/pay/pull/812";
const EMPTY_SEARCH: &str = r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#;
/// What `cpd watch --ensure` prints when the engine starts.
const WATCH_OK: &str = r#"{"ok": true, "started": true, "pane": "%9"}"#;

fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

/// `[hooks]` text of the repo file with one `pre-commit`/`pre-merge` command line each.
fn hooks_file(pre_commit: &str, pre_merge: &str) -> String {
    format!("[hooks]\npre-merge = \"{pre_merge}\"\n[hooks.pre-commit]\ncheck = \"{pre_commit}\"\n")
}

/// The commands the hooks were run with (`sh -c <command>`), in order.
fn commands(fx: &Fx) -> Vec<String> {
    fx.calls("sh")
        .iter()
        .map(|call| call.args[1].clone())
        .collect()
}

/// A bare `origin` for `repo` holding `main`: the GitHub URL of the repo resolves to it.
fn with_origin(fx: &Fx, repo: &std::path::Path) {
    let origin = fx.env.root().join("origins/pay.git");
    fs::create_dir_all(&origin).unwrap();
    fx.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    fx.local_origin(repo, &origin);
    fx.git(repo, &["push", "-q", "origin", "main"]);
}

// ---------- a started PR's Lead worktree (pre-start) ----------

/// Plan `pay` in phase `plan` with a repo `pay` (keyed `acme/pay`, with its bare `origin`) and one PR `api`,
/// ready for `plan lock`. It derefs to the [`Fx`].
struct Start {
    fx: Fx,
    repo: PathBuf,
}

impl Deref for Start {
    type Target = Fx;

    fn deref(&self) -> &Fx {
        &self.fx
    }
}

fn start_fx() -> Start {
    let fx = Fx::with_ports();
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[watch]\ntick_seconds = {}\n[tmux]\nsocket = \"{}\"\n",
        common::TICK_SECONDS,
        fx.env.tmux_socket()
    ));
    let _ = (*fx.fakes.runner).clone().on(
        "cpd",
        |call: &Call| call.args == ["watch", "--ensure"],
        Reply::ok(WATCH_OK),
    );
    fx.fake_gh(Reply::ok(EMPTY_SEARCH));
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'plan', 'lead1', 't');",
    );
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    with_origin(&fx, &repo);
    fx.cpd(&[
        "workflow",
        "add",
        "api",
        "--repo",
        repo.to_str().unwrap(),
        "--title",
        "Change the PR",
        "--shape",
        "config",
        "--plan",
        "pay",
    ])
    .ok();
    fx.cpd(&[
        "job",
        "cancel",
        "api-config",
        "--reason",
        "a stub of the template",
        "--plan",
        "pay",
    ])
    .ok();
    fx.env.sql(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
         VALUES ('pay', 'api', 'schema', 'Job schema', 'S2', 'g', 'c', '[\"a.txt\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't');",
    );
    Start { fx, repo }
}

impl Start {
    /// Approves the Plan and locks it: the PR `api` is started.
    fn lock(&self) {
        self.outline_for_review("pay");
        let review = self
            .cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
            .ok();
        let id = review["review"]["id"].as_str().unwrap().to_owned();
        self.cpd(&["plan", "decide", &id, "--approve", "--plan", "pay"])
            .ok();
        self.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    }

    fn lead_path(&self) -> PathBuf {
        self.repo.parent().unwrap().join("pay.pay.api")
    }

    /// Commits `.config/wt.toml` to `main` and pushes it.
    fn commit_wt_toml(&self, text: &str) {
        self.commit_file(&self.repo, ".config/wt.toml", text, "wt.toml");
        self.git(&self.repo, &["push", "-q", "origin", "main"]);
    }
}

// ---------- a PR with a Lead worktree and an Attempt (quick check, Gate) ----------

/// A repo `pay` (keyed `acme/pay`) with a bare `origin`, the Lead worktree of Plan `pay` (branch `cpd/pay`) pushed to
/// it, PR `repo` of the Plan (name `pay`, in `status`) and Job `api` with Attempt `api-a1` (`running`) in its worktree.
/// It derefs to the [`Fx`].
struct Core {
    fx: Fx,
    repo: PathBuf,
    origin: PathBuf,
    lead: PathBuf,
    work: PathBuf,
}

impl Deref for Core {
    type Target = Fx;

    fn deref(&self) -> &Fx {
        &self.fx
    }
}

fn core(status: &str) -> Core {
    let fx = Fx::with_ports();
    fx.env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    let origin = fx.env.root().join("origins/pay.git");
    fx.git(
        fx.env.root(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    fx.local_origin(&repo, &origin);
    for (key, value) in [
        ("user.name", "cpd test"),
        ("user.email", "test@cpd.invalid"),
        ("commit.gpgsign", "false"),
    ] {
        fx.git(&repo, &["config", key, value]);
    }
    fx.git(&repo, &["push", "-q", "origin", "main"]);
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("setup.log"),
    };
    let lead = fx
        .scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup))
        .unwrap()
        .path;
    fx.git(&lead, &["push", "-q", "-u", "origin", "cpd/pay"]);
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n",
        fx.env.tmux_socket()
    ));
    let (phase, extra_columns, extra_values, pull_request) = match status {
        "dispatch" => ("dispatch", "", String::new(), String::new()),
        "open" => (
            "deploy",
            ", pr_number, pr_url, pr_opened_at, pull_request",
            format!(", 812, '{URL}', 't', (SELECT id FROM pull_requests)"),
            format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                 VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, 'open', 'none', 'none', 't', 't');"
            ),
        ),
        other => panic!("no status {other}"),
    };
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay the bills', '{phase}', 'lead1', 't', 't');
         {pull_request}
         INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at{extra_columns})
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', '{status}', 'cpd/pay', '{lead}', cpd_txn(), 't', 't'{extra_values});",
        repo = repo.display(),
        lead = lead.display(),
    ));
    let mut core = Core {
        fx,
        repo,
        origin,
        lead,
        work: PathBuf::new(),
    };
    core.work = core.attempt();
    core
}

impl Core {
    /// Job `api` (`running`) with Attempt `api-a1` (`running`), its worktree cut from the Lead Branch.
    fn attempt(&self) -> PathBuf {
        let attempt = AttemptId::parse("api-a1").unwrap();
        let setup = Setup {
            timeout: Duration::from_secs(30),
            log: self.env.root().join("attempt.log"),
        };
        let made = self
            .scoped(|| create_attempt_worktree(&self.repo, &self.lead, &plan(), &attempt, &setup))
            .unwrap();
        self.env.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pay', 'repo', 'api', 'T', 'S2', 'g', 'c', '[\"a.txt\"]', '[\"b\"]', '[]', 'running', 0, '{}', cpd_txn(), 't');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
               VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', '{}', '{}', '{}', 't');",
            "f".repeat(64),
            made.branch,
            made.path.display(),
            made.base_sha
        ));
        made.path
    }

    /// Commits `a.txt` in the Attempt worktree.
    fn commit_work(&self) {
        self.commit_file(&self.work, "a.txt", "a\n", "edit a.txt");
    }

    fn submit(&self) -> common::Run {
        self.cpd_in(
            &self.work,
            &["attempt", "submit", "api-a1", "--plan", "pay"],
        )
    }

    fn push(&self) -> common::Run {
        self.cpd(&["pr", "push", "--plan", "pay"])
    }

    fn attempt_status(&self) -> String {
        self.env
            .query("SELECT status FROM attempts WHERE id = 'api-a1'")
    }

    /// Ends the fixture Attempt `api-a1`, so that `pr push` does not refuse `attempt_running` and the Gate is reached.
    fn end_attempt(&self) {
        self.env.sql("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE plan = 'pay' AND id = 'api-a1'");
    }

    fn lead_sha(&self) -> String {
        self.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay"])
    }

    fn remote_sha(&self) -> String {
        self.git(&self.origin, &["rev-parse", "refs/heads/cpd/pay"])
    }

    /// Commits a new file on the Lead Branch, so that there is something to push.
    fn commit_lead(&self) {
        self.commit_file(&self.lead, "more.txt", "more\n", "more");
    }

    /// Commits `.config/wt.toml` on the Lead Branch.
    fn commit_wt_toml(&self, text: &str) {
        self.commit_file(&self.lead, ".config/wt.toml", text, "wt.toml");
    }

    /// The merge (A8) of `api-a1`, in this process, with the same store and config the driver uses. The Gate at the
    /// merge (contract §7.4 step 6, hs-d679) is `gate.merge` (`pre-commit`) in the Attempt worktree at HEAD.
    fn merge(&self) -> cpd_core::merge::Merge {
        let home = CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap();
        let config = Config::load(&home).unwrap();
        let model = |m: &str| TierModel {
            model: m.to_owned(),
            efforts: vec![0, 1, 2, 3, 4, 5],
        };
        let catalog = Catalog {
            j: cpd_core::config::TierModel {
                model: String::new(),
                efforts: Vec::new(),
            },
            pi_version: "1.0.0".to_owned(),
            o: model("p/opus"),
            s: model("p/sonnet"),
            h: model("p/haiku"),
            g: TierModel {
                model: "p/gemini".to_owned(),
                efforts: vec![],
            },
        };
        let ctx = Ctx {
            home: &home,
            config: &config,
            catalog: &catalog,
            source: "hooks test",
            metrics: &|_| None,
            classifier: cpd_core::merge::no_verdict_classifier(),
        };
        let attempt = AttemptId::parse("api-a1").unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        self.scoped(|| cpd_core::merge::merge(&mut store, &ctx, &plan(), &attempt))
            .unwrap()
    }
}

// ---------- the tests ----------

#[test]
fn pre_start_of_the_repo_file_runs_in_the_lead_worktree_of_a_started_pr() {
    let fx = start_fx();
    fx.env.repo_policy(
        "acme",
        "pay",
        "[hooks]\npre-start = \"touch ran-pre-start\"\n",
    );
    fx.fake("sh", Reply::ok(""));

    fx.lock();

    let calls = fx.calls("sh");
    assert_eq!(
        commands(&fx),
        ["touch ran-pre-start"],
        "the repo file's pre-start did not run"
    );
    assert_eq!(
        calls[0].cwd.as_deref(),
        Some(fx.lead_path().as_path()),
        "the repo file's pre-start ran elsewhere than in {}",
        fx.lead_path().display()
    );
}

#[test]
fn the_merge_gate_is_the_pre_commit_of_the_repo_file() {
    let fx = core("dispatch");
    // The merge gate is the repo's `pre-commit` (the plan's A4/D5, hs-d679); the contract's §7.4 says
    // `pre-merge`. The test pins the merge-time behavior, so it sets both and passes whichever runs.
    let hook = "touch merge-gate-ran";
    fx.env.repo_policy("acme", "pay", &hooks_file(hook, hook));
    fx.commit_work();
    fx.fake("sh", Reply::ok(""));

    // Submit runs no quick check any more: the repo file's pre-commit is the merge gate, not the submit check.
    fx.submit().ok();
    assert_eq!(
        commands(&fx),
        Vec::<String>::new(),
        "submit ran the repo file's pre-commit (the quick check)"
    );
    assert_eq!(fx.attempt_status(), "merging");

    fx.merge();

    assert_eq!(
        commands(&fx),
        [hook],
        "the repo file's pre-commit did not run at the Job merge"
    );
}

#[test]
fn the_gate_runs_the_pre_merge_of_the_repo_file_before_pr_push() {
    let fx = core("open");
    let gate = "touch gate-ran";
    fx.env.repo_policy("acme", "pay", &hooks_file("true", gate));
    fx.fake("sh", Reply::ok(""));
    fx.commit_lead();
    assert_ne!(fx.remote_sha(), fx.lead_sha());
    fx.end_attempt();

    let out = fx.push().ok();

    assert_eq!(
        commands(&fx),
        [gate],
        "the repo file's pre-merge did not run"
    );
    assert_eq!(out["pushed"], json!(true));
    assert_eq!(fx.remote_sha(), fx.lead_sha());
}

#[test]
fn a_failing_pre_merge_of_the_repo_file_refuses_pr_push_with_gate_failed() {
    let fx = core("open");
    fx.env.repo_policy(
        "acme",
        "pay",
        &hooks_file("true", "echo the suite is red; exit 1"),
    );
    fx.fake("sh", Reply::exit(1, "the suite is red\n", ""));
    fx.commit_lead();
    let before = fx.remote_sha();
    fx.end_attempt();

    let error = refused(&fx.push(), "gate_failed", 3);

    assert!(
        error["details"]["tail"]
            .as_str()
            .unwrap()
            .contains("the suite is red"),
        "{error}"
    );
    assert_eq!(fx.remote_sha(), before, "nothing is pushed");
}

#[test]
fn a_wt_toml_in_the_repo_tree_runs_no_hook() {
    let wt_toml = "pre-start = \"touch wt-pre-start\"\npre-merge = \"touch wt-pre-merge\"\n[pre-commit]\ncheck = \"touch wt-pre-commit\"\n";

    // pre-start: the tree's `.config/wt.toml` is on `main` when the PR starts.
    // (Its `Fx` is dropped before the next: one `Fx` holds the process-wide lock at a time.)
    {
        let start = start_fx();
        start.commit_wt_toml(wt_toml);
        start.lock();
        assert!(start.lead_path().exists(), "the PR started");
        assert_eq!(
            commands(&start),
            Vec::<String>::new(),
            "the tree's pre-start ran"
        );
    }

    // pre-commit and pre-merge: the tree's file is on the Lead Branch, the Attempt is cut from it.
    let fx = core("open");
    fx.commit_wt_toml(wt_toml);
    let attempt = fx.attempt_after_wt_toml();
    fx.commit_file(&attempt, "b.txt", "b\n", "b");
    fx.cpd_in(
        &attempt,
        &["attempt", "submit", "api-b-a1", "--plan", "pay"],
    )
    .ok();
    assert_eq!(
        commands(&fx),
        Vec::<String>::new(),
        "the tree's pre-commit ran"
    );
    fx.end_attempt();
    fx.push().ok();
    assert_eq!(
        commands(&fx),
        Vec::<String>::new(),
        "the tree's pre-merge ran"
    );
}

impl Core {
    /// Job `api-b` with Attempt `api-b-a1` (`running`), cut from the Lead Branch as it is now.
    fn attempt_after_wt_toml(&self) -> PathBuf {
        let attempt = AttemptId::parse("api-b-a1").unwrap();
        let setup = Setup {
            timeout: Duration::from_secs(30),
            log: self.env.root().join("attempt-b.log"),
        };
        let made = self
            .scoped(|| create_attempt_worktree(&self.repo, &self.lead, &plan(), &attempt, &setup))
            .unwrap();
        self.env.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pay', 'repo', 'api-b', 'T', 'S2', 'g', 'c', '[\"b.txt\"]', '[\"b\"]', '[]', 'running', 0, '{}', cpd_txn(), 't');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
               VALUES ('pay', 'api-b-a1', 'api-b', 1, 'S2', 'ladder', 'running', '{}', '{}', '{}', 't');",
            "e".repeat(64),
            made.branch,
            made.path.display(),
            made.base_sha
        ));
        made.path
    }
}

// ---------- the Worker Spec ----------

#[test]
fn the_worker_spec_lists_the_pre_commit_commands_of_the_repo_file() {
    // `render_spec` is what `job dispatch`'s driver calls for a Job's Attempt: it reads the store and the policy files
    // under `$CPD_HOME` and writes the Spec. No git, tmux, pi or driver is involved, so none is started.
    let fx = Fx::new();
    fx.env.repo_policy("acme", "pay", "[hooks.pre-commit]\nlint = \"make lint-from-the-repo-file\"\nbuild = \"make build-from-the-repo-file\"\n");
    let worktree = fx.env.root().join("worktrees/spec-a1");
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'dispatch', 'lead1', 't', 't');
         INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '/repos/pay', 'pay', 'main', 'dispatch', 'cpd/pay', '/worktrees/lead', cpd_txn(), 't', 't');
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
           VALUES ('pay', 'repo', 'spec', 'Title', 'S2', 'Do it.', 'Because.', '[\"spec.txt\"]', '[\"it works\"]', '[]', 'running', 0, '{}', cpd_txn(), 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
           VALUES ('pay', 'spec-a1', 'spec', 1, 'S2', 'ladder', 'running', 'cpd/pay--spec-a1', '{}', '{}', 't');",
        "e".repeat(64),
        worktree.display(),
        "0".repeat(40),
    ));
    let home = cpd_core::home::CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    let mut store = cpd_core::store::Store::open(&home, 5000).unwrap();
    let path = store
        .read(|conn| {
            let target = ledger::load(conn, &plan(), &AttemptId::parse("spec-a1").unwrap())?;
            let jobs = cpd_core::plan::jobs::load(conn, &plan())?;
            let job = jobs
                .iter()
                .find(|j| j.key == "spec")
                .expect("the Job `spec`");
            render_spec(conn, &home, &target, job)
        })
        .unwrap();
    assert_eq!(path, fx.env.cpd_home().join("run/pay/specs/spec-a1.md"));
    let spec = fs::read_to_string(&path).unwrap();
    assert!(spec.contains("make lint-from-the-repo-file"), "{spec}");
    assert!(spec.contains("make build-from-the-repo-file"), "{spec}");
}
