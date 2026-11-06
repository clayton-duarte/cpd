//! Engine step E15, model discovery (contract §3.7 E15, §4.4 `swr_models`, §8.2 `watch.models_discovery_seconds`;
//! Context decisions pro-d13, pro-d17, pro-d20): with the engine running, E14 starts the helper `pi/lib/models-list.ts`
//! (no argument, under pi's Node, the engine's environment) when no cached list exists or the newest `polled_at` is old
//! enough, validates its answer, writes one `swr_models` row per provider that has a good list and reports a
//! failure once.
//!
//! In-process, on virtual time (docs/implementing.md, "Processes in tests", "Virtual-time loops"): the engine runs on the
//! `Fx`'s `ManualClock` and `FakeRunner`, its tmux is the fake one, and the helper is the route of the program `node` of
//! the `FakeRunner` (the real one is tested in `pi/test/models-list.test.ts`): it records what the call carried and answers
//! `<exit> <json>`. No process, no script and no sleep: an engine run ends when a state the test reads holds, and a week
//! passes by moving the clock.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fx::Fx as Ip;
use cpd_core::clock::{Clock, ManualClock};
use cpd_core::host::runner::{Call, Handler, Reply};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The fake key: set in the engine's environment, as pi's `models.json` would name a variable. It must reach the
/// helper and nothing else.
const KEY: &str = "sk-bf-test-key";
const BASE: &str = "http://127.0.0.1:8788";

/// The naps of the engine between two ticks: a tick is a second (`tick_seconds = 1`) and a nap a tenth of one.
const NAPS_PER_TICK: usize = 10;

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

/// One call of the helper, as the route saw it.
struct Seen {
    program: String,
    args: Vec<String>,
    /// What a child of this call would have: the process's environment with the call's changes applied.
    env: Vec<(String, String)>,
}

/// The helper: the route of `node`. It answers `<exit> <json>` (what `say` set last) and keeps what each call carried.
struct Node {
    says: Mutex<(i32, String)>,
    seen: Mutex<Vec<Seen>>,
}

impl Handler for Node {
    fn handle(&self, call: &Call) -> Reply {
        // Recorded and answered under one lock: the answer is the one `say` had set when the call was recorded.
        let says = self.says.lock().unwrap();
        let mut env: Vec<(String, String)> = std::env::vars().collect();
        for (key, value) in &call.envs {
            env.retain(|(k, _)| k != key);
            if let Some(value) = value {
                env.push((key.clone(), value.clone()));
            }
        }
        self.seen.lock().unwrap().push(Seen {
            program: call.program.clone(),
            args: call.args.clone(),
            env,
        });
        Reply::exit(says.0, &says.1, "")
    }
}

/// What a test reads of the home, the helper and the clock: `Clone` and `'static`, so an engine run's step holds one.
#[derive(Clone)]
struct Probe {
    home: PathBuf,
    node: Arc<Node>,
    clock: ManualClock,
}

impl Probe {
    fn calls(&self) -> usize {
        self.node.seen.lock().unwrap().len()
    }

