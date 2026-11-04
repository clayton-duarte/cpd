//! `cpd context stats` (commands.md §3.20; Plan `ctx-observability`, pro-d13), in this process over the harness `Fx`.

use crate::common;

use common::fx::Fx;

fn fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_config("version = 1\nmachine = \"hs\"\n");
    for (id, title) in [("alpha", "Alpha"), ("beta", "Beta")] {
        fx.cpd(&["context", "new", id, "--title", title, "--owner", "acme"])
            .ok();
    }
    fx.cpd(&["context", "note", "Kept fact.", "--context", "alpha"])
        .ok();
    fx.cpd(&["context", "note", "Another fact.", "--context", "alpha"])
        .ok();
    fx.cpd(&["context", "decide", "First decision.", "--context", "alpha"])
        .ok();
    fx.cpd(&["context", "note", "Beta fact.", "--context", "beta"])
        .ok();
    fx
}

#[test]
fn stats_flow_prints_json_and_markdown() {
    let fx = fx();
    let out = fx.cpd(&["context", "stats", "--context", "alpha"]).ok();
    assert_eq!(out["since"], serde_json::Value::Null, "{out}");
    let contexts = out["contexts"]
        .as_array()
        .unwrap_or_else(|| panic!("{out}"));
    assert_eq!(contexts.len(), 1, "{out}");
    let c = &contexts[0];
    assert_eq!(c["context"], "alpha", "{out}");
    let by_kind = &c["sizes"]["records"]["by_kind"];
    assert_eq!(by_kind["note"]["count"], 2, "{out}");
    assert_eq!(by_kind["decision"]["count"], 1, "{out}");
    assert_eq!(
        c["sizes"]["records"]["by_status"]["in_force"]["count"], 3,
        "{out}"
    );
    assert_eq!(c["writing_and_curation"]["records_written"], 3, "{out}");
    for section in ["sizes", "writing_and_curation", "cost", "retrieval"] {
        assert!(c[section].is_object(), "{section}: {out}");
    }
    let markdown = out["markdown"].as_str().unwrap_or_else(|| panic!("{out}"));
    assert!(markdown.contains("## alpha"), "{markdown}");
}

#[test]
fn stats_flow_all_lists_every_live_context() {
    let fx = fx();
    let out = fx.cpd(&["context", "stats", "--all"]).ok();
    let ids: Vec<&str> = out["contexts"]
        .as_array()
        .unwrap_or_else(|| panic!("{out}"))
        .iter()
        .map(|c| c["context"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["alpha", "beta"], "{out}");
    assert_eq!(
        out["contexts"][1]["sizes"]["records"]["by_kind"]["note"]["count"], 1,
        "{out}"
    );
    let both = fx
        .cpd(&["context", "stats", "--all", "--context", "alpha"])
        .error();
    assert_eq!(both["code"], "usage", "{both}");
}

#[test]
fn stats_flow_rejects_a_bad_since() {
    let fx = fx();
    let error = fx
        .cpd(&[
            "context",
            "stats",
            "--context",
            "alpha",
            "--since",
            "yesterday",
        ])
        .error();
    assert_eq!(error["code"], "usage", "{error}");
    assert_eq!(error["details"]["since"], "yesterday", "{error}");
    let ok = fx
        .cpd(&[
            "context",
            "stats",
            "--context",
            "alpha",
            "--since",
            "2026-01-01T00:00:00Z",
        ])
        .ok();
    assert_eq!(ok["since"], "2026-01-01T00:00:00Z", "{ok}");
}
