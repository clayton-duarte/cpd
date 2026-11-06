//! The team review poll, now the `slack` Source under E24 (contract §3.7; docs/architecture/swr-slack.md §3): with the
//! engine running, E24 runs the Slack helper through the Runner port when the channel query is due, stores its posts in
//! `swr_slack_posts` (no `run/team-review.json`), keeps the last good rows when a run fails and reports an outage once.
//! The helper is the `node` route of the `FakeRunner` (the real one is `pi/lib/slack-channel.ts`, tested in `pi/test/`);
//! the engine runs in-process on the `ManualClock` ([`Fx::watch_steps`]), so a period is virtual seconds.
//! The helper is a synchronous call on the Runner port (A7), so the old "never holds the tick" case has no subject.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fx::Fx;
use common::healthy::PI_RELEASE;
use cpd_core::clock::Clock;
use cpd_core::host::runner::{Call, Handler, Reply};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

const CHANNEL: &str = "C02JU6C2Z1D";
const ME: &str = "U04DB4MTUMR";

/// What the helper prints when it read the channel: one post (`commands.md` §21.6).
const OK: &str = r#"{"ok":true,"channel":"C02JU6C2Z1D","read":74,"posts":[{"repo":"acme/pay","number":812,"ts":1760951522.0113,"ts_raw":"1760951522.011300","author":"Ryan Case","author_id":"U01","author_is_me":false,"primary":true,"reactions":{"eyes":1},"reply_count":null,"latest_reply":null,"claim":{"kind":"claimed","reason":"claimed with :eyes: in Slack"},"eyes_me":false,"thread_tags_me":false}]}"#;
/// What it prints when Slack needs a sign-in.
const AUTH: &str = r#"{"ok":false,"error":{"code":"slack_auth","message":"Slack needs a sign-in. Run: pi mcp login slack","details":{}}}"#;

/// One virtual tick (`watch.tick_seconds`).
const TICK: Duration = Duration::from_secs(1);

/// The `node` route: the Slack helper, answering what `answer` says (`<exit> <json>`) when it is called.
struct Helper {
    answer: Mutex<String>,
}

