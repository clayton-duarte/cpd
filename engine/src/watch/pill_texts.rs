//! Red tests for the engine-fed status texts of contract §6.1.1 (hs-d749, hs-d750): `workers Q·W·N/M` and the short `Q·W·N/M`,
//! queued, waiting, running and the limit, PLAIN: no style, no colour, a middle dot between the numbers.

use super::workers::Counts;

#[test]
fn workers_texts_are_queued_waiting_running_joined_by_middle_dots() {
    let a = Counts {
        running: 1,
        limit: 5,
        waiting: 0,
        queued: 5,
    };
    assert_eq!(a.long(), "workers 5·0·1/5");
    assert_eq!(a.short(), "5·0·1/5");
    let b = Counts {
        running: 4,
        limit: 10,
        waiting: 2,
        queued: 2,
    };
    assert_eq!(b.long(), "workers 2·2·4/10");
    assert_eq!(b.short(), "2·2·4/10");
    let c = Counts {
        running: 12,
        limit: 16,
        waiting: 103,
        queued: 1000,
    };
    assert_eq!(c.long(), "workers 1000·103·12/16");
    assert_eq!(c.short(), "1000·103·12/16");
}

#[test]
fn workers_texts_carry_no_style_or_labels() {
    for counts in [
        Counts {
            running: 1,
            limit: 5,
            waiting: 0,
            queued: 5,
        },
        Counts {
            running: 12,
            limit: 16,
            waiting: 103,
            queued: 1000,
        },
        Counts {
            running: 0,
            limit: 3,
            waiting: 0,
            queued: 0,
        },
    ] {
        for text in [counts.long(), counts.short()] {
            for bad in [
                "#", "[", "]", "colour", "dim", "default", "waiting", "queued",
            ] {
                assert!(!text.contains(bad), "{text:?} contains {bad:?}");
            }
        }
    }
}

/// Display columns: the texts have no directive, one cell per char.
fn columns(text: &str) -> usize {
    text.chars().count()
}

#[test]
fn workers_texts_are_15_and_7_columns_for_single_digit_counts() {
    let wide = Counts {
        running: 6,
        limit: 6,
        waiting: 4,
        queued: 9,
    };
    assert_eq!((columns(&wide.long()), columns(&wide.short())), (15, 7));
    let zero = Counts {
        running: 0,
        limit: 3,
        waiting: 0,
        queued: 1,
    };
    assert_eq!((columns(&zero.long()), columns(&zero.short())), (15, 7));
}
