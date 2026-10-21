//! Engine step E8, the Inbox (contract §2.2 "Inbox item", §3.7): with the engine running on a prepared
//! store, items open and close within two ticks with their Events; a failed Job is stuck only after
//! `inbox.stuck_job_minutes` and then goes to the Context's Manager as one message, not as an item (hs-d318);
//! and a tick with no change writes nothing.

use crate::common;

use std::fs;
use std::process::{Child, Stdio};
use std::time::Duration;

use common::{CPD, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::format_description;

/// A time `minutes` ago, as cpd writes times (contract §1.3).
fn ago(minutes: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::minutes(minutes))
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A running `cpd watch`, SIGTERMed on drop (killed if it does not stop).
struct Engine(Child);

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(self.0.id() as i32), nix::sys::signal::Signal::SIGTERM);
        common::poll(Duration::from_secs(10), || (!matches!(self.0.try_wait(), Ok(None))).then_some(()));
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A prepared store: Context `ctx`, Plan `pay` in `dispatch` with its PR `repo` and its Lead Session `lead1`
/// (`working`, in a live tmux pane so that E2 leaves it alone), a tick of `common::TICK_SECONDS` and `stuck_job_minutes = 1`.
struct Fx {
    env: TestEnv,
    pane: String,
}

fn fx() -> Fx {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[watch]\ntick_seconds = {}\n[inbox]\nstuck_job_minutes = 1\n[tmux]\nsocket = \"{}\"\n",
            common::TICK_SECONDS,
            env.tmux_socket()
        ),
    )
    .unwrap();
    // E5 runs `cpd pr poll` from `PATH`: the one under test, and a `gh` that finds no pull requests.
    env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    let out = env.tmux(&["new-session", "-d", "-s", "x", "-P", "-F", "#{pane_id}", "sleep", "600"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    let fx = Fx { env, pane };
    fx.fixture(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, tmux_pane, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', '{}', 'working', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '/nowhere', 'main', 'dispatch', 'cpd/pay', '/nowhere/lead', cpd_txn(), 't', 't');",
        fx.pane
    ));
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap(), 5000).unwrap()
    }

    fn fixture(&self, sql: &str) {
        self.store().write("fixture", |tx| exec(tx, sql)).unwrap();
    }

    /// A `failed` Job `key` of Plan `pay` whose `job-failed` Event is `minutes` old.
    fn failed_job(&self, key: &str, minutes: i64) {
        self.fixture(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                               fingerprint, failed_cause, created_txn, created_at)
             VALUES ('pay', 'repo', '{key}', 'Title', 'S2', 'Goal.', 'Because.', '[\"a\"]', '[\"ok\"]', '[]', 'failed', 0,
                     hex(zeroblob(32)), 'crashed', cpd_txn(), 't');
             INSERT INTO events (txn, at, kind, context, plan, job, text, data)
             VALUES (cpd_txn(), '{}', 'job-failed', 'ctx', 'pay', '{key}', 'x', '{{}}');",
            ago(minutes)
        ));
    }

    /// `cpd watch`, polling every tick (`common::PR_POLL_EVERY_TICK`).
    fn start(&self) -> Engine {
        let child = self.env.command(CPD).arg("watch").envs([common::PR_POLL_EVERY_TICK]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let engine = Engine(child);
        wait_until("watch-started", || self.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'watch-started'") == 1);
        engine
    }

    /// The open items: `(kind, urgency, subject, label)`.
    fn open_items(&self) -> Vec<(String, String, String, String)> {
        self.env.read(|conn| {
            let mut stmt =
                conn.prepare("SELECT kind, urgency, subject, label FROM inbox_items WHERE closed_at IS NULL ORDER BY id")?;
            Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    fn events(&self, kind: &str) -> Vec<Value> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT data FROM events WHERE kind = ?1 ORDER BY id")?;
            let rows = stmt.query_map([kind], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.iter().map(|d| serde_json::from_str(d).unwrap()).collect())
        })
    }

    /// A live Manager Session `mgr1` of Context `ctx`, idle in the live pane.
    fn manager(&self) {
        self.fixture(&format!(
            "INSERT INTO sessions (id, role, context, name, cwd, tier, tmux_pane, state, state_at, created_at)
               VALUES ('mgr1', 'manager', 'ctx', 'Mgr', '/', 'O3', '{}', 'idle', 't', 't');",
            self.pane
        ));
    }

    /// The `session-message` Events, oldest first: `(session, from, wake, text)`.
    fn messages(&self) -> Vec<(String, Option<String>, bool, String)> {
        self.store()
            .read(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT session, json_extract(data, '$.from'), json_extract(data, '$.wake'), json_extract(data, '$.text')
                       FROM events WHERE kind = 'session-message' ORDER BY id",
                )?;
                Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }
}

/// The Lead of a Plan in `dispatch` that blocks is a red `dialog` within two ticks, with `inbox-opened`; when it
/// works again the item is closed, with `inbox-closed`.
#[test]
fn a_blocked_lead_in_dispatch_is_a_red_dialog_until_it_works_again() {
    let fx = fx();
    let _engine = fx.start();
    fx.fixture("UPDATE sessions SET state = 'blocked', state_label = 'Which repo?' WHERE id = 'lead1';");

    wait_until("the dialog", || !fx.open_items().is_empty());
    assert_eq!(fx.open_items(), [("dialog".into(), "red".into(), "session:lead1".into(), "pay asks you".into())]);
    let id: i64 = fx.env.query("SELECT id FROM inbox_items WHERE kind = 'dialog'");
    assert_eq!(fx.events("inbox-opened"), [json!({"item": id, "kind": "dialog", "urgency": "red", "label": "pay asks you"})]);
    assert_eq!(fx.env.query::<String>("SELECT plan || '/' || session FROM events WHERE kind = 'inbox-opened'"), "pay/lead1");
    assert!(fx.events("inbox-closed").is_empty());

    fx.fixture("UPDATE sessions SET state = 'working', state_label = NULL WHERE id = 'lead1';");
    wait_until("the item to close", || fx.open_items().is_empty());
    assert_eq!(fx.events("inbox-closed"), [json!({"item": id, "kind": "dialog"})]);
    assert!(fx.env.query::<Option<String>>(&format!("SELECT closed_at FROM inbox_items WHERE id = {id}")).is_some());
    assert_eq!(fx.events("error"), Vec::<Value>::new());
}

/// The text of the message of a stuck Job (contract §3.7 "A stuck Job"): `<T>` stays for the Manager to choose.
fn stuck_text(key: &str, minutes: i64, cause: &str) -> String {
    format!(
        "Job pay/{key} failed {minutes} min ago ({cause}) and its Lead has not retried or cancelled it. \
         Retry it (cpd job retry {key} --plan pay --tier <T>), nudge the Lead, or ask the User."
    )
}

/// hs-d318: a failed Job, `stuck_job_minutes` (here 1) after its `job-failed` Event and with its Lead not working, is one
/// message to its Context's Manager, from the User's side waking, once, and opens no `job-stuck` item. A fresh
/// failure is not stuck yet.
#[test]
fn a_stuck_job_is_one_message_to_the_manager_and_opens_no_item() {
    let fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';"); // a working Lead holds the message back (audit Q1)
    fx.manager();
    fx.failed_job("old", 2);
    fx.failed_job("new", 0);
    let _engine = fx.start();

    wait_until("the message", || !fx.messages().is_empty());
    // Three whole ticks later: still the one message, the fresh failure is not stuck, and no item opened.
    fx.env.wait_whole_ticks(3);
    assert_eq!(fx.messages(), [("mgr1".to_owned(), None, true, stuck_text("old", 2, "crashed"))]);
    assert_eq!(fx.open_items(), []);
    assert_eq!(fx.events("inbox-opened"), Vec::<Value>::new());
}

/// The record that a Job was told of is the message itself, in the store: a restarted engine sends nothing again.
#[test]
fn a_restarted_engine_does_not_tell_the_manager_of_the_same_failure_again() {
    let fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    fx.manager();
    fx.failed_job("old", 2);
    let first = fx.start();
    wait_until("the message", || fx.messages().len() == 1);
    drop(first); // SIGTERM, and it is gone when the drop returns

    let second = Engine(fx.env.command(CPD).arg("watch").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap());
    wait_until("the second watch-started", || fx.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'watch-started'") == 2);
    fx.env.wait_whole_ticks(3);
    // The restarted engine also tells the Manager how long it was down (contract §3.7, hs-d328): not a stuck-Job message.
    let messages: Vec<_> = fx.messages().into_iter().filter(|(_, _, _, text)| !text.contains("The engine was down for ")).collect();
    assert_eq!(messages, [("mgr1".to_owned(), None, true, stuck_text("old", 2, "crashed"))]);
    drop(second);
}

/// After `job retry` and a new failure (a newer `job-failed` Event) the Job is told again, once more.
#[test]
fn a_job_that_fails_again_after_a_retry_is_sent_again() {
    let fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    fx.manager();
    fx.failed_job("old", 2);
    let _engine = fx.start();
    wait_until("the first message", || fx.messages().len() == 1);

    // The retry and the new failure in one transaction: the engine sees only the Job failed again.
    fx.fixture(&format!(
        "UPDATE jobs SET status = 'queued', failed_cause = NULL WHERE plan = 'pay' AND key = 'old';
         UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'old';
         UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE plan = 'pay' AND key = 'old';
         INSERT INTO events (txn, at, kind, context, plan, job, text, data)
         VALUES (cpd_txn(), '{}', 'job-failed', 'ctx', 'pay', 'old', 'x', '{{}}');",
        ago(3)
    ));
    wait_until("the second message", || fx.messages().len() == 2);
    fx.env.wait_whole_ticks(3);
    assert_eq!(
        fx.messages().into_iter().map(|m| m.3).collect::<Vec<_>>(),
        [stuck_text("old", 2, "crashed"), stuck_text("old", 3, "timed_out")]
    );
    assert_eq!(fx.open_items(), []);
}

/// No Manager: nothing is sent and nothing is written, and a Manager that comes later is told at the next tick.
#[test]
fn with_no_manager_nothing_is_sent_and_a_later_manager_is_told() {
    let fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    fx.failed_job("old", 2);
    let _engine = fx.start();
    fx.env.wait_whole_ticks(3);
    assert_eq!(fx.messages(), []);
    assert_eq!(fx.open_items(), []);
    let txns = fx.env.query::<i64>("SELECT count(*) FROM txns");
    fx.env.wait_whole_ticks(2);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns, "a stuck Job nobody can be told of writes nothing");

    fx.manager();
    wait_until("the message", || !fx.messages().is_empty());
    assert_eq!(fx.messages(), [("mgr1".to_owned(), None, true, stuck_text("old", 2, "crashed"))]);
}

/// A working Lead is handling the failure: nothing is sent until it stops (audit Q1).
#[test]
fn a_working_lead_holds_the_message_back() {
    let fx = fx();
    fx.manager();
    fx.failed_job("old", 2);
    let _engine = fx.start();
    fx.env.wait_whole_ticks(3);
    assert_eq!(fx.messages(), []);

    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    wait_until("the message", || !fx.messages().is_empty());
    assert_eq!(fx.messages().len(), 1);
}

/// An item an older engine opened for a stuck Job closes at the next reconcile, with its Event, and none opens again.
#[test]
fn an_open_job_stuck_item_closes_and_none_opens() {
    let fx = fx();
    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    fx.manager();
    fx.failed_job("old", 2);
    fx.fixture(
        "INSERT INTO inbox_items (kind, urgency, subject, label, context, plan, job, opened_at)
         VALUES ('job-stuck', 'red', 'job:pay/old', 'pay/old failed', 'ctx', 'pay', 'old', 't');",
    );
    assert_eq!(fx.open_items().len(), 1);
    let _engine = fx.start();

    wait_until("the item to close", || fx.open_items().is_empty());
    assert_eq!(fx.events("inbox-closed").len(), 1);
    fx.env.wait_whole_ticks(2);
    assert_eq!(fx.open_items(), []);
    assert_eq!(fx.events("inbox-opened"), Vec::<Value>::new());
}

/// A tick that changes nothing writes nothing: no `txns` row, no Event (a stuck Job already sent included).
#[test]
fn a_tick_with_no_change_writes_no_transaction() {
    let fx = fx();
    fx.manager();
    fx.fixture("UPDATE sessions SET state = 'blocked', state_label = 'Which repo?' WHERE id = 'lead1';");
    fx.failed_job("old", 2);
    let _engine = fx.start();
    wait_until("the dialog", || !fx.open_items().is_empty());
    // The dialog holds the Lead `blocked`, which is not `working`: the stuck Job is sent as well.
    wait_until("the message", || !fx.messages().is_empty());
    let reconciles = "SELECT count(*) FROM txns WHERE command = 'inbox reconcile'";
    assert_eq!(fx.env.query::<i64>(reconciles), 1);
    let (txns, events) = (fx.env.query::<i64>("SELECT count(*) FROM txns"), fx.env.query::<i64>("SELECT count(*) FROM events"));

    fx.env.wait_whole_ticks(3);
    assert_eq!(fx.env.query::<i64>(reconciles), 1);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns, "no transaction of any step");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM events"), events);
}

