//! The Worker Spec (commands.md §16.1): snapshots of a first Attempt and of a later one
//! with Previous Attempts, and the pieces around them.

use std::fs;
use std::path::Path;

use cpd_core::config::Tier;
use cpd_core::error::ErrorCode;
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, EscalationId, PlanId};
use cpd_core::attempt::ledger::{Cause, Stored};
use cpd_core::spec::{self, Answer, Evidence, Input, JobKind, Previous};

fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

fn attempt(id: &str) -> AttemptId {
    AttemptId::parse(id).unwrap()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A first Attempt: a quick check, nothing out of scope, nothing earlier.
fn first_input<'a>(plan: &'a PlanId, attempt: &'a AttemptId, lists: &'a Lists) -> Input<'a> {
    Input {
        plan,
        attempt,
        title: "API schema",
        tier: Tier::parse("S3").unwrap(),
        worktree: Path::new("/w/pay--api-schema-a1"),
        kind: JobKind::Work,
        goal: "Define the schema.",
        context: "The API has none yet.",
        touches: &lists.touches,
        acceptance: &lists.acceptance,
        out_of_scope: &[],
        resources: &[],
        quick_check: &lists.quick_check,
        protected: &[],
        generated: &[],
        approved_touches: &[],
        previous: &[],
        answers: &[],
    }
}

struct Lists {
    touches: Vec<String>,
    acceptance: Vec<String>,
    quick_check: Vec<String>,
}

fn lists() -> Lists {
    Lists {
        touches: strings(&["src/schema.rs", "tests/schema.rs"]),
        acceptance: strings(&["`cargo test` passes"]),
        quick_check: strings(&["cargo check --all-targets"]),
    }
}

#[test]
fn the_first_attempt_s_spec() {
    let (plan, attempt, lists) = (plan(), attempt("api-schema-a1"), lists());
    assert_eq!(spec::render(&first_input(&plan, &attempt, &lists)).unwrap(), FIRST);
}

#[test]
fn the_spec_bounds_the_worker_s_commands() {
    let (plan, attempt, lists) = (plan(), attempt("api-schema-a1"), lists());
    let spec = spec::render(&first_input(&plan, &attempt, &lists)).unwrap();
    assert!(spec.contains("Commit a skeleton within 10 minutes"));
    assert!(spec.contains("Wrap every test and measurement in `timeout`."));
    assert!(spec.contains("Write no synthetic benchmarks."));
    assert!(spec.contains("Never start `cpd events follow`, `cpd watch`, `cpd tui`, `cpd session run`"));
}

#[test]
fn the_spec_tells_the_worker_to_tear_down_what_it_started() {
    let (plan, attempt, lists) = (plan(), attempt("api-schema-a1"), lists());
    let spec = spec::render(&first_input(&plan, &attempt, &lists)).unwrap();
    assert!(spec.contains("- Before submitting, stop every process you started"));
    assert!(spec.contains("`tmux -L <name> kill-server`"));
    assert!(spec.contains("Never leave anything running when you submit."));
    assert!(spec.contains("Before you submit, tear down what you started:"));
}

/// Attempt 3, after one that conflicted and one that crashed, with a question already answered,
/// no quick check, things out of scope, a protected list of the repo's own (one entry repeating
/// an always-protected path) and Spec text that looks like template syntax.
fn later() -> String {
    let (plan, attempt) = (plan(), attempt("api-schema-a3"));
    let touches = strings(&["src/**"]);
    let acceptance = strings(&["`cargo test` passes", "no new warnings"]);
    let out_of_scope = strings(&["Changing the CLI", "Renaming {{ things }}"]);
    let protected = strings(&["verify.sh", "scripts/ci/", ".cpd/"]);
    let previous = [
        Previous {
            number: 1,
            tier: Tier::parse("S2").unwrap(),
            cause: Stored::Failed(Cause::MergeConflict),
            evidence: Evidence::Files(strings(&["src/schema.rs", "Cargo.lock"])),
        },
        Previous { number: 2, tier: Tier::parse("S3").unwrap(), cause: Stored::Failed(Cause::Crashed), evidence: Evidence::None },
    ];
    let answers = [Answer {
        id: EscalationId::parse("api-schema-a1-q1").unwrap(),
        question: "Which crate?\nserde or miniserde?".to_owned(),
        answer: "serde.".to_owned(),
    }];
    spec::render(&Input {
        plan: &plan,
        attempt: &attempt,
        title: "API schema",
        tier: Tier::parse("S3").unwrap(),
        worktree: Path::new("/w/pay--api-schema-a3"),
        kind: JobKind::Work,
        goal: "Define the schema: {% raw %}.",
        context: "",
        touches: &touches,
        acceptance: &acceptance,
        out_of_scope: &out_of_scope,
        resources: &[],
        quick_check: &[],
        protected: &protected,
        generated: &[],
        approved_touches: &[],
        previous: &previous,
        answers: &answers,
    })
    .unwrap()
}

