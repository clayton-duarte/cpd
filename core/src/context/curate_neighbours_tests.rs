//! Tests of write-time curation: neighbours.

use super::curate::Candidate;
use super::curate_neighbours::*;
use crate::ids::{RecordId, RecordKind};

fn id(s: &str) -> RecordId {
    RecordId::parse(s).unwrap()
}

fn ids(v: &[&str]) -> Vec<RecordId> {
    v.iter().map(|s| id(s)).collect()
}

fn cand(i: &str, seq: i64, text: &str) -> Candidate {
    Candidate {
        id: id(i),
        seq,
        kind: RecordKind::Decision,
        at: "2026-11-03T12:00:00Z".to_string(),
        text: text.to_string(),
    }
}

fn pool() -> Vec<Candidate> {
    vec![
        cand("pro-d1", 1, "alpha"),
        cand("pro-d2", 2, "beta"),
        cand("hs-n12", 3, "gamma"),
        cand("pro-d366", 4, "delta"),
    ]
}

#[test]
fn forced_ids_include_every_in_force_id_named_in_the_text() {
    let got = forced_ids(
        "see hs-n12, then pro-d366. Also pro-d1 and hs-n12 again",
        &[],
        &id("pro-d9"),
        &pool(),
    );
    assert_eq!(got, ids(&["hs-n12", "pro-d366", "pro-d1"]));
}

#[test]
fn forced_ids_put_the_supersedes_targets_first_in_order() {
    let got = forced_ids(
        "mentions pro-d1 and hs-n12",
        &ids(&["pro-d366", "pro-d2"]),
        &id("pro-d9"),
        &pool(),
    );
    assert_eq!(got, ids(&["pro-d366", "pro-d2", "pro-d1", "hs-n12"]));
}

#[test]
fn forced_ids_ignore_unknown_and_out_of_force_ids() {
    let got = forced_ids(
        "pro-d77 hs-n99 pro-d1 xpro-d2 pro-d22",
        &ids(&["pro-d500"]),
        &id("pro-d9"),
        &pool(),
    );
    assert_eq!(got, ids(&["pro-d1"]));
}

#[test]
fn forced_ids_ignore_the_record_itself() {
    let got = forced_ids("pro-d1 pro-d2", &ids(&["pro-d1"]), &id("pro-d1"), &pool());
    assert_eq!(got, ids(&["pro-d2"]));
}

#[test]
fn lexical_rank_orders_by_bm25_over_in_force_candidates_only() {
    let in_force = vec![
        cand("pro-d1", 1, "zebra stripes are unique"),
        cand("pro-d2", 2, "zebra zebra zebra zebra stripes"),
        cand("pro-d3", 3, "nothing relevant at all"),
    ];
    let got = lexical_rank("zebra stripes", &in_force, TOP).unwrap();
    assert_eq!(got.len(), 2);
    assert!(!got.contains(&id("pro-d3")));
    assert!(!got.contains(&id("pro-d4")));
    assert_eq!(lexical_rank("zebra", &in_force, 1).unwrap().len(), 1);
}

#[test]
fn lexical_rank_survives_fts5_syntax_in_the_text() {
    let in_force = vec![
        cand("pro-d1", 1, "it quoted a b c d e f g"),
        cand("pro-d2", 2, "unrelated words"),
    ];
    let got = lexical_rank("it's \"quoted\" a.b/c d-e:f* (g)", &in_force, TOP).unwrap();
    assert_eq!(got.first(), Some(&id("pro-d1")));
    let got = lexical_rank("hs-d655", &[cand("pro-d1", 1, "hs d655 here")], TOP).unwrap();
    assert_eq!(got, ids(&["pro-d1"]));
}

#[test]
fn lexical_rank_uses_the_twenty_rarest_terms() {
    // 21 terms: t01..t20 appear in one candidate each, "common" in all.
    let mut in_force = vec![cand("pro-d1", 1, "only common")];
    in_force.push(cand("pro-d2", 2, "common filler"));
    let rare: Vec<String> = (1..=20).map(|n| format!("rare{n:02}")).collect();
    in_force.push(cand("pro-d3", 3, &rare.join(" ")));
    // The 21st term, "common", is the most frequent, so it is dropped:
    // pro-d1 and pro-d2 match only through it and must not be returned.
    let query = format!("{} common", rare.join(" "));
    let got = lexical_rank(&query, &in_force, TOP).unwrap();
    assert_eq!(got, ids(&["pro-d3"]));
}

