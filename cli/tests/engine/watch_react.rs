//! Engine step E16, the merge reaction (contract §3.7 E16; commands.md §21.8, §21.9; Plan pr-post, decision pro-d64 step 7):
//! after a PR that was posted in Slack has merged, the engine runs the helper `pi/lib/slack-react.ts` once, detached, to
//! add the owner's emoji to the post, and then sets the row's `reacted_at`. A failure leaves `reacted_at` NULL, is
//! reported once per code and tried again `watch.error_repeat_seconds` after the failed run's end.
//!
//! In-process, on virtual time, like `watch_team.rs`: the engine runs on the `Fx`'s `ManualClock`, `FakeRunner` and
//! `FakeSpawn`, the helper is the `node` route of the `FakeRunner` (the real one is `pi/test/slack-react.test.ts`). No
//! process, no Slack, no sleep: an engine run ends when a state the test reads holds (or at [`CAP`] naps, so that a step
//! that never runs fails the test on its assertion instead of running for ever).
//!
//! The helper runs on a thread of its own and answers when its thread is scheduled, not when the engine ticks: a test
//! never counts on a tick for an answer, it runs the engine until the row or the Event it checks is there. Every test that
//! says "nothing runs" has a control: a post that must be reacted to, so that the silence is shown after the step proved
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
    seen: Mutex<Vec<Call>>,
    gate: Mutex<Option<Gate>>,
}

impl Handler for Node {
    fn handle(&self, call: &Call) -> Reply {
        self.seen.lock().unwrap().push(call.clone());
        if let Some(gate) = self.gate.lock().unwrap().take() {
            gate.entered.lock().unwrap().send(()).expect("the test holds the receiver of `entered`");
            if gate.open.lock().unwrap().recv_timeout(GATE_DEADLINE).is_err() {
                return Reply::exit(70, "", "the gate of the helper never opened");
            }
        }
        let answer = self.says.lock().unwrap().clone();
        let (status, json) = answer.split_once(' ').expect("an answer is `<exit> <json>`");
        Reply::exit(status.parse().expect("the exit status is a number"), &format!("{json}\n"), "")
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
        self.calls().iter().map(|c| c.args[c.args.iter().position(|a| a == "--ts").expect("--ts") + 1].clone()).collect()
    }

    fn says(&self, answer: &str) {
        *self.node.says.lock().unwrap() = answer.to_owned();
    }

