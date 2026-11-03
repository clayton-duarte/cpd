//! `cpd session retier` (commands.md §10.15, contract §2.2 Session `tier`, §5.2 `session-retiered`, §6.5 "A Session's
//! Tier while it runs"; hs-d506), and a relaunch (commands.md §10.8) on the row's current Tier.
//!
//! Every test here fails before the command exists, for the reason its comment gives.
//!
//! Every test runs the command tree in this process (`Fx`). The two relaunch tests run `cpd session open` on the fake
//! tmux, and the `session run` of the pane it made on a thread of the test, on virtual time (`common::wrapper`): the pi
//! it starts is a pid of the `FakeSpawn`, whose `Launch` is what they read.

use crate::common::fx::Fx;
use crate::common::session::{events, home, setup, show};
use crate::common::wrapper::{Pane, installed_cpd};
use crate::common::{TestEnv, refused};

use std::fs;

use serde_json::{Value, json};

const LEAD1: &str = "lead1";
const LEAD2: &str = "lead2";
const MANAGER: &str = "manager1";
const WORKER: &str = "worker1";
const FORK: &str = "fork1";
const ON_TARGET: &str = "lead4";
const ARCHIVED_LEAD: &str = "lead5";
const ARCHIVED_MANAGER: &str = "manager0";

/// `session.rs`'s fixture (Leads `lead1` on `pay` and `lead2` on `other`, Manager `manager1`, Worker `worker1`, every
/// Lead on `O3`, the Manager on `O2`), with the config the User set on 2026-10-20 (`lead_tier = "O2"`, `manager_tier =
/// "O4"`, hs-d506) and more Sessions: a side fork `fork1` (a Lead on `O3`), a Lead already on its target `lead4`
/// (`O2`), and an archived Lead `lead5` and an archived Manager `manager0`, both stale.
fn fixture(env: &TestEnv) {
    setup(env);
    env.write_models();
    env.write_tmux_config(
        "lead_tier = \"O2\"\nmanager_tier = \"O4\"\n[timeouts]\nfork_quiet_seconds = 1\n",
    );
    let cwd = env.root().display().to_string();
    env.sql(&format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, forked_from, cwd, tier, state, state_at, created_at, archived_at) VALUES
           ('fork1', 'lead', 'ctx', 'pay', NULL, 'Side', 'lead1', '{cwd}', 'O3', 'stopped', '2020-01-01T00:00:00Z', 't5', NULL),
           ('lead4', 'lead', 'ctx', 'pay', NULL, 'On target', 'lead1', '{cwd}', 'O2', 'stopped', '2020-01-01T00:00:00Z', 't6', NULL),
           ('lead5', 'lead', 'ctx', 'pay', NULL, 'Archived lead', 'lead1', '{cwd}', 'O3', 'stopped', '2020-01-01T00:00:00Z', 't7', 't9'),
           ('manager0', 'manager', 'ctx', NULL, NULL, 'Old manager', NULL, '{cwd}', 'O2', 'stopped', '2020-01-01T00:00:00Z', 't8', 't9');"
    ));
}

fn tier(fx: &Fx, id: &str) -> String {
    show(fx, id)["tier"].as_str().unwrap().to_owned()
}

/// Every Session's `tier`, by id.
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

/// A Lead goes to `lead_tier`: its row, one Event addressed to it with `{from, to}`, and `session show` names the
/// model and thinking level of the new Tier (the machine `[model]` O, `O2` = `medium`).
/// Fails now: `usage` (no such subcommand), exit 2, and no Event.
#[test]
fn retier_one_moves_a_lead_to_lead_tier_with_one_event() {
    let fx = Fx::new();
    fixture(&fx.env);
    assert_eq!(tier(&fx, LEAD1), "O3");

    let out = fx.cpd(&["session", "retier", LEAD1]).ok();
    assert_eq!(out["retiered"].as_array().unwrap().len(), 1, "{out}");
    let one = &out["retiered"][0];
    assert_eq!(
        (
            one["session"].as_str(),
            one["role"].as_str(),
            one["from"].as_str(),
            one["to"].as_str()
        ),
        (Some(LEAD1), Some("lead"), Some("O3"), Some("O2"))
    );
    assert_eq!(out["unchanged"], json!([]));
    assert_eq!(out.get("already"), None);

    assert_eq!(tier(&fx, LEAD1), "O2");
    let shown = show(&fx, LEAD1);
    assert_eq!(
        (shown["model"].as_str(), shown["thinking"].as_str()),
        (Some("p/opus"), Some("medium")),
        "{shown}"
    );
    let sent = events(&fx, "session-retiered");
    assert_eq!(sent.len(), 1, "one Event: {sent:?}");
    let event = &sent[0];
    assert_eq!(event["session"], LEAD1);
    assert_eq!(event["context"], "ctx");
    assert_eq!(event["plan"], "pay");
    assert_eq!(event["data"], json!({"from": "O3", "to": "O2"}));
    assert_eq!(one["event"], event["id"]);

    // Only that Session.
    assert_eq!(tier(&fx, LEAD2), "O3");
    assert_eq!(tier(&fx, MANAGER), "O2");
}

