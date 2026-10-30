//! The Gates at the merge (contract §7.4 step 6, §7.7; hs-d679, Plan `gate-redefine` D5–D7).
//! `gate.merge` (the repo's `pre-commit`) runs at every Job merge, in its
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
         INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, description, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', 'dispatch', 'cpd/pay', '{lead}', 'Charge the card at checkout', 'A description', cpd_txn(), 't', 't');",
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

// ---------- the next Spec after a Gate cause (contract §3.4, §4.4, §7.4 step 6; commands.md §16.1) ----------

use cpd_core::attempt::ledger;
use cpd_core::driver::worker::render_spec;
use cpd_core::home::Hook;

/// The gate_failed sentence of commands.md §16.1 'Gate causes in Previous Attempts', hook and log known.
fn gate_failed_sentence(hook: &str) -> String {
    format!("The Gate (`{hook}`) failed at its merge. Its whole output is in this log; read it and fix what it reports:")
}

/// The gate_error sentence of the same section, hook and log known.
fn gate_error_sentence(hook: &str) -> String {
    format!(
        "The Gate (`{hook}`) could not be judged at its merge: it was killed, timed out or could not run, which is a problem of the Gate, not a failing check of your work. Its output so far is in this log, if the hook started:"
    )
}

impl Fx {
    /// Attempt 2 of Job `api` (`running`), as a retry of the Attempt that ended.
    fn second_attempt(&self) {
        self.env.sql(&format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
               VALUES ('pay', 'api-a2', 'api', 2, 'S2', 'retry', 'running', 'cpd/pay--api-a2', '{}/api-a2', '{}', 't');",
            self.env.root().display(),
            "0".repeat(40)
        ));
    }

    /// The text of the Spec `render_spec` writes for Attempt `id` of Job `job`.
    fn spec_text(&self, id: &str, job: &str) -> String {
        let mut store = self.store();
        let path = store
            .read(|conn| {
                let target = ledger::load(conn, &plan(), &AttemptId::parse(id).unwrap())?;
                let jobs = cpd_core::plan::jobs::load(conn, &plan())?;
                let job = jobs.iter().find(|j| j.key == job).expect("the Job");
                render_spec(conn, &self.home, &target, job)
            })
            .unwrap_or_else(|e| panic!("the Spec of {id} is not rendered: {} ({:?})", e.message(), e.code()));
        fs::read_to_string(path).unwrap()
    }
}

#[test]
fn every_cause_the_attempts_table_admits_is_rendered_into_the_next_spec() {
    let fx = fx();
    let causes = common::causes::attempt_causes(&fx.env);
    assert!(causes.iter().any(|c| c == "merged") && causes.iter().any(|c| c == "gate_failed"), "the cause list read is {causes:?}");
    // Job `api` has Attempt 1 `running`; each cause gets a Job of its own with an ended Attempt 1 and a running Attempt 2.
    for (i, cause) in causes.iter().enumerate() {
        let job = format!("c{i}");
        let outcome = common::causes::outcome_of(cause);
        let merged = if outcome == "merged" { "'abc123'" } else { "NULL" };
        let worktree = fx.env.root().join(&job);
        fx.env.sql(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
               VALUES ('pay', 'repo', '{job}', 'Title of {job}', 'S2', 'Do it.', 'Because.', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, 'work', '{fp}', cpd_txn(), 't');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha, merged_sha, started_at, ended_at)
               VALUES ('pay', '{job}-a1', '{job}', 1, 'S2', 'ladder', 'ended', '{outcome}', '{cause}', 'cpd/pay--{job}-a1', '{wt}-a1', '{zero}', {merged}, 't', 't');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
               VALUES ('pay', '{job}-a2', '{job}', 2, 'S2', 'retry', 'running', 'cpd/pay--{job}-a2', '{wt}-a2', '{zero}', 't');",
            fp = "f".repeat(64),
            zero = "0".repeat(40),
            wt = worktree.display()
        ));

        let spec = fx.spec_text(&format!("{job}-a2"), &job);

        assert!(spec.contains(&format!("- Cause: {cause}\n")), "the Spec after cause {cause:?} has no Cause line:\n{spec}");
        assert!(spec.contains(&format!("- Outcome: {outcome}\n")), "the Spec after cause {cause:?} has no Outcome line:\n{spec}");
    }
}

#[test]
fn the_next_spec_after_a_merge_gate_failure_names_the_pre_commit_log() {
    let fx = fx();
    fx.policy(&work_gate("echo the merge gate is red; exit 3"));
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    fx.merge().unwrap();
    assert_eq!(fx.ended(), "failed|gate_failed");
    fx.second_attempt();

    let spec = fx.spec_text("api-a2", "api");

    let log = fx.home.hook_log(&plan(), &attempt(), Hook::PreCommit);
    assert!(spec.contains("- Cause: gate_failed\n"), "{spec}");
    assert!(spec.contains(&format!("- {}\n", gate_failed_sentence("pre-commit"))), "{spec}");
    assert!(spec.contains(&format!("\n  - `{}`\n", log.display())), "the Spec does not list the log {} as an item:\n{spec}", log.display());
}

