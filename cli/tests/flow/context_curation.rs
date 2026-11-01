//! Record retirement (contract §4.6 "Retirement", hs-d549, hs-d551; commands.md §3.15 to §3.17), run in this process (`Fx`).
//! Proposals are written through `curation::propose`, as the next PR's distiller will.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::refused;
use cpd_core::context::curation::{Curation, Proposal, Reason, propose};
use cpd_core::ids::ContextId;
use serde_json::{Value, json};

const CTX: &str = "building-cpd";

/// A machine `hs` with Context `building-cpd` holding notes hs-n1 to hs-n4 and the decision hs-d1.
fn fx() -> Fx {
    let fx = Fx::new();
    fs::create_dir_all(fx.env.cpd_home()).unwrap();
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"hs\"\n",
    )
    .unwrap();
    fx.cpd(&[
        "context",
        "new",
        CTX,
        "--title",
        "Building cpd",
        "--owner",
        "acme",
    ])
    .ok();
    for text in ["Old fact.", "Same fact.", "Documented fact.", "Kept fact."] {
        fx.cpd(&["context", "note", text, "--context", CTX]).ok();
    }
    fx.cpd(&["context", "decide", "Worth documenting.", "--context", CTX])
        .ok();
    fx
}

fn curation(record: &str, short: &str, doc_paths: Option<&[&str]>) -> Curation {
    Curation {
        record: record.parse().unwrap(),
        topic: "engine".into(),
        short: short.into(),
        doc_paths: doc_paths.map(|p| p.iter().map(|s| (*s).to_owned()).collect()),
    }
}

fn proposal(record: &str, reason: Reason, target: &str) -> Proposal {
    Proposal {
        record: record.parse().unwrap(),
        reason,
        target: target.into(),
    }
}

/// Curates every record; proposes hs-n1 (duplicate of hs-n2), hs-n3 (documented in docs/a.md) and hs-n4
/// (superseded by hs-d1) for retirement; flags hs-d1 as a doc candidate.
fn proposed(fx: &Fx) {
    let curations = [
        curation("hs-n1", "old fact", None),
        curation("hs-n2", "same fact", None),
        curation("hs-n3", "documented fact", None),
        curation("hs-n4", "kept fact", None),
        curation(
            "hs-d1",
            "worth documenting",
            Some(&["docs/a.md", "docs/b.md"]),
        ),
    ];
    let proposals = [
        proposal("hs-n1", Reason::Duplicate, "hs-n2"),
        proposal("hs-n3", Reason::Documented, "docs/a.md"),
        proposal("hs-n4", Reason::Superseded, "hs-d1"),
    ];
    propose(
        &mut fx.env.store(),
        &ContextId::parse(CTX).unwrap(),
        &curations,
        &proposals,
    )
    .unwrap();
}

fn retire(fx: &Fx, flag: &str, ids: &str) -> common::Run {
    fx.cpd(&["context", "retire", "--context", CTX, flag, ids])
}

fn shown(fx: &Fx) -> Value {
    fx.cpd(&["context", "show", "--context", CTX]).ok()
}

fn item_line(md: &str, id: &str) -> String {
    md.lines()
        .find(|l| l.starts_with(&format!("- {id} (")))
        .unwrap_or_else(|| panic!("{id} in {md}"))
        .to_owned()
}

fn events(fx: &Fx, kind: &str) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", kind]).ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

/// The ids of the records in force, from the first message `context show` prints is not available: the digest's
/// own in-force count is `context distill`'s; here the markdown's active sections stand for it.
fn active_notes(md: &str) -> Vec<String> {
    let section = md.split("## Notes").nth(1).unwrap_or("");
    section
        .lines()
        .filter(|l| {
            l.starts_with("- hs-n") && !l.contains("superseded by") && !l.contains("retired (")
        })
        .map(|l| l[2..].split(' ').next().unwrap().to_owned())
        .collect()
}

