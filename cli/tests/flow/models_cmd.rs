//! `cpd models refresh` and `cpd models show [--provider <p>]` (commands.md §14.5; contract §3.7 E15, §4.4): `refresh` runs
//! the helper `pi/lib/models-list.ts` now and writes the `swr_models` rows; `show` prints the cached lists and runs
//! nothing. The helper is a fake `node` on `PATH` (the real one is tested in `pi/test/models-list.test.ts`).

use crate::common::{Run, TestEnv, refused};

use std::fs;

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The fake key: in the caller's environment, as pi's `models.json` would name a variable; it must reach the helper and
/// nothing else.
const KEY: &str = "sk-bf-test-key";
const BASE: &str = "http://127.0.0.1:8788";

struct Fx {
    env: TestEnv,
}

fn listed(provider: &str, auth: &str, base: Option<&str>, ids: &[&str]) -> Value {
    json!({"provider": provider, "auth": auth, "base": base, "ids": ids})
}

fn answer(providers: Vec<Value>) -> Value {
    json!({"ok": true, "pi_version": "1.3.0", "providers": providers})
}

impl Fx {
    /// A fake pi install (agent directory at the env root) and a fake helper `node` that records its environment and
    /// prints `first`.
    fn new(first: &Value) -> Fx {
        let env = TestEnv::new();
        let adapter = env.root().join("adapter");
        fs::create_dir_all(&adapter).unwrap();
        fs::write(adapter.join("package.json"), "{}").unwrap();
        env.write_config(&format!(
            "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n",
            adapter.display()
        ));
        env.fake_pi("exit 0");
        fs::create_dir_all(env.root().join("install")).unwrap();
        fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
        let scope = env
            .root()
            .join("install/releases/1.0.0/node_modules/@earendil-works");
        for package in ["pi-coding-agent", "pi-ai"] {
            fs::create_dir_all(scope.join(package).join("dist")).unwrap();
            fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
        }
        fs::create_dir_all(env.cpd_home().join("run")).unwrap();
        let fx = Fx { env };
        fx.say(0, first);
        fx.env.fake(
            "node",
            &format!("env > '{root}/helper-env'\nread -r status json < '{root}/helper-answer'\necho \"$json\"\nexit \"$status\"", root = fx.env.root().display()),
        );
        fx
    }

    fn say(&self, exit: i32, json: &Value) {
        fs::write(
            self.env.root().join("helper-answer"),
            format!("{exit} {json}"),
        )
        .unwrap();
    }

    /// The row of `provider` as one value: its catalog, `polled_at`, `changes` and `last_error`.
    fn file(&self, provider: &str) -> Option<Value> {
        let (catalog, changes, polled, error): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = self.env.read(|c| {
            c.query_row(
                "SELECT catalog_json, changes_json, polled_at, last_error FROM swr_models WHERE provider = ?1",
                [provider],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok((None, None, None, None)),
                e => Err(e),
            })
        });
        let mut file: Value = serde_json::from_str(&catalog?).ok()?;
        file["polled_at"] = json!(polled);
        file["changes"] = changes.map_or(Value::Null, |c| serde_json::from_str(&c).unwrap());
        file["last_error"] = json!(error);
        Some(file)
    }

    /// A row of an earlier run, `polled_at` `ago` seconds ago.
    fn cache(&self, provider: &str, ago: i64, auth: &str, ids: &[&str], changes: Option<Value>) {
        let polled_at = (OffsetDateTime::now_utc() - time::Duration::seconds(ago))
            .format(&Rfc3339)
            .unwrap();
        let catalog = json!({
            "pi_version": "1.2.0",
            "source": {"kind": if auth == "custom" { "openai-models" } else { "copilot-available" }, "auth": auth, "base": if auth == "custom" { json!(BASE) } else { Value::Null }},
            "ids": ids
        });
        let changes = changes.map_or("NULL".to_owned(), |c| format!("'{c}'"));
        self.env.sql(&format!(
            "INSERT OR REPLACE INTO swr_models (provider, catalog_json, changes_json, polled_at) VALUES ('{provider}', '{catalog}', {changes}, '{polled_at}');"
        ));
    }

    fn cpd(&self, args: &[&str]) -> Run {
        self.env
            .cpd_with(self.env.root(), args, &[("BIFROST_TEST_KEY", KEY)])
    }
}

