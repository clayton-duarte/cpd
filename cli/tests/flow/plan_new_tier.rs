//! `plan new`'s Lead Tier (commands.md §4.1, contract §2.2, §8.2; pro-d139, pro-d140): the default for a Plan in the review
//! Context (`review_lead_tier`), `--tier`, its refusals and its idempotency. On the fake tmux; the Tier catalog is seeded in the
//! store with a fake pi install, so no real pi, node or tmux runs.

use crate::common;

use std::fs;

use common::fx::Fx;
use serde_json::Value;

const ALLOWED: &str = "O0-O4, S0-S4, H0-H4";

/// The config: `lead_tier = "O2"`, the review Context `review`, and `extra` (top-level keys, before `[inbox]`).
fn config(fx: &Fx, extra: &str, review_context: &str) {
    fx.env.write_tmux_config(&format!("lead_tier = \"O2\"\n{extra}\n[inbox]\nreview_context = \"{review_context}\"\n"));
}

/// Contexts `ctx` and `review`, a fake pi install with a Tier catalog (O and S efforts 0-4, H 0-3), and the cockpit.
fn setup(fx: &Fx, extra: &str, review_context: &str) {
    fx.env.write_models();
    config(fx, extra, review_context);
    let scope = fx.env.root().join("install/releases/1.0.0/node_modules/@earendil-works");
    fs::create_dir_all(fx.env.root().join("install")).unwrap();
    fs::write(fx.env.root().join("install/current-version"), "1.0.0\n").unwrap();
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    fx.env.owner_policy("acme", "");
    fx.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    fx.cpd(&["context", "new", "review", "--title", "Review", "--owner", "acme"]).ok();
    seed_catalog(fx, "[0,1,2,3,4]", "[0,1,2,3,4]");
    fx.cockpit();
}

/// The catalog rows: `s` and `o` efforts, H 0-3.
fn seed_catalog(fx: &Fx, s: &str, o: &str) {
    let rows: String = [("O", "p/opus", o), ("S", "p/sonnet", s), ("H", "p/haiku", "[0,1,2,3]")]
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT OR REPLACE INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    fx.env.sql(&rows);
}

fn plan_new(fx: &Fx, id: &str, context: &str, tier: Option<&str>) -> common::Run {
    let mut args = vec!["plan", "new", id, "--context", context, "--title", "Plan"];
    if let Some(tier) = tier {
        args.extend(["--tier", tier]);
    }
    fx.cpd(&args)
}

fn lead(out: &Value) -> String {
    out["lead"]["session"].as_str().unwrap().to_owned()
}

fn tier_of(fx: &Fx, session: &str) -> String {
    fx.cpd(&["session", "show", session]).ok()["session"]["tier"].as_str().unwrap().to_owned()
}

/// The Tier a new Plan's Lead gets, read from the session.
fn lead_tier(fx: &Fx, id: &str, context: &str, tier: Option<&str>) -> String {
    let out = plan_new(fx, id, context, tier).ok();
    tier_of(fx, &lead(&out))
}

fn count(fx: &Fx, table: &str) -> i64 {
    fx.env.query(&format!("SELECT count(*) FROM {table}"))
}

fn counts(fx: &Fx) -> (i64, i64, i64, i64) {
    (count(fx, "txns"), count(fx, "events"), count(fx, "plans"), count(fx, "sessions"))
}

fn not_allowed(run: &common::Run, tier: &str) {
    let e = common::refused(run, "tier_not_allowed", 2);
    assert_eq!(e["details"]["tier"], tier, "{run:?}");
    assert_eq!(e["details"]["allowed"], ALLOWED, "{run:?}");
    assert_eq!(e["details"]["rule"], "hs-d506, hs-d562", "{run:?}");
    let message = e["message"].as_str().unwrap();
    assert!(message.contains("Lead") && message.contains(tier), "{message}");
}

#[test]
fn a_review_context_plan_defaults_its_lead_to_review_lead_tier() {
    let fx = Fx::with_ports();
    setup(&fx, "", "review");
    assert_eq!(lead_tier(&fx, "rv", "review", None), "S3");
}

#[test]
fn a_plan_in_another_context_defaults_its_lead_to_lead_tier() {
    let fx = Fx::with_ports();
    setup(&fx, "review_lead_tier = \"H1\"", "review");
    assert_eq!(lead_tier(&fx, "pay", "ctx", None), "O2");
    assert_eq!(lead_tier(&fx, "rv", "review", None), "H1");
}

