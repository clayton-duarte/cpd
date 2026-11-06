//! Engine step E11, the team review poll (contract §3.7, §2.2 "The team review rule"; commands.md §21.5, §21.6): with
//! the engine running, E11 starts the Slack helper detached every `watch.team_review_seconds` after the previous run
//! ended, writes `run/team-review.json` from its answer, keeps the last good copy when a run fails and reports the
//! failure once. The helper is the `node` route of the `FakeRunner` (the real one is `pi/lib/slack-channel.ts`, tested in
//! `pi/test/`); the engine runs in-process on the `ManualClock` ([`Fx::watch_steps`]), so a period is virtual seconds.
//!
//! The helper runs on a thread of its own and answers when its thread is scheduled, not when the engine ticks: a test
//! runs the engine until the state it checks holds (a file, a row), and never counts on a tick for an answer.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fx::Fx;
use common::healthy::PI_RELEASE;
use cpd_core::clock::{Clock, ManualClock};
use cpd_core::host::runner::{Call, Handler, Reply};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

const CHANNEL: &str = "C02JU6C2Z1D";
const ME: &str = "U04DB4MTUMR";

/// What the helper prints when it read the channel: one post (`commands.md` §21.6).
const OK: &str = r#"{"ok":true,"channel":"C02JU6C2Z1D","read":74,"posts":[{"repo":"acme/pay","number":812,"ts":1760951522.0113,"ts_raw":"1760951522.011300","author":"Ryan Case","primary":true,"reactions":["eyes"],"claim":{"kind":"claimed","reason":"claimed with :eyes: in Slack"},"eyes_me":false,"thread_tags_me":false}]}"#;
/// What it prints when Slack needs a sign-in.
const AUTH: &str = r#"{"ok":false,"error":{"code":"slack_auth","message":"Slack needs a sign-in. Run: pi mcp login slack","details":{}}}"#;

/// How long a gated helper waits for its gate before it gives up: only a regression (E11 holding the tick, so that the
/// engine never reaches the step that opens the gate) waits that long, and then the test fails on its assertion.
const GATE_DEADLINE: Duration = Duration::from_secs(30);

/// One virtual tick (`watch.tick_seconds`).
const TICK: Duration = Duration::from_secs(1);

/// The `node` route: the Slack helper, answering what `answer` says (`<exit> <json>`) when it is called. With a gate it
/// first says it started and then waits for the gate to open: a helper that is still running.
struct Helper {
    answer: Mutex<String>,
    gate: Option<Gate>,
}

struct Gate {
    entered: Mutex<Sender<()>>,
    open: Mutex<Receiver<()>>,
}