#[test]
fn lexical_rank_of_a_text_with_no_terms_is_empty() {
    let got = lexical_rank("\"\" -- ... ::: *", &pool(), TOP).unwrap();
    assert!(got.is_empty());
    assert!(lexical_rank("", &pool(), TOP).unwrap().is_empty());
}

#[test]
fn semantic_rank_orders_by_cosine_and_skips_other_dimensions() {
    let vectors = vec![
        (id("pro-d1"), vec![0.0, 1.0]),
        (id("pro-d2"), vec![1.0, 0.0]),
        (id("pro-d3"), vec![1.0, 0.0, 0.0]),
        (id("pro-d4"), vec![1.0, 1.0]),
        (id("pro-d5"), vec![2.0, 0.0]),
    ];
    let got = semantic_rank(&[1.0, 0.0], &vectors, TOP);
    assert_eq!(got, ids(&["pro-d2", "pro-d5", "pro-d4", "pro-d1"]));
    assert_eq!(
        semantic_rank(&[1.0, 0.0], &vectors, 2),
        ids(&["pro-d2", "pro-d5"])
    );
}

#[test]
fn fuse_ranks_by_reciprocal_rank_with_constant_sixty_and_breaks_ties_by_id() {
    assert_eq!(RRF_C, 60.0);
    // a: 1/61+1/62, b: 1/62+1/61 (tie with a, by id), c: 1/63, d: 1/63 (tie)
    let lists = vec![
        ids(&["pro-d1", "pro-d2", "pro-d3"]),
        ids(&["pro-d2", "pro-d1", "pro-d4"]),
    ];
    assert_eq!(fuse(&lists), ids(&["pro-d1", "pro-d2", "pro-d3", "pro-d4"]));
    // Rank 1 in one list beats rank 2 in another.
    let lists = vec![ids(&["pro-d9", "pro-d1"]), ids(&["pro-d5"])];
    assert_eq!(fuse(&lists), ids(&["pro-d5", "pro-d9", "pro-d1"]));
    assert!(fuse(&[]).is_empty());
}

#[test]
fn neighbours_put_forced_ids_first_and_cap_at_eight() {
    assert_eq!(K, 8);
    let lexical: Vec<RecordId> = (10..30).map(|n| id(&format!("pro-d{n}"))).collect();
    let got = neighbours(&id("pro-d99"), ids(&["pro-d2", "pro-d1"]), lexical, None);
    assert_eq!(got.len(), K);
    assert_eq!(
        got[0],
        Neighbour {
            id: id("pro-d2"),
            forced: true
        }
    );
    assert_eq!(
        got[1],
        Neighbour {
            id: id("pro-d1"),
            forced: true
        }
    );
    assert!(got[2..].iter().all(|n| !n.forced));
    assert_eq!(got[2].id, id("pro-d10"));
}

#[test]
fn neighbours_fall_back_to_lexical_only_when_semantic_is_none() {
    let lexical = ids(&["pro-d3", "pro-d1", "pro-d2"]);
    let got = neighbours(&id("pro-d99"), vec![], lexical.clone(), None);
    let want: Vec<Neighbour> = lexical
        .into_iter()
        .map(|id| Neighbour { id, forced: false })
        .collect();
    assert_eq!(got, want);
    let fused = neighbours(
        &id("pro-d99"),
        vec![],
        ids(&["pro-d1"]),
        Some(ids(&["pro-d2"])),
    );
    assert_eq!(fused.len(), 2);
}

#[test]
fn neighbours_never_include_the_record_itself() {
    let me = id("pro-d5");
    let got = neighbours(
        &me,
        ids(&["pro-d5", "pro-d1"]),
        ids(&["pro-d5", "pro-d1", "pro-d2"]),
        Some(ids(&["pro-d5", "pro-d2"])),
    );
    assert!(got.iter().all(|n| n.id != me));
    let mut seen = got.iter().map(|n| n.id.clone()).collect::<Vec<_>>();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), got.len());
    assert!(got[0].forced && got[0].id == id("pro-d1"));
}
