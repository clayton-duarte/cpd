//! `plan cancel` (commands.md §4.12, E1), in this process (`Fx::with_ports`) against a temporary `$CPD_HOME`: tmux and git are
//! the fake routes of the `FakeRunner`, `gh` is a scripted reply of it, and the wait for the stopped Attempts naps on the
//! `ManualClock` (`Fx::on_nap` plays the engine that ends them). Jobs, Attempts and PR rows that only the engine reaches are
//! written straight into the database.

use crate::common;

use std::path::Path;

use common::fx::Fx;
use common::{Run, refused};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Reply};
use cpd_core::store::Store;
use serde_json::{Value, json};

// ---------- fixtures ----------

fn events(fx: &Fx, kind: &str) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

fn has_pane(fx: &Fx, session: &str) -> bool {
    fx.panes().iter().any(|p| p.session == session)
}

fn pane_of(fx: &Fx, session: &str) -> String {
    fx.panes().into_iter().find(|p| p.session == session).unwrap().pane
}

/// `gh pr <sub> ...`.
fn gh_pr(sub: &'static str) -> impl Fn(&Call) -> bool + Send + Sync + 'static {
    move |call| call.args.first().is_some_and(|a| a == "pr") && call.args.get(1).is_some_and(|a| a == sub)
}

/// `gh` says every pull request is open and takes everything else.
fn gh_says_open(fx: &Fx) {
    let _ = (*fx.fakes.runner).clone().on("gh", gh_pr("view"), Reply::ok("{\"state\":\"OPEN\"}\n"));
    fx.fake_gh(Reply::ok(""));
}

/// Plan `pay` in phase `dispatch` with its Lead and a side fork (both with a live pane), three PRs
/// and Jobs in every state:
///
/// | PR | status | its pull request | Jobs |
/// |---|---|---|---|
/// | `api` | `open` | `acme/api#11`, open | `d` draft, `q` queued, `f` failed, `r` running (Attempt `r-a1`), `m` merged |
/// | `web` | `open` | `acme/web#22`, merged by the poller | |
/// | `ui` | `waiting` | none | |
///
/// `gh` is whatever the test scripted before; [`setup`] scripts [`gh_says_open`].
fn world(fx: &Fx) -> (String, String) {
    fx.env.write_tmux_config("");
    fx.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    let lead = fx.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Plan pay"]).ok()["lead"]["session"].as_str().unwrap().to_owned();
    for key in ["api", "web", "ui"] {
        let repo = fx.keyed_repo(key);
        fx.cpd(&["workflow", "add", key, "--repo", repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pay"]).ok();
        // The stub the shape lays out is cancelled, as a Lead cancels a stub it does not need: these Jobs are the table's.
        fx.cpd(&["job", "cancel", &format!("{key}-config"), "--reason", "a stub of the template", "--plan", "pay"]).ok();
        fx.cpd(&["job", "cancel", &format!("open-{key}"), "--reason", "a stub of the template", "--plan", "pay"]).ok();
    }
    let fork = fx.cpd(&["session", "fork", &lead]).ok()["session"]["id"].as_str().unwrap().to_owned();

    // The Jobs and PRs reach their states the way the engine moves them, one legal step at a time.
    let pull_request = |repo: &str, number: i64, state: &str| {
        let merged = if state == "merged" { "'t'" } else { "NULL" };
        format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
             VALUES ('acme/{repo}', {number}, 'https://github.com/acme/{repo}/pull/{number}', 'T', 0, '{state}', 'none', 'none', 't', {merged}, 't');"
        )
    };
    let start_pr = |key: &str| {
        format!("UPDATE workflows SET status = 'dispatch', lead_worktree = '/wt/{key}', started_at = 't' WHERE plan = 'pay' AND key = '{key}';")
    };
    let open_pr = |key: &str, number: i64, status: &str| {
        let merged = if status == "merged" { ", merged_at = 't'" } else { "" };
        format!(
            "UPDATE workflows SET status = 'open', pr_number = {number}, pr_url = 'https://github.com/acme/{key}/pull/{number}', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/{key}' AND number = {number})
              WHERE plan = 'pay' AND key = '{key}';
             UPDATE workflows SET status = '{status}'{merged} WHERE plan = 'pay' AND key = '{key}' AND status <> '{status}';"
        )
    };
    let draft = |key: &str| {
        format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', 'api', '{key}', 'Job {key}', 'S2', 'Goal.', 'Context.', '[\"src/{key}.rs\"]', '[\"cargo test\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        )
    };
    let attempt = |key: &str, ended: Option<(&str, &str)>| {
        let (status, outcome, cause, ended_at) = match ended {
            Some((outcome, cause)) => ("ended", format!("'{outcome}'"), format!("'{cause}'"), "'t'"),
            None => ("running", "NULL".to_owned(), "NULL".to_owned(), "NULL"),
        };
        format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at)
             VALUES ('pay', '{key}-a1', '{key}', 1, 'S2', 'ladder', '{status}', {outcome}, {cause}, 'cpd/pay--{key}-a1', '/w', 't', {ended_at});"
        )
    };
    let fingerprint = "f".repeat(64);
    fx.env.sql(
        &format!(
            "UPDATE plans SET phase = 'plan' WHERE id = 'pay';
             {d}{q}{f}{r}{m}
             UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
             UPDATE jobs SET status = 'queued', fingerprint = '{fingerprint}' WHERE key <> 'd' AND status = 'draft';
             {start_api}{start_web}
             UPDATE jobs SET status = 'running' WHERE key IN ('f', 'r', 'm');
             {attempt_f}{attempt_r}
             UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'f';
             UPDATE jobs SET status = 'merged', merged_sha = 'c0ffee' WHERE key = 'm';
             {api}{web}{open_api}{open_web}
             INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
               VALUES ('pay', 'r-a1-q1', 'r-a1', 1, 'Which?', 'open', 1, 't');",
            d = draft("d"),
            q = draft("q"),
            f = draft("f"),
            r = draft("r"),
            m = draft("m"),
            start_api = start_pr("api"),
            start_web = start_pr("web"),
            attempt_f = attempt("f", Some(("failed", "timed_out"))),
            attempt_r = attempt("r", None),
            api = pull_request("api", 11, "open"),
            web = pull_request("web", 22, "merged"),
            open_api = open_pr("api", 11, "open"),
            open_web = open_pr("web", 22, "merged"),
        ),
    );
    (lead, fork)
}

fn setup(fx: &Fx) -> (String, String) {
    gh_says_open(fx);
    world(fx)
}


/// What the engine does for the stopped Attempts: ends them `cancelled`, and their Jobs.
const ENDS_ATTEMPTS: &str = "UPDATE attempts SET status = 'ended', outcome = 'cancelled', cause = 'cancel_requested', ended_at = 't' WHERE status <> 'ended';
         UPDATE jobs SET status = 'cancelled' WHERE status = 'running';";

/// Runs `plan cancel`, and plays the engine: at the command's first nap (it waits for the stopped Attempts on the
/// `ManualClock`, after the stop requests are recorded) the Attempts end, by SQL on a connection of the hook's own.
fn cancel_with_engine(fx: &mut Fx, extra: &[&str]) -> Run {
    let home = fx.env.cpd_home();
    fx.on_nap(move |_| ends_attempts(&home));
    let mut args = vec!["plan", "cancel", "--plan", "pay"];
    args.extend(extra);
    fx.cpd(&args)
}

fn ends_attempts(home: &Path) {
    let mut store = Store::open(&CpdHome::resolve(Some(home.into()), None).unwrap(), 5000).unwrap();
    common::sql(&mut store, ENDS_ATTEMPTS);
}

fn job_status(fx: &Fx, key: &str) -> String {
    fx.env.query(&format!("SELECT status FROM jobs WHERE key = '{key}'"))
}

fn pr_status(fx: &Fx, key: &str) -> String {
    fx.env.query(&format!("SELECT status FROM workflows WHERE key = '{key}'"))
}

fn gh_closes(fx: &Fx) -> Vec<Vec<String>> {
    fx.calls("gh").into_iter().map(|c| c.args).filter(|a| a[..2] == ["pr", "close"]).collect()
}

// ---------- the command ----------

#[test]
fn cancels_jobs_closes_open_pull_requests_and_the_lead_and_archives_the_plan() {
    let mut fx = Fx::with_ports();
    let (lead, fork) = setup(&fx);
    let (lead_pane, fork_pane) = (pane_of(&fx, &lead), pane_of(&fx, &fork));

    let out = cancel_with_engine(&mut fx, &["--reason", "abandoned"]).ok();

    // Jobs: every one not merged ends cancelled; the running one by the engine.
    for key in ["d", "q", "f", "r"] {
        assert_eq!(job_status(&fx, key), "cancelled", "{key}");
    }
    assert_eq!(job_status(&fx, "m"), "merged");
    let mut cancelled: Vec<&str> = out["cancelled"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
    cancelled.sort();
    assert_eq!(cancelled, ["d", "f", "q", "r"]);
    assert_eq!(fx.env.query::<String>("SELECT cancel_reason FROM jobs WHERE key = 'q'"), "abandoned");
    assert_eq!(events(&fx, "job-cancelled").iter().filter(|e| e["data"]["reason"] == "abandoned").count(), 3);
    assert_eq!(events(&fx, "job-cancel-requested").len(), 1);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts WHERE status <> 'ended'"), 0);

    // The open Escalation expired.
    assert_eq!(fx.env.query::<String>("SELECT status FROM escalations"), "expired");
    assert_eq!(events(&fx, "escalation-expired")[0]["data"], json!({"escalation": "r-a1-q1", "reason": "abandoned"}));

    // Pull requests: the open one is closed, the merged one and the waiting PR are not touched.
    assert_eq!(gh_closes(&fx), [["pr", "close", "11", "--repo", "acme/api"]]);
    assert_eq!(out["closed_prs"], json!([{"pr": "api", "number": 11}]));
    assert_eq!((pr_status(&fx, "api").as_str(), pr_status(&fx, "web").as_str(), pr_status(&fx, "ui").as_str()), ("open", "merged", "waiting"));

    // The Lead and its fork lose their panes.
    assert!(!has_pane(&fx, &lead) && !has_pane(&fx, &fork));
    let mut closed: Vec<&str> = out["closed"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
    closed.sort();
    let mut panes_closed = vec![lead_pane.as_str(), fork_pane.as_str()];
    panes_closed.sort();
    assert_eq!(closed, panes_closed);

    // The Plan and its Sessions are archived: the Plan holds no port.
    assert_eq!(out["archived"][0], json!({"kind": "plan", "id": "pay"}));
    assert_eq!(out["archived"].as_array().unwrap().len(), 3);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"), 1);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM sessions WHERE archived_at IS NULL"), 0);
    assert_eq!(events(&fx, "plan-archived").len(), 1);
    assert_eq!(events(&fx, "session-archived").len(), 2);
    assert!(out.get("already").is_none());
}

#[test]
fn reason_defaults_to_plan_cancelled() {
    let mut fx = Fx::with_ports();
    setup(&fx);
    cancel_with_engine(&mut fx, &[]).ok();
    assert_eq!(fx.env.query::<String>("SELECT cancel_reason FROM jobs WHERE key = 'q'"), "plan cancelled");
}

#[test]
fn a_rerun_writes_nothing_and_calls_nothing() {
    let mut fx = Fx::with_ports();
    setup(&fx);
    cancel_with_engine(&mut fx, &[]).ok();
    // The poller has read the closure since.
    fx.env.sql("UPDATE pull_requests SET state = 'closed' WHERE repo = 'acme/api';");
    let (events_before, calls) = (fx.env.query::<i64>("SELECT count(*) FROM events"), fx.calls("gh").len());

    let again = fx.cpd(&["plan", "cancel", "--plan", "pay"]).ok();
    assert_eq!(again, json!({"ok": true, "cancelled": [], "closed_prs": [], "closed": [], "archived": [], "left_open": [], "already": true}));
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM events"), events_before);
    assert_eq!(fx.calls("gh").len(), calls);
}

#[test]
fn a_rerun_finishes_what_a_failed_run_left() {
    let mut fx = Fx::with_ports();
    // gh fails (for `pr close`, and for the `pr view` that follows it), then works again.
    fx.fake_once("gh", Reply::exit(1, "", "HTTP 502\n"));
    fx.fake_once("gh", Reply::exit(1, "", "HTTP 502\n"));
    gh_says_open(&fx);
    let (lead, _) = world(&fx);
    // The Jobs are cancelled, the pull request is not closed, nothing is archived.
    let failed = cancel_with_engine(&mut fx, &[]);
    let error = refused(&failed, "gh_failed", 1);
    assert_eq!(error["details"]["argv"], json!(["gh", "pr", "close", "11", "--repo", "acme/api"]));
    assert_eq!(job_status(&fx, "q"), "cancelled");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"), 0);
    assert!(has_pane(&fx, &lead), "the Lead is closed after the pull requests");

    // gh works again: the rest is done.
    let out = fx.cpd(&["plan", "cancel", "--plan", "pay"]).ok();
    assert_eq!(out["cancelled"], json!([]));
    assert_eq!(out["closed_prs"], json!([{"pr": "api", "number": 11}]));
    assert_eq!(out["archived"].as_array().unwrap().len(), 3);
    assert!(!has_pane(&fx, &lead));
}

#[test]
fn a_failing_job_cancel_halfway_is_finished_by_a_rerun() {
    let mut fx = Fx::with_ports();
    setup(&fx);
    // Job `x` is running with no Attempt: a bad row, on which `job cancel` fails after the Jobs before it.
    fx.env.sql(
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', 'api', 'x', 'Job x', 'S2', 'Goal.', 'Context.', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, cpd_txn(), 't');
             UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE key = 'x';
             UPDATE jobs SET status = 'running' WHERE key = 'x';",
            "f".repeat(64)
        ),
    );
    let first = fx.cpd(&["plan", "cancel", "--plan", "pay"]);
    assert_eq!(first.error()["code"], "store_failed", "{first:?}");
    assert_eq!((job_status(&fx, "d").as_str(), job_status(&fx, "q").as_str(), job_status(&fx, "f").as_str()), ("cancelled", "cancelled", "cancelled"));
    assert_eq!(events(&fx, "job-cancel-requested").len(), 1, "the running Job before it has its request");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"), 0);
    assert!(gh_closes(&fx).is_empty(), "nothing after the failure ran");

    // The Attempt of `x` exists now: the rerun asks for both stops, and finishes.
    fx.env.sql(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
         VALUES ('pay', 'x-a1', 'x', 1, 'S2', 'ladder', 'running', 'cpd/pay--x-a1', '/w', 't');",
    );
    let out = cancel_with_engine(&mut fx, &[]).ok();
    assert_eq!(events(&fx, "job-cancel-requested").len(), 2, "the second stop request");
    assert_eq!(out["cancelled"], json!(["x"]), "`r` was asked before");
    assert_eq!(out["archived"].as_array().unwrap().len(), 3);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs WHERE status NOT IN ('merged', 'cancelled')"), 0);
    assert_eq!(gh_closes(&fx).len(), 1);
}

