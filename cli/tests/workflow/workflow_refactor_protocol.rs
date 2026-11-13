//! Refactor protocol red tests (commands.md §34, decisions pro-d579 / pro-d581).

use crate::common;
use crate::common::Run;
use crate::common::fx::Fx;
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

const MAKE_GREEN: &str = "UPDATE jobs SET status = 'queued', tier = 'S2', goal = 'goal', context = 'context', touches = '[\"src/**\"]', acceptance = '[\"works\"]', out_of_scope = '[]', fingerprint = printf('%064d', 0) WHERE workflow = 'WF'; UPDATE jobs SET status = 'running' WHERE workflow = 'WF'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE workflow = 'WF';";

impl RefactorFixture {
    /// A merged coverage workflow `key` whose lead branch exists, and a policy that measures coverage with a mock.
    fn green_coverage(&self, key: &str) {
        self.add_workflow(key, &["--shape", "coverage"]).ok();
        self.env.sql(&MAKE_GREEN.replace("WF", key));
        self.git(&self.repo, &["branch", &format!("pay-1/{key}")]);
    }

    fn measure_with_mock(&self, replies: &[&str]) {
        use cpd_core::host::runner::Reply;
        self.env.repo_policy(
            "acme",
            "pay",
            &format!("{TESTS_TABLE}\n[coverage-commands]\nmeasure = \"mock-coverage\"\n"),
        );
        let mut runner = (*self.fakes.runner).clone();
        for reply in replies {
            runner = runner.once(
                "sh",
                |call| call.args.last().is_some_and(|arg| arg == "mock-coverage"),
                Reply::ok(reply),
            );
        }
    }

