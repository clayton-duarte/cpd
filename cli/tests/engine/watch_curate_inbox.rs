//! Engine step E8, the `curate-retire` Inbox item (contract §2.2 "Inbox item", §3.7; hs-d549,
//! hs-d551): one white item per Context, its count following the pending rows, closed when none is.

use crate::common;

use std::fs;

use common::fx::Fx;
use cpd_core::context::curation::{Approval, Curation, Proposal, Reason, propose};
use cpd_core::context::{By, Draft, Record};
use cpd_core::ids::{ContextId, RecordId};
use serde_json::Value;

/// The engine in-process (`Fx::watch`, on the `ManualClock`) over a prepared store with Contexts `building` and `control`.
/// The engine ticks when a test says so ([`Fx::watch_ticks`]): what it opens it opens at that tick, no wait.
struct Curate {
    fx: Fx,
}

fn fx() -> Curate {
    let fx = Fx::with_ports();
    fx.engine_config("context_curation_min_records = 1");
    fx.engine_routes();
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('building', 'B', 't', 'm1');
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('control', 'C', 't', 'm1');",
    );
    Curate { fx }
}

impl Curate {
    /// The engine starts, ticks once and stops.
    fn tick(&self) {
        self.fx.watch_ticks(1).unwrap();
    }

    /// Notes `<prefix>1 .. <prefix>n` (ids `hs-n<k>`) in `context`.
    fn records(&self, context: &str, ids: &[&str]) {
        let context = ContextId::parse(context).unwrap();
        self.fx
            .env
            .store()
            .write("fixture", |tx| {
                for id in ids {
                    let record = Record {
                        id: RecordId::parse(id).unwrap(),
                        at: "2026-10-20T10:00:00.000Z".into(),
                        by: By {
                            session: None,
                            plan: None,
                        },
                        text: format!("text of {id}"),
                        draft: Draft::Note,
                    };
                    record.insert(tx, &context).unwrap();
                }
                Ok(())
            })
            .unwrap();
    }

    /// Curates `ids` of `context`; proposes the retirement of `retire` (all duplicates of the last of `ids`).
    fn propose(&self, context: &str, ids: &[&str], retire: &[&str]) {
        let curations: Vec<Curation> = ids
            .iter()
            .map(|id| Curation {
                record: RecordId::parse(id).unwrap(),
                topic: "engine".into(),
                short: format!("short of {id}"),
            })
            .collect();
        let proposals: Vec<Proposal> = retire
            .iter()
            .map(|id| Proposal {
                record: RecordId::parse(id).unwrap(),
                reason: Reason::Duplicate,
                target: ids.last().unwrap().to_string(),
                approval: Approval::Pending,
            })
            .collect();
        propose(
            &mut self.fx.env.store(),
            &ContextId::parse(context).unwrap(),
            &curations,
            &proposals,
        )
        .unwrap();
    }

    /// The open items of `kind` of `cpd inbox list`, as JSON.
    fn items(&self, kind: &str) -> Vec<Value> {
        let json = self.fx.cpd(&["inbox", "list"]).ok();
        json["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["kind"] == kind)
            .cloned()
            .collect()
    }

    fn assert_item(&self, kind: &str, context: &str, label: &str) {
        let items = self.items(kind);
        assert!(
            items
                .iter()
                .any(|i| i["context"] == context && i["label"] == label),
            "no {kind} item `{label}` of {context}: {items:?}"
        );
        let item = items.into_iter().find(|i| i["context"] == context).unwrap();
        assert_eq!(item["urgency"], "white", "{item}");
        assert_eq!(item["subject"], format!("{kind}:{context}"), "{item}");
        for null in ["plan", "job", "session", "record"] {
            assert!(item[null].is_null(), "{null} must be null: {item}");
        }
    }
}

#[test]
fn e8_opens_one_curate_retire_item_per_context_with_its_count() {
    let fx = fx();
    fx.records("building", &["hs-n1", "hs-n2", "hs-n3", "hs-n9"]);
    fx.records("control", &["hs-n4", "hs-n5"]);
    fx.propose(
        "building",
        &["hs-n1", "hs-n2", "hs-n3", "hs-n9"],
        &["hs-n1", "hs-n2"],
    );
    fx.propose("control", &["hs-n4", "hs-n5"], &["hs-n4"]);
    fx.tick();
    fx.assert_item("curate-retire", "building", "building: 2 to retire");
    fx.assert_item("curate-retire", "control", "control: 1 to retire");
    assert_eq!(
        fx.items("curate-retire").len(),
        2,
        "{:?}",
        fx.items("curate-retire")
    );

    // the count follows the pending rows, and the item closes when none is
    fx.fx
        .cpd(&[
            "context",
            "retire",
            "--context",
            "building",
            "--approve",
            "hs-n1",
        ])
        .ok();
    fx.tick();
    fx.assert_item("curate-retire", "building", "building: 1 to retire");
    fx.fx
        .cpd(&[
            "context",
            "retire",
            "--context",
            "building",
            "--reject",
            "hs-n2",
        ])
        .ok();
    fx.tick();
    assert!(
        fx.items("curate-retire")
            .iter()
            .all(|i| i["context"] != "building"),
        "building is closed: {:?}",
        fx.items("curate-retire")
    );
    assert_eq!(fx.items("curate-retire").len(), 1);
}

#[test]
fn e8_opens_the_curate_items_white() {
    let fx = fx();
    fx.records("building", &["hs-n1", "hs-n2", "hs-n3"]);
    fx.propose("building", &["hs-n1", "hs-n2", "hs-n3"], &["hs-n1"]);
    fx.tick();
    fx.assert_item("curate-retire", "building", "building: 1 to retire");
    assert!(
        fx.items("curate-retire")
            .iter()
            .all(|i| i["urgency"] == "white"),
        "{:?}",
        fx.items("curate-retire")
    );
}

impl Curate {
    /// Rewrites `context_curation_min_records` in config.toml, every other line kept.
    fn set_floor(&self, floor: u32) {
        let path = self.fx.env.cpd_home().join("config.toml");
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<String> = text
            .lines()
            .map(|l| {
                if l.starts_with("context_curation_min_records") {
                    format!("context_curation_min_records = {floor}")
                } else {
                    l.to_owned()
                }
            })
            .collect();
        fs::write(&path, lines.join("\n") + "\n").unwrap();
    }