#[test]
fn with_no_review_context_a_plan_defaults_its_lead_to_lead_tier() {
    let fx = Fx::with_ports();
    setup(&fx, "review_lead_tier = \"H1\"", "");
    assert_eq!(lead_tier(&fx, "pay", "ctx", None), "O2");
    assert_eq!(lead_tier(&fx, "rv", "review", None), "O2");
}

/// `--tier S2` wins in `context`: the session row and the `session-registered` Event say S2.
fn override_wins(context: &str) {
    let fx = Fx::with_ports();
    setup(&fx, "review_lead_tier = \"H1\"", "review");
    assert_eq!(lead_tier(&fx, "pl", context, Some("S2")), "S2");
    let registered = fx.cpd(&["events", "list", "--kind", "session-registered"]).ok()["events"].as_array().unwrap().clone();
    assert_eq!(registered.last().unwrap()["data"]["tier"], "S2", "{registered:?}");
}

#[test]
fn tier_flag_overrides_the_default_in_the_review_context() {
    override_wins("review");
}

#[test]
fn tier_flag_overrides_the_default_in_another_context() {
    override_wins("ctx");
}

#[test]
fn tier_flag_refuses_a_g_tier_and_writes_nothing() {
    let fx = Fx::with_ports();
    setup(&fx, "review_lead_tier = \"S3\"", "review");
    let before = counts(&fx);
    not_allowed(&plan_new(&fx, "pl", "ctx", Some("GX")), "GX");
    assert_eq!(counts(&fx), before);
}

#[test]
fn tier_flag_refuses_effort_5_and_writes_nothing() {
    let fx = Fx::with_ports();
    setup(&fx, "review_lead_tier = \"S3\"", "review");
    seed_catalog(&fx, "[0,1,2,3,4,5]", "[0,1,2,3,4]");
    let before = counts(&fx);
    not_allowed(&plan_new(&fx, "pl", "ctx", Some("S5")), "S5");
    assert_eq!(counts(&fx), before);
}

#[test]
fn tier_flag_refuses_an_effort_pi_does_not_support_and_writes_nothing() {
    let fx = Fx::with_ports();
    setup(&fx, "review_lead_tier = \"S3\"", "review");
    let before = counts(&fx);
    // H lists efforts 0-3 only.
    let e = common::refused(&plan_new(&fx, "pl", "ctx", Some("H4")), "tier_invalid", 2);
    assert!(!e["details"]["efforts"].is_null(), "{e}");
    for bad in ["S9", "x"] {
        common::refused(&plan_new(&fx, "pl", "ctx", Some(bad)), "tier_invalid", 2);
    }
    assert_eq!(counts(&fx), before);
}

#[test]
fn a_rerun_with_the_same_tier_is_already() {
    let fx = Fx::with_ports();
    setup(&fx, "review_lead_tier = \"S3\"", "review");
    plan_new(&fx, "pl", "ctx", Some("S2")).ok();
    assert_eq!(plan_new(&fx, "pl", "ctx", Some("S2")).ok()["already"], true);
    assert_eq!((count(&fx, "plans"), count(&fx, "sessions")), (1, 1));
}

#[test]
fn a_rerun_with_another_tier_is_plan_exists() {
    let fx = Fx::with_ports();
    setup(&fx, "review_lead_tier = \"S3\"", "review");
    plan_new(&fx, "pl", "ctx", Some("S2")).ok();
    common::refused(&plan_new(&fx, "pl", "ctx", Some("S1")), "plan_exists", 3);
    // A first run without `--tier` stored `lead_tier`: the same Tier is `already`, another is `plan_exists`.
    plan_new(&fx, "p2", "ctx", None).ok();
    assert_eq!(plan_new(&fx, "p2", "ctx", Some("O2")).ok()["already"], true);
    common::refused(&plan_new(&fx, "p2", "ctx", Some("S1")), "plan_exists", 3);
}

#[test]
fn a_rerun_without_tier_compares_no_tier() {
    let fx = Fx::with_ports();
    setup(&fx, "review_lead_tier = \"S3\"", "review");
    let first = plan_new(&fx, "pl", "ctx", Some("S2")).ok();
    assert_eq!(plan_new(&fx, "pl", "ctx", None).ok()["already"], true);
    assert_eq!(tier_of(&fx, &lead(&first)), "S2");
}
