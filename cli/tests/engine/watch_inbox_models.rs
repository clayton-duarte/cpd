//! Engine step E8, the `models-changed` Inbox items (contract §2.2 "Inbox item" and "The models-changed rule", §3.7 E8;
//! commands.md §21.4; pro-d13, pro-d16, pro-n12): E8 reads each `run/models-<provider>.json` that E15 wrote and opens one white
//! item per provider whose `changes.at` is within the last 7 days and whose announced part is not empty. The engine runs
//! in-process on the `ManualClock` ([`Fx::watch_ticks`]); no helper runs (`Fx::engine_config` writes a fresh cache file, so E15
//! is not due), except in the one test about E15's error.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use common::fx::Fx;
use cpd_core::clock::Clock;
use cpd_core::host::runner::Reply;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const DAY: i64 = 86400;

/// The announced ids the rule leaves out of Bifrost's list, one of each kind, and what each is called in the pattern.
const NON_CHAT: [&str; 7] = [
    "vertex/text-embedding-004",
    "openai/tts-1",
    "openai/whisper-transcribe",
    "gemini/gemini-live-2",
    "openai/gpt-image-1",
    "vertex/nano-banana",
    "mistral/mistral-ocr",
];

struct Models {
    fx: Fx,
}

fn fx() -> Models {
    let fx = Fx::with_ports();
    fx.engine_config("");
    fx.engine_routes();
    Models { fx }
}

/// `at` as the file writes it: RFC 3339 in UTC, whole seconds.
fn stamp(at: OffsetDateTime) -> String {
    at.replace_nanosecond(0).unwrap().format(&Rfc3339).unwrap()
}

fn cache_file(
    home: &std::path::Path,
    provider: &str,
    polled_at: OffsetDateTime,
    changes: Option<Value>,
) -> PathBuf {
    let auth = if provider == "bifrost" {
        "custom"
    } else {
        "oauth"
    };
    let mut file = json!({
        "version": 1, "provider": provider, "polled_at": stamp(polled_at), "pi_version": "1.3.0",
        "source": {"kind": if provider == "bifrost" { "openai-models" } else { "copilot-available" }, "auth": auth, "base": null},
        "ids": ["anthropic/claude-sonnet-5"]
    });
    if let Some(changes) = changes {
        file["changes"] = changes;
    }
    fs::create_dir_all(home.join("run")).unwrap();
    let path = home.join(format!("run/models-{provider}.json"));
    fs::write(&path, file.to_string()).unwrap();
    path
}

impl Models {
    /// The time `days` days (fractions allowed, in seconds) before the engine's clock.
    fn ago(&self, seconds: i64) -> OffsetDateTime {
        self.fx.fakes.clock.utc() - time::Duration::seconds(seconds)
    }

    /// `run/models-<provider>.json` as E15 wrote it a minute ago, with `changes` made `changed_ago` seconds ago. Returns the `at`.
    fn change(
        &self,
        provider: &str,
        changed_ago: i64,
        appeared: &[&str],
        went_away: &[&str],
    ) -> String {
        let at = stamp(self.ago(changed_ago));
        let changes = json!({"at": at, "appeared": appeared, "went_away": went_away});
        cache_file(
            &self.fx.env.cpd_home(),
            provider,
            self.ago(60),
            Some(changes),
        );
        at
    }

    /// A first run: the file has no `changes`.
    fn first_run(&self, provider: &str) {
        cache_file(&self.fx.env.cpd_home(), provider, self.ago(60), None);
    }

    fn path(&self, provider: &str) -> PathBuf {
        self.fx
            .env
            .cpd_home()
            .join(format!("run/models-{provider}.json"))
    }

    /// The engine starts, ticks once and stops.
    fn tick(&self) {
        self.fx.watch_ticks(1).unwrap();
    }

    /// The open visible items of `kind` of `cpd inbox list`, as JSON, in the order of the list.
    fn items(&self) -> Vec<Value> {
        let json = self.fx.cpd(&["inbox", "list"]).ok();
        json["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["kind"] == "models-changed")
            .cloned()
            .collect()
    }

    fn labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = self
            .items()
            .iter()
            .map(|i| i["label"].as_str().unwrap().to_owned())
            .collect();
        labels.sort();
        labels
    }

