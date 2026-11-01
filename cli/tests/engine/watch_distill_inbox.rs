//! No `distill-tier` Inbox item (hs-d555, superseding hs-d539): the distill ladder stops at H3, so a failed run opens
//! nothing, whatever `runs.jsonl` holds.

use crate::common;

use std::fs;

use common::fx::Fx;
use serde_json::{Value, json};

/// The engine in-process (`Fx::watch`, on the `ManualClock`) over a prepared store with Contexts `building` and `control`.
struct Distill {
    fx: Fx,
}

fn fx() -> Distill {
    let fx = Fx::with_ports();
    fx.engine_config("context_curation_min_records = 1");
    fx.engine_routes();
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('building', 'B', 't', 'm1');
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('control', 'C', 't', 'm1');",
    );
    Distill { fx }
}

/// One `runs.jsonl` line (contract §4.6) of a pi run on `tier` for records up to `upto_seq`.
fn line(context: &str, upto_seq: i64, tier: &str) -> String {
    json!({"started_at": "2026-10-20T10:00:00Z", "seconds": 1.5, "model": "m:low", "tier": tier, "tokens_in": 1, "tokens_out": 1,
           "cost": 0.0, "context": context, "upto_seq": upto_seq, "result": "invalid", "kind": "distill",
           "contradictions": [], "contradictions_dropped": 0})
    .to_string()
}

impl Distill {
    fn runs(&self, context: &str) -> std::path::PathBuf {
        self.fx
            .env
            .cpd_home()
            .join("run/context-digests")
            .join(context)
            .join("runs.jsonl")
    }

    /// Writes (replaces) the lines of `context`'s `runs.jsonl`.
    fn write_runs(&self, context: &str, lines: &[String]) {
        let path = self.runs(context);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
        )
        .unwrap();
    }

    /// The open `distill-tier` items of `cpd inbox list`, as JSON.
    fn items(&self) -> Vec<Value> {
        let json = self.fx.cpd(&["inbox", "list"]).ok();
        json["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["kind"] == "distill-tier")
            .cloned()
            .collect()
    }

    /// Two ticks of the engine, so that a missing item had its chance to open.
    fn two_ticks(&self) {
        self.fx.watch_ticks(2).unwrap();
    }

    fn errors(&self) -> i64 {
        self.fx
            .env
            .query::<i64>("SELECT count(*) FROM events WHERE kind = 'error'")
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
    fx.write_runs(
        "control",
        &runs("control", 3, &["H0", "H1", "H2", "H3", "S1"]),
    );
    fx.two_ticks();
    assert!(fx.items().is_empty(), "{:?}", fx.items());
    assert_eq!(fx.errors(), 0);
}
