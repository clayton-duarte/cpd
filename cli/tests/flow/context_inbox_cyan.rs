//! The Inbox's notice tier is cyan, and blue is only for focus (contract §2.2 "Inbox item", commands.md §21; pro-d16,
//! which refines hs-d570), run as the built binary. The curation notices come from the engine's rule
//! (`rules::conditions`), as E8 reads them; the other tiers are fixture conditions.

use crate::common;

use std::fs;

use common::TestEnv;
use cpd_core::context::curation::{Curation, Proposal, Reason, propose};
use cpd_core::home::CpdHome;
use cpd_core::ids::ContextId;
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::inbox::rules;
use serde_json::Value;

const CTX: &str = "building-cpd";
const T0: &str = "2026-01-01T00:00:00.000Z";
const T1: &str = "2026-01-01T01:00:00.000Z";

/// A machine `hs` with Context `building-cpd`, a pending retirement.
fn fx() -> TestEnv {
    let env = TestEnv::new();
    fs::create_dir_all(env.cpd_home()).unwrap();
    fs::write(
        env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"hs\"\ncontext_curation_min_records = 1\n",
    )
    .unwrap();
    env.cpd(&[
        "context",
        "new",
        CTX,
        "--title",
        "Building cpd",
        "--owner",
        "acme",
    ])
    .ok();
    for text in ["Old fact.", "Same fact.", "Worth documenting."] {
        env.cpd(&["context", "note", text, "--context", CTX]).ok();
    }
    let curation = |record: &str| Curation {
        record: record.parse().unwrap(),
        topic: "engine".into(),
        short: format!("short of {record}"),
    };
    let curations = [curation("hs-n1"), curation("hs-n2"), curation("hs-n3")];
    let proposals = [Proposal {
        record: "hs-n1".parse().unwrap(),
        reason: Reason::Duplicate,
        target: "hs-n2".into(),
    }];
    propose(
        &mut env.store(),
        &ContextId::parse(CTX).unwrap(),
        &curations,
        &proposals,
    )
    .unwrap();
    env
}

fn home(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

/// What E8 reads for the store: the curation notices (and nothing else, in this fixture).
fn notices(env: &TestEnv) -> Vec<Condition> {
    let all = rules::conditions(&mut env.store(), &home(env)).unwrap();
    let kinds: Vec<Kind> = all.iter().map(|c| c.kind).collect();
    assert!(
        kinds.contains(&Kind::CurateRetire) && !kinds.contains(&Kind::CurateDocs),
        "{kinds:?}"
    );
    all
}

fn plain(kind: Kind, urgency: Urgency, subject: &str) -> Condition {
    Condition {
        kind,
        urgency,
        subject: subject.to_owned(),
        label: format!("{subject} label"),
        url: None,
        context: Some(ContextId::parse(CTX).unwrap()),
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
    }
}

fn listed(env: &TestEnv) -> Vec<Value> {
    let out = env.cpd(&["inbox", "list"]).ok();
    out["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] != "engine-down")
        .cloned()
        .collect()
}

#[test]
fn the_curation_notices_are_opened_with_urgency_cyan_and_no_inbox_output_prints_blue() {
    let env = fx();
    reconcile(&mut env.store(), &notices(&env), T0).unwrap();

    let items = listed(&env);
    let item = items
        .iter()
        .find(|i| i["kind"] == "curate-retire")
        .unwrap_or_else(|| panic!("curate-retire in {items:?}"));
    assert_eq!(item["urgency"], "cyan", "{item}");
    let stored: Vec<String> = env
        .store()
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT urgency FROM inbox_items WHERE kind = 'curate-retire' ORDER BY id",
            )?;
            Ok(stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()?)
        })
        .unwrap();
    assert_eq!(stored, ["cyan"], "the stored value");

    let list = env.cpd(&["inbox", "list"]).ok().to_string();
    assert!(list.contains("\"urgency\":\"cyan\""), "{list}");
    let id = items[0]["id"].as_i64().unwrap().to_string();
    let outputs = [
        list,
        env.cpd(&["inbox", "show", &id]).ok().to_string(),
        env.cpd(&["inbox", "list", "--all"]).ok().to_string(),
    ];
    for out in outputs {
        assert!(!out.contains("blue"), "no inbox output prints blue: {out}");
    }
}

#[test]
fn inbox_list_orders_red_yellow_magenta_cyan_white_and_the_oldest_first_within_a_tier() {
    let env = fx();
    let notices = notices(&env);
    let of = |kind: Kind| {
        notices
            .iter()
            .filter(|c| c.kind == kind)
            .cloned()
            .collect::<Vec<_>>()
    };
    let first = vec![
        plain(Kind::Unread, Urgency::White, "w-old"),
        plain(Kind::Dialog, Urgency::Magenta, "m"),
        plain(Kind::PrReady, Urgency::Yellow, "y"),
        plain(Kind::Dialog, Urgency::Red, "r"),
    ];
    reconcile(&mut env.store(), &first, T0).unwrap();
    let mut second = first.clone();
    second.extend(of(Kind::CurateRetire));
    second.push(plain(Kind::Unread, Urgency::White, "w-new"));
    reconcile(&mut env.store(), &second, T1).unwrap();

    let order: Vec<(String, String)> = listed(&env)
        .iter()
        .map(|i| {
            (
                i["urgency"].as_str().unwrap().to_owned(),
                i["subject"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let want: Vec<(&str, String)> = vec![
        ("red", "r".into()),
        ("yellow", "y".into()),
        ("magenta", "m".into()),
        ("cyan", format!("curate-retire:{CTX}")),
        ("white", "w-old".into()),
        ("white", "w-new".into()),
    ];
    assert_eq!(
        order,
        want.into_iter()
            .map(|(u, s)| (u.to_owned(), s))
            .collect::<Vec<_>>()
    );
}

#[test]
fn an_urgency_is_parsed_as_cyan_and_never_as_blue() {
    // The one place an urgency is read from text (the tiers are named by the palette's colours, hs-d534, pro-d16).
    assert_eq!(Urgency::parse("cyan").map(Urgency::as_str), Some("cyan"));
    assert_eq!(
        Urgency::parse("blue").map(Urgency::as_str),
        Some("blue"),
        "blue is a tier now (review-requested)"
    );
    assert_eq!(Urgency::parse("teal"), None, "an unknown value");
    for tier in ["white", "magenta", "yellow", "red"] {
        assert_eq!(Urgency::parse(tier).map(Urgency::as_str), Some(tier));
    }
    let cyan = Urgency::parse("cyan").expect("cyan is a tier");
    assert!(
        Urgency::White < cyan && cyan < Urgency::Magenta,
        "the same place in the order"
    );
}