impl Handler for Helper {
    fn handle(&self, _call: &Call) -> Reply {
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
    team_with(period, NOBODY_REVIEWED)
}

/// What the `gh` of E8's one lookup answers when no pull request is asked for.
const NOBODY_REVIEWED: &str = r#"{"data":{"viewer":{"login":"clayton-duarte"}}}"#;

/// [`team`] with `gh` as the one answer of the fake `gh`.
fn team_with(period: Option<u32>, gh: &str) -> Team {
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

    /// The rows of `swr_slack_posts`: `(ts, number, reactions_json)`.
    fn posts(&self) -> Vec<(String, i64, String)> {
        posts(&self.db())
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

fn posts(db: &Path) -> Vec<(String, i64, String)> {
    let conn = reader(db);
    let Ok(mut stmt) =
        conn.prepare("SELECT ts, number, reactions_json FROM swr_slack_posts ORDER BY ts, number")
    else {
        return Vec::new();
    };
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn reader(db: &Path) -> Connection {
    Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

/// The `error` Events of E24 and E8: `(code, data)`, but those of `gh` (E8's lookup of the one fake answer).
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
fn e11_runs_the_helper_with_its_arguments_and_stores_the_posts_in_swr() {
    let t = team(Some(3600));
    let db = t.db();
    // A period of an hour: the next run is not due, whatever the ticks (three more after the rows).
    t.run_until(then_ticks(3, move || !posts(&db).is_empty()));

    assert_eq!(
        t.posts(),
        vec![(
            "1760951522.011300".to_owned(),
            812,
            r#"{"eyes":1}"#.to_owned()
        )]
    );
    assert!(
        !t.fx.env.cpd_home().join("run/team-review.json").exists(),
        "the file is retired"
    );

    let calls = t.fx.calls("node");
    let script =
        t.fx.env
            .root()
            .canonicalize()
            .unwrap()
            .join("adapter/lib/slack-channel.ts");
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
    let stdin: Value = serde_json::from_slice(calls[0].stdin.as_ref().expect("a stdin")).unwrap();
    assert_eq!(
        stdin,
        serde_json::json!({"version": 1, "known": [], "terminal": []})
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

#[test]
fn e11_keeps_the_last_good_cache_when_a_run_fails_and_reports_the_failure_once() {
    let t = team(Some(1));
    let db = t.db();
    let (helper, runner) = (t.helper.clone(), t.fx.fakes.runner.clone());
    // phase 0: the first good rows; phase 1: the helper fails (Slack needs a sign-in), three runs of it; then it is
    // signed in again and the rows are replaced.
    let mut phase = 0;
    let (mut good, mut from) = (None, 0);
    let failed = Arc::new(Mutex::new(None));
    let record = failed.clone();
    t.run_until(move |_| {
        match phase {
            0 => {
                let first = posts(&db);
                if !first.is_empty() {
                    good = Some(first);
                    *helper.answer.lock().unwrap() = format!("3 {AUTH}");
                    from = node_calls(&runner);
                    phase = 1;
                }
            }
            1 => {
                if node_calls(&runner) >= from + 3 {
                    *record.lock().unwrap() =
                        Some((good.take().unwrap(), posts(&db), errors(&db), items(&db)));
                    *helper.answer.lock().unwrap() =
                        format!("0 {}", OK.replace("\"eyes\":1", "\"eyes\":2"));
                    phase = 2;
                }
            }
            _ => return posts(&db).first().is_some_and(|p| p.2 == r#"{"eyes":2}"#),
        }
        false
    });

    let (good, kept, errors, items) = failed.lock().unwrap().take().expect("three runs failed");
    assert_eq!(kept, good, "the last good rows stay");
    assert_eq!(errors.len(), 1, "once, however many runs fail: {errors:?}");
    assert_eq!(errors[0]["code"], "swr_endpoint_down");
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

    // Signed in again: the rows are replaced.
    assert_eq!(t.posts()[0].2, r#"{"eyes":2}"#);
}

#[test]
fn e11_does_not_replace_the_cache_with_an_answer_that_holds_no_posts() {
    let t = team(Some(1));
    t.answer("0 {\"ok\":true}");
    let db = t.db();
    t.run_until(move |_| !errors(&db).is_empty());

    assert_eq!(t.errors()[0]["code"], "swr_endpoint_down");
    assert!(
        t.errors()[0]["message"]
            .as_str()
            .unwrap()
            .contains("slack_helper_failed")
    );
    assert!(t.posts().is_empty());
}

#[test]
fn e11_runs_nothing_without_a_team_channel() {
    let t = team(None);
    t.run_until(|ticked| ticked >= TICK * 3);
    assert_eq!(t.helper_calls(), 0);
    assert!(t.posts().is_empty());
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
    assert!(
        errors[0]["message"]
            .as_str()
            .unwrap()
            .contains("pi_not_found")
    );
    assert_eq!(t.helper_calls(), 0);
}

#[test]
fn e8_opens_the_review_item_for_a_cached_post_hermes_calls_needs_review() {
    // GitHub (the one answer of the fake) says nobody reviewed the pull request.
    let gh = r#"{"data":{"viewer":{"login":"clayton-duarte"},"p0":{"pullRequest":{"title":"Add retries","isDraft":false,"state":"OPEN","reviewDecision":null,"author":{"login":"ryan"},"latestReviews":{"nodes":[]}}}}}"#;
    let t = team_with(Some(3600), gh);
    // The post is unclaimed and just made, on the clock the engine reads.
    let now = t.fx.fakes.clock.utc().unix_timestamp();
    let post = r#"{"ok":true,"channel":"C02JU6C2Z1D","read":1,"posts":[{"repo":"acme/pay","number":812,"ts":TS,"ts_raw":"TSRAW","author":"Ryan Case","author_id":"U01","author_is_me":false,"primary":true,"reactions":{},"reply_count":null,"latest_reply":null,"claim":null,"eyes_me":null,"thread_tags_me":false}]}"#
        .replace("TSRAW", &format!("{now}.000100"))
        .replace("TS", &now.to_string());
    t.answer(&format!("0 {post}"));
    let db = t.db();
    t.run_until(move |_| {
        items(&db).contains(&(
            "pr:acme/pay#812".to_owned(),
            "review pay#812\nAdd retries".to_owned(),
        ))
    });
}
