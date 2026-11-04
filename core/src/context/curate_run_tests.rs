//! Red tests of `context curate` (commands.md §3.13a; Plan exp-write-curation, A8, A10, A13..A16): the pick, the kind
//! gate, the neighbours, the verdicts, the writes, the degraded modes and the flag. Every port is a fake.

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::path::Path;

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::datetime;

use super::curate::{Judged, Verdict};
use super::curate_run::{DIMS, FILL, Models, WINDOW_HOURS, curate_with, next_record};
use super::curate_vectors::VectorStore;
use super::curation::{Approval, Curation, Proposal, Reason as CurationReason, propose};
use super::distill::State;
use super::{By, Draft, PlanRef, Record};
use crate::classifier::Chosen;
use crate::config::Config;
use crate::embedding::Embedded;
use crate::error::{CpdError, ErrorCode};
use crate::fakes::classifier::FakeClassifier;
use crate::fakes::embedding::FakeEmbedding;
use crate::fakes::judge::FakeJudge;
use crate::home::CpdHome;
use crate::ids::{ContextId, MachineName, PlanId, RecordId, RecordKind};
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-11-03 12:00:00 UTC);
const OLD_TEXT: &str = "Use Postgres for the store because transactions matter.";
const NEW_TEXT: &str = "Use Postgres for the store, transactions matter most.";
const QUOTE: &str = "Use Postgres for the store";

fn id(text: &str) -> RecordId {
    RecordId::parse(text).unwrap()
}

fn context() -> ContextId {
    ContextId::parse("ctx").unwrap()
}

/// The RFC 3339 `at` of a record written `minutes` before `NOW`.
fn at(minutes: i64) -> String {
    crate::clock::timestamp(NOW - time::Duration::minutes(minutes))
}

const HOURS: i64 = 60;

struct Fx {
    _dir: tempfile::TempDir,
    home: CpdHome,
    config: Config,
    store: Store,
}

fn fixture(flag: bool) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let text = format!("version = 1\nmachine = \"hs\"\n[experimental]\nwrite_curation = {flag}\n");
    let (config, _) = Config::parse(&text, Path::new("config.toml")).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'C', '2026-11-01T00:00:00.000Z', 'hs');",
            )?;
            Ok(())
        })
        .unwrap();
    Fx {
        _dir: dir,
        home,
        config,
        store,
    }
}

fn draft_record(id: &str, minutes: i64, text: &str, draft: Draft) -> Record {
    Record {
        id: self::id(id),
        at: at(minutes),
        by: By {
            session: None,
            plan: None,
        },
        text: text.into(),
        draft,
    }
}

impl Fx {
    /// Writes a record of the kind of its id's letter, `minutes` before `NOW`; records get their seq in call order.
    fn add(&mut self, rid: &str, minutes: i64, text: &str) {
        let draft = match self::id(rid).kind() {
            RecordKind::Decision => Draft::Decision { supersedes: vec![] },
            RecordKind::Note => Draft::Note,
            kind => panic!("fixture.add does not make a {kind:?}"),
        };
        self.insert(draft_record(rid, minutes, text, draft));
    }

    fn insert(&mut self, record: Record) {
        let ctx = context();
        self.store
            .write("fixture", |tx| {
                record.insert(tx, &ctx)?;
                Ok(())
            })
            .unwrap();
    }