    /// The `models-changed` items of `cpd inbox list --closed`.
    fn closed(&self) -> Vec<Value> {
        let json = self.fx.cpd(&["inbox", "list", "--closed"]).ok();
        json["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["kind"] == "models-changed")
            .cloned()
            .collect()
    }

    /// Every `models-changed` row of the store, open or closed, hidden or not.
    fn rows(&self) -> i64 {
        let conn = rusqlite::Connection::open_with_flags(
            self.fx.env.cpd_home().join("cpd.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        conn.query_row(
            "SELECT count(*) FROM inbox_items WHERE kind = 'models-changed'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }
}

#[test]
fn a_change_within_7_days_opens_one_white_item_per_provider_with_no_context_and_the_subject_and_label_of_the_rule()
 {
    let fx = fx();
    let bifrost = fx.change(
        "bifrost",
        2 * DAY,
        &["anthropic/claude-sonnet-5", "openai/gpt-5.6"],
        &["anthropic/claude-sonnet-4"],
    );
    let copilot = fx.change("github-copilot", 3600, &["gpt-6-astra"], &[]);
    fx.tick();

    let items = fx.items();
    assert_eq!(items.len(), 2, "one item per provider: {items:?}");
    let of = |provider: &str| {
        items
            .iter()
            .find(|i| {
                i["subject"]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!("models:{provider}/"))
            })
            .unwrap_or_else(|| panic!("no item of {provider}: {items:?}"))
            .clone()
    };
    let (b, c) = (of("bifrost"), of("github-copilot"));
    assert_eq!(b["subject"], format!("models:bifrost/{bifrost}"));
    assert_eq!(b["label"], "bifrost: 2 new, 1 gone");
    assert_eq!(c["subject"], format!("models:github-copilot/{copilot}"));
    assert_eq!(
        c["label"], "github-copilot: 1 new",
        "a zero part is omitted"
    );
    for item in [&b, &c] {
        assert_eq!(item["urgency"], "white", "{item}");
        for null in ["context", "plan", "job", "session", "record"] {
            assert!(item[null].is_null(), "{null} must be null: {item}");
        }
    }

    // nothing changed: the next tick opens nothing and writes nothing
    let ids: Vec<Value> = items.iter().map(|i| i["id"].clone()).collect();
    fx.tick();
    assert_eq!(
        fx.items()
            .iter()
            .map(|i| i["id"].clone())
            .collect::<Vec<_>>(),
        ids,
        "the same items, not new ones"
    );
    assert_eq!(fx.rows(), 2);
}

#[test]
fn a_gone_only_change_is_labelled_with_the_gone_part_alone() {
    let fx = fx();
    fx.change("github-copilot", 3600, &[], &["gpt-5.5"]);
    fx.tick();
    assert_eq!(fx.labels(), ["github-copilot: 1 gone"]);
}

#[test]
fn a_file_with_no_changes_the_first_run_opens_nothing_while_another_providers_change_does() {
    let fx = fx();
    fx.first_run("bifrost");
    fx.change("github-copilot", 3600, &["gpt-6-astra"], &[]);
    fx.tick();
    assert_eq!(
        fx.labels(),
        ["github-copilot: 1 new"],
        "only the provider that has `changes`"
    );
}

#[test]
fn changes_older_than_7_days_open_nothing_and_close_an_open_item_when_the_window_passes() {
    let fx = fx();
    // too old from the start
    fx.change("bifrost", 8 * DAY, &["openai/gpt-5.6"], &[]);
    fx.change("github-copilot", 6 * DAY, &["gpt-6-astra"], &[]);
    fx.tick();
    assert_eq!(
        fx.labels(),
        ["github-copilot: 1 new"],
        "the 8-day-old change opens nothing"
    );

    // the window passes: 6 days and a few hours later the 6-day-old change is 7 days old
    let item = fx.items()[0].clone();
    fx.fx
        .advance(std::time::Duration::from_secs((DAY + 3600) as u64));
    fx.tick();
    assert_eq!(
        fx.items(),
        Vec::<Value>::new(),
        "closed when the window passes"
    );
    let closed = fx.closed();
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert_eq!(
        closed[0]["id"], item["id"],
        "the item closed, it was not deleted"
    );
}

#[test]
fn non_chat_bifrost_ids_are_not_announced_and_a_change_made_only_of_them_opens_nothing_while_copilot_announces_every_id()
 {
    let fx = fx();
    // the same ids in two providers: Bifrost filters, Copilot's whole `available` list is announced
    fx.change(
        "bifrost",
        3600,
        &NON_CHAT,
        &["openai/tts-1-hd", "openai/DALL-E-IMAGE"],
    );
    fx.change("github-copilot", 3600, &NON_CHAT, &["openai/tts-1-hd"]);
    fx.tick();
    assert_eq!(
        fx.labels(),
        ["github-copilot: 7 new, 1 gone"],
        "Bifrost's change is all non-chat: nothing; Copilot announces every id"
    );
}

#[test]
fn each_non_chat_pattern_is_dropped_whatever_its_case_and_the_label_counts_only_the_chat_ids() {
    let fx = fx();
    let mut appeared: Vec<&str> = NON_CHAT.to_vec();
    appeared.extend([
        "Vertex/TEXT-EMBED-1",
        "anthropic/claude-sonnet-5",
        "openai/gpt-5.6",
    ]);
    fx.change(
        "bifrost",
        3600,
        &appeared,
        &[
            "anthropic/claude-sonnet-4",
            "openai/whisper-transcribe",
            "mistral/MISTRAL-OCR",
        ],
    );
    fx.tick();
    assert_eq!(
        fx.labels(),
        ["bifrost: 2 new, 1 gone"],
        "a mixed change counts only its chat ids"
    );
}

#[test]
fn a_change_whose_announced_part_is_empty_opens_nothing_and_a_gone_part_of_chat_ids_alone_does() {
    let fx = fx();
    fx.change("bifrost", 3600, &[], &["vertex/text-embedding-004"]);
    fx.change("github-copilot", 3600, &[], &["gpt-5.5"]);
    fx.tick();
    assert_eq!(fx.labels(), ["github-copilot: 1 gone"]);
}

#[test]
fn a_newer_change_closes_the_old_item_and_opens_a_new_one() {
    let fx = fx();
    let first = fx.change("bifrost", 3 * DAY, &["openai/gpt-5.6"], &[]);
    fx.tick();
    let old = fx.items();
    assert_eq!(old.len(), 1, "{old:?}");
    assert_eq!(old[0]["subject"], format!("models:bifrost/{first}"));

    let second = fx.change(
        "bifrost",
        60,
        &["openai/gpt-5.7", "openai/gpt-5.8"],
        &["openai/gpt-5.6"],
    );
    assert!(first != second);
    fx.tick();
    let now = fx.items();
    assert_eq!(now.len(), 1, "the previous one closes: {now:?}");
    assert_eq!(now[0]["subject"], format!("models:bifrost/{second}"));
    assert_eq!(now[0]["label"], "bifrost: 2 new, 1 gone");
    assert!(now[0]["id"] != old[0]["id"], "a new change is a new item");
    let closed = fx.closed();
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert_eq!(closed[0]["id"], old[0]["id"]);
}

#[test]
fn dismiss_hides_the_item_while_its_change_lives_and_a_newer_change_shows_again() {
    let fx = fx();
    let first = fx.change("bifrost", 3 * DAY, &["openai/gpt-5.6"], &[]);
    fx.tick();
    let id = fx.items()[0]["id"].as_i64().expect("an item");
    fx.fx.cpd(&["inbox", "dismiss", &id.to_string()]).ok();
    assert_eq!(fx.items(), Vec::<Value>::new(), "dismissed: hidden");

    fx.tick();
    assert_eq!(
        fx.items(),
        Vec::<Value>::new(),
        "the same change does not come back"
    );
    assert_eq!(fx.rows(), 1, "and does not open a second row");
    let all = fx.fx.cpd(&["inbox", "list", "--all"]).ok();
    let hidden: Vec<&Value> = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "models-changed")
        .collect();
    assert_eq!(hidden.len(), 1, "{all}");
    assert_eq!(hidden[0]["hidden"], "dismissed");
    assert_eq!(hidden[0]["subject"], format!("models:bifrost/{first}"));

    let second = fx.change("bifrost", 60, &["openai/gpt-5.7"], &[]);
    fx.tick();
    let shown = fx.items();
    assert_eq!(
        shown.len(),
        1,
        "a new cause is a new item and shows again: {shown:?}"
    );
    assert_eq!(shown[0]["subject"], format!("models:bifrost/{second}"));
}

#[test]
fn a_missing_or_unparseable_file_keeps_the_open_item_and_a_file_of_another_version_too() {
    let fx = fx();
    fx.change("bifrost", 3600, &["openai/gpt-5.6"], &[]);
    fx.tick();
    let open = fx.items();
    assert_eq!(open.len(), 1, "{open:?}");

    fs::write(fx.path("bifrost"), "{not json").unwrap();
    fx.tick();
    assert_eq!(
        fx.items(),
        open,
        "unparseable: the open item stays as it is"
    );

    let mut other = json!({"version": 2, "provider": "bifrost", "polled_at": stamp(fx.ago(60)), "ids": ["a"], "changes": {"at": stamp(fx.ago(60)), "appeared": ["b"], "went_away": []}});
    other["version"] = json!(2);
    fs::write(fx.path("bifrost"), other.to_string()).unwrap();
    fx.tick();
    assert_eq!(
        fx.items(),
        open,
        "not `version` 1: the open item stays as it is"
    );

    fs::remove_file(fx.path("bifrost")).unwrap();
    fx.tick();
    assert_eq!(fx.items(), open, "missing: the open item stays as it is");
    assert_eq!(fx.closed(), Vec::<Value>::new());
    let errors = fx.fx.cpd(&["inbox", "list"]).ok()["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "engine-error")
        .count();
    assert_eq!(errors, 0, "no `engine-error` condition");
}

/// The engine of E15's tests: the helper `node` is a route that fails, the cache is old enough for E15 to be due.
#[test]
fn a_discovery_error_is_not_an_inbox_item_only_the_files_change_is() {
    let fx = Fx::with_ports();
    let adapter = fx.env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fx.env.write_models();
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n[watch]\ntick_seconds = 1\npr_poll_seconds = 3600\nmodels_discovery_seconds = 3600\nerror_repeat_seconds = 3600\n",
        adapter.display()
    ));
    fx.engine_routes();
    fx.install_pi();
    let down = json!({"ok": false, "error": {"code": "models_refresh_failed", "message": "pi.dev: 500", "details": {}}});
    fx.fake_node(Reply::exit(1, &down.to_string(), ""));
    let at = stamp(fx.fakes.clock.utc() - time::Duration::hours(2));
    cache_file(
        &fx.env.cpd_home(),
        "github-copilot",
        fx.fakes.clock.utc() - time::Duration::hours(2),
        Some(json!({"at": at, "appeared": ["gpt-6-astra"], "went_away": []})),
    );

    let home = fx.env.cpd_home();
    let raised = move || {
        let conn = rusqlite::Connection::open_with_flags(
            home.join("cpd.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM events WHERE kind = 'error' AND data LIKE '%models_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        n > 0
    };
    let mut seen = None;
    let raised = Arc::new(raised);
    let waits = raised.clone();
    fx.watch_steps(move |nap| {
        if seen.is_none() && waits() {
            seen = Some(nap);
        }
        seen.is_some_and(|at| nap >= at + 30)
    })
    .unwrap();
    assert!(raised(), "E15 raised its error as an Event");

    let items = fx.cpd(&["inbox", "list"]).ok()["items"]
        .as_array()
        .unwrap()
        .clone();
    let models: Vec<&Value> = items
        .iter()
        .filter(|i| i["kind"] == "models-changed")
        .collect();
    assert_eq!(
        models.len(),
        1,
        "the file's change, and nothing for the error: {items:?}"
    );
    assert_eq!(models[0]["subject"], format!("models:github-copilot/{at}"));
    assert_eq!(
        items.iter().filter(|i| i["kind"] == "engine-error").count(),
        0,
        "the error goes to the Manager, not the Inbox: {items:?}"
    );
}
