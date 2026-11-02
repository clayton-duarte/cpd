use super::footprint::*;
use crate::plan::shared::is_shared;
use serde_json::json;

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn no(_: &str) -> bool {
    false
}

fn fp(class: FootprintClass, undeclared: &[&str], untouched: &[&str]) -> Footprint {
    Footprint {
        class,
        undeclared: v(undeclared),
        untouched: v(untouched),
    }
}

use FootprintClass::{Match, Subset, UndeclaredWrite};

#[test]
fn equal_sets_match() {
    let got = classify(&v(&["a.rs", "docs/x.md"]), &v(&["docs/x.md", "a.rs"]), &no);
    assert_eq!(got, fp(Match, &[], &[]));
}

#[test]
fn untouched_declared_entry_is_a_subset() {
    let got = classify(&v(&["a.rs", "b.rs"]), &v(&["a.rs"]), &no);
    assert_eq!(got, fp(Subset, &[], &["b.rs"]));
}

#[test]
fn a_changed_file_outside_touches_is_undeclared_write() {
    let got = classify(&v(&["a.rs"]), &v(&["a.rs", "cli/tests/pr/main.rs"]), &no);
    assert_eq!(got, fp(UndeclaredWrite, &["cli/tests/pr/main.rs"], &[]));
}

#[test]
fn a_directory_entry_covers_files_below_it() {
    let got = classify(&v(&["docs/"]), &v(&["docs/a.md", "docs/x/b.md"]), &no);
    assert_eq!(got, fp(Match, &[], &[]));
    let got = classify(&v(&["docs/"]), &v(&["docs.md"]), &no);
    assert_eq!(got, fp(UndeclaredWrite, &["docs.md"], &["docs/"]));
}

#[test]
fn lockfiles_never_make_undeclared_write() {
    let got = classify(
        &v(&["a.rs"]),
        &v(&["a.rs", "Cargo.lock", "web/pnpm-lock.yaml"]),
        &is_shared,
    );
    assert_eq!(got, fp(Match, &[], &[]));
    let got = classify(&v(&["a.rs"]), &v(&["Cargo.lock"]), &is_shared);
    assert_eq!(got, fp(Subset, &[], &["a.rs"]));
    let got = classify(&v(&["a.rs"]), &v(&["a.rs", "Cargo.lock"]), &no);
    assert_eq!(got, fp(UndeclaredWrite, &["Cargo.lock"], &[]));
}

#[test]
fn a_rename_counts_both_paths() {
    let actual = v(&["old/a.rs", "new/a.rs"]);
    let got = classify(&v(&["new/a.rs"]), &actual, &no);
    assert_eq!(got, fp(UndeclaredWrite, &["old/a.rs"], &[]));
    let got = classify(&v(&["old/a.rs", "new/a.rs"]), &actual, &no);
    assert_eq!(got, fp(Match, &[], &[]));
}

#[test]
fn a_glob_entry_covers_the_paths_it_matches() {
    let got = classify(
        &v(&["cli/tests/**", "core/src/*.rs"]),
        &v(&["cli/tests/flow/a.rs", "core/src/lib.rs"]),
        &no,
    );
    assert_eq!(got, fp(Match, &[], &[]));
    let got = classify(&v(&["core/src/*.rs"]), &v(&["core/src/x/y.rs"]), &no);
    assert_eq!(
        got,
        fp(UndeclaredWrite, &["core/src/x/y.rs"], &["core/src/*.rs"])
    );
}

#[test]
fn undeclared_keeps_the_order_of_the_actual_paths() {
    let got = classify(&v(&["a"]), &v(&["z.rs", "a", "m.rs", "b.rs"]), &no);
    assert_eq!(got, fp(UndeclaredWrite, &["z.rs", "m.rs", "b.rs"], &[]));
}

#[test]
fn untouched_keeps_the_order_of_touches_and_lists_an_entry_once() {
    let got = classify(&v(&["c", "a", "c", "b"]), &v(&["a"]), &no);
    assert_eq!(got, fp(Subset, &[], &["c", "b"]));
}

#[test]
fn a_written_shared_file_counts_as_touching_its_entry() {
    let touches = v(&["Cargo.lock", "a.rs"]);
    let got = classify(&touches, &v(&["a.rs", "Cargo.lock"]), &is_shared);
    assert_eq!(got, fp(Match, &[], &[]));
    let got = classify(&touches, &v(&["a.rs"]), &is_shared);
    assert_eq!(got, fp(Subset, &[], &["Cargo.lock"]));
}

#[test]
fn an_empty_actual_with_touches_is_a_subset() {
    let got = classify(&v(&["a", "b/"]), &[], &no);
    assert_eq!(got, fp(Subset, &[], &["a", "b/"]));
}

#[test]
fn nothing_declared_and_nothing_written_is_a_match() {
    assert_eq!(classify(&[], &[], &no), fp(Match, &[], &[]));
}

#[test]
fn undeclared_write_wins_over_untouched() {
    let got = classify(&v(&["a", "b"]), &v(&["x"]), &no);
    assert_eq!(got, fp(UndeclaredWrite, &["x"], &["a", "b"]));
}

#[test]
fn the_json_shape_is_class_undeclared_untouched() {
    let got = classify(&v(&["a"]), &v(&["x"]), &no);
    assert_eq!(
        serde_json::to_value(&got).unwrap(),
        json!({"class": "undeclared_write", "undeclared": ["x"], "untouched": ["a"]})
    );
    let classes = [
        (classify(&v(&["a"]), &v(&["a"]), &no), "match"),
        (classify(&v(&["a", "b"]), &v(&["a"]), &no), "subset"),
    ];
    for (footprint, name) in classes {
        assert_eq!(serde_json::to_value(footprint.class).unwrap(), json!(name));
    }
}
