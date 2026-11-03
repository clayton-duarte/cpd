//! The in-memory FTS5 seam: `rank` and `floor`.

use super::fts::{Doc, FLOOR, Scored, floor, rank};
use crate::error::ErrorCode;

fn doc(id: &str, topic: &str, short: &str, text: &str) -> Doc {
    Doc {
        id: id.into(),
        topic: topic.into(),
        short: short.into(),
        text: text.into(),
    }
}

fn ids(ranked: &[Scored]) -> Vec<&str> {
    ranked.iter().map(|s| s.id.as_str()).collect()
}

fn scored(id: &str, score: f64) -> Scored {
    Scored {
        id: id.into(),
        score,
    }
}

#[test]
fn rank_returns_the_record_whose_id_is_the_query_first() {
    let docs = [
        doc("hs-n1", "", "", "Mentions hs-d300 in passing."),
        doc("hs-d300", "", "", "The decision itself."),
        doc("hs-n2", "", "", "Also quotes hs-d300 and hs-d300 again."),
    ];
    let ranked = rank(&docs, " HS-D300 ").unwrap();
    assert_eq!(ranked[0].id, "hs-d300", "{ranked:?}");
    let top = ranked.iter().map(|s| s.score).fold(f64::MIN, f64::max);
    assert!(ranked[0].score >= top, "{ranked:?}");
}

#[test]
fn rank_matches_topic_short_and_text() {
    let docs = [
        doc("hs-n1", "zebra", "", "nothing here"),
        doc("hs-n2", "", "giraffe", "nothing here"),
        doc("hs-n3", "", "", "an okapi walks"),
        doc("hs-n4", "", "", "unrelated"),
    ];
    assert_eq!(ids(&rank(&docs, "zebra").unwrap()), ["hs-n1"]);
    assert_eq!(ids(&rank(&docs, "giraffe").unwrap()), ["hs-n2"]);
    assert_eq!(ids(&rank(&docs, "okapi").unwrap()), ["hs-n3"]);
}

#[test]
fn rank_applies_porter_stemming() {
    let docs = [
        doc("hs-n1", "", "", "The retrieval of facts."),
        doc("hs-n2", "", "", "Nothing relevant."),
    ];
    assert_eq!(ids(&rank(&docs, "retrieving").unwrap()), ["hs-n1"]);
}

#[test]
fn rank_puts_an_id_column_match_above_a_text_mention() {
    let docs = [
        doc("hs-n1", "", "", "alpha1 alpha1 alpha1 alpha1 alpha1"),
        doc("alpha1", "", "", "something else entirely"),
    ];
    let ranked = rank(&docs, "alpha1").unwrap();
    assert_eq!(ids(&ranked), ["alpha1", "hs-n1"], "{ranked:?}");
    assert!(ranked[0].score > ranked[1].score, "{ranked:?}");
}

#[test]
fn rank_of_no_docs_or_no_match_is_empty() {
    assert_eq!(rank(&[], "anything").unwrap(), vec![]);
    let docs = [doc("hs-n1", "", "", "some text")];
    assert_eq!(rank(&docs, "absent").unwrap(), vec![]);
}

#[test]
fn rank_refuses_a_malformed_phrase_as_usage() {
    let docs = [doc("hs-n1", "", "", "some text")];
    let error = rank(&docs, "\"unterminated").unwrap_err();
    assert_eq!(error.code(), ErrorCode::Usage);
    assert_eq!(error.details()["query"], "\"unterminated");
    assert!(error.details()["message"].is_string(), "{error:?}");
}

#[test]
fn rank_treats_punctuation_as_text() {
    let docs = [
        doc(
            "hs-n1",
            "",
            "",
            "see db.migrations and plan/key, don't panic",
        ),
        doc("hs-d655", "", "", "other"),
        doc("hs-n3", "", "", "unrelated"),
    ];
    for typed in ["db.migrations", "plan/key", "don't", "hs-d655"] {
        let ranked = rank(&docs, typed).unwrap();
        assert!(!ranked.is_empty(), "{typed}");
    }
    assert_eq!(ids(&rank(&docs, "db.migrations").unwrap()), ["hs-n1"]);
    assert_eq!(rank(&docs, "hs-d655").unwrap()[0].id, "hs-d655");
}

#[test]
fn floor_drops_hits_below_a_third_of_the_top_score() {
    let top = 10.0;
    let ranked = vec![
        scored("a", top),
        scored("b", top * FLOOR),
        scored("c", top * FLOOR - 1e-6),
        scored("d", 0.1),
    ];
    assert_eq!(ids(&floor(ranked, None)), ["a", "b"]);
}

#[test]
fn floor_keeps_the_exempt_id_even_when_it_is_below_the_floor() {
    let ranked = vec![scored("a", 10.0), scored("b", 1.0), scored("c", 0.5)];
    let kept = floor(ranked, Some("c"));
    assert_eq!(ids(&kept), ["a", "c"]);
}

#[test]
fn floor_keeps_everything_when_the_top_score_is_not_positive() {
    let ranked = vec![scored("a", 0.0), scored("b", -1.0)];
    assert_eq!(floor(ranked.clone(), None), ranked);
    let ranked = vec![scored("a", -1.0), scored("b", -50.0)];
    assert_eq!(floor(ranked.clone(), None), ranked);
}
