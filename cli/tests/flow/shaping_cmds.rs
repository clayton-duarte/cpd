//! `plan new --shaping` and the Manager's rights on a Lead-less shaping Plan (commands.md §4.1 "A Manager on a
//! shaping Plan", §5.1–§5.4, §6.2–§6.5, §8.2; contract §2.2 "Explore Jobs", §3.1 "The Context's Manager"; air-d19,
//! air-d22; assumptions A1–A4).
//!
//! A **Manager** is a Session with `role = manager` in a Context (`context manager`), and a **Lead** is the role of
//! an existing Plan (`plan new`). Every command runs in this process (`Fx::with_ports`) on the fake tmux and the fake
//! pi install, against a temporary `$CPD_HOME`: no real tmux, git, gh or pi is ever started.
//!
//! `plan new --shaping` does not exist yet in the base, so every test here is red until the feature lands.

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
fn new_shaping(fx: &Fx, manager: &str, context: &str, id: &str) -> Value {
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
    .ok()
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

/// The five-field Spec of a `work` Job.
fn work_spec(env: &TestEnv, name: &str) -> String {
    spec_file(
        env,
        name,
        &json!({"goal": "g", "context": "c", "touches": ["src/a.rs"], "acceptance": ["b"], "out_of_scope": []}),
    )
}

/// `cpd job add <key> --kind <kind> --tier <tier> --spec <spec>` on `plan`, run by `manager`.
fn add_kind(
    fx: &Fx,
    manager: &str,
    plan: &str,
    key: &str,
    kind: &str,
    tier: &str,
    spec: &str,
) -> Run {
    run_as(
        fx,
        manager,
        &[
            "job",
            "add",
            key,
            "--title",
            &format!("Title of {key}"),
            "--tier",
            tier,
            "--spec",
            spec,
            "--kind",
            kind,
            "--plan",
            plan,
        ],
    )
}

/// `cpd job add <key> --kind scout ...` on `plan`, run by `manager`.
fn add_scout(fx: &Fx, manager: &str, plan: &str, key: &str, tier: &str) -> Run {
    let spec = explore_spec(&fx.env, &format!("{key}-scout.json"));
    add_kind(fx, manager, plan, key, "scout", tier, &spec)
}

fn count(fx: &Fx, table: &str) -> i64 {
    fx.env.query(&format!("SELECT count(*) FROM {table}"))
}

fn phase(fx: &Fx, plan: &str) -> String {
    fx.env
        .query(&format!("SELECT phase FROM plans WHERE id = '{plan}'"))
}

fn events(fx: &Fx, kind: &str) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", kind]).ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

fn window_names(fx: &Fx) -> Vec<String> {
    fx.tmux_state()
        .map(|state| state.windows().into_iter().map(|w| w.name).collect())
        .unwrap_or_default()
}

// ---------- plan new --shaping ----------

#[test]
fn shaping_cmds_manager_plan_new_shaping_creates_leadless_plan() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");

    let out = new_shaping(&fx, &manager, "ctx", "shape1");
    assert_eq!(
        out["plan"],
        json!({"id": "shape1", "context": "ctx", "title": "Plan shape1", "jira": null, "phase": "shaping", "lead_session": null, "workflows": []}),
        "{out}"
    );
    assert_eq!(out["phase"], json!("shaping"), "{out}");
    assert_eq!(out["lead_session"], Value::Null, "{out}");

    let (phase, lead): (String, Option<String>) = fx.env.read(|c| {
        c.query_row(
            "SELECT phase, lead_session FROM plans WHERE id = 'shape1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    });
    assert_eq!(phase, "shaping");
    assert_eq!(lead, None, "a shaping Plan has a null lead_session");
    assert_eq!(
        count(&fx, "sessions"),
        1,
        "only the Manager's Session exists: no Lead Session"
    );
}

#[test]
fn shaping_cmds_plan_new_shaping_starts_no_lead_session() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");

    new_shaping(&fx, &manager, "ctx", "shape1");

    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM sessions WHERE plan = 'shape1'"),
        0,
        "a shaping Plan has no Session row"
    );
    assert!(
        !window_names(&fx).iter().any(|w| w == "park-shape1"),
        "no Lead window: {:?}",
        window_names(&fx)
    );
    assert!(
        !fx.env
            .cpd_home()
            .join("run/shape1/lead-message.md")
            .exists(),
        "no first message for a Lead-less Plan"
    );
}

#[test]
fn shaping_cmds_lead_plan_new_shaping_is_refused() {
    let fx = setup();
    let lead = fx
        .cpd(&[
            "plan",
            "new",
            "pl",
            "--context",
            "ctx",
            "--title",
            "A Lead's Plan",
        ])
        .ok()["lead"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let plans = count(&fx, "plans");

    let error = refused(
        &run_as(
            &fx,
            &lead,
            &[
                "plan",
                "new",
                "shape1",
                "--context",
                "ctx",
                "--title",
                "Shape",
                "--shaping",
            ],
        ),
        "shaping_manager_only",
        2,
    );
    assert_eq!(error["details"]["caller"], json!("lead"), "{error}");
    assert_eq!(count(&fx, "plans"), plans, "nothing is written");
}

#[test]
fn shaping_cmds_manager_plan_new_without_flag_is_unchanged() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    // A shaping Plan in the same Context, so the test runs the feature it checks alongside.
    new_shaping(&fx, &manager, "ctx", "shape1");

    let out = run_as(
        &fx,
        &manager,
        &[
            "plan",
            "new",
            "pl",
            "--context",
            "ctx",
            "--title",
            "A Lead's Plan",
        ],
    )
    .ok();
    let lead = out["plan"]["lead_session"]
        .as_str()
        .expect("plan new without --shaping still registers a Lead")
        .to_owned();
    assert_eq!(out["plan"]["phase"], json!("grounding"));
    assert_eq!(
        fx.env
            .query::<String>(&format!("SELECT role FROM sessions WHERE id = '{lead}'")),
        "lead"
    );
    assert!(
        fx.env.cpd_home().join("run/pl/lead-message.md").exists(),
        "plan new without --shaping still writes the first message"
    );
    assert_eq!(phase(&fx, "pl"), "grounding");
}

// ---------- job add on a shaping Plan ----------

#[test]
fn shaping_cmds_manager_adds_scout_to_shaping_plan() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");

    let out = add_scout(&fx, &manager, "shape1", "scan", "H2").ok();
    assert_eq!(
        (
            &out["job"]["key"],
            &out["job"]["kind"],
            &out["job"]["status"],
            &out["job"]["pr"],
        ),
        (
            &json!("scan"),
            &json!("scout"),
            &json!("queued"),
            &Value::Null
        ),
        "{out}"
    );

    let (status, tier, touches): (String, String, String) = fx.env.read(|c| {
        c.query_row(
            "SELECT status, tier, touches FROM jobs WHERE plan = 'shape1' AND key = 'scan'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    });
    assert_eq!(status, "queued", "queued at once, as for a Lead");
    assert_eq!(tier, "H2");
    assert_eq!(touches, json!(["run/shape1/explore/scan.md"]).to_string());
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT length(fingerprint) FROM jobs WHERE plan = 'shape1' AND key = 'scan'"
        ),
        64,
        "the Job carries its Locked-Job fingerprint"
    );

    // The Tier rules are the kind's, as for a Lead: O2 is no scout Tier.
    let spec = explore_spec(&fx.env, "bad.json");
    let refused_run = add_kind(&fx, &manager, "shape1", "bad", "scout", "O2", &spec);
    refused(&refused_run, "tier_not_allowed", 2);
    assert_eq!(count(&fx, "jobs"), 1, "a refused Tier writes nothing");
}

#[test]
fn shaping_cmds_manager_add_non_explore_kind_is_refused() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");

    // A kind other than scout or research.
    let work = work_spec(&fx.env, "work.json");
    let error = refused(
        &add_kind(&fx, &manager, "shape1", "w", "work", "S2", &work),
        "shaping_explore_only",
        2,
    );
    assert_eq!(
        (&error["details"]["plan"], &error["details"]["kind"]),
        (&json!("shape1"), &json!("work")),
        "{error}"
    );

    // --workflow on an explore kind: the same refusal.
    let spec = explore_spec(&fx.env, "s.json");
    let error = refused(
        &run_as(
            &fx,
            &manager,
            &[
                "job",
                "add",
                "s",
                "--title",
                "S",
                "--tier",
                "H2",
                "--spec",
                &spec,
                "--kind",
                "scout",
                "--workflow",
                "api",
                "--plan",
                "shape1",
            ],
        ),
        "shaping_explore_only",
        2,
    );
    assert_eq!(error["details"]["workflow"], json!("api"), "{error}");

    // --draft stays the explore no-draft refusal.
    refused(
        &run_as(
            &fx,
            &manager,
            &[
                "job", "add", "d", "--title", "D", "--tier", "H2", "--spec", &spec, "--kind",
                "scout", "--draft", "--plan", "shape1",
            ],
        ),
        "explore_no_draft",
        3,
    );

    assert_eq!(count(&fx, "jobs"), 0, "nothing is written");
}

