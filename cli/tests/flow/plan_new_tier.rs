//! `plan new`'s Lead Tier (commands.md §4.1, contract §2.2, §8.2; pro-d139, pro-d140): the default (`lead_tier`, in every
//! Context), `--tier`, its refusals and its idempotency. On the fake tmux; the Tier catalog is seeded in the
//! store with a fake pi install, so no real pi, node or tmux runs.

use crate::common;

use std::fs;

use common::fx::Fx;
use serde_json::Value;

/// The config: the role tiers GX / H3 / `lead` / O3, the review Context `review`, and `extra` (top-level keys, before
/// `[inbox]`). On the catalog of [`seed_catalog`] the lead range is `lead` up to O2 (just below `manager_tier`).
fn config_with(fx: &Fx, lead: &str, extra: &str, review_context: &str) {
    fx.env.write_tmux_config(&format!(
        "context_distill_tier = \"GX\"\nworker_tier = \"H3\"\nlead_tier = \"{lead}\"\nmanager_tier = \"O3\"\n{extra}\n[inbox]\nreview_context = \"{review_context}\"\n"
    ));
}

/// The default config: `lead_tier = "S4"`, so the lead range is S4..O2.
fn config(fx: &Fx, extra: &str, review_context: &str) {
    config_with(fx, "S4", extra, review_context);
}

/// Contexts `ctx` and `review`, a fake pi install with a Tier catalog (S 0-4, O 0, 2, 3, 4, H 0-3, G none), and the cockpit.
fn setup(fx: &Fx, extra: &str, review_context: &str) {
    fx.env.write_models();
    config(fx, extra, review_context);
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
    fx.env.owner_policy("acme", "");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.cpd(&[
        "context", "new", "review", "--title", "Review", "--owner", "acme",
    ])
    .ok();
    seed_catalog(fx, "[0,1,2,3,4]", "[0,2,3,4]");
    fx.cockpit();
}

/// The catalog rows: `s` and `o` efforts, H 0-3.
fn seed_catalog(fx: &Fx, s: &str, o: &str) {
    let rows: String = [("O", "p/opus", o), ("S", "p/sonnet", s), ("H", "p/haiku", "[0,1,2,3]"), ("G", "p/gemini", "[]")]
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
    fx.cpd(&["session", "show", session]).ok()["session"]["tier"]
        .as_str()
        .unwrap()
        .to_owned()
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
    (
        count(fx, "txns"),
        count(fx, "events"),
        count(fx, "plans"),
        count(fx, "sessions"),
    )
}

/// The refusal of the lead range `floor`..`ceiling`, set by `lead_tier` and below `manager_tier`.
fn not_allowed(run: &common::Run, tier: &str, floor: &str, ceiling: &str) {
    let e = common::refused(run, "tier_not_allowed", 2);
    assert_eq!(
        e["details"],
        serde_json::json!({
            "tier": tier,
            "role": "lead",
            "floor": floor,
            "ceiling": ceiling,
            "allowed": format!("{floor}-{ceiling}"),
            "key": "lead_tier",
        }),
        "{run:?}"
    );
    let message = e["message"].as_str().unwrap();
    for part in [
        tier,
        "lead",
        &format!("{floor}-{ceiling}"),
        "lead_tier",
        "manager_tier",
    ] {
        assert!(message.contains(part), "`{part}` missing from: {message}");
    }
}

#[test]
fn a_plan_defaults_its_lead_to_lead_tier_in_every_context() {
    let fx = Fx::with_ports();
    setup(&fx, "", "review");
    assert_eq!(lead_tier(&fx, "pay", "ctx", None), "S4");
    assert_eq!(lead_tier(&fx, "rv", "review", None), "S4");
}

#[test]
fn with_no_review_context_a_plan_defaults_its_lead_to_lead_tier() {
    let fx = Fx::with_ports();
    setup(&fx, "", "");
    assert_eq!(lead_tier(&fx, "pay", "ctx", None), "S4");
    assert_eq!(lead_tier(&fx, "rv", "review", None), "S4");
}

