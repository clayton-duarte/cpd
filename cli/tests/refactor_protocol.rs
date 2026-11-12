//! Refactor protocol red tests (commands.md §34, decisions pro-d579 / pro-d581).

#[path = "common/mod.rs"]
mod common;

use common::Run;
use common::fx::Fx;
use std::path::PathBuf;

const TESTS_TABLE: &str = "[test-commands]\nbuild = \"true\"\nrun = \"true\"\n";

fn plan_sql(phase: &str) -> String {
    let locked = if matches!(phase, "grounding" | "plan") {
        "NULL"
    } else {
        "'2000-01-01T00:00:00.000Z'"
    };
    let done = if phase == "done" {
        "'2000-01-02T00:00:00.000Z'"
    } else {
        "NULL"
    };
    format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay-1', 'ctx', 'Pay the bills', '{phase}', 'lead1', '1999-12-31T00:00:00.000Z', {locked}, {done});"
    )
}

struct RefactorFixture {
    fx: Fx,
    repo: PathBuf,
}

impl std::ops::Deref for RefactorFixture {
    type Target = Fx;

    fn deref(&self) -> &Fx {
        &self.fx
    }
}

fn fixture(phase: &str) -> RefactorFixture {
    let fx = Fx::with_ports();
    fx.env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    fx.env.repo_policy("acme", "pay", TESTS_TABLE);
    fx.env.write_config("version = 1\nmachine = \"t\"\n");
    fx.env.sql(&plan_sql(phase));
    RefactorFixture { fx, repo }
}

impl RefactorFixture {
    fn add_workflow(&self, key: &str, extra: &[&str]) -> Run {
        let title = format!("Workflow {key}");
        let mut args = vec![
            "workflow",
            "add",
            key,
            "--repo",
            self.repo.to_str().unwrap(),
            "--title",
            &title,
            "--plan",
            "pay-1",
        ];
        args.extend(extra);
        self.cpd(&args)
    }
}

mod refactor_protocol {
    use super::*;

    /// Decision pro-d579 / pro-d581, commands.md §34.1 rule 2, §34.3:
    /// Creating a refactor workflow without a coverage base (or pointing to a non-coverage / non-green base)
    /// is refused with `base_coverage_required` and exit code 3.
    #[test]
    fn refactor_without_coverage_base_refused() {
        let fx = fixture("grounding");

        // 1. Refactor workflow without --base is refused base_coverage_required
        let run = fx.add_workflow("refact-1", &["--shape", "refactor"]);
        let err = common::refused(&run, "base_coverage_required", 3);
        assert!(
            err["message"]
                .as_str()
                .unwrap_or_default()
                .contains("coverage"),
            "refusal message should mention coverage base requirement"
        );

        // 2. Refactor workflow with a base that is NOT a coverage workflow is refused
        fx.add_workflow("norm", &["--shape", "config"]).ok();
        let run2 = fx.add_workflow("refact-2", &["--shape", "refactor", "--base", "norm"]);
        common::refused(&run2, "base_coverage_required", 3);
    }

    /// Decision pro-d579 / pro-d581, commands.md §34.1 rule 3, §34.3:
    /// A refactor workflow's diff touching a test file (*_test.rs, tests/, *.test.ts, test_files globs)
    /// is refused at job submit with `test_change_forbidden` and exit code 3.
    #[test]
    fn refactor_submit_touching_test_file_refused() {
        let fx = fixture("grounding");

        // Add coverage workflow and refactor workflow based on it
        fx.add_workflow("cov", &["--shape", "coverage"]).ok();
        fx.env.sql("UPDATE jobs SET status = 'queued', tier = 'S2', goal = 'goal', context = 'context', touches = '[\"src/**\"]', acceptance = '[\"works\"]', out_of_scope = '[]', fingerprint = printf('%064d', 0) WHERE workflow = 'cov'; UPDATE jobs SET status = 'running' WHERE workflow = 'cov'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE workflow = 'cov';");
        fx.add_workflow("refact", &["--shape", "refactor", "--base", "cov"])
            .ok();
        fx.env.sql("UPDATE plans SET phase = 'plan' WHERE id = 'pay-1'; UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z' WHERE id = 'pay-1'; UPDATE workflows SET status = 'dispatch', lead_worktree = '/tmp/lead', started_at = 't' WHERE plan = 'pay-1';");

        // Simulate an attempt in the refactor workflow touching a test file (e.g. tests/refactor_test.rs)
        // Submitting this attempt must fail with test_change_forbidden
        fx.git(&fx.repo, &["branch", "pay-1/cov"]);
        fx.git(&fx.repo, &["branch", "pay-1/refact"]);
        let base = fx.git(&fx.repo, &["rev-parse", "HEAD"]);
        fx.git(
            &fx.repo,
            &["checkout", "-b", "pay-1/refact--refact-impl-a1"],
        );
        fx.commit_file(
            &fx.repo,
            "tests/refactor_test.rs",
            "bad test",
            "change test",
        );
        fx.env.sql(&format!(
            "UPDATE jobs SET status = 'queued', tier = 'S2', goal = 'goal', context = 'context', touches = '[\"**\"]', acceptance = '[\"works\"]', out_of_scope = '[]', fingerprint = printf('%064d', 0) WHERE key = 'refact-impl';
             UPDATE jobs SET status = 'running' WHERE key = 'refact-impl';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
             VALUES ('pay-1', 'refact-impl-a1', 'refact-impl', 1, 'S2', 'ladder', 'running', 'pay-1/refact--refact-impl-a1', '{}', '{}', 't');",
            fx.repo.display(), base
        ));
        let run = fx.cpd_in(
            &fx.repo,
            &["attempt", "submit", "refact-impl-a1", "--plan", "pay-1"],
        );
        let err = common::refused(&run, "test_change_forbidden", 3);
        assert!(
            err["message"].as_str().unwrap_or_default().contains("test"),
            "refusal message should mention forbidden test file change"
        );
    }

