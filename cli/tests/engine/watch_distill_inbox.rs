//! Engine step E8, the `distill-tier` Inbox item (contract §2.2 "Inbox item", §3.7, §4.6 "The run log"; hs-d539):
//! with the engine running on a prepared store, a Context whose highest-`upto_seq` runs in `runs.jsonl` include
//! an `S` Tier opens one yellow item; a newer run replaces it; an unreadable file or line yields no item.
//! The "no item" tests carry a control Context that does reach `S`, so they cannot pass before E8 reads the files.

use crate::common;

use std::fs;
use std::process::{Child, Stdio};
use std::time::Duration;

use common::{CPD, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A running `cpd watch`, SIGTERMed on drop (killed if it does not stop).
struct Engine(Child);

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(self.0.id() as i32), nix::sys::signal::Signal::SIGTERM);
        common::poll(Duration::from_secs(10), || (!matches!(self.0.try_wait(), Ok(None))).then_some(()));
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Fx {
    env: TestEnv,
}

/// A prepared store with Contexts `building` and `control`, and a tick of `common::TICK_SECONDS`.
fn fx() -> Fx {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[watch]\ntick_seconds = {}\n[tmux]\nsocket = \"{}\"\n",
            common::TICK_SECONDS,
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    let fx = Fx { env };
    fx.store()
        .write("fixture", |tx| {
            exec(
                tx,
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('building', 'B', 't', 'm1');
                 INSERT INTO contexts (id, title, created_at, created_on) VALUES ('control', 'C', 't', 'm1');",
            )
        })
        .unwrap();
    fx
}

/// One `runs.jsonl` line (contract §4.6) of a pi run on `tier` for records up to `upto_seq`.
fn line(context: &str, upto_seq: i64, tier: &str) -> String {
    json!({"started_at": "2026-10-20T10:00:00Z", "seconds": 1.5, "model": "m:low", "tier": tier, "tokens_in": 1, "tokens_out": 1,
           "cost": 0.0, "context": context, "upto_seq": upto_seq, "result": "invalid", "kind": "distill",
           "contradictions": [], "contradictions_dropped": 0})
    .to_string()
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap(), 5000).unwrap()
    }

    fn runs(&self, context: &str) -> std::path::PathBuf {
        self.env.cpd_home().join("run/context-digests").join(context).join("runs.jsonl")
    }

    /// Writes (replaces) the lines of `context`'s `runs.jsonl`.
    fn write_runs(&self, context: &str, lines: &[String]) {
        let path = self.runs(context);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    }

    fn start(&self) -> Engine {
        let child = self.env.command(CPD).arg("watch").envs([common::PR_POLL_EVERY_TICK]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let engine = Engine(child);
        wait_until("watch-started", || self.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'watch-started'") == 1);
        engine
    }

    /// The open `distill-tier` items of `cpd inbox list`, as JSON.
    fn items(&self) -> Vec<Value> {
        let json = self.env.cpd(&["inbox", "list"]).ok();
        json["items"].as_array().unwrap().iter().filter(|i| i["kind"] == "distill-tier").cloned().collect()
    }

    fn items_of(&self, context: &str) -> Vec<Value> {
        self.items().into_iter().filter(|i| i["context"] == context).collect()
    }

    fn assert_item(&self, context: &str, upto: i64, tier: &str) {
        wait_until("distill-tier item", || self.items_of(context).iter().any(|i| i["subject"] == format!("distill:{context}/{upto}")));
        let items = self.items_of(context);
        assert_eq!(items.len(), 1, "{items:?}");
        let item = &items[0];
        assert_eq!(item["urgency"], "yellow", "{item}");
        assert_eq!(item["subject"], format!("distill:{context}/{upto}"), "{item}");
        assert_eq!(item["label"], format!("{context} digest needed {tier}"), "{item}");
        assert_eq!(item["context"], context, "{item}");
        for null in ["plan", "job", "session", "record"] {
            assert!(item[null].is_null(), "{null} must be null: {item}");
        }
    }

    /// Two more ticks, so that a missing item had its chance to open.
    fn two_ticks(&self) {
        std::thread::sleep(Duration::from_secs(common::TICK_SECONDS.parse::<u64>().unwrap() * 2 + 1));
    }

    fn errors(&self) -> i64 {
        self.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'error'")
    }
}

fn runs(context: &str, upto: i64, tiers: &[&str]) -> Vec<String> {
    tiers.iter().map(|t| line(context, upto, t)).collect()
}

#[test]
fn a_distill_run_that_reached_s_opens_a_yellow_distill_tier_item() {
    let fx = fx();
    fx.write_runs("building", &runs("building", 7, &["H0", "H1", "H2", "H3", "S1"]));
    let _engine = fx.start();
    fx.assert_item("building", 7, "S1");
}

#[test]
fn a_run_that_stayed_on_h_opens_no_item() {
    let fx = fx();
    fx.write_runs("building", &runs("building", 7, &["H0", "H1"]));
    fx.write_runs("control", &runs("control", 3, &["H0", "S1"]));
    let _engine = fx.start();
    fx.assert_item("control", 3, "S1");
    fx.two_ticks();
    assert!(fx.items_of("building").is_empty(), "{:?}", fx.items());
}

#[test]
fn a_newer_run_replaces_the_item() {
    let fx = fx();
    fx.write_runs("building", &runs("building", 7, &["H0", "S1"]));
    let _engine = fx.start();
    fx.assert_item("building", 7, "S1");

    // A newer run on H only: the old item closes and none opens.
    let mut lines = runs("building", 7, &["H0", "S1"]);
    lines.extend(runs("building", 9, &["H0", "H1"]));
    fx.write_runs("building", &lines);
    wait_until("old item closed", || fx.items_of("building").is_empty());
    fx.two_ticks();
    assert!(fx.items_of("building").is_empty(), "{:?}", fx.items());

    // A newer run that reached S2: a new item for it.
    lines.extend(runs("building", 11, &["H0", "S1", "S2"]));
    fx.write_runs("building", &lines);
    fx.assert_item("building", 11, "S2");
}

#[test]
fn an_unreadable_runs_file_opens_no_item_and_no_error() {
    let fx = fx();
    // `building`: a directory where the file should be; `control`: garbage lines around a valid S one.
    fs::create_dir_all(fx.runs("building")).unwrap();
    let mut lines = vec!["not json".to_owned(), "{\"tier\":".to_owned()];
    lines.extend(runs("control", 3, &["S1"]));
    lines.push("\u{1}\u{2}".to_owned());
    fx.write_runs("control", &lines);
    let _engine = fx.start();
    fx.assert_item("control", 3, "S1");
    fx.two_ticks();
    assert!(fx.items_of("building").is_empty(), "{:?}", fx.items());
    assert_eq!(fx.errors(), 0);
}
