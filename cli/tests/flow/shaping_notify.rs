//! The Manager's message for a Lead-less shaping Plan's Job Events (contract §5.3 "The Events of a Lead-less Plan",
//! §3.1 "Shaping"; commands.md §4.1, §5.1, §8.1, §8.2; air-d19, air-d22).
//!
//! When cpd appends a `job-merged`, a `job-failed` or an `escalation` Event (the open of a `cpd ask`) for a Plan whose
//! `lead_session` is NULL, it appends in the same transaction one `session-message` Event to the live Manager of the
//! Plan's Context, as engine E8's `tell_managers` does: `from` null, `wake` true. The message's `data.text` is the
//! triggering Event's `text` for `job-merged` and `job-failed`; for an Escalation it is `Worker <agent-key> of Plan
//! <plan> asks: "<question, first 300 characters>" (escalation <id>). Answer with: cpd answer <id> "<text>" --plan
//! <plan>`. With no live Manager nothing is written, and a Plan with a Lead is unchanged.
//!
//! Every command runs in this process (`Fx::with_ports`) on the fake tmux and the fake pi install, against a temporary
//! `$CPD_HOME`: no real tmux, git, gh or pi is ever started. The notify does not exist yet in the base, so every test
//! here is red until the feature lands.

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
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.cockpit();
    fx
}

/// A Context `id`, owner `acme`.
fn context(fx: &Fx, id: &str) {
    fx.cpd(&["context", "new", id, "--title", "Other", "--owner", "acme"])
        .ok();
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
fn new_shaping(fx: &Fx, manager: &str, context: &str, id: &str) {
    run_as(
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

// ---------- job-merged ----------

#[test]
fn shaping_notify_job_merged_wakes_the_manager() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &manager, "shape1", "scan");
    let attempt = running_explore(&fx, "ctx", "shape1", "scan");

    submit_explore(
        &fx,
        "w-scan",
        "shape1",
        &attempt,
        "The ladder is in engine/src/ladder.rs.",
    )
    .ok();

    let merged = events(&fx, "job-merged");
    assert_eq!(merged.len(), 1, "{merged:?}");
    assert_eq!(
        merged[0]["data"]["artefact"],
        json!("run/shape1/explore/scan.md")
    );
    assert!(
        merged[0]["text"]
            .as_str()
            .unwrap()
            .contains("Summary: The ladder is in engine/src/ladder.rs."),
        "{merged:?}"
    );

    let sent = messages(&fx);
    assert_eq!(sent.len(), 1, "one session-message: {sent:?}");
    assert_eq!(sent[0]["session"], json!(manager));
    assert_eq!(sent[0]["context"], json!("ctx"));
    assert_eq!(sent[0]["plan"], Value::Null, "a Manager is in no Plan");
    assert_eq!(sent[0]["data"]["from"], Value::Null);
    assert_eq!(sent[0]["data"]["wake"], json!(true));
    assert_eq!(
        sent[0]["data"]["text"], merged[0]["text"],
        "the message is the job-merged Event's text, summary and artefact path included"
    );
}

// ---------- job-failed ----------

#[test]
fn shaping_notify_job_failed_wakes_the_manager() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &manager, "shape1", "scan");
    let attempt = running_explore(&fx, "ctx", "shape1", "scan");

    abort_explore(&fx, &manager, "shape1", &attempt).ok();

    let failed = events(&fx, "job-failed");
    assert_eq!(failed.len(), 1, "{failed:?}");

    let sent = messages(&fx);
    assert_eq!(sent.len(), 1, "one session-message: {sent:?}");
    assert_eq!(sent[0]["session"], json!(manager));
    assert_eq!(sent[0]["data"]["from"], Value::Null);
    assert_eq!(sent[0]["data"]["wake"], json!(true));
    assert_eq!(
        sent[0]["data"]["text"], failed[0]["text"],
        "the message is the job-failed Event's text"
    );
}

// ---------- escalation ----------