    fn sql(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    fn count(&mut self, sql: &str) -> i64 {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    fn text(&mut self, sql: &str) -> String {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    fn state(&mut self) -> State {
        let ctx = context();
        self.store.read(|c| State::read(c, &ctx)).unwrap()
    }

    fn checked(&self) -> HashSet<RecordId> {
        VectorStore::at(&self.home).checked(&context()).unwrap()
    }

    fn mark(&self, rid: &str) {
        VectorStore::at(&self.home)
            .mark_checked(&context(), &id(rid), &at(0))
            .unwrap();
    }

    fn events(&mut self) -> i64 {
        self.count("SELECT count(*) FROM events WHERE kind = 'context-curated'")
    }

    fn rows(&mut self) -> (i64, i64) {
        (
            self.count("SELECT count(*) FROM record_curations"),
            self.count("SELECT count(*) FROM record_retirements"),
        )
    }

    fn run(&mut self, ports: &Ports, only: Option<&RecordId>) -> Result<Value, CpdError> {
        let models = Models {
            classifier: &ports.classifier,
            embedding: &ports.embedding,
            judge: &ports.judge,
        };
        curate_with(
            &mut self.store,
            &self.home,
            &self.config,
            &context(),
            &models,
            NOW,
            only,
        )
    }

    /// An older decision `hs-d1` (out of the 24 h window) and the new `hs-d2`: the pair most tests judge.
    fn pair() -> Fx {
        let mut f = fixture(true);
        f.add("hs-d1", 30 * HOURS, OLD_TEXT);
        f.add("hs-d2", HOURS, NEW_TEXT);
        f
    }
}

struct Ports {
    classifier: FakeClassifier,
    embedding: FakeEmbedding,
    judge: FakeJudge,
}

fn chosen(label: &str, confidence: Option<f32>) -> Result<Chosen, CpdError> {
    Ok(Chosen {
        label: label.into(),
        confidence,
        usage: None,
        model: "kev-4b".into(),
    })
}

fn embedded() -> Result<Embedded, CpdError> {
    Ok(Embedded {
        vector: vec![1.0, 0.0, 0.0, 0.0],
        model: "m".into(),
        tokens: None,
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

fn ports(kev: Result<Chosen, CpdError>, judge: FakeJudge) -> Ports {
    Ports {
        classifier: FakeClassifier::new(vec![kev]),
        embedding: FakeEmbedding::new(vec![embedded()]),
        judge,
    }
}

fn code_err(code: ErrorCode) -> CpdError {
    CpdError::new(code, "scripted")
}

/// The records `next_record` picks one after the other, marking each checked.
fn picks(f: &mut Fx, mut checked: HashSet<RecordId>) -> Vec<RecordId> {
    let state = f.state();
    let mut out = Vec::new();
    while let Some(next) = next_record(&state, &checked, NOW) {
        checked.insert(next.clone());
        out.push(next);
    }
    out
}

fn plan_ref() -> PlanRef {
    PlanRef {
        id: PlanId::parse("alpha").unwrap(),
        machine: MachineName::parse("hs").unwrap(),
        jira: None,
    }
}

// ---- the pick -----------------------------------------------------------------------------------------------

#[test]
fn next_record_takes_the_oldest_unchecked_decision_or_note() {
    let mut f = fixture(true);
    f.add("hs-n1", 5 * HOURS, "A note.");
    f.add("hs-d1", 4 * HOURS, "A decision.");
    assert_eq!(picks(&mut f, HashSet::new()), [id("hs-n1"), id("hs-d1")]);
}

#[test]
fn next_record_never_picks_a_plan_or_a_brief() {
    let mut f = fixture(true);
    f.insert(draft_record(
        "hs-p1",
        4 * HOURS,
        "A plan.",
        Draft::Plan(plan_ref()),
    ));
    let brief = RecordId::new(
        MachineName::parse("hs").unwrap(),
        RecordKind::Brief,
        NonZeroU32::new(1).unwrap(),
    );
    let mut brief_record = draft_record("hs-p2", 3 * HOURS, "A brief.", Draft::Brief(plan_ref()));
    brief_record.id = brief;
    f.insert(brief_record);
    f.add("hs-n1", 2 * HOURS, "A note.");
    assert_eq!(picks(&mut f, HashSet::new()), [id("hs-n1")]);
}

#[test]
fn next_record_skips_checked_records_and_records_older_than_24_hours() {
    assert_eq!(WINDOW_HOURS, 24);
    let mut f = fixture(true);
    f.add("hs-n1", 25 * HOURS, "Too old.");
    f.add("hs-n2", 23 * HOURS, "Checked.");
    f.add("hs-n3", 22 * HOURS, "Due.");
    let checked = HashSet::from([id("hs-n2")]);
    assert_eq!(picks(&mut f, checked), [id("hs-n3")]);
}

#[test]
fn next_record_is_none_when_nothing_is_due() {
    let mut f = fixture(true);
    let state = f.state();
    assert_eq!(next_record(&state, &HashSet::new(), NOW), None);
    f.add("hs-n1", 25 * HOURS, "Too old.");
    f.add("hs-n2", HOURS, "Checked.");
    let state = f.state();
    assert_eq!(
        next_record(&state, &HashSet::from([id("hs-n2")]), NOW),
        None
    );
}

// ---- the flag and the gate ------------------------------------------------------------------------------------

#[test]
fn flag_off_makes_no_port_call_and_creates_no_vectors_db() {
    let mut f = fixture(false);
    f.add("hs-d1", 30 * HOURS, OLD_TEXT);
    f.add("hs-d2", HOURS, NEW_TEXT);
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(out, json!({"context": "ctx", "skipped": "off"}));
    assert!(p.classifier.calls().is_empty());
    assert!(p.embedding.calls().is_empty());
    assert!(p.judge.calls().is_empty());
    assert!(!f.home.vectors_db().exists());
    assert_eq!(f.rows(), (0, 0));
    assert_eq!(f.events(), 0);
}

// ---- the screen -----------------------------------------------------------------------------------------------

#[test]
fn an_independent_pair_accepted_by_kev_at_080_makes_no_judge_call() {
    let mut f = Fx::pair();
    let p = ports(chosen("independent", Some(0.80)), FakeJudge::new(vec![]));
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["record"], json!("hs-d2"));
    assert_eq!(out["neighbours"], json!(1));
    assert_eq!(out["free"], json!(1));
    assert_eq!(out["judged"], json!(0));
    assert!(p.judge.calls().is_empty());
    assert_eq!(p.classifier.calls().len(), 1);
    assert_eq!(f.rows(), (0, 0));
    assert_eq!(f.events(), 0);
    assert!(f.checked().contains(&id("hs-d2")));
}

#[test]
fn a_low_confidence_pair_goes_to_the_judge() {
    let mut f = Fx::pair();
    let p = ports(
        chosen("independent", Some(0.79)),
        FakeJudge::answering(judged(Verdict::Independent, "")),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["free"], json!(0));
    assert_eq!(out["judged"], json!(1));
    assert_eq!(p.judge.calls().len(), 1);
    assert_eq!(f.rows(), (0, 0));
}

// ---- the verdicts and the writes --------------------------------------------------------------------------------

const ROW: &str = "SELECT record || '|' || target || '|' || reason || '|' || status || '|' || coalesce(decided_by, '') FROM record_retirements";
const MODELS: &str = "kev-4b+gemini-3.8-flash";

#[test]
fn agreement_and_a_verbatim_quote_write_an_approved_duplicate_retirement() {
    let mut f = Fx::pair();
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(
        out["retire"],
        json!([{"record": "hs-d2", "target": "hs-d1", "reason": "duplicate"}])
    );
    assert_eq!(f.rows(), (1, 1));
    assert_eq!(
        f.text(ROW),
        format!("hs-d2|hs-d1|duplicate|approved|{MODELS}")
    );
    assert_eq!(
        f.count("SELECT count(*) FROM record_retirements WHERE decided_at = proposed_at"),
        1
    );
    assert_eq!(f.text("SELECT record FROM record_curations"), "hs-d2");
    assert_eq!(f.events(), 1);
    assert!(f.checked().contains(&id("hs-d2")));
}

#[test]
fn supersedes_writes_an_approved_retirement_of_the_older_neighbour() {
    let mut f = Fx::pair();
    let p = ports(
        chosen("supersedes", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Supersedes, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(
        out["retire"],
        json!([{"record": "hs-d1", "target": "hs-d2", "reason": "superseded"}])
    );
    assert_eq!(
        f.text(ROW),
        format!("hs-d1|hs-d2|superseded|approved|{MODELS}")
    );
    assert_eq!(
        f.count("SELECT count(*) FROM record_retirements WHERE decided_at = proposed_at"),
        1
    );
    assert_eq!(f.text("SELECT record FROM record_curations"), "hs-d1");
    assert_eq!(f.events(), 1);
}

#[test]
fn an_approved_write_appends_one_records_retired_event() {
    let mut f = Fx::pair();
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    f.run(&p, None).unwrap();
    assert_eq!(f.retired_events(), 1);
}

#[test]
fn without_kev_an_agreed_retirement_stays_pending() {
    let mut f = Fx::pair();
    let down = ports(
        Err(code_err(ErrorCode::ClassifierUnreachable)),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    f.run(&down, None).unwrap();
    assert_eq!(f.text(ROW), "hs-d2|hs-d1|duplicate|pending|");
    assert_eq!(f.retired_events(), 0);
    // kev is back: the next run's sweep decides the row it left pending
    let up = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = f.run(&up, None).unwrap();
    assert_eq!(out["swept"], json!({"approved": 1, "rejected": 0}));
    assert_eq!(
        f.text(ROW),
        format!("hs-d2|hs-d1|duplicate|approved|{MODELS}")
    );
}

// ---- the sweep of pending rows ------------------------------------------------------------------------------------

/// Two records out of the window per pair, `hs-d{2n-1}` (older) and `hs-d{2n}`, and a pending duplicate row
/// retiring the later one: nothing is due, so only the sweep has work.
fn swept(pairs: i64) -> Fx {
    let mut f = fixture(true);
    for n in 1..=pairs {
        f.add(&format!("hs-d{}", 2 * n - 1), 40 * HOURS, OLD_TEXT);
        f.add(&format!("hs-d{}", 2 * n), 30 * HOURS, NEW_TEXT);
        f.seed(&format!("hs-d{}", 2 * n), &format!("hs-d{}", 2 * n - 1));
    }
    f
}

impl Fx {
    fn retired_events(&mut self) -> i64 {
        self.count("SELECT count(*) FROM events WHERE kind = 'records-retired'")
    }

    /// A pending duplicate row of `record` (curated) retiring `target`.
    fn seed(&mut self, record: &str, target: &str) {
        let curation = Curation {
            record: id(record),
            topic: "postgres-store".into(),
            short: "Use Postgres for the store".into(),
        };
        let proposal = Proposal {
            record: id(record),
            reason: CurationReason::Duplicate,
            target: target.into(),
            approval: Approval::Pending,
        };
        propose(&mut self.store, &context(), &[curation], &[proposal]).unwrap();
    }

    fn pending(&mut self) -> i64 {
        self.count("SELECT count(*) FROM record_retirements WHERE status = 'pending'")
    }
}

#[test]
fn the_sweep_approves_a_pending_pair_both_models_confirm() {
    let mut f = swept(1);
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["swept"], json!({"approved": 1, "rejected": 0}));
    assert_eq!(
        f.text(ROW),
        format!("hs-d2|hs-d1|duplicate|approved|{MODELS}")
    );
    assert_eq!(f.retired_events(), 1);
    let calls = p.judge.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].new.id, id("hs-d2"));
    assert_eq!(calls[0].neighbour.as_ref().unwrap().id, id("hs-d1"));
}

#[test]
fn the_sweep_rejects_a_pending_pair_the_models_disagree_on() {
    let mut f = swept(1);
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Supersedes, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["swept"], json!({"approved": 0, "rejected": 1}));
    assert_eq!(
        f.text(ROW),
        format!("hs-d2|hs-d1|duplicate|rejected|{MODELS}")
    );
}

#[test]
fn the_sweep_rejects_a_pending_pair_kev_accepts_as_independent() {
    let mut f = swept(1);
    let p = ports(chosen("independent", Some(0.9)), FakeJudge::new(vec![]));
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["swept"], json!({"approved": 0, "rejected": 1}));
    assert_eq!(
        f.text(ROW),
        format!("hs-d2|hs-d1|duplicate|rejected|{MODELS}")
    );
    assert!(p.judge.calls().is_empty());
}

#[test]
fn the_sweep_rejects_a_pending_pair_whose_quote_is_in_neither_record() {
    let mut f = swept(1);
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Duplicate, "a phrase in neither record")),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["swept"], json!({"approved": 0, "rejected": 1}));
    assert_eq!(
        f.text(ROW),
        format!("hs-d2|hs-d1|duplicate|rejected|{MODELS}")
    );
}

