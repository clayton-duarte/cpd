//! E24 running the `jira` source (docs/architecture/swr-jira.md §5, §8, §9): the cases of the retired E19 engine tests that the
//! §14 tests do not cover. The helper is the `node` route of a `FakeRunner`; the clock is a `ManualClock`; no process, no
//! Jira, no sleep. Retired case -> test: moves and settles `a_due_row_runs_the_helper_with_the_request_and_settles_moved` ->
//! `a_moved_answer_settles_the_row_and_writes_the_issue`; `an_unchanged_answer_settles_the_row_unchanged` ->
//! `an_unchanged_answer_settles_the_row`; `a_target_with_no_status_name_settles_skipped_without_calling_the_helper` ->
//! `a_target_with_no_status_name_is_skipped_without_the_helper`; `a_cancelled_prs_row_is_not_run` ->
//! `a_cancelled_workflows_row_is_skipped_without_the_helper`; the refusal cases are `jira_refused_parks_with_one_error_event`
//! and `a_parked_row_is_reopened_once_when_the_ticket_changes`; `an_auth_failure_is_reported_once_and_retried_never_parked`
//! -> `an_auth_failure_is_one_machine_wide_event_and_the_row_stays_pending`; `a_failed_run_...` and the per-row event
//! cases (R1 to R3) are replaced by `a_transient_failure_retries_and_raises_no_event_at_once` (swr-jira.md §8, A3, J7);
//! `a_row_of_a_pr_with_no_ticket_waits_and_runs_once_the_ticket_is_set` -> core's `jira_adopt_tests`;
//! `incident_mode_holds_the_jira_step` has no equivalent: E24 is not held by incident mode.

use std::os::unix::fs::PermissionsExt;

use serde_json::json;

use super::swr;
use super::testkit::Fx;
use cpd_core::host::runner::Reply;
use cpd_core::ports::Fakes;
use cpd_core::swr::outbox;
use cpd_core::swr::source::jira::{self, Jira};

const MOVED: &str = r#"{"ok":true,"outcome":"moved","ticket":"EEC-1","target":"in_review","cloud_id":"c1","from":"In Progress","to":"In Review","summary":"S","estimates":{}}"#;
const UNCHANGED: &str = r#"{"ok":true,"outcome":"unchanged","ticket":"EEC-1","target":"in_review","cloud_id":"c1","status_name":"Done","status_category":"done","why":"done_category","summary":"S","estimates":{}}"#;
const REFUSED: &str = r#"{"ok":false,"error":{"code":"jira_refused","message":"m","details":{"reason":"no_transition"}}}"#;
const AUTH: &str = r#"{"ok":false,"error":{"code":"jira_auth","message":"Atlassian needs a sign-in.","details":{}}}"#;
const FAILED: &str = r#"{"ok":false,"error":{"code":"jira_failed","message":"transitionJiraIssue failed.","details":{"tool":"transitionJiraIssue","status_code":503}}}"#;

struct Rig {
    _kit: tempfile::TempDir,
    fx: Fx,
    fakes: Fakes,
}

/// Plan `pay`, Workflow `repo` with ticket EEC-1, the owner's names (`done` is `done_name`), the server, pi on `PATH`.
/// `set_var` is safe: nextest runs each test in its own process.
fn rig(done_name: &str) -> Rig {
    let kit = tempfile::tempdir().unwrap();
    let bin = kit.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let pi = bin.join("pi");
    std::fs::write(&pi, "").unwrap();
    std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o755)).unwrap();
    for package in ["pi-coding-agent", "pi-ai"] {
        let dist = kit
            .path()
            .join("install/releases/1.2.0/node_modules/@earendil-works")
            .join(package)
            .join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.js"), "").unwrap();
    }
    std::fs::write(kit.path().join("install/current-version"), "1.2.0\n").unwrap();
    let adapter = kit.path().join("adapter");
    std::fs::create_dir_all(&adapter).unwrap();
    std::fs::write(adapter.join("package.json"), "{}").unwrap();
    unsafe { std::env::set_var("PATH", &bin) };

    let mut fx = Fx::with_plan(&format!("pi_adapter = \"{}\"", adapter.display()));
    let fakes = Fakes::new();
    fx.engine.ports = fakes.ports.clone();
    swr::set_sources_for_test(vec![&Jira]);
    fx.fixture("UPDATE workflows SET jira = 'EEC-1' WHERE plan = 'pay' AND key = 'repo';");
    let policy = fx.engine.home.root().join("policy");
    std::fs::create_dir_all(policy.join("acme")).unwrap();
    std::fs::write(
        policy.join("host.toml"),
        "[mcp.servers.atlassian]\nurl = \"https://mcp.atlassian.com/v1/mcp/authv2\"\n",
    )
    .unwrap();
    std::fs::write(
        policy.join("acme/owner.toml"),
        format!("[pr.jira]\nin_progress = \"In Progress\"\nin_review = \"In Review\"\ndone = \"{done_name}\"\nestimate_fields = []\n"),
    )
    .unwrap();
    Rig {
        _kit: kit,
        fx,
        fakes,
    }
}