#[test]
fn an_approved_retirement_takes_the_record_out_of_force() {
    let fx = fx();
    proposed(&fx);
    assert_eq!(
        active_notes(shown(&fx)["markdown"].as_str().unwrap()),
        ["hs-n1", "hs-n2", "hs-n3", "hs-n4"],
        "a proposal changes nothing"
    );

    let out = retire(&fx, "--approve", "hs-n1,hs-n3").ok();
    assert_eq!(
        (&out["approved"], &out["rejected"]),
        (&json!(["hs-n1", "hs-n3"]), &json!([])),
        "{out}"
    );

    assert_eq!(
        active_notes(shown(&fx)["markdown"].as_str().unwrap()),
        ["hs-n2", "hs-n4"]
    );
    // the row and its history stay; the retired record is still readable and no Context record was written
    let one = fx
        .cpd(&["context", "show", "--record", "hs-n1", "--context", CTX])
        .ok();
    assert_eq!(one["records"][0]["text"], "Old fact.", "{one}");
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM context_records"),
        5
    );
    assert_eq!(
        fx.env.query::<String>(
            "SELECT status || '/' || decided_by FROM record_retirements WHERE record = 'hs-n1'"
        ),
        "approved/user"
    );
}

#[test]
fn a_rejected_retirement_keeps_the_record_in_force_and_is_not_proposed_again() {
    let fx = fx();
    proposed(&fx);
    retire(&fx, "--reject", "hs-n1").ok();

    assert!(active_notes(shown(&fx)["markdown"].as_str().unwrap()).contains(&"hs-n1".to_owned()));
    let listed = fx.cpd(&["context", "curation", "--context", CTX]).ok();
    assert!(
        listed["retire"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["record"] != "hs-n1"),
        "{listed}"
    );

    // a later proposal of the same record changes nothing
    propose(
        &mut fx.env.store(),
        &ContextId::parse(CTX).unwrap(),
        &[curation("hs-n1", "old fact again", None)],
        &[proposal("hs-n1", Reason::Superseded, "hs-n2")],
    )
    .unwrap();
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM record_retirements WHERE record = 'hs-n1'"),
        1
    );
    assert_eq!(
        fx.env.query::<String>(
            "SELECT status || '/' || reason FROM record_retirements WHERE record = 'hs-n1'"
        ),
        "rejected/duplicate"
    );
    let listed = fx.cpd(&["context", "curation", "--context", CTX]).ok();
    assert!(
        listed["retire"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["record"] != "hs-n1"),
        "{listed}"
    );
}

#[test]
fn curation_lists_pending_retirements_and_doc_candidates() {
    let fx = fx();
    proposed(&fx);
    let out = fx.cpd(&["context", "curation", "--context", CTX]).ok();
    assert_eq!(out["context"], CTX, "{out}");
    assert_eq!(
        (&out["retire_total"], &out["docs_total"]),
        (&json!(3), &json!(1)),
        "{out}"
    );
    assert_eq!(
        out["retire"],
        json!([
            {"record": "hs-n1", "kind": "note", "text": "Old fact.", "short": "old fact", "reason": "duplicate", "target": "hs-n2"},
            {"record": "hs-n3", "kind": "note", "text": "Documented fact.", "short": "documented fact", "reason": "documented", "target": "docs/a.md"},
            {"record": "hs-n4", "kind": "note", "text": "Kept fact.", "short": "kept fact", "reason": "superseded", "target": "hs-d1"},
        ]),
        "{out}"
    );
    assert_eq!(
        out["docs"],
        json!([{"record": "hs-d1", "kind": "decision", "text": "Worth documenting.", "short": "worth documenting", "paths": ["docs/a.md", "docs/b.md"]}]),
        "{out}"
    );
    // read-only
    assert_eq!(fx.cpd(&["context", "curation", "--context", CTX]).ok(), out);
    retire(&fx, "--approve", "hs-n1").ok();
    let after = fx.cpd(&["context", "curation", "--context", CTX]).ok();
    assert_eq!(after["retire"].as_array().unwrap().len(), 2, "{after}");
}