#[test]
fn the_sweep_leaves_a_pending_pair_when_kev_is_unreachable_and_stops() {
    let mut f = swept(2);
    let p = ports(
        Err(code_err(ErrorCode::ClassifierUnreachable)),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(p.classifier.calls().len(), 1);
    assert!(p.judge.calls().is_empty());
    assert_eq!(f.pending(), 2);
    assert!(out.get("swept").is_none());
}

#[test]
fn the_sweep_leaves_a_pending_pair_on_a_judge_error() {
    let mut f = swept(2);
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::new(vec![Err(code_err(ErrorCode::JudgeUnreachable))]),
    );
    let _ = f.run(&p, None);
    assert_eq!(p.judge.calls().len(), 1);
    assert_eq!(p.classifier.calls().len(), 1);
    assert_eq!(f.pending(), 2);
}

#[test]
fn the_sweep_leaves_a_contradicting_pending_pair_pending() {
    let mut f = swept(1);
    let p = ports(
        chosen("contradicts", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Contradicts, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(p.judge.calls().len(), 1);
    assert_eq!(f.text(ROW), "hs-d2|hs-d1|duplicate|pending|");
    assert!(out.get("swept").is_none());
}

#[test]
fn the_sweep_runs_when_no_record_is_due() {
    let mut f = swept(1);
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["already"], json!(true));
    assert_eq!(out["remaining"], json!(0));
    assert_eq!(out["swept"], json!({"approved": 1, "rejected": 0}));
}

#[test]
fn the_sweep_checks_at_most_three_pairs_per_run() {
    let mut f = swept(4);
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(p.classifier.calls().len(), 3);
    assert_eq!(out["swept"], json!({"approved": 3, "rejected": 0}));
    assert_eq!(f.pending(), 1);
    assert_eq!(f.retired_events(), 1);
}

#[test]
fn the_sweep_ignores_documented_rows() {
    let mut f = fixture(true);
    f.add("hs-d1", 30 * HOURS, OLD_TEXT);
    let curation = Curation {
        record: id("hs-d1"),
        topic: "postgres-store".into(),
        short: "Use Postgres for the store".into(),
    };
    let proposal = Proposal {
        record: id("hs-d1"),
        reason: CurationReason::Documented,
        target: "docs/store.md".into(),
        approval: Approval::GitGrep,
    };
    propose(&mut f.store, &context(), &[curation], &[proposal]).unwrap();
    f.sql(
        "UPDATE record_retirements SET status = 'pending', decided_at = NULL, decided_by = NULL;",
    );
    // a duplicate row beside it: only that one is swept
    f.add("hs-d2", 30 * HOURS, NEW_TEXT);
    f.seed("hs-d2", "hs-d1");
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(p.classifier.calls().len(), 1);
    assert_eq!(p.judge.calls().len(), 1);
    assert_eq!(out["swept"], json!({"approved": 1, "rejected": 0}));
    assert_eq!(
        f.text("SELECT status FROM record_retirements WHERE reason = 'documented'"),
        "pending"
    );
}

#[test]
fn a_run_that_sweeps_nothing_has_no_swept_key() {
    let mut f = swept(1);
    let p = ports(
        chosen("contradicts", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Contradicts, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    // the sweep did look at the pair, and decided nothing
    assert_eq!(p.judge.calls().len(), 1);
    assert_eq!(
        out,
        json!({"context": "ctx", "already": true, "remaining": 0})
    );
}

#[test]
fn a_disagreeing_judge_writes_nothing_and_is_counted() {
    let mut f = Fx::pair();
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Supersedes, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["disagreed"], json!(1));
    assert_eq!(out["retire"], json!([]));
    assert_eq!(f.rows(), (0, 0));
    assert_eq!(f.events(), 0);
}

#[test]
fn a_quote_found_in_neither_text_writes_nothing_and_is_counted() {
    let mut f = Fx::pair();
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Duplicate, "a phrase in neither record")),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["quote_failed"], json!(1));
    assert_eq!(out["retire"], json!([]));
    assert_eq!(f.rows(), (0, 0));
    assert_eq!(f.events(), 0);
}

