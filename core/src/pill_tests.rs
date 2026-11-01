use crate::pill::{Spec, spec};

const SHIPPED: &str = include_str!("../../pi/lib/pill-spec.json");

fn err(json: &str) -> String {
    Spec::parse(json).expect_err("parse must be rejected")
}

#[test]
fn pill_spec_shipped_file_has_the_pinned_caps_meanings_and_clickable_rule() {
    let s = spec();
    assert_eq!(s.caps.left, "\u{e0b6}");
    assert_eq!(s.caps.right, "\u{e0b4}");
    let m = &s.meanings;
    assert_eq!(
        [
            m.plain,
            m.in_progress,
            m.review,
            m.done,
            m.failed,
            m.planning,
            m.nothing,
            m.focus
        ],
        [7, 6, 3, 2, 1, 5, 8, 4]
    );
    assert_eq!(
        (s.clickable.add, s.clickable.from, s.clickable.to),
        (8, 1, 7)
    );
}

#[test]
fn pill_spec_rejects_an_unknown_key_naming_it() {
    let top = SHIPPED.replacen("{\n", "{\n  \"bogus_top\": 1,\n", 1);
    assert!(err(&top).contains("bogus_top"));
    let inner = SHIPPED.replacen(
        "\"meanings\": {\n",
        "\"meanings\": {\n    \"bogus_inner\": 1,\n",
        1,
    );
    assert!(err(&inner).contains("bogus_inner"));
}

#[test]
fn pill_spec_rejects_a_missing_key_naming_it() {
    let no_focus = SHIPPED
        .replace("\"focus\": 4", "\"x\": 4")
        .replace("\"x\": 4", "");
    let no_focus = no_focus.replace("\"nothing\": 8,", "\"nothing\": 8");
    assert!(err(&no_focus).contains("focus"));
    let start = SHIPPED.find("\"caps\"").unwrap();
    let end = SHIPPED.find("\"clickable\"").unwrap();
    let no_caps = format!("{}{}", &SHIPPED[..start], &SHIPPED[end..]);
    assert!(err(&no_caps).contains("caps"));
}

#[test]
fn pill_spec_rejects_index_zero() {
    let e = err(&SHIPPED.replace("\"focus\": 4", "\"focus\": 0"));
    assert!(e.contains("focus") && e.contains('0'), "{e}");
}

#[test]
fn pill_spec_rejects_an_index_above_15() {
    let e = err(&SHIPPED.replace("\"focus\": 4", "\"focus\": 16"));
    assert!(e.contains("focus") && e.contains("16"), "{e}");
}

#[test]
fn pill_spec_clickable_adds_8_to_1_through_7_and_keeps_the_rest() {
    let s = Spec::parse(SHIPPED).unwrap();
    for (n, want) in [
        (1, 9),
        (2, 10),
        (3, 11),
        (4, 12),
        (5, 13),
        (6, 14),
        (7, 15),
        (8, 8),
        (9, 9),
        (15, 15),
    ] {
        assert_eq!(s.clickable(n), want, "clickable({n})");
    }
}
