//! `cpd session retier <id> --tier <T>` (commands.md §10.15, contract §2.2, §9; pro-d142): one Session to a Tier the
//! User names, instead of the config's target.
//!
//! The catalog `--tier` reads is seeded in the store, with a fake pi install: no real pi, node or tmux runs.

use crate::common::fx::Fx;
use crate::common::session::{events, setup, show};
use crate::common::{TestEnv, refused};

use std::fs;

use serde_json::json;

const LEAD1: &str = "lead1";
const LEAD2: &str = "lead2";
const MANAGER: &str = "manager1";
const WORKER: &str = "worker1";

/// The config: roles GX / H3 / `lead` / O3 (plus `tables`). On the catalog of [`fixture`] the lead range is `lead` up to
/// O2 and the manager range O3..O4.
fn config(env: &TestEnv, lead: &str, tables: &str) {
    env.write_tmux_config(&format!(
        "context_distill_tier = \"GX\"\nworker_tier = \"H3\"\nlead_tier = \"{lead}\"\nmanager_tier = \"O3\"\n{tables}\n[timeouts]\nfork_quiet_seconds = 1\n"
    ));
}

/// `session.rs`'s fixture (Leads `lead1` on `pay`, `lead2`, Manager `manager1` on `O2`, Worker `worker1`), the config
/// of [`config`] with `lead_tier = "S4"` (plus `tables`), a fake pi install, and a catalog of `efforts` for O, S and H
/// (G has none).
fn fixture(env: &TestEnv, tables: &str, efforts: [&str; 3]) {
    setup(env);
    env.write_models();
    config(env, "S4", tables);
    env.fake_pi("exit 0");
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env
        .root()
        .join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    let rows: String = [("O", "p/opus", efforts[0]), ("S", "p/sonnet", efforts[1]), ("H", "p/haiku", efforts[2]), ("G", "p/gemini", "[]")]
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    env.owner_policy("acme", "");
    env.sql(&format!(
        "UPDATE contexts SET owner = 'acme' WHERE id = 'ctx'; {rows}"
    ));
}

/// O has efforts 0, 2, 3, 4, S 0-4, H 0-3.
const FOUR: [&str; 3] = ["[0,2,3,4]", "[0,1,2,3,4]", "[0,1,2,3]"];
/// Efforts 0-5 for S.
const FIVE: [&str; 3] = ["[0,2,3,4]", "[0,1,2,3,4,5]", "[0,1,2,3]"];
const REVIEW: &str = "[inbox]\nreview_context = \"ctx\"\n";

fn tier(fx: &Fx, id: &str) -> String {
    show(fx, id)["tier"].as_str().unwrap().to_owned()
}

fn tiers(fx: &Fx) -> Vec<(String, String)> {
    let all = fx.cpd(&["session", "list"]).ok();
    let mut out: Vec<(String, String)> = all["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["id"].as_str().unwrap().to_owned(),
                s["tier"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    out.sort();
    out
}

/// Everything a refusal must leave as it was: every Tier, the Events, the transactions.
fn state(fx: &Fx) -> (Vec<(String, String)>, i64, i64) {
    (
        tiers(fx),
        fx.env.query("SELECT count(*) FROM events"),
        fx.env.query("SELECT count(*) FROM txns"),
    )
}

/// A refusal of the `role` range `floor`..`ceiling`, whose floor `key` sets; it changes nothing.
fn not_allowed(fx: &Fx, id: &str, tier: &str, role: &str, floor: &str, ceiling: &str, key: &str) {
    let before = state(fx);
    let e = refused(
        &fx.cpd(&["session", "retier", id, "--tier", tier]),
        "tier_not_allowed",
        2,
    );
    assert_eq!(
        e["details"],
        json!({"tier": tier, "role": role, "floor": floor, "ceiling": ceiling, "allowed": format!("{floor}-{ceiling}"), "key": key}),
        "{id} {tier}"
    );
    let message = e["message"].as_str().unwrap();
    for part in [tier, role, &format!("{floor}-{ceiling}"), key] {
        assert!(message.contains(part), "`{part}` missing from: {message}");
    }
    assert_eq!(state(fx), before, "a refusal changes nothing");
}

#[test]
fn retier_tier_moves_one_session_to_that_tier_with_one_event() {
    let fx = Fx::new();
    fixture(&fx.env, "", FOUR);
    let out = fx.cpd(&["session", "retier", LEAD1, "--tier", "S4"]).ok();
    assert_eq!(out["retiered"].as_array().unwrap().len(), 1, "{out}");
    let one = &out["retiered"][0];
    assert_eq!(
        (
            one["session"].as_str(),
            one["from"].as_str(),
            one["to"].as_str()
        ),
        (Some(LEAD1), Some("O3"), Some("S4"))
    );
    assert_eq!(out["unchanged"], json!([]));

    assert_eq!(tier(&fx, LEAD1), "S4");
    let shown = show(&fx, LEAD1);
    assert_eq!(
        (shown["model"].as_str(), shown["thinking"].as_str()),
        (Some("p/sonnet"), Some("xhigh")),
        "{shown}"
    );
    let sent = events(&fx, "session-retiered");
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["session"], LEAD1);
    assert_eq!(sent[0]["data"], json!({"from": "O3", "to": "S4"}));

    assert_eq!(tier(&fx, LEAD2), "O3");
    assert_eq!(tier(&fx, MANAGER), "O2");
    assert_eq!(tier(&fx, WORKER), "S2");
}

#[test]
fn retier_tier_moves_the_manager_too() {
    let fx = Fx::new();
    fixture(&fx.env, "", FOUR);
    fx.cpd(&["session", "retier", MANAGER, "--tier", "O4"]).ok();
    assert_eq!(tier(&fx, MANAGER), "O4", "not manager_tier O3");
    let sent = events(&fx, "session-retiered");
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0]["session"].as_str(), sent[0]["data"].clone()),
        (Some(MANAGER), json!({"from": "O2", "to": "O4"}))
    );
}