#[test]
fn shaping_cmds_manager_add_to_lead_plan_is_refused() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    fx.cpd(&[
        "plan",
        "new",
        "pl",
        "--context",
        "ctx",
        "--title",
        "A Lead's Plan",
    ])
    .ok();

    let error = refused(
        &add_scout(&fx, &manager, "pl", "scan", "H2"),
        "plan_has_lead",
        3,
    );
    assert_eq!(error["details"]["plan"], json!("pl"), "{error}");
    assert_eq!(count(&fx, "jobs"), 0, "nothing is written");
}

#[test]
fn shaping_cmds_manager_add_to_other_context_plan_is_refused() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    context(&fx, "ctx2");
    let manager2 = manager_of(&fx, "ctx2");
    new_shaping(&fx, &manager2, "ctx2", "other");

    let error = refused(
        &add_scout(&fx, &manager, "other", "scan", "H2"),
        "plan_other_context",
        2,
    );
    assert_eq!(
        (
            &error["details"]["plan"],
            &error["details"]["plan_context"],
            &error["details"]["context"],
        ),
        (&json!("other"), &json!("ctx2"), &json!("ctx")),
        "{error}"
    );
    assert_eq!(count(&fx, "jobs"), 0, "nothing is written");
}

#[test]
fn shaping_cmds_manager_job_add_reopens_done_shaping_plan() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");
    fx.env
        .sql("UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'shape1';");
    assert_eq!(phase(&fx, "shape1"), "done");

    add_scout(&fx, &manager, "shape1", "scan", "H2").ok();

    assert_eq!(
        phase(&fx, "shape1"),
        "shaping",
        "an explore add reopens a done Lead-less Plan to shaping"
    );
    let phases = events(&fx, "plan-phase");
    let last = phases.last().expect("a plan-phase Event");
    assert_eq!(
        (&last["data"]["from"], &last["data"]["to"]),
        (&json!("done"), &json!("shaping")),
        "{phases:?}"
    );
}