#[test]
fn a_later_attempt_s_spec_carries_what_the_earlier_ones_teach() {
    assert_eq!(later(), LATER);
}

#[test]
fn every_cause_has_its_outcome_and_a_sentence() {
    let cases = [
        ("merged", "merged", "It was merged."),
        ("cancel_requested", "cancelled", "The Lead cancelled the Job while the Attempt ran."),
        ("setup_failed", "failed", "The worktree or its `pre-start` hook failed, so the Worker never started."),
        ("timed_out", "failed", "The Worker ran out of time before it submitted."),
        ("crashed", "failed", "The Worker or its driver died before the work was merged."),
        ("no_submit", "failed", "The Worker went idle twice without running `cpd attempt submit`, even after a nudge."),
        ("merge_conflict", "failed", "Rebasing it onto the newer Lead Branch conflicted."),
        ("cas_lost", "failed", "The Lead Branch kept moving, so the merge could not be completed."),
        ("dirty_target", "failed", "The Lead worktree was not clean when the merge ran."),
        ("tampered", "failed", "Its diff touched a protected path."),
        ("gate_failed", "failed", "The Gate failed at its merge."),
        ("gate_error", "failed", "The Gate could not be judged at its merge: it was killed, timed out or could not run, which is a problem of the Gate, not a failing check of your work."),
    ];
    let (plan, attempt, lists) = (plan(), attempt("api-schema-a2"), lists());
    for (cause, outcome, sentence) in cases {
        let cause = spec::parse_cause(cause).unwrap();
        assert_eq!(cause.outcome(), outcome);
        let previous = [Previous { number: 1, tier: Tier::parse("S2").unwrap(), cause, evidence: Evidence::None }];
        let text = spec::render(&Input { previous: &previous, ..first_input(&plan, &attempt, &lists) }).unwrap();
        let expected = format!("- Outcome: {outcome}\n- Cause: {}\n- {sentence}\n", cause.as_str());
        assert!(text.contains(&expected), "{text}");
    }
    assert_eq!(spec::parse_cause("review_rejected").unwrap_err().code(), ErrorCode::StoreFailed);
}

#[test]
fn write_puts_the_spec_where_the_home_says_and_rewrites_it() {
    let home_dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(home_dir.path().into()), None).unwrap();
    let (plan, attempt, lists) = (plan(), attempt("api-schema-a1"), lists());
    let input = first_input(&plan, &attempt, &lists);
    let path = spec::write(&home, &input).unwrap();
    assert_eq!(path, home_dir.path().join("run/pay/specs/api-schema-a1.md"));
    assert_eq!(fs::read_to_string(&path).unwrap(), spec::render(&input).unwrap());
    fs::write(&path, "stale").unwrap();
    assert_eq!(spec::write(&home, &input).unwrap(), path);
    assert_eq!(fs::read_to_string(&path).unwrap(), spec::render(&input).unwrap());
}

/// The `hooks.pre-commit` pipeline of the repo policy file text `file`.
fn pre_commit_of(file: &str) -> Result<cpd_core::git::Pipeline, cpd_core::error::CpdError> {
    let scope = cpd_core::policy::parse_scope(std::path::Path::new("pay.toml"), file)?;
    Ok(scope.hooks.and_then(|h| h.pre_commit).unwrap_or_default())
}

#[test]
fn the_quick_check_is_the_pre_commit_commands_verbatim_in_file_order() {
    let pipeline = pre_commit_of("[hooks]\npre-merge = \"make gate\"\n[hooks.pre-commit]\nlint = \"cargo clippy -- {{ branch }}\"\nbuild = \"cargo build\"\n").unwrap();
    assert_eq!(spec::quick_check(&pipeline), ["cargo clippy -- {{ branch }}", "cargo build"]);
}

#[test]
fn no_pre_commit_table_means_no_quick_check_and_no_fallback_to_pre_merge() {
    assert!(spec::quick_check(&pre_commit_of("[hooks]\npre-merge = \"make gate\"\n").unwrap()).is_empty());
    assert!(spec::quick_check(&pre_commit_of("").unwrap()).is_empty());
}

#[test]
fn a_malformed_pre_commit_is_policy_invalid() {
    let err = pre_commit_of("[hooks]\npre-commit = [1]\n").unwrap_err();
    assert_eq!(err.code(), ErrorCode::PolicyInvalid);
}

