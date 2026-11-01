//! Engine step E17, the Slack reaction queue (commands.md §21.8, §21.9; decisions pro-d64 step 7, pro-d107, pro-d110):
//! the engine runs the helper `pi/lib/slack-react.ts` once, detached, for the oldest `slack_reactions` row whose
//! `reacted_at` is NULL, and then sets that row's `reacted_at`. A failure leaves `reacted_at` NULL, is reported once per
//! code and the row is tried again `watch.error_repeat_seconds` after the failed run's end; one row that keeps failing
//! never blocks the rows behind it.
//!
//! In-process, on virtual time, like `watch_team.rs`: the engine runs on the `Fx`'s `ManualClock`, `FakeRunner` and
//! `FakeSpawn`, the helper is the `node` route of the `FakeRunner` (the real one is `pi/test/slack-react.test.ts`). No
//! process, no Slack, no sleep: an engine run ends when a state the test reads holds (or at [`CAP`] naps, so that a step
//! that never runs fails the test on its assertion instead of running for ever).
//!
//! The helper runs on a thread of its own and answers when its thread is scheduled, not when the engine ticks: a test
//! never counts on a tick for an answer, it runs the engine until the row or the Event it checks is there. Every test that
//! says "nothing runs" has a control: a row that must be reacted to, so that the silence is shown after the step proved
//! it is alive.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fx::Fx;
use common::healthy::PI_RELEASE;
use cpd_core::clock::{Clock, ManualClock};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::store::Store;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

const CHANNEL: &str = "C0POLICY1";
const TS: &str = "1793040197.490209";
const EMOJI: &str = "git-merge";

/// `watch.error_repeat_seconds` of the tests that do not look at it: far above the virtual time of any run.
const LONG: u32 = 3600;

/// The helper's answers (`commands.md` §21.8).
const OK: &str = r#"0 {"ok":true,"channel":"C0POLICY1","ts":"1793040197.490209","emoji":"git-merge","already":false}"#;
const ALREADY: &str = r#"0 {"ok":true,"channel":"C0POLICY1","ts":"1793040197.490209","emoji":"git-merge","already":true}"#;
const AUTH: &str = r#"3 {"ok":false,"error":{"code":"slack_auth","message":"Slack needs a sign-in. Run: pi mcp login slack","details":{}}}"#;

/// The naps of the engine between two ticks: a tick is a second (`tick_seconds = 1`) and a nap a tenth of one.
const NAPS_PER_TICK: usize = 10;

/// The most naps a run lasts: 150 virtual seconds. Every test's state holds within a few ticks.
const CAP: usize = 1500;

/// How long a gated helper waits for its gate before it gives up: only a regression waits that long.
const GATE_DEADLINE: Duration = Duration::from_secs(30);

/// The helper held in flight: it says it started, then waits for the gate to open.
struct Gate {
    entered: Mutex<Sender<()>>,
    open: Mutex<Receiver<()>>,
}

/// The `node` route: the Slack helper. It answers `<exit> <json>` (what `says` holds when it is called) and keeps every
/// call. The first call waits for its gate when there is one.
struct Node {
    says: Mutex<String>,
    /// The `--ts` values the helper answers [`AUTH`] for, whatever `says` holds.
    fails: Mutex<Vec<String>>,
    seen: Mutex<Vec<Call>>,
    gate: Mutex<Option<Gate>>,
}

impl Handler for Node {
    fn handle(&self, call: &Call) -> Reply {
        self.seen.lock().unwrap().push(call.clone());
        if let Some(gate) = self.gate.lock().unwrap().take() {
            gate.entered
                .lock()
                .unwrap()
                .send(())
                .expect("the test holds the receiver of `entered`");
            if gate
                .open
                .lock()
                .unwrap()
                .recv_timeout(GATE_DEADLINE)
                .is_err()
            {
                return Reply::exit(70, "", "the gate of the helper never opened");
            }
        }
        let ts = call
            .args
            .iter()
            .position(|a| a == "--ts")
            .map(|at| call.args[at + 1].clone());
        let answer = if ts.is_some_and(|ts| self.fails.lock().unwrap().contains(&ts)) {
            AUTH.to_owned()
        } else {
            self.says.lock().unwrap().clone()
        };
        let (status, json) = answer
            .split_once(' ')
            .expect("an answer is `<exit> <json>`");
        Reply::exit(
            status.parse().expect("the exit status is a number"),
            &format!("{json}\n"),
            "",
        )
    }
}

/// What a test reads while the engine runs: `Clone` and `'static`, so a step of the run holds one.
#[derive(Clone)]
struct Probe {
    home: PathBuf,
    node: Arc<Node>,
    clock: ManualClock,
}

impl Probe {
    fn calls(&self) -> Vec<Call> {
        self.node.seen.lock().unwrap().clone()
    }