#[test]
fn a_plan_with_nothing_left_to_cancel_is_only_archived() {
    let fx = Fx::with_ports();
    setup(&fx);
    for key in ["d", "q", "f", "r"] {
        fx.cpd(&["job", "cancel", key, "--reason", "No.", "--plan", "pay"]).ok();
    }
    fx.env.sql(ENDS_ATTEMPTS);
    fx.env.sql("UPDATE escalations SET status = 'expired', expired_at = 't'; UPDATE pull_requests SET state = 'closed' WHERE repo = 'acme/api';");
    let out = fx.cpd(&["plan", "cancel", "--plan", "pay"]).ok();
    assert_eq!(out["cancelled"], json!([]));
    assert_eq!(out["closed_prs"], json!([]));
    assert_eq!(out["archived"].as_array().unwrap().len(), 3);
    assert!(gh_closes(&fx).is_empty());
}

// ---------- a `tdd` PR's last test and impl Jobs do not stop a plan cancel (hs-d573, rule B2) ----------

/// Adds to the Plan of [`setup`] a started `tdd` PR `tdd` whose only `test` and `impl` Jobs are `tdd-tests` and
/// `tdd-impl` (`impl_status`, `queued` or `running` with its Attempt `tdd-impl-a1`), and a queued `open` Job `tdd-open`.
fn add_tdd_pr(fx: &Fx, impl_status: &str) {
    let repo = fx.keyed_repo("tdd");
    let fingerprint = "f".repeat(64);
    let job = |key: &str, kind: &str, tests: &str| {
        format!(
            "INSERT INTO jobs (plan, workflow, key, kind, title, tier, goal, context, touches, acceptance, out_of_scope, tests, test_files, status, appended, fingerprint, created_txn, created_at)
             VALUES ('pay', 'tdd', '{key}', '{kind}', 'Job {key}', 'S2', 'Goal.', 'Context.', '[\"src/{key}.rs\"]', '[\"cargo test\"]', '[]', {tests}, {tests},
                     'queued', 0, '{fingerprint}', cpd_txn(), 't');"
        )
    };
    let running = if impl_status == "running" {
        "UPDATE jobs SET status = 'running' WHERE key = 'tdd-impl';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
         VALUES ('pay', 'tdd-impl-a1', 'tdd-impl', 1, 'S2', 'ladder', 'running', 'cpd/pay--tdd-impl-a1', '/w', 't');"
    } else {
        ""
    };
    fx.env.sql(
        &format!(
            "INSERT INTO workflows (plan, key, repo, base, status, shape, lead_branch, lead_worktree, started_at, created_txn, created_at)
               VALUES ('pay', 'tdd', '{repo}', 'main', 'dispatch', 'tdd', 'cpd/pay/tdd', '/wt/tdd', 't', cpd_txn(), 't');
             {tests}{imp}{open}{running}",
            repo = repo.display(),
            tests = job("tdd-tests", "test", "'[\"t::x\"]'").replace("'[\"t::x\"]', '[\"t::x\"]'", "'[\"t::x\"]', '[\"tests/x.rs\"]'"),
            imp = job("tdd-impl", "impl", "NULL"),
            open = job("tdd-open", "open", "NULL"),
        ),
    );
}

