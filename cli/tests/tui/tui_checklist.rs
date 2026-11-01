//! The `checklist` kind of the dialog kit (commands.md §13.2; hs-d549, hs-d551, hs-d568, hs-d569 CD2, CD3): what stays
//! here is the refusal of an items file, whose JSON reply is the command's result (`run_with` writes it to the writer
//! the test reads). The drawing and the keys are in-process (`tui/src/dialog/checklist_tests.rs`, on a `TestBackend`).

use crate::common::fx::Fx;

use std::fs;

use serde_json::{Value, json};

const SQLITE: &str = "use sqlite as the store";

#[test]
fn an_items_file_with_a_short_field_is_refused() {
    let fx = Fx::with_ports();
    let items = fx.env.root().join("items.json");
    let out = format!("--out={}/o.json", fx.env.root().display());
    let items_arg = format!("--items={}", items.display());
    let run = |rows: Value| {
        fs::write(&items, rows.to_string()).unwrap();
        fx.cpd_without_a_terminal(&[
            "tui",
            "view",
            "dialog",
            "checklist",
            "--title=Title",
            &items_arg,
            &out,
        ])
    };
    let run_short = run(
        json!([{"id": "r-1", "short": "use sqlite", "text": SQLITE, "detail": "duplicate r-0"}]),
    );
    let reply = run_short.json();
    assert_eq!(reply["ok"], json!(false), "{reply}");
    assert_eq!(reply["error"]["code"], json!("args_invalid"), "{reply}");
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("short"),
        "the refusal names the field: {reply}"
    );
    // the same entry without `short` is the shape: the file is accepted, so the view goes on to ask for a terminal (its
    // drawing, `[ ] r-1 — duplicate r-0` with its text under the header, is `checklist_tests.rs`'s, on a `TestBackend`)
    let accepted = run(json!([{"id": "r-1", "text": SQLITE, "detail": "duplicate r-0"}]));
    assert_eq!(accepted.error()["code"], "not_a_terminal", "{accepted:?}");
}