#[test]
fn retire_refuses_an_id_that_is_not_pending() {
    let fx = fx();
    proposed(&fx);
    // hs-n2 was never proposed
    let error = refused(
        &retire(&fx, "--approve", "hs-n1,hs-n2"),
        "curation_not_pending",
        3,
    );
    assert_eq!(error["details"]["record"], "hs-n2", "{error}");
    // all or nothing
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM record_retirements WHERE status = 'pending'"),
        3
    );
    assert!(events(&fx, "records-retired").is_empty());

    // an id decided already
    retire(&fx, "--approve", "hs-n1").ok();
    let error = refused(&retire(&fx, "--reject", "hs-n1"), "curation_not_pending", 3);
    assert_eq!(error["details"]["record"], "hs-n1", "{error}");

    // an id in both lists
    refused(
        &fx.cpd(&[
            "context",
            "retire",
            "--context",
            CTX,
            "--approve",
            "hs-n3",
            "--reject",
            "hs-n3",
        ]),
        "args_invalid",
        2,
    );
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM record_retirements WHERE status = 'pending'"),
        2
    );
}

#[test]
fn retire_refuses_inside_an_agent_session() {
    let fx = fx();
    proposed(&fx);
    let cwd = fx.env.root().to_path_buf();
    for var in ["CPD_SESSION", "CPD_ATTEMPT"] {
        let run = fx.cpd_with(
            &cwd,
            &["context", "retire", "--context", CTX, "--approve", "hs-n1"],
            &[(var, "x")],
        );
        refused(&run, "user_only", 3);
        let run = fx.cpd_with(
            &cwd,
            &["context", "curation", "--context", CTX],
            &[(var, "x")],
        );
        refused(&run, "user_only", 3);
        let run = fx.cpd_with(
            &cwd,
            &["context", "docs", "--context", CTX, "--dismiss", "hs-d1"],
            &[(var, "x")],
        );
        refused(&run, "user_only", 3);
    }
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM record_retirements WHERE status = 'pending'"),
        3
    );
    assert!(events(&fx, "records-retired").is_empty());
}

#[test]
fn retire_appends_one_records_retired_event() {
    let fx = fx();
    proposed(&fx);
    let out = fx
        .cpd(&[
            "context",
            "retire",
            "--context",
            CTX,
            "--approve",
            "hs-n1",
            "--reject",
            "hs-n3,hs-n4",
        ])
        .ok();
    assert_eq!(out["approved"], json!(["hs-n1"]), "{out}");
    assert_eq!(out["rejected"], json!(["hs-n3", "hs-n4"]), "{out}");
    let found = events(&fx, "records-retired");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        found[0]["data"],
        json!({"context": CTX, "approved": ["hs-n1"], "rejected": ["hs-n3", "hs-n4"]})
    );
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM record_retirements WHERE status = 'pending'"),
        0
    );
}

/// A live Manager Session of the Context.
fn manager(fx: &Fx) {
    fx.env.sql(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, forked_from)
         VALUES ('mgr1', 'manager', 'building-cpd', NULL, NULL, 'building-cpd-manager', '/', 'O5', 'idle', 't5', 't5', NULL)",
    );
}