// ---------- job list, job show, cancel and retry ----------

#[test]
fn shaping_cmds_manager_lists_and_shows_jobs_of_shaping_plan() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &manager, "shape1", "scan", "H2").ok();

    let listed = run_as(&fx, &manager, &["job", "list", "--plan", "shape1"]).ok();
    let jobs = listed["jobs"].as_array().expect("job list lists jobs");
    let found = jobs
        .iter()
        .find(|j| j["key"] == "scan")
        .unwrap_or_else(|| panic!("job list shows the scout: {listed}"));
    assert_eq!(found["kind"], json!("scout"));
    assert_eq!(found["status"], json!("queued"));
    assert_eq!(found["tier"], json!("H2"));

    let shown = run_as(&fx, &manager, &["job", "show", "scan", "--plan", "shape1"]).ok();
    assert_eq!(shown["job"]["key"], json!("scan"));
    assert_eq!(shown["job"]["kind"], json!("scout"));
    assert_eq!(
        shown["job"]["artefact"],
        json!("run/shape1/explore/scan.md")
    );
    assert_eq!(shown["job"]["summary"], Value::Null);
}

#[test]
fn shaping_cmds_manager_cancels_and_retries_explore_job() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &manager, "shape1", "scan", "H2").ok();
    add_scout(&fx, &manager, "shape1", "probe", "H2").ok();
    fx.env.sql(
        "UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE plan = 'shape1' AND key = 'probe';",
    );

    let out = run_as(
        &fx,
        &manager,
        &[
            "job",
            "cancel",
            "scan",
            "--reason",
            "no longer needed",
            "--plan",
            "shape1",
        ],
    )
    .ok();
    assert_eq!(out["status"], json!("cancelled"), "{out}");
    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM jobs WHERE plan = 'shape1' AND key = 'scan'"),
        "cancelled"
    );

    let out = run_as(
        &fx,
        &manager,
        &["job", "retry", "probe", "--tier", "H2", "--plan", "shape1"],
    )
    .ok();
    assert_eq!(
        (&out["status"], &out["tier"]),
        (&json!("queued"), &json!("H2")),
        "{out}"
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM jobs WHERE plan = 'shape1' AND key = 'probe'"),
        "queued"
    );
}

