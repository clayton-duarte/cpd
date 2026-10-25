//! Engine step E14, model discovery (contract §3.7 E14, §1.4 `models-<provider>.json`, §8.2 `watch.models_discovery_seconds`;
//! Context decisions pro-d13, pro-d17, pro-d20): with the engine running, E14 starts the helper `pi/lib/models-list.ts`
//! (no argument, under pi's Node, the engine's environment) when no cached list exists or the newest `polled_at` is old
//! enough, validates its answer, writes one `run/models-<provider>.json` per provider that has a good list and reports a
//! failure once. The helper is a fake `node` on `PATH` (the real one is tested in `pi/test/models-list.test.ts`).

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::time::Duration;

use common::{CPD, TestEnv, wait_until};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The fake key: set in the engine's environment, as pi's `models.json` would name a variable. It must reach the
/// helper and nothing else.
const KEY: &str = "sk-bf-test-key";
const BASE: &str = "http://127.0.0.1:8788";

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

/// An entry of the helper's answer for a provider that was listed.
fn listed(provider: &str, auth: &str, base: Option<&str>, ids: &[&str]) -> Value {
    json!({"provider": provider, "auth": auth, "base": base, "ids": ids})
}

/// An entry for a provider that was not.
fn failed(provider: &str, code: &str) -> Value {
    json!({"provider": provider, "error": {"code": code, "message": format!("{provider}: {code}"), "details": {"status": 401}}})
}

fn answer(providers: Vec<Value>) -> Value {
    json!({"ok": true, "pi_version": "1.3.0", "providers": providers})
}

struct Fx {
    env: TestEnv,
    /// Where the fake helper takes its answer from: `<exit> <json>` in the first line.
    answer: PathBuf,
}

/// A fake pi install (agent directory at the env root), a fake `gh` (E5 polls at every tick) and the fake helper
/// `node`, which records its environment and answers with the contents of `answer`. `watch` is more `[watch]` keys.
fn fx(watch: &str, first: &Value) -> Fx {
    let env = TestEnv::new();
    let adapter = env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n[watch]\ntick_seconds = 1\npr_poll_seconds = 1\n{watch}\n[tmux]\nsocket = \"{}\"\n",
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
    fs::create_dir_all(env.cpd_home().join("run")).unwrap();
    let fx = Fx { answer: env.root().join("helper-answer"), env };
    fx.say(0, first);
    fx.env.fake(
        "node",
        &format!(
            "env > '{root}/helper-env'\nread -r status json < '{answer}'\necho \"$json\"\nexit \"$status\"",
            root = fx.env.root().display(),
            answer = fx.answer.display()
        ),
    );
    fx
}

impl Fx {
    /// What the helper answers from now on (written whole, then renamed: the helper never reads half of it).
    fn say(&self, exit: i32, json: &Value) {
        let tmp = self.answer.with_extension("tmp");
        fs::write(&tmp, format!("{exit} {json}")).unwrap();
        fs::rename(&tmp, &self.answer).unwrap();
    }

    fn start(&self) -> Engine {
        let child = self
            .env
            .command(CPD)
            .arg("watch")
            .env_remove("CPD_TEST_MODELS_DISCOVERY")
            .env("BIFROST_TEST_KEY", KEY)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let engine = Engine(child);
        wait_until("watch-started", || self.query("SELECT count(*) FROM events WHERE kind = 'watch-started'") >= 1);
        engine
    }

    fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap(), 5000).unwrap()
    }

    fn query(&self, sql: &str) -> i64 {
        self.store().read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }

    fn path(&self, provider: &str) -> PathBuf {
        self.env.cpd_home().join(format!("run/models-{provider}.json"))
    }

    fn file(&self, provider: &str) -> Option<Value> {
        serde_json::from_str(&fs::read_to_string(self.path(provider)).ok()?).ok()
    }

    /// A cached file the engine wrote on an earlier run: `polled_at` is `ago` seconds ago.
    fn cache(&self, provider: &str, ago: i64, auth: &str, ids: &[&str], changes: Option<Value>) {
        let polled_at = (OffsetDateTime::now_utc() - time::Duration::seconds(ago)).format(&Rfc3339).unwrap();
        let mut file = json!({
            "version": 1, "provider": provider, "polled_at": polled_at, "pi_version": "1.2.0",
            "source": {"kind": if auth == "custom" { "openai-models" } else { "copilot-available" }, "auth": auth, "base": if auth == "custom" { json!(BASE) } else { Value::Null }},
            "ids": ids
        });
        if let Some(changes) = changes {
            file["changes"] = changes;
        }
        fs::write(self.path(provider), file.to_string()).unwrap();
    }

    fn helper_calls(&self) -> usize {
        self.env.calls("node").len()
    }

    /// The `error` Events of E14: their data.
    fn errors(&self) -> Vec<Value> {
        self.store()
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT data FROM events WHERE kind = 'error' ORDER BY id")?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows.iter().map(|d| serde_json::from_str::<Value>(d).unwrap()).filter(|d| d["code"].as_str().is_some_and(|c| c.starts_with("models_") || c == "tool_timeout")).collect())
            })
            .unwrap()
    }

    /// The helper has run and the engine has ticked past it, so an absence can be asserted.
    fn settle(&self) {
        self.env.wait_whole_ticks(3);
    }
}

