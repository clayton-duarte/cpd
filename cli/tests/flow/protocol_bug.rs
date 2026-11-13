//! `cpd protocol bug report|mitigate|close|list|show` (commands.md §33; contract §2.2 "Bug record";
//! air-d103, air-d104, air-d107): the bug report protocol as commands over the `bug_records` table.
//! The command tree runs in this process over the flow harness `Fx`; only the store is touched, so no
//! external dependency is reached.

use crate::common;

use common::fx::Fx;
use serde_json::Value;

/// A fresh store with the machine config, on the fake ports; no command of this file needs a Session or a Worker.
fn bug_fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_config("version = 1\nmachine = \"m1\"\n");
    fx
}

/// `cpd protocol bug report` and its `bug` object.
fn report(fx: &Fx, title: &str, symptom: &str) -> Value {
    fx.cpd(&[
        "protocol",
        "bug",
        "report",
        "--title",
        title,
        "--symptom",
        symptom,
    ])
    .ok()["bug"]
        .clone()
}

/// `cpd protocol bug show <id>` and its `bug` object.
fn show(fx: &Fx, id: &str) -> Value {
    fx.cpd(&["protocol", "bug", "show", id]).ok()["bug"].clone()
}

/// The issue every mitigation of these tests names (`--issue` is required: a bug is not mitigated without its issue).
const ISSUE: &str = "https://github.com/clayton-duarte/cpd/issues/1156";

/// `cpd protocol bug mitigate <id> --mitigation <text> --issue <ISSUE>` as Session `session`, and its `bug` object.
fn mitigate(fx: &Fx, id: &str, mitigation: &str, session: &str) -> Value {
    mitigate_run(fx, id, mitigation, &["--issue", ISSUE], session).ok()["bug"].clone()
}

/// `cpd protocol bug mitigate <id> --mitigation <text> <extra>` as Session `session`, as it is.
fn mitigate_run(fx: &Fx, id: &str, mitigation: &str, extra: &[&str], session: &str) -> common::Run {
    let mut args = vec![
        "protocol",
        "bug",
        "mitigate",
        id,
        "--mitigation",
        mitigation,
    ];
    args.extend(extra);
    fx.cpd_with(fx.env.root(), &args, &[("CPD_SESSION", session)])
}

/// The bug ids of a `protocol bug list` result, in order.
fn ids(out: &Value) -> Vec<&str> {
    out["bugs"]
        .as_array()
        .unwrap_or_else(|| panic!("a bugs array in {out}"))
        .iter()
        .map(|b| {
            b["id"]
                .as_str()
                .unwrap_or_else(|| panic!("a bug id in {b}"))
        })
        .collect()
}

/// A bug is inserted `open`, with no cause, no mitigation and no fix: every protocol field but its id and
/// what was seen is null, and `mitigation applied: no` follows from `mitigation`, `mitigated_by` and
/// `mitigated_at` all being null (contract §2.2 "Bug record", commands.md §33.1; air-d103).
#[test]
fn report_records_a_bug_with_mitigation_not_applied() {
    let fx = bug_fx();
    let bug = report(
        &fx,
        "pr merge refused on a clean stacked PR",
        "A stacked PR with no conflict was refused `needs_other_workflow`.",
    );

    assert_eq!(bug["id"], "b1");
    assert_eq!(bug["status"], "open");
    assert_eq!(bug["title"], "pr merge refused on a clean stacked PR");
    assert_eq!(
        bug["symptom"],
        "A stacked PR with no conflict was refused `needs_other_workflow`."
    );
    for field in [
        "plan",
        "cause",
        "mitigation",
        "mitigated_by",
        "mitigated_at",
        "root_fix",
        "regression_test",
        "closed_at",
        "closed_by",
    ] {
        assert!(bug[field].is_null(), "report set {field}: {bug}");
    }
    // The command ran outside a Session, so `reported_by` is null (commands.md §33.1).
    assert!(bug["reported_by"].is_null(), "{bug}");
    assert!(bug["reported_at"].is_string(), "{bug}");

    // The record is stored: a later read is the same bug, still with no mitigation applied.
    assert_eq!(show(&fx, "b1"), bug);
}

