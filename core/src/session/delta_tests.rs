//! Red tests for the op applier `delta::apply` (contract §6.20): the vectors of the table, one test per
//! op, and one per refusal. Red until the implementation Job writes the applier.

use serde_json::{Value, json};

use super::delta::apply;

fn base() -> Value {
    json!({"a": {"text": "he"}, "list": [{"id": 1}]})
}

fn one(value: Value, op: Value) -> Value {
    apply(value, &[op]).expect("the op applies")
}

fn refused(op: Value) {
    let value = base();
    assert!(
        apply(value, std::slice::from_ref(&op)).is_err(),
        "{op} must be refused"
    );
}

#[test]
fn delta_append_extends_a_string() {
    let out = one(base(), json!(["a", ["a", "text"], "llo"]));
    assert_eq!(out["a"]["text"], json!("hello"));
}

#[test]
fn delta_trim_drops_leading_characters() {
    let out = one(
        json!({"a": {"text": "hello"}}),
        json!(["t", ["a", "text"], 1]),
    );
    assert_eq!(out["a"]["text"], json!("ello"));
    // JS `slice` counts UTF-16 units: a non-BMP character is two.
    let out = one(json!({"s": "😀x"}), json!(["t", ["s"], 2]));
    assert_eq!(out["s"], json!("x"));
}

#[test]
fn delta_splice_inserts_and_removes_items() {
    let out = one(base(), json!(["p", ["list"], 1, 0, [{"id": 2}, {"id": 3}]]));
    assert_eq!(out["list"], json!([{"id": 1}, {"id": 2}, {"id": 3}]));
    let out = one(out, json!(["p", ["list"], 0, 2, []]));
    assert_eq!(out["list"], json!([{"id": 3}]));
}

#[test]
fn delta_splice_at_the_root_of_an_array() {
    let out = one(json!([1, 2, 3]), json!(["p", [], 1, 1, [9, 8]]));
    assert_eq!(out, json!([1, 9, 8, 3]));
}

#[test]
fn delta_reorder_permutes_an_array() {
    let out = one(
        json!({"list": [{"id": 1}, {"id": 2}, {"id": 3}]}),
        json!(["m", ["list"], [2, 0, 1]]),
    );
    assert_eq!(out["list"], json!([{"id": 3}, {"id": 1}, {"id": 2}]));
}

#[test]
fn delta_set_adds_an_object_key() {
    let out = one(base(), json!(["s", ["a", "k"], true]));
    assert_eq!(out["a"]["k"], json!(true));
}

#[test]
fn delta_set_replaces_an_array_item() {
    let out = one(base(), json!(["s", ["list", 0], {"id": 7}]));
    assert_eq!(out["list"], json!([{"id": 7}]));
}

#[test]
fn delta_delete_removes_an_array_item() {
    let out = one(base(), json!(["d", ["list", 0]]));
    assert_eq!(out["list"], json!([]));
}

#[test]
fn delta_delete_removes_an_object_key() {
    let out = one(base(), json!(["d", ["a", "text"]]));
    assert_eq!(out["a"], json!({}));
}

#[test]
fn delta_replace_swaps_the_whole_value() {
    let out = one(base(), json!(["r", {"v": 1}]));
    assert_eq!(out, json!({"v": 1}));
}

#[test]
fn delta_refuses_a_missing_path() {
    refused(json!(["s", ["nope", "k"], 1]));
    refused(json!(["d", ["a", "nope", "deeper"]]));
    refused(json!(["a", ["a", "nope"], "x"]));
}

#[test]
fn delta_refuses_a_string_segment_into_an_array() {
    refused(json!(["s", ["list", "id"], 1]));
}

#[test]
fn delta_refuses_an_index_out_of_range() {
    refused(json!(["s", ["list", 5], 1]));
    refused(json!(["d", ["list", 5]]));
    refused(json!(["p", ["list"], 5, 0, [1]]));
}

#[test]
fn delta_refuses_append_to_a_non_string() {
    refused(json!(["a", ["list"], "x"]));
    refused(json!(["t", ["list"], 1]));
}

#[test]
fn delta_refuses_an_empty_path_where_one_is_required() {
    refused(json!(["s", [], 1]));
    refused(json!(["d", []]));
    refused(json!(["a", [], "x"]));
    refused(json!(["t", [], 1]));
}

#[test]
fn delta_refuses_a_reserved_segment() {
    refused(json!(["s", ["__proto__", "x"], 1]));
    refused(json!(["s", ["a", "constructor"], 1]));
    refused(json!(["s", ["a", "prototype"], 1]));
}

#[test]
fn delta_refuses_a_malformed_op() {
    refused(json!(["x", ["a"], 1]));
    refused(json!(["s", ["a"]]));
    refused(json!(["r"]));
    refused(json!("not a tuple"));
}

#[test]
fn delta_refusal_leaves_no_partial_result() {
    let ops = [json!(["a", ["a", "text"], "llo"]), json!(["d", ["nope"]])];
    assert!(apply(base(), &ops).is_err(), "the second op is refused");
    // The applier takes the value by move and is pure: the same input applies the first op alone cleanly.
    let out = apply(base(), &ops[..1]).expect("the first op alone applies");
    assert_eq!(out["a"]["text"], json!("hello"));
    assert_eq!(base()["a"]["text"], json!("he"));
}
