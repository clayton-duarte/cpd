//! The Gates at the merge (contract §7.4 step 6, §7.7; hs-d679, Plan `gate-redefine` D5–D7).
//! `gate.merge` (the repo's `pre-commit`) runs at every non-review Job merge, in its
//! Attempt worktree at HEAD, before the fast-forward; `gate.pr` (the repo's `pre-merge`)
//! runs only at an `open`/`update` Job merge. Both run while the Attempt is `merging`
//! (the tree shows it yellow). A gate that exits non-zero ends the Attempt `failed` /
//! `gate_failed` and the Job `failed`; one that cannot be judged ends them `gate_error`.
//! `attempt submit` runs no quick check and no Gate: it runs only the tamper check, the
//! red-test guard and the kind's `[test-commands]`.
//!
//! The built binary runs `attempt submit` against a temporary repo; the merge is
//! [`cpd_core::merge::merge`], A8 itself, on the same store. The tests read the CLI JSON
//! and the store, never a Rust cause variant, so the branch builds before the behavior
//! exists and the tests fail for the missing behavior.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{Run, TestEnv};
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::error::CpdError;
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Ctx};
use cpd_core::store::Store;
use serde_json::json;

const PLAN: &str = "pay";
const ATTEMPT: &str = "api-a1";

fn plan() -> PlanId {
    PlanId::parse(PLAN).unwrap()
}

fn attempt() -> AttemptId {
    AttemptId::parse(ATTEMPT).unwrap()
}

// ---------- fixture ----------

/// A repo `pay` (keyed `acme/pay`), the Lead worktree of Plan `pay` (branch `cpd/pay`),
/// PR `repo` of the Plan (`dispatch`, with a stored title for an `open` Job) and Job
/// `api` (`running`) with Attempt `api-a1` (`running`) in its own worktree, cut from the
/// Lead Branch. The repo policy file is `policy`; the machine config is minimal.
struct Fx {
    env: TestEnv,
    home: CpdHome,
    repo: PathBuf,
    lead: PathBuf,
    work: PathBuf,
}

fn fixture(kind: &str, policy: &str) -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(&repo, &["config", key, value]);
    }
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    env.repo_policy("acme", "pay", policy);
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("setup.log") };
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap().path;
    env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', 'dispatch', 'cpd/pay', '{lead}', 'Charge the card at checkout', cpd_txn(), 't', 't');",
        repo = repo.display(),
        lead = lead.display()
    ));
    let mut fx = Fx { env, home, repo, lead, work: PathBuf::new() };
    fx.work = fx.attempt(kind);
    fx
}

/// A work Job `api` with Attempt `api-a1` (`running`), its worktree cut from the Lead Branch.
fn fx() -> Fx {
    fixture("work", "")
}