    /// The `--ts` of each call of the helper, in order.
    fn ts_called(&self) -> Vec<String> {
        self.calls()
            .iter()
            .map(|c| c.args[c.args.iter().position(|a| a == "--ts").expect("--ts") + 1].clone())
            .collect()
    }

    fn says(&self, answer: &str) {
        *self.node.says.lock().unwrap() = answer.to_owned();
    }

    fn store(&self) -> Connection {
        Connection::open_with_flags(self.home.join("cpd.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
    }

    /// The `reacted_at` of the queue row of the post at `ts`.
    fn reacted_at(&self, ts: &str) -> Option<String> {
        self.store()
            .query_row(
                "SELECT reacted_at FROM slack_reactions WHERE ts = ?1",
                [ts],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// The data of every `error` Event but those of `gh` (E8's one lookup is not E17's).
    fn errors(&self) -> Vec<Value> {
        let conn = self.store();
        let mut stmt = conn
            .prepare("SELECT data FROM events WHERE kind = 'error' ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows.iter()
            .map(|d| serde_json::from_str::<Value>(d).unwrap())
            .filter(|d| d["code"].as_str().is_some_and(|c| c != "gh_failed"))
            .collect()
    }

    /// The kinds of the Events the engine appended but its own `watch-started` and `watch-stopped`, `error` Events apart.
    fn events_but_errors(&self) -> Vec<String> {
        let conn = self.store();
        let mut stmt = conn.prepare("SELECT kind FROM events WHERE kind NOT IN ('watch-started', 'watch-stopped', 'error') ORDER BY id").unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    /// A write of the store, as another process's: a fixture the engine finds at its next tick.
    fn write(&self, sql: &str) {
        common::sql(
            &mut Store::open(
                &CpdHome::resolve(Some(self.home.clone().into()), None).unwrap(),
                5000,
            )
            .unwrap(),
            sql,
        );
    }

    /// Moves the engine's clock forward by `seconds`.
    fn skip(&self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
    }
}

struct React {
    fx: Fx,
    node: Arc<Node>,
}

/// A fake pi install (agent directory at the env root; nothing runs it), the adapter directory, a `gh` for E8's one
/// lookup, the helper route answering [`OK`], `watch.error_repeat_seconds = repeat`; Plan `pay` (its Lead stopped, the Plan
/// done now) in Context `ctx`. No PR, no post yet.
fn react(repeat: u32) -> React {
    let fx = Fx::with_ports();
    let adapter = fx.env.root().join("adapter");
    fs::create_dir_all(adapter.join("lib")).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n[watch]\ntick_seconds = 1\npr_poll_seconds = 3600\nerror_repeat_seconds = {repeat}\n[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        fx.env.tmux_socket()
    ));
    fx.env.quiet_models(fx.fakes.clock.utc());
    fx.engine_routes();
    fx.install_pi();
    fx.fake_gh(Reply::ok(
        r#"{"data":{"viewer":{"login":"clayton-duarte"}}}"#,
    ));
    let node = Arc::new(Node {
        says: Mutex::new(OK.to_owned()),
        fails: Mutex::new(Vec::new()),
        seen: Mutex::new(Vec::new()),
        gate: Mutex::new(None),
    });
    let _ = (*fx.fakes.runner).clone().route("node", node.clone());
    let now = fx.stamp_ago(0);
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('pay-lead', 'lead', 'ctx', 'pay', 'Lead pay', '/', 'O3', 'stopped', '{now}', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'done', 'pay-lead', 't', 't', '{now}');"
    ));
    React { fx, node }
}

impl React {
    fn probe(&self) -> Probe {
        Probe {
            home: self.fx.env.cpd_home(),
            node: self.node.clone(),
            clock: (*self.fx.fakes.clock).clone(),
        }
    }

    /// One row of the queue, queued last: the post at `ts` of [`CHANNEL`], `emoji`.
    fn queue(&self, ts: &str, emoji: &str) {
        self.fx.env.sql(&queue_sql(ts, emoji));
    }

    /// PR `key` of Plan `pay`, pull request number `number`: merged `merged_ago` seconds ago, or open.
    fn pr(&self, key: &str, number: u32, merged_ago: Option<i64>) {
        self.fx.env.sql(&pr_sql(&self.fx, key, number, merged_ago));
    }

    /// The post of PR `key`: in [`CHANNEL`] at `ts`, with the emoji `reaction` of the policy (none: no reaction wanted).
    fn post(&self, key: &str, ts: &str, reaction: Option<&str>) {
        self.fx.env.sql(&post_sql(key, ts, reaction));
    }

    /// The engine runs until `step` says so (it is called before each nap, after the tick that came before it) or [`CAP`] naps.
    fn run(&self, mut step: impl FnMut(&Probe, usize) -> bool + Send + 'static) {
        let probe = self.probe();
        self.fx
            .watch_steps(move |nap| nap >= CAP || step(&probe, nap))
            .unwrap();
    }
}

fn pr_sql(fx: &Fx, key: &str, number: u32, merged_ago: Option<i64>) -> String {
    let (state, merged_at) = match merged_ago {
        Some(seconds) => ("merged", format!("'{}'", fx.stamp_ago(seconds * 1000))),
        None => ("open", "NULL".to_owned()),
    };
    let url = format!("https://github.com/acme/pay/pull/{number}");
    format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', {number}, '{url}', 'PR {key}', 0, '{state}', 'none', 'none', 't', {merged_at}, 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at, merged_at)
           VALUES ('pay', '{key}', '/nowhere', 'main', '{state}', 'cpd/pay/{key}', '/nowhere/lead', {number}, '{url}', 't',
                   (SELECT id FROM pull_requests WHERE repo = 'acme/pay' AND number = {number}), cpd_txn(), 't', 't', {merged_at});"
    )
}

fn queue_sql(ts: &str, emoji: &str) -> String {
    format!(
        "INSERT INTO slack_reactions (channel, ts, emoji, queued_at) VALUES ('{CHANNEL}', '{ts}', '{emoji}', 't');"
    )
}

fn post_sql(key: &str, ts: &str, reaction: Option<&str>) -> String {
    let reaction = reaction.map_or("NULL".to_owned(), |r| format!("'{r}'"));
    format!(
        "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, reaction, reacted_at)
           VALUES ('pay', '{key}', '{CHANNEL}', '{ts}', 'https://acme.slack.com/archives/{CHANNEL}/p{}', 't', {reaction}, NULL);",
        ts.replace('.', "")
    )
}

/// `done` once `cond` has held for `ticks` whole ticks: the state the engine reached, and that it stays so.
fn then_ticks(
    ticks: usize,
    mut cond: impl FnMut(&Probe) -> bool + Send + 'static,
) -> impl FnMut(&Probe, usize) -> bool + Send + 'static {
    let mut since = None;
    move |p, nap| {
        if since.is_none() && cond(p) {
            since = Some(nap);
        }
        since.is_some_and(|at| nap >= at + ticks * NAPS_PER_TICK)
    }
}

const TS2: &str = "1793040002.000002";

#[test]
fn a_queued_reaction_is_reacted_to_and_gets_reacted_at() {
    let t = react(LONG);
    t.queue(TS, EMOJI);
    t.run(then_ticks(3, |p| p.reacted_at(TS).is_some()));
    let p = t.probe();

    let calls = p.calls();
    assert_eq!(
        calls.len(),
        1,
        "one run, and later ticks run none: {calls:?}"
    );
    // The product reports canonical paths: /var is a symlink into /private on a Mac.
    let root = t.fx.env.root().canonicalize().unwrap();
    let argv: Vec<&str> = calls[0].args.iter().map(String::as_str).collect();
    assert_eq!(
        argv,
        [
            root.join("adapter/lib/slack-react.ts").to_str().unwrap(),
            "--coding-agent",
            root.join(format!("install/releases/{PI_RELEASE}/node_modules/@earendil-works/pi-coding-agent/dist/index.js")).to_str().unwrap(),
            "--channel",
            CHANNEL,
            "--ts",
            TS,
            "--emoji",
            EMOJI
        ]
    );
    let env = |name: &str| {
        calls[0]
            .envs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_deref())
    };
    assert_eq!(
        env("PI_CODING_AGENT_DIR"),
        Some(Some(root.to_str().unwrap()))
    );
    for removed in ["CPD_PLAN", "CPD_SESSION", "CPD_ATTEMPT"] {
        assert_eq!(
            env(removed),
            Some(None),
            "{removed} is removed from the helper's environment"
        );
    }
    assert_eq!(
        calls[0].cwd,
        Some(t.fx.env.cpd_home().canonicalize().unwrap())
    );
    assert_eq!(calls[0].stdin, None, "no stdin");
    assert_eq!(calls[0].deadline, Duration::from_secs(120));