#[test]
fn a_multi_line_quick_check_command_stays_one_indented_block() {
    let (plan, attempt) = (plan(), attempt("api-schema-a1"));
    let lists = Lists { quick_check: strings(&["set -e\ncargo check\n\ncargo test"]), ..lists() };
    let text = spec::render(&first_input(&plan, &attempt, &lists)).unwrap();
    assert!(text.contains("verbatim:\n\n      set -e\n      cargo check\n\n      cargo test\n\n- Never edit"), "{text}");
}

const FIRST: &str = r####"# Worker Spec: api-schema, Attempt 1

## Who you are

You are a CPD Worker on Attempt 1 of Job `api-schema` (API schema) in Plan `pay`.
Your attempt id is `api-schema-a1`. Your Tier is `S3`.
Your worktree is `/w/pay--api-schema-a1`. Your current directory is that worktree.

## Rules

- Work only in this worktree, and touch the files listed under "Files you may touch". A diff outside those files is refused `touches_violation`; the repo's generated paths (listed in the Spec) are exempt; a path the Lead approved with `job approve-touches` is exempt; otherwise ask the Lead (`cpd ask`) or revert the file. Never a guarded or protected path.
- Commit your work. Uncommitted changes are refused at submit.
- Before submitting, run the quick check yourself, from this worktree. It is `hooks.pre-commit` of the repo's policy file; its commands, verbatim:

      cargo check --all-targets

- Never edit the protected paths. A diff that touches one is refused. They are:
  - `.cpd/`
- Never ask the User anything. You have no conversation with them.
- If continuing would require guessing, ask the Lead and wait for the answer. The command blocks until the Lead answers:

      cpd ask api-schema-a1 "<your question>"

  Ask one real blocker at a time. Do not ask what this Spec or the code already answers.
  Run `cpd ask` as a call of its own, in the foreground, with the bash tool's `timeout` input set to 7200 (seconds). Never put it under the `timeout` command, in a pipeline or in the background: it ends only when the Lead answers or the wait expires.
- Commit a skeleton within 10 minutes of starting, then commit at least every 20 minutes.
- Run only commands that exit. Wrap every test and measurement in `timeout`. Write no synthetic benchmarks. Never start `cpd events follow`, `cpd watch`, `cpd tui`, `cpd session run` or any other command that does not exit.
- Run only the tests your change touches: the test files or modules of what you changed and your new tests, named in your Spec's acceptance. Never the full suite (`cargo test` or `nextest` with `--workspace` or no filter, `npm test` of everything): the Gate runs it, in the PR's open Job, and the adapter refuses it in a Worker session.
- Before submitting, stop every process you started (background commands, private tmux servers you started with `tmux -L <name>` — kill them with `tmux -L <name> kill-server` —, any `cpd watch`, servers, watchers) and remove the temporary homes and files you created outside the worktree. Never leave anything running when you submit.
- Do not stop until you have submitted.

## Job

### Goal

Define the schema.

### Context

The API has none yet.

### Files you may touch

- `src/schema.rs`
- `tests/schema.rs`

### Acceptance

Every item must hold when you submit.

- `cargo test` passes

### Out of scope

Nothing is listed. Still do only what the Goal and Acceptance require.

## Finish

Before you submit, tear down what you started: stop every process you started and remove the temporary homes and files you created outside the worktree.

When the Acceptance items hold and your work is committed, run exactly:

    cpd attempt submit api-schema-a1

If it succeeds, stop at once and make no more changes. If it refuses, the error code says what to do:

- `wrong_worktree`: you ran it outside your worktree. Run it from the worktree.
- `dirty_tree`: you have uncommitted changes. Commit them (or delete files that do not belong), then submit again.
- `no_commits`: you have not committed anything on top of the base. Commit your work, then submit again.
- `gate_tampered`: your diff touches a protected path. Revert those files to the base version and commit, then submit again.
- `touches_violation`: your diff touches a path outside the files you may touch, and it is neither generated nor approved. Revert those files or ask the Lead (`cpd ask`), then submit again.
- `attempt_not_running`: the Attempt was stopped. Stop.

Do not stop until `cpd attempt submit` has succeeded.
"####;

const LATER: &str = r####"# Worker Spec: api-schema, Attempt 3

## Who you are

You are a CPD Worker on Attempt 3 of Job `api-schema` (API schema) in Plan `pay`.
Your attempt id is `api-schema-a3`. Your Tier is `S3`.
Your worktree is `/w/pay--api-schema-a3`. Your current directory is that worktree.