fn cancel_by_hand_is_refused_shape_incomplete(fx: &Fx) {
    let before = fx.env.query::<i64>("SELECT count(*) FROM events");
    let error = refused(&fx.cpd(&["job", "cancel", "tdd-impl", "--reason", "No.", "--plan", "pay"]), "shape_incomplete", 3);
    assert_eq!(error["details"]["missing"], json!(["impl"]));
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM events"), before, "a refusal writes nothing");
}

#[test]
fn a_tdd_pr_with_its_last_test_and_impl_jobs_does_not_stop_a_plan_cancel() {
    let mut fx = Fx::with_ports();
    setup(&fx);
    add_tdd_pr(&fx, "queued");
    cancel_by_hand_is_refused_shape_incomplete(&fx);
    assert_eq!(job_status(&fx, "tdd-impl"), "queued");

    cancel_with_engine(&mut fx, &[]).ok();
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs WHERE status NOT IN ('merged', 'cancelled')"), 0);
    for key in ["tdd-tests", "tdd-impl", "tdd-open"] {
        assert_eq!(fx.env.query::<String>(&format!("SELECT cancel_reason FROM jobs WHERE key = '{key}'")), "plan cancelled", "{key}");
    }
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"), 1);
    fx.env.sql("UPDATE pull_requests SET state = 'closed' WHERE repo = 'acme/api';");
    assert_eq!(fx.cpd(&["plan", "cancel", "--plan", "pay"]).ok()["already"], true);
}