    fn baseline(&self, workflow: &str) -> (String, String) {
        self.env.read(|conn| {
            conn.query_row(
                "SELECT base_sha, report FROM refactor_coverage_baselines WHERE plan = 'pay-1' AND workflow = ?1",
                [workflow],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
    }
}

mod tests {
    use super::*;

    /// The baseline is captured at the remote tip, fetched first: a stale remote-tracking ref is not used.
    #[test]
    fn baseline_capture_fetches_the_base_before_measuring() {
        let fx = fixture("grounding");
        let origin = fx.env.root().join("origin.git");
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
        fx.local_origin(&fx.repo, &origin);
        fx.green_coverage("cov");
        fx.git(&fx.repo, &["push", "-q", "origin", "pay-1/cov"]);
        let stale = fx.git(&fx.repo, &["rev-parse", "pay-1/cov"]);
        fx.git(&fx.repo, &["checkout", "-q", "pay-1/cov"]);
        fx.commit_file(&fx.repo, "src/new.rs", "new", "advance the remote");
        fx.git(&fx.repo, &["push", "-q", "origin", "pay-1/cov"]);
        let tip = fx.git(&fx.repo, &["rev-parse", "pay-1/cov"]);
        fx.git(&fx.repo, &["checkout", "-q", "main"]);
        fx.git(
            &fx.repo,
            &["update-ref", "refs/remotes/origin/pay-1/cov", &stale],
        );
        // The remote-tracking ref lags the remote's tip.
        fx.git(
            &fx.repo,
            &["update-ref", "refs/remotes/origin/pay-1/cov", &stale],
        );
        fx.measure_with_mock(&[r#"{"line": 90.0, "branch": 80.0}"#]);

        fx.add_workflow("refact", &["--shape", "refactor", "--base", "cov"])
            .ok();

        let (sha, report) = fx.baseline("refact");
        assert_eq!(
            sha, tip,
            "the baseline is the fetched remote tip, not the stale ref"
        );
        assert!(
            report.contains("90"),
            "the measured report is stored: {report}"
        );
    }

    /// Editing a refactor's base re-measures and replaces its stored baseline.
    #[test]
    fn editing_the_base_of_a_refactor_refreshes_its_baseline() {
        let fx = fixture("grounding");
        fx.green_coverage("cov");
        fx.green_coverage("cov2");
        fx.git(&fx.repo, &["checkout", "-q", "pay-1/cov2"]);
        fx.commit_file(&fx.repo, "src/two.rs", "two", "second base");
        fx.git(&fx.repo, &["checkout", "-q", "main"]);
        let second = fx.git(&fx.repo, &["rev-parse", "pay-1/cov2"]);
        fx.measure_with_mock(&[
            r#"{"line": 50.0, "branch": 40.0}"#,
            r#"{"line": 95.0, "branch": 85.0}"#,
        ]);
        fx.add_workflow("refact", &["--shape", "refactor", "--base", "cov"])
            .ok();
        let (before, _) = fx.baseline("refact");
        assert_ne!(before, second);

        fx.cpd(&[
            "workflow",
            "edit",
            "refact",
            "--base",
            "cov2",
            "--no-needs",
            "--plan",
            "pay-1",
        ])
        .ok();

        let (sha, report) = fx.baseline("refact");
        assert_eq!(sha, second, "the baseline follows the new base");
        assert!(
            report.contains("95"),
            "the baseline was re-measured: {report}"
        );
    }

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
        fx.git(&fx.repo, &["branch", "pay-1/cov"]);
        fx.env.repo_policy(
            "acme",
            "pay",
            &format!("{TESTS_TABLE}\n[coverage-commands]\nmeasure = \"mock-coverage\"\n"),
        );
        use cpd_core::host::runner::Reply;
        let _ = (*fx.fakes.runner)
            .clone()
            .once(
                "sh",
                |call| call.args.last().is_some_and(|arg| arg == "mock-coverage"),
                Reply::ok(r#"{"line": 90.0, "branch": 80.0}"#),
            )
            .once(
                "sh",
                |call| call.args.last().is_some_and(|arg| arg == "mock-coverage"),
                Reply::ok(r#"{"line": 70.0, "branch": 80.0}"#),
            );

        fx.add_workflow("refact", &["--shape", "refactor", "--base", "cov"])
            .ok();
        fx.env.sql("UPDATE plans SET phase = 'plan' WHERE id = 'pay-1'; UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z' WHERE id = 'pay-1'; UPDATE workflows SET status = 'dispatch', lead_worktree = '/tmp/lead', started_at = 't' WHERE plan = 'pay-1';");

        fx.git(&fx.repo, &["checkout", "-b", "pay-1/refact"]);
        fx.commit_file(&fx.repo, "src/lib.rs", "implementation", "refactor");
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

    /// Renaming an instrumented file while lowering its coverage is refused: the new path inherits the old path's baseline.
    #[test]
    fn renaming_an_instrumented_file_below_its_baseline_fails_gate() {
        let fx = fixture("grounding");
        fx.commit_file(
            &fx.repo,
            "src/old.rs",
            "implementation\nline two\nline three\n",
            "add old",
        );
        fx.green_coverage("cov");
        fx.measure_with_mock(&[
            r#"{"line": 90.0, "branch": 80.0, "files": {"src/old.rs": {"line": 90.0, "branch": 80.0}}}"#,
            r#"{"line": 90.0, "branch": 80.0, "files": {"src/new.rs": {"line": 50.0, "branch": 80.0}}}"#,
        ]);
        fx.add_workflow("refact", &["--shape", "refactor", "--base", "cov"])
            .ok();
        fx.env.sql("UPDATE plans SET phase = 'plan' WHERE id = 'pay-1'; UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z' WHERE id = 'pay-1'; UPDATE workflows SET status = 'dispatch', lead_worktree = '/tmp/lead', started_at = 't' WHERE plan = 'pay-1';");

        fx.git(&fx.repo, &["checkout", "-b", "pay-1/refact"]);
        fx.git(&fx.repo, &["mv", "src/old.rs", "src/new.rs"]);
        fx.git(&fx.repo, &["commit", "-q", "-m", "rename"]);
        let run = fx.cpd(&["pr", "push", "--workflow", "refact", "--plan", "pay-1"]);
        common::refused(&run, "coverage_below_baseline", 3);
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

    /// A coverage base whose Jobs are all merged but whose PR checks have not passed is not green.
    #[test]
    fn coverage_base_with_unpassed_checks_refused() {
        let fx = fixture("grounding");

        fx.add_workflow("cov", &["--shape", "coverage"]).ok();
        fx.env.sql("UPDATE jobs SET status = 'queued', tier = 'S2', goal = 'goal', context = 'context', touches = '[\"src/**\"]', acceptance = '[\"works\"]', out_of_scope = '[]', fingerprint = printf('%064d', 0) WHERE workflow = 'cov'; UPDATE jobs SET status = 'running' WHERE workflow = 'cov'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE workflow = 'cov';
            INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
              VALUES ('acme/pay', 900, 'https://github.com/acme/pay/pull/900', 'Cov', 0, 'open', 'none', 'pending', 'opened', NULL, 'opened');
            UPDATE workflows SET status = 'dispatch', lead_worktree = '/tmp/lead', started_at = 't' WHERE key = 'cov'; UPDATE workflows SET status = 'open', pr_number = 900, pr_url = 'https://github.com/acme/pay/pull/900', pr_opened_at = 'opened', pull_request = (SELECT id FROM pull_requests WHERE number = 900) WHERE key = 'cov';");
        let run = fx.add_workflow("refact", &["--shape", "refactor", "--base", "cov"]);
        common::refused(&run, "base_coverage_required", 3);
    }
}