#[test]
fn docs_send_posts_one_board_post_to_the_manager() {
    let fx = fx();
    proposed(&fx);
    // a second candidate, so that one post lists two records
    propose(
        &mut fx.env.store(),
        &ContextId::parse(CTX).unwrap(),
        &[curation("hs-n2", "same fact", Some(&["docs/c.md"]))],
        &[],
    )
    .unwrap();

    // no Manager: refused, nothing written
    refused(
        &fx.cpd(&["context", "docs", "--context", CTX, "--send", "hs-d1"]),
        "manager_missing",
        3,
    );
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM board_posts"), 0);
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM record_curations WHERE doc_candidate = 'pending'"),
        2
    );

    manager(&fx);
    let out = fx
        .cpd(&[
            "context",
            "docs",
            "--context",
            CTX,
            "--send",
            "hs-d1",
            "--dismiss",
            "hs-n2",
        ])
        .ok();
    assert_eq!(
        (&out["sent"], &out["dismissed"]),
        (&json!(["hs-d1"]), &json!(["hs-n2"])),
        "{out}"
    );
    assert_eq!(out["manager"], "mgr1", "{out}");

    let posts = fx.cpd(&["board", "list", "--session", "mgr1"]).ok();
    let posts = posts["posts"].as_array().unwrap();
    assert_eq!(posts.len(), 1, "{posts:?}");
    let text = posts[0]["text"].as_str().unwrap();
    for needle in ["hs-d1", "worth documenting", "docs/a.md", "docs/b.md"] {
        assert!(text.contains(needle), "{needle} in {text}");
    }
    assert!(
        !text.contains("hs-n2") && !text.contains("docs/c.md"),
        "a dismissed candidate is not sent: {text}"
    );
    assert_eq!(
        fx.env.query::<String>("SELECT group_concat(record || ':' || doc_candidate) FROM (SELECT * FROM record_curations WHERE doc_candidate IS NOT NULL ORDER BY record)"),
        "hs-d1:sent,hs-n2:dismissed"
    );
    let decided = events(&fx, "docs-decided");
    assert_eq!(decided.len(), 1, "{decided:?}");
    assert_eq!(
        decided[0]["data"],
        json!({"context": CTX, "sent": ["hs-d1"], "dismissed": ["hs-n2"]})
    );

    // decided: refused again
    let error = refused(
        &fx.cpd(&["context", "docs", "--context", CTX, "--send", "hs-d1"]),
        "curation_not_pending",
        3,
    );
    assert_eq!(error["details"]["record"], "hs-d1", "{error}");
}

#[test]
fn show_labels_a_retired_record() {
    let fx = fx();
    proposed(&fx);
    retire(&fx, "--approve", "hs-n1,hs-n3").ok();
    let md = shown(&fx)["markdown"].as_str().unwrap().to_owned();
    let duplicate = item_line(&md, "hs-n1");
    assert!(
        duplicate.contains("Old fact.") && duplicate.contains("retired (duplicate hs-n2)"),
        "{md}"
    );
    let documented = item_line(&md, "hs-n3");
    assert!(
        documented.contains("retired (documented docs/a.md)"),
        "{md}"
    );
    let kept = item_line(&md, "hs-n2");
    assert!(!kept.contains("retired"), "{md}");
    // in the section of its kind, after the active ones
    assert!(
        md.find(&kept).unwrap() < md.find(&duplicate).unwrap(),
        "{md}"
    );
}

/// As `proposed`, plus hs-n2 as a second doc candidate: three retirements (hs-n1, hs-n3, hs-n4), two candidates
/// (hs-n2, hs-d1; oldest first is record order).
fn proposed_two_candidates(fx: &Fx) {
    let curations = [
        curation("hs-n1", "old fact", None),
        curation("hs-n2", "same fact", Some(&["docs/c.md"])),
        curation("hs-n3", "documented fact", None),
        curation("hs-n4", "kept fact", None),
        curation(
            "hs-d1",
            "worth documenting",
            Some(&["docs/a.md", "docs/b.md"]),
        ),
    ];
    let proposals = [
        proposal("hs-n1", Reason::Duplicate, "hs-n2"),
        proposal("hs-n3", Reason::Documented, "docs/a.md"),
        proposal("hs-n4", Reason::Superseded, "hs-d1"),
    ];
    propose(
        &mut fx.env.store(),
        &ContextId::parse(CTX).unwrap(),
        &curations,
        &proposals,
    )
    .unwrap();
}

