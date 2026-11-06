//! `cpd swr health record <system> <endpoint> ok|error [--text]` (docs/architecture/swr-classifier.md §3.3). The
//! command's clock is the `ManualClock` of `Fx::with_ports` (2026-01-01T00:00:00.000Z).

use crate::common;

use common::fx::Fx;
use common::refused;
use serde_json::json;

fn fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_config("version = 1\nmachine = \"m1\"\n");
    fx
}

#[test]
fn health_record_command_updates_swr_endpoints() {
    let fx = fx();
    let failed = fx
        .cpd(&[
            "swr",
            "health",
            "record",
            "classifier",
            "ollama.lab",
            "error",
            "--text",
            "no route",
        ])
        .ok();
    assert_eq!(
        failed,
        json!({"ok": true, "system": "classifier", "endpoint": "ollama.lab", "status": "error",
               "error_count": 1, "failing_since": "2026-01-01T00:00:00.000Z"}),
        "{failed}"
    );
    fx.cpd(&[
        "swr",
        "health",
        "record",
        "classifier",
        "ollama.lab",
        "error",
        "--text",
        "no route",
    ])
    .ok();
    assert_eq!(
        fx.env
            .query::<i64>("SELECT error_count FROM swr_endpoints WHERE endpoint = 'ollama.lab'"),
        2,
        "each error is one failure"
    );

    let ok = fx
        .cpd(&["swr", "health", "record", "classifier", "ollama.lab", "ok"])
        .ok();
    assert_eq!(
        ok,
        json!({"ok": true, "system": "classifier", "endpoint": "ollama.lab", "status": "success",
               "error_count": 0, "failing_since": null}),
        "{ok}"
    );

    refused(
        &fx.cpd(&["swr", "health", "record", "github", "ollama.lab", "ok"]),
        "usage",
        2,
    );
    refused(
        &fx.cpd(&["swr", "health", "record", "classifier", "bifrost", "error"]),
        "usage",
        2,
    );
    refused(
        &fx.cpd(&[
            "swr",
            "health",
            "record",
            "classifier",
            "bifrost",
            "ok",
            "--text",
            "x",
        ]),
        "usage",
        2,
    );
}