// ---------- answer and the attempt commands ----------

#[test]
fn shaping_cmds_manager_answers_escalation_of_shaping_plan() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &manager, "shape1", "scan", "H2").ok();
    fx.env.sql(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, runner)
           VALUES ('shape1', 'scan-a1', 'scan', 1, 'H2', 'ladder', 'running', 'cpd/shape1--scan-a1', '/w', 't', 'durable');
         INSERT INTO escalations (plan, id, attempt, number, question, status, answer, asker_pid, asked_at)
           VALUES ('shape1', 'scan-a1-q1', 'scan-a1', 1, 'Which field names?', 'open', NULL, 1, 't');",
    );

    run_as(
        &fx,
        &manager,
        &[
            "answer",
            "scan-a1-q1",
            "Use the v2 names.",
            "--plan",
            "shape1",
        ],
    )
    .ok();
    assert_eq!(
        fx.env.query::<String>(
            "SELECT status FROM escalations WHERE plan = 'shape1' AND id = 'scan-a1-q1'"
        ),
        "answered"
    );
}

#[test]
fn shaping_cmds_manager_attempt_commands_on_shaping_plan() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");
    add_scout(&fx, &manager, "shape1", "scan", "H2").ok();
    // A running Attempt whose Worker Session's file is a readable pi transcript.
    let transcript = fx.env.root().join("w1.jsonl");
    fs::write(
        &transcript,
        "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"}}\n",
    )
    .unwrap();
    fx.env.sql(&format!(
        "UPDATE jobs SET status = 'running' WHERE plan = 'shape1' AND key = 'scan';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at, runner)
           VALUES ('shape1', 'scan-a1', 'scan', 1, 'H2', 'ladder', 'running', 'cpd/shape1--scan-a1', '/w', 'w1', 't', 'pi');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, state, state_at, created_at)
           VALUES ('w1', 'worker', 'ctx', 'shape1', 'scan-a1', 'scan-a1', '{}', '/w', 'H2', 'working', 't', 't');",
        transcript.display()
    ));

    let listed = run_as(&fx, &manager, &["attempt", "list", "--plan", "shape1"]).ok();
    let attempts = listed["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 1, "{listed}");
    assert_eq!(attempts[0]["id"], json!("scan-a1"));

    let shown = run_as(
        &fx,
        &manager,
        &["attempt", "show", "scan-a1", "--plan", "shape1"],
    )
    .ok();
    assert_eq!(shown["attempt"]["id"], json!("scan-a1"));

    let read = run_as(
        &fx,
        &manager,
        &["attempt", "read", "scan-a1", "--plan", "shape1"],
    )
    .ok();
    assert_eq!(read["lines"], json!(["assistant: Hello"]), "{read}");

    let aborted = run_as(
        &fx,
        &manager,
        &["attempt", "abort", "scan-a1", "--plan", "shape1"],
    )
    .ok();
    assert_eq!(
        (&aborted["outcome"], &aborted["cause"], &aborted["attempt"]),
        (&json!("failed"), &json!("aborted"), &json!("scan-a1")),
        "{aborted}"
    );
}

// ---------- the Lead-only commands ----------

#[test]
fn shaping_cmds_manager_phase_lock_and_workflow_stay_refused() {
    let fx = setup();
    let manager = manager_of(&fx, "ctx");
    new_shaping(&fx, &manager, "ctx", "shape1");

    // Each names no Manager right: `run.error()` panics unless the command was refused.
    run_as(
        &fx,
        &manager,
        &["plan", "phase", "plan", "--plan", "shape1"],
    )
    .error();
    run_as(&fx, &manager, &["plan", "lock", "--plan", "shape1"]).error();
    let repo = fx.env.root().display().to_string();
    run_as(
        &fx,
        &manager,
        &[
            "workflow", "add", "pay", "--repo", &repo, "--title", "Change", "--plan", "shape1",
        ],
    )
    .error();

    assert_eq!(phase(&fx, "shape1"), "shaping", "nothing moved the Plan");
}