    let at = p.reacted_at(TS).unwrap();
    assert!(at.ends_with('Z'), "{at}");
    assert_eq!(p.errors(), Vec::<Value>::new());
    assert_eq!(
        p.events_but_errors(),
        Vec::<String>::new(),
        "a success appends no Event"
    );
}

#[test]
fn an_already_reacted_answer_counts_as_done() {
    let t = react(LONG);
    t.probe().says(ALREADY);
    t.queue(TS, EMOJI);
    t.run(then_ticks(3, |p| p.reacted_at(TS).is_some()));
    let p = t.probe();

    assert_eq!(p.calls().len(), 1);
    assert!(p.reacted_at(TS).is_some());
    assert_eq!(p.errors(), Vec::<Value>::new());
}

#[test]
fn a_failed_helper_leaves_the_row_and_reports_one_error_per_code() {
    let t = react(LONG);
    t.probe().says(AUTH);
    t.queue(TS, EMOJI);
    // The first run fails; its error is reported. Once the repeat period has passed the same row is tried again and
    // fails with the same code: no second Event.
    let mut skipped = false;
    let mut tail = then_ticks(5, |p| p.calls().len() >= 2);
    t.run(move |p, nap| {
        if !skipped {
            if !p.errors().is_empty() {
                p.skip(u64::from(LONG) + 1);
                skipped = true;
            }
            return false;
        }
        tail(p, nap)
    });
    let p = t.probe();

    assert_eq!(p.reacted_at(TS), None, "a failure leaves reacted_at NULL");
    let errors = p.errors();
    assert_eq!(errors.len(), 1, "one Event per code: {errors:?}");
    assert_eq!(errors[0]["code"], "slack_auth");
    assert_eq!(errors[0]["source"], "watch");
    assert!(
        errors[0]["message"]
            .as_str()
            .unwrap()
            .contains("pi mcp login slack"),
        "{errors:?}"
    );
    let scoped: i64 = p.store().query_row("SELECT count(*) FROM events WHERE kind = 'error' AND (plan IS NOT NULL OR session IS NOT NULL)", [], |r| r.get(0)).unwrap();
    assert_eq!(
        scoped, 0,
        "it concerns the User's account: no Plan, no Session"
    );
    let items: i64 = p
        .store()
        .query_row("SELECT count(*) FROM inbox_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        items, 0,
        "an engine error opens no Inbox item: it goes to the Manager"
    );
}