/// Re-fakes `gh` so the PR poll's search returns missionlane/cc-api-orc-apollo#2385 with the given review decision and merge state.
fn gh_returns_2385(fx: &Fx, review_decision: &str, merge_state: &str) {
    let node = format!(
        r#"{{"repository":{{"nameWithOwner":"missionlane/cc-api-orc-apollo"}},"number":2385,"url":"https://github.com/missionlane/cc-api-orc-apollo/pull/2385","title":"T2385","isDraft":false,"state":"OPEN","createdAt":"2026-10-20T10:00:00Z","mergedAt":null,"baseRefName":"main","reviewDecision":"{review_decision}","mergeable":"MERGEABLE","mergeStateStatus":"{merge_state}","commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{{"state":"SUCCESS"}}}}}}]}}}}"#
    );
    fx.env.fake_gh(&format!(
        r#"echo '{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}}}}'"#
    ));
}

const PR_READY_OPEN: &str = "SELECT count(*) FROM inbox_items WHERE kind = 'pr-ready' AND closed_at IS NULL";

/// hs-d481: a pull request GitHub reports REVIEW_REQUIRED and BLOCKED is never `pr-ready`, through the poll.
/// Red until ready-impl merges (E8's rule); `a_clean_approved_pull_request_is_ready` passes on both.
#[test]
fn a_review_required_blocked_pull_request_is_never_ready() {
    let fx = fx();
    gh_returns_2385(&fx, "REVIEW_REQUIRED", "BLOCKED");
    let _engine = fx.start();
    wait_until("the polled row", || {
        fx.env.query::<i64>("SELECT count(*) FROM pull_requests WHERE number = 2385 AND merge_state = 'blocked'") == 1
    });
    fx.env.wait_whole_ticks(2);
    assert_eq!(fx.env.query::<i64>(PR_READY_OPEN), 0, "{:?}", fx.open_items());
}

/// The positive control: the same pull request, approved and clean, opens one `pr-ready` item.
#[test]
fn a_clean_approved_pull_request_is_ready() {
    let fx = fx();
    gh_returns_2385(&fx, "APPROVED", "CLEAN");
    let _engine = fx.start();
    wait_until("the pr-ready item", || fx.env.query::<i64>(PR_READY_OPEN) == 1);
}
