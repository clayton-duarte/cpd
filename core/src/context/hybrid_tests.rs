//! The hybrid seam: `semantic_list` and `rrf`.

use super::fts::Scored;
use super::hybrid::{SEM_FLOOR, SEM_TOP, rrf, semantic_list};

fn scored(id: &str, score: f64) -> Scored {
    Scored {
        id: id.into(),
        score,
    }
}

fn ids(list: &[Scored]) -> Vec<&str> {
    list.iter().map(|s| s.id.as_str()).collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

#[test]
fn semantic_list_drops_cosines_below_the_floor_and_keeps_the_top_25() {
    assert!(near(SEM_FLOOR, 0.60));
    assert_eq!(SEM_TOP, 25);
    let query = [1.0_f32, 0.0];
    // Thirty vectors whose cosine falls with k, all far above the floor.
    let mut vectors: Vec<(String, Vec<f32>)> = (0..30)
        .map(|k| (format!("r{k:02}"), vec![1.0, 0.01 * k as f32]))
        .collect();
    // Exactly on the floor (3/5) is kept in the boundary run below; here, one far below.
    vectors.push(("far".into(), vec![0.0, 1.0]));
    let list = semantic_list(&query, &vectors);
    let expected: Vec<String> = (0..25).map(|k| format!("r{k:02}")).collect();
    assert_eq!(
        ids(&list),
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert!(near(list[0].score, 1.0), "{list:?}");
    assert!(list.windows(2).all(|w| w[0].score >= w[1].score));

    // The boundary: 0.60 is kept, just below is dropped.
    let edge = [
        ("at".to_owned(), vec![3.0_f32, 4.0]),
        ("under".to_owned(), vec![29.0_f32, 40.0]),
    ];
    let list = semantic_list(&query, &edge);
    assert_eq!(ids(&list), ["at"], "{list:?}");
    assert!(near(list[0].score, 0.6), "{list:?}");
}

#[test]
fn semantic_list_skips_vectors_of_another_dimension() {
    let query = [1.0_f32, 0.0];
    let vectors = [
        ("two".to_owned(), vec![1.0_f32, 0.0]),
        ("three".to_owned(), vec![1.0_f32, 0.0, 0.0]),
        ("one".to_owned(), vec![1.0_f32]),
    ];
    let list = semantic_list(&query, &vectors);
    assert_eq!(ids(&list), ["two"], "{list:?}");
}

#[test]
fn rrf_fuses_two_lists_with_constant_sixty() {
    let lexical = [scored("a", 9.0), scored("b", 5.0), scored("c", 1.0)];
    let semantic = [scored("c", 0.9), scored("d", 0.8)];
    let fused = rrf(&lexical, &semantic, None);
    // c: 1/63 + 1/61; a: 1/61; b and d: 1/62, tied, so by id.
    assert_eq!(ids(&fused), ["c", "a", "b", "d"], "{fused:?}");
    let want = [1.0 / 63.0 + 1.0 / 61.0, 1.0 / 61.0, 1.0 / 62.0, 1.0 / 62.0];
    for (hit, want) in fused.iter().zip(want) {
        assert!(near(hit.score, want), "{fused:?}");
    }
}

#[test]
fn rrf_ranks_a_hit_found_by_both_lists_above_one_found_by_one() {
    let lexical = [scored("x", 9.0), scored("y", 5.0)];
    let semantic = [scored("z", 0.9), scored("y", 0.8)];
    let fused = rrf(&lexical, &semantic, None);
    assert_eq!(ids(&fused), ["y", "x", "z"], "{fused:?}");
}

#[test]
fn rrf_moves_the_id_named_record_first() {
    let lexical = [scored("a", 9.0), scored("b", 5.0)];
    let semantic = [scored("a", 0.9), scored("low", 0.7)];
    let fused = rrf(&lexical, &semantic, Some("low"));
    assert_eq!(ids(&fused), ["low", "a", "b"], "{fused:?}");
    // Absent from both lists: nothing is added or moved.
    let fused = rrf(&lexical, &semantic, Some("nowhere"));
    assert_eq!(ids(&fused), ["a", "b", "low"], "{fused:?}");
}

#[test]
fn rrf_with_an_empty_semantic_list_keeps_the_lexical_order() {
    let lexical = [scored("b", 9.0), scored("a", 3.0)];
    let fused = rrf(&lexical, &[], None);
    assert_eq!(ids(&fused), ["b", "a"], "{fused:?}");
    assert!(near(fused[0].score, 1.0 / 61.0), "{fused:?}");
    assert!(near(fused[1].score, 1.0 / 62.0), "{fused:?}");
}
