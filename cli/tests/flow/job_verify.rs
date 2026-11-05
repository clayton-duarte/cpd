//! The Job kind `verify` (contract §2.2 "Job kinds", §4.6, §4.8; commands.md §1.3, §4.6, §4.8, §5.1, §5.2; pro-d80): where it is
//! allowed, the `impl_job_required` refusal, and its Spec. Written by Job verify-tests. The storage (the CHECK) is not tested here.

use serde_json::{Value, json};

use super::job::{
    add_kind, add_needing, count, events, fixture, job, job_row, lock, plan, plan_lock,
    pull_request, review, setup, shaped_pr, spec_file, test_spec_file,
};
use crate::common::fx::Fx;
use crate::common::refused;

/// Plan `pl` in phase `plan` with the `tdd` PR `api`: docs Job `d`, test Job `t` (needs `d`), impl Job `i` (needs `t`), all drafts.
fn tdd(fx: &Fx) {
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    shaped_pr(&fx.env, "pl", "api", "tdd");
    add_kind(fx, "pl", "d", "docs", &spec_file(&fx.env), "api").ok();
    add_needing(fx, "pl", "t", "test", &["d"], "api").ok();
    add_needing(fx, "pl", "i", "impl", &["t"], "api").ok();
}

/// `job add <key> --kind verify` into PR `pr`, with the plain Spec, needing `needs`.
fn verify(fx: &Fx, key: &str, needs: &[&str], pr: &str, draft: bool) -> crate::common::Run {
    let spec = spec_file(&fx.env);
    let needs = needs.join(",");
    let title = format!("Title of {key}");
    let mut args = vec![
        "job",
        "add",
        key,
        "--title",
        &title,
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--kind",
        "verify",
        "--workflow",
        pr,
        "--plan",
        "pl",
    ];
    if !needs.is_empty() {
        args.extend(["--needs", &needs]);
    }
    if draft {
        args.push("--draft");
    }
    fx.cpd(&args)
}

fn writes(fx: &Fx) -> (i64, i64, i64) {
    (
        count(&fx.env, "jobs"),
        count(&fx.env, "events"),
        count(&fx.env, "txns"),
    )
}

#[test]
fn a_verify_job_is_added_to_a_tdd_pr_when_it_needs_an_impl_job() {
    let fx = Fx::new();
    tdd(&fx);
    lock(&fx.env, "pl");
    let out = verify(&fx, "v", &["i"], "api", false).ok();
    assert_eq!(
        (&out["job"]["kind"], &out["job"]["status"]),
        (&json!("verify"), &json!("queued")),
        "{out}"
    );
    assert_eq!(job_row(&fx.env, "pl", "v", "kind"), json!("verify"));
    assert_ne!(
        job_row(&fx.env, "pl", "v", "fingerprint"),
        Value::Null,
        "a queued Job has its fingerprint"
    );
    assert_eq!(
        (
            job_row(&fx.env, "pl", "v", "tests"),
            job_row(&fx.env, "pl", "v", "test_files")
        ),
        (Value::Null, Value::Null)
    );
    assert_eq!(
        events(&fx, "job-added").last().unwrap()["data"]["kind"],
        "verify"
    );

    // After the lock the stub form (no Tier, no Spec) is refused, and a complete draft that waits for the queued `i`
    // is parked: nothing is written (pro-d404).
    let before = writes(&fx);
    let e = fx.cpd(&[
        "job",
        "add",
        "v2",
        "--title",
        "Title of v2",
        "--kind",
        "verify",
        "--needs",
        "i",
        "--draft",
        "--workflow",
        "api",
        "--plan",
        "pl",
    ]);
    refused(&e, "usage", 2);
    assert_eq!(writes(&fx), before, "the stub wrote");
    let e = verify(&fx, "v2", &["i"], "api", true);
    refused(&e, "draft_parked", 3);
    assert_eq!(writes(&fx), before, "the parked draft wrote");
}

