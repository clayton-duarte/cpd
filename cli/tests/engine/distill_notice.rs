//! Engine step E8's `distill-tier` item as a magenta counting notice, and `tier_notice` (hs-d546, hs-d547).
//! Driven as cli/tests/pr/pr_merge.rs does: `conditions` + `reconcile` directly, no engine.

use crate::common;

use std::fs;

use common::TestEnv;
use cpd_core::ids::ContextId;
use cpd_core::home::CpdHome;
use cpd_core::inbox::model::reconcile;
use cpd_core::inbox::rules::{conditions, tier_notice};
use cpd_core::store::Store;
use serde_json::json;

const CTX: &str = "building";

struct Fx {
    env: TestEnv,
    home: CpdHome,
}

fn fx() -> Fx {
    let env = TestEnv::new();
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    env.sql(&format!("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('{CTX}', 'B', 't', 't')"));
    Fx { env, home }
}

impl Fx {
    /// Appends runs.jsonl lines: (upto_seq, tier, started_at).
    fn runs(&self, lines: &[(i64, &str, &str)]) {
        let path = self.env.cpd_home().join("run/context-digests").join(CTX).join("runs.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = fs::read_to_string(&path).unwrap_or_default();
        let new: String = lines
            .iter()
            .map(|(u, t, at)| {
                format!(
                    "{}\n",
                    json!({"started_at": at, "seconds": 1.5, "model": "m:low", "tier": t, "tokens_in": 1, "tokens_out": 1, "cost": 0.0,
                           "context": CTX, "upto_seq": u, "result": "invalid", "kind": "distill", "contradictions": [], "contradictions_dropped": 0})
                )
            })
            .collect();
        fs::write(path, old + &new).unwrap();
    }

    fn reconcile(&self, now: &str) {
        let mut store = Store::open(&self.home, 5000).unwrap();
        let found = conditions(&mut store, &self.home).unwrap();
        reconcile(&mut store, &found, now).unwrap();
    }

    /// Open distill-tier items: (urgency, subject, label).
    fn open(&self) -> Vec<(String, String, String)> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT urgency, subject, label FROM inbox_items WHERE kind = 'distill-tier' AND closed_at IS NULL ORDER BY id")?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()
        })
    }
}

fn open(urgency: &str, subject: &str, label: &str) -> (String, String, String) {
    (urgency.to_owned(), subject.to_owned(), label.to_owned())
}

#[test]
fn a_distill_run_that_reached_s_opens_a_magenta_item_counting_one_run() {
    let fx = fx();
    fx.runs(&[(7, "H0", "2026-10-22T10:00:00Z"), (7, "S2", "2026-10-22T10:00:00Z")]);
    fx.reconcile("2026-10-22T10:01:00.000Z");
    assert_eq!(fx.open(), vec![open("magenta", "distill:building/7", "building digest needed S2 ×1")]);
}

#[test]
fn a_newer_run_that_reached_s_replaces_the_item_counting_two() {
    let fx = fx();
    fx.runs(&[(7, "S1", "2026-10-22T10:00:00Z")]);
    fx.reconcile("2026-10-22T10:01:00.000Z");
    fx.runs(&[(9, "H0", "2026-10-22T11:00:00Z"), (9, "S1", "2026-10-22T11:00:00Z")]);
    fx.reconcile("2026-10-22T11:01:00.000Z");
    assert_eq!(fx.open(), vec![open("magenta", "distill:building/9", "building digest needed S1 ×2")]);
}

#[test]
fn a_dismissal_closes_the_item_and_the_next_s_run_counts_one() {
    let fx = fx();
    fx.runs(&[(7, "S1", "2026-10-22T10:00:00Z")]);
    fx.reconcile("2026-10-22T10:01:00.000Z");
    let id = fx.env.query::<i64>("SELECT id FROM inbox_items WHERE kind = 'distill-tier' AND closed_at IS NULL").to_string();
    fx.env.cpd(&["inbox", "dismiss", &id]).ok();
    fx.reconcile("2999-01-01T00:00:00.000Z");
    assert_eq!(fx.open(), vec![]);
    fx.runs(&[(9, "S3", "2999-06-01T00:00:00Z")]);
    fx.reconcile("2999-06-01T00:01:00.000Z");
    assert_eq!(fx.open(), vec![open("magenta", "distill:building/9", "building digest needed S3 ×1")]);
}

#[test]
fn an_open_yellow_item_closes_and_a_magenta_one_opens() {
    let fx = fx();
    fx.runs(&[(7, "S1", "2026-10-22T10:00:00Z")]);
    fx.env.sql(
        "INSERT INTO inbox_items (kind, urgency, subject, context, label, opened_at)
         VALUES ('distill-tier', 'yellow', 'distill:building/7', 'building', 'building digest needed S1', '2026-10-22T10:00:30.000Z')",
    );
    fx.reconcile("2026-10-22T10:01:00.000Z");
    assert_eq!(fx.open(), vec![open("magenta", "distill:building/7", "building digest needed S1 ×1")]);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM inbox_items WHERE urgency = 'yellow' AND closed_at IS NOT NULL"), 1);
}

#[test]
fn tier_notice_lists_the_steps_of_the_highest_run_in_order() {
    let fx = fx();
    let at = "2026-10-22T10:00:00Z";
    fx.runs(&[(5, "S1", at), (9, "H0", at), (9, "H1", at), (9, "H1", at), (9, "S1", at), (9, "S2", at)]);
    let ctx = ContextId::parse(CTX).unwrap();
    let notice = Store::open(&fx.home, 5000).unwrap().read(|conn| tier_notice(conn, &fx.home, &ctx)).unwrap().unwrap();
    assert_eq!(notice.upto, 9);
    assert_eq!(notice.top, "S2");
    assert_eq!(notice.steps, ["H0", "H1", "S1", "S2"]);
    assert_eq!(notice.runs, 2);
}