/// `--tier O2` wins in `context`: the session row and the `session-registered` Event say O2.
fn override_wins(context: &str) {
    let fx = Fx::with_ports();
    setup(&fx, "", "review");
    assert_eq!(lead_tier(&fx, "pl", context, Some("O2")), "O2");
    let registered = fx
        .cpd(&["events", "list", "--kind", "session-registered"])
        .ok()["events"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        registered.last().unwrap()["data"]["tier"],
        "O2",
        "{registered:?}"
    );
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
fn tier_flag_refuses_effort_5_as_tier_invalid_and_writes_nothing() {
    let fx = Fx::with_ports();
    setup(&fx, "", "review");
    seed_catalog(&fx, "[0,1,2,3,4,5]", "[0,1,2,3,4]");
    let before = counts(&fx);
    // Tier model v2: effort 5 exists for no letter, so it is a syntax error, not a role refusal.
    let e = common::refused(&plan_new(&fx, "pl", "ctx", Some("S5")), "tier_invalid", 2);
    assert_eq!(e["details"]["tier"], "S5", "{e}");
    assert_eq!(counts(&fx), before);
}

#[test]
fn tier_flag_refuses_an_effort_pi_does_not_support_and_writes_nothing() {
    let fx = Fx::with_ports();
    setup(&fx, "", "review");
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
    setup(&fx, "", "review");
    plan_new(&fx, "pl", "ctx", Some("O2")).ok();
    assert_eq!(plan_new(&fx, "pl", "ctx", Some("O2")).ok()["already"], true);
    assert_eq!((count(&fx, "plans"), count(&fx, "sessions")), (1, 1));
}

#[test]
fn a_rerun_with_another_tier_is_plan_exists() {
    let fx = Fx::with_ports();
    setup(&fx, "", "review");
    plan_new(&fx, "pl", "ctx", Some("O2")).ok();
    common::refused(&plan_new(&fx, "pl", "ctx", Some("S4")), "plan_exists", 3);
    // A first run without `--tier` stored `lead_tier`: the same Tier is `already`, another is `plan_exists`.
    plan_new(&fx, "p2", "ctx", None).ok();
    assert_eq!(plan_new(&fx, "p2", "ctx", Some("S4")).ok()["already"], true);
    common::refused(&plan_new(&fx, "p2", "ctx", Some("O2")), "plan_exists", 3);
}

#[test]
fn a_rerun_without_tier_compares_no_tier() {
    let fx = Fx::with_ports();
    setup(&fx, "", "review");
    let first = plan_new(&fx, "pl", "ctx", Some("O2")).ok();
    assert_eq!(plan_new(&fx, "pl", "ctx", None).ok()["already"], true);
    assert_eq!(tier_of(&fx, &lead(&first)), "O2");
}

/// Roles GX / H3 / S4 / O3 on S [0-4], O [0,2,3,4], H [0-3], G []: the lead range is S4..O2. A Tier outside it is
/// `tier_not_allowed` with the details of the range and nothing written; one inside is taken. Then `lead_tier = S2`
/// moves the floor: the range is S2..O2.
#[test]
fn plan_new_tier_flag_is_checked_against_the_lead_range() {
    let fx = Fx::with_ports();
    setup(&fx, "", "review");
    let before = counts(&fx);
    for tier in ["S3", "H3", "GX", "O3", "O4"] {
        not_allowed(&plan_new(&fx, "pl", "ctx", Some(tier)), tier, "S4", "O2");
    }
    assert_eq!(counts(&fx), before);
    // `tier_invalid` first: O1 is no effort of O.
    common::refused(&plan_new(&fx, "pl", "ctx", Some("O1")), "tier_invalid", 2);
    assert_eq!(counts(&fx), before);
    // The ends and the middle of the range are taken.
    for (id, tier) in [("pa", "S4"), ("pb", "O0"), ("pc", "O2")] {
        assert_eq!(lead_tier(&fx, id, "ctx", Some(tier)), tier);
    }
    // A second config moves the floor.
    config_with(&fx, "S2", "", "review");
    for tier in ["S1", "H3"] {
        not_allowed(&plan_new(&fx, "pl", "ctx", Some(tier)), tier, "S2", "O2");
    }
    assert_eq!(lead_tier(&fx, "pd", "ctx", Some("S3")), "S3");
}