#[test]
fn contradicts_is_reported_and_writes_no_retirement() {
    let mut f = Fx::pair();
    let p = ports(
        chosen("contradicts", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Contradicts, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(
        out["contradicts"],
        json!([{"neighbour": "hs-d1", "quote": QUOTE, "reason": "the same decision"}])
    );
    assert_eq!(out["retire"], json!([]));
    assert_eq!(f.rows(), (0, 0));
    assert_eq!(f.events(), 0);
}

#[test]
fn an_invalid_topic_from_the_judge_is_rejected_and_counted() {
    let mut f = Fx::pair();
    let mut bad = judged(Verdict::Duplicate, QUOTE);
    bad.topic = "Bad Topic!".into();
    let p = ports(chosen("duplicate", Some(0.9)), FakeJudge::answering(bad));
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["rejected"], json!(1));
    assert_eq!(out["retire"], json!([]));
    assert_eq!(f.rows(), (0, 0));
    assert_eq!(f.events(), 0);
}

#[test]
fn a_neighbour_that_already_has_a_curation_keeps_it() {
    let mut f = Fx::pair();
    f.sql(
        "INSERT INTO record_curations (context, record, topic, short, curated_at)
         VALUES ('ctx', 'hs-d1', 'old-topic', 'Old short', '2026-11-02T00:00:00.000Z');",
    );
    let p = ports(
        chosen("supersedes", Some(0.9)),
        FakeJudge::answering(judged(Verdict::Supersedes, QUOTE)),
    );
    f.run(&p, None).unwrap();
    assert_eq!(
        p.judge.calls()[0]
            .neighbour
            .as_ref()
            .unwrap()
            .topic
            .as_deref(),
        Some("old-topic")
    );
    assert_eq!(
        f.text("SELECT topic || '|' || short FROM record_curations WHERE record = 'hs-d1'"),
        "old-topic|Old short"
    );
    assert_eq!(
        f.count("SELECT count(*) FROM record_retirements WHERE record = 'hs-d1'"),
        1
    );
}

// ---- degraded modes and failure ---------------------------------------------------------------------------------

#[test]
fn an_unreachable_classifier_falls_back_to_the_judge_alone() {
    let mut f = Fx::pair();
    let p = ports(
        Err(code_err(ErrorCode::ClassifierUnreachable)),
        FakeJudge::answering(judged(Verdict::Duplicate, QUOTE)),
    );
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["kev_unreachable"], json!(true));
    assert_eq!(out["judged"], json!(1));
    assert_eq!(p.judge.calls().len(), 1);
    assert_eq!(out["retire"].as_array().unwrap().len(), 1);
    assert_eq!(f.rows(), (1, 1));
}