    fn pending(&self, context: &str) -> i64 {
        self.fx.env.query(&format!("SELECT count(*) FROM record_retirements WHERE context = '{context}' AND status = 'pending'"))
    }
}

/// hs-d592 R6: an archived Context has no curate item (derived from its state: unarchiving reopens them), its rows untouched.
#[test]
fn e8_opens_no_curate_item_for_an_archived_context_and_reopens_it_unarchived() {
    let fx = fx();
    fx.records("building", &["hs-n1", "hs-n2", "hs-n3", "hs-n4"]);
    fx.propose(
        "building",
        &["hs-n1", "hs-n2", "hs-n3", "hs-n4"],
        &["hs-n1", "hs-n2"],
    );
    fx.tick();
    fx.assert_item("curate-retire", "building", "building: 2 to retire");
    let before = fx.pending("building");

    fx.fx
        .env
        .sql("UPDATE contexts SET archived_at = '2026-10-20T10:00:00.000Z' WHERE id = 'building'");
    fx.tick();
    assert!(
        fx.items("curate-retire").is_empty(),
        "the item closed: {:?}",
        fx.items("curate-retire")
    );
    assert_eq!(fx.pending("building"), before, "the rows stay pending");
    assert_eq!(before, 2);

    fx.fx
        .env
        .sql("UPDATE contexts SET archived_at = NULL WHERE id = 'building'");
    fx.tick();
    fx.assert_item("curate-retire", "building", "building: 2 to retire");
    assert_eq!(fx.pending("building"), before);
}

/// hs-d592 R2, R5, R6: a Context under `context_curation_min_records` has no curate item, the floor read at every tick.
#[test]
fn e8_opens_no_curate_item_for_a_context_under_the_floor() {
    let fx = fx();
    fx.records("building", &["hs-n1", "hs-n2", "hs-n3"]);
    fx.propose(
        "building",
        &["hs-n1", "hs-n2", "hs-n3"],
        &["hs-n1", "hs-n2"],
    );
    fx.records(
        "control",
        &["hs-n4", "hs-n5", "hs-n6", "hs-n7", "hs-n8", "hs-n9"],
    );
    fx.propose(
        "control",
        &["hs-n4", "hs-n5", "hs-n6", "hs-n7", "hs-n8", "hs-n9"],
        &["hs-n4"],
    );
    fx.tick();
    fx.assert_item("curate-retire", "building", "building: 2 to retire");
    fx.assert_item("curate-retire", "control", "control: 1 to retire");
    let before = (fx.pending("building"), fx.pending("control"));

    fx.set_floor(5);
    fx.tick();
    assert!(
        fx.items("curate-retire")
            .iter()
            .all(|i| i["context"] != "building"),
        "building's item closed: {:?}",
        fx.items("curate-retire")
    );
    assert_eq!(
        (fx.pending("building"), fx.pending("control")),
        before,
        "the rows stay pending"
    );
    assert_eq!(
        fx.items("curate-retire").len(),
        1,
        "control keeps its items"
    );

    fx.set_floor(3);
    fx.tick();
    fx.assert_item("curate-retire", "building", "building: 2 to retire");
    fx.assert_item("curate-retire", "control", "control: 1 to retire");
    assert_eq!((fx.pending("building"), fx.pending("control")), before);
}
