//! Red test for the engine-fed status texts of contract §6.1.1 (hs-d530 R1): the pill's `fg=colour7` is the only text colour,
//! so `@cpd_workers` and `@cpd_workers_short` carry no state colour and no `dim`.

use super::workers::Counts;

/// Every `#[…]` of `text` is `#[fg=colour7]`, the one colour a pill text may name (the docs say the texts carry no style at all).
fn assert_colour7_only(what: &str, text: &str) {
    let mut rest = text;
    while let Some(at) = rest.find("#[") {
        let end = rest[at..].find(']').expect("a style closes") + at;
        let style = &rest[at..=end];
        assert_eq!(style, "#[fg=colour7]", "{what}: {text:?} names a style other than colour 7");
        rest = &rest[end + 1..];
    }
    assert!(!text.contains("dim"), "{what}: {text:?} is dim");
}

#[test]
fn workers_texts_are_colour7_only() {
    let counts = Counts { running: 6, limit: 6, waiting: 4, queued: 9 };
    assert_colour7_only("workers long", &counts.long());
    assert_colour7_only("workers short", &counts.short());
}