#[test]
fn a_verify_job_that_needs_no_impl_job_of_its_pr_is_refused() {
    let fx = Fx::new();
    tdd(&fx);
    lock(&fx.env, "pl");
    let before = writes(&fx);
    for needs in [&[][..], &["d"], &["t"], &["d", "t"]] {
        let e = refused(
            &verify(&fx, "v", needs, "api", false),
            "impl_job_required",
            3,
        );
        assert_eq!(e["details"]["plan"], "pl", "{needs:?}");
        assert_eq!(e["details"]["job"], "v", "{needs:?}");
        assert_eq!(e["details"]["jobs"], json!(["i"]), "{needs:?}");
        assert_eq!(writes(&fx), before, "{needs:?} wrote");
    }
    // A cancelled impl Job does not count.
    add_needing(&fx, "pl", "i2", "impl", &["t"], "api").ok();
    fixture(
        &fx.env,
        "UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE key = 'i2'",
    );
    let before = writes(&fx);
    let e = refused(
        &verify(&fx, "v", &["i2"], "api", false),
        "impl_job_required",
        3,
    );
    assert_eq!(e["details"]["job"], "v");
    let jobs = e["details"]["jobs"].as_array().unwrap();
    assert!(jobs.contains(&json!("i")), "{jobs:?}");
    assert_eq!(writes(&fx), before, "a refusal wrote");
    // Needing the live impl Job is enough.
    assert_eq!(
        verify(&fx, "v", &["i"], "api", false).ok()["job"]["status"],
        "queued"
    );
}

#[test]
fn a_plan_review_and_a_lock_refuse_a_draft_verify_job_without_an_impl_need() {
    let fx = Fx::new();
    tdd(&fx);
    // A draft verify Job is added with no impl need: it is checked where it stops being a draft.
    verify(&fx, "v", &["t"], "api", true).ok();
    // PR `repo` has no draft Job: a review is refused `pr_empty` while a live PR is empty, so end it.
    fx.cpd(&["workflow", "cancel", "repo", "--plan", "pl"]).ok();
    fx.env.outline_for_review("pl");
    let before = (writes(&fx), count(&fx.env, "plan_reviews"));
    let e = refused(&review(&fx, "pl"), "impl_job_required", 3);
    assert_eq!(
        (
            &e["details"]["plan"],
            &e["details"]["job"],
            &e["details"]["jobs"]
        ),
        (&json!("pl"), &json!("v"), &json!(["i"]))
    );
    let e = refused(&plan_lock(&fx, "pl"), "impl_job_required", 3);
    assert_eq!(
        (&e["details"]["job"], &e["details"]["jobs"]),
        (&json!("v"), &json!(["i"]))
    );
    assert_eq!(
        (writes(&fx), count(&fx.env, "plan_reviews")),
        before,
        "a refusal wrote"
    );

    // A missing test Job is refused first: an impl draft that needs no test Job of its PR.
    add_needing(&fx, "pl", "i2", "impl", &["d"], "api").ok();
    let e = refused(&review(&fx, "pl"), "test_job_required", 3);
    assert_eq!(e["details"]["jobs"], json!(["i2"]));
    refused(&plan_lock(&fx, "pl"), "test_job_required", 3);
    job(&fx, &["edit", "i2", "--needs", "t"], "pl").ok();
    refused(&review(&fx, "pl"), "impl_job_required", 3);

    // It needs the impl Job: the review is requested, and the lock goes on to the review it lacks.
    job(&fx, &["edit", "v", "--needs", "i"], "pl").ok();
    assert_eq!(review(&fx, "pl").ok()["review"]["status"], "pending");
    let lock = plan_lock(&fx, "pl");
    assert_ne!(
        lock.json()["error"]["code"],
        "impl_job_required",
        "{lock:?}"
    );
}

