//! The optional Spec field `resources` (contract §2.2, commands.md §5.1, §5.2, §5.7): stored, checked against the
//! repo policy's declared names, shown by `job show` and `plan render`, not fingerprinted.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::{Run, refused};
use serde_json::{Value, json};

use super::job::{events, fixture, lock, plan, setup};
use super::resource;

/// Plan `pl` in phase `plan`, its PR `repo` named `cpd` of owner `acme`, whose policy declares `e2e-port`.
fn world() -> Fx {
    let fx = Fx::new();
    setup(&fx.env);
    fs::write(
        fx.env.cpd_home().join("policy/acme/cpd.toml"),
        "[resources.e2e-port]\n",
    )
    .unwrap();
    plan(&fx.env, "pl", "plan");
    fixture(
        &fx.env,
        "UPDATE workflows SET name = 'cpd' WHERE plan = 'pl';",
    );
    fx
}

/// The host file and the owner file `acme` declare `shared-port` and `staging`; the repo file declares `e2e-port`.
fn declared_at_three_levels(fx: &Fx) {
    let home = fx.env.cpd_home();
    let host = home.join("policy/host.toml");
    let held = fs::read_to_string(&host).unwrap_or_default();
    fs::write(&host, format!("{held}[resources.shared-port]\nslots = 2\n")).unwrap();
    fs::write(home.join("policy/acme/owner.toml"), "[resources.staging]\n").unwrap();
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
    let mut args = vec![
        "job", "add", key, "--title", &title, "--tier", "S2", "--spec", &spec, "--plan", "pl",
    ];
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
    assert_eq!(
        (details["job"].as_str(), details["resource"].as_str()),
        (Some("a"), Some("gpu"))
    );
    assert_eq!(details["declared"], json!(["e2e-port"]));
}

