//! Tests of write-time curation: the kind gate, the screen and the confirmation rule.

use super::curate::{Judged, Verdict};
use super::curate_verdict::{
    GATE, Gate, Outcome, Reason, Screen, Screened, Side, Why, confirm, gate, screen,
};
use super::curation::Approval;
use crate::ids::{RecordId, RecordKind};

fn id(s: &str) -> RecordId {
    RecordId::parse(s).unwrap()
}

fn kev(verdict: Verdict, confidence: f32) -> Screen {
    Screen {
        verdict,
        confidence,
    }
}

fn judged(verdict: Verdict, quote: &str) -> Judged {
    Judged {
        verdict,
        quote: quote.into(),
        reason: "because".into(),
        topic: "the topic".into(),
        short: "the short".into(),
        usage: None,
    }
}

const NEW_TEXT: &str = "we use sqlite for the cache";
const OLD_TEXT: &str = "we use postgres for the cache";

struct Pair {
    new: RecordId,
    old: RecordId,
}

fn pair() -> Pair {
    Pair {
        new: id("hs-d9"),
        old: id("hs-d3"),
    }
}

fn run(new_seq: i64, old_seq: i64, k: Option<Screen>, j: Judged) -> Outcome {
    let p = pair();
    confirm(
        Side {
            id: &p.new,
            seq: new_seq,
            text: NEW_TEXT,
        },
        Side {
            id: &p.old,
            seq: old_seq,
            text: OLD_TEXT,
        },
        k,
        j,
    )
}

#[test]
fn gate_skips_plan_and_brief_with_no_port_call() {
    assert_eq!(gate(RecordKind::Plan), Gate::Skip);
    assert_eq!(gate(RecordKind::Brief), Gate::Skip);
}

#[test]
fn gate_proceeds_for_decision_and_note() {
    assert_eq!(gate(RecordKind::Decision), Gate::Neighbours);
    assert_eq!(gate(RecordKind::Note), Gate::Neighbours);
}

#[test]
fn screen_accepts_independent_at_or_above_the_gate() {
    assert_eq!(
        screen(Ok(kev(Verdict::Independent, GATE))),
        Screened::AcceptIndependent
    );
    assert_eq!(
        screen(Ok(kev(Verdict::Independent, 0.99))),
        Screened::AcceptIndependent
    );
}

#[test]
fn screen_escalates_independent_below_the_gate() {
    let k = kev(Verdict::Independent, 0.79);
    assert_eq!(screen(Ok(k)), Screened::Escalate(Some(k)));
}

#[test]
fn screen_escalates_every_retire_class_and_contradicts_result() {
    for v in [
        Verdict::Duplicate,
        Verdict::Supersedes,
        Verdict::Contradicts,
    ] {
        let k = kev(v, 0.99);
        assert_eq!(screen(Ok(k)), Screened::Escalate(Some(k)));
    }
}

#[test]
fn screen_escalates_when_the_classifier_is_unreachable() {
    assert_eq!(screen(Err(())), Screened::Escalate(None));
}

#[test]
fn confirm_retires_when_classifier_and_judge_agree_and_the_quote_is_verbatim() {
    let out = run(
        9,
        3,
        Some(kev(Verdict::Duplicate, 0.9)),
        judged(Verdict::Duplicate, "for the cache"),
    );
    assert!(matches!(out, Outcome::Retire { .. }), "{out:?}");
}

#[test]
fn confirm_turns_a_disagreement_into_independent() {
    let out = run(
        9,
        3,
        Some(kev(Verdict::Supersedes, 0.9)),
        judged(Verdict::Duplicate, "for the cache"),
    );
    assert_eq!(
        out,
        Outcome::Independent {
            why: Some(Why::Disagreed)
        }
    );
    let low = run(
        9,
        3,
        Some(kev(Verdict::Independent, 0.5)),
        judged(Verdict::Duplicate, "for the cache"),
    );
    assert_eq!(
        low,
        Outcome::Independent {
            why: Some(Why::Disagreed)
        }
    );
}

#[test]
fn confirm_turns_a_quote_found_in_neither_text_into_independent() {
    for quote in ["use mysql", "", "   "] {
        let out = run(
            9,
            3,
            Some(kev(Verdict::Duplicate, 0.9)),
            judged(Verdict::Duplicate, quote),
        );
        assert_eq!(
            out,
            Outcome::Independent {
                why: Some(Why::QuoteNotFound)
            },
            "quote {quote:?}"
        );
    }
}