#[test]
fn a_failed_row_is_tried_again_only_after_the_repeat_period() {
    const REPEAT: u32 = 600;
    let t = react(REPEAT);
    t.probe().says(AUTH);
    t.queue(TS, EMOJI);
    // The first run fails (its Event is the signal that its end was seen). Then 500 s pass and three ticks: below the
    // period, no call. Then the node is signed in and 200 s pass: the period is over, one call, the same row.
    let (mut failed_at, mut again) = (None, false);
    let mut tail = then_ticks(3, |p| p.reacted_at(TS).is_some());
    let seen = Arc::new(Mutex::new(None));
    let record = seen.clone();
    t.run(move |p, nap| {
        let Some(at) = failed_at else {
            if !p.errors().is_empty() {
                p.skip(u64::from(REPEAT) - 100);
                failed_at = Some(nap);
            }
            return false;
        };
        if !again {
            if nap >= at + 3 * NAPS_PER_TICK {
                *record.lock().unwrap() = Some(p.calls().len());
                p.says(OK);
                p.skip(200);
                again = true;
            }
            return false;
        }
        tail(p, nap)
    });
    let p = t.probe();

    assert_eq!(
        seen.lock().unwrap().take(),
        Some(1),
        "500 s after the failed run's end is not 600 s: no call"
    );
    assert_eq!(
        p.ts_called(),
        [TS, TS],
        "after the period the same row is tried again, once"
    );
    assert!(p.reacted_at(TS).is_some(), "and it succeeds");
    assert_eq!(
        p.errors().len(),
        1,
        "the one failure is the only Event: {:?}",
        p.errors()
    );
}

#[test]
fn a_row_that_keeps_failing_does_not_block_the_rows_behind_it() {
    let t = react(LONG);
    // The helper fails for the first row only; the row behind it is fine.
    t.node.fails.lock().unwrap().push(TS.to_owned());
    t.queue(TS, EMOJI);
    t.queue(TS2, "eyes");
    t.run(then_ticks(5, |p| p.reacted_at(TS2).is_some()));
    let p = t.probe();

    assert_eq!(
        p.ts_called(),
        [TS, TS2],
        "the failed row once, then the row behind it, and no retry within the period"
    );
    assert_eq!(p.reacted_at(TS), None, "the failing row stays queued");
    assert!(
        p.reacted_at(TS2).is_some(),
        "the row behind it is reacted to"
    );
    assert_eq!(
        p.errors().len(),
        1,
        "one Event for the one code: {:?}",
        p.errors()
    );
}

#[test]
fn queued_reactions_are_reacted_to_oldest_first_one_at_a_time() {
    let t = react(LONG);
    // The queue order is the id order, not the order of the ts: the first queued is the later post.
    t.queue(TS2, "eyes");
    t.queue("1793040001.000001", EMOJI);
    // The first run is held in flight while the engine ticks on.
    let (entered_tx, entered) = channel();
    let (open_tx, open) = channel();
    *t.node.gate.lock().unwrap() = Some(Gate {
        entered: Mutex::new(entered_tx),
        open: Mutex::new(open),
    });
    let mut opened = false;
    let during = Arc::new(Mutex::new(None));
    let record = during.clone();
    t.run(move |p, nap| {
        if nap == 1 {
            // The first tick started the helper's thread: wait until its route has been called.
            entered
                .recv_timeout(Duration::from_secs(10))
                .expect("the first tick starts the helper");
        }
        if !opened && nap > 2 * NAPS_PER_TICK {
            *record.lock().unwrap() = Some(p.ts_called());
            open_tx.send(()).unwrap();
            opened = true;
        }
        opened && p.reacted_at(TS2).is_some() && p.reacted_at("1793040001.000001").is_some()
    });
    let p = t.probe();

    assert_eq!(
        during.lock().unwrap().take().unwrap(),
        [TS2],
        "one at a time: the oldest row, and no second run while it is in flight"
    );
    assert_eq!(
        p.ts_called(),
        [TS2, "1793040001.000001"],
        "the second run is for the other, with no wait"
    );
    assert_eq!(p.errors(), Vec::<Value>::new());
}

