//! `cpd swr badge get|put` (docs/architecture/swr-classifier.md §3): every command prints one JSON value; `put`
//! validates at the edge and an unchanged put opens no write transaction. The command's clock is the
//! `ManualClock` of `Fx::with_ports` (2026-01-01T00:00:00.000Z).

use crate::common;

use common::fx::Fx;
use common::refused;
use serde_json::json;

fn fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_config("version = 1\nmachine = \"m1\"\n");
    fx
}

const PUT: [&str; 14] = [
    "swr",
    "badge",
    "put",
    "s1",
    "--entry",
    "e1",
    "--label",
    "safe_to_archive",
    "--score",
    "93",
    "--text",
    "Safe to archive · 93%",
    "--tone",
    "done",
];

fn txns(fx: &Fx) -> i64 {
    fx.env.query::<i64>("SELECT count(*) FROM txns")
}

#[test]
fn swr_badge_get_is_null_without_a_row() {
    let fx = fx();
    assert_eq!(
        fx.cpd(&["swr", "badge", "get", "s1"]).ok(),
        json!({"ok": true, "badge": null})
    );
}

#[test]
fn swr_badge_put_then_get_omits_absent_others() {
    let fx = fx();
    let put = fx.cpd(&PUT).ok();
    assert_eq!(
        put,
        json!({"ok": true, "session_id": "s1", "entry_id": "e1", "changed": true, "synced_at": "2026-01-01T00:00:00.000Z"}),
        "{put}"
    );
    let got = fx.cpd(&["swr", "badge", "get", "s1"]).ok();
    assert_eq!(
        got["badge"],
        json!({"session_id": "s1", "entry_id": "e1", "label": "safe_to_archive", "score": 93,
               "text": "Safe to archive · 93%", "tone": "done", "synced_at": "2026-01-01T00:00:00.000Z"}),
        "no others key when none was given: {got}"
    );
}

#[test]
fn swr_badge_unchanged_put_opens_no_write_txn() {
    let fx = fx();
    fx.cpd(&PUT).ok();
    let before = txns(&fx);
    let again = fx.cpd(&PUT).ok();
    assert_eq!(again["changed"], false, "{again}");
    assert_eq!(txns(&fx), before);
}

#[test]
fn swr_badge_others_round_trips() {
    let fx = fx();
    let mut args = PUT.to_vec();
    let others = r#"[{"label":"open_work","score":30}]"#;
    args.extend(["--others", others]);
    fx.cpd(&args).ok();
    let got = fx.cpd(&["swr", "badge", "get", "s1"]).ok();
    assert_eq!(
        got["badge"]["others"],
        json!([{"label": "open_work", "score": 30}]),
        "{got}"
    );
}

#[test]
fn swr_badge_put_refuses_bad_values_and_writes_nothing() {
    let fx = fx();
    let with = |flag: &str, value: &str| {
        let mut args = PUT.to_vec();
        let at = args.iter().position(|a| *a == flag).unwrap();
        args[at + 1] = value;
        args.iter().map(|s| s.to_string()).collect::<Vec<_>>()
    };
    for (flag, value) in [
        ("--label", "nope"),
        ("--score", "101"),
        ("--score", "-1"),
        ("--tone", "nope"),
        ("--text", ""),
        ("--entry", ""),
        ("--others", "[]"),
        ("--others", r#"[{"label":"nope","score":1}]"#),
    ] {
        let mut args = with(if flag == "--others" { "--tone" } else { flag }, "done");
        if flag == "--others" {
            args.extend(["--others".into(), value.into()]);
        } else {
            args = with(flag, value);
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        refused(&fx.cpd(&refs), "usage", 2);
    }
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM swr_badges"), 0);
}