#[test]
fn confirm_accepts_the_judge_alone_when_the_classifier_was_unreachable() {
    let ok = run(9, 3, None, judged(Verdict::Duplicate, "postgres"));
    assert!(matches!(ok, Outcome::Retire { .. }), "{ok:?}");
    let bad = run(9, 3, None, judged(Verdict::Duplicate, "mongo"));
    assert_eq!(
        bad,
        Outcome::Independent {
            why: Some(Why::QuoteNotFound)
        }
    );
}

#[test]
fn confirm_duplicate_retires_the_new_record_and_keeps_the_older() {
    let p = pair();
    let out = run(9, 3, None, judged(Verdict::Duplicate, "sqlite"));
    assert_eq!(
        out,
        Outcome::Retire {
            record: p.new.clone(),
            target: p.old.clone(),
            reason: Reason::Duplicate,
            topic: "the topic".into(),
            short: "the short".into(),
            approval: Approval::Pending,
        }
    );
    // The neighbour is the newer one: it is retired, the new record is kept.
    let out = run(3, 9, None, judged(Verdict::Duplicate, "sqlite"));
    assert_eq!(
        out,
        Outcome::Retire {
            record: p.old,
            target: p.new,
            reason: Reason::Duplicate,
            topic: "the topic".into(),
            short: "the short".into(),
            approval: Approval::Pending,
        }
    );
}

#[test]
fn confirm_supersedes_retires_the_older_neighbour() {
    let p = pair();
    let out = run(9, 3, None, judged(Verdict::Supersedes, "sqlite"));
    assert_eq!(
        out,
        Outcome::Retire {
            record: p.old,
            target: p.new,
            reason: Reason::Superseded,
            topic: "the topic".into(),
            short: "the short".into(),
            approval: Approval::Pending,
        }
    );
}

#[test]
fn confirm_supersedes_onto_a_newer_neighbour_is_rejected() {
    for (n, o) in [(3, 9), (5, 5)] {
        let out = run(n, o, None, judged(Verdict::Supersedes, "sqlite"));
        assert!(
            matches!(
                out,
                Outcome::Independent {
                    why: Some(Why::Rejected(_))
                }
            ),
            "{out:?}"
        );
    }
}

#[test]
fn confirm_contradicts_retires_nothing_and_is_reported() {
    let p = pair();
    let out = run(
        9,
        3,
        Some(kev(Verdict::Contradicts, 0.9)),
        judged(Verdict::Contradicts, "postgres"),
    );
    assert_eq!(
        out,
        Outcome::Contradicts {
            neighbour: p.old,
            quote: "postgres".into(),
            reason: "because".into(),
        }
    );
}

#[test]
fn confirm_independent_from_the_judge_stays_independent() {
    for k in [
        None,
        Some(kev(Verdict::Independent, 0.9)),
        Some(kev(Verdict::Duplicate, 0.9)),
    ] {
        let out = run(9, 3, k, judged(Verdict::Independent, ""));
        assert_eq!(out, Outcome::Independent { why: None });
    }
}

#[test]
fn agreement_with_kev_retires_with_models_approval() {
    let out = run(
        9,
        3,
        Some(kev(Verdict::Duplicate, 0.9)),
        judged(Verdict::Duplicate, "for the cache"),
    );
    let p = pair();
    assert_eq!(
        out,
        Outcome::Retire {
            record: p.new,
            target: p.old,
            reason: Reason::Duplicate,
            topic: "the topic".into(),
            short: "the short".into(),
            approval: Approval::Models,
        }
    );
}

#[test]
fn supersedes_with_kev_agreement_retires_with_models_approval() {
    let out = run(
        9,
        3,
        Some(kev(Verdict::Supersedes, 0.9)),
        judged(Verdict::Supersedes, "sqlite"),
    );
    let p = pair();
    assert_eq!(
        out,
        Outcome::Retire {
            record: p.old,
            target: p.new,
            reason: Reason::Superseded,
            topic: "the topic".into(),
            short: "the short".into(),
            approval: Approval::Models,
        }
    );
}

#[test]
fn no_kev_retires_pending_approval() {
    let out = run(9, 3, None, judged(Verdict::Duplicate, "postgres"));
    assert!(
        matches!(
            out,
            Outcome::Retire {
                approval: Approval::Pending,
                ..
            }
        ),
        "{out:?}"
    );
}