    /// The row of `provider` as one value: its catalog, `polled_at`, `changes` (when it has them) and `last_error`.
    fn file(&self, provider: &str) -> Option<Value> {
        let (catalog, changes, polled, error): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = self
            .store()
            .query_row(
                "SELECT catalog_json, changes_json, polled_at, last_error FROM swr_models WHERE provider = ?1",
                [provider],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .ok()?;
        let mut file: Value = serde_json::from_str(&catalog?).ok()?;
        file["polled_at"] = json!(polled);
        if let Some(changes) = changes {
            file["changes"] = serde_json::from_str(&changes).unwrap();
        }
        if let Some(error) = error {
            file["last_error"] = json!(error);
        }
        Some(file)
    }

    /// A row the engine wrote on an earlier run: `polled_at` is `ago` seconds before the clock's now. The store exists.
    fn cache(&self, provider: &str, ago: i64, auth: &str, ids: &[&str], changes: Option<Value>) {
        let polled_at = (self.clock.utc() - time::Duration::seconds(ago))
            .format(&Rfc3339)
            .unwrap();
        let catalog = json!({
            "pi_version": "1.2.0",
            "source": {"kind": if auth == "custom" { "openai-models" } else { "copilot-available" }, "auth": auth, "base": if auth == "custom" { json!(BASE) } else { Value::Null }},
            "ids": ids
        });
        rusqlite::Connection::open(self.home.join("cpd.db"))
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO swr_models (provider, catalog_json, changes_json, polled_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![provider, catalog.to_string(), changes.map(|c| c.to_string()), polled_at],
            )
            .unwrap();
    }

    /// The engine's store, read on a connection of its own.
    fn store(&self) -> rusqlite::Connection {
        rusqlite::Connection::open_with_flags(
            self.home.join("cpd.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }

    /// The first column of `sql` (text) over its rows.
    fn column(&self, sql: &str) -> Vec<String> {
        let conn = self.store();
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    fn query(&self, sql: &str) -> i64 {
        self.store().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// The `error` Events of E14: their data.
    fn errors(&self) -> Vec<Value> {
        self.column("SELECT data FROM events WHERE kind = 'error' ORDER BY id")
            .iter()
            .map(|d| serde_json::from_str::<Value>(d).unwrap())
            .filter(|d| {
                d["code"]
                    .as_str()
                    .is_some_and(|c| c.starts_with("models_") || c == "tool_timeout")
            })
            .collect()
    }

    /// What the helper answers from now on.
    fn say(&self, exit: i32, json: &Value) {
        *self.node.says.lock().unwrap() = (exit, json.to_string());
    }

    /// Moves virtual time forward by `seconds`: nothing else happens in it but what the engine's next tick does.
    fn skip(&self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
    }
}

struct Fx {
    ip: Ip,
    node: Arc<Node>,
    adapter: PathBuf,
}

/// The engine on virtual time (E15 is due: no row is cached yet), a pi install, the helper `node` answering
/// `first` and the fake tmux. `watch` is more `[watch]` keys.
fn fx(watch: &str, first: &Value) -> Fx {
    let ip = Ip::with_ports();
    let adapter = ip.env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    ip.env.write_models();
    ip.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n[watch]\ntick_seconds = 1\npr_poll_seconds = 3600\n{watch}\n",
        adapter.display()
    ));
    ip.engine_routes();
    ip.install_pi();
    ip.export("BIFROST_TEST_KEY", KEY);
    let node = Arc::new(Node {
        says: Mutex::new((0, first.to_string())),
        seen: Mutex::default(),
    });
    let _ = (*ip.fakes.runner).clone().route("node", node.clone());
    Fx { ip, node, adapter }
}

impl Fx {
    fn probe(&self) -> Probe {
        Probe {
            home: self.ip.env.cpd_home(),
            node: self.node.clone(),
            clock: (*self.ip.fakes.clock).clone(),
        }
    }

    /// The engine runs, in this process, until `step` says it is done: it sees the state after each tick, with the number
    /// of the nap (1-based) it is before. A step that never says so ends the test after `MAX_NAPS` naps of virtual time.
    fn run(&self, mut step: impl FnMut(&Probe, usize) -> bool + Send + 'static) {
        let probe = self.probe();
        self.ip.watch_steps(move |nap| step(&probe, nap)).unwrap();
    }

    /// The engine runs until `done` holds, then `ticks` more ticks: what it does not do after that can be asserted.
    fn run_until(&self, done: impl Fn(&Probe) -> bool + Send + 'static, ticks: usize) {
        let mut seen = None;
        self.run(move |probe, nap| {
            if seen.is_none() && done(probe) {
                seen = Some(nap);
            }
            seen.is_some_and(|at| nap >= at + ticks * NAPS_PER_TICK)
        });
    }

    /// The engine runs `ticks` ticks.
    fn ticks(&self, ticks: usize) {
        self.run_until(|_| true, ticks);
    }

    fn cache(&self, provider: &str, ago: i64, auth: &str, ids: &[&str], changes: Option<Value>) {
        drop(self.ip.env.store());
        self.probe().cache(provider, ago, auth, ids, changes);
    }

    fn file(&self, provider: &str) -> Option<Value> {
        self.probe().file(provider)
    }

    fn helper_calls(&self) -> usize {
        self.probe().calls()
    }

    fn errors(&self) -> Vec<Value> {
        self.probe().errors()
    }

    fn query(&self, sql: &str) -> i64 {
        self.probe().query(sql)
    }

    /// How old `polled_at` of `file` is, by the engine's clock.
    fn age(&self, file: &Value) -> i64 {
        let polled = OffsetDateTime::parse(file["polled_at"].as_str().unwrap(), &Rfc3339).unwrap();
        (self.ip.fakes.clock.utc() - polled).whole_seconds()
    }
}

const WEEK: i64 = 7 * 86400;

/// Both files of the first run are there.
fn both(p: &Probe) -> bool {
    p.file("bifrost").is_some() && p.file("github-copilot").is_some()
}

#[test]
fn the_first_poll_writes_a_row_per_provider_with_sorted_ids_and_no_changes_and_opens_nothing_else()
{
    let fx = fx(
        "",
        &answer(vec![
            listed("bifrost", "custom", Some(BASE), &["vertex/b", "vertex/a"]),
            json!({"provider": "github-copilot", "auth": "oauth", "base": null, "ids": ["gpt-5.6-sol", "claude-opus-5"], "known": ["gpt-6-astra", "claude-opus-5", "gpt-5.6-sol"]}),
        ]),
    );
    fx.run_until(both, 3);

    let bifrost = fx.file("bifrost").unwrap();
    assert_eq!(bifrost["ids"], json!(["vertex/a", "vertex/b"]));
    assert_eq!(bifrost["pi_version"], "1.3.0");
    assert_eq!(
        bifrost["source"],
        json!({"kind": "openai-models", "auth": "custom", "base": BASE})
    );
    assert!(fx.age(&bifrost).abs() < 60, "polled_at is now: {bifrost}");
    assert!(
        bifrost.get("changes").is_none(),
        "the first run announces nothing: {bifrost}"
    );
    assert!(bifrost.get("known").is_none());

    let copilot = fx.file("github-copilot").unwrap();
    assert_eq!(copilot["ids"], json!(["claude-opus-5", "gpt-5.6-sol"]));
    assert_eq!(
        copilot["known"],
        json!(["claude-opus-5", "gpt-5.6-sol", "gpt-6-astra"])
    );
    assert_eq!(
        copilot["source"],
        json!({"kind": "copilot-available", "auth": "oauth", "base": null})
    );
    assert!(copilot.get("changes").is_none());

    for provider in ["bifrost", "github-copilot"] {
        assert!(
            !fx.ip
                .env
                .cpd_home()
                .join(format!("run/models-{provider}.json"))
                .exists(),
            "no file"
        );
    }
    // The helper takes no argument: pi's Node runs the script, and nothing else.
    let script = fx.adapter.join("lib/models-list.ts");
    let seen = fx.node.seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "a week is not up");
    assert_eq!(Path::new(&seen[0].program).file_name().unwrap(), "node");
    assert_eq!(seen[0].args, [script.to_str().unwrap()]);
    drop(seen);

    assert_eq!(fx.errors(), Vec::<Value>::new());
    assert_eq!(
        fx.query("SELECT count(*) FROM inbox_items"),
        0,
        "the notice is a later PR"
    );
}

#[test]
fn a_second_poll_with_ids_added_and_removed_records_the_changes() {
    let fx = fx(
        "",
        &answer(vec![listed(
            "bifrost",
            "custom",
            Some(BASE),
            &["m-d", "m-b", "m-c"],
        )]),
    );
    fx.cache(
        "bifrost",
        WEEK + 3600,
        "custom",
        &["m-a", "m-b", "m-c"],
        None,
    );
    fx.run_until(
        |p| {
            p.file("bifrost")
                .is_some_and(|f| f.get("changes").is_some())
        },
        0,
    );

    let file = fx.file("bifrost").unwrap();
    assert_eq!(file["ids"], json!(["m-b", "m-c", "m-d"]));
    assert_eq!(file["changes"]["appeared"], json!(["m-d"]));
    assert_eq!(file["changes"]["went_away"], json!(["m-a"]));
    assert!(file["changes"]["at"].as_str().unwrap().ends_with('Z'));
    assert!(
        fx.age(&file).abs() < 60,
        "polled_at moved to this list: {file}"
    );
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn the_poll_is_not_due_before_the_newest_polled_at_is_a_week_old_even_on_a_fresh_engine() {
    let fx = fx(
        "",
        &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-b"])]),
    );
    // Two files: the old one does not make it due, the newest counts.
    fx.cache("github-copilot", WEEK + 100, "oauth", &["a"], None);
    fx.cache("bifrost", 3600, "custom", &["m-a"], None);
    let mut aged = false;
    fx.run(move |p, nap| {
        if !aged {
            if nap >= 3 * NAPS_PER_TICK {
                assert_eq!(p.calls(), 0, "the newest file is an hour old");
                assert_eq!(p.file("bifrost").unwrap()["ids"], json!(["m-a"]));
                // Once the newest file is a week old too, it is due.
                p.cache("bifrost", WEEK + 60, "custom", &["m-a"], None);
                aged = true;
            }
            return false;
        }
        p.calls() == 1
    });
    assert_eq!(fx.helper_calls(), 1);
}

#[test]
fn a_fresh_engine_on_a_week_old_file_runs_the_helper() {
    let fx = fx(
        "",
        &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-b"])]),
    );
    fx.cache("bifrost", WEEK + 60, "custom", &["m-a"], None);
    fx.run_until(
        |p| {
            p.calls() == 1
                && p.file("bifrost")
                    .is_some_and(|f| f["ids"] == json!(["m-b"]))
        },
        0,
    );
    assert_eq!(fx.helper_calls(), 1);
}

#[test]
fn a_restart_repeats_nothing_the_week_counts_from_polled_at() {
    let fx = fx(
        "",
        &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-a"])]),
    );
    fx.run_until(|p| p.file("bifrost").is_some(), 3);
    assert_eq!(fx.helper_calls(), 1);
    // A second engine on the same home, a minute later: it has nothing in memory, the file says when it was polled.
    fx.ip.advance(Duration::from_secs(60));
    fx.ticks(3);
    assert_eq!(
        fx.helper_calls(),
        1,
        "the file is fresh: a restart does not poll"
    );
}

#[test]
fn the_period_is_the_key_models_discovery_seconds_counted_from_polled_at() {
    // Older than the key: due.
    {
        let old = fx(
            "models_discovery_seconds = 3600",
            &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-b"])]),
        );
        old.cache("bifrost", 2 * 3600, "custom", &["m-a"], None);
        old.run_until(|p| p.calls() == 1, 0);
        assert_eq!(old.helper_calls(), 1);
    }

    // Younger: not due, though far from a week.
    let young = fx(
        "models_discovery_seconds = 3600",
        &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-b"])]),
    );
    young.cache("bifrost", 600, "custom", &["m-a"], None);
    let mut aged = false;
    young.run(move |p, nap| {
        if !aged {
            if nap >= 3 * NAPS_PER_TICK {
                assert_eq!(p.calls(), 0);
                p.cache("bifrost", 2 * 3600, "custom", &["m-a"], None);
                aged = true;
            }
            return false;
        }
        p.calls() == 1
    });
    assert_eq!(young.helper_calls(), 1);
}

#[test]
fn a_failed_run_keeps_the_files_raises_one_error_and_waits_error_repeat_seconds() {
    let down = json!({"ok": false, "error": {"code": "models_no_provider", "message": "pi can use no provider", "details": {}}});
    let fx = fx("error_repeat_seconds = 3600", &down);
    fx.probe().say(1, &down);
    fx.cache("bifrost", WEEK + 3600, "custom", &["m-a"], None);
    let before = fx.file("bifrost").unwrap();
    // The error is raised; three ticks later 3000 s pass, and the next attempt is not before 3600 s after the run's end;
    // then 700 s more.
    let mut raised_at = None;
    fx.run(move |p, nap| {
        let Some(at) = raised_at else {
            raised_at = (!p.errors().is_empty()).then_some(nap);
            return false;
        };
        match nap - at {
            n if n == 3 * NAPS_PER_TICK => {
                assert_eq!(
                    p.calls(),
                    1,
                    "the next attempt is not before error_repeat_seconds"
                );
                p.skip(3000);
                false
            }
            n if n == 6 * NAPS_PER_TICK => {
                assert_eq!(p.calls(), 1, "3000 s after the run's end is not 3600 s");
                p.skip(700);
                false
            }
            n => n > 6 * NAPS_PER_TICK && p.calls() == 2,
        }
    });

    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "once: {errors:?}");
    assert_eq!(errors[0]["code"], "models_no_provider");
    assert_eq!(errors[0]["source"], "watch");
    assert_eq!(
        fx.helper_calls(),
        2,
        "the next attempt is after error_repeat_seconds"
    );
    assert_eq!(
        fx.file("bifrost").unwrap(),
        before,
        "the last good row stays"
    );
    assert_eq!(
        fx.query("SELECT count(*) FROM inbox_items"),
        0,
        "an engine error opens no Inbox item"
    );
}

#[test]
fn after_error_repeat_seconds_it_tries_again_and_a_good_answer_writes_the_files() {
    let down = json!({"ok": false, "error": {"code": "models_refresh_failed", "message": "pi.dev: 500", "details": {}}});
    let fx = fx("error_repeat_seconds = 1", &down);
    fx.probe().say(1, &down);
    let good = answer(vec![listed("bifrost", "custom", Some(BASE), &["m-a"])]);
    let mut said = false;
    fx.run(move |p, _| {
        if !said {
            if p.calls() >= 2 {
                assert!(
                    p.file("bifrost").is_none(),
                    "a failing run with no file writes nothing"
                );
                p.say(0, &good);
                said = true;
            }
            return false;
        }
        p.file("bifrost").is_some()
    });
    assert!(fx.helper_calls() >= 3, "two failed attempts and a good one");
    assert_eq!(fx.file("bifrost").unwrap()["ids"], json!(["m-a"]));
}

#[test]
fn an_empty_list_is_an_error_and_never_replaces_a_good_file() {
    let fx = fx(
        "error_repeat_seconds = 3600",
        &answer(vec![listed("bifrost", "custom", Some(BASE), &[])]),
    );
    fx.cache("bifrost", WEEK + 3600, "custom", &["m-a"], None);
    let before = fx.file("bifrost").unwrap();
    // `models_empty` is transient: it is reported when the outage is ten minutes old, at the next run.
    let mut skipped = false;
    fx.run(move |p, _| {
        if !skipped
            && p.file("bifrost")
                .is_some_and(|f| f.get("last_error").is_some())
        {
            p.skip(3600);
            skipped = true;
        }
        !p.errors().is_empty()
    });

    assert_eq!(fx.errors()[0]["code"], "models_empty");
    assert_eq!(fx.errors()[0]["details"]["provider"], "bifrost");
    let after = fx.file("bifrost").unwrap();
    assert_eq!(after["ids"], before["ids"]);
    assert_eq!(after["polled_at"], before["polled_at"]);
}

#[test]
fn an_answer_that_is_not_the_contract_is_models_helper_failed_and_writes_nothing() {
    for bad in [
        json!({"ok": true, "pi_version": "1.3.0"}),
        json!({"ok": true, "providers": []}),
        json!({"ok": true, "pi_version": "1.3.0", "providers": [{"provider": "bifrost", "auth": "token", "base": null, "ids": ["a"]}]}),
    ] {
        let fx = fx("error_repeat_seconds = 3600", &bad);
        fx.run_until(|p| !p.errors().is_empty(), 0);
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
    let before = fx.file("bifrost").unwrap();
    fx.run_until(
        |p| p.file("github-copilot").is_some() && !p.errors().is_empty(),
        0,
    );

    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["code"], "models_unauthorized");
    assert_eq!(errors[0]["details"]["provider"], "bifrost");
    let after = fx.file("bifrost").unwrap();
    assert_eq!(
        after["ids"], before["ids"],
        "the failing provider keeps its row"
    );
    assert_eq!(after["polled_at"], before["polled_at"]);
    assert_eq!(
        fx.file("github-copilot").unwrap()["ids"],
        json!(["gpt-5.6-sol"])
    );
}

#[test]
fn an_auth_type_change_rewrites_the_row_with_no_changes() {
    let fx = fx(
        "",
        &answer(vec![
            json!({"provider": "github-copilot", "auth": "oauth", "base": null, "ids": ["gpt-5.6-sol", "claude-opus-5"]}),
        ]),
    );
    // An API-key login lists everything: the two lists are not comparable.
    let earlier = json!({"at": "2026-01-01T00:00:00Z", "appeared": ["x"], "went_away": []});
    fx.cache(
        "github-copilot",
        WEEK + 3600,
        "api_key",
        &["a", "b", "c", "claude-opus-5"],
        Some(earlier),
    );
    fx.run_until(
        |p| {
            p.file("github-copilot")
                .is_some_and(|f| f["source"]["auth"] == "oauth")
        },
        0,
    );

    let file = fx.file("github-copilot").unwrap();
    assert_eq!(file["ids"], json!(["claude-opus-5", "gpt-5.6-sol"]));
    assert!(
        file.get("changes").is_none(),
        "a previous `changes` is dropped: {file}"
    );
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn a_provider_the_helper_no_longer_lists_keeps_its_row_untouched() {
    let fx = fx(
        "",
        &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-b"])]),
    );
    fx.cache("bifrost", WEEK + 3600, "custom", &["m-a"], None);
    fx.cache("ghost", WEEK + 7200, "custom", &["g-1"], None);
    let ghost = fx.file("ghost").unwrap();
    fx.run_until(
        |p| {
            p.file("bifrost")
                .is_some_and(|f| f["ids"] == json!(["m-b"]))
        },
        0,
    );

    assert_eq!(fx.file("ghost").unwrap(), ghost);
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn a_row_the_engine_cannot_parse_is_a_store_fault_reported_once() {
    let fx = fx(
        "error_repeat_seconds = 3600",
        &answer(vec![listed("bifrost", "custom", Some(BASE), &["m-a"])]),
    );
    fx.cache("bifrost", WEEK + 3600, "custom", &["m-a"], None);
    fx.ip
        .env
        .sql("UPDATE swr_models SET catalog_json = '{not json' WHERE provider = 'bifrost';");
    fx.run_until(
        |p| {
            p.query("SELECT count(*) FROM events WHERE kind = 'error' AND data LIKE '%store_failed%' AND text LIKE '%E15%'")
                > 0
        },
        3,
    );

    assert_eq!(
        fx.query(
            "SELECT count(*) FROM events WHERE kind = 'error' AND data LIKE '%store_failed%' AND text LIKE '%E15%'"
        ),
        1,
        "once"
    );
}

#[test]
fn the_helper_runs_with_the_engines_environment_and_the_key_is_never_an_argument_or_an_output() {
    // A failure whose message tries to carry the key: the engine must not add it anywhere itself.
    let fx = fx(
        "error_repeat_seconds = 3600",
        &answer(vec![
            listed("bifrost", "custom", Some(BASE), &["m-a"]),
            failed("github-copilot", "models_unauthorized"),
        ]),
    );
    fx.run_until(|p| p.file("bifrost").is_some() && !p.errors().is_empty(), 0);

    let seen = fx.node.seen.lock().unwrap();
    assert!(!seen.is_empty(), "the helper ran");
    for call in seen.iter() {
        assert!(
            call.env
                .contains(&("BIFROST_TEST_KEY".to_owned(), KEY.to_owned())),
            "the helper inherits the engine's environment"
        );
        assert!(
            call.args.iter().all(|a| !a.contains(KEY)),
            "no argument has the key: {:?}",
            call.args
        );
        assert!(
            call.env
                .iter()
                .filter(|(k, _)| k != "BIFROST_TEST_KEY")
                .all(|(_, v)| !v.contains(KEY)),
            "the key is in no other variable"
        );
    }
    drop(seen);
    let events = fx.probe().column("SELECT data FROM events").join("\n");
    assert!(!events.contains(KEY), "no Event holds the key");
    assert!(!fx.file("bifrost").unwrap().to_string().contains(KEY));
}
