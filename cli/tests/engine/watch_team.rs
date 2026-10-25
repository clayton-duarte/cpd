//! Engine step E11, the team review poll (contract §3.7, §2.2 "The team review rule"; commands.md §21.5, §21.6): with
//! the engine running, E11 starts the Slack helper detached every `watch.team_review_seconds` after the previous run
//! ended, writes `run/team-review.json` from its answer, keeps the last good copy when a run fails and reports the
//! failure once. The helper is a fake `node` on `PATH` (the real one is `pi/lib/slack-channel.ts`, tested in `pi/test/`).

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::time::Duration;

use common::{CPD, TestEnv, wait_until};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::Value;

/// A running `cpd watch`, SIGTERMed on drop (killed if it does not stop).
struct Engine(Child);

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(self.0.id() as i32), nix::sys::signal::Signal::SIGTERM);
        // it stops at its next tick; killed if it has not within 10 s
        let _ = common::poll(Duration::from_secs(10), || (!matches!(self.0.try_wait(), Ok(None))).then_some(()));
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const CHANNEL: &str = "C02JU6C2Z1D";
const ME: &str = "U04DB4MTUMR";

/// What the helper prints when it read the channel: one post (`commands.md` §21.6).
const OK: &str = r#"{"ok":true,"channel":"C02JU6C2Z1D","read":74,"posts":[{"repo":"acme/pay","number":812,"ts":1760951522.0113,"ts_raw":"1760951522.011300","author":"Ryan Case","primary":true,"reactions":["eyes"],"claim":{"kind":"claimed","reason":"claimed with :eyes: in Slack"},"eyes_me":false,"thread_tags_me":false}]}"#;
/// What it prints when Slack needs a sign-in.
const AUTH: &str = r#"{"ok":false,"error":{"code":"slack_auth","message":"Slack needs a sign-in. Run: pi mcp login slack","details":{}}}"#;

struct Fx {
    env: TestEnv,
    /// Where the fake helper takes its answer from: `<exit> <json>` in the first line.
    answer: PathBuf,
}

/// A fake pi install (agent directory at the env root), a fake `gh` (E5 polls at every tick) and the fake helper
/// `node`, which answers with the contents of `answer` after `sleep` seconds. With a `period`, the team channel is
/// on and `watch.team_review_seconds` is that; without, it is off.
fn fx(period: Option<u32>, sleep: u32) -> Fx {
    let config = match period {
        Some(seconds) => format!("team_review_seconds = {seconds}\n[inbox]\nteam_channel = \"{CHANNEL}\"\nslack_user_id = \"{ME}\"\n"),
        None => String::new(),
    };
    let env = TestEnv::new();
    let adapter = env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n[watch]\ntick_seconds = 1\npr_poll_seconds = 1\n{config}[tmux]\nsocket = \"{}\"\n",
            adapter.display(),
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    env.fake_pi("exit 0");
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env.root().join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    let answer = env.root().join("helper-answer");
    fs::write(&answer, format!("0 {OK}")).unwrap();
    env.fake(
        "node",
        &format!(
            "echo \"$PI_CODING_AGENT_DIR\" > '{root}/helper-agent-dir'\nsleep {sleep}\nread status json < '{answer}'\necho \"$json\"\nexit \"$status\"",
            root = env.root().display(),
            answer = answer.display()
        ),
    );
    Fx { env, answer }
}

impl Fx {
    fn start(&self) -> Engine {
        let child = self.env.command(CPD).arg("watch").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let engine = Engine(child);
        wait_until("watch-started", || self.query("SELECT count(*) FROM events WHERE kind = 'watch-started'") == 1);
        engine
    }

    fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap(), 5000).unwrap()
    }

    fn query(&self, sql: &str) -> i64 {
        self.store().read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }

    fn cache_path(&self) -> PathBuf {
        self.env.cpd_home().join("run/team-review.json")
    }

    fn cache(&self) -> Option<Value> {
        serde_json::from_str(&fs::read_to_string(self.cache_path()).ok()?).ok()
    }

    fn helper_calls(&self) -> usize {
        self.env.calls("node").len()
    }

    /// The `error` Events of E11: `(code, data)`.
    fn errors(&self) -> Vec<Value> {
        self.store()
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT data FROM events WHERE kind = 'error' ORDER BY id")?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows.iter().map(|d| serde_json::from_str::<Value>(d).unwrap()).filter(|d| d["code"].as_str().is_some_and(|c| c != "gh_failed")).collect())
            })
            .unwrap()
    }

    /// The open Inbox items: `(subject, label)`.
    fn items(&self) -> Vec<(String, String)> {
        self.store()
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT subject, label FROM inbox_items WHERE closed_at IS NULL ORDER BY id")?;
                Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }
}