fn polled_at(file: &Value) -> OffsetDateTime {
    OffsetDateTime::parse(file["polled_at"].as_str().unwrap(), &Rfc3339).unwrap()
}

fn age(file: &Value) -> i64 {
    (OffsetDateTime::now_utc() - polled_at(file)).whole_seconds()
}

const WEEK: i64 = 7 * 86400;

#[test]
fn the_first_poll_writes_a_file_per_provider_with_sorted_ids_and_no_changes_and_opens_nothing_else() {
    let fx = fx("", &answer(vec![
        listed("bifrost", "custom", Some(BASE), &["vertex/b", "vertex/a"]),
        json!({"provider": "github-copilot", "auth": "oauth", "base": null, "ids": ["gpt-5.6-sol", "claude-opus-5"], "known": ["gpt-6-astra", "claude-opus-5", "gpt-5.6-sol"]}),
    ]));
    let _engine = fx.start();

    wait_until("both files", || fx.file("bifrost").is_some() && fx.file("github-copilot").is_some());
    let bifrost = fx.file("bifrost").unwrap();
    assert_eq!(bifrost["version"], 1);
    assert_eq!(bifrost["provider"], "bifrost");
    assert_eq!(bifrost["ids"], json!(["vertex/a", "vertex/b"]));
    assert_eq!(bifrost["pi_version"], "1.3.0");
    assert_eq!(bifrost["source"], json!({"kind": "openai-models", "auth": "custom", "base": BASE}));
    assert!(age(&bifrost).abs() < 60, "polled_at is now: {bifrost}");
    assert!(bifrost.get("changes").is_none(), "the first run announces nothing: {bifrost}");
    assert!(bifrost.get("known").is_none());

    let copilot = fx.file("github-copilot").unwrap();
    assert_eq!(copilot["ids"], json!(["claude-opus-5", "gpt-5.6-sol"]));
    assert_eq!(copilot["known"], json!(["claude-opus-5", "gpt-5.6-sol", "gpt-6-astra"]));
    assert_eq!(copilot["source"], json!({"kind": "copilot-available", "auth": "oauth", "base": null}));
    assert!(copilot.get("changes").is_none());

    for provider in ["bifrost", "github-copilot"] {
        assert!(!fx.env.cpd_home().join(format!("run/models-{provider}.json.tmp")).exists(), "written by rename");
    }
    // The helper takes no argument: pi's Node runs the script, and nothing else.
    let script = fx.env.root().join("adapter/lib/models-list.ts");
    assert_eq!(fx.env.calls("node")[0], [script.to_str().unwrap()]);

    fx.settle();
    assert_eq!(fx.helper_calls(), 1, "a week is not up");
    assert_eq!(fx.errors(), Vec::<Value>::new());
    assert_eq!(fx.query("SELECT count(*) FROM inbox_items"), 0, "the notice is a later PR");
}