#[test]
fn a_draft_can_be_edited_to_kind_verify_in_a_tdd_pr() {
    let fx = Fx::new();
    tdd(&fx);
    add_kind(&fx, "pl", "e", "docs", &spec_file(&fx.env), "api").ok();
    let out = job(&fx, &["edit", "e", "--kind", "verify"], "pl").ok();
    assert_eq!(out["fields"], json!(["kind"]), "{out}");
    assert_eq!(job_row(&fx.env, "pl", "e", "kind"), json!("verify"));
    assert_eq!(
        events(&fx, "job-edited").last().unwrap()["data"],
        json!({"fields": ["kind"]})
    );
    assert_eq!(
        job(&fx, &["edit", "e", "--kind", "verify"], "pl").ok()["already"],
        true
    );
    // `job add --kind verify` takes it as a value too.
    assert_eq!(
        add_kind(&fx, "pl", "f", "verify", &spec_file(&fx.env), "api").ok()["job"]["kind"],
        "verify"
    );
}

#[test]
fn docs_and_config_prs_refuse_a_verify_job_and_a_free_pr_allows_it() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    shaped_pr(&fx.env, "pl", "docs", "docs");
    shaped_pr(&fx.env, "pl", "cfg", "config");
    let spec = spec_file(&fx.env);
    let before = writes(&fx);
    for (pr, allowed) in [("docs", json!(["docs"])), ("cfg", json!(["work"]))] {
        let e = refused(
            &add_kind(&fx, "pl", "v", "verify", &spec, pr),
            "shape_violation",
            3,
        );
        assert_eq!(e["details"]["kind"], "verify", "{pr}");
        assert_eq!(e["details"]["pr"], pr);
        assert_eq!(e["details"]["allowed"], allowed, "{pr}");
        assert_eq!(writes(&fx), before, "{pr} wrote");
    }
    // `free` (the PR `repo` of `plan`): allowed.
    assert_eq!(
        add_kind(&fx, "pl", "v", "verify", &spec, "repo").ok()["job"]["kind"],
        "verify"
    );
}

#[test]
fn an_open_pr_refuses_a_verify_job() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "deploy");
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, shape, created_txn, created_at)
             VALUES ('pl', 'fix', '/repo', 'main', 'waiting', 'cpd/pl/fix', 'tdd', cpd_txn(), 't');
             UPDATE workflows SET status = 'dispatch', lead_worktree = '/wt2', started_at = 't' WHERE key = 'fix';
             {}
             UPDATE workflows SET status = 'open', pr_number = 2, pr_url = 'https://github.com/acme/fix/pull/2', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/fix') WHERE key = 'fix';",
            pull_request("fix", 2, "open")
        ),
    );
    let before = writes(&fx);
    let e = refused(
        &add_kind(&fx, "pl", "v", "verify", &spec_file(&fx.env), "fix"),
        "shape_violation",
        3,
    );
    assert_eq!(
        (
            &e["details"]["status"],
            &e["details"]["kind"],
            &e["details"]["allowed"]
        ),
        (&json!("open"), &json!("verify"), &json!(["work"]))
    );
    assert_eq!(writes(&fx), before, "a refusal wrote");
}

#[test]
fn a_verify_job_has_no_tests_and_a_spec_with_tests_is_refused() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let tests = test_spec_file(
        &fx.env,
        "tests.json",
        json!(["t::one"]),
        json!(["tests/t.rs"]),
    );
    let before = writes(&fx);
    let e = refused(
        &add_kind(&fx, "pl", "v", "verify", &tests, "repo"),
        "spec_invalid",
        2,
    );
    assert_eq!(e["details"]["path"], "$.tests");
    assert_eq!(writes(&fx), before, "a refusal wrote");
    // The plain Spec is complete for it, and a draft cannot be given tests by an edit.
    add_kind(&fx, "pl", "v", "verify", &spec_file(&fx.env), "repo").ok();
    assert_eq!(
        (
            job_row(&fx.env, "pl", "v", "tests"),
            job_row(&fx.env, "pl", "v", "test_files")
        ),
        (Value::Null, Value::Null)
    );
    assert_eq!(
        refused(
            &job(&fx, &["edit", "v", "--spec", &tests], "pl"),
            "spec_invalid",
            2
        )["details"]["path"],
        "$.tests"
    );
}