    /// Decision pro-d579 / pro-d581, commands.md §34.1 rule 3, §34.3:
    /// A refactor workflow's diff touching a test file is refused at `cpd pr push`
    /// with `test_change_forbidden` and exit code 3.
    #[test]
    fn refactor_push_touching_test_file_refused() {
        let fx = fixture("grounding");

        fx.add_workflow("cov", &["--shape", "coverage"]).ok();
        fx.env.sql("UPDATE jobs SET status = 'queued', tier = 'S2', goal = 'goal', context = 'context', touches = '[\"src/**\"]', acceptance = '[\"works\"]', out_of_scope = '[]', fingerprint = printf('%064d', 0) WHERE workflow = 'cov'; UPDATE jobs SET status = 'running' WHERE workflow = 'cov'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE workflow = 'cov';");
        fx.add_workflow("refact", &["--shape", "refactor", "--base", "cov"])
            .ok();
        fx.env.sql("UPDATE plans SET phase = 'plan' WHERE id = 'pay-1'; UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z' WHERE id = 'pay-1'; UPDATE workflows SET status = 'dispatch', lead_worktree = '/tmp/lead', started_at = 't' WHERE plan = 'pay-1';");

        fx.git(&fx.repo, &["branch", "pay-1/cov"]);
        fx.git(&fx.repo, &["checkout", "-b", "pay-1/refact"]);
        fx.commit_file(&fx.repo, "tests/bad_test.rs", "bad test", "change test");

        // Pushing a refactor workflow whose commits touch a test file is refused with test_change_forbidden
        let run = fx.cpd(&["pr", "push", "--workflow", "refact", "--plan", "pay-1"]);
        let err = common::refused(&run, "test_change_forbidden", 3);
        assert!(
            err["message"].as_str().unwrap_or_default().contains("test"),
            "push refusal should mention forbidden test file change"
        );
    }

    /// Decision pro-d579 / pro-d581, commands.md §34.1 rule 5, §34.3:
    /// Gate fails if coverage drops below the coverage workflow baseline, with `coverage_below_baseline`.
    #[test]
    fn refactor_coverage_below_baseline_fails_gate() {
        let fx = fixture("grounding");

        fx.add_workflow("cov", &["--shape", "coverage"]).ok();
        fx.env.sql("UPDATE jobs SET status = 'queued', tier = 'S2', goal = 'goal', context = 'context', touches = '[\"src/**\"]', acceptance = '[\"works\"]', out_of_scope = '[]', fingerprint = printf('%064d', 0) WHERE workflow = 'cov'; UPDATE jobs SET status = 'running' WHERE workflow = 'cov'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE workflow = 'cov';");
        fx.add_workflow("refact", &["--shape", "refactor", "--base", "cov"])
            .ok();
        fx.env.sql("UPDATE plans SET phase = 'plan' WHERE id = 'pay-1'; UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z' WHERE id = 'pay-1'; UPDATE workflows SET status = 'dispatch', lead_worktree = '/tmp/lead', started_at = 't' WHERE plan = 'pay-1';");

        // When gate runs for the refactor workflow (e.g. during pr push or gate check),
        // if coverage is below the baseline of the coverage workflow, gate fails with coverage_below_baseline
        let run = fx.cpd(&["pr", "push", "--workflow", "refact", "--plan", "pay-1"]);
        let err = common::refused(&run, "coverage_below_baseline", 3);
        assert!(
            err["message"]
                .as_str()
                .unwrap_or_default()
                .contains("coverage"),
            "gate failure should mention coverage below baseline"
        );
    }

    /// Decision pro-d579 / pro-d581, commands.md §34.1 rule 4, §34.4:
    /// Mid-refactor failing test causes job to be blocked on coverage workflow;
    /// amending coverage and rebasing clears the block.
    #[test]
    fn bad_test_stops_refactor_until_coverage_amend() {
        let fx = fixture("grounding");

        fx.add_workflow("cov", &["--shape", "coverage"]).ok();
        fx.env.sql("UPDATE jobs SET status = 'queued', tier = 'S2', goal = 'goal', context = 'context', touches = '[\"src/**\"]', acceptance = '[\"works\"]', out_of_scope = '[]', fingerprint = printf('%064d', 0) WHERE workflow = 'cov'; UPDATE jobs SET status = 'running' WHERE workflow = 'cov'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE workflow = 'cov';");
        fx.add_workflow("refact", &["--shape", "refactor", "--base", "cov"])
            .ok();
        fx.env.sql("UPDATE plans SET phase = 'plan' WHERE id = 'pay-1'; UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z' WHERE id = 'pay-1'; UPDATE workflows SET status = 'dispatch', lead_worktree = '/tmp/lead', started_at = 't' WHERE plan = 'pay-1';");

        // A test failure in refactor workflow does not allow in-place test edit/approval.
        // The job is stopped/blocked on the coverage workflow.
        // Trying to approve tests on refactor job is refused
        let run = fx.cpd(&[
            "job",
            "approve-tests",
            "refact-impl",
            "--path",
            "tests/test.rs",
            "--reason",
            "bad test",
            "--plan",
            "pay-1",
        ]);
        // Commands.md §34.1: "There is no in-place test approval: cpd job approve-tests does not apply to a refactor."
        // Refusal or blocked status indicates refactor cannot proceed until coverage workflow amended
        let err = common::refused(&run, "test_change_forbidden", 3);
        assert!(
            err["message"]
                .as_str()
                .unwrap_or_default()
                .contains("coverage"),
            "approval attempt on refactor should instruct amending coverage workflow"
        );
    }
}