## Rules

- Work only in this worktree, and touch the files listed under "Files you may touch". A diff outside those files is refused `touches_violation`; the repo's generated paths (listed in the Spec) are exempt; a path the Lead approved with `job approve-touches` is exempt; otherwise ask the Lead (`cpd ask`) or revert the file. Never a guarded or protected path.
- Commit your work. Uncommitted changes are refused at submit.
- Before submitting, run the quick check yourself. This repo defines no quick check.
- Never edit the protected paths. A diff that touches one is refused. They are:
  - `.cpd/`
  - `verify.sh`
  - `scripts/ci/`
- Never ask the User anything. You have no conversation with them.
- If continuing would require guessing, ask the Lead and wait for the answer. The command blocks until the Lead answers:

      cpd ask api-schema-a3 "<your question>"

  Ask one real blocker at a time. Do not ask what this Spec or the code already answers.
  Run `cpd ask` as a call of its own, in the foreground, with the bash tool's `timeout` input set to 7200 (seconds). Never put it under the `timeout` command, in a pipeline or in the background: it ends only when the Lead answers or the wait expires.
- Commit a skeleton within 10 minutes of starting, then commit at least every 20 minutes.
- Run only commands that exit. Wrap every test and measurement in `timeout`. Write no synthetic benchmarks. Never start `cpd events follow`, `cpd watch`, `cpd tui`, `cpd session run` or any other command that does not exit.
- Run only the tests your change touches: the test files or modules of what you changed and your new tests, named in your Spec's acceptance. Never the full suite (`cargo test` or `nextest` with `--workspace` or no filter, `npm test` of everything): the Gate runs it, in the PR's open Job, and the adapter refuses it in a Worker session.
- Before submitting, stop every process you started (background commands, private tmux servers you started with `tmux -L <name>` — kill them with `tmux -L <name> kill-server` —, any `cpd watch`, servers, watchers) and remove the temporary homes and files you created outside the worktree. Never leave anything running when you submit.
- Do not stop until you have submitted.

## Job

### Goal

Define the schema: {% raw %}.

### Context

### Files you may touch

- `src/**`

### Acceptance

Every item must hold when you submit.

- `cargo test` passes
- no new warnings

### Out of scope

Do not do any of these:

- Changing the CLI
- Renaming {{ things }}

## Previous Attempts

Earlier Attempts of this Job did not end Merged. Do not repeat their mistakes.

### Attempt 1 (Tier `S2`)

- Outcome: failed
- Cause: merge_conflict
- Rebasing it onto the newer Lead Branch conflicted in these files:
  - `src/schema.rs`
  - `Cargo.lock`

### Attempt 2 (Tier `S3`)

- Outcome: failed
- Cause: crashed
- The Worker or its driver died before the work was merged.

## Answers already given

Earlier Attempts of this Job asked these questions and the Lead answered them. Do not ask them again.

### `api-schema-a1-q1`

Question:

Which crate?
serde or miniserde?

Answer:

serde.

## Finish

Before you submit, tear down what you started: stop every process you started and remove the temporary homes and files you created outside the worktree.

When the Acceptance items hold and your work is committed, run exactly:

    cpd attempt submit api-schema-a3

If it succeeds, stop at once and make no more changes. If it refuses, the error code says what to do:

- `wrong_worktree`: you ran it outside your worktree. Run it from the worktree.
- `dirty_tree`: you have uncommitted changes. Commit them (or delete files that do not belong), then submit again.
- `no_commits`: you have not committed anything on top of the base. Commit your work, then submit again.
- `gate_tampered`: your diff touches a protected path. Revert those files to the base version and commit, then submit again.
- `touches_violation`: your diff touches a path outside the files you may touch, and it is neither generated nor approved. Revert those files or ask the Lead (`cpd ask`), then submit again.
- `attempt_not_running`: the Attempt was stopped. Stop.

Do not stop until `cpd attempt submit` has succeeded.
"####;

/// The Spec of the first Attempt with `kind` (commands.md §16.1 "The kinds that have checks").
fn kind_spec(kind: JobKind<'_>) -> String {
    let (plan, attempt, lists) = (plan(), attempt("api-schema-a1"), lists());
    spec::render(&Input { kind, ..first_input(&plan, &attempt, &lists) }).unwrap()
}

fn assert_has(text: &str, wanted: &[&str]) {
    for w in wanted {
        assert!(text.contains(w), "the Spec lacks {w:?}:\n{text}");
    }
}

fn assert_lacks(text: &str, unwanted: &[&str]) {
    for w in unwanted {
        assert!(!text.contains(w), "the Spec has {w:?}:\n{text}");
    }
}

