//! `pr view`'s `feedback` (Plan fb-path, DESIGN item 4): the stored list of open items, null when never read.

use serde_json::{Value, json};

use super::pr_view::fx;

fn item(id: &str, kind: &str, path: Value) -> Value {
    json!({"id": id, "kind": kind, "last": id, "author": "reviewer", "bot": false, "path": path, "outdated": false,
           "url": format!("https://github.com/acme/pay/pull/812#{id}"), "excerpt": "This retry never backs off."})
}

#[test]
fn pr_view_lists_the_stored_feedback_items() {
    let fx = fx("open");
    let list = json!([
        item("PRRT_1", "thread", json!("src/payout.rs")),
        item("IC_1", "comment", Value::Null)
    ]);
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "UPDATE pull_requests SET feedback = '{list}' WHERE number = 812;"
            ))
            .map_err(Into::into)
        })
        .unwrap();

    let out = fx
        .fx
        .cpd(&["pr", "view", "--pr", "repo", "--plan", "pay"])
        .ok();

    assert_eq!(out["feedback"], list);
    // Its existing fields stay.
    assert_eq!(out["comments"][0]["id"], "IC_1");
    assert_eq!(out["inline_comments"][0]["id"], "1893");
}

#[test]
fn pr_view_gives_null_feedback_for_a_pr_never_read_and_keeps_its_other_fields() {
    let fx = fx("open");

    let out = fx
        .fx
        .cpd(&["pr", "view", "--pr", "repo", "--plan", "pay"])
        .ok();

    let object = out.as_object().unwrap();
    assert!(object.contains_key("feedback"), "{out}");
    assert_eq!(object["feedback"], Value::Null);
    assert_eq!(out["reviews"][0]["id"], "PRR_1");
    assert_eq!(out["comments"][0]["id"], "IC_1");
    assert_eq!(out["inline_comments"][0]["id"], "1893");
    assert_eq!(out["pr"]["key"], "repo");
}