#[test]
fn an_unreachable_embedder_runs_fts_only_and_says_so() {
    let mut f = Fx::pair();
    let p = Ports {
        classifier: FakeClassifier::new(vec![chosen("independent", Some(0.9))]),
        embedding: FakeEmbedding::new(vec![Err(code_err(ErrorCode::EmbeddingUnreachable))]),
        judge: FakeJudge::new(vec![]),
    };
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["fts_only"], json!(true));
    assert_eq!(out["kev_unreachable"], json!(false));
    assert_eq!(out["neighbours"], json!(1));
    assert!(f.checked().contains(&id("hs-d2")));
}

#[test]
fn an_unreachable_judge_fails_the_run_and_leaves_the_record_unchecked() {
    let mut f = Fx::pair();
    let p = ports(
        chosen("duplicate", Some(0.9)),
        FakeJudge::new(vec![Err(code_err(ErrorCode::JudgeUnreachable))]),
    );
    let err = f.run(&p, None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::JudgeUnreachable);
    assert_eq!(f.rows(), (0, 0));
    assert_eq!(f.events(), 0);
    assert!(!f.checked().contains(&id("hs-d2")));
}

#[test]
fn a_finished_run_marks_the_record_checked_and_the_next_run_takes_the_next_record() {
    let mut f = fixture(true);
    f.add("hs-d1", 30 * HOURS, OLD_TEXT);
    f.add("hs-d2", 2 * HOURS, NEW_TEXT);
    f.add("hs-d3", HOURS, "Rotate the zebra keys monthly.");
    let p = ports(chosen("independent", Some(0.9)), FakeJudge::new(vec![]));
    let first = f.run(&p, None).unwrap();
    assert_eq!(first["record"], json!("hs-d2"));
    assert_eq!(first["remaining"], json!(1));
    let second = f.run(&p, None).unwrap();
    assert_eq!(second["record"], json!("hs-d3"));
    assert_eq!(second["remaining"], json!(0));
    let third = f.run(&p, None).unwrap();
    assert_eq!(
        third,
        json!({"context": "ctx", "already": true, "remaining": 0})
    );
    assert_eq!(f.checked(), HashSet::from([id("hs-d2"), id("hs-d3")]));
}