#[test]
fn incident_mode_holds_the_step() {
    let t = react(LONG);
    t.queue(TS, EMOJI);
    t.fx.env.sql(&format!(
        "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, '{}', 'pay-lead', 'why', cpd_txn());
         INSERT INTO incident_plans (plan, started_txn) VALUES ('pay', cpd_txn());",
        t.fx.stamp_ago(0)
    ));
    // Held for five ticks; then incident mode ends and the step runs.
    let held = Arc::new(Mutex::new(None));
    let record = held.clone();
    let mut over = false;
    let mut tail = then_ticks(3, |p| p.reacted_at(TS).is_some());
    t.run(move |p, nap| {
        if !over {
            if nap >= 5 * NAPS_PER_TICK {
                *record.lock().unwrap() = Some((p.calls().len(), p.reacted_at(TS)));
                p.write("DELETE FROM incident_plans; DELETE FROM incident;");
                over = true;
            }
            return false;
        }
        tail(p, nap)
    });
    let p = t.probe();

    assert_eq!(
        held.lock().unwrap().take(),
        Some((0, None)),
        "while incident mode is on: no helper call, no row written"
    );
    assert_eq!(p.calls().len(), 1, "off again: it runs");
    assert!(p.reacted_at(TS).is_some());
}

#[test]
fn nothing_runs_and_nothing_is_reported_when_nothing_is_queued() {
    let t = react(LONG);
    // Nothing waits: a merged post with a reaction that was never queued (only the queue is read, never `pr_posts`), and
    // a queue row already reacted to.
    t.pr("bare", 1, Some(7200));
    t.post("bare", "1793040001.000001", Some(EMOJI));
    t.queue("1793040002.000002", EMOJI);
    t.fx.env
        .sql("UPDATE slack_reactions SET reacted_at = 'earlier';");
    // Five ticks of nothing; then a row is queued: the step is alive.
    let quiet = Arc::new(Mutex::new(None));
    let record = quiet.clone();
    let mut queued = false;
    let mut tail = then_ticks(3, |p| p.reacted_at(TS).is_some());
    t.run(move |p, nap| {
        if !queued {
            if nap >= 5 * NAPS_PER_TICK {
                *record.lock().unwrap() =
                    Some((p.calls().len(), p.errors(), p.events_but_errors()));
                p.write(&queue_sql(TS, EMOJI));
                queued = true;
            }
            return false;
        }
        tail(p, nap)
    });
    let p = t.probe();

    let (calls, errors, events) = quiet.lock().unwrap().take().expect("five ticks passed");
    assert_eq!(
        (calls, errors, events),
        (0, Vec::<Value>::new(), Vec::<String>::new()),
        "nothing due: no helper, no Event"
    );
    assert_eq!(
        p.ts_called(),
        [TS],
        "the row queued after the quiet ticks is the only one reacted to"
    );
    assert_eq!(
        p.reacted_at("1793040002.000002").as_deref(),
        Some("earlier"),
        "an earlier reaction is not touched"
    );
    assert_eq!(p.errors(), Vec::<Value>::new());
}

// ---- E17 and the policy: the emoji of a merge reaction is the PR's policy now; a permanent refusal is parked (pro-d124, pro-d125) ----

/// The helper's refusal answer for Slack's error `reason`.
fn refused(reason: &str) -> String {
    format!(
        r#"1 {{"ok":false,"error":{{"code":"slack_refused","message":"slack_add_reaction returned an error: execution_failed: {reason}","details":{{"tool":"slack_add_reaction","reason":"{reason}"}}}}}}"#
    )
}

/// The owner's policy: posting on, the channel, and the merge reaction `emoji` when there is one.
fn owner_toml(emoji: Option<&str>) -> String {
    let reaction = emoji.map_or(String::new(), |e| format!("merged_reaction = \"{e}\"\n"));
    format!("[pr.post]\nenabled = true\nchannel = \"{CHANNEL}\"\n{reaction}")
}

/// A repo policy file that sets only the merge reaction.
fn repo_toml(emoji: &str) -> String {
    format!("[pr.post]\nmerged_reaction = \"{emoji}\"\n")
}