    fn store(&self) -> Connection {
        Connection::open_with_flags(self.home.join("cpd.db"), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

    /// The `reacted_at` of PR `pr`'s post.
    fn reacted_at(&self, pr: &str) -> Option<String> {
        self.store().query_row("SELECT reacted_at FROM pr_posts WHERE plan = 'pay' AND pr = ?1", [pr], |r| r.get(0)).unwrap()
    }

    /// The data of every `error` Event but those of `gh` (E8's one lookup is not E16's).
    fn errors(&self) -> Vec<Value> {
        let conn = self.store();
        let mut stmt = conn.prepare("SELECT data FROM events WHERE kind = 'error' ORDER BY id").unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        rows.iter().map(|d| serde_json::from_str::<Value>(d).unwrap()).filter(|d| d["code"].as_str().is_some_and(|c| c != "gh_failed")).collect()
    }

    /// The kinds of the Events the engine appended but its own `watch-started` and `watch-stopped`, `error` Events apart.
    fn events_but_errors(&self) -> Vec<String> {
        let conn = self.store();
        let mut stmt = conn.prepare("SELECT kind FROM events WHERE kind NOT IN ('watch-started', 'watch-stopped', 'error') ORDER BY id").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap()
    }

    /// A write of the store, as another process's: a fixture the engine finds at its next tick.
    fn write(&self, sql: &str) {
        common::sql(&mut Store::open(&CpdHome::resolve(Some(self.home.clone().into()), None).unwrap(), 5000).unwrap(), sql);
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
    fx.fake_gh(Reply::ok(r#"{"data":{"viewer":{"login":"clayton-duarte"}}}"#));
    let node = Arc::new(Node { says: Mutex::new(OK.to_owned()), seen: Mutex::new(Vec::new()), gate: Mutex::new(None) });
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
        Probe { home: self.fx.env.cpd_home(), node: self.node.clone(), clock: (*self.fx.fakes.clock).clone() }
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
        self.fx.watch_steps(move |nap| nap >= CAP || step(&probe, nap)).unwrap();
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
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at, merged_at)
           VALUES ('pay', '{key}', '/nowhere', 'main', '{state}', 'cpd/pay/{key}', '/nowhere/lead', {number}, '{url}', 't',
                   (SELECT id FROM pull_requests WHERE repo = 'acme/pay' AND number = {number}), cpd_txn(), 't', 't', {merged_at});"
    )
}

fn post_sql(key: &str, ts: &str, reaction: Option<&str>) -> String {
    let reaction = reaction.map_or("NULL".to_owned(), |r| format!("'{r}'"));
    format!(
        "INSERT INTO pr_posts (plan, pr, channel, ts, link, posted_at, reaction, reacted_at)
           VALUES ('pay', '{key}', '{CHANNEL}', '{ts}', 'https://acme.slack.com/archives/{CHANNEL}/p{}', 't', {reaction}, NULL);",
        ts.replace('.', "")
    )
}

/// `done` once `cond` has held for `ticks` whole ticks: the state the engine reached, and that it stays so.
fn then_ticks(ticks: usize, mut cond: impl FnMut(&Probe) -> bool + Send + 'static) -> impl FnMut(&Probe, usize) -> bool + Send + 'static {
    let mut since = None;
    move |p, nap| {
        if since.is_none() && cond(p) {
            since = Some(nap);
        }
        since.is_some_and(|at| nap >= at + ticks * NAPS_PER_TICK)
    }
}

#[test]
fn a_merged_posted_pr_gets_its_reaction_and_reacted_at() {
    let t = react(LONG);
    t.pr("repo", 7, Some(3600));
    t.post("repo", TS, Some(EMOJI));
    t.run(then_ticks(3, |p| p.reacted_at("repo").is_some()));
    let p = t.probe();

    let calls = p.calls();
    assert_eq!(calls.len(), 1, "one run, and later ticks run none: {calls:?}");
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
    let env = |name: &str| calls[0].envs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_deref());
    assert_eq!(env("PI_CODING_AGENT_DIR"), Some(Some(root.to_str().unwrap())));
    for removed in ["CPD_PLAN", "CPD_SESSION", "CPD_ATTEMPT"] {
        assert_eq!(env(removed), Some(None), "{removed} is removed from the helper's environment");
    }
    assert_eq!(calls[0].cwd, Some(t.fx.env.cpd_home().canonicalize().unwrap()));
    assert_eq!(calls[0].stdin, None, "no stdin");
    assert_eq!(calls[0].deadline, Duration::from_secs(120));

    let at = p.reacted_at("repo").unwrap();
    assert!(at.ends_with('Z'), "{at}");
    assert_eq!(p.errors(), Vec::<Value>::new());
    assert_eq!(p.events_but_errors(), Vec::<String>::new(), "a success appends no Event");
}

#[test]
fn an_open_pr_or_a_post_without_a_reaction_is_left_alone() {
    let t = react(LONG);
    // Left alone: an open PR with a reaction, a merged PR whose post wants none, a merged PR already reacted to.
    t.pr("open", 1, None);
    t.post("open", "1793040001.000001", Some(EMOJI));
    t.pr("plain", 2, Some(7200));
    t.post("plain", "1793040002.000002", None);
    t.pr("done", 3, Some(7200));
    t.post("done", "1793040003.000003", Some(EMOJI));
    t.fx.env.sql("UPDATE pr_posts SET reacted_at = 'earlier' WHERE pr = 'done';");
    // The control: a merged PR whose post wants one, merged the latest of the four.
    t.pr("go", 4, Some(60));
    t.post("go", TS, Some(EMOJI));
    t.run(then_ticks(3, |p| p.reacted_at("go").is_some()));
    let p = t.probe();

    assert_eq!(p.ts_called(), [TS], "only the control's post is reacted to");
    assert_eq!(p.reacted_at("open"), None);
    assert_eq!(p.reacted_at("plain"), None);
    assert_eq!(p.reacted_at("done").as_deref(), Some("earlier"), "an earlier reaction is not touched");
}

#[test]
fn an_already_reacted_answer_counts_as_done() {
    let t = react(LONG);
    t.probe().says(ALREADY);
    t.pr("repo", 7, Some(3600));
    t.post("repo", TS, Some(EMOJI));
    t.run(then_ticks(3, |p| p.reacted_at("repo").is_some()));
    let p = t.probe();

    assert_eq!(p.calls().len(), 1);
    assert!(p.reacted_at("repo").is_some());
    assert_eq!(p.errors(), Vec::<Value>::new());
}

#[test]
fn a_failed_helper_leaves_the_row_and_reports_one_error_per_code() {
    let t = react(LONG);
    t.probe().says(AUTH);
    t.pr("repo", 7, Some(3600));
    t.post("repo", TS, Some(EMOJI));
    // The first run fails; its error is reported. Once the repeat period has passed the same post is tried again and
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

    assert_eq!(p.reacted_at("repo"), None, "a failure leaves reacted_at NULL");
    let errors = p.errors();
    assert_eq!(errors.len(), 1, "one Event per code: {errors:?}");
    assert_eq!(errors[0]["code"], "slack_auth");
    assert_eq!(errors[0]["source"], "watch");
    assert!(errors[0]["message"].as_str().unwrap().contains("pi mcp login slack"), "{errors:?}");
    let scoped: i64 = p.store().query_row("SELECT count(*) FROM events WHERE kind = 'error' AND (plan IS NOT NULL OR session IS NOT NULL)", [], |r| r.get(0)).unwrap();
    assert_eq!(scoped, 0, "it concerns the User's account: no Plan, no Session");
    let items: i64 = p.store().query_row("SELECT count(*) FROM inbox_items", [], |r| r.get(0)).unwrap();
    assert_eq!(items, 0, "an engine error opens no Inbox item: it goes to the Manager");
}

#[test]
fn a_failed_post_is_tried_again_only_after_the_repeat_period_and_then_succeeds() {
    const REPEAT: u32 = 600;
    let t = react(REPEAT);
    t.probe().says(AUTH);
    t.pr("repo", 7, Some(3600));
    t.post("repo", TS, Some(EMOJI));
    // The first run fails (its Event is the signal that its end was seen). Then 500 s pass and three ticks: below the
    // period, no call. Then the node is signed in and 200 s pass: the period is over, one call, the same post.
    let (mut failed_at, mut again) = (None, false);
    let mut tail = then_ticks(3, |p| p.reacted_at("repo").is_some());
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

    assert_eq!(seen.lock().unwrap().take(), Some(1), "500 s after the failed run's end is not 600 s: no call");
    assert_eq!(p.ts_called(), [TS, TS], "after the period the same post is tried again, once");
    assert!(p.reacted_at("repo").is_some(), "and it succeeds");
    assert_eq!(p.errors().len(), 1, "the one failure is the only Event: {:?}", p.errors());
}

#[test]
fn posts_are_reacted_to_one_at_a_time_oldest_merge_first() {
    let t = react(LONG);
    // The key order is the reverse of the merge order, and the one merged later is inserted first.
    t.pr("alpha", 1, Some(3600));
    t.post("alpha", "1793040001.000001", Some(EMOJI));
    t.pr("zeta", 2, Some(10_800));
    t.post("zeta", "1793040002.000002", Some(EMOJI));
    // The first run is held in flight while the engine ticks on.
    let (entered_tx, entered) = channel();
    let (open_tx, open) = channel();
    *t.node.gate.lock().unwrap() = Some(Gate { entered: Mutex::new(entered_tx), open: Mutex::new(open) });
    let mut opened = false;
    let during = Arc::new(Mutex::new(None));
    let record = during.clone();
    t.run(move |p, nap| {
        if nap == 1 {
            // The first tick started the helper's thread: wait until its route has been called.
            entered.recv_timeout(Duration::from_secs(10)).expect("the first tick starts the helper");
        }
        if !opened && nap >= 1 + 2 * NAPS_PER_TICK {
            *record.lock().unwrap() = Some(p.ts_called());
            open_tx.send(()).unwrap();
            opened = true;
        }
        opened && p.reacted_at("alpha").is_some() && p.reacted_at("zeta").is_some()
    });
    let p = t.probe();

    assert_eq!(during.lock().unwrap().take().unwrap(), ["1793040002.000002"], "one at a time: the oldest merge, and no second run while it is in flight");
    assert_eq!(p.ts_called(), ["1793040002.000002", "1793040001.000001"], "the second run is for the other, with no wait");
    assert_eq!(p.errors(), Vec::<Value>::new());
}

#[test]
fn incident_mode_holds_the_step() {
    let t = react(LONG);
    t.pr("repo", 7, Some(3600));
    t.post("repo", TS, Some(EMOJI));
    t.fx.env.sql(&format!(
        "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, '{}', 'pay-lead', 'why', cpd_txn());
         INSERT INTO incident_plans (plan, started_txn) VALUES ('pay', cpd_txn());",
        t.fx.stamp_ago(0)
    ));
    // Held for five ticks; then incident mode ends and the step runs.
    let held = Arc::new(Mutex::new(None));
    let record = held.clone();
    let mut over = false;
    let mut tail = then_ticks(3, |p| p.reacted_at("repo").is_some());
    t.run(move |p, nap| {
        if !over {
            if nap >= 5 * NAPS_PER_TICK {
                *record.lock().unwrap() = Some((p.calls().len(), p.reacted_at("repo")));
                p.write("DELETE FROM incident_plans; DELETE FROM incident;");
                over = true;
            }
            return false;
        }
        tail(p, nap)
    });
    let p = t.probe();

    assert_eq!(held.lock().unwrap().take(), Some((0, None)), "while incident mode is on: no helper call, no row written");
    assert_eq!(p.calls().len(), 1, "off again: it runs");
    assert!(p.reacted_at("repo").is_some());
}

#[test]
fn nothing_runs_and_nothing_is_reported_when_no_post_waits() {
    let t = react(LONG);
    // No post waits: a merged PR with no post, a merged post already reacted to, an open PR with a post that wants one.
    t.pr("bare", 1, Some(7200));
    t.pr("done", 2, Some(7200));
    t.post("done", "1793040002.000002", Some(EMOJI));
    t.fx.env.sql("UPDATE pr_posts SET reacted_at = 'earlier' WHERE pr = 'done';");
    t.pr("open", 3, None);
    t.post("open", "1793040003.000003", Some(EMOJI));
    // Five ticks of nothing; then a PR whose post waits merges (the poll saw it, or `pr merge` did): the step is alive.
    let quiet = Arc::new(Mutex::new(None));
    let record = quiet.clone();
    let mut merged = false;
    let mut tail = then_ticks(3, |p| p.reacted_at("open").is_some());
    t.run(move |p, nap| {
        if !merged {
            if nap >= 5 * NAPS_PER_TICK {
                *record.lock().unwrap() = Some((p.calls().len(), p.errors(), p.events_but_errors()));
                p.write(
                    "UPDATE prs SET status = 'merged', merged_at = 't' WHERE key = 'open';
                     UPDATE pull_requests SET state = 'merged', merged_at = 't' WHERE number = 3;",
                );
                merged = true;
            }
            return false;
        }
        tail(p, nap)
    });
    let p = t.probe();

    let (calls, errors, events) = quiet.lock().unwrap().take().expect("five ticks passed");
    assert_eq!((calls, errors, events), (0, Vec::<Value>::new(), Vec::<String>::new()), "no candidate: no helper, no Event");
    assert_eq!(p.ts_called(), ["1793040003.000003"], "a post that waits after the merge is the only one reacted to");
    assert_eq!(p.reacted_at("done").as_deref(), Some("earlier"));
    assert_eq!(p.errors(), Vec::<Value>::new());
}