/// A mitigation records the stopgap, the caller's Session id and when it was applied; the bug stays `open`
/// (contract §2.2 "Bug record", commands.md §33.2; air-d103).
#[test]
fn mitigate_records_who_and_when() {
    let fx = bug_fx();
    report(
        &fx,
        "the engine stopped ticking",
        "No engine step ran for an hour.",
    );

    let bug = mitigate(
        &fx,
        "b1",
        "Restarted the engine by hand and watched it tick.",
        "s-42",
    );
    assert_eq!(
        bug["mitigation"],
        "Restarted the engine by hand and watched it tick."
    );
    assert_eq!(bug["mitigated_by"], "s-42");
    assert!(bug["mitigated_at"].is_string(), "{bug}");
    assert_eq!(bug["status"], "open", "a mitigation does not close: {bug}");

    // Who and when are on the stored record, not only in this command's answer.
    let shown = show(&fx, "b1");
    assert_eq!(
        shown["mitigation"],
        "Restarted the engine by hand and watched it tick."
    );
    assert_eq!(shown["mitigated_by"], "s-42");
    assert_eq!(shown["mitigated_at"], bug["mitigated_at"]);
}

/// A close without a root-fix link and a regression test is refused `root_fix_required`, exit 3, and writes
/// nothing; with both it closes the bug (commands.md §33.3, contract §2.2 "Bug record"; air-d107).
#[test]
fn close_requires_a_root_fix_and_a_regression_test() {
    let fx = bug_fx();
    report(&fx, "pr merge refused", "A clean stack was refused.");

    // Neither flag: both are missing and the bug stays open.
    let neither = fx.cpd(&["protocol", "bug", "close", "b1"]);
    let error = common::refused(&neither, "root_fix_required", 3);
    assert_eq!(
        error["details"]["missing"],
        serde_json::json!(["root-fix", "regression-test"]),
        "{error}"
    );
    assert_eq!(show(&fx, "b1")["status"], "open");

    // Only the root fix: the regression test is still missing.
    let only_fix = fx.cpd(&[
        "protocol",
        "bug",
        "close",
        "b1",
        "--root-fix",
        "protocol-enforce/ship/b1",
    ]);
    let error = common::refused(&only_fix, "root_fix_required", 3);
    assert_eq!(
        error["details"]["missing"],
        serde_json::json!(["regression-test"]),
        "{error}"
    );
    assert_eq!(show(&fx, "b1")["status"], "open");

    // Both: it closes, keeping the root-fix reference and the regression test, and records when.
    let bug = fx
        .cpd(&[
            "protocol",
            "bug",
            "close",
            "b1",
            "--root-fix",
            "protocol-enforce/ship/b1",
            "--regression-test",
            "protocol_bug::close_requires_a_root_fix_and_a_regression_test",
        ])
        .ok()["bug"]
        .clone();
    assert_eq!(bug["status"], "closed");
    assert_eq!(bug["root_fix"], "protocol-enforce/ship/b1");
    assert_eq!(
        bug["regression_test"],
        "protocol_bug::close_requires_a_root_fix_and_a_regression_test"
    );
    assert!(bug["closed_at"].is_string(), "{bug}");
    assert_eq!(show(&fx, "b1"), bug);
}

/// `list` shows open bugs id-ascending, each with its mitigation state, and the `--mitigated yes|no` filter
/// is exactly that state (commands.md §33.4; air-d104: `mitigation applied: no` is queryable).
#[test]
fn list_shows_open_bugs_and_their_mitigation_state() {
    let fx = bug_fx();
    report(&fx, "first bug", "one");
    report(&fx, "second bug", "two");
    mitigate(&fx, "b1", "A stopgap.", "s-42");

    let all = fx.cpd(&["protocol", "bug", "list"]).ok();
    assert_eq!(ids(&all), ["b1", "b2"]);
    let bugs = all["bugs"].as_array().unwrap_or_else(|| panic!("{all}"));
    // b1's mitigation state is in the list; b2 reads `mitigation applied: no`.
    assert_eq!(bugs[0]["mitigation"], "A stopgap.");
    assert_eq!(bugs[0]["mitigated_by"], "s-42");
    assert!(bugs[0]["mitigated_at"].is_string(), "{all}");
    assert!(bugs[1]["mitigation"].is_null(), "{all}");
    assert!(bugs[1]["mitigated_by"].is_null(), "{all}");
    assert!(bugs[1]["mitigated_at"].is_null(), "{all}");

    let mitigated = fx
        .cpd(&["protocol", "bug", "list", "--mitigated", "yes"])
        .ok();
    assert_eq!(ids(&mitigated), ["b1"]);
    let unmitigated = fx
        .cpd(&["protocol", "bug", "list", "--mitigated", "no"])
        .ok();
    assert_eq!(ids(&unmitigated), ["b2"]);
    let open = fx
        .cpd(&["protocol", "bug", "list", "--status", "open"])
        .ok();
    assert_eq!(ids(&open), ["b1", "b2"]);
}

/// A mitigation without `--issue` is refused `issue_required`, exit 3, and writes nothing: the bug stays unmitigated.
#[test]
fn mitigate_without_issue_is_refused() {
    let fx = bug_fx();
    report(&fx, "the engine stopped ticking", "No tick for an hour.");

    let run = mitigate_run(&fx, "b1", "Restarted the engine.", &[], "s-42");
    common::refused(&run, "issue_required", 3);

    let bug = show(&fx, "b1");
    assert!(bug["mitigation"].is_null(), "{bug}");
    assert!(bug["mitigated_at"].is_null(), "{bug}");
}

