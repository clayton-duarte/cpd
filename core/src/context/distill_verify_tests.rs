//! Red tests of `distill::verify` (Plan ctx-auto-retire, stage C): kev-4b and the judge verify each duplicate or
//! superseded proposal of a checked output; a documented one is never sent to them. Every port is a fake.

use std::collections::{HashMap, HashSet};

use super::{Checked, State, Verified, verify, verify_lazy};
use crate::classifier::Chosen;
use crate::context::curate::{Judged, Verdict};
use crate::context::curation::{Approval, Curation, Proposal, Reason, propose_rejecting};
use crate::context::render::Indexed;
use crate::context::{By, Draft, Record};
use crate::error::{CpdError, ErrorCode};
use crate::fakes::classifier::FakeClassifier;
use crate::fakes::judge::FakeJudge;
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};
use crate::store::Store;

const OLD_TEXT: &str = "Use Postgres for the store because transactions matter.";
const NEW_TEXT: &str = "Use Postgres for the store, transactions matter most.";
const QUOTE: &str = "Use Postgres for the store";
const DOC: &str = "/r/a:docs/store.md";

fn id(text: &str) -> RecordId {
    RecordId::parse(text).unwrap()
}

fn record(seq: i64, text_id: &str, text: &str) -> Indexed {
    (
        seq,
        Record {
            id: id(text_id),
            at: "2026-10-01T10:00:00.000Z".into(),
            by: By {
                session: None,
                plan: None,
            },
            text: text.into(),
            draft: Draft::Decision { supersedes: vec![] },
        },
    )
}

/// `hs-d1` (seq 1, the old text), `hs-d2` (seq 2, the new text) and `hs-d3` (seq 3, the new text again), curated
/// as the Context has them.
fn state() -> State {
    State {
        records: vec![
            record(1, "hs-d1", OLD_TEXT),
            record(2, "hs-d2", NEW_TEXT),
            record(3, "hs-d3", NEW_TEXT),
        ],
        retired: HashSet::new(),
        curated: HashMap::new(),
    }
}

fn curation(record: &str) -> Curation {
    Curation {
        record: id(record),
        topic: "postgres-store".into(),
        short: "Use Postgres for the store".into(),
    }
}

fn proposal(record: &str, reason: Reason, target: &str, approval: Approval) -> Proposal {
    Proposal {
        record: id(record),
        reason,
        target: target.into(),
        approval,
    }
}

/// `hs-d2` is a duplicate of `hs-d1`: the later one is retired.
fn duplicate() -> Proposal {
    proposal("hs-d2", Reason::Duplicate, "hs-d1", Approval::Pending)
}

/// `hs-d1` is superseded by `hs-d2`: the older one is retired.
fn superseded() -> Proposal {
    proposal("hs-d1", Reason::Superseded, "hs-d2", Approval::Pending)
}

fn checked(records: &[&str], proposals: Vec<Proposal>) -> Checked {
    Checked {
        curations: records.iter().map(|r| curation(r)).collect(),
        proposals,
        rejected: Vec::new(),
    }
}

fn chosen(label: &str, confidence: f32) -> Result<Chosen, CpdError> {
    Ok(Chosen {
        label: label.into(),
        confidence: Some(confidence),
        usage: None,
        model: "kev-4b".into(),
    })
}

fn judged(verdict: Verdict, quote: &str) -> Judged {
    Judged {
        verdict,
        quote: quote.into(),
        reason: "the same decision".into(),
        topic: "postgres-store".into(),
        short: "Use Postgres for the store".into(),
    }
}

fn down() -> Result<Chosen, CpdError> {
    Err(CpdError::new(ErrorCode::ClassifierUnreachable, "scripted"))
}

fn approvals(v: &Verified) -> Vec<(String, Approval)> {
    v.checked
        .proposals
        .iter()
        .map(|p| (p.record.to_string(), p.approval))
        .collect()
}

