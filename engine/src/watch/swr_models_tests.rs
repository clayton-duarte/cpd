//! E15 on the models datasource (docs/architecture/swr-models.md §2-§4): the helper `node` is a `FakeRunner` route, the
//! clock a `ManualClock`, the cache the `swr_models` table. No process, no sleep: the detached run is pumped by ticking
//! until a state the test reads holds. `pi` is a stub file on PATH (each nextest test is its own process).

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::models;
use super::testkit::Fx;
use cpd_core::clock::{Clock, timestamp};
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ports::Fakes;
use cpd_core::swr::endpoint;

const WEEK: u64 = 604_800;

/// The helper: answers `says` to every call.
struct Node(Mutex<String>);

impl Handler for Node {
    fn handle(&self, _call: &Call) -> Reply {
        Reply::exit(0, &self.0.lock().unwrap(), "")
    }
}

struct Ctx {
    fx: Fx,
    fakes: Fakes,
    node: Arc<Node>,
    _dir: tempfile::TempDir,
}

fn install_pi(root: &Path) {
    let bin = root.join("agent/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let pi = bin.join("pi");
    std::fs::write(&pi, "").unwrap();
    std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o755)).unwrap();
    let install = root.join("agent/install");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(install.join("current-version"), "1.2.0\n").unwrap();
    for package in ["pi-coding-agent", "pi-ai"] {
        let dist = install
            .join("releases/1.2.0/node_modules/@earendil-works")
            .join(package)
            .join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.js"), "").unwrap();
    }
    let path = std::env::var("PATH").unwrap_or_default();
    // SAFETY: every test runs in its own process under nextest; no other thread reads the environment yet.
    unsafe { std::env::set_var("PATH", format!("{}:{path}", bin.display())) };
}

fn ctx() -> Ctx {
    let dir = tempfile::tempdir().unwrap();
    install_pi(dir.path());
    let adapter = dir.path().join("adapter");
    std::fs::create_dir_all(&adapter).unwrap();
    std::fs::write(adapter.join("package.json"), "{}").unwrap();
    let mut fx = Fx::new(&format!("pi_adapter = \"{}\"", adapter.display()));
    let fakes = Fakes::new();
    fx.engine.ports = fakes.ports.clone();
    let node = Arc::new(Node(Mutex::new(String::new())));
    let _ = (*fakes.runner).clone().route("node", node.clone());
    Ctx {
        fx,
        fakes,
        node,
        _dir: dir,
    }
}

fn listed(provider: &str, auth: &str, base: Option<&str>, ids: &[&str]) -> Value {
    json!({"provider": provider, "auth": auth, "base": base, "ids": ids})
}

fn failed(provider: &str, code: &str) -> Value {
    json!({"provider": provider, "error": {"code": code, "message": format!("{provider}: {code}"), "details": {}}})
}

impl Ctx {
    fn say(&self, providers: Vec<Value>) {
        *self.node.0.lock().unwrap() =
            json!({"ok": true, "pi_version": "1.3.0", "providers": providers}).to_string();
    }

    fn now(&self) -> time::OffsetDateTime {
        self.fakes.clock.utc()
    }

    fn at(&self, ago: u64) -> String {
        timestamp(self.now() - time::Duration::seconds(ago as i64))
    }

    /// A row of an earlier run, polled `ago` seconds before now.
    fn seed(&mut self, provider: &str, ago: u64, ids: &[&str]) {
        let catalog = json!({"pi_version": "1.2.0", "source": {"kind": "copilot-available", "auth": "oauth", "base": null}, "ids": ids});
        let polled = self.at(ago);
        self.fx.fixture(&format!(
            "INSERT INTO swr_models (provider, catalog_json, polled_at) VALUES ('{provider}', '{catalog}', '{polled}')"
        ));
    }

    fn calls(&self) -> usize {
        self.fakes
            .runner
            .calls()
            .iter()
            .filter(|c| c.program.ends_with("node"))
            .count()
    }

    fn text(&mut self, sql: &str) -> Option<String> {
        self.fx
            .engine
            .store
            .read(|c| {
                let mut stmt = c.prepare(sql)?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, Option<String>>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows.into_iter().next().flatten())
            })
            .unwrap()
    }

    fn int(&mut self, sql: &str) -> i64 {
        self.fx
            .engine
            .store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    fn txns(&mut self) -> i64 {
        self.int("SELECT count(*) FROM txns")
    }

    fn skip(&self, secs: u64) {
        self.fakes.clock.advance(Duration::from_secs(secs));
    }

    /// Ticks E15 a bounded number of times (the run is detached, so its answer arrives at a later tick).
    fn ticks(&mut self, n: usize) {
        for _ in 0..n {
            models::run(&mut self.fx.engine);
            std::thread::yield_now();
        }
    }

    /// Ticks until `done` holds; panics when it never does.
    fn until(&mut self, what: &str, done: impl Fn(&mut Ctx) -> bool) {
        for _ in 0..20_000 {
            models::run(&mut self.fx.engine);
            if done(self) {
                return;
            }
            std::thread::yield_now();
        }
        panic!("never held: {what}");
    }

    fn polled(&mut self, provider: &str) -> Option<String> {
        self.text(&format!(
            "SELECT polled_at FROM swr_models WHERE provider = '{provider}'"
        ))
    }

    fn errors(&mut self, code: &str) -> i64 {
        self.int(&format!(
            "SELECT count(*) FROM events WHERE kind = 'error' AND data LIKE '%{code}%'"
        ))
    }
}