/// An `--issue` that is not `https://github.com/<owner>/<repo>/issues/<N>` is refused `issue_invalid`, exit 2, and writes nothing.
#[test]
fn mitigate_with_malformed_issue_is_refused() {
    let fx = bug_fx();
    report(&fx, "the engine stopped ticking", "No tick for an hour.");

    for bad in [
        "1156",
        "#1156",
        "https://github.com/clayton-duarte/cpd/pull/1156",
        "https://github.com/clayton-duarte/cpd/issues/",
        "https://github.com/clayton-duarte/cpd/issues/abc",
        "https://example.com/clayton-duarte/cpd/issues/1156",
    ] {
        let run = mitigate_run(
            &fx,
            "b1",
            "Restarted the engine.",
            &["--issue", bad],
            "s-42",
        );
        common::refused(&run, "issue_invalid", 2);
        assert!(show(&fx, "b1")["mitigation"].is_null(), "{bad} was stored");
    }
}

/// The issue is stored on the bug: `mitigate`, `show` and `list` print it as `issue`; an unmitigated bug has `issue` null.
#[test]
fn mitigate_records_issue_shown_by_show_and_list() {
    let fx = bug_fx();
    report(&fx, "first bug", "one");
    report(&fx, "second bug", "two");
    assert!(show(&fx, "b1")["issue"].is_null());

    let bug = mitigate(&fx, "b1", "A stopgap.", "s-42");
    assert_eq!(bug["issue"], ISSUE, "{bug}");
    assert_eq!(show(&fx, "b1")["issue"], ISSUE);

    let all = fx.cpd(&["protocol", "bug", "list"]).ok();
    let bugs = all["bugs"].as_array().unwrap_or_else(|| panic!("{all}"));
    assert_eq!(bugs[0]["issue"], ISSUE, "{all}");
    assert!(bugs[1]["issue"].is_null(), "{all}");
}

/// `bug close --root-fix` of a bug with no issue is refused `bug_issue_required`, exit 3, and the bug stays open.
#[test]
fn close_root_fix_refused_without_issue() {
    let fx = bug_fx();
    report(&fx, "pr merge refused", "A clean stack was refused.");

    let run = fx.cpd(&[
        "protocol",
        "bug",
        "close",
        "b1",
        "--root-fix",
        "protocol-enforce/ship/b1",
        "--regression-test",
        "protocol_bug::close_root_fix_refused_without_issue",
    ]);
    common::refused(&run, "bug_issue_required", 3);
    assert_eq!(show(&fx, "b1")["status"], "open");
}

/// The repo `pay` (keyed `acme/pay`, with `[test-commands]`), a config and Plan `pay-1` with its Lead, for `workflow add`.
fn workflow_fx() -> (Fx, String) {
    let fx = Fx::with_ports();
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    fx.env.repo_policy(
        "acme",
        "pay",
        "[test-commands]\nbuild = \"true\"\nrun = \"true\"\n",
    );
    fx.env.write_config("version = 1\nmachine = \"m1\"\n");
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay-1', 'ctx', 'Pay the bills', 'grounding', 'lead1', '1999-12-31T00:00:00.000Z');",
    );
    (fx, repo.display().to_string())
}

/// `cpd workflow add change --repo <repo> --title Change --plan pay-1 --bug <bug>`.
fn workflow_add_bug(fx: &Fx, repo: &str, bug: &str) -> common::Run {
    fx.cpd(&[
        "workflow", "add", "change", "--repo", repo, "--title", "Change", "--shape", "config",
        "--plan", "pay-1", "--bug", bug,
    ])
}

/// `workflow add --bug <id>` of a bug with no issue is refused `bug_issue_required`, exit 3, and adds no Workflow.
#[test]
fn workflow_add_bug_refused_without_issue() {
    let (fx, repo) = workflow_fx();
    report(&fx, "pr merge refused", "A clean stack was refused.");

    let run = workflow_add_bug(&fx, &repo, "b1");
    common::refused(&run, "bug_issue_required", 3);
}

/// `workflow add --bug <id>` of an unknown bug is refused `bug_unknown`, and adds no Workflow.
#[test]
fn workflow_add_bug_unknown_is_refused() {
    let (fx, repo) = workflow_fx();

    let run = workflow_add_bug(&fx, &repo, "b99");
    // The contract names no exit code for `bug_unknown`: the code is the contract.
    assert_eq!(run.error()["code"], "bug_unknown", "{run:?}");
}