// ---- the neighbours ---------------------------------------------------------------------------------------------

#[test]
fn a_forced_neighbour_is_judged_whatever_its_score() {
    let mut f = fixture(true);
    f.add("hs-d1", 30 * HOURS, "Use Postgres for the store.");
    f.add("hs-d3", 30 * HOURS, "Rotate the zebra keys monthly.");
    f.add("hs-d4", HOURS, "Adopt SQLite instead, see hs-d3 first.");
    let p = Ports {
        classifier: FakeClassifier::new(vec![chosen("independent", Some(0.5))]),
        embedding: FakeEmbedding::new(vec![Err(code_err(ErrorCode::EmbeddingUnreachable))]),
        judge: FakeJudge::answering(judged(Verdict::Independent, "")),
    };
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["record"], json!("hs-d4"));
    assert_eq!(out["neighbours"], json!(1));
    let calls = p.judge.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].neighbour.as_ref().unwrap().id, id("hs-d3"));
    let state = &p.classifier.calls()[0].state;
    assert_eq!(state["new"]["id"], json!("hs-d4"));
    assert_eq!(state["neighbour"]["id"], json!("hs-d3"));
}

#[test]
fn the_run_embeds_the_new_record_and_at_most_25_missing_vectors_newest_first() {
    assert_eq!((DIMS, FILL), (768, 25));
    let mut f = fixture(true);
    let note = |i: i64| format!("Note number {i} concerns subject{i}.");
    for i in 1..=30 {
        f.add(&format!("hs-n{i}"), 48 * HOURS - i, &note(i));
    }
    f.add("hs-d1", HOURS, "Decision about the cache layout.");
    VectorStore::at(&f.home)
        .put(
            &context(),
            "m",
            &id("hs-n30"),
            &[1.0, 0.0, 0.0, 0.0],
            &at(0),
        )
        .unwrap();
    let p = ports(chosen("independent", Some(0.9)), FakeJudge::new(vec![]));
    f.run(&p, None).unwrap();
    let calls = p.embedding.calls();
    let mut expected = vec!["Decision about the cache layout.".to_owned()];
    expected.extend((5..=29).rev().map(note));
    assert_eq!(calls.len(), 26);
    assert_eq!(
        calls.iter().map(|c| c.0.clone()).collect::<Vec<_>>(),
        expected
    );
    assert!(calls.iter().all(|c| c.1 == 768));
}