impl Handler for Helper {
    fn handle(&self, _call: &Call) -> Reply {
        if let Some(gate) = &self.gate {
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
        let answer = self.answer.lock().unwrap().clone();
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

struct Team {
    fx: Fx,
    helper: Arc<Helper>,
}

/// A fake pi install (agent directory at the env root; nothing runs it), a `gh` for E8's one lookup, and the helper route
/// answering [`OK`]. E5 polls at every tick (`pr_poll_seconds = 1`, the `cpd` route
/// of `engine_routes`). With a `period`, the team channel is on and `watch.team_review_seconds` is that; without, it is
/// off.
fn team(period: Option<u32>) -> Team {
    team_with(period, None, NOBODY_REVIEWED)
}

/// What the `gh` of E8's one lookup answers when no pull request is asked for.
const NOBODY_REVIEWED: &str = r#"{"data":{"viewer":{"login":"clayton-duarte"}}}"#;

/// [`team`] with the helper behind `gate` and `gh` as the one answer of the fake `gh`.
fn team_with(period: Option<u32>, gate: Option<Gate>, gh: &str) -> Team {
    // `team_review_seconds` is a key of `[watch]`, the channel is the `[inbox]` table.
    let (period_key, inbox) = match period {
        Some(seconds) => (
            format!("team_review_seconds = {seconds}\n"),
            format!("[inbox]\nteam_channel = \"{CHANNEL}\"\nslack_user_id = \"{ME}\"\n"),
        ),
        None => (String::new(), String::new()),
    };
    let fx = Fx::with_ports();
    let adapter = fx.env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n[watch]\ntick_seconds = 1\npr_poll_seconds = 1\n{period_key}{inbox}[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        fx.env.tmux_socket()
    ));
    fx.env.quiet_models(fx.fakes.clock.utc());
    fx.engine_routes();
    fx.install_pi();
    fx.fake_gh(Reply::ok(gh));
    let helper = Arc::new(Helper {
        answer: Mutex::new(format!("0 {OK}")),
        gate,
    });
    let _ = (*fx.fakes.runner).clone().route("node", helper.clone());
    Team { fx, helper }
}

impl Team {
    fn answer(&self, status_and_json: &str) {
        *self.helper.answer.lock().unwrap() = status_and_json.to_owned();
    }

    fn db(&self) -> PathBuf {
        self.fx.env.cpd_home().join("cpd.db")
    }

    fn cache_path(&self) -> PathBuf {
        self.fx.env.cpd_home().join("run/team-review.json")
    }

    fn cache(&self) -> Option<Value> {
        cache(&self.cache_path())
    }

    fn helper_calls(&self) -> usize {
        self.fx.calls("node").len()
    }

    fn errors(&self) -> Vec<Value> {
        errors(&self.db())
    }

    /// The engine runs until `done` says so (it is called before each nap, after the tick that came before it); `done`
    /// is given the virtual time since the engine started.
    fn run_until(&self, mut done: impl FnMut(Duration) -> bool + Send + 'static) {
        let clock = self.fx.fakes.clock.clone();
        let start = clock.elapsed();
        self.fx
            .watch_steps(move |_| done(clock.elapsed() - start))
            .unwrap();
    }
}

/// `done` once `cond` has held for `ticks` whole ticks.
fn then_ticks(
    ticks: u32,
    mut cond: impl FnMut() -> bool + Send + 'static,
) -> impl FnMut(Duration) -> bool + Send + 'static {
    let mut since = None;
    move |now| {
        if since.is_none() && cond() {
            since = Some(now);
        }
        since.is_some_and(|at| now - at >= TICK * ticks)
    }
}

fn cache(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn reader(db: &Path) -> Connection {
    Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

/// The `error` Events of E11 and E8: `(code, data)`, but those of `gh` (E8's lookup of the one fake answer).
fn errors(db: &Path) -> Vec<Value> {
    let conn = reader(db);
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

/// The open Inbox items: `(subject, label)`.
fn items(db: &Path) -> Vec<(String, String)> {
    let conn = reader(db);
    let mut stmt = conn
        .prepare("SELECT subject, label FROM inbox_items WHERE closed_at IS NULL ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn node_calls(runner: &cpd_core::host::runner::FakeRunner) -> usize {
    runner
        .calls()
        .iter()
        .filter(|call| call.program.rsplit('/').next() == Some("node"))
        .count()
}

#[test]
fn e11_runs_the_helper_with_its_arguments_and_writes_the_cache_by_rename() {
    let t = team(Some(3600));
    let cache_file = t.cache_path();
    // A period of an hour: the next run is not due, whatever the ticks (three more after the cache).
    t.run_until(then_ticks(3, move || cache(&cache_file).is_some()));

    let cache = t.cache().unwrap();
    assert_eq!(cache["version"], 1);
    assert_eq!(cache["channel"], CHANNEL);
    assert_eq!(cache["read"], 74);
    assert_eq!(
        cache["posts"],
        serde_json::from_str::<Value>(OK).unwrap()["posts"]
    );
    assert!(cache["polled_at"].as_str().unwrap().ends_with('Z'));
    assert!(
        !t.fx
            .env
            .cpd_home()
            .join("run/team-review.json.tmp")
            .exists(),
        "written by rename"
    );

    let calls = t.fx.calls("node");
    let script = t.fx.env.root().join("adapter/lib/slack-channel.ts");
    // The product reports canonical paths: /var is a symlink into /private on a Mac.
    let root = t.fx.env.root().canonicalize().unwrap();
    let argv: Vec<&str> = calls[0].args.iter().map(String::as_str).collect();
    assert_eq!(
        argv,
        [
            script.to_str().unwrap(),
            "--coding-agent",
            root.join(format!("install/releases/{PI_RELEASE}/node_modules/@earendil-works/pi-coding-agent/dist/index.js")).to_str().unwrap(),
            "--channel",
            CHANNEL,
            "--me",
            ME
        ]
    );
    let agent_dir = calls[0]
        .envs
        .iter()
        .find(|(key, _)| key == "PI_CODING_AGENT_DIR")
        .and_then(|(_, value)| value.as_deref());
    assert_eq!(agent_dir, Some(root.to_str().unwrap()));

    assert_eq!(t.helper_calls(), 1);
    assert_eq!(t.errors(), Vec::<Value>::new());
}

/// E11 never holds the tick: the helper's thread has not answered, and the engine goes on ticking. The gate is opened
/// by the engine's own thread (the step), two whole ticks after the helper started: an E11 that held the tick would
/// never get there (the helper waits [`GATE_DEADLINE`] for its gate and the cache check below fails).
#[test]
fn e11_never_holds_the_tick_while_the_helper_runs() {
    let (entered_tx, entered) = channel();
    let (open_tx, open) = channel();
    let t = team_with(
        Some(3600),
        Some(Gate {
            entered: Mutex::new(entered_tx),
            open: Mutex::new(open),
        }),
        NOBODY_REVIEWED,
    );
    let cache_file = t.cache_path();
    let runner = t.fx.fakes.runner.clone();
    let clock: Arc<ManualClock> = t.fx.fakes.clock.clone();
    let start = clock.elapsed();
    // What the engine saw while the helper ran: the cache, the helper's calls and the virtual time, at the step that opened the gate.
    let seen = Arc::new(Mutex::new(None));
    let record = seen.clone();
    let mut opened = false;
    t.fx.watch_steps(move |nap| {
        if nap == 1 {
            // The first tick started the helper's thread: wait until its route has been called.
            entered.recv().unwrap();
        }
        let ticked = clock.elapsed() - start;
        if !opened && ticked >= TICK * 2 {
            *record.lock().unwrap() = Some((cache(&cache_file), node_calls(&runner), ticked));
            open_tx.send(()).unwrap();
            opened = true;
        }
        opened && cache(&cache_file).is_some()
    })
    .unwrap();

    let (while_running, calls, ticked) = seen
        .lock()
        .unwrap()
        .take()
        .expect("the engine ticked on while the helper ran");
    assert!(
        ticked >= TICK * 2,
        "the engine ticked at least twice while the helper ran: {ticked:?}"
    );
    assert_eq!(calls, 1);
    assert!(while_running.is_none(), "the helper has not answered yet");
    assert!(t.cache().is_some(), "and then it did");
}

#[test]
fn e11_keeps_the_last_good_cache_when_a_run_fails_and_reports_the_failure_once() {
    let t = team(Some(1));
    let (cache_file, db) = (t.cache_path(), t.db());
    let (helper, runner) = (t.helper.clone(), t.fx.fakes.runner.clone());
    // phase 0: the first good cache; phase 1: the helper fails (Slack needs a sign-in), three runs of it; then it is
    // signed in again and the cache is replaced.
    let mut phase = 0;
    let (mut good, mut from) = (None, 0);
    let failed = Arc::new(Mutex::new(None));
    let record = failed.clone();
    t.run_until(move |_| {
        match phase {
            0 => {
                if let Some(first) = cache(&cache_file) {
                    good = Some(first);
                    *helper.answer.lock().unwrap() = format!("3 {AUTH}");
                    from = node_calls(&runner);
                    phase = 1;
                }
            }
            1 => {
                if node_calls(&runner) >= from + 3 {
                    *record.lock().unwrap() = Some((
                        good.take().unwrap(),
                        cache(&cache_file),
                        errors(&db),
                        items(&db),
                    ));
                    *helper.answer.lock().unwrap() =
                        format!("0 {}", OK.replace("\"read\":74", "\"read\":75"));
                    phase = 2;
                }
            }
            _ => return cache(&cache_file).is_some_and(|c| c["read"] == 75),
        }
        false
    });

    let (good, kept, errors, items) = failed.lock().unwrap().take().expect("three runs failed");
    assert_eq!(kept.unwrap(), good, "the last good copy stays");
    assert_eq!(errors.len(), 1, "once, however many runs fail: {errors:?}");
    assert_eq!(errors[0]["code"], "slack_auth");
    assert_eq!(errors[0]["source"], "watch");
    assert!(
        errors[0]["message"]
            .as_str()
            .unwrap()
            .contains("pi mcp login slack")
    );
    assert!(
        items.is_empty(),
        "an engine error opens no Inbox item: it goes to the Manager (hs-d314)"
    );

    // Signed in again: the cache is replaced.
    assert_eq!(t.cache().unwrap()["read"], 75);
}

#[test]
fn e11_does_not_replace_the_cache_with_an_answer_that_holds_no_posts() {
    let t = team(Some(1));
    t.answer("0 {\"ok\":true}");
    let db = t.db();
    t.run_until(move |_| !errors(&db).is_empty());

    assert_eq!(t.errors()[0]["code"], "slack_helper_failed");
    assert!(t.cache().is_none());
    assert!(
        !t.fx
            .env
            .cpd_home()
            .join("run/team-review.json.tmp")
            .exists()
    );
}

#[test]
fn e11_runs_nothing_without_a_team_channel() {
    let t = team(None);
    t.run_until(|ticked| ticked >= TICK * 3);
    assert_eq!(t.helper_calls(), 0);
    assert!(t.cache().is_none());
    assert_eq!(t.errors(), Vec::<Value>::new());
}

#[test]
fn e11_reports_pi_not_found_once_and_keeps_ticking() {
    let t = team(Some(1));
    fs::remove_file(t.fx.env.root().join("install/current-version")).unwrap();
    let db = t.db();
    t.run_until(then_ticks(3, move || !errors(&db).is_empty()));

    let errors = t.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["code"], "pi_not_found");
    assert_eq!(t.helper_calls(), 0);
}