#[test]
fn a_second_poll_with_ids_added_and_removed_records_the_changes() {
    let fx = fx("", &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-d", "m-b", "m-c"])]));
    fx.cache("bifrost", WEEK + 3600, "custom", &["m-a", "m-b", "m-c"], None);
    let _engine = fx.start();

    wait_until("the changes", || fx.file("bifrost").is_some_and(|f| f.get("changes").is_some()));
    let file = fx.file("bifrost").unwrap();
    assert_eq!(file["ids"], json!(["m-b", "m-c", "m-d"]));
    assert_eq!(file["changes"]["appeared"], json!(["m-d"]));
    assert_eq!(file["changes"]["went_away"], json!(["m-a"]));
    assert!(file["changes"]["at"].as_str().unwrap().ends_with('Z'));
    assert!(age(&file).abs() < 60, "polled_at moved to this list: {file}");
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn the_poll_is_not_due_before_the_newest_polled_at_is_a_week_old_even_on_a_fresh_engine() {
    let fx = fx("", &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-b"])]));
    // Two files: the old one does not make it due, the newest counts.
    fx.cache("github-copilot", WEEK + 100, "oauth", &["a"], None);
    fx.cache("bifrost", 3600, "custom", &["m-a"], None);
    let _engine = fx.start();
    fx.settle();
    assert_eq!(fx.helper_calls(), 0, "the newest file is an hour old");
    assert_eq!(fx.file("bifrost").unwrap()["ids"], json!(["m-a"]));
    // Once the newest file is a week old too, it is due.
    fx.cache("bifrost", WEEK + 60, "custom", &["m-a"], None);
    wait_until("the helper", || fx.helper_calls() == 1);
}

#[test]
fn a_fresh_engine_on_a_week_old_file_runs_the_helper() {
    let fx = fx("", &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-b"])]));
    fx.cache("bifrost", WEEK + 60, "custom", &["m-a"], None);
    let _engine = fx.start();
    wait_until("the helper", || fx.helper_calls() == 1);
    wait_until("the new list", || fx.file("bifrost").is_some_and(|f| f["ids"] == json!(["m-b"])));
}

#[test]
fn a_restart_repeats_nothing_the_week_counts_from_polled_at() {
    let fx = fx("", &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-a"])]));
    {
        let _first = fx.start();
        wait_until("the first list", || fx.file("bifrost").is_some());
        fx.settle();
    }
    assert_eq!(fx.helper_calls(), 1);
    let _second = fx.start();
    fx.settle();
    assert_eq!(fx.helper_calls(), 1, "the file is fresh: a restart does not poll");
}

#[test]
fn the_period_is_the_key_models_discovery_seconds_counted_from_polled_at() {
    // Older than the key: due.
    let old = fx("models_discovery_seconds = 3600", &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-b"])]));
    old.cache("bifrost", 2 * 3600, "custom", &["m-a"], None);
    let _engine = old.start();
    wait_until("the helper", || old.helper_calls() == 1);
    drop(_engine);

    // Younger: not due, though far from a week.
    let young = fx("models_discovery_seconds = 3600", &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-b"])]));
    young.cache("bifrost", 600, "custom", &["m-a"], None);
    let _engine = young.start();
    young.settle();
    assert_eq!(young.helper_calls(), 0);
    young.cache("bifrost", 2 * 3600, "custom", &["m-a"], None);
    wait_until("the helper", || young.helper_calls() == 1);
}

#[test]
fn a_failed_run_keeps_the_files_raises_one_error_and_waits_error_repeat_seconds() {
    let down = json!({"ok": false, "error": {"code": "models_no_provider", "message": "pi can use no provider", "details": {}}});
    let fx = fx("error_repeat_seconds = 3600", &down);
    fx.say(1, &down);
    fx.cache("bifrost", WEEK + 3600, "custom", &["m-a"], None);
    let before = fs::read_to_string(fx.path("bifrost")).unwrap();
    let _engine = fx.start();

    wait_until("the error", || !fx.errors().is_empty());
    fx.settle();
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "once: {errors:?}");
    assert_eq!(errors[0]["code"], "models_no_provider");
    assert_eq!(errors[0]["source"], "watch");
    assert_eq!(fx.helper_calls(), 1, "the next attempt is not before error_repeat_seconds");
    assert_eq!(fs::read_to_string(fx.path("bifrost")).unwrap(), before, "the last good file stays");
    assert_eq!(fx.query("SELECT count(*) FROM inbox_items"), 0, "an engine error opens no Inbox item");
}

#[test]
fn after_error_repeat_seconds_it_tries_again_and_a_good_answer_writes_the_files() {
    let down = json!({"ok": false, "error": {"code": "models_refresh_failed", "message": "pi.dev: 500", "details": {}}});
    let fx = fx("error_repeat_seconds = 1", &down);
    fx.say(1, &down);
    let _engine = fx.start();
    wait_until("two attempts", || fx.helper_calls() >= 2);
    assert!(fx.file("bifrost").is_none(), "a failing run with no file writes nothing");

    fx.say(0, &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-a"])]));
    wait_until("the file", || fx.file("bifrost").is_some());
}

#[test]
fn an_empty_list_is_an_error_and_never_replaces_a_good_file() {
    let fx = fx("error_repeat_seconds = 3600", &answer(vec![listed("bifrost", "custom", Some(BASE), &[])]));
    fx.cache("bifrost", WEEK + 3600, "custom", &["m-a"], None);
    let before = fs::read_to_string(fx.path("bifrost")).unwrap();
    let _engine = fx.start();

    wait_until("the error", || !fx.errors().is_empty());
    assert_eq!(fx.errors()[0]["code"], "models_empty");
    assert_eq!(fx.errors()[0]["details"]["provider"], "bifrost");
    assert_eq!(fs::read_to_string(fx.path("bifrost")).unwrap(), before);
}

#[test]
fn an_answer_that_is_not_the_contract_is_models_helper_failed_and_writes_nothing() {
    for bad in [json!({"ok": true, "pi_version": "1.3.0"}), json!({"ok": true, "providers": []}), json!({"ok": true, "pi_version": "1.3.0", "providers": [{"provider": "bifrost", "auth": "token", "base": null, "ids": ["a"]}]})] {
        let fx = fx("error_repeat_seconds = 3600", &bad);
        let _engine = fx.start();
        wait_until("the error", || !fx.errors().is_empty());
        assert_eq!(fx.errors()[0]["code"], "models_helper_failed", "{bad}");
        assert!(fx.file("bifrost").is_none(), "{bad}");
    }
}

#[test]
fn one_failing_provider_keeps_its_file_and_raises_its_own_error_while_the_others_are_written() {
    let fx = fx(
        "error_repeat_seconds = 3600",
        &answer(vec![
            failed("bifrost", "models_unauthorized"),
            json!({"provider": "github-copilot", "auth": "oauth", "base": null, "ids": ["gpt-5.6-sol"]}),
        ]),
    );
    fx.cache("bifrost", WEEK + 3600, "custom", &["m-a"], None);
    let before = fs::read_to_string(fx.path("bifrost")).unwrap();
    let _engine = fx.start();

    wait_until("the copilot file", || fx.file("github-copilot").is_some());
    wait_until("the error", || !fx.errors().is_empty());
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["code"], "models_unauthorized");
    assert_eq!(errors[0]["details"]["provider"], "bifrost");
    assert_eq!(fs::read_to_string(fx.path("bifrost")).unwrap(), before, "the failing provider keeps its file");
    assert_eq!(fx.file("github-copilot").unwrap()["ids"], json!(["gpt-5.6-sol"]));
}

#[test]
fn an_auth_type_change_rewrites_the_file_with_no_changes() {
    let fx = fx("", &answer(vec![json!({"provider": "github-copilot", "auth": "oauth", "base": null, "ids": ["gpt-5.6-sol", "claude-opus-5"]})]));
    // An API-key login lists everything: the two lists are not comparable.
    let earlier = json!({"at": "2026-01-01T00:00:00Z", "appeared": ["x"], "went_away": []});
    fx.cache("github-copilot", WEEK + 3600, "api_key", &["a", "b", "c", "claude-opus-5"], Some(earlier));
    let _engine = fx.start();

    wait_until("the rewrite", || fx.file("github-copilot").is_some_and(|f| f["source"]["auth"] == "oauth"));
    let file = fx.file("github-copilot").unwrap();
    assert_eq!(file["ids"], json!(["claude-opus-5", "gpt-5.6-sol"]));
    assert!(file.get("changes").is_none(), "a previous `changes` is dropped: {file}");
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn a_provider_the_helper_no_longer_lists_keeps_its_file_untouched() {
    let fx = fx("", &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-b"])]));
    fx.cache("bifrost", WEEK + 3600, "custom", &["m-a"], None);
    fx.cache("ghost", WEEK + 7200, "custom", &["g-1"], None);
    let ghost = fs::read_to_string(fx.path("ghost")).unwrap();
    let _engine = fx.start();

    wait_until("the new list", || fx.file("bifrost").is_some_and(|f| f["ids"] == json!(["m-b"])));
    assert_eq!(fs::read_to_string(fx.path("ghost")).unwrap(), ghost);
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn a_file_the_engine_cannot_parse_is_treated_as_missing_and_reported_once() {
    let fx = fx("", &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-a"])]));
    fs::write(fx.path("bifrost"), "{not json").unwrap();
    let _engine = fx.start();

    wait_until("the new file", || fx.file("bifrost").is_some());
    let file = fx.file("bifrost").unwrap();
    assert!(file.get("changes").is_none(), "a missing file is a first run");
    wait_until("the report", || !fx.errors().is_empty());
    fx.settle();
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["code"], "models_unparseable");
    assert!(errors[0]["details"]["file"].as_str().unwrap().ends_with("models-bifrost.json"));
}

#[test]
fn the_helper_runs_with_the_engines_environment_and_the_key_is_never_an_argument_or_an_output() {
    // A failure whose message tries to carry the key: the engine must not add it anywhere itself.
    let fx = fx("error_repeat_seconds = 3600", &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-a"]), failed("github-copilot", "models_unreachable")]));
    let _engine = fx.start();
    wait_until("the file and the error", || fx.file("bifrost").is_some() && !fx.errors().is_empty());

    let helper_env = fs::read_to_string(fx.env.root().join("helper-env")).unwrap();
    assert!(helper_env.lines().any(|l| l == format!("BIFROST_TEST_KEY={KEY}")), "the helper inherits the engine's environment");
    for call in fx.env.calls("node") {
        assert!(call.iter().all(|a| !a.contains(KEY)), "no argument has the key: {call:?}");
    }
    let events: String = fx
        .store()
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT data FROM events")?;
            Ok(stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?.join("\n"))
        })
        .unwrap();
    assert!(!events.contains(KEY), "no Event holds the key");
    assert!(!fs::read_to_string(fx.path("bifrost")).unwrap().contains(KEY));
}