#[test]
fn record_curates_the_named_record_whatever_its_age_and_marker() {
    let mut f = Fx::pair();
    f.mark("hs-d2");
    let p = ports(chosen("independent", Some(0.9)), FakeJudge::new(vec![]));
    let old = f.run(&p, Some(&id("hs-d1"))).unwrap();
    assert_eq!(old["record"], json!("hs-d1"));
    assert_eq!(old["neighbours"], json!(1));
    let marked = f.run(&p, Some(&id("hs-d2"))).unwrap();
    assert_eq!(marked["record"], json!("hs-d2"));
    assert!(f.checked().contains(&id("hs-d1")));
}

// ---- the provisional topic (commands.md §3.13a; decision pro-d11) -----------------------------------------------

const KEV_PAIR: &str = "independent";

/// Two old records curated `engine` and `store` (out of the window) and the new, uncurated `hs-d1`: a run picks `hs-d1`.
fn topics_fixture() -> Fx {
    let mut f = fixture(true);
    f.add("hs-n1", 30 * HOURS, "The engine ranks hits.");
    f.add("hs-n2", 31 * HOURS, "The store keeps rows.");
    f.sql(
        "INSERT INTO record_curations (context, record, topic, short, curated_at) VALUES
           ('ctx', 'hs-n1', 'engine', 'engine ranks', '2026-11-01T00:00:00.000Z'),
           ('ctx', 'hs-n2', 'store', 'store keeps', '2026-11-01T00:00:00.000Z');",
    );
    f.add("hs-d1", HOURS, NEW_TEXT);
    f
}

/// Kev accepts every pair as independent; the topic question (it offers `engine`) gets `topic`.
fn topic_ports(topic: Result<Chosen, CpdError>) -> Ports {
    Ports {
        classifier: FakeClassifier::routed(vec![("engine", topic)], chosen(KEV_PAIR, Some(0.9))),
        embedding: FakeEmbedding::new(vec![embedded()]),
        judge: FakeJudge::new(vec![]),
    }
}

fn topic_calls(p: &Ports) -> usize {
    p.classifier
        .calls()
        .iter()
        .filter(|c| c.labels.iter().any(|(name, _)| name == "engine"))
        .count()
}

fn provisional_rows(f: &mut Fx) -> i64 {
    f.count("SELECT count(*) FROM record_topics")
}