/// A Manager goes to `manager_tier` (`O4` = `xhigh`), a Context's Event with no Plan.
/// Fails now: `usage`, exit 2.
#[test]
fn retier_one_moves_a_manager_to_manager_tier() {
    let fx = Fx::new();
    fixture(&fx.env);

    let out = fx.cpd(&["session", "retier", MANAGER]).ok();
    assert_eq!(
        (
            out["retiered"][0]["role"].as_str(),
            out["retiered"][0]["from"].as_str(),
            out["retiered"][0]["to"].as_str()
        ),
        (Some("manager"), Some("O2"), Some("O4"))
    );
    assert_eq!(tier(&fx, MANAGER), "O4");
    let shown = show(&fx, MANAGER);
    assert_eq!(
        (shown["model"].as_str(), shown["thinking"].as_str()),
        (Some("p/opus"), Some("xhigh"))
    );
    let sent = events(&fx, "session-retiered");
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (
            sent[0]["session"].as_str(),
            sent[0]["plan"].clone(),
            sent[0]["data"].clone()
        ),
        (
            Some(MANAGER),
            Value::Null,
            json!({"from": "O2", "to": "O4"})
        )
    );
}

/// `--all`: every live Lead (a Plan's, a side fork) to `lead_tier`, the live Manager to `manager_tier`; a Worker
/// (`S2`, its Job's Tier), an archived Lead and an archived Manager are not touched, and a Session already on its
/// Tier is `unchanged`; one Event per changed Session and none for the others.
/// Fails now: `usage`, exit 2.
#[test]
fn retier_all_moves_live_leads_and_managers_and_nothing_else() {
    let fx = Fx::new();
    fixture(&fx.env);
    let before = tiers(&fx);

    let out = fx.cpd(&["session", "retier", "--all"]).ok();
    let mut moved: Vec<(String, String, String)> = out["retiered"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["session"].as_str().unwrap().to_owned(),
                r["from"].as_str().unwrap().to_owned(),
                r["to"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    moved.sort();
    let want = |id: &str, from: &str, to: &str| (id.to_owned(), from.to_owned(), to.to_owned());
    assert_eq!(
        moved,
        [
            want(FORK, "O3", "O2"),
            want(LEAD1, "O3", "O2"),
            want(LEAD2, "O3", "O2"),
            want(MANAGER, "O2", "O4")
        ]
    );
    assert_eq!(
        out["unchanged"],
        json!([ON_TARGET]),
        "the live Session already on its Tier; Workers and archived Sessions are not listed"
    );

    let after = tiers(&fx);
    for (id, was) in &before {
        let want = match id.as_str() {
            LEAD1 | LEAD2 | FORK => "O2",
            MANAGER => "O4",
            _ => was.as_str(), // the Worker, the archived Lead and Manager, the Lead on its Tier
        };
        assert_eq!(after.iter().find(|(i, _)| i == id).unwrap().1, want, "{id}");
    }
    assert_eq!(tier(&fx, WORKER), "S2");
    assert_eq!(tier(&fx, ARCHIVED_LEAD), "O3");
    assert_eq!(tier(&fx, ARCHIVED_MANAGER), "O2");

    // One Event per changed Session, none for the others.
    let mut sessions: Vec<String> = events(&fx, "session-retiered")
        .iter()
        .map(|e| e["session"].as_str().unwrap().to_owned())
        .collect();
    sessions.sort();
    assert_eq!(sessions, [FORK, LEAD1, LEAD2, MANAGER]);
}

/// Idempotent: the second run changes nothing, writes no Event (the cursor does not move) and says `already`; a named
/// Session already on its Tier is the same.
/// Fails now: `usage`, exit 2.
#[test]
fn retier_is_idempotent_and_a_session_on_its_tier_gets_no_event() {
    let fx = Fx::new();
    fixture(&fx.env);
    fx.cpd(&["session", "retier", "--all"]).ok();
    let events_before = events(&fx, "session-retiered").len();
    let cursor = fx.cpd(&["session", "list"]).ok()["events_cursor"].clone();
    let tiers_before = tiers(&fx);

    let again = fx.cpd(&["session", "retier", "--all"]).ok();
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(again["retiered"], json!([]));
    assert_eq!(events(&fx, "session-retiered").len(), events_before);
    assert_eq!(
        fx.cpd(&["session", "list"]).ok()["events_cursor"],
        cursor,
        "nothing was written"
    );
    assert_eq!(tiers(&fx), tiers_before);

    let one = fx.cpd(&["session", "retier", ON_TARGET]).ok();
    assert_eq!(
        (
            one["already"].clone(),
            one["retiered"].clone(),
            one["unchanged"].clone()
        ),
        (json!(true), json!([]), json!([ON_TARGET]))
    );
    assert_eq!(events(&fx, "session-retiered").len(), events_before);
}

/// The refusals write nothing: a Worker (`not_a_lead`, its Tier is its Job's), an archived Session (`archived`), an
/// unknown one, and an argument that is neither one Session nor `--all`, or is both (`usage`).
/// Fails now: every call is `usage`, exit 2: the `not_a_lead`, `archived` and `session_not_found` ones differ.
#[test]
fn retier_refuses_what_it_may_not_touch_and_writes_nothing() {
    let fx = Fx::new();
    fixture(&fx.env);
    let tiers_before = tiers(&fx);
    let cursor = fx.cpd(&["session", "list"]).ok()["events_cursor"].clone();

    refused(&fx.cpd(&["session", "retier", WORKER]), "not_a_lead", 3);
    let archived = refused(
        &fx.cpd(&["session", "retier", ARCHIVED_LEAD]),
        "archived",
        3,
    );
    assert_eq!(
        archived["details"]["next"],
        format!("cpd session unarchive {ARCHIVED_LEAD}")
    );
    refused(
        &fx.cpd(&["session", "retier", "no-such-session"]),
        "session_not_found",
        4,
    );
    refused(&fx.cpd(&["session", "retier"]), "usage", 2);
    refused(&fx.cpd(&["session", "retier", "--all", LEAD1]), "usage", 2);

    assert_eq!(tiers(&fx), tiers_before);
    assert_eq!(fx.cpd(&["session", "list"]).ok()["events_cursor"], cursor);
    assert!(events(&fx, "session-retiered").is_empty());
}

/// A relaunch **resumes** on the row's current Tier (hs-d506): `--model <O of [model]>:<level>` before `--session <file>`,
/// so pi runs the conversation on the new model instead of the one its file recorded. The row is set by the fixture
/// (no command): the assertion is about the launch alone.
/// Fails now: the resume argv is `--session <file>` with no `--model`.
#[test]
fn a_resumed_lead_runs_on_the_rows_current_tier() {
    let fx = Fx::with_ports();
    fixture(&fx.env);
    fx.cockpit();
    let sessions = fx.env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    let file = sessions.join("2026_lead1.jsonl");
    fs::write(&file, "{\"type\":\"session\"}\n").unwrap();
    fx.env
        .sql("UPDATE sessions SET tier = 'O2' WHERE id = 'lead1'");

    let opened = fx.cpd(&["session", "open", LEAD1]).ok();
    let pane = Pane::at(&fx, opened["pane"].as_str().unwrap(), &installed_cpd(&fx));
    let mut wrapper = pane.run(&["session", "run", LEAD1]);
    wrapper.until("the resume", || !pane.spawn.pis().is_empty());
    let argv = &pane.spawn.pis()[0].args;
    assert!(argv.iter().any(|a| a == "--session"), "a resume: {argv:?}");
    let at = |flag: &str| {
        argv.iter()
            .position(|a| a == flag)
            .unwrap_or_else(|| panic!("no {flag} in {argv:?}"))
    };
    assert_eq!(argv[at("--model") + 1], "p/opus:medium", "{argv:?}");
    assert_eq!(argv[at("--session") + 1], file.display().to_string());
    assert!(
        !argv
            .iter()
            .any(|a| a == "--fork" || a == "--session-id" || a == "--"),
        "a resume has no first message: {argv:?}"
    );
}

/// A relaunch of a Session **retiered** while it had no pi (and no file yet) starts fresh on the new Tier, not on the
/// Tier it was registered with.
/// Fails now: `usage` (no `retier`), exit 2.
#[test]
fn a_fresh_lead_started_after_a_retier_runs_on_the_new_tier() {
    let fx = Fx::with_ports();
    fixture(&fx.env);
    fx.cockpit();
    fx.cpd(&["session", "retier", LEAD1]).ok();

    let opened = fx.cpd(&["session", "open", LEAD1]).ok();
    let pane = Pane::at(&fx, opened["pane"].as_str().unwrap(), &installed_cpd(&fx));
    let mut wrapper = pane.run(&["session", "run", LEAD1]);
    wrapper.until("lead1's pi", || !pane.spawn.pis().is_empty());
    let argv = &pane.spawn.pis()[0].args;
    assert!(
        argv.iter().any(|a| a == "--session-id"),
        "a fresh start: {argv:?}"
    );
    let at = argv
        .iter()
        .position(|a| a == "--model")
        .unwrap_or_else(|| panic!("no --model in {argv:?}"));
    assert_eq!(argv[at + 1], "p/opus:medium", "{argv:?}");
    let message = home(&fx.env).lead_message(&cpd_core::ids::PlanId::parse("pay").unwrap());
    assert!(
        argv.ends_with(&["--".to_owned(), format!("@{}", message.display())]),
        "{argv:?}"
    );
}

/// `fixture` plus the Context `codereview`, holding `lead1` and `fork1`; `lead2` moves to another Context, so the
/// Manager stays in `ctx`.
fn review_fixture(env: &TestEnv) {
    fixture(env);
    env.write_tmux_config(
        "lead_tier = \"O2\"\nmanager_tier = \"O4\"\n[timeouts]\nfork_quiet_seconds = 1\n",
    );
    env.sql("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('elsewhere', 'Else', 't', 'm1')");
    env.sql("UPDATE sessions SET context = 'elsewhere' WHERE id = 'lead2'");
    env.sql("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('codereview', 'codereview', 't', 'm1')");
    env.sql("UPDATE sessions SET context = 'codereview' WHERE id IN ('lead1', 'fork1')");
}

/// `--all`: every Lead goes to `lead_tier` whatever its Context (`codereview` included), the Manager to `manager_tier`.
#[test]
fn retier_all_moves_every_lead_to_lead_tier_whatever_its_context() {
    let fx = Fx::new();
    review_fixture(&fx.env);

    fx.cpd(&["session", "retier", "--all"]).ok();
    for id in [LEAD1, FORK, LEAD2] {
        assert_eq!(tier(&fx, id), "O2", "{id}");
    }
    assert_eq!(tier(&fx, MANAGER), "O4");
}

/// A Lead on an override (`S2`, as `plan new --tier S2` leaves it) is reset to `lead_tier`; the Manager
/// goes to `manager_tier`.
#[test]
fn retier_resets_a_plan_new_tier_override_and_leaves_the_manager_alone() {
    let fx = Fx::new();
    review_fixture(&fx.env);
    fx.env
        .sql("UPDATE sessions SET tier = 'S2' WHERE id = 'lead1'");

    let out = fx.cpd(&["session", "retier", "--all"]).ok();
    let moved: Vec<(&str, &str, &str)> = out["retiered"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["session"] == LEAD1 || r["session"] == MANAGER)
        .map(|r| {
            (
                r["session"].as_str().unwrap(),
                r["from"].as_str().unwrap(),
                r["to"].as_str().unwrap(),
            )
        })
        .collect();
    assert!(moved.contains(&(LEAD1, "S2", "O2")), "{out}");
    assert!(moved.contains(&(MANAGER, "O2", "O4")), "{out}");
    assert_eq!(tier(&fx, LEAD1), "O2");
    assert_eq!(tier(&fx, MANAGER), "O4");
}
