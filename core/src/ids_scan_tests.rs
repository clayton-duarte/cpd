//! `RecordId::scan` (decision pro-d10 item 5): the record ids in free text, with boundaries.

use crate::ids::{RecordId, RecordKind};

fn scan(text: &str) -> Vec<String> {
    RecordId::scan(text)
        .iter()
        .map(|id| id.to_string())
        .collect()
}

#[test]
fn scan_finds_ids_with_punctuation_around_them() {
    assert_eq!(scan("see pro-d10, pro-n7."), ["pro-d10", "pro-n7"]);
    assert_eq!(scan("(hs-d3)"), ["hs-d3"]);
    assert_eq!(scan("**pro-d1**"), ["pro-d1"]);
    assert_eq!(scan("hs-p2"), ["hs-p2"]);
    assert_eq!(scan("a\nhs-b4\n"), ["hs-b4"]);
    assert_eq!(scan("`hs-d3`: x"), ["hs-d3"]);
}

#[test]
fn scan_rejects_boundary_and_malformed_tokens() {
    for text in [
        "foo-hs-d3",
        "hs-d3x",
        "hs-d03",
        "hs-d0",
        "HS-d3",
        "hs-x3",
        "hs_d3",
        "1hs-d3",
        "_hs-d3",
        "hs-d3_",
        "hs-d3a",
        "hs-d",
        "hs-",
        "hs-d-3",
    ] {
        assert_eq!(scan(text), Vec::<String>::new(), "{text:?}");
    }
    assert_eq!(scan(""), Vec::<String>::new());
}

#[test]
fn scan_dedupes_in_order_of_first_appearance() {
    assert_eq!(
        scan("hs-n2 then hs-d1, hs-n2 again, hs-d1 and hs-b3"),
        ["hs-n2", "hs-d1", "hs-b3"]
    );
}

#[test]
fn scan_ignores_letters_outside_record_kinds() {
    assert!(RecordKind::from_letter('x').is_none());
    assert_eq!(scan("hs-x1 and hs-d1"), ["hs-d1"]);
    for kind in [
        RecordKind::Decision,
        RecordKind::Note,
        RecordKind::Plan,
        RecordKind::Brief,
    ] {
        assert!(RecordKind::ALL.contains(&kind));
        let text = format!("see hs-{}5.", kind.letter());
        assert_eq!(scan(&text), [format!("hs-{}5", kind.letter())]);
    }
}
