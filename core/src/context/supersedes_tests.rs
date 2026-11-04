//! Multi-target supersession in the pure in-force fns and the record JSON (D1-D3, A1 of the Plan ctx-supersedes-multi).

use std::collections::HashSet;

use super::inforce::{in_force, in_force_without};
use super::render::Indexed;
use super::{By, Draft, Record};
use crate::ids::RecordId;
use serde_json::json;

fn id(s: &str) -> RecordId {
    RecordId::parse(s).unwrap()
}

fn record(s: &str, draft: Draft) -> Record {
    Record {
        id: id(s),
        at: "2026-10-18T05:53:00.000Z".into(),
        by: By {
            session: None,
            plan: None,
        },
        text: format!("text of {s}"),
        draft,
    }
}

fn decision(s: &str, supersedes: &[&str]) -> Record {
    record(
        s,
        Draft::Decision {
            supersedes: supersedes.iter().map(|t| id(t)).collect(),
        },
    )
}

/// Records numbered by position, from 1.
fn indexed(records: Vec<Record>) -> Vec<Indexed> {
    records
        .into_iter()
        .enumerate()
        .map(|(i, r)| (i as i64 + 1, r))
        .collect()
}

fn ids(found: &[&Indexed]) -> Vec<String> {
    found.iter().map(|(_, r)| r.id.to_string()).collect()
}

#[test]
fn a_record_superseding_two_records_retires_both() {
    let records = indexed(vec![
        decision("hs-d1", &[]),
        record("hs-n1", Draft::Note),
        decision("hs-d2", &["hs-d1", "hs-n1"]),
    ]);
    assert_eq!(ids(&in_force(&records, 3)), ["hs-d2"]);
    assert_eq!(
        ids(&in_force_without(&records, 3, &HashSet::new())),
        ["hs-d2"]
    );
}

#[test]
fn as_of_a_seq_a_later_multi_supersede_does_not_count() {
    let records = indexed(vec![
        decision("hs-d1", &[]),
        decision("hs-d2", &[]),
        decision("hs-d3", &["hs-d1", "hs-d2"]),
    ]);
    assert_eq!(ids(&in_force(&records, 2)), ["hs-d1", "hs-d2"]);
    assert_eq!(ids(&in_force(&records, 3)), ["hs-d3"]);
}

#[test]
fn a_record_with_several_supersedes_survives_its_line() {
    let machine = crate::ids::MachineName::parse("hs").unwrap();
    let line = r#"{"v":1,"id":"hs-d3","kind":"decision","at":"2026-10-14T21:12:00.000Z","machine":"hs","by":{"session":null,"plan":null},"text":"One engine.","supersedes":["hs-d1","hs-n2"]}"#;
    let record = Record::parse_line(line, &machine).unwrap();
    assert_eq!(record.supersedes(), [id("hs-d1"), id("hs-n2")]);
    let mut value = record.to_json();
    value["v"] = json!(1);
    let again = Record::parse_line(&value.to_string(), &machine).unwrap();
    assert_eq!(again, record);
}

#[test]
fn the_supersedes_json_is_an_array_and_absent_when_empty() {
    let machine = crate::ids::MachineName::parse("hs").unwrap();
    let line = r#"{"v":1,"id":"hs-d3","kind":"decision","at":"2026-10-14T21:12:00.000Z","machine":"hs","by":{"session":null,"plan":null},"text":"One engine.","supersedes":["hs-d1","hs-d2"]}"#;
    let two = Record::parse_line(line, &machine).unwrap().to_json();
    assert_eq!(two["supersedes"], json!(["hs-d1", "hs-d2"]));
    let one = decision("hs-d2", &["hs-d1"]).to_json();
    assert_eq!(one["supersedes"], json!(["hs-d1"]));
    let none = decision("hs-d1", &[]).to_json();
    assert!(none.get("supersedes").is_none(), "{none}");
}
