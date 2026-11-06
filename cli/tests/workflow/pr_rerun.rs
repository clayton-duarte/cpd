//! `pr rerun` (decision pro-d155): re-runs the failed GitHub Actions workflow runs of a pull request's head commit.

use serde_json::{Value, json};

use super::pr_view::{Pay, fx};
use crate::common::Run;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// A check of the head's rollup that ran in workflow run `id` and concluded `conclusion`.
fn run(id: u64, name: &str, status: &str, conclusion: &str) -> Value {
    json!({"__typename": "CheckRun", "name": name, "conclusion": conclusion, "status": status, "detailsUrl": null,
           "checkSuite": {"workflowRun": {"databaseId": id, "workflow": {"name": name}}}})
}

/// Open PR whose cached detail has the head's checks `runs`.
fn setup(runs: &[Value]) -> Pay {
    let pay = fx("open");
    let detail = json!({"commits": {"nodes": [{"commit": {"oid": SHA, "statusCheckRollup": {"state": "FAILURE", "contexts": {"nodes": runs}}}}]}});
    pay.fx.env.sql(&format!(
        "UPDATE pull_requests SET head_oid = '{SHA}', checks = 'failed' WHERE number = 812;
         INSERT INTO swr_pr_detail (repo, number, head_oid, updated_at, hydrated_at, detail_json, detail_hash)
           VALUES ('acme/pay', 812, '{SHA}', 'U', 't', '{detail}', 'h');"
    ));
    pay
}

fn queued(pay: &Pay) -> Vec<(String, Value, Value)> {
    pay.fx.env.read(|c| {
        let mut stmt =
            c.prepare("SELECT op_kind, vars_json, patch_json FROM swr_mutations ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    serde_json::from_str(&r.get::<_, String>(1)?).unwrap(),
                    serde_json::from_str(&r.get::<_, String>(2)?).unwrap(),
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
}

const ARGS: [&str; 5] = ["pr", "rerun", "--workflow", "repo", "--plan"];

fn rerun(pay: &Pay) -> Run {
    pay.fx
        .cpd(&[ARGS[0], ARGS[1], ARGS[2], ARGS[3], ARGS[4], "pay"])
}

#[test]
fn a_failed_run_of_the_cached_head_is_queued_with_the_checks_pending_and_listed() {
    let pay = setup(&[
        run(111, "CI", "COMPLETED", "SUCCESS"),
        run(222, "Lint", "COMPLETED", "FAILURE"),
    ]);

    let out = rerun(&pay).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert!(out["mutation_id"].is_i64());
    assert_eq!(out["head_sha"], SHA);
    assert_eq!(out["rerun"], json!([{"id": 222, "name": "Lint"}]));
    assert_eq!(out["effective"]["checks"], "pending");
    assert_eq!(
        queued(&pay),
        [(
            "rerun".to_owned(),
            json!({"run_ids": [222]}),
            json!({"checks": "pending"})
        )]
    );
    assert!(pay.fx.calls("gh").is_empty(), "the command calls no GitHub");
}

#[test]
fn every_failed_conclusion_is_queued_whatever_its_case_and_no_other_run_is() {
    let runs = [
        run(1, "A", "COMPLETED", "FAILURE"),
        run(2, "B", "COMPLETED", "SUCCESS"),
        run(3, "C", "COMPLETED", "TIMED_OUT"),
        run(4, "D", "COMPLETED", "CANCELLED"),
        run(5, "E", "COMPLETED", "STARTUP_FAILURE"),
        run(6, "F", "COMPLETED", "SKIPPED"),
        run(7, "G", "COMPLETED", "NEUTRAL"),
        run(8, "H", "COMPLETED", "ACTION_REQUIRED"),
        run(9, "I", "IN_PROGRESS", ""),
        run(10, "J", "COMPLETED", "failure"),
    ];
    let pay = setup(&runs);

    let out = rerun(&pay).ok();

    assert_eq!(
        out["rerun"],
        json!([{"id": 1, "name": "A"}, {"id": 3, "name": "C"}, {"id": 5, "name": "E"}, {"id": 10, "name": "J"}])
    );
    assert_eq!(queued(&pay)[0].1, json!({"run_ids": [1, 3, 5, 10]}));
}

#[test]
fn a_head_with_no_failed_run_is_refused_no_failed_run_and_queues_nothing() {
    for runs in [
        vec![
            run(1, "CI", "COMPLETED", "SUCCESS"),
            run(2, "Lint", "IN_PROGRESS", ""),
        ],
        vec![],
    ] {
        let pay = setup(&runs);
        let r = rerun(&pay);
        assert_eq!(
            (r.error()["code"].as_str().unwrap(), r.exit),
            ("no_failed_run", 3)
        );
        assert_eq!(r.error()["details"], json!({"pr": "repo", "head_oid": SHA}));
        assert!(queued(&pay).is_empty());
    }
}

#[test]
fn a_pr_that_is_not_open_is_refused_pr_not_open_and_calls_gh_not_at_all() {
    for status in ["waiting", "dispatch", "merged"] {
        let pay = if status == "merged" {
            let pay = fx("open");
            pay.store().write("fixture", |tx| tx.execute_batch("UPDATE workflows SET status = 'merged', merged_at = 't' WHERE plan = 'pay' AND key = 'repo';").map_err(Into::into)).unwrap();
            pay
        } else {
            fx(status)
        };

        let r = rerun(&pay);

        assert_eq!(
            (r.error()["code"].as_str().unwrap(), r.exit),
            ("workflow_not_open", 3),
            "{status}"
        );
        assert_eq!(
            r.error()["details"],
            json!({"pr": "repo", "status": status})
        );
        assert!(pay.fx.calls("gh").is_empty(), "{status}");
    }
}
