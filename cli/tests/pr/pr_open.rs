//! The `open` Job's merge and the PR gate (contract §7.4 step 6, §7.7; hs-d679). `pr open`
//! itself is the repair and queues the opener; the **merge** of the `open` Job runs `gate.pr`,
//! the repo's `pre-merge`, in its Attempt worktree at HEAD, before the push and the pull
//! request. Submit runs no Gate. The old `pr open`/submit timing is rewritten here: the Gate
//! that matters for an `open` Job runs at its merge, not at `pr open` or `attempt submit`.
//!
//! The built binary runs `attempt submit`; the merge is `cpd_core::merge::merge` (A8) on the
//! same store. The tests read the store, never a Rust cause variant, so the branch builds and
//! the tests fail for the missing behavior.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use common::fx::Fx as Lock;
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Ctx};
use cpd_core::store::Store;

const PLAN: &str = "pay";
const ATTEMPT: &str = "open-repo-a1";

fn plan() -> PlanId {
    PlanId::parse(PLAN).unwrap()
}

/// A repo `pay` (keyed `acme/pay`), the Lead worktree of Plan `pay`, PR `repo` (`dispatch`,
/// title set) and its `open` Job `open-repo` with Attempt `open-repo-a1` (`running`).
struct Fx {
    /// Holds the process's environment (`PATH` with the env's fake `gh`) for the test: `merge` runs in this process.
    fx: Lock,
    home: CpdHome,
    repo: PathBuf,
    lead: PathBuf,
    work: PathBuf,
}

fn fixture(policy: &str) -> Fx {
    let fx = Lock::new();
    let env = &fx.env;
    let repo = env.git_repo("pay").canonicalize().unwrap();
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(&repo, &["config", key, value]);
    }
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    env.repo_policy("acme", "pay", policy);
    // No pull request of the Lead Branch yet: `gh pr list` answers none, so the open Job creates one.
    env.fake_gh("case \"$1 $2\" in\n  \"pr list\") echo '[]' ;;\n  *) exit 1 ;;\nesac");
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("setup.log") };
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap().path;
    env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, description, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', 'dispatch', 'cpd/pay', '{lead}', 'Charge the card at checkout', 'Why and how.', cpd_txn(), 't', '2026-10-26T00:00:00.000Z');",
        repo = repo.display(),
        lead = lead.display()
    ));
    let made = create_attempt_worktree(&repo, &lead, &plan(), &AttemptId::parse(ATTEMPT).unwrap(), &setup).unwrap();
    env.sql(&format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
           VALUES ('pay', 'repo', 'open-repo', 'Open the pull request', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 1, 'open', '{}', cpd_txn(), 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
           VALUES ('pay', '{ATTEMPT}', 'open-repo', 1, 'S2', 'ladder', 'running', '{}', '{}', '{}', 't');",
        "f".repeat(64),
        made.branch,
        made.path.display(),
        made.base_sha
    ));
    Fx { fx, home, repo, lead, work: made.path }
}

impl Fx {
    fn submit(&self) -> common::Run {
        self.fx.env.cpd_in(&self.work, &["attempt", "submit", ATTEMPT, "--plan", PLAN])
    }

    fn merge(&self) -> merge::Merge {
        let config = Config::load(&self.home).unwrap();
        let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![0, 1, 2, 3, 4, 5] };
        let catalog = Catalog {
            pi_version: "1.0.0".to_owned(),
            o: model("p/opus"),
            s: model("p/sonnet"),
            h: model("p/haiku"),
            g: TierModel { model: "p/gemini".to_owned(), efforts: vec![] },
        };
        let ctx = Ctx { home: &self.home, config: &config, catalog: &catalog, source: "pr open test", metrics: &|_| None };
        let mut store = Store::open(&self.home, 5000).unwrap();
        merge::merge(&mut store, &ctx, &plan(), &AttemptId::parse(ATTEMPT).unwrap()).unwrap()
    }
}

#[test]
fn the_open_jobs_merge_runs_the_pr_gate() {
    let fx = fixture("");
    let marker = fx.fx.env.root().join("pr-gate-ran");
    fx.fx.env.repo_policy("acme", "pay", &format!("[gate]\npr = \"local\"\n\n[hooks]\npre-merge = \"touch {}\"\n", marker.display()));
    fs::write(fx.work.join("a.txt"), "a\n").unwrap();
    fx.fx.env.git(&fx.work, &["add", "-A"]);
    fx.fx.env.git(&fx.work, &["commit", "-q", "-m", "a"]);

    // Submit runs no Gate: the open Job's Gate is at its merge.
    fx.submit().ok();
    assert!(!marker.exists(), "submit ran the PR gate");

    let _ = fx.merge();

    assert!(marker.exists(), "the open Job's merge did not run the PR gate (the repo's pre-merge)");
}
