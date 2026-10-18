//! Engine step E8, the Inbox (contract §2.2 "Inbox item", §3.7): with the engine running on a prepared
//! store, items open and close within two ticks with their Events, a failed Job is stuck only after
//! `inbox.stuck_job_minutes`, and a tick with no change writes nothing.

mod common;

use std::fs;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

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
        let deadline = Instant::now() + Duration::from_secs(10);
        while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A prepared store: Context `ctx`, Plan `pay` in `dispatch` with its PR `repo` and its Lead Session `lead1`
/// (`working`, in a live tmux pane so that E2 leaves it alone), a one-second tick and `stuck_job_minutes = 1`.
struct Fx {
    env: TestEnv,
    pane: String,
}

fn fx() -> Fx {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[watch]\ntick_seconds = 1\n[inbox]\nstuck_job_minutes = 1\n[tmux]\nsocket = \"{}\"\n",
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

    fn query<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.store().read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
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

    fn start(&self) -> Engine {
        let child = self.env.command(CPD).arg("watch").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let engine = Engine(child);
        wait_until("watch-started", || self.query::<i64>("SELECT count(*) FROM events WHERE kind = 'watch-started'") == 1);
        engine
    }

    /// The open items: `(kind, urgency, subject, label)`.
    fn open_items(&self) -> Vec<(String, String, String, String)> {
        self.store()
            .read(|conn| {
                let mut stmt =
                    conn.prepare("SELECT kind, urgency, subject, label FROM inbox_items WHERE closed_at IS NULL ORDER BY id")?;
                Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    fn events(&self, kind: &str) -> Vec<Value> {
        self.store()
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT data FROM events WHERE kind = ?1 ORDER BY id")?;
                let rows = stmt.query_map([kind], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows.iter().map(|d| serde_json::from_str(d).unwrap()).collect())
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
    let id: i64 = fx.query("SELECT id FROM inbox_items WHERE kind = 'dialog'");
    assert_eq!(fx.events("inbox-opened"), [json!({"item": id, "kind": "dialog", "urgency": "red", "label": "pay asks you"})]);
    assert_eq!(fx.query::<String>("SELECT plan || '/' || session FROM events WHERE kind = 'inbox-opened'"), "pay/lead1");
    assert!(fx.events("inbox-closed").is_empty());

    fx.fixture("UPDATE sessions SET state = 'working', state_label = NULL WHERE id = 'lead1';");
    wait_until("the item to close", || fx.open_items().is_empty());
    assert_eq!(fx.events("inbox-closed"), [json!({"item": id, "kind": "dialog"})]);
    assert!(fx.query::<Option<String>>(&format!("SELECT closed_at FROM inbox_items WHERE id = {id}")).is_some());
    assert_eq!(fx.events("error"), Vec::<Value>::new());
}

/// A failed Job is `job-stuck` only once its `job-failed` Event is `stuck_job_minutes` (here 1) old.
#[test]
fn a_failed_job_is_stuck_only_after_stuck_job_minutes() {
    let fx = fx();
    fx.failed_job("old", 2);
    fx.failed_job("new", 0);
    let _engine = fx.start();

    wait_until("job-stuck", || !fx.open_items().is_empty());
    // Several ticks later the fresh failure is still not stuck.
    std::thread::sleep(Duration::from_millis(3500));
    assert_eq!(fx.open_items(), [("job-stuck".into(), "red".into(), "job:pay/old".into(), "pay/old failed".into())]);
    assert_eq!(fx.events("inbox-opened").len(), 1);

    // It clears with the Job: a cancel.
    fx.fixture(
        "UPDATE jobs SET status = 'cancelled', failed_cause = NULL, cancel_reason = 'no', cancel_requested_at = 't'
          WHERE plan = 'pay' AND key = 'old';",
    );
    wait_until("the item to close", || fx.open_items().is_empty());
    assert_eq!(fx.events("inbox-closed").len(), 1);
}

/// A tick that changes nothing writes nothing: no `txns` row, no Event.
#[test]
fn a_tick_with_no_change_writes_no_transaction() {
    let fx = fx();
    fx.failed_job("old", 2);
    let _engine = fx.start();
    wait_until("job-stuck", || !fx.open_items().is_empty());
    let reconciles = "SELECT count(*) FROM txns WHERE command = 'inbox reconcile'";
    assert_eq!(fx.query::<i64>(reconciles), 1);
    let (txns, events) = (fx.query::<i64>("SELECT count(*) FROM txns"), fx.query::<i64>("SELECT count(*) FROM events"));

    std::thread::sleep(Duration::from_millis(3500));
    assert_eq!(fx.query::<i64>(reconciles), 1);
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM txns"), txns, "no transaction of any step");
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM events"), events);
}