#[test]
fn retier_tier_refuses_effort_5_as_tier_invalid_and_changes_nothing() {
    let fx = Fx::new();
    fixture(&fx.env, "", FIVE);
    // Tier model v2: effort 5 exists for no letter, so it is a syntax error, not a role refusal.
    let e = refused(
        &fx.cpd(&["session", "retier", LEAD1, "--tier", "S5"]),
        "tier_invalid",
        2,
    );
    assert_eq!(e["details"]["tier"], "S5", "{e}");
}

#[test]
fn retier_tier_refuses_an_effort_pi_does_not_support_and_changes_nothing() {
    let fx = Fx::new();
    fixture(&fx.env, "", FOUR);
    let before = state(&fx);
    let e = refused(
        &fx.cpd(&["session", "retier", LEAD1, "--tier", "H4"]),
        "tier_invalid",
        2,
    );
    assert!(e["details"].get("efforts").is_some(), "{e}");
    refused(
        &fx.cpd(&["session", "retier", LEAD1, "--tier", "x9"]),
        "tier_invalid",
        2,
    );
    assert_eq!(state(&fx), before);
}

#[test]
fn retier_tier_with_all_is_usage_and_changes_nothing() {
    let fx = Fx::new();
    fixture(&fx.env, "", FOUR);
    let before = state(&fx);
    refused(
        &fx.cpd(&["session", "retier", "--all", "--tier", "S2"]),
        "usage",
        2,
    );
    assert_eq!(state(&fx), before);
}

#[test]
fn retier_tier_on_the_tier_it_has_is_already() {
    let fx = Fx::new();
    fixture(&fx.env, "", FOUR);
    fx.cpd(&["session", "retier", LEAD1, "--tier", "S4"]).ok();
    let out = fx.cpd(&["session", "retier", LEAD1, "--tier", "S4"]).ok();
    assert_eq!(out["already"], true, "{out}");
    assert_eq!(out["retiered"], json!([]));
    assert_eq!(out["unchanged"], json!([LEAD1]));
    assert_eq!(events(&fx, "session-retiered").len(), 1, "no second Event");
}

#[test]
fn retier_tier_is_not_sticky() {
    let fx = Fx::new();
    fixture(&fx.env, "", FOUR);
    fx.cpd(&["session", "retier", LEAD1, "--tier", "S4"]).ok();
    assert_eq!(tier(&fx, LEAD1), "S4");
    fx.cpd(&["session", "retier", LEAD1]).ok();
    assert_eq!(tier(&fx, LEAD1), "S4", "lead_tier");

    drop(fx);
    let review = Fx::new();
    fixture(&review.env, REVIEW, FOUR);
    review
        .cpd(&["session", "retier", LEAD1, "--tier", "S4"])
        .ok();
    review.cpd(&["session", "retier", LEAD1]).ok();
    assert_eq!(tier(&review, LEAD1), "S4", "lead_tier");
}

#[test]
fn retier_tier_refuses_a_worker() {
    let fx = Fx::new();
    fixture(&fx.env, "", FOUR);
    let before = state(&fx);
    refused(
        &fx.cpd(&["session", "retier", WORKER, "--tier", "S2"]),
        "not_a_lead",
        3,
    );
    assert_eq!(state(&fx), before);
}

/// Roles GX / H3 / S4 / O3 on S [0-4], O [0,2,3,4], H [0-3], G []: the lead range is S4..O2 and the manager range O3..O4.
/// A Lead is refused a Tier outside the first, the Manager a Tier outside the second, with the details of the range.
/// Then `lead_tier = S2` moves the lead floor.
#[test]
fn retier_tier_is_checked_against_the_lead_and_manager_ranges() {
    let fx = Fx::new();
    fixture(&fx.env, "", FOUR);
    for tier in ["S3", "H3", "GX", "O3", "O4"] {
        not_allowed(&fx, LEAD1, tier, "lead", "S4", "O2", "lead_tier");
    }
    for tier in ["O2", "S4", "H3", "GX", "O0"] {
        not_allowed(&fx, MANAGER, tier, "manager", "O3", "O4", "manager_tier");
    }
    // `tier_invalid` first: O1 is no effort of O.
    refused(
        &fx.cpd(&["session", "retier", LEAD1, "--tier", "O1"]),
        "tier_invalid",
        2,
    );
    // Inside the ranges: the lead floor and ceiling, the manager floor.
    fx.cpd(&["session", "retier", LEAD1, "--tier", "O2"]).ok();
    assert_eq!(tier(&fx, LEAD1), "O2");
    fx.cpd(&["session", "retier", LEAD1, "--tier", "S4"]).ok();
    assert_eq!(tier(&fx, LEAD1), "S4");
    fx.cpd(&["session", "retier", MANAGER, "--tier", "O3"]).ok();
    assert_eq!(tier(&fx, MANAGER), "O3");
    // A second config moves the lead floor to S2; the manager range is the same.
    config(&fx.env, "S2", "");
    not_allowed(&fx, LEAD1, "S1", "lead", "S2", "O2", "lead_tier");
    fx.cpd(&["session", "retier", LEAD1, "--tier", "S3"]).ok();
    assert_eq!(tier(&fx, LEAD1), "S3");
}