#[test]
fn the_next_spec_after_a_pr_gate_failure_names_the_pre_merge_log() {
    let fx = fixture("open", "");
    gh_no_pr(&fx.env);
    fx.policy(&pr_gate("echo the PR gate is red; exit 3"));
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    fx.merge().unwrap();
    assert_eq!(fx.ended(), "failed|gate_failed");
    fx.second_attempt();

    let spec = fx.spec_text("api-a2", "api");

    let log = fx.home.hook_log(&plan(), &attempt(), Hook::PreMerge);
    assert!(spec.contains("- Cause: gate_failed\n"), "{spec}");
    assert!(spec.contains(&format!("- {}\n", gate_failed_sentence("pre-merge"))), "{spec}");
    assert!(spec.contains(&format!("\n  - `{}`\n", log.display())), "the Spec does not list the log {} as an item:\n{spec}", log.display());
}

#[test]
fn the_next_spec_after_a_gate_error_says_the_gate_could_not_be_judged_and_names_its_log() {
    let fx = fx();
    fx.policy(&work_gate("{{ no_such_gate_variable }}"));
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    fx.merge().unwrap();
    assert_eq!(fx.ended(), "failed|gate_error");
    fx.second_attempt();

    let spec = fx.spec_text("api-a2", "api");

    let log = fx.home.hook_log(&plan(), &attempt(), Hook::PreCommit);
    assert!(spec.contains("- Cause: gate_error\n"), "{spec}");
    assert!(spec.contains(&format!("- {}\n", gate_error_sentence("pre-commit"))), "{spec}");
    assert!(spec.contains(&format!("\n  - `{}`\n", log.display())), "the Spec does not list the log {} as an item:\n{spec}", log.display());
}

// ---------- the Gate facts (Plan gate-fail-facts): a failed Gate writes its facts on its Attempt ----------

impl Fx {
    /// `quote(gate_hook)|quote(gate_command)|quote(gate_exit)|quote(check_tail is null)`; `NULL` is a NULL column.
    fn gate_facts(&self) -> String {
        self.env.query(
            "SELECT quote(gate_hook) || '|' || quote(gate_command) || '|' || quote(gate_exit) || '|' || quote(check_tail IS NULL)
               FROM attempts WHERE id = 'api-a1'",
        )
    }

    fn check_tail(&self) -> Option<String> {
        self.env.query("SELECT check_tail FROM attempts WHERE id = 'api-a1'")
    }

    /// Submits and merges the Attempt with the current policy.
    fn merged(&self) {
        self.commit("a.txt", "a\n");
        self.submit().ok();
        assert_eq!(self.status(), "merging");
        self.merge().unwrap();
    }
}

#[test]
fn a_failed_merge_gate_records_its_hook_command_exit_and_tail() {
    let fx = fx();
    fx.policy(&work_gate("echo the merge gate is red; exit 3"));
    fx.merged();

    assert_eq!(fx.ended(), "failed|gate_failed");
    assert_eq!(fx.gate_facts(), "'pre-commit'|'echo the merge gate is red; exit 3'|3|0");
    let tail = fx.check_tail().expect("check_tail is written");
    assert!(tail.contains("the merge gate is red"), "{tail:?}");
}

#[test]
fn a_failed_pr_gate_records_pre_merge_as_its_hook() {
    let fx = fixture("open", "");
    gh_no_pr(&fx.env);
    fx.policy(&pr_gate("echo the PR gate is red; exit 3"));
    fx.merged();

    assert_eq!(fx.ended(), "failed|gate_failed");
    assert_eq!(fx.gate_facts(), "'pre-merge'|'echo the PR gate is red; exit 3'|3|0");
    let tail = fx.check_tail().expect("check_tail is written");
    assert!(tail.contains("the PR gate is red"), "{tail:?}");
}

#[test]
fn the_recorded_command_is_the_first_failing_one_in_file_order() {
    let fx = fx();
    fx.policy("[gate]\nmerge = \"local\"\n\n[hooks.pre-commit]\nlint = \"true\"\nbuild = \"echo build is red; exit 7\"\ntest = \"echo test is red; exit 9\"\n");
    fx.merged();

    assert_eq!(fx.ended(), "failed|gate_failed");
    assert_eq!(fx.gate_facts(), "'pre-commit'|'echo build is red; exit 7'|7|0");
}

#[test]
fn a_passing_gate_records_no_gate_facts() {
    let fx = fx();
    fx.policy(&work_gate("true"));
    fx.merged();

    assert_eq!(fx.ended(), "merged|merged");
    assert_eq!(fx.gate_facts(), "NULL|NULL|NULL|1");
}

#[test]
fn a_gate_that_cannot_run_records_no_gate_facts() {
    let fx = fx();
    fx.policy(&work_gate("{{ no_such_gate_variable }}"));
    fx.merged();

    assert_eq!(fx.ended(), "failed|gate_error");
    assert_eq!(fx.gate_facts(), "NULL|NULL|NULL|1");
}

#[test]
fn attempt_show_and_job_show_print_the_gate_facts() {
    let fx = fx();
    fx.policy(&work_gate("echo the merge gate is red; exit 3"));
    fx.merged();

    let shown = fx.env.cpd(&["attempt", "show", ATTEMPT, "--plan", PLAN]).ok();
    let job = fx.env.cpd(&["job", "show", "api", "--plan", PLAN]).ok();
    for (what, row) in [("attempt show", &shown["attempt"]), ("job show", &job["attempts"][0])] {
        assert_eq!(row["gate_hook"], json!("pre-commit"), "{what}: {row}");
        assert_eq!(row["gate_command"], json!("echo the merge gate is red; exit 3"), "{what}: {row}");
        assert_eq!(row["gate_exit"], json!(3), "{what}: {row}");
        assert!(row["check_tail"].as_str().is_some_and(|t| t.contains("the merge gate is red")), "{what}: {row}");
    }
}