#[test]
fn an_accepted_independent_record_gets_a_provisional_topic() {
    let mut f = topics_fixture();
    let p = topic_ports(chosen("engine", Some(0.9)));
    f.run(&p, None).unwrap();
    assert_eq!(topic_calls(&p), 1);
    let labels: Vec<String> = p
        .classifier
        .calls()
        .into_iter()
        .find(|c| c.labels.iter().any(|(n, _)| n == "engine"))
        .unwrap()
        .labels
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(labels, ["engine", "store"]);
    assert_eq!(
        f.text("SELECT topic || '|' || model FROM record_topics WHERE context = 'ctx' AND record = 'hs-d1'"),
        "engine|kev-4b"
    );
    assert_eq!(provisional_rows(&mut f), 1);
    assert_eq!(
        f.count("SELECT count(*) FROM record_curations WHERE record = 'hs-d1'"),
        0
    );
    assert_eq!(f.events(), 0);
}

#[test]
fn a_low_confidence_label_writes_no_provisional_topic() {
    let mut f = topics_fixture();
    let p = topic_ports(chosen("engine", Some(0.5)));
    f.run(&p, None).unwrap();
    assert_eq!(provisional_rows(&mut f), 0);
    // the ask itself was made: only the confidence kept the row out
    assert_eq!(topic_calls(&p), 1);
}

#[test]
fn a_label_outside_the_topics_writes_no_provisional_topic() {
    let mut f = topics_fixture();
    let p = topic_ports(chosen("cache", Some(0.95)));
    f.run(&p, None).unwrap();
    assert_eq!(provisional_rows(&mut f), 0);
    assert_eq!(topic_calls(&p), 1);
}

#[test]
fn a_classifier_error_writes_nothing_and_the_run_succeeds() {
    let mut f = topics_fixture();
    let p = topic_ports(Err(code_err(ErrorCode::ClassifierUnreachable)));
    let out = f.run(&p, None).unwrap();
    assert_eq!(out["record"], json!("hs-d1"));
    assert_eq!(provisional_rows(&mut f), 0);
    assert_eq!(topic_calls(&p), 1);
    assert!(f.checked().contains(&id("hs-d1")));
}

#[test]
fn no_curated_topics_means_no_classifier_call() {
    let mut f = fixture(true);
    f.add("hs-d1", HOURS, NEW_TEXT);
    let p = topic_ports(chosen("engine", Some(0.9)));
    f.run(&p, None).unwrap();
    assert!(p.classifier.calls().is_empty());
    assert_eq!(provisional_rows(&mut f), 0);
}

#[test]
fn an_off_run_does_not_classify() {
    let mut f = topics_fixture();
    f.config = Config::parse(
        "version = 1\nmachine = \"hs\"\n[experimental]\nwrite_curation = false\n",
        Path::new("config.toml"),
    )
    .unwrap()
    .0;
    let p = topic_ports(chosen("engine", Some(0.9)));
    let out = f.run(&p, None).unwrap();
    assert_eq!(out, json!({"context": "ctx", "skipped": "off"}));
    assert!(p.classifier.calls().is_empty());
    assert_eq!(provisional_rows(&mut f), 0);
}

#[test]
fn a_curated_record_is_never_classified() {
    let mut f = topics_fixture();
    f.sql(
        "INSERT INTO record_curations (context, record, topic, short, curated_at)
           VALUES ('ctx', 'hs-d1', 'store', 'already curated', '2026-11-03T11:00:00.000Z');",
    );
    let p = topic_ports(chosen("engine", Some(0.9)));
    f.run(&p, None).unwrap();
    assert_eq!(topic_calls(&p), 0);
    assert_eq!(provisional_rows(&mut f), 0);
    assert_eq!(
        f.text("SELECT topic FROM record_curations WHERE record = 'hs-d1'"),
        "store"
    );
}

#[test]
fn a_record_with_a_provisional_topic_is_not_classified_again() {
    let mut f = topics_fixture();
    f.sql(
        "INSERT INTO record_topics (context, record, topic, model, at)
           VALUES ('ctx', 'hs-d1', 'store', 'kev-4b', '2026-11-03T11:00:00.000Z');",
    );
    let p = topic_ports(chosen("engine", Some(0.9)));
    f.run(&p, None).unwrap();
    assert_eq!(topic_calls(&p), 0);
    assert_eq!(provisional_rows(&mut f), 1);
    assert_eq!(
        f.text(
            "SELECT topic || '|' || model || '|' || at FROM record_topics WHERE record = 'hs-d1'"
        ),
        "store|kev-4b|2026-11-03T11:00:00.000Z"
    );
}