#[test]
fn a_work_and_a_docs_job_have_no_section_of_their_own() {
    for kind in [JobKind::Work, JobKind::Docs] {
        let text = kind_spec(kind);
        assert_has(&text, &["- Commit your work.", "- `no_commits`:"]);
        assert_lacks(&text, &["### Tests", "**test Job**", "**implementation Job**", "**open Job**", "**update Job**", "tests_", "base_conflict"]);
    }
}

#[test]
fn a_test_job_s_spec_names_its_tests_files_and_refusals() {
    let (tests, files) = (strings(&["schema::parses", "schema::rejects"]), strings(&["tests/schema.rs"]));
    let text = kind_spec(JobKind::Test { tests: &tests, test_files: &files });
    assert_has(
        &text,
        &[
            "This is a **test Job**",
            "- Commit your work.",
            "Add stubs",
            "### Tests",
            "- `schema::parses`",
            "- `schema::rejects`",
            "- `tests/schema.rs`",
            "- `test_files_missing`:",
            "- `tests_build_failed`:",
            "- `tests_not_found`:",
            "- `tests_not_red`:",
            "- `tests_error`:",
            "- `no_commits`:",
        ],
    );
    assert_lacks(&text, &["tests_tampered", "tests_failed", "**open Job**", "base_conflict"]);
}

#[test]
fn an_impl_job_s_spec_names_the_test_jobs_the_guarded_files_and_the_approved_paths() {
    let (jobs, tests) = (strings(&["schema-tests", "more-tests"]), strings(&["schema::parses"]));
    let (guarded, approved) = (strings(&["tests/schema.rs", "tests/more/"]), strings(&["tests/schema.rs"]));
    let text = kind_spec(JobKind::Impl { test_jobs: &jobs, tests: &tests, guarded: &guarded, approved: &approved });
    assert_has(
        &text,
        &[
            "This is an **implementation Job**",
            "`schema-tests`, `more-tests`",
            "- `schema::parses`",
            "- `tests/more/`",
            "The Lead approved you to change these guarded paths:",
            "`cpd ask`",
            "- `tests_tampered`:",
            "- `tests_failed`:",
            "- `tests_build_failed`:",
            "- `tests_not_found`:",
            "- `tests_error`:",
            "- `no_commits`:",
        ],
    );
    assert_lacks(&text, &["tests_not_red", "test_files_missing", "base_conflict"]);
    let none = kind_spec(JobKind::Impl { test_jobs: &jobs, tests: &tests, guarded: &guarded, approved: &[] });
    assert_lacks(&none, &["The Lead approved you"]);
}

#[test]
fn an_open_and_an_update_job_merge_the_base_only_on_a_conflict_and_leave_the_rest_to_cpd() {
    for kind in [JobKind::Open, JobKind::Update] {
        let text = kind_spec(kind);
        assert_has(
            &text,
            &[
                "**only if the Lead Branch conflicts with it**",
                "merely moved ahead is no reason",
                "Do not push, do not run `gh`, do not run the Gate",
                "- `owner_unset`, `owner_unconfirmed`, `policy_invalid`:",
            ],
        );
        assert_lacks(&text, &["- Commit your work.", "- `no_commits`:", "### Tests", "tests_",
                "`pr_title_required`", "`jira_required`", "`pr_closed`", "`illegal_transition`", "`lead_branch_moved`", "`base_not_found`", "`base_conflict`", "`gate_failed`", "`gate_error`"]);
    }
    assert_has(&kind_spec(JobKind::Open), &["This is the **open Job**"]);
    assert_has(&kind_spec(JobKind::Update), &["This is an **update Job**"]);
}

#[test]
fn the_spec_tells_the_worker_how_to_run_cpd_ask() {
    let artefact = std::path::Path::new("scout.md");
    let tests = strings(&["t::a"]);
    let files = strings(&["a.rs"]);
    let jobs = strings(&["j"]);
    let kinds = [
        JobKind::Work,
        JobKind::Docs,
        JobKind::Test { tests: &tests, test_files: &files },
        JobKind::Impl { test_jobs: &jobs, tests: &tests, guarded: &files, approved: &[] },
        JobKind::Open,
        JobKind::Update,
        JobKind::Explore { kind: "scout", artefact },
    ];
    for kind in kinds {
        let text = kind_spec(kind);
        assert_has(
            &text,
            &[
                "as a call of its own, in the foreground",
                "`timeout` input set to 7200",
                "Never put it under the `timeout` command, in a pipeline or in the background",
            ],
        );
    }
}