impl Probe {
    /// Replaces the policy file `policy/acme/<name>.toml`, as the User's edit: the engine finds it at its next tick.
    fn policy_file(&self, name: &str, toml: &str) {
        fs::write(
            self.home.join("policy/acme").join(format!("{name}.toml")),
            toml,
        )
        .unwrap();
    }

    /// The `--emoji` of each call of the helper, in order.
    fn emojis_called(&self) -> Vec<String> {
        self.calls()
            .iter()
            .map(|c| {
                c.args[c.args.iter().position(|a| a == "--emoji").expect("--emoji") + 1].clone()
            })
            .collect()
    }

    /// `(emoji, reacted_at, refused_at, refusal)` of the queue row of the post at `ts`.
    fn row(&self, ts: &str) -> (String, Option<String>, Option<String>, Option<String>) {
        self.store()
            .query_row(
                "SELECT emoji, reacted_at, refused_at, refusal FROM slack_reactions WHERE ts = ?1",
                [ts],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap()
    }

    fn refused_at(&self, ts: &str) -> Option<String> {
        self.row(ts).2
    }

    /// Every `error` Event but those of `gh`: its Plan and its data.
    fn error_events(&self) -> Vec<(Option<String>, Value)> {
        let conn = self.store();
        let mut stmt = conn
            .prepare("SELECT plan, data FROM events WHERE kind = 'error' ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows.into_iter()
            .map(|(plan, data)| (plan, serde_json::from_str::<Value>(&data).unwrap()))
            .filter(|(_, d)| d["code"].as_str().is_some_and(|c| c != "gh_failed"))
            .collect()
    }
}

impl React {
    /// A merge reaction: the Context owned by `acme`, merged PR `api` (repo name `pay`) with its post at [`TS`] (the stale copy
    /// `git-merge` of the policy at post time), the queue row linked to it with the emoji `git-merge`, the owner's policy file
    /// `owner` and the repo's `repo`.
    fn merge_row(&self, owner: &str, repo: &str) {
        self.fx.env.owner_policy("acme", owner);
        self.fx.env.repo_policy("acme", "pay", repo);
        self.pr("api", 1, Some(7200));
        self.post("api", TS, Some(EMOJI));
        self.fx.env.sql(&format!(
            "UPDATE contexts SET owner = 'acme' WHERE id = 'ctx';
             UPDATE workflows SET name = 'pay' WHERE plan = 'pay' AND key = 'api';
             INSERT INTO slack_reactions (channel, ts, emoji, queued_at, plan, workflow) VALUES ('{CHANNEL}', '{TS}', '{EMOJI}', 't', 'pay', 'api');"
        ));
    }
}

/// The canonical and the plain path of `file` under the home of `p`: the product reports one or the other.
fn mentions(text: &str, p: &Probe, file: &str) -> bool {
    let plain = p.home.join("policy/acme").join(file);
    let canonical = p
        .home
        .canonicalize()
        .unwrap()
        .join("policy/acme")
        .join(file);
    text.contains(plain.to_str().unwrap()) || text.contains(canonical.to_str().unwrap())
}

#[test]
fn a_queued_merge_reaction_is_placed_with_the_emoji_the_policy_names_now() {
    let t = react(LONG);
    t.merge_row(&owner_toml(Some("git-merged")), "");
    t.queue(TS2, "eyes");
    t.run(then_ticks(3, |p| {
        p.reacted_at(TS).is_some() && p.reacted_at(TS2).is_some()
    }));
    let p = t.probe();

    assert_eq!(p.ts_called(), [TS, TS2]);
    assert_eq!(
        p.emojis_called(),
        ["git-merged", "eyes"],
        "the merge reaction as the policy names it now, the unlinked one as queued"
    );
    let (emoji, reacted, refused, refusal) = p.row(TS);
    assert_eq!(
        (emoji.as_str(), refused, refusal),
        ("git-merged", None, None)
    );
    assert!(reacted.is_some());
    assert_eq!(p.row(TS2).0, "eyes");
    assert_eq!(p.errors(), Vec::<Value>::new());
}

/// Runs until the row at [`TS`] is parked and three ticks more.
fn park(t: &React) -> Probe {
    t.probe().says(&refused("invalid_name"));
    t.run(then_ticks(3, |p| p.refused_at(TS).is_some()));
    t.probe()
}

#[test]
fn a_permanent_refusal_parks_the_row_and_reports_one_error_naming_the_emoji_and_the_policy_file() {
    // (owner file, repo file, the file that sets the key)
    let cases = [
        (owner_toml(Some("git-merge")), String::new(), "owner.toml"),
        (owner_toml(None), repo_toml("git-merge"), "pay.toml"),
    ];
    for (owner, repo, file) in cases {
        let t = react(LONG);
        t.merge_row(&owner, &repo);
        let p = park(&t);

        assert_eq!(
            p.emojis_called(),
            ["git-merge"],
            "{file}: one try, then parked"
        );
        let (emoji, reacted, refused_at, refusal) = p.row(TS);
        assert_eq!(
            (emoji.as_str(), reacted, refusal.as_deref()),
            ("git-merge", None, Some("invalid_name")),
            "{file}"
        );
        assert!(refused_at.is_some_and(|at| at.ends_with('Z')), "{file}");
        let events = p.error_events();
        assert_eq!(events.len(), 1, "{file}: one Event: {events:?}");
        let (plan, data) = &events[0];
        assert_eq!(plan.as_deref(), Some("pay"), "scoped to the Plan");
        assert_eq!(
            (data["code"].as_str(), data["source"].as_str()),
            (Some("slack_refused"), Some("watch"))
        );
        let text = data["message"].as_str().unwrap();
        assert!(
            text.contains("git-merge") && text.contains("invalid_name") && mentions(text, &p, file),
            "{file}: {text}"
        );
        let d = &data["details"];
        assert_eq!(
            (
                d["emoji"].as_str(),
                d["reason"].as_str(),
                d["channel"].as_str(),
                d["ts"].as_str()
            ),
            (
                Some("git-merge"),
                Some("invalid_name"),
                Some(CHANNEL),
                Some(TS)
            )
        );
        assert_eq!(
            (d["plan"].as_str(), d["pr"].as_str()),
            (Some("pay"), Some("api"))
        );
        assert!(d["id"].is_i64(), "{d}");
        assert!(
            mentions(d["file"].as_str().unwrap(), &p, file),
            "{file}: {d}"
        );
        let items: i64 = p
            .store()
            .query_row("SELECT count(*) FROM inbox_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(items, 0, "a parked row is not an Inbox item");
    }
}

#[test]
fn a_parked_row_is_not_tried_again_by_a_later_tick_or_a_restarted_engine() {
    const REPEAT: u32 = 60;
    let t = react(REPEAT);
    t.merge_row(&owner_toml(Some("git-merge")), "");
    park(&t);
    // The control: a row queued behind the parked one is placed while the parked one is left alone.
    let p = t.probe();
    p.says(OK);
    p.write(&queue_sql(TS2, "eyes"));
    let mut skipped = false;
    let mut tail = then_ticks(10, |p| p.reacted_at(TS2).is_some());
    t.run(move |p, nap| {
        if !skipped {
            if p.reacted_at(TS2).is_some() {
                p.skip(u64::from(REPEAT) * 5);
                skipped = true;
            }
            return false;
        }
        tail(p, nap)
    });
    assert_eq!(
        p.ts_called(),
        [TS, TS2],
        "the parked row is not called again, past the repeat period either"
    );
    assert_eq!(p.error_events().len(), 1, "no second Event");
    assert!(p.refused_at(TS).is_some());

    // A second engine on the same home, with fresh memory: the same, with a new control row.
    let third = "1793040003.000003";
    p.write(&queue_sql(third, "eyes"));
    t.run(then_ticks(10, move |p| p.reacted_at(third).is_some()));
    assert_eq!(
        p.ts_called(),
        [TS, TS2, third.to_owned().as_str()],
        "the restarted engine places the control and not the parked row"
    );
    assert_eq!(p.error_events().len(), 1, "no second Event");
    assert!(p.refused_at(TS).is_some() && p.reacted_at(TS).is_none());
}

#[test]
fn a_parked_row_is_reopened_and_placed_when_the_policy_names_another_emoji() {
    // A rewrite to the same refused emoji reopens nothing; the control row behind shows the step is alive.
    let t = react(LONG);
    t.merge_row(&owner_toml(Some("git-merge")), "");
    let p = park(&t);
    p.says(OK);
    p.write(&queue_sql(TS2, "eyes"));
    p.policy_file(
        "owner",
        &format!("# edited\n{}", owner_toml(Some("git-merge"))),
    );
    t.run(then_ticks(10, |p| p.reacted_at(TS2).is_some()));
    assert_eq!(
        p.ts_called(),
        [TS, TS2],
        "the same emoji: the row stays parked"
    );
    assert!(p.refused_at(TS).is_some());

    // Another emoji: reopened and placed.
    p.policy_file("owner", &owner_toml(Some("git-merged")));
    t.run(then_ticks(3, |p| p.reacted_at(TS).is_some()));
    assert_eq!(p.emojis_called(), ["git-merge", "eyes", "git-merged"]);
    let (emoji, reacted, refused_at, refusal) = p.row(TS);
    assert_eq!(
        (emoji.as_str(), refused_at, refusal),
        ("git-merged", None, None)
    );
    assert!(reacted.is_some());
    assert_eq!(p.error_events().len(), 1, "the parking is the only Event");
}

#[test]
fn a_merge_reaction_whose_policy_names_no_emoji_is_parked_as_policy_empty() {
    let t = react(LONG);
    t.merge_row(&owner_toml(Some("")), "");
    t.queue(TS2, "eyes");
    t.run(then_ticks(3, |p| {
        p.refused_at(TS).is_some() && p.reacted_at(TS2).is_some()
    }));
    let p = t.probe();

    assert_eq!(
        p.ts_called(),
        [TS2],
        "no helper call for the merge reaction; the control row is placed"
    );
    let (_, reacted, refused_at, refusal) = p.row(TS);
    assert_eq!((reacted, refusal.as_deref()), (None, Some("policy_empty")));
    assert!(refused_at.is_some());
    let events = p.error_events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].0.as_deref(), Some("pay"));
    assert_eq!(events[0].1["code"], "slack_refused");
    let text = events[0].1["message"].as_str().unwrap();
    assert!(
        text.contains("pr.post.merged_reaction") && mentions(text, &p, "owner.toml"),
        "{text}"
    );

    // A later non-empty policy reopens and places it.
    p.policy_file("owner", &owner_toml(Some("git-merged")));
    t.run(then_ticks(3, |p| p.reacted_at(TS).is_some()));
    assert_eq!(p.emojis_called(), ["eyes", "git-merged"]);
    let (emoji, _, refused_at, refusal) = p.row(TS);
    assert_eq!(
        (emoji.as_str(), refused_at, refusal),
        ("git-merged", None, None)
    );
    assert_eq!(p.error_events().len(), 1);
}

#[test]
fn a_merge_reaction_whose_policy_does_not_resolve_is_reported_and_retried_later() {
    const REPEAT: u32 = 600;
    let t = react(REPEAT);
    t.merge_row("this is = = not toml\n", "");
    t.queue(TS2, "eyes");
    t.run(then_ticks(3, |p| {
        p.reacted_at(TS2).is_some() && !p.error_events().is_empty()
    }));
    let p = t.probe();

    assert_eq!(
        p.ts_called(),
        [TS2],
        "no helper call for the merge reaction (the control row is placed)"
    );
    let events = p.error_events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        (events[0].0.as_deref(), events[0].1["code"].as_str()),
        (Some("pay"), Some("policy_invalid"))
    );
    let (_, reacted, refused_at, _) = p.row(TS);
    assert_eq!((reacted, refused_at), (None, None), "pending, not parked");

