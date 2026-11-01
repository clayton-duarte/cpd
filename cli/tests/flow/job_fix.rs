//! A Job added to an `open` PR is a feedback fix (pro-d68): only `work` Jobs may be appended, and none queues an
//! `update` Job. Written by Job fix-path-tests.

use serde_json::json;

use super::job::{count, fixture, plan, pull_request, setup, shaped_pr, spec_file, test_spec_file};
use crate::common::fx::Fx;
use crate::common::refused;

/// Plan `pl` in `deploy` whose PR `fix` is `open`, of shape `tdd`.
fn open_tdd(fx: &Fx) {
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
}

fn add(fx: &Fx, key: &str, extra: &[&str]) -> crate::common::Run {
    let spec = spec_file(&fx.env);
    let mut args = vec![
        "job",
        "add",
        key,
        "--title",
        "Fix",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--workflow",
        "fix",
        "--plan",
        "pl",
    ];
    args.extend(extra);
    fx.cpd(&args)
}

fn update_jobs(fx: &Fx) -> (i64, i64) {
    (
        fx.env.query("SELECT count(*) FROM jobs WHERE kind = 'update'"),
        fx.env.query("SELECT count(*) FROM events WHERE kind = 'job-added' AND json_extract(data, '$.kind') = 'update'"),
    )
}

#[test]
fn a_work_job_added_to_an_open_pr_is_queued_and_queues_no_update_job() {
    let fx = Fx::new();
    open_tdd(&fx);
    for key in ["fix", "fix2"] {
        let out = add(&fx, key, &[]).ok();
        assert_eq!(
            (&out["job"]["kind"], &out["job"]["status"]),
            (&json!("work"), &json!("queued")),
            "{key}"
        );
    }
    assert_eq!(
        update_jobs(&fx),
        (0, 0),
        "no update Job: a fix Job's own merge pushes"
    );
}

#[test]
fn docs_test_and_impl_jobs_are_refused_on_an_open_pr() {
    let fx = Fx::new();
    open_tdd(&fx);
    let writes = (
        count(&fx.env, "jobs"),
        count(&fx.env, "events"),
        count(&fx.env, "txns"),
    );
    for kind in ["docs", "test", "impl"] {
        // A complete `test` Spec, so that only the shape can refuse it.
        let spec = if kind == "test" {
            test_spec_file(
                &fx.env,
                "tests.json",
                json!(["t::one"]),
                json!(["tests/t.rs"]),
            )
        } else {
            spec_file(&fx.env)
        };
        let run = fx.cpd(&[
            "job",
            "add",
            "late",
            "--title",
            "Fix",
            "--tier",
            "S2",
            "--spec",
            &spec,
            "--kind",
            kind,
            "--workflow",
            "fix",
            "--plan",
            "pl",
        ]);
        let e = refused(&run, "shape_violation", 3);
        assert_eq!(e["details"]["status"], "open", "{kind}");
        assert_eq!(e["details"]["kind"], kind);
        assert_eq!(e["details"]["allowed"], json!(["work"]), "{kind}");
        assert_eq!(
            (
                count(&fx.env, "jobs"),
                count(&fx.env, "events"),
                count(&fx.env, "txns")
            ),
            writes,
            "{kind} wrote"
        );
    }
}

#[test]
fn a_waiting_tdd_pr_still_refuses_work_jobs() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    shaped_pr(&fx.env, "pl", "next", "tdd");
    let spec = spec_file(&fx.env);
    refused(
        &fx.cpd(&[
            "job",
            "add",
            "w",
            "--title",
            "W",
            "--tier",
            "S2",
            "--spec",
            &spec,
            "--workflow",
            "next",
            "--plan",
            "pl",
        ]),
        "shape_violation",
        3,
    );
}

#[test]
fn approving_a_plan_review_that_holds_drafts_of_an_open_pr_queues_no_update_job() {
    let fx = Fx::new();
    open_tdd(&fx);
    // Pull request row of the open PR exists (plan() made it); drafts are added to it in `deploy`, then reviewed.
    assert_eq!(add(&fx, "d1", &["--draft"]).ok()["job"]["status"], "draft");
    assert_eq!(add(&fx, "d2", &["--draft"]).ok()["job"]["status"], "draft");
    fx.outline_for_review("pl");
    let review = fx
        .cpd(&["plan", "review", "--plan", "pl", "--title", "Review title"])
        .ok()["review"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let out = fx
        .cpd(&["plan", "decide", &review, "--approve", "--plan", "pl"])
        .ok();
    assert_eq!(out["review"]["status"], "approved", "{out}");
    for key in ["d1", "d2"] {
        let status: String = fx.env.query(&format!(
            "SELECT status FROM jobs WHERE plan = 'pl' AND key = '{key}'"
        ));
        assert_eq!(status, "queued", "{key}");
    }
    assert_eq!(update_jobs(&fx), (0, 0));
}
