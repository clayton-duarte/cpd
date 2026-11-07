//! plan list, plan show and context pending with a Lead-less shaping Plan (air-d19,
//! air-d22; commands.md sections 4.2, 4.3 and 3.11): the command tree in this process
//! (Fx) against a temporary CPD_HOME whose Plan is written straight into the store,
//! because plan new --shaping does not exist yet.

use crate::common;

use std::fs;

use common::TestEnv;
use common::fx::Fx;
use cpd_core::home::CpdHome;
use serde_json::{Value, json};

/// The fingerprint a Job carries (64 hex characters, contract section 4.4).
const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// An Fx with a Context ctx and nothing else.
fn env() -> Fx {
    let fx = Fx::new();
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');",
    );
    fx
}

/// A Lead-less Plan, phase shaping, with a null lead_session and no Lead Session row.
fn shaping(env: &TestEnv, id: &str) {
    env.sql(&format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at)
         VALUES ('{id}', 'ctx', 'Title of {id}', 'shaping', NULL, '2000-01-01T00:00:00.000Z');"
    ));
}

/// A queued explore Job of the Plan, scout or research, touching explore/<key>.md.
fn explore(env: &TestEnv, plan: &str, key: &str, kind: &str) {
    env.sql(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                           kind, status, appended, fingerprint, created_txn, created_at)
         VALUES ('{plan}', NULL, '{key}', 'Title {key}', 'H2', 'Goal.', 'Context.',
                 '[\"explore/{key}.md\"]', '[\"ok\"]', '[]', '{kind}', 'queued', 1, '{FP}', cpd_txn(), 't');"
    ));
}

#[test]
fn shaping_listing_plan_list_shows_shaping_plan() {
    let fx = env();
    shaping(&fx.env, "shape1");
    let out = fx.cpd(&["plan", "list"]).ok();
    let plans = out["plans"].as_array().expect("plan list lists plans");
    let plan = plans
        .iter()
        .find(|p| p["id"] == "shape1")
        .unwrap_or_else(|| panic!("plan list shows the shaping Plan: {out}"));
    assert_eq!(plan["phase"], json!("shaping"));
    assert_eq!(
        plan["lead_session"],
        Value::Null,
        "a Lead-less Plan has a null lead_session"
    );
    assert_eq!(plan["workflows"], json!([]));
}

#[test]
fn shaping_listing_plan_show_has_null_lead() {
    let fx = env();
    shaping(&fx.env, "shape1");
    let out = fx.cpd(&["plan", "show", "--plan", "shape1"]).ok();
    assert_eq!(out["plan"]["phase"], json!("shaping"));
    assert_eq!(
        out["plan"]["lead_session"],
        Value::Null,
        "plan show does not crash on the null lead_session"
    );
}

#[test]
fn shaping_listing_context_pending_lists_shaping_plan() {
    let fx = env();
    shaping(&fx.env, "shape1");
    explore(&fx.env, "shape1", "scan", "scout");
    let out = fx.cpd(&["context", "pending", "--context", "ctx"]).ok();
    let plans = out["plans"]
        .as_array()
        .expect("context pending lists plans");
    assert_eq!(plans.len(), 1, "{out}");
    let plan = &plans[0];
    assert_eq!(
        (
            plan["id"].clone(),
            plan["phase"].clone(),
            plan["lead_session"].clone()
        ),
        (json!("shape1"), json!("shaping"), Value::Null),
        "{out}"
    );
    assert_eq!(plan["workflows"], json!([]));
    assert_eq!(
        plan["jobs"],
        json!([{"key": "scan", "kind": "scout", "status": "queued", "tier": "H2", "title": "Title scan", "touches": ["explore/scan.md"]}]),
        "{out}"
    );
    // --markdown prints on the process's own stdout (print!): the binary, which Fx cannot read.
    let run = fx
        .env
        .cpd(&["context", "pending", "--context", "ctx", "--markdown"]);
    assert_eq!(run.exit, 0, "{run:?}");
    assert_eq!(
        run.stdout,
        "# Pending work in ctx\n\n## shape1 (shaping): Title of shape1\n\n### Jobs\n\n- scan scout queued: Title scan (explore/scan.md)\n"
    );
}