#[test]
fn refresh_runs_the_helper_now_writes_the_files_and_prints_provider_polled_at_count_and_changes() {
    let fx = Fx::new(&answer(vec![
        listed("bifrost", "custom", Some(BASE), &["m-b", "m-a", "m-c"]),
        json!({"provider": "github-copilot", "auth": "oauth", "base": null, "ids": ["gpt-5.6-sol", "claude-opus-5"], "known": ["gpt-6-astra", "gpt-5.6-sol", "claude-opus-5"]}),
    ]));
    // A minute old: `refresh` reads whatever the age.
    fx.cache("bifrost", 60, "custom", &["m-a", "m-b"], None);

    let run = fx.cpd(&["models", "refresh"]);
    let out = run.ok();
    assert_eq!(out["pi_version"], "1.3.0");
    let providers = out["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 2, "{out}");
    assert_eq!(providers[0]["provider"], "bifrost");
    assert_eq!(providers[0]["count"], 3);
    assert_eq!(providers[0]["changes"]["appeared"], json!(["m-c"]));
    assert_eq!(providers[0]["changes"]["went_away"], json!([]));
    assert!(
        providers[0]["changes"]["at"]
            .as_str()
            .unwrap()
            .ends_with('Z')
    );
    assert_eq!(providers[1]["provider"], "github-copilot");
    assert_eq!(providers[1]["count"], 2);
    assert_eq!(
        providers[1]["changes"],
        Value::Null,
        "a first run reports no change"
    );
    for p in providers {
        assert!(
            OffsetDateTime::parse(p["polled_at"].as_str().unwrap(), &Rfc3339).is_ok(),
            "{p}"
        );
    }

    let bifrost = fx.file("bifrost").unwrap();
    assert_eq!(bifrost["ids"], json!(["m-a", "m-b", "m-c"]));
    assert_eq!(bifrost["polled_at"], providers[0]["polled_at"]);
    assert_eq!(bifrost["changes"], providers[0]["changes"]);
    let copilot = fx.file("github-copilot").unwrap();
    assert_eq!(
        copilot["known"],
        json!(["claude-opus-5", "gpt-5.6-sol", "gpt-6-astra"])
    );
    assert_eq!(
        copilot["source"],
        json!({"kind": "copilot-available", "auth": "oauth", "base": null})
    );
    assert!(
        !fx.env.cpd_home().join("run/models-bifrost.json").exists(),
        "no file"
    );

    // The helper took no argument and the caller's environment; the key is in no argument, output or file.
    let script = fx.env.root().join("adapter/lib/models-list.ts");
    assert_eq!(
        fx.env.calls("node"),
        vec![vec![script.to_str().unwrap().to_owned()]]
    );
    assert!(
        fs::read_to_string(fx.env.root().join("helper-env"))
            .unwrap()
            .lines()
            .any(|l| l == format!("BIFROST_TEST_KEY={KEY}"))
    );
    assert!(!run.stdout.contains(KEY) && !run.stderr.contains(KEY));
    assert!(!fx.file("bifrost").unwrap().to_string().contains(KEY));
}

#[test]
fn refresh_writes_the_good_providers_then_fails_with_the_failing_ones_code() {
    let fx = Fx::new(&answer(vec![
        json!({"provider": "bifrost", "error": {"code": "models_unauthorized", "message": "401", "details": {"status": 401}}}),
        json!({"provider": "github-copilot", "auth": "oauth", "base": null, "ids": ["gpt-5.6-sol"]}),
    ]));
    fx.cache("bifrost", 3600, "custom", &["m-a"], None);
    let before = fx.file("bifrost").unwrap()["ids"].clone();

    let run = fx.cpd(&["models", "refresh"]);
    let error = refused(&run, "models_unauthorized", 1);
    assert_eq!(
        error["details"]["providers"].as_array().unwrap().len(),
        2,
        "{error}"
    );
    assert_eq!(
        fx.file("github-copilot").unwrap()["ids"],
        json!(["gpt-5.6-sol"]),
        "the good one is written"
    );
    let kept = fx.file("bifrost").unwrap();
    assert_eq!(kept["ids"], before, "the failing one keeps its row");
    assert!(
        kept["last_error"]
            .as_str()
            .unwrap()
            .starts_with("models_unauthorized: ")
    );
    assert!(!run.stdout.contains(KEY) && !run.stderr.contains(KEY));
}

#[test]
fn show_prints_the_cached_lists_with_their_age_and_runs_no_helper() {
    let fx = Fx::new(&answer(vec![listed(
        "bifrost",
        "custom",
        Some(BASE),
        &["never"],
    )]));
    let changes = json!({"at": "2026-11-02T09:00:00Z", "appeared": ["m-b"], "went_away": []});
    fx.cache(
        "github-copilot",
        7200,
        "oauth",
        &["gpt-5.6-sol", "claude-opus-5"],
        None,
    );
    fx.cache(
        "bifrost",
        3600,
        "custom",
        &["m-a", "m-b"],
        Some(changes.clone()),
    );

    let out = fx.cpd(&["models", "show"]).ok();
    let providers = out["providers"].as_array().unwrap();
    assert_eq!(
        providers
            .iter()
            .map(|p| p["provider"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["bifrost", "github-copilot"],
        "in name order"
    );
    let bifrost = &providers[0];
    assert_eq!(bifrost["pi_version"], "1.2.0");
    assert_eq!(bifrost["auth"], "custom");
    assert_eq!(bifrost["count"], 2);
    assert_eq!(bifrost["ids"], json!(["m-a", "m-b"]));
    assert_eq!(bifrost["changes"], changes);
    let seconds = bifrost["age_seconds"].as_i64().unwrap();
    assert!((3600..3700).contains(&seconds), "age_seconds {seconds}");
    assert!(OffsetDateTime::parse(bifrost["polled_at"].as_str().unwrap(), &Rfc3339).is_ok());
    assert_eq!(providers[1]["auth"], "oauth");
    assert_eq!(
        providers[1]["changes"],
        Value::Null,
        "no change recorded is null"
    );
    assert_eq!(out["warnings"], json!([]));

    assert_eq!(fx.env.calls("node").len(), 0, "show runs no helper");
}

#[test]
fn show_with_provider_prints_only_that_one() {
    let fx = Fx::new(&answer(vec![]));
    fx.cache("github-copilot", 100, "oauth", &["a"], None);
    fx.cache("bifrost", 100, "custom", &["m-a"], None);

    let out = fx.cpd(&["models", "show", "--provider", "bifrost"]).ok();
    let providers = out["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 1, "{out}");
    assert_eq!(providers[0]["provider"], "bifrost");

    let none = fx
        .cpd(&["models", "show", "--provider", "nothing-cached"])
        .ok();
    assert_eq!(
        none["providers"],
        json!([]),
        "a provider with no row is an empty result, not an error"
    );

    let run = fx.cpd(&["models", "show", "--provider", "../x"]);
    refused(&run, "usage", 2);
}

#[test]
fn show_with_no_row_says_so_with_an_empty_result_not_an_error() {
    let fx = Fx::new(&answer(vec![]));
    let out = fx.cpd(&["models", "show"]).ok();
    assert_eq!(out["providers"], json!([]));
    assert_eq!(out["warnings"], json!([]));
    assert_eq!(fx.env.calls("node").len(), 0);
}

#[test]
fn two_concurrent_refresh_runs_never_interleave_their_diffs() {
    let fx = Fx::new(&answer(vec![]));
    fx.cache("bifrost", 3600, "custom", &["a"], None);
    let root = fx.env.root().display().to_string();
    for fifo in ["f1", "f2"] {
        nix::unistd::mkfifo(
            &fx.env.root().join(fifo),
            nix::sys::stat::Mode::from_bits_truncate(0o600),
        )
        .unwrap();
    }
    // The two helpers meet before either answers (a rendezvous on two fifos), so both runs reach the lock together:
    // the first to start says [a, b], the other [a, c].
    let say = |ids: &[&str]| answer(vec![listed("bifrost", "custom", Some(BASE), ids)]).to_string();
    fx.env.fake(
        "node",
        &format!(
            "if mkdir '{root}/first' 2>/dev/null; then\n  echo go > '{root}/f1'\n  cat '{root}/f2' > /dev/null\n  echo '{x}'\nelse\n  cat '{root}/f1' > /dev/null\n  echo go > '{root}/f2'\n  echo '{y}'\nfi",
            x = say(&["a", "b"]),
            y = say(&["a", "c"])
        ),
    );

    let (one, two) = std::thread::scope(|s| {
        let one = s.spawn(|| fx.cpd(&["models", "refresh"]));
        let two = s.spawn(|| fx.cpd(&["models", "refresh"]));
        (one.join().unwrap(), two.join().unwrap())
    });
    one.ok();
    two.ok();

    // Whoever wrote last diffed against what the other had written, not against the row both started from.
    let file = fx.file("bifrost").unwrap();
    let (appeared, went_away) = if file["ids"] == json!(["a", "b"]) {
        (json!(["b"]), json!(["c"]))
    } else {
        (json!(["c"]), json!(["b"]))
    };
    assert_eq!(file["changes"]["appeared"], appeared, "{file}");
    assert_eq!(file["changes"]["went_away"], went_away, "{file}");
}
