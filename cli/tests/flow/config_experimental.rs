//! `cpd config experimental` (commands.md §14.2b; contract §8.1, §8.2), through the command tree in this process.

use std::fs;

use crate::common;
use common::fx::Fx;
use serde_json::{Value, json};

const MINIMAL: &str = "version = 1\nmachine = \"hs\"\n";

fn text(fx: &Fx) -> String {
    fs::read_to_string(fx.env.cpd_home().join("config.toml")).unwrap()
}

fn defaults() -> Value {
    json!({"durable_workers": false, "write_curation": true, "failure_triage": true, "turn_question_guard": true})
}

#[test]
fn config_experimental_prints_the_four_flags() {
    let fx = Fx::new();
    fx.env.write_config(MINIMAL);
    let out = fx.cpd(&["config", "experimental"]).ok();
    assert_eq!(out["experimental"], defaults(), "{out}");
    assert_eq!(text(&fx), MINIMAL, "printing writes nothing");
}

#[test]
fn config_experimental_on_sets_one_key_and_prints_all() {
    let fx = Fx::new();
    fx.env.write_config(MINIMAL);
    let out = fx
        .cpd(&["config", "experimental", "durable_workers", "on"])
        .ok();
    let mut want = defaults();
    want["durable_workers"] = json!(true);
    assert_eq!(out["experimental"], want, "{out}");
    let written = text(&fx);
    assert!(
        written.contains("[experimental]") && written.contains("durable_workers = true"),
        "{written}"
    );
    assert!(!written.contains("write_curation"), "{written}");
    let again = fx.cpd(&["config", "experimental"]).ok();
    assert_eq!(again["experimental"], want, "{again}");
}

#[test]
fn config_experimental_off_keeps_comments_and_other_keys() {
    let fx = Fx::new();
    let before = "# my config\nversion = 1\nmachine = \"hs\" # this box\nconcurrency = 4\n\n# switches\n[experimental]\ndurable_workers = true # keep\nwrite_curation = true\nfailure_triage = false\n";
    fx.env.write_config(before);
    let out = fx
        .cpd(&["config", "experimental", "write_curation", "off"])
        .ok();
    assert_eq!(
        out["experimental"],
        json!({"durable_workers": true, "write_curation": false, "failure_triage": false, "turn_question_guard": true}),
        "{out}"
    );
    assert_eq!(
        text(&fx),
        before.replace("write_curation = true", "write_curation = false")
    );
}

#[test]
fn config_experimental_refuses_an_unknown_feature_and_a_bad_state() {
    let fx = Fx::new();
    fx.env.write_config(MINIMAL);
    for args in [
        vec!["config", "experimental", "nope", "on"],
        vec!["config", "experimental", "durable_workers", "maybe"],
        vec!["config", "experimental", "durable_workers"],
    ] {
        let run = fx.cpd(&args);
        common::refused(&run, "usage", 2);
        assert_eq!(text(&fx), MINIMAL, "{args:?} writes nothing");
    }
}

#[test]
fn config_experimental_durable_workers_changes_the_worker_runner() {
    let fx = Fx::new();
    fx.env.write_config(MINIMAL);
    let before = fx.cpd(&["config"]).ok();
    assert_eq!(before["config"]["experimental"]["durable_workers"], false);
    fx.cpd(&["config", "experimental", "durable_workers", "on"])
        .ok();
    let after = fx.cpd(&["config"]).ok();
    assert_eq!(
        after["config"]["experimental"]["durable_workers"], true,
        "{after}"
    );
    fx.cpd(&["config", "experimental", "durable_workers", "off"])
        .ok();
    let off = fx.cpd(&["config"]).ok();
    assert_eq!(off["config"]["experimental"]["durable_workers"], false);
}