impl Rig {
    fn helper(&self, reply: Reply) {
        (*self.fakes.runner).clone().on("node", |_| true, reply);
    }

    fn enqueue(&mut self, target: &str) -> i64 {
        jira::enqueue_transition(
            &mut self.fx.engine.store,
            self.fakes.ports.clock.utc(),
            "EEC-1",
            target,
            Some("In Review"),
            json!({"plan": "pay", "workflow": "repo"}),
        )
        .unwrap()
    }

    fn status(&mut self, id: i64) -> String {
        outbox::get(&mut self.fx.engine.store, id)
            .unwrap()
            .unwrap()
            .status
    }

    fn calls(&self) -> usize {
        self.fakes.runner.calls().len()
    }
}

#[test]
fn a_moved_answer_settles_the_row_and_writes_the_issue() {
    let mut rig = rig("Done");
    rig.helper(Reply::ok(MOVED));
    let id = rig.enqueue("in_review");
    swr::run(&mut rig.fx.engine);
    assert_eq!(rig.status(id), "done");
    assert_eq!(rig.calls(), 1);
    let name: String = rig
        .fx
        .query("SELECT status_name FROM swr_jira_issues WHERE ticket = 'EEC-1'");
    assert_eq!(name, "In Review");
    assert!(rig.fx.events("error").is_empty());
}

#[test]
fn an_unchanged_answer_settles_the_row() {
    let mut rig = rig("Done");
    rig.helper(Reply::ok(UNCHANGED));
    let id = rig.enqueue("in_review");
    swr::run(&mut rig.fx.engine);
    assert_eq!(rig.status(id), "done");
    let category: String = rig
        .fx
        .query("SELECT status_category FROM swr_jira_issues WHERE ticket = 'EEC-1'");
    assert_eq!(category, "done", "the fresh read is committed");
}

#[test]
fn a_target_with_no_status_name_is_skipped_without_the_helper() {
    let mut rig = rig("");
    rig.helper(Reply::ok(MOVED));
    let id = rig.enqueue("done");
    swr::run(&mut rig.fx.engine);
    assert_eq!(rig.status(id), "done");
    assert_eq!(rig.calls(), 0);
}

#[test]
fn a_cancelled_workflows_row_is_skipped_without_the_helper() {
    let mut rig = rig("Done");
    rig.helper(Reply::ok(MOVED));
    rig.fx
        .fixture("UPDATE workflows SET status = 'cancelled', lead_worktree = NULL, started_at = NULL WHERE key = 'repo';");
    let id = rig.enqueue("in_review");
    swr::run(&mut rig.fx.engine);
    assert_eq!(rig.status(id), "done");
    assert_eq!(rig.calls(), 0);
}

#[test]
fn an_auth_failure_is_one_machine_wide_event_and_the_row_stays_pending() {
    let mut rig = rig("Done");
    rig.helper(Reply::exit(3, AUTH, ""));
    let id = rig.enqueue("in_review");
    swr::run(&mut rig.fx.engine);
    swr::run(&mut rig.fx.engine);
    assert_eq!(rig.status(id), "pending", "retried later, never parked");
    assert_eq!(rig.calls(), 1, "not claimed again before its retry time");
    let errors = rig.fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].0, None, "machine-wide");
    assert_eq!(errors[0].3["code"], "jira_auth");
}

#[test]
fn a_transient_failure_retries_and_raises_no_event_at_once() {
    let mut rig = rig("Done");
    rig.helper(Reply::exit(1, FAILED, ""));
    let id = rig.enqueue("in_review");
    swr::run(&mut rig.fx.engine);
    assert_eq!(rig.status(id), "pending");
    assert!(
        rig.fx.events("error").is_empty(),
        "an outage is reported after ten minutes"
    );
    let cached: i64 = rig.fx.query("SELECT count(*) FROM swr_jira_issues");
    assert_eq!(cached, 0, "a failure writes no row");
}

#[test]
fn a_parked_row_is_reopened_once_when_the_ticket_changes() {
    let mut rig = rig("Done");
    rig.helper(Reply::exit(1, REFUSED, ""));
    rig.enqueue("in_review");
    swr::run(&mut rig.fx.engine);
    rig.fx
        .fixture("UPDATE workflows SET jira = 'EEC-2' WHERE key = 'repo';");
    swr::run(&mut rig.fx.engine);
    swr::run(&mut rig.fx.engine);
    let reopened: i64 = rig
        .fx
        .query("SELECT count(*) FROM swr_mutations WHERE target_key = 'jira:issue:EEC-2'");
    assert_eq!(reopened, 1, "one new row, however many passes");
    assert_eq!(
        rig.fx.events("error").len(),
        2,
        "the reopened row was refused again: a second Event, no more"
    );
}