#[test]
fn a_plan_cancel_stops_a_running_last_impl_job_of_a_tdd_pr_without_the_shape_check() {
    let mut fx = Fx::with_ports();
    setup(&fx);
    add_tdd_pr(&fx, "running");
    cancel_by_hand_is_refused_shape_incomplete(&fx);

    // `r` and `tdd-impl` both run: two stop requests, then the engine ends the Attempts.
    cancel_with_engine(&mut fx, &[]).ok();
    assert_eq!(events(&fx, "job-cancel-requested").len(), 2, "both stop requests");
    let requested: Vec<Value> = events(&fx, "job-cancel-requested").iter().map(|e| e["job"].clone()).collect();
    assert!(requested.contains(&json!("tdd-impl")), "{requested:?}");
    assert_eq!(job_status(&fx, "tdd-impl"), "cancelled");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs WHERE status NOT IN ('merged', 'cancelled')"), 0);
}

#[test]
fn a_pending_plan_review_is_superseded_and_the_cancel_goes_on() {
    let mut fx = Fx::with_ports();
    setup(&fx);
    fx.env.sql(
        &format!(
            "INSERT INTO plan_reviews (plan, id, number, status, fingerprint, requested_at)
             VALUES ('pay', 'r1', 1, 'pending', '{}', 't');",
            "f".repeat(64)
        ),
    );
    let before: i64 = fx.env.query("SELECT max(id) FROM events");
    cancel_with_engine(&mut fx, &[]).ok();

    // The review is closed, with one Event, before the Jobs are cancelled; the Job is cancelled and the Plan archived.
    let (status, decided_at): (String, Option<String>) =
        fx.env.read(|c| c.query_row("SELECT status, decided_at FROM plan_reviews WHERE plan = 'pay' AND id = 'r1'", [], |r| Ok((r.get(0)?, r.get(1)?))));
    assert_eq!(status, "superseded");
    assert!(decided_at.is_some());
    let closed = events(&fx, "plan-review");
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert_eq!((&closed[0]["data"]["review"], &closed[0]["data"]["status"]), (&json!("r1"), &json!("superseded")));
    assert!(closed[0]["text"].as_str().unwrap().contains("cancelled"), "{}", closed[0]["text"]);
    let first_cancel: i64 = fx.env.query(&format!("SELECT min(id) FROM events WHERE kind LIKE 'job-cancel%' AND id > {before}"));
    let review_event: i64 = fx.env.query("SELECT id FROM events WHERE kind = 'plan-review'");
    assert!(before < review_event && review_event < first_cancel, "the review closes first");
    assert_eq!(job_status(&fx, "q"), "cancelled");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"), 1);

    // A rerun is already done and writes nothing.
    fx.env.sql("UPDATE pull_requests SET state = 'closed' WHERE repo = 'acme/api';");
    let events_before = fx.env.query::<i64>("SELECT count(*) FROM events");
    let again = fx.cpd(&["plan", "cancel", "--plan", "pay"]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM events"), events_before);
}

#[test]
fn the_lead_in_the_chat_slot_blocks_and_nothing_is_written() {
    let fx = Fx::with_ports();
    fx.cockpit();
    let (lead, _) = setup(&fx);
    fx.cpd(&["session", "open", &lead]).ok();
    let before = fx.env.query::<i64>("SELECT count(*) FROM events");
    let error = refused(&fx.cpd(&["plan", "cancel", "--plan", "pay"]), "in_chat_slot", 3);
    assert_eq!(error["details"]["next"], "cpd session park");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM events"), before);
    assert_eq!(job_status(&fx, "q"), "queued");
    assert!(gh_closes(&fx).is_empty());
}

#[test]
fn a_gh_failure_for_a_pull_request_already_closed_is_not_a_failure() {
    let mut fx = Fx::with_ports();
    // `gh pr close` fails (a merged pull request cannot be closed); `gh pr view` says it is merged.
    let _ = (*fx.fakes.runner).clone().on("gh", gh_pr("close"), Reply::exit(1, "", "already merged\n"));
    let _ = (*fx.fakes.runner).clone().on("gh", gh_pr("view"), Reply::ok("{\"state\":\"MERGED\"}\n"));
    fx.fake_gh(Reply::ok(""));
    world(&fx);
    cancel_with_engine(&mut fx, &[]).ok();
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"), 1);
}

// ---------- refusals ----------

#[test]
fn an_unknown_plan_is_plan_not_found() {
    let fx = Fx::new();
    fx.env.write_config("version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n");
    refused(&fx.cpd(&["plan", "cancel", "--plan", "nope"]), "plan_not_found", 4);
}