#[test]
fn a_duplicate_agreed_by_kev_and_judge_is_approved() {
    let (kev, judge) = (
        FakeClassifier::answering(chosen("duplicate", 0.9).unwrap()),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = verify(
        &kev,
        &judge,
        &state(),
        checked(&["hs-d2"], vec![duplicate()]),
    )
    .unwrap();
    assert_eq!(approvals(&out), [("hs-d2".to_owned(), Approval::Models)]);
    assert_eq!(out.dropped, 0);
    assert_eq!((kev.calls().len(), judge.calls().len()), (1, 1));
    // The judge sees the later record as the new one and the earlier as its neighbour.
    let asked = &judge.calls()[0];
    assert_eq!(asked.new.id, id("hs-d2"));
    assert_eq!(asked.neighbour.as_ref().unwrap().id, id("hs-d1"));
}

#[test]
fn a_superseded_agreed_by_kev_and_judge_is_approved() {
    let (kev, judge) = (
        FakeClassifier::answering(chosen("supersedes", 0.9).unwrap()),
        FakeJudge::answering(judged(Verdict::Supersedes, QUOTE)),
    );
    let out = verify(
        &kev,
        &judge,
        &state(),
        checked(&["hs-d1"], vec![superseded()]),
    )
    .unwrap();
    assert_eq!(approvals(&out), [("hs-d1".to_owned(), Approval::Models)]);
    assert_eq!(out.dropped, 0);
    // The later record (the target) is the new one.
    assert_eq!(judge.calls()[0].new.id, id("hs-d2"));
}

#[test]
fn an_agreed_proposal_with_kev_unreachable_stays_pending() {
    let (kev, judge) = (
        FakeClassifier::new(vec![down()]),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = verify(
        &kev,
        &judge,
        &state(),
        checked(&["hs-d2"], vec![duplicate()]),
    )
    .unwrap();
    assert_eq!(approvals(&out), [("hs-d2".to_owned(), Approval::Pending)]);
    assert_eq!(out.dropped, 0);
    assert_eq!(kev.calls().len(), 1);
    assert!(judge.calls().is_empty());
    assert_eq!(out.checked.curations.len(), 1);
}

#[test]
fn a_proposal_the_models_disagree_on_is_dropped_and_its_curation_kept() {
    // kev is unsure it is independent, so the judge is asked; it says duplicate: the models disagree.
    let (kev, judge) = (
        FakeClassifier::answering(chosen("independent", 0.5).unwrap()),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = verify(
        &kev,
        &judge,
        &state(),
        checked(&["hs-d2"], vec![duplicate()]),
    )
    .unwrap();
    assert!(out.checked.proposals.is_empty());
    assert_eq!(out.dropped, 1);
    assert_eq!(out.checked.curations.len(), 1);
    assert_eq!(out.checked.curations[0].record, id("hs-d2"));
    assert_eq!(judge.calls().len(), 1);
}

#[test]
fn a_proposal_whose_quote_is_in_neither_record_is_dropped() {
    let (kev, judge) = (
        FakeClassifier::answering(chosen("duplicate", 0.9).unwrap()),
        FakeJudge::answering(judged(Verdict::Duplicate, "words nobody wrote")),
    );
    let out = verify(
        &kev,
        &judge,
        &state(),
        checked(&["hs-d2"], vec![duplicate()]),
    )
    .unwrap();
    assert!(out.checked.proposals.is_empty());
    assert_eq!(out.dropped, 1);
    assert_eq!(out.checked.curations.len(), 1);
}

#[test]
fn a_proposal_the_judge_reads_in_the_other_direction_is_dropped() {
    // Proposed: the older hs-d1 is superseded. The models call the pair a duplicate, which retires the later hs-d2.
    let (kev, judge) = (
        FakeClassifier::answering(chosen("duplicate", 0.9).unwrap()),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = verify(
        &kev,
        &judge,
        &state(),
        checked(&["hs-d1"], vec![superseded()]),
    )
    .unwrap();
    assert!(out.checked.proposals.is_empty());
    assert_eq!(out.dropped, 1);
    assert_eq!(out.checked.curations.len(), 1);
}

#[test]
fn a_contradicts_verdict_drops_the_proposal() {
    let (kev, judge) = (
        FakeClassifier::answering(chosen("contradicts", 0.9).unwrap()),
        FakeJudge::answering(judged(Verdict::Contradicts, QUOTE)),
    );
    let out = verify(
        &kev,
        &judge,
        &state(),
        checked(&["hs-d2"], vec![duplicate()]),
    )
    .unwrap();
    assert!(out.checked.proposals.is_empty());
    assert_eq!(out.dropped, 1);
}

#[test]
fn a_judge_error_keeps_the_proposal_pending_and_stops_asking() {
    let kev = FakeClassifier::answering(chosen("duplicate", 0.9).unwrap());
    let judge = FakeJudge::new(vec![Err(CpdError::new(
        ErrorCode::Internal,
        "the judge is down",
    ))]);
    // hs-d3 (seq 3) duplicates hs-d1 as well: a second pair to ask about.
    let second = proposal("hs-d3", Reason::Duplicate, "hs-d1", Approval::Pending);
    let out = verify(
        &kev,
        &judge,
        &state(),
        checked(&["hs-d2", "hs-d3"], vec![duplicate(), second]),
    )
    .unwrap();
    assert_eq!(
        approvals(&out),
        [
            ("hs-d2".to_owned(), Approval::Pending),
            ("hs-d3".to_owned(), Approval::Pending)
        ]
    );
    assert_eq!(out.dropped, 0);
    assert_eq!((kev.calls().len(), judge.calls().len()), (1, 1));
}

#[test]
fn a_documented_proposal_is_not_sent_to_the_models() {
    let (kev, judge) = (
        FakeClassifier::answering(chosen("duplicate", 0.9).unwrap()),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let documented = proposal("hs-d3", Reason::Documented, DOC, Approval::GitGrep);
    let out = verify(
        &kev,
        &judge,
        &state(),
        checked(&["hs-d2", "hs-d3"], vec![documented, duplicate()]),
    )
    .unwrap();
    assert_eq!(
        approvals(&out),
        [
            ("hs-d3".to_owned(), Approval::GitGrep),
            ("hs-d2".to_owned(), Approval::Models)
        ]
    );
    // Only the duplicate went to the models.
    assert_eq!((kev.calls().len(), judge.calls().len()), (1, 1));
}

#[test]
fn verified_proposals_are_written_approved_with_one_records_retired_event() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let context = ContextId::parse("ctx").unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'C', '2026-11-01T00:00:00.000Z', 'hs');",
            )?;
            for (_, r) in state().records {
                r.insert(tx, &context)?;
            }
            Ok(())
        })
        .unwrap();
    let read = store.read(|conn| State::read(conn, &context)).unwrap();
    let (kev, judge) = (
        FakeClassifier::answering(chosen("duplicate", 0.9).unwrap()),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let documented = proposal("hs-d3", Reason::Documented, DOC, Approval::GitGrep);
    let out = verify(
        &kev,
        &judge,
        &read,
        checked(&["hs-d2", "hs-d3"], vec![duplicate(), documented]),
    )
    .unwrap();
    let written = propose_rejecting(
        &mut store,
        &context,
        &out.checked.curations,
        &out.checked.proposals,
        0,
    )
    .unwrap();
    assert_eq!(written, 2);
    let rows: Vec<String> = store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT record || '|' || status || '|' || coalesce(decided_by, '') FROM record_retirements ORDER BY record",
            )?;
            Ok(stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(
        rows,
        [
            "hs-d2|approved|kev-4b+gemini-3.8-flash",
            "hs-d3|approved|git-grep"
        ]
    );
    let events: i64 = store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM events WHERE kind = 'records-retired'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(events, 1);
}

#[test]
fn a_run_without_a_duplicate_or_superseded_proposal_builds_no_models() {
    let documented = proposal("hs-d2", Reason::Documented, DOC, Approval::Pending);
    let none = checked(&["hs-d2"], vec![documented]);
    let build = || -> Result<super::Models, CpdError> {
        Err(CpdError::new(ErrorCode::PiNotFound, "no pi install"))
    };
    let v = verify_lazy(&build, &state(), none).unwrap();
    assert_eq!(v.dropped, 0);
    assert_eq!(v.checked.proposals.len(), 1);
    let v = verify_lazy(&build, &state(), checked(&["hs-d2"], vec![])).unwrap();
    assert_eq!(v.checked.proposals.len(), 0);
}

#[test]
fn a_duplicate_proposal_builds_the_models_and_fails_loud_without_pi() {
    let build = || -> Result<super::Models, CpdError> {
        Err(CpdError::new(ErrorCode::PiNotFound, "no pi install"))
    };
    let e = verify_lazy(&build, &state(), checked(&["hs-d2"], vec![duplicate()]))
        .err()
        .unwrap();
    assert_eq!(e.code(), ErrorCode::PiNotFound);
}