impl Fx {
    fn attempt(&self, kind: &str) -> PathBuf {
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("attempt.log") };
        let made = create_attempt_worktree(&self.repo, &self.lead, &plan(), &attempt(), &setup).unwrap();
        self.env.sql(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
               VALUES ('pay', 'repo', 'api', 'Title of api', 'S2', 'Do api.', 'Because.', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{kind}', '{}', cpd_txn(), 't');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
               VALUES ('pay', '{ATTEMPT}', 'api', 1, 'S2', 'ladder', 'running', '{branch}', '{work}', '{base}', 't');",
            "f".repeat(64),
            branch = made.branch,
            work = made.path.display(),
            base = made.base_sha
        ));
        made.path
    }

    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    /// Commits `name` in the Attempt worktree; returns the new tip.
    fn commit(&self, name: &str, body: &str) -> String {
        fs::write(self.work.join(name), body).unwrap();
        self.env.git(&self.work, &["add", "-A"]);
        self.env.git(&self.work, &["commit", "-q", "-m", name]);
        self.env.git(&self.work, &["rev-parse", "HEAD"])
    }

    /// `attempt submit api-a1 --plan pay` in the Attempt worktree, as the built binary runs it.
    fn submit(&self) -> Run {
        self.env.cpd_in(&self.work, &["attempt", "submit", ATTEMPT, "--plan", PLAN])
    }

    /// Driver step A8 for `api-a1`, in this process, with the same store and config the driver uses.
    fn merge(&self) -> Result<merge::Merge, CpdError> {
        let config = Config::load(&self.home).unwrap();
        let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![0, 1, 2, 3, 4, 5] };
        let catalog = Catalog {
            pi_version: "1.0.0".to_owned(),
            o: model("p/opus"),
            s: model("p/sonnet"),
            h: model("p/haiku"),
            g: TierModel { model: "p/gemini".to_owned(), efforts: vec![] },
        };
        let ctx = Ctx { home: &self.home, config: &config, catalog: &catalog, source: "gate test", metrics: &|_| None };
        merge::merge(&mut self.store(), &ctx, &plan(), &attempt())
    }

    /// The Attempt's `status`.
    fn status(&self) -> String {
        self.env.query("SELECT status FROM attempts WHERE id = 'api-a1'")
    }

    /// The Attempt's `outcome|cause`, after it ended.
    fn ended(&self) -> String {
        self.env.query("SELECT ifnull(outcome, '') || '|' || ifnull(cause, '') FROM attempts WHERE id = 'api-a1'")
    }

    /// The Job's `status|ifnull(failed_cause, '')`, after it ended.
    fn job(&self) -> String {
        self.env.query("SELECT status || '|' || ifnull(failed_cause, '') FROM jobs WHERE plan = 'pay' AND key = 'api'")
    }

    /// The repo policy file's `[gate]` and one hook, plus the machine config's short gate timeout so a
    /// paused Gate ends by itself if a test fails to release it.
    fn policy(&self, toml: &str) {
        self.env.repo_policy("acme", "pay", toml);
    }

    /// A marker under the env's root, outside the worktree.
    fn marker(&self, name: &str) -> PathBuf {
        self.env.root().join(name)
    }
}

/// `[gate] merge = "local"` with `hook` as the merge hook. The plan's A4 names the repo's
/// `pre-commit` (D5), but the contract's §7.4 says `pre-merge`; the test pins the *merge-time*
/// behavior, so it sets both and passes whichever the implementation chooses.
fn work_gate(hook: &str) -> String {
    format!("[gate]\nmerge = \"local\"\n\n[hooks]\npre-commit = \"{hook}\"\npre-merge = \"{hook}\"\n")
}

/// `[gate] pr = "local"` with the `pre-merge` command `hook` (the PR gate).
fn pr_gate(hook: &str) -> String {
    format!("[gate]\npr = \"local\"\n\n[hooks]\npre-merge = \"{hook}\"\n")
}

/// A `gh` that finds no pull request of the Lead Branch and fails to create one: enough for an
/// `open` Job's merge to reach (and be judged by) the Gate before any `gh pr create` matters.
fn gh_no_pr(env: &TestEnv) {
    env.fake_gh("case \"$1 $2\" in\n  \"pr list\") echo '[]' ;;\n  *) echo \"unexpected gh call: $*\" >&2; exit 1 ;;\nesac");
}

/// A `pre-commit`/`pre-merge` command paused until `<name>-go` appears (at most 30 s), so a test can
/// read the state while the Gate runs. Absolute paths: the hook runs with cwd the Attempt worktree.
fn paused(root: &Path, name: &str) -> String {
    let started = root.join(format!("{name}-started"));
    let go = root.join(format!("{name}-go"));
    format!(
        "touch {s}; i=0; while [ ! -e {g} ] && [ $i -lt 300 ]; do sleep 0.1; i=$((i+1)); done",
        s = started.display(),
        g = go.display()
    )
}

fn await_paused(root: &Path, name: &str) {
    let started = root.join(format!("{name}-started"));
    common::wait_until_for("the Gate to start", Duration::from_secs(60), || started.exists());
}

fn release_paused(root: &Path, name: &str) {
    fs::write(root.join(format!("{name}-go")), "").unwrap();
}

// ---------- the work gate (`gate.merge`, the repo's `pre-commit`) ----------