fn ids(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|e| e["record"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn curation_gives_each_entry_the_records_full_text() {
    let fx = fx();
    proposed_two_candidates(&fx);
    let out = fx.cpd(&["context", "curation", "--context", CTX]).ok();
    let texts = |list: &Value| {
        list.as_array()
            .unwrap()
            .iter()
            .map(|e| e["text"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        texts(&out["retire"]),
        [
            json!("Old fact."),
            json!("Documented fact."),
            json!("Kept fact.")
        ],
        "{out}"
    );
    assert_eq!(
        texts(&out["docs"]),
        [json!("Same fact."), json!("Worth documenting.")],
        "{out}"
    );
}

#[test]
fn curation_limit_returns_the_first_n_of_each_list_oldest_first() {
    let fx = fx();
    proposed_two_candidates(&fx);
    let out = fx
        .cpd(&["context", "curation", "--context", CTX, "--limit", "2"])
        .ok();
    assert_eq!(ids(&out["retire"]), ["hs-n1", "hs-n3"], "{out}");
    assert_eq!(ids(&out["docs"]), ["hs-n2", "hs-d1"], "{out}");
    let out = fx
        .cpd(&["context", "curation", "--context", CTX, "--limit=1"])
        .ok();
    assert_eq!(ids(&out["retire"]), ["hs-n1"], "{out}");
    assert_eq!(ids(&out["docs"]), ["hs-n2"], "{out}");
}

#[test]
fn curation_totals_count_every_pending_entry_whatever_the_limit() {
    let fx = fx();
    proposed_two_candidates(&fx);
    let out = fx
        .cpd(&["context", "curation", "--context", CTX, "--limit", "1"])
        .ok();
    assert_eq!(
        (&out["retire_total"], &out["docs_total"]),
        (&json!(3), &json!(2)),
        "{out}"
    );
    // a decided entry leaves the total
    retire(&fx, "--approve", "hs-n1").ok();
    let out = fx
        .cpd(&["context", "curation", "--context", CTX, "--limit", "1"])
        .ok();
    assert_eq!(
        (&out["retire_total"], &out["docs_total"]),
        (&json!(2), &json!(2)),
        "{out}"
    );
    assert_eq!(ids(&out["retire"]), ["hs-n3"], "{out}");
}

#[test]
fn curation_without_a_limit_returns_every_entry() {
    let fx = fx();
    proposed_two_candidates(&fx);
    let out = fx.cpd(&["context", "curation", "--context", CTX]).ok();
    assert_eq!(ids(&out["retire"]), ["hs-n1", "hs-n3", "hs-n4"], "{out}");
    assert_eq!(ids(&out["docs"]), ["hs-n2", "hs-d1"], "{out}");
    assert_eq!(
        (&out["retire_total"], &out["docs_total"]),
        (&json!(3), &json!(2)),
        "{out}"
    );
}

#[test]
fn curation_refuses_a_limit_of_zero() {
    let fx = fx();
    proposed_two_candidates(&fx);
    refused(
        &fx.cpd(&["context", "curation", "--context", CTX, "--limit", "0"]),
        "args_invalid",
        2,
    );
}

/// hs-d592 R9: a dismissed (or sent) doc candidate is never proposed again, even by a second curation line.
#[test]
fn a_dismissed_doc_candidate_is_never_proposed_again() {
    let fx = fx();
    manager(&fx);
    proposed_two_candidates(&fx);
    fx.cpd(&[
        "context",
        "docs",
        "--context",
        CTX,
        "--send",
        "hs-d1",
        "--dismiss",
        "hs-n2",
    ])
    .ok();

    let again = [
        curation("hs-n2", "same fact", Some(&["docs/z.md"])),
        curation("hs-d1", "worth documenting", Some(&["docs/z.md"])),
    ];
    propose(
        &mut fx.env.store(),
        &ContextId::parse(CTX).unwrap(),
        &again,
        &[],
    )
    .unwrap();

    assert_eq!(
        fx.env.query::<String>("SELECT group_concat(record || ':' || doc_candidate) FROM (SELECT * FROM record_curations WHERE doc_candidate IS NOT NULL ORDER BY record)"),
        "hs-d1:sent,hs-n2:dismissed"
    );
    let listed = fx.cpd(&["context", "curation", "--context", CTX]).ok();
    assert_eq!(listed["docs_total"], 0, "{listed}");
}
