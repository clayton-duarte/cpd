//! Paired shaping ownership and its ordinary Plan event routing (pro-d561).
//! All external ports use fakes.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::fx::Fx;
use common::{Run, TestEnv, refused};
use serde_json::{Value, json};

// ---------- fixtures ----------

/// O carries efforts 0–5, S 0–5, H 0–3 (a scout keeps letter H, a research letter S).
const EFFORTS: [(&str, &str, &str); 4] = [
    ("O", "p/opus", "[0,1,2,3,4,5]"),
    ("S", "p/sonnet", "[0,1,2,3,4,5]"),
    ("H", "p/haiku", "[0,1,2,3]"),
    ("G", "p/gemini", "[]"),
];

/// The env: a fake pi install with its Tier catalog in the cache, a Context `ctx` (owner `acme`), and the cockpit.
fn setup() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_models();
    fx.env.write_tmux_config("");
    fx.env.fake_pi("exit 0");
    fx.env.owner_policy("acme", "");
    let scope = fx
        .env
        .root()
        .join("install/releases/1.0.0/node_modules/@earendil-works");
    fs::create_dir_all(fx.env.root().join("install")).unwrap();
    fs::write(fx.env.root().join("install/current-version"), "1.0.0\n").unwrap();
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    let rows: String = EFFORTS
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    fx.env.sql(&rows);
    fx.cockpit();
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx
}

/// The Context's live Manager (commands.md §3.10), its Session id.
fn manager_of(fx: &Fx, context: &str) -> String {
    let out = fx.cpd(&["context", "manager", "--context", context]).ok();
    out["manager"]["session"]
        .as_str()
        .expect("the Manager's Session")
        .to_owned()
}

/// A command run as `session` (`CPD_SESSION`), from the test root.
fn run_as(fx: &Fx, session: &str, args: &[&str]) -> Run {
    fx.cpd_with(fx.env.root(), args, &[("CPD_SESSION", session)])
}

/// `cpd plan new <id> --context <c> --title <t> --shaping`, run by the Context's Manager.
fn new_shaping(fx: &Fx, manager: &str, context: &str, id: &str) -> String {
    let out = run_as(
        fx,
        manager,
        &[
            "plan",
            "new",
            id,
            "--context",
            context,
            "--title",
            &format!("Plan {id}"),
            "--shaping",
        ],
    )
    .ok();
    out["lead_session"].as_str().unwrap().to_owned()
}

/// `cpd plan new <id> --context <c> --title <t>` (a Lead), run by `caller`; returns the Lead Session id.
fn new_lead_plan(fx: &Fx, caller: &str, context: &str, id: &str) -> String {
    let out = run_as(
        fx,
        caller,
        &[
            "plan",
            "new",
            id,
            "--context",
            context,
            "--title",
            &format!("Plan {id}"),
        ],
    )
    .ok();
    out["plan"]["lead_session"]
        .as_str()
        .expect("a Lead Session")
        .to_owned()
}

/// A JSON Spec file under the test root; returns its path.
fn spec_file(env: &TestEnv, name: &str, value: &Value) -> String {
    let path: PathBuf = env.root().join(name);
    fs::write(&path, value.to_string()).unwrap();
    path.to_str().unwrap().to_owned()
}

/// The four-field Spec of an explore Job (contract §2.2 "Explore Jobs").
fn explore_spec(env: &TestEnv, name: &str) -> String {
    spec_file(
        env,
        name,
        &json!({"goal": "Where is the retry ladder written?", "context": "Look in the engine.", "acceptance": ["names the file"], "out_of_scope": []}),
    )
}

/// `cpd job add <key> --kind scout --tier H2 --spec <spec> --plan <plan>`, run by `caller`.
fn add_scout(fx: &Fx, caller: &str, plan: &str, key: &str) {
    let spec = explore_spec(&fx.env, &format!("{key}-scout.json"));
    run_as(
        fx,
        caller,
        &[
            "job",
            "add",
            key,
            "--title",
            &format!("Title of {key}"),
            "--tier",
            "H2",
            "--spec",
            &spec,
            "--kind",
            "scout",
            "--plan",
            plan,
        ],
    )
    .ok();
}

/// The artefact path of explore Job `key` of `plan` (`$CPD_HOME/run/<plan>/explore/<key>.md`).
fn artefact(fx: &Fx, plan: &str, key: &str) -> PathBuf {
    fx.env
        .cpd_home()
        .join("run")
        .join(plan)
        .join("explore")
        .join(format!("{key}.md"))
}

/// A running Attempt of explore Job `key` of `plan`, its Worker Session named by its agent key, and its artefact file.
/// Returns the Attempt id.
fn running_explore(fx: &Fx, context: &str, plan: &str, key: &str) -> String {
    let attempt = format!("{key}-a1");
    let worker = format!("w-{key}");
    let artefact = artefact(fx, plan, key);
    fs::create_dir_all(artefact.parent().unwrap()).unwrap();
    fs::write(
        &artefact,
        "# Findings\n\nWhere is the retry ladder written?\n",
    )
    .unwrap();
    fx.env.sql(&format!(
        "UPDATE jobs SET status = 'running' WHERE plan = '{plan}' AND key = '{key}';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at, runner)
           VALUES ('{plan}', '{attempt}', '{key}', 1, 'H2', 'ladder', 'running', 'cpd/{plan}--{attempt}', '/w', '{worker}', 't', 'durable');
         INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
           VALUES ('{worker}', 'worker', '{context}', '{plan}', '{attempt}', '{context}:{plan}:{key}', '/w', 'H2', 'working', 't', 't');"
    ));
    attempt
}

