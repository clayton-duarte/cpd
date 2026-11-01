//! `pr rerun` (decision pro-d155): re-runs the failed GitHub Actions workflow runs of a pull request's head commit.

use cpd_core::host::runner::{Call, Reply};
use serde_json::{Value, json};

use crate::common::Run;
use super::pr_view::{Pay, fx};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn has(text: &'static str) -> impl Fn(&Call) -> bool {
    move |c: &Call| c.args.join("\n").contains(text)
}

fn run(id: u64, name: &str, status: &str, conclusion: &str) -> Value {
    json!({"databaseId": id, "name": name, "status": status, "conclusion": conclusion})
}

/// Open PR with `gh` answering the head commit, the run list `runs`, and `run rerun <id> --failed` ok for each of `ok`.
fn setup(runs: &[Value], ok: &[u64]) -> Pay {
    let pay = fx("open");
    let runner = (*pay.fx.fakes.runner).clone();
    let mut runner = runner.on("gh", has("headRefOid"), Reply::ok(&json!({"headRefOid": SHA}).to_string()));
    runner = runner.on("gh", |c: &Call| c.args.iter().any(|a| a == "list") && c.args.iter().any(|a| a == SHA), Reply::ok(&Value::Array(runs.to_vec()).to_string()));
    for id in ok {
        let want = format!("run\nrerun\n{id}\n--failed");
        runner = runner.on("gh", move |c: &Call| c.args.join("\n") == want, Reply::ok(""));
    }
    pay
}

fn reruns(pay: &Pay) -> Vec<Call> {
    pay.fx.calls("gh").into_iter().filter(|c| c.args.starts_with(&["run".to_string(), "rerun".to_string()])).collect()
}

const ARGS: [&str; 5] = ["pr", "rerun", "--workflow", "repo", "--plan"];

fn rerun(pay: &Pay) -> Run {
    pay.fx.cpd(&[ARGS[0], ARGS[1], ARGS[2], ARGS[3], ARGS[4], "pay"])
}

#[test]
fn a_failed_run_of_the_head_commit_is_rerun_with_failed_only_and_listed() {
    let pay = setup(&[run(111, "CI", "completed", "success"), run(222, "Lint", "completed", "failure")], &[222]);

    let out = rerun(&pay).ok();

    assert_eq!(out, json!({"ok": true, "pr": "repo", "head_sha": SHA, "rerun": [{"id": 222, "name": "Lint"}]}));
    let calls = reruns(&pay);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].args, ["run", "rerun", "222", "--failed"]);
    assert_eq!(calls[0].cwd.as_deref(), Some(pay.fx.env.root().join("lead").as_path()));
    let list = pay.fx.calls("gh").into_iter().find(|c| c.args.starts_with(&["run".to_string(), "list".to_string()])).expect("run list call");
    let at = list.args.iter().position(|a| a == "--commit").expect("--commit");
    assert_eq!(list.args[at + 1], SHA);
}

#[test]
fn every_failed_conclusion_is_rerun_whatever_its_case_and_no_other_run_is() {
    let runs = [
        run(1, "A", "completed", "failure"),
        run(2, "B", "completed", "success"),
        run(3, "C", "completed", "timed_out"),
        run(4, "D", "completed", "cancelled"),
        run(5, "E", "completed", "startup_failure"),
        run(6, "F", "completed", "skipped"),
        run(7, "G", "completed", "neutral"),
        run(8, "H", "completed", "action_required"),
        run(9, "I", "in_progress", ""),
        run(10, "J", "completed", "FAILURE"),
    ];
    let pay = setup(&runs, &[1, 3, 5, 10]);

    let out = rerun(&pay).ok();

    let want = json!([{"id": 1, "name": "A"}, {"id": 3, "name": "C"}, {"id": 5, "name": "E"}, {"id": 10, "name": "J"}]);
    assert_eq!(out, json!({"ok": true, "pr": "repo", "head_sha": SHA, "rerun": want}));
    assert_eq!(reruns(&pay).len(), 4);
}

#[test]
fn a_head_commit_with_no_failed_run_is_refused_no_failed_run_and_reruns_nothing() {
    let listed = [run(1, "CI", "completed", "success"), run(2, "Lint", "in_progress", "")];
    let pay = setup(&listed, &[]);

    let r = rerun(&pay);

    assert_eq!((r.error()["code"].as_str().unwrap(), r.exit), ("no_failed_run", 3));
    assert_eq!(
        r.error()["details"],
        json!({"pr": "repo", "head_sha": SHA, "runs": [
            {"id": 1, "name": "CI", "status": "completed", "conclusion": "success"},
            {"id": 2, "name": "Lint", "status": "in_progress", "conclusion": ""}]})
    );
    assert!(reruns(&pay).is_empty());

    drop(pay);
    let pay = setup(&[], &[]);
    let r = rerun(&pay);
    assert_eq!((r.error()["code"].as_str().unwrap(), r.exit), ("no_failed_run", 3));
    assert_eq!(r.error()["details"], json!({"pr": "repo", "head_sha": SHA, "runs": []}));
    assert!(reruns(&pay).is_empty());
}

#[test]
fn a_pr_that_is_not_open_is_refused_pr_not_open_and_calls_gh_not_at_all() {
    for status in ["waiting", "dispatch", "merged"] {
        let pay = if status == "merged" {
            let pay = fx("open");
            pay.store().write("fixture", |tx| tx.execute_batch("UPDATE prs SET status = 'merged', merged_at = 't' WHERE plan = 'pay' AND key = 'repo';").map_err(Into::into)).unwrap();
            pay
        } else {
            fx(status)
        };

        let r = rerun(&pay);

        assert_eq!((r.error()["code"].as_str().unwrap(), r.exit), ("workflow_not_open", 3), "{status}");
        assert_eq!(r.error()["details"], json!({"pr": "repo", "status": status}));
        assert!(pay.fx.calls("gh").is_empty(), "{status}");
    }
}

#[test]
fn a_failing_gh_run_rerun_is_gh_failed_and_lists_the_runs_already_rerun() {
    let pay = setup(&[run(222, "Lint", "completed", "failure"), run(333, "CI", "completed", "failure")], &[222]);
    let runner = (*pay.fx.fakes.runner).clone();
    let _ = runner.on("gh", |c: &Call| c.args.join("\n") == "run\nrerun\n333\n--failed", Reply::exit(1, "", "HTTP 502: bad gateway\n"));

    let r = rerun(&pay);

    assert_eq!((r.error()["code"].as_str().unwrap(), r.exit), ("gh_failed", 1));
    assert_eq!(r.error()["details"]["rerun"], json!([{"id": 222, "name": "Lint"}]));
}