#[test]
fn e11_runs_the_helper_with_its_arguments_and_writes_the_cache_by_rename() {
    let fx = fx(Some(3600), 0);
    let _engine = fx.start();

    wait_until("the cache", || fx.cache().is_some());
    let cache = fx.cache().unwrap();
    assert_eq!(cache["version"], 1);
    assert_eq!(cache["channel"], CHANNEL);
    assert_eq!(cache["read"], 74);
    assert_eq!(cache["posts"], serde_json::from_str::<Value>(OK).unwrap()["posts"]);
    assert!(cache["polled_at"].as_str().unwrap().ends_with('Z'));
    assert!(!fx.env.cpd_home().join("run/team-review.json.tmp").exists(), "written by rename");

    let calls = fx.env.calls("node");
    let script = fx.env.root().join("adapter/lib/slack-channel.ts");
    // The product reports canonical paths: /var is a symlink into /private on a Mac.
    let root = fx.env.root().canonicalize().unwrap();
    assert_eq!(
        calls[0],
        [
            script.to_str().unwrap(),
            "--coding-agent",
            root.join("install/releases/1.0.0/node_modules/@earendil-works/pi-coding-agent/dist/index.js").to_str().unwrap(),
            "--channel",
            CHANNEL,
            "--me",
            ME
        ]
    );
    assert_eq!(fs::read_to_string(fx.env.root().join("helper-agent-dir")).unwrap().trim(), root.to_str().unwrap());

    // A period of an hour: the next run is not due, whatever the ticks.
    fx.env.wait_whole_ticks(3);
    assert_eq!(fx.helper_calls(), 1);
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn e11_keeps_the_last_good_cache_when_a_run_fails_and_reports_the_failure_once() {
    let fx = fx(Some(1), 0);
    let _engine = fx.start();
    wait_until("the first cache", || fx.cache().is_some());
    let good = fx.cache().unwrap();

    fs::write(&fx.answer, format!("3 {AUTH}")).unwrap();
    let from = fx.helper_calls();
    wait_until("three failed runs", || fx.helper_calls() >= from + 3);
    assert_eq!(fx.cache().unwrap(), good, "the last good copy stays");
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "once, however many runs fail: {errors:?}");
    assert_eq!(errors[0]["code"], "slack_auth");
    assert_eq!(errors[0]["source"], "watch");
    assert!(errors[0]["message"].as_str().unwrap().contains("pi mcp login slack"));
    assert!(fx.items().is_empty(), "an engine error opens no Inbox item: it goes to the Manager (hs-d314)");

    // Signed in again: the cache is replaced.
    fs::write(&fx.answer, format!("0 {}", OK.replace("\"read\":74", "\"read\":75"))).unwrap();
    wait_until("a new cache", || fx.cache().is_some_and(|c| c["read"] == 75));
}

#[test]
fn e11_does_not_replace_the_cache_with_an_answer_that_holds_no_posts() {
    let fx = fx(Some(1), 0);
    fs::write(&fx.answer, "0 {\"ok\":true}").unwrap();
    let _engine = fx.start();

    wait_until("the helper's failure", || !fx.errors().is_empty());
    assert_eq!(fx.errors()[0]["code"], "slack_helper_failed");
    assert!(fx.cache().is_none());
    assert!(!fx.env.cpd_home().join("run/team-review.json.tmp").exists());
}

#[test]
fn e11_runs_nothing_without_a_team_channel() {
    let fx = fx(None, 0);
    let _engine = fx.start();
    fx.env.wait_whole_ticks(3);
    assert_eq!(fx.helper_calls(), 0);
    assert!(fx.cache().is_none());
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn e11_reports_pi_not_found_once_and_keeps_ticking() {
    let fx = fx(Some(1), 0);
    fs::remove_file(fx.env.root().join("install/current-version")).unwrap();
    let _engine = fx.start();

    wait_until("pi_not_found", || !fx.errors().is_empty());
    fx.env.wait_whole_ticks(3);
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["code"], "pi_not_found");
    assert_eq!(fx.helper_calls(), 0);
}

#[test]
fn e8_opens_the_review_item_for_a_cached_post_hermes_calls_needs_review() {
    let fx = fx(Some(3600), 0);
    // The post is unclaimed; GitHub (the second query of the fake) says nobody reviewed the pull request.
    let post = r#"{"ok":true,"channel":"C02JU6C2Z1D","read":1,"posts":[{"repo":"acme/pay","number":812,"ts":TS,"ts_raw":"1.1","author":"Ryan Case","primary":true,"reactions":[],"claim":null,"eyes_me":null,"thread_tags_me":false}]}"#
        .replace("TS", &std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs().to_string());
    fs::write(&fx.answer, format!("0 {post}")).unwrap();
    fx.env.fake_gh(
        r#"case "$*" in
*viewer*) echo '{"data":{"viewer":{"login":"clayton-duarte"},"p0":{"pullRequest":{"isDraft":false,"state":"OPEN","reviewDecision":null,"author":{"login":"ryan"},"latestReviews":{"nodes":[]}}}}}';;
*) echo '{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}';;
esac"#,
    );
    let _engine = fx.start();

    wait_until("the review item", || fx.items().contains(&("pr:acme/pay#812".to_owned(), "review pay#812".to_owned())));
}
