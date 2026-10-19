//! `plan cancel` (commands.md §4.12, E1), run as the built binary against a temporary `$CPD_HOME`,
//! a private tmux server, a fake `pi`-less cockpit (a fake `cpd` for the panes) and a fake `gh`.
//! Jobs, Attempts and PR rows that only the engine reaches are written straight into the database.

mod common;

use std::fs;
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::{CPD, Run, SLEEP_SECONDS, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

// ---------- fixtures ----------

fn store(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn fixture(env: &TestEnv, sql: &str) {
    store(env).write("fixture", |tx| exec(tx, sql)).unwrap();
}

fn query<T: rusqlite::types::FromSql>(env: &TestEnv, sql: &str) -> T {
    store(env).read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn refused(run: &Run, code: &str, exit: i32) -> Value {
    let error = run.error();
    assert_eq!(error["code"], code, "{run:?}");
    assert_eq!(run.exit, exit, "{run:?}");
    error
}

/// A fake `cpd` on `PATH`: `session run <id>` records its environment and sleeps (the pane's
/// program); `watch --ensure` succeeds; everything else is the built binary.
fn fake_cpd(env: &TestEnv) {
    env.fake(
        "cpd",
        &format!(
            "case \"$1 $2\" in\n\
             \"session run\") env > '{root}/pane-'\"$3\".env; exec sleep {SLEEP_SECONDS};;\n\
             \"watch --ensure\") echo '{{\"ok\": true, \"started\": true, \"pane\": \"%9\"}}'; exit 0;;\n\
             esac\nexec '{CPD}' \"$@\"",
            root = env.root().display()
        ),
    );
}

/// The cockpit: session `cpd`, window `main` with a tree pane and the placeholder in the chat slot.
fn cockpit(env: &TestEnv) {
    let ok = |args: &[&str]| {
        let out = env.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let tree = ok(&["new-session", "-d", "-s", "cpd", "-n", "main", "-x", "215", "-y", "62", "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    ok(&["set", "-p", "-t", &tree, "@cpd_view", "tree"]);
    let chat = ok(&["split-window", "-d", "-t", &tree, "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    ok(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
}

/// The panes of the private server: `(pane, @cpd_session)`.
fn panes(env: &TestEnv) -> Vec<(String, String)> {
    let out = env.tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{@cpd_session}"]);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let (pane, session) = l.split_once('\t').unwrap();
            (pane.to_owned(), session.to_owned())
        })
        .collect()
}

fn has_pane(env: &TestEnv, session: &str) -> bool {
    panes(env).iter().any(|p| p.1 == session)
}

/// Where the fake `session run <id>` writes its environment: it exists once the pane runs.
fn wait_for_pane(env: &TestEnv, session: &str) {
    let path = env.root().join(format!("pane-{session}.env"));
    wait_until("the pane's session run", || path.exists() && fs::read_to_string(&path).is_ok_and(|t| t.contains("CPD_SESSION")));
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
/// `gh` is a fake that records its calls and says every pull request is open.
fn setup(env: &TestEnv) -> (String, String) {
    let config = format!("version = 1\nmachine = \"m1\"\nlead_pi_packages = []\nplannotator_ports = \"19432-19432\"\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket());
    fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    fake_cpd(env);
    env.fake_gh("case \"$1 $2\" in\n\"pr view\") echo '{\"state\":\"OPEN\"}';;\nesac\nexit 0");
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    let lead = env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Plan pay"]).ok()["lead"]["session"].as_str().unwrap().to_owned();
    for key in ["api", "web", "ui"] {
        let repo = env.git_repo(key);
        env.cpd(&["pr", "add", key, "--repo", repo.to_str().unwrap(), "--plan", "pay"]).ok();
    }
    wait_for_pane(env, &lead);
    let fork = env.cpd(&["session", "fork", &lead]).ok()["session"]["id"].as_str().unwrap().to_owned();
    wait_for_pane(env, &fork);

    // The Jobs and PRs reach their states the way the engine moves them, one legal step at a time.
    let pull_request = |repo: &str, number: i64, state: &str| {
        let merged = if state == "merged" { "'t'" } else { "NULL" };
        format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
             VALUES ('acme/{repo}', {number}, 'https://github.com/acme/{repo}/pull/{number}', 'T', 0, '{state}', 'none', 'none', 't', {merged}, 't');"
        )
    };
    let start_pr = |key: &str| {
        format!("UPDATE prs SET status = 'dispatch', lead_worktree = '/wt/{key}', started_at = 't' WHERE plan = 'pay' AND key = '{key}';")
    };
    let open_pr = |key: &str, number: i64, status: &str| {
        let merged = if status == "merged" { ", merged_at = 't'" } else { "" };
        format!(
            "UPDATE prs SET status = 'open', pr_number = {number}, pr_url = 'https://github.com/acme/{key}/pull/{number}', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/{key}' AND number = {number})
              WHERE plan = 'pay' AND key = '{key}';
             UPDATE prs SET status = '{status}'{merged} WHERE plan = 'pay' AND key = '{key}' AND status <> '{status}';"
        )
    };
    let draft = |key: &str| {
        format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
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
    fixture(
        env,
        &format!(
            "UPDATE plans SET phase = 'plan' WHERE id = 'pay';
             {d}{q}{f}{r}{m}
             UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
             UPDATE jobs SET status = 'queued', fingerprint = '{fingerprint}' WHERE key <> 'd';
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

/// What the engine does for the stopped Attempts: ends them `cancelled`, and their Jobs.
fn engine_ends_attempts(env: &TestEnv) {
    fixture(
        env,
        "UPDATE attempts SET status = 'ended', outcome = 'cancelled', cause = 'cancel_requested', ended_at = 't' WHERE status <> 'ended';
         UPDATE jobs SET status = 'cancelled' WHERE status = 'running';",
    );
}

/// `cpd plan cancel` running in the background, so the test can play the engine while it waits.
fn spawn_cancel(env: &TestEnv, extra: &[&str]) -> std::process::Child {
    let mut args = vec!["plan", "cancel", "--plan", "pay"];
    args.extend(extra);
    env.command(CPD).args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
}

fn finish(child: std::process::Child) -> Run {
    let out = child.wait_with_output().unwrap();
    Run {
        exit: out.status.code().unwrap(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Runs `plan cancel`, and ends the running Attempt as soon as the stop request is recorded.
fn cancel_with_engine(env: &TestEnv, extra: &[&str]) -> Run {
    let child = spawn_cancel(env, extra);
    wait_until("the stop request", || query::<i64>(env, "SELECT count(*) FROM events WHERE kind = 'job-cancel-requested'") == 1);
    engine_ends_attempts(env);
    finish(child)
}

fn job_status(env: &TestEnv, key: &str) -> String {
    query(env, &format!("SELECT status FROM jobs WHERE key = '{key}'"))
}

fn pr_status(env: &TestEnv, key: &str) -> String {
    query(env, &format!("SELECT status FROM prs WHERE key = '{key}'"))
}

fn gh_closes(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("gh").into_iter().filter(|c| c[..2] == ["pr", "close"]).collect()
}

// ---------- the command ----------

#[test]
fn cancels_jobs_closes_open_prs_and_the_lead_and_archives_the_plan() {
    let env = TestEnv::new();
    let (lead, fork) = setup(&env);
    let (lead_pane, fork_pane) = (
        panes(&env).into_iter().find(|p| p.1 == lead).unwrap().0,
        panes(&env).into_iter().find(|p| p.1 == fork).unwrap().0,
    );

    let out = cancel_with_engine(&env, &["--reason", "abandoned"]).ok();

    // Jobs: every one not merged ends cancelled; the running one by the engine.
    for key in ["d", "q", "f", "r"] {
        assert_eq!(job_status(&env, key), "cancelled", "{key}");
    }
    assert_eq!(job_status(&env, "m"), "merged");
    let mut cancelled: Vec<&str> = out["cancelled"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
    cancelled.sort();
    assert_eq!(cancelled, ["d", "f", "q", "r"]);
    assert_eq!(query::<String>(&env, "SELECT cancel_reason FROM jobs WHERE key = 'q'"), "abandoned");
    assert_eq!(events(&env, "job-cancelled").iter().filter(|e| e["data"]["reason"] == "abandoned").count(), 3);
    assert_eq!(events(&env, "job-cancel-requested").len(), 1);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM attempts WHERE status <> 'ended'"), 0);

    // The open Escalation expired.
    assert_eq!(query::<String>(&env, "SELECT status FROM escalations"), "expired");
    assert_eq!(events(&env, "escalation-expired")[0]["data"], json!({"escalation": "r-a1-q1", "reason": "abandoned"}));

    // Pull requests: the open one is closed, the merged one and the waiting PR are not touched.
    assert_eq!(gh_closes(&env), [["pr", "close", "11", "--repo", "acme/api"]]);
    assert_eq!(out["closed_prs"], json!([{"pr": "api", "number": 11}]));
    assert_eq!((pr_status(&env, "api").as_str(), pr_status(&env, "web").as_str(), pr_status(&env, "ui").as_str()), ("open", "merged", "waiting"));

    // The Lead and its fork lose their panes.
    assert!(!has_pane(&env, &lead) && !has_pane(&env, &fork));
    let mut closed: Vec<&str> = out["closed"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
    closed.sort();
    let mut panes_closed = vec![lead_pane.as_str(), fork_pane.as_str()];
    panes_closed.sort();
    assert_eq!(closed, panes_closed);

    // The Plan and its Sessions are archived: the Plan holds no port.
    assert_eq!(out["archived"][0], json!({"kind": "plan", "id": "pay"}));
    assert_eq!(out["archived"].as_array().unwrap().len(), 3);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"), 1);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM sessions WHERE archived_at IS NULL"), 0);
    assert_eq!(events(&env, "plan-archived").len(), 1);
    assert_eq!(events(&env, "session-archived").len(), 2);
    assert!(out.get("already").is_none());
}

#[test]
fn reason_defaults_to_plan_cancelled() {
    let env = TestEnv::new();
    setup(&env);
    cancel_with_engine(&env, &[]).ok();
    assert_eq!(query::<String>(&env, "SELECT cancel_reason FROM jobs WHERE key = 'q'"), "plan cancelled");
}

#[test]
fn a_rerun_writes_nothing_and_calls_nothing() {
    let env = TestEnv::new();
    setup(&env);
    cancel_with_engine(&env, &[]).ok();
    // The poller has read the closure since.
    fixture(&env, "UPDATE pull_requests SET state = 'closed' WHERE repo = 'acme/api';");
    let (events_before, calls) = (query::<i64>(&env, "SELECT count(*) FROM events"), env.calls("gh").len());

    let again = env.cpd(&["plan", "cancel", "--plan", "pay"]).ok();
    assert_eq!(again, json!({"ok": true, "cancelled": [], "closed_prs": [], "closed": [], "archived": [], "already": true}));
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM events"), events_before);
    assert_eq!(env.calls("gh").len(), calls);
}

#[test]
fn a_rerun_finishes_what_a_failed_run_left() {
    let env = TestEnv::new();
    let (lead, _) = setup(&env);
    // gh fails: the Jobs are cancelled, the pull request is not closed, nothing is archived.
    env.fake_gh("echo 'HTTP 502' >&2; exit 1");
    let failed = cancel_with_engine(&env, &[]);
    let error = refused(&failed, "gh_failed", 1);
    assert_eq!(error["details"]["argv"], json!(["gh", "pr", "close", "11", "--repo", "acme/api"]));
    assert_eq!(job_status(&env, "q"), "cancelled");
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"), 0);
    assert!(has_pane(&env, &lead), "the Lead is closed after the pull requests");

    // gh works again: the rest is done.
    env.fake_gh("exit 0");
    let out = env.cpd(&["plan", "cancel", "--plan", "pay"]).ok();
    assert_eq!(out["cancelled"], json!([]));
    assert_eq!(out["closed_prs"], json!([{"pr": "api", "number": 11}]));
    assert_eq!(out["archived"].as_array().unwrap().len(), 3);
    assert!(!has_pane(&env, &lead));
}

#[test]
fn a_failing_job_cancel_halfway_is_finished_by_a_rerun() {
    let env = TestEnv::new();
    setup(&env);
    // Job `x` is running with no Attempt: a bad row, on which `job cancel` fails after the Jobs before it.
    fixture(
        &env,
        &format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', 'api', 'x', 'Job x', 'S2', 'Goal.', 'Context.', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, cpd_txn(), 't');
             UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE key = 'x';
             UPDATE jobs SET status = 'running' WHERE key = 'x';",
            "f".repeat(64)
        ),
    );
    let first = env.cpd(&["plan", "cancel", "--plan", "pay"]);
    assert_eq!(first.error()["code"], "store_failed", "{first:?}");
    assert_eq!((job_status(&env, "d").as_str(), job_status(&env, "q").as_str(), job_status(&env, "f").as_str()), ("cancelled", "cancelled", "cancelled"));
    assert_eq!(events(&env, "job-cancel-requested").len(), 1, "the running Job before it has its request");
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"), 0);
    assert!(gh_closes(&env).is_empty(), "nothing after the failure ran");

    // The Attempt of `x` exists now: the rerun asks for both stops, and finishes.
    fixture(
        &env,
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
         VALUES ('pay', 'x-a1', 'x', 1, 'S2', 'ladder', 'running', 'cpd/pay--x-a1', '/w', 't');",
    );
    let child = spawn_cancel(&env, &[]);
    wait_until("the second stop request", || events(&env, "job-cancel-requested").len() == 2);
    engine_ends_attempts(&env);
    let out = finish(child).ok();
    assert_eq!(out["cancelled"], json!(["x"]), "`r` was asked before");
    assert_eq!(out["archived"].as_array().unwrap().len(), 3);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM jobs WHERE status NOT IN ('merged', 'cancelled')"), 0);
    assert_eq!(gh_closes(&env).len(), 1);
}

#[test]
fn a_plan_with_nothing_left_to_cancel_is_only_archived() {
    let env = TestEnv::new();
    setup(&env);
    for key in ["d", "q", "f", "r"] {
        env.cpd(&["job", "cancel", key, "--reason", "No.", "--plan", "pay"]).ok();
    }
    engine_ends_attempts(&env);
    fixture(&env, "UPDATE escalations SET status = 'expired', expired_at = 't'; UPDATE pull_requests SET state = 'closed' WHERE repo = 'acme/api';");
    let out = env.cpd(&["plan", "cancel", "--plan", "pay"]).ok();
    assert_eq!(out["cancelled"], json!([]));
    assert_eq!(out["closed_prs"], json!([]));
    assert_eq!(out["archived"].as_array().unwrap().len(), 3);
    assert!(gh_closes(&env).is_empty());
}

/// A new window running the built `cpd session run <id>` (its pi is the fake that records its port).
fn run_lead(env: &TestEnv, id: &str) -> String {
    let command = format!("'{CPD}' session run {id}");
    let out = env.tmux(&["new-window", "-d", "-t", "=cpd:", "-P", "-F", "#{pane_id}", "-e", "PLANNOTATOR_PORT=1", &command]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    env.tmux(&["set", "-p", "-t", &pane, "@cpd_session", id]);
    pane
}

fn capture(env: &TestEnv, pane: &str) -> String {
    String::from_utf8(env.tmux(&["capture-pane", "-p", "-t", pane]).stdout).unwrap()
}

#[test]
fn the_cancelled_plans_port_is_free_for_another_plan() {
    let env = TestEnv::new();
    cockpit(&env);
    setup(&env);
    // The range holds one port, and Plan `pay` has it.
    fs::create_dir_all(env.cpd_home().join("run/pay")).unwrap();
    fs::write(env.cpd_home().join("run/pay/plannotator-port"), "19432\n").unwrap();
    env.fake_pi("echo \"${PLANNOTATOR_PORT-unset}\" >> \"$HOME/ports.$CPD_SESSION\"\nexit 1");
    let other = env.cpd(&["plan", "new", "other", "--context", "ctx", "--title", "Other"]).ok()["lead"]["session"].as_str().unwrap().to_owned();
    wait_for_pane(&env, &other);
    env.cpd(&["session", "close", &other]).ok();

    let pane = run_lead(&env, &other);
    wait_until("no_port", || capture(&env, &pane).contains("Press Enter to retry."));
    assert!(capture(&env, &pane).contains("held by pay (19432)"), "{}", capture(&env, &pane));

    cancel_with_engine(&env, &[]).ok();
    env.tmux(&["send-keys", "-t", &pane, "Enter"]);
    let recorded = env.home().join(format!("ports.{other}"));
    wait_until("pi with the port", || fs::read_to_string(&recorded).is_ok_and(|t| t == "19432\n"));
}

// ---------- refusals ----------

#[test]
fn an_unknown_plan_is_plan_not_found() {
    let env = TestEnv::new();
    setup(&env);
    refused(&env.cpd(&["plan", "cancel", "--plan", "nope"]), "plan_not_found", 4);
}

#[test]
fn a_pending_plan_review_blocks_and_nothing_is_written() {
    let env = TestEnv::new();
    setup(&env);
    fixture(
        &env,
        &format!(
            "INSERT INTO plan_reviews (plan, id, number, status, fingerprint, requested_at)
             VALUES ('pay', 'r1', 1, 'pending', '{}', 't');",
            "f".repeat(64)
        ),
    );
    let before = query::<i64>(&env, "SELECT count(*) FROM events");
    let error = refused(&env.cpd(&["plan", "cancel", "--plan", "pay"]), "archive_blocked", 3);
    assert_eq!(error["details"]["blockers"][0]["kind"], "review");
    assert_eq!(error["details"]["blockers"].as_array().unwrap().len(), 1, "a running Job does not block");
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM events"), before);
    assert_eq!(job_status(&env, "q"), "queued");
}

#[test]
fn the_lead_in_the_chat_slot_blocks_and_nothing_is_written() {
    let env = TestEnv::new();
    cockpit(&env);
    let (lead, _) = setup(&env);
    env.cpd(&["session", "open", &lead]).ok();
    let before = query::<i64>(&env, "SELECT count(*) FROM events");
    let error = refused(&env.cpd(&["plan", "cancel", "--plan", "pay"]), "in_chat_slot", 3);
    assert_eq!(error["details"]["next"], "cpd session park");
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM events"), before);
    assert_eq!(job_status(&env, "q"), "queued");
    assert!(gh_closes(&env).is_empty());
}

#[test]
fn a_gh_failure_for_a_pull_request_already_closed_is_not_a_failure() {
    let env = TestEnv::new();
    setup(&env);
    // `gh pr close` fails (a merged pull request cannot be closed); `gh pr view` says it is merged.
    env.fake_gh("case \"$1 $2\" in\n\"pr close\") echo 'already merged' >&2; exit 1;;\n\"pr view\") echo '{\"state\":\"MERGED\"}';;\nesac\nexit 0");
    cancel_with_engine(&env, &[]).ok();
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM plans WHERE archived_at IS NOT NULL"), 1);
}