/// `cpd attempt submit <attempt> --summary-file <path> --plan <plan>`, as the Worker submits its explore artefact.
fn submit_explore(fx: &Fx, worker: &str, plan: &str, attempt: &str, summary: &str) -> Run {
    let path = fx.env.root().join(format!("{attempt}-summary.md"));
    fs::write(&path, summary).unwrap();
    run_as(
        fx,
        worker,
        &[
            "attempt",
            "submit",
            attempt,
            "--summary-file",
            path.to_str().unwrap(),
            "--plan",
            plan,
        ],
    )
}

/// `cpd attempt abort <attempt> --plan <plan>`, as `caller` (the Plan's Manager on a shaping Plan) runs it.
fn abort_explore(fx: &Fx, caller: &str, plan: &str, attempt: &str) -> Run {
    run_as(fx, caller, &["attempt", "abort", attempt, "--plan", plan])
}

/// `cpd ask <attempt> <question> --timeout-minutes 0 --plan <plan>`: the open of the Escalation, cut short at once so the
/// command exits without sleeping. The escalation Event (and its message) is written before the wait begins.
fn ask_timeout(fx: &Fx, plan: &str, attempt: &str, question: &str) -> Run {
    fx.cpd(&[
        "ask",
        attempt,
        question,
        "--timeout-minutes",
        "0",
        "--plan",
        plan,
    ])
}

/// Every Event of `kind`, machine-wide, newest last.
fn events(fx: &Fx, kind: &str) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", kind]).ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

/// Every `session-message` Event, machine-wide, newest last.
fn messages(fx: &Fx) -> Vec<Value> {
    events(fx, "session-message")
}

// Paired shaping Plans use the ordinary Plan event route. `events list --session` is the
// Lead's subscription; `session-message` is only for legacy Lead-less Plans.
fn lead_events(fx: &Fx, lead: &str, kind: &str) -> Vec<Value> {
    fx.cpd(&["events", "list", "--session", lead, "--kind", kind])
        .ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn shaping_notify_job_merged_routes_plan_event_to_lead() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    let lead = new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &lead, "shape1", "scan");
    let attempt = running_explore(&fx, "ctx", "shape1", "scan");
    assert!(lead_events(&fx, &lead, "job-merged").is_empty());
    submit_explore(
        &fx,
        "w-scan",
        "shape1",
        &attempt,
        "The ladder is in engine/src/ladder.rs.",
    )
    .ok();
    let merged = lead_events(&fx, &lead, "job-merged");
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0]["plan"], json!("shape1"));
    assert_eq!(
        merged[0]["data"]["artefact"],
        json!("run/shape1/explore/scan.md")
    );
    assert!(
        merged[0]["text"]
            .as_str()
            .unwrap()
            .contains("Summary: The ladder is in engine/src/ladder.rs.")
    );
    assert!(messages(&fx).is_empty());
    assert!(lead_events(&fx, &manager, "job-merged").is_empty());
}

#[test]
fn shaping_notify_job_failed_routes_plan_event_to_lead() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    let lead = new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &lead, "shape1", "scan");
    let attempt = running_explore(&fx, "ctx", "shape1", "scan");
    assert!(lead_events(&fx, &lead, "job-failed").is_empty());
    abort_explore(&fx, &lead, "shape1", &attempt).ok();
    let failed = lead_events(&fx, &lead, "job-failed");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["plan"], json!("shape1"));
    assert!(messages(&fx).is_empty());
    assert!(lead_events(&fx, &manager, "job-failed").is_empty());
}

#[test]
fn shaping_notify_escalation_routes_plan_event_to_lead() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    let lead = new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &lead, "shape1", "scan");
    let attempt = running_explore(&fx, "ctx", "shape1", "scan");
    assert!(lead_events(&fx, &lead, "escalation").is_empty());
    refused(
        &ask_timeout(&fx, "shape1", &attempt, "Which field names?"),
        "ask_timeout",
        5,
    );
    let asked = lead_events(&fx, &lead, "escalation");
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["plan"], json!("shape1"));
    assert_eq!(asked[0]["data"]["question"], json!("Which field names?"));
    assert!(messages(&fx).is_empty());
    assert!(lead_events(&fx, &manager, "escalation").is_empty());
}

#[test]
fn shaping_notify_multiple_events_stay_with_their_plan() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    let lead = new_shaping(&fx, &manager, "ctx", "shape1");
    let other_lead = new_lead_plan(&fx, &manager, "ctx", "pl");
    add_scout(&fx, &lead, "shape1", "scan");
    add_scout(&fx, &other_lead, "pl", "probe");
    let scan = running_explore(&fx, "ctx", "shape1", "scan");
    let probe = running_explore(&fx, "ctx", "pl", "probe");
    submit_explore(&fx, "w-scan", "shape1", &scan, "Shaping.").ok();
    submit_explore(&fx, "w-probe", "pl", &probe, "Lead.").ok();
    assert_eq!(events(&fx, "job-merged").len(), 2);
    assert_eq!(lead_events(&fx, &lead, "job-merged").len(), 1);
    assert_eq!(lead_events(&fx, &other_lead, "job-merged").len(), 1);
    assert!(messages(&fx).is_empty());
}