    // The file is fixed; the row is tried again after the repeat period.
    p.policy_file("owner", &owner_toml(Some("git-merged")));
    p.skip(u64::from(REPEAT) + 1);
    t.run(then_ticks(3, |p| p.reacted_at(TS).is_some()));
    assert_eq!(p.emojis_called(), ["eyes", "git-merged"]);
    assert_eq!(p.error_events().len(), 1, "no more Events");
}

#[test]
fn a_refused_row_without_a_pr_is_parked_with_an_event_that_names_no_policy_file() {
    const REPEAT: u32 = 60;
    let t = react(REPEAT);
    t.probe().says(&refused("message_not_found"));
    t.queue(TS, "eyes");
    t.run(then_ticks(3, |p| p.refused_at(TS).is_some()));
    let p = t.probe();

    let (emoji, reacted, _, refusal) = p.row(TS);
    assert_eq!(
        (emoji.as_str(), reacted, refusal.as_deref()),
        ("eyes", None, Some("message_not_found"))
    );
    let events = p.error_events();
    assert_eq!(events.len(), 1, "{events:?}");
    let (plan, data) = &events[0];
    assert_eq!(*plan, None, "machine-level: no Plan");
    assert_eq!(data["code"], "slack_refused");
    let text = data["message"].as_str().unwrap();
    assert!(
        text.contains("eyes")
            && text.contains("message_not_found")
            && text.contains(CHANNEL)
            && text.contains(TS),
        "{text}"
    );
    assert!(
        !text.contains(".toml") && !text.contains("policy"),
        "{text}"
    );
    let d = &data["details"];
    assert_eq!(
        (
            d["emoji"].as_str(),
            d["reason"].as_str(),
            d["channel"].as_str(),
            d["ts"].as_str()
        ),
        (
            Some("eyes"),
            Some("message_not_found"),
            Some(CHANNEL),
            Some(TS)
        )
    );
    assert!(
        d.get("file").is_none() && d.get("plan").is_none() && d.get("pr").is_none(),
        "{d}"
    );

    // Never retried, past the repeat period either; the control: a row queued after is placed.
    p.says(OK);
    p.write(&queue_sql(TS2, "eyes"));
    p.skip(u64::from(REPEAT) * 5);
    t.run(then_ticks(10, |p| p.reacted_at(TS2).is_some()));
    assert_eq!(p.ts_called(), [TS, TS2]);
    assert_eq!(p.error_events().len(), 1);
}