#[test]
fn the_merge_gate_runs_the_pre_commit_at_a_job_merge() {
    let fx = fx();
    let marker = fx.marker("merge-gate-ran");
    fx.policy(&work_gate(&format!("touch {}", marker.display())));
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
    // Whatever submit did, the gate is what the merge runs: clear and see it appear.
    let _ = fs::remove_file(&marker);

    fx.merge().unwrap();

    assert!(marker.exists(), "the merge gate (the repo's pre-commit) did not run at the Job merge");
    assert_eq!(fx.ended(), "merged|merged");
}

#[test]
fn the_merge_gate_failure_fails_the_job_with_gate_failed() {
    let fx = fx();
    fx.policy(&work_gate("echo the merge gate is red; exit 3"));
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");

    let end = fx.merge().unwrap();

    assert_eq!(end.end.outcome(), "failed", "{end:?}");
    assert_eq!(fx.ended(), "failed|gate_failed", "the merge gate failure is the Attempt's cause");
    assert_eq!(fx.job(), "failed|gate_failed");
}

#[test]
fn the_gate_runs_while_the_attempt_is_merging_yellow() {
    let fx = fx();
    let hook = paused(fx.env.root(), "work-gate");
    fx.policy(&work_gate(&hook));
    fx.commit("a.txt", "a\n");

    std::thread::scope(|scope| {
        scope.spawn(|| {
            fx.submit().ok();
            let _ = fx.merge();
        });
        await_paused(fx.env.root(), "work-gate");
        // The tree is yellow while the Gate runs: the Attempt is `merging`, not `running` or `ended`.
        let status = fx.status();
        release_paused(fx.env.root(), "work-gate");
        assert_eq!(status, "merging", "the Gate did not run under the yellow merging state");
    });
}

#[test]
fn a_gate_that_cannot_run_fails_the_job_with_gate_error() {
    let fx = fx();
    // A template variable that does not exist: the hook cannot be rendered, so it cannot be judged.
    fx.policy(&work_gate("{{ no_such_gate_variable }}"));
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");

    let end = fx.merge().unwrap();

    assert_eq!(end.end.outcome(), "failed", "{end:?}");
    assert_eq!(fx.ended(), "failed|gate_error", "a gate that cannot run is the Attempt's cause");
    assert_eq!(fx.job(), "failed|gate_error");
}

// ---------- submit runs no quick check and no Gate ----------

#[test]
fn the_submit_runs_no_quick_check_and_no_gate() {
    let fx = fx();
    let quick = fx.marker("submit-quick-check");
    let gate = fx.marker("submit-gate");
    fx.policy(&format!(
        "[gate]\nmerge = \"local\"\npr = \"local\"\n\n[hooks]\npre-commit = \"touch {}\"\npre-merge = \"touch {}\"\n",
        quick.display(),
        gate.display()
    ));
    fx.commit("a.txt", "a\n");

    let out = fx.submit().ok();

    assert_eq!(out["submitted"], json!(true), "{out}");
    assert_eq!(fx.status(), "merging");
    assert!(!quick.exists(), "submit ran the quick check (the repo's pre-commit)");
    assert!(!gate.exists(), "submit ran a Gate (the repo's pre-merge)");
}

// ---------- the PR gate (`gate.pr`, the repo's `pre-merge`) ----------

#[test]
fn the_pr_gate_runs_the_pre_merge_at_an_open_job_merge() {
    let fx = fixture("open", "");
    gh_no_pr(&fx.env);
    let marker = fx.marker("pr-gate-ran");
    fx.policy(&pr_gate(&format!("touch {}", marker.display())));
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
    let _ = fs::remove_file(&marker);

    let _ = fx.merge();

    assert!(marker.exists(), "the PR gate (the repo's pre-merge) did not run at the open Job merge");
}

#[test]
fn the_pr_gate_failure_fails_the_open_job_with_gate_failed() {
    let fx = fixture("open", "");
    gh_no_pr(&fx.env);
    fx.policy(&pr_gate("echo the PR gate is red; exit 3"));
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");

    let end = fx.merge().unwrap();

    assert_eq!(end.end.outcome(), "failed", "{end:?}");
    assert_eq!(fx.ended(), "failed|gate_failed", "the PR gate failure is the open Job's cause");
    assert_eq!(fx.job(), "failed|gate_failed");
}