#[test]
fn resource_spec_duplicate_empty_or_malformed_names_are_spec_invalid() {
    let fx = world();
    for bad in [
        json!(["e2e-port", "e2e-port"]),
        json!([]),
        json!(["E2E"]),
        json!(["1a"]),
        json!(["a_b"]),
        json!(["a".repeat(33)]),
        json!([""]),
    ] {
        let run = add(&fx, "a", Some(bad.clone()), &[]);
        assert_eq!(
            refused(&run, "spec_invalid", 2)["details"]["path"]
                .as_str()
                .unwrap()
                .split('[')
                .next(),
            Some("$.resources"),
            "{bad}"
        );
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
    fx.cpd(&["job", "edit", "a", "--spec", &spec, "--plan", "pl"])
        .ok();
    assert_eq!(
        shown(&fx, "a")["resources"],
        json!(["e2e-port"]),
        "the edit stored the resources"
    );
    assert_eq!(fingerprint(), before);
}

#[test]
fn resource_spec_job_edit_of_a_queued_job_checks_and_stores_resources() {
    let fx = world();
    add(&fx, "a", None, &[]).ok();
    lock(&fx.env, "pl");
    let edit = |resources: Value| {
        let spec = spec_with(&fx, Some(resources));
        fx.cpd(&["job", "edit", "a", "--spec", &spec, "--plan", "pl"])
    };
    edit(json!(["e2e-port"])).ok();
    assert_eq!(shown(&fx, "a")["status"], "queued");
    assert_eq!(shown(&fx, "a")["resources"], json!(["e2e-port"]));
    let details = refused(&edit(json!(["gpu"])), "resource_undeclared", 3)["details"].clone();
    assert_eq!(
        (details["job"].as_str(), details["resource"].as_str()),
        (Some("a"), Some("gpu"))
    );
    assert_eq!(
        shown(&fx, "a")["resources"],
        json!(["e2e-port"]),
        "the refused edit stored nothing"
    );
}

#[test]
fn job_add_accepts_a_resource_declared_at_host_or_owner_level() {
    let fx = world();
    declared_at_three_levels(&fx);

    add(
        &fx,
        "a",
        Some(json!(["e2e-port", "shared-port", "staging"])),
        &[],
    )
    .ok();

    assert_eq!(
        shown(&fx, "a")["resources"],
        json!(["e2e-port", "shared-port", "staging"])
    );
    add(&fx, "b", None, &[]).ok();
    let spec = spec_with(&fx, Some(json!(["staging"])));
    fx.cpd(&["job", "edit", "b", "--spec", &spec, "--plan", "pl"])
        .ok();
    assert_eq!(shown(&fx, "b")["resources"], json!(["staging"]));
}

#[test]
fn job_add_refuses_a_resource_visible_at_no_level_with_the_visible_names() {
    let fx = world();
    declared_at_three_levels(&fx);

    let run = add(&fx, "a", Some(json!(["shared-port", "gpu"])), &[]);

    let details = refused(&run, "resource_undeclared", 3)["details"].clone();
    assert_eq!(
        (
            details["job"].as_str(),
            details["pr"].as_str(),
            details["resource"].as_str()
        ),
        (Some("a"), Some("repo"), Some("gpu"))
    );
    assert_eq!(
        details["declared"],
        json!(["e2e-port", "shared-port", "staging"]),
        "the union over host, owner and repo"
    );
    assert!(
        details["file"]
            .as_str()
            .unwrap()
            .ends_with("policy/acme/cpd.toml"),
        "{details}"
    );
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs"), 0);
}

/// Plan `pl` with `concurrency = 1`: Job `a` runs (its Attempt `a-a1` takes the one Worker slot), Jobs `b` (no resources) and
/// `c` (lists the free `e2e-port`) are queued.
pub(super) fn one_worker_slot_taken() -> Fx {
    let fx = world();
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"m1\"\nconcurrency = 1\n",
    )
    .unwrap();
    add(&fx, "a", None, &[]).ok();
    add(&fx, "b", None, &[]).ok();
    add(&fx, "c", Some(json!(["e2e-port"])), &[]).ok();
    lock(&fx.env, "pl");
    fixture(
        &fx.env,
        "UPDATE jobs SET status = 'running' WHERE plan = 'pl' AND key = 'a';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, driver_pid)
           VALUES ('pl', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pl--a-a1', '/w', '2026-10-26T21:41:00.000Z', 4242);",
    );
    fx
}

#[test]
fn job_show_waiting_for_names_workers_when_no_worker_slot_is_free() {
    let fx = one_worker_slot_taken();

    let b = shown(&fx, "b");
    let c = shown(&fx, "c");

    // A queued Job with no resources of its own waits for `workers` (commands.md §5.7): scope host, slots = concurrency, and the
    // holders are the Attempts not ended.
    let workers = |job: &Value| {
        let waiting = job["waiting_for"]
            .as_array()
            .unwrap_or_else(|| panic!("no waiting_for: {job}"));
        assert_eq!(waiting.len(), 1, "{waiting:?}");
        waiting[0].clone()
    };
    for job in [&b, &c] {
        let entry = workers(job);
        assert_eq!(
            (&entry["resource"], &entry["scope"], &entry["slots"]),
            (&json!("workers"), &json!("host"), &json!(1)),
            "{entry}"
        );
        let holders = entry["holders"].as_array().unwrap();
        assert_eq!(holders.len(), 1, "{entry}");
        assert_eq!(
            (
                &holders[0]["by"],
                &holders[0]["plan"],
                &holders[0]["job"],
                &holders[0]["attempt"]
            ),
            (&json!("attempt"), &json!("pl"), &json!("a"), &json!("a-a1"))
        );
        assert!(holders[0]["at"].is_string(), "{entry}");
    }
    // A running Job has none.
    assert!(shown(&fx, "a").get("waiting_for").is_none());

    // `a` fails and its Attempt ends: a `failed` Job keeps its worker (contract §3.4 "Tenancy"), so `b` and `c` still wait for `workers`.
    fixture(&fx.env, "UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE id = 'a-a1';
                      UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE plan = 'pl' AND key = 'a';");
    for job in ["b", "c"] {
        let waiting = shown(&fx, job)["waiting_for"].clone();
        assert_eq!(
            waiting.as_array().map(Vec::len),
            Some(1),
            "{job} still waits for the worker the failed Job keeps: {waiting}"
        );
        assert_eq!(
            (&waiting[0]["resource"], &waiting[0]["holders"][0]["job"]),
            (&json!("workers"), &json!("a"))
        );
    }

    // The Lead cancels `a`, with no live Attempt: the slot frees at once and the entry goes (no other resource is held: `c`'s
    // `e2e-port` is free).
    fixture(
        &fx.env,
        "UPDATE jobs SET status = 'cancelled', failed_cause = NULL, cancel_reason = 'no longer needed', cancel_requested_at = 't' WHERE plan = 'pl' AND key = 'a';",
    );
    assert!(
        shown(&fx, "b").get("waiting_for").is_none(),
        "no resources and a free Worker slot: the key is absent"
    );
    assert_eq!(
        shown(&fx, "c")["waiting_for"],
        json!([]),
        "it lists resources and none is full: `[]`"
    );
}

#[test]
fn resource_list_shows_job_tenants_and_command_tenants() {
    let (fx, repo) = resource::tenancy_world("[resources.e2e-port]\nslots = 3\n");
    let _command = resource::hold(&fx, "e2e-port", &common::command_record());
    resource::job_tenant(&fx, "api", "repo:acme/cpd:e2e-port", resource::API_AT);
    // A Job of another repo's pool of the same name is no tenant of this pool.
    resource::job_tenant(
        &fx,
        "other",
        "repo:acme/elsewhere:e2e-port",
        "2026-10-26T21:42:00.000Z",
    );

    let out = fx.cpd_in(&repo, &["resource", "list"]).ok();

    let command: Value = serde_json::from_str(&common::command_record()).unwrap();
    assert_eq!(
        resource::declared_only(&out),
        json!([{
            "name": "e2e-port", "scope": "repo", "slots": 3, "free": 1,
            "holders": [{"slot": 1, "holder": command}, {"slot": null, "holder": resource::api_holder()}],
        }]),
        "the held command slots first, with their slot; then the Job tenants, with no slot number"
    );
}

/// The one fact `scripts/cargo.sh` reads from cpd: `cpd resource list` is one compact line naming `cargo` (pro-d132, A1).
#[test]
fn resource_list_names_cargo_in_the_compact_form_cargo_sh_reads() {
    let (fx, repo) = resource::tenancy_world("");
    let host = fx.env.cpd_home().join("policy/host.toml");
    let held = fs::read_to_string(&host).unwrap_or_default();
    fs::write(&host, format!("{held}[resources.cargo]\nslots = 3\n")).unwrap();

    let run = fx.cpd_in(&repo, &["resource", "list"]);

    assert_eq!(run.stdout.lines().count(), 1, "{}", run.stdout);
    assert!(run.stdout.contains("\"name\":\"cargo\""), "{}", run.stdout);
}
