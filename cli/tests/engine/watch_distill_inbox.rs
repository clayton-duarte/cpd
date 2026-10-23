//! No `distill-tier` Inbox item (hs-d555, superseding hs-d539): the distill ladder stops at H3, so a failed run opens
//! nothing, whatever `runs.jsonl` holds.

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
            "version = 1\nmachine = \"m1\"\ncontext_curation_min_records = 1\n[watch]\ntick_seconds = {}\n[tmux]\nsocket = \"{}\"\n",
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

        /// Two more ticks, so that a missing item had its chance to open.
    fn two_ticks(&self) {
        std::thread::sleep(Duration::from_secs_f64(common::TICK_SECONDS.parse::<f64>().unwrap() * 2.0 + 1.0));
    }

    fn errors(&self) -> i64 {
        self.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'error'")
    }
}

fn runs(context: &str, upto: i64, tiers: &[&str]) -> Vec<String> {
    tiers.iter().map(|t| line(context, upto, t)).collect()
}

#[test]
fn a_run_that_fails_every_step_opens_no_inbox_item() {
    // The ladder stops at H3 (hs-d555) and no distill-tier item exists: even a log with S steps opens nothing.
    let fx = fx();
    fx.write_runs("building", &runs("building", 7, &["H0", "H1", "H2", "H3"]));
    fx.write_runs("control", &runs("control", 3, &["H0", "H1", "H2", "H3", "S1"]));
    let _engine = fx.start();
    fx.two_ticks();
    assert!(fx.items().is_empty(), "{:?}", fx.items());
    assert_eq!(fx.errors(), 0);
}
