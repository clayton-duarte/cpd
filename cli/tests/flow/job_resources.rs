//! The optional Spec field `resources` (contract §2.2, commands.md §5.1, §5.2, §5.7): stored, checked against the
//! repo policy's declared names, refused on a review Job, shown by `job show` and `plan render`, not fingerprinted.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::{Run, refused};
use serde_json::{Value, json};

use super::job::{events, fixture, plan, setup};

/// Plan `pl` in phase `plan`, its PR `repo` named `cpd` of owner `acme`, whose policy declares `e2e-port`.
fn world() -> Fx {
    let fx = Fx::new();
    setup(&fx.env);
    fs::write(fx.env.cpd_home().join("policy/acme/cpd.toml"), "[resources.e2e-port]\n").unwrap();
    plan(&fx.env, "pl", "plan");
    fixture(&fx.env, "UPDATE prs SET name = 'cpd' WHERE plan = 'pl';");
    fx
}

fn spec_with(fx: &Fx, resources: Option<Value>) -> String {
    let mut spec = json!({"goal": "Do it.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test"], "out_of_scope": []});
    if let Some(r) = resources {
        spec["resources"] = r;
    }
    let path = fx.env.root().join("spec-resources.json");
    fs::write(&path, spec.to_string()).unwrap();
    path.to_str().unwrap().to_owned()
}

fn add(fx: &Fx, key: &str, resources: Option<Value>, extra: &[&str]) -> Run {
    let spec = spec_with(fx, resources);
    let title = format!("Title of {key}");
    let mut args = vec!["job", "add", key, "--title", &title, "--tier", "S2", "--spec", &spec, "--plan", "pl"];
    args.extend(extra);
    fx.cpd(&args)
}

fn shown(fx: &Fx, key: &str) -> Value {
    fx.cpd(&["job", "show", key, "--plan", "pl"]).ok()["job"].clone()
}

#[test]
fn resource_spec_job_add_stores_the_resources_of_a_spec() {
    let fx = world();
    add(&fx, "a", Some(json!(["e2e-port"])), &[]).ok();
    assert_eq!(shown(&fx, "a")["resources"], json!(["e2e-port"]));
    assert_eq!(events(&fx, "job-added").len(), 1);
}

#[test]
fn resource_spec_job_add_refuses_an_undeclared_resource() {
    let fx = world();
    let run = add(&fx, "a", Some(json!(["e2e-port", "gpu"])), &[]);
    let details = refused(&run, "resource_undeclared", 3)["details"].clone();
    assert_eq!(details["job"], "a");
    assert_eq!(details["pr"], "repo");
    assert_eq!(details["resource"], "gpu");
    assert_eq!(details["declared"], json!(["e2e-port"]));
    assert!(details["file"].is_string(), "{details}");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs"), 0);
}

#[test]
fn resource_spec_job_edit_spec_refuses_an_undeclared_resource() {
    let fx = world();
    add(&fx, "a", None, &[]).ok();
    let spec = spec_with(&fx, Some(json!(["gpu"])));
    let run = fx.cpd(&["job", "edit", "a", "--spec", &spec, "--plan", "pl"]);
    let details = refused(&run, "resource_undeclared", 3)["details"].clone();
    assert_eq!((details["job"].as_str(), details["resource"].as_str()), (Some("a"), Some("gpu")));
    assert_eq!(details["declared"], json!(["e2e-port"]));
}

#[test]
fn resource_spec_resources_are_refused_on_a_review_job() {
    let fx = world();
    add(&fx, "a", None, &[]).ok();
    let run = add(&fx, "rev", Some(json!(["e2e-port"])), &["--review"]);
    assert_eq!(refused(&run, "spec_invalid", 2)["details"]["path"], "$.resources");
}

#[test]
fn resource_spec_duplicate_empty_or_malformed_names_are_spec_invalid() {
    let fx = world();
    for bad in [json!(["e2e-port", "e2e-port"]), json!([]), json!(["E2E"]), json!(["1a"]), json!(["a_b"]), json!(["a".repeat(33)]), json!([""])] {
        let run = add(&fx, "a", Some(bad.clone()), &[]);
        assert_eq!(refused(&run, "spec_invalid", 2)["details"]["path"].as_str().unwrap().split('[').next(), Some("$.resources"), "{bad}");
    }
}

#[test]
fn resource_spec_job_show_prints_the_resources() {
    let fx = world();
    add(&fx, "a", Some(json!(["e2e-port"])), &[]).ok();
    add(&fx, "b", None, &[]).ok();
    assert_eq!(shown(&fx, "a")["resources"], json!(["e2e-port"]));
    assert_eq!(shown(&fx, "b")["resources"], json!([]));
}

#[test]
fn resource_spec_plan_render_shows_the_resources_of_a_job() {
    let fx = world();
    add(&fx, "a", Some(json!(["e2e-port"])), &[]).ok();
    let out = fx.cpd(&["plan", "render", "--plan", "pl"]).ok();
    let text = fs::read_to_string(out["path"].as_str().unwrap()).unwrap();
    assert!(text.contains("#### Resources\n\n- `e2e-port`"), "{text}");
}

#[test]
fn resource_spec_the_fingerprint_ignores_resources() {
    let fx = world();
    add(&fx, "a", None, &[]).ok();
    let fingerprint = || fx.cpd(&["plan", "render", "--plan", "pl"]).ok()["fingerprint"].clone();
    let before = fingerprint();
    let spec = spec_with(&fx, Some(json!(["e2e-port"])));
    fx.cpd(&["job", "edit", "a", "--spec", &spec, "--plan", "pl"]).ok();
    assert_eq!(shown(&fx, "a")["resources"], json!(["e2e-port"]), "the edit stored the resources");
    assert_eq!(fingerprint(), before);
}