#[test]
fn shaping_notify_escalation_wakes_the_manager() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &manager, "shape1", "scan");
    let attempt = running_explore(&fx, "ctx", "shape1", "scan");

    let run = ask_timeout(&fx, "shape1", &attempt, "Which field names?");
    refused(&run, "ask_timeout", 5);

    let asked = events(&fx, "escalation");
    assert_eq!(asked.len(), 1, "{asked:?}");

    let sent = messages(&fx);
    assert_eq!(sent.len(), 1, "one session-message: {sent:?}");
    assert_eq!(sent[0]["session"], json!(manager));
    assert_eq!(sent[0]["data"]["from"], Value::Null);
    assert_eq!(sent[0]["data"]["wake"], json!(true));
    assert_eq!(
        sent[0]["data"]["text"],
        json!(concat!(
            "Worker ctx:shape1:scan of Plan shape1 asks: \"Which field names?\" (escalation scan-a1-q1). ",
            "Answer with: cpd answer scan-a1-q1 \"<text>\" --plan shape1"
        )),
        "the escalation message names the Worker's agent key, the question, the escalation and the answer command"
    );
}

// ---------- no live Manager ----------

#[test]
fn shaping_notify_without_live_manager_writes_nothing() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &manager, "shape1", "scan");
    let attempt = running_explore(&fx, "ctx", "shape1", "scan");
    // The Manager kept the Plan and added its Job while live; it is gone before the Job ends.
    fx.env.sql(&format!(
        "UPDATE sessions SET archived_at = 't' WHERE id = '{manager}';"
    ));

    submit_explore(&fx, "w-scan", "shape1", &attempt, "Done.").ok();

    assert_eq!(events(&fx, "job-merged").len(), 1, "the Job still merged");
    assert!(
        messages(&fx).is_empty(),
        "with no live Manager nothing is written: {:?}",
        messages(&fx)
    );
}

// ---------- a Plan with a Lead is unchanged ----------

#[test]
fn shaping_notify_lead_plan_events_are_unchanged() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    let lead = new_lead_plan(&fx, &manager, "ctx", "pl");
    add_scout(&fx, &lead, "pl", "scan");
    let attempt = running_explore(&fx, "ctx", "pl", "scan");

    submit_explore(&fx, "w-scan", "pl", &attempt, "Done.").ok();

    assert_eq!(events(&fx, "job-merged").len(), 1, "the Job merged");
    assert!(
        messages(&fx).is_empty(),
        "a Lead Plan's Events are unchanged: no session-message {:?}",
        messages(&fx)
    );
}

// ---------- one message per event ----------

#[test]
fn shaping_notify_one_message_per_event() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &manager, "shape1", "scan");
    add_scout(&fx, &manager, "shape1", "probe");
    let scan = running_explore(&fx, "ctx", "shape1", "scan");
    let probe = running_explore(&fx, "ctx", "shape1", "probe");

    submit_explore(&fx, "w-scan", "shape1", &scan, "First.").ok();
    submit_explore(&fx, "w-probe", "shape1", &probe, "Second.").ok();

    let merged = events(&fx, "job-merged");
    assert_eq!(merged.len(), 2, "{merged:?}");
    let sent = messages(&fx);
    assert_eq!(sent.len(), 2, "one message per Event: {sent:?}");
    for (message, event) in sent.iter().zip(merged.iter()) {
        assert_eq!(message["session"], json!(manager));
        assert_eq!(message["data"]["text"], event["text"]);
    }
}

// ---------- another Context's Manager hears nothing ----------

#[test]
fn shaping_notify_other_context_manager_hears_nothing() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    context(&fx, "ctx2");
    let other = manager_of(&fx, "ctx2");
    new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &manager, "shape1", "scan");
    let attempt = running_explore(&fx, "ctx", "shape1", "scan");

    submit_explore(&fx, "w-scan", "shape1", &attempt, "Done.").ok();

    let sent = messages(&fx);
    assert_eq!(sent.len(), 1, "exactly one message: {sent:?}");
    assert_eq!(sent[0]["session"], json!(manager));
    assert!(
        sent.iter().all(|m| m["session"] != json!(other)),
        "the other Context's Manager hears nothing: {sent:?}"
    );
}