#[test]
fn models_due_when_no_row_or_newest_poll_is_a_week_old() {
    // No row: due at once.
    let mut c = ctx();
    c.say(vec![listed("p", "oauth", None, &["a"])]);
    c.until("a row after the first run", |c| c.polled("p").is_some());
    assert_eq!(c.calls(), 1);

    // A row one second short of a week: not due. At a week: due.
    let mut c = ctx();
    c.say(vec![listed("p", "oauth", None, &["a"])]);
    c.seed("p", WEEK - 1, &["a"]);
    c.ticks(300);
    assert_eq!(c.calls(), 0, "not due before the week is over");
    c.skip(1);
    c.until("the run at a week", |c| c.calls() == 1);
}

#[test]
fn models_run_end_gates_the_next_poll_for_600_seconds() {
    let mut c = ctx();
    c.seed("p", WEEK + 1, &["a"]);
    c.say(vec![failed("p", "models_unreachable")]);
    c.until("the failed run is applied", |c| {
        c.text("SELECT last_error FROM swr_models WHERE provider = 'p'")
            .is_some()
    });
    assert_eq!(c.calls(), 1);

    c.skip(599);
    c.ticks(300);
    assert_eq!(c.calls(), 1, "no run before 600 s after the run's end");
    c.skip(1);
    c.until("the next run at 600 s", |c| c.calls() == 2);
}

#[test]
fn models_failed_provider_keeps_its_row() {
    let mut c = ctx();
    c.seed("github-copilot", WEEK + 1, &["a", "b"]);
    let before = c.text("SELECT catalog_json FROM swr_models").unwrap();
    let polled = c.polled("github-copilot");
    c.say(vec![failed("github-copilot", "models_unreachable")]);
    c.until("last_error is set", |c| {
        c.text("SELECT last_error FROM swr_models").is_some()
    });

    assert!(
        c.text("SELECT last_error FROM swr_models")
            .unwrap()
            .starts_with("models_unreachable: ")
    );
    assert_eq!(
        c.text("SELECT catalog_json FROM swr_models").unwrap(),
        before
    );
    assert_eq!(c.polled("github-copilot"), polled, "polled_at is kept");
}

#[test]
fn models_one_healthy_provider_resets_the_week() {
    let mut c = ctx();
    c.seed("p1", WEEK + 1, &["a"]);
    c.seed("p2", WEEK + 1, &["x"]);
    let old = c.polled("p2");
    c.say(vec![
        listed("p1", "oauth", None, &["a"]),
        failed("p2", "models_unreachable"),
    ]);
    let now = timestamp(c.now());
    c.until("p1 is polled now", |c| c.polled("p1") == Some(now.clone()));
    assert_eq!(c.polled("p2"), old, "a failed provider resets nothing");

    c.skip(WEEK - 1);
    c.ticks(300);
    assert_eq!(c.calls(), 1, "p1's poll holds the week for all");
    c.skip(1);
    c.until("the run a week after p1's poll", |c| c.calls() == 2);
}

// The name is historical: it pins "no catalog write". The polled_at-only txn is the documented D5 exception
// (swr-models.md §3.2).
#[test]
fn models_unchanged_catalog_opens_no_write_txn() {
    let mut c = ctx();
    c.seed("p", WEEK + 1, &["a", "b"]);
    c.fx.fixture(&format!(
        "UPDATE swr_models SET changes_json = '{}' WHERE provider = 'p'",
        json!({"at": c.at(WEEK * 2), "appeared": ["a"], "went_away": []})
    ));
    let catalog = c.text("SELECT catalog_json FROM swr_models");
    let changes = c.text("SELECT changes_json FROM swr_models");
    let before = c.txns();
    c.say(vec![listed("p", "oauth", None, &["b", "a"])]);
    let now = timestamp(c.now());
    c.until("polled_at moves", |c| c.polled("p") == Some(now.clone()));

    assert_eq!(c.txns(), before + 1, "one small write txn, polled_at only");
    assert_eq!(c.text("SELECT catalog_json FROM swr_models"), catalog);
    assert_eq!(c.text("SELECT changes_json FROM swr_models"), changes);
    assert_eq!(c.text("SELECT last_error FROM swr_models"), None);

    // A tick with no provider run opens no write txn.
    c.skip(601);
    c.ticks(300);
    assert_eq!(c.calls(), 1);
    assert_eq!(c.txns(), before + 1);
}

#[test]
fn models_provider_failure_feeds_the_endpoint_health() {
    let mut c = ctx();
    c.say(vec![
        failed("github-copilot", "models_unauthorized"),
        listed("bifrost", "custom", Some("http://127.0.0.1:8788"), &["m"]),
    ]);
    let now = c.now();
    c.until("the copilot endpoint records the failure", |c| {
        endpoint::get(&mut c.fx.engine.store, "github-copilot")
            .unwrap()
            .is_some()
    });
    let copilot = endpoint::get(&mut c.fx.engine.store, "github-copilot")
        .unwrap()
        .unwrap();
    assert_eq!(copilot.failing_since, Some(now));
    assert_eq!(copilot.error_count, 1);
    assert!(copilot.last_error.unwrap().contains("models_unauthorized"));
    let custom = endpoint::get(&mut c.fx.engine.store, "127.0.0.1:8788")
        .unwrap()
        .expect("a custom provider's endpoint is its host:port");
    assert_eq!(custom.last_ok_at, Some(now));
    assert_eq!(custom.failing_since, None);

    // A permanent failure is reported at once, once per outage.
    assert_eq!(c.errors("models_unauthorized"), 1);
    c.skip(601);
    c.until("the second run", |c| c.calls() == 2);
    c.ticks(300);
    assert_eq!(c.errors("models_unauthorized"), 1);
}
