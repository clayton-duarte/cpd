//! `cpd tui action incident` (commands.md §9.13, hs-d678): a click on the incident pill opens the
//! incident's stored Lead in the chat slot and sends it no message; with incident mode off or no
//! resolvable Lead it reports on the clicking client. The built `fake-tmux` is the only tmux,
//! wrapped so its calls are recorded; no real tmux, git, pi or gh runs.

use crate::common;

use common::{Run, SLEEP_SECONDS, TestEnv};
use cpd_core::home::CpdHome;
use cpd_core::host::layout::MAIN_WINDOW;
use cpd_core::store::Store;
use serde_json::Value;

/// The Context and the Plan of the fixture (contract §2.2).
const CONTEXT: &str = "ctx";
const PLAN: &str = "pay";
/// The incident's Lead, the Plan's `lead_session` (contract §2.2).
const LEAD: &str = "lead1";
/// The tmux client the StatusBar's click names.
const CLIENT: &str = "c1";
/// The reports of a click with no incident and with no resolvable Lead (commands.md §9.13).
const NO_INCIDENT: &str = "no incident: nothing to open";
const NO_LEAD: &str = "incidents require a lead";

fn home(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

/// What the fixture's store holds for the incident.
#[derive(Clone, Copy)]
enum Incident {
    /// An `incident` row whose `lead` is `lead1` and one `incident_plans` row for `pay`.
    On,
    /// No `incident` row.
    Off,
    /// An `incident` row whose `lead` is null and no `incident_plans` row.
    NoLead,
}

/// The env: config on its private fake tmux, a recording `tmux` that runs the built `fake-tmux`, a
/// `cpd` shim, a cockpit with the chat placeholder, and the seeded Context, Plan and incident.
fn fixture(incident: Incident) -> TestEnv {
    let env = TestEnv::new();
    env.write_tmux_config("");
    recording_fake_tmux(&env);
    env.fake_cpd_views();
    seed(&env, incident);
    cockpit(&env);
    env
}

/// A `tmux` on `PATH` that records its argv (env.calls) and then runs the built fake tmux, so no real
/// tmux server ever starts.
fn recording_fake_tmux(env: &TestEnv) {
    env.fake("tmux", &format!("exec '{}' \"$@\"", env!("CARGO_BIN_EXE_fake-tmux")));
}

/// Context `ctx`, the Plan `pay` whose Lead is `lead1`, and — when `incident` — the `incident` and
/// `incident_plans` rows a `cpd incident start --plan pay` writes (contract §2.2, §4.4).
fn seed(env: &TestEnv, incident: Incident) {
    let cwd = env.root().display().to_string();
    let mut store = Store::open(&home(env), 5000).unwrap();
    let mut sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('{CONTEXT}', 'Ctx', '2026-01-01T00:00:00Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('{LEAD}', 'lead', '{CONTEXT}', '{PLAN}', 'Lead', '{cwd}', 'O3', 'working', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('{PLAN}', '{CONTEXT}', 'Pay', 'dispatch', '{LEAD}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');"
    );
    match incident {
        Incident::Off => {}
        Incident::On => sql.push_str(&format!(
            "INSERT INTO incident (id, started_at, started_by, reason, started_txn, lead)
               VALUES (1, '2026-01-01T00:00:00Z', '{LEAD}', 'database migration in progress', cpd_txn(), '{LEAD}');
             INSERT INTO incident_plans (plan, started_txn) VALUES ('{PLAN}', cpd_txn());"
        )),
        Incident::NoLead => sql.push_str(
            "INSERT INTO incident (id, started_at, started_by, reason, started_txn)
               VALUES (1, '2026-01-01T00:00:00Z', 'lead1', 'database migration in progress', cpd_txn());",
        ),
    }
    common::sql(&mut store, &sql);
}

/// The cockpit: session `cpd`, window `main`, one tree pane and the chat placeholder (§13.1).
fn cockpit(env: &TestEnv) {
    let ok = |args: &[&str]| {
        let out = env.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let tree = ok(&["new-session", "-d", "-s", "cpd", "-n", MAIN_WINDOW, "-x", "215", "-y", "62", "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    ok(&["set", "-p", "-t", &tree, "@cpd_view", "tree"]);
    let chat = ok(&["split-window", "-d", "-t", &tree, "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    ok(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
}

/// `cpd tui action incident --client <client>`.
fn action(env: &TestEnv) -> Run {
    env.cpd(&["tui", "action", "incident", "--client", CLIENT])
}

/// The `session-message` Events, in order.
fn messages(env: &TestEnv) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", "session-message"]).ok()["events"].as_array().unwrap().clone()
}

/// The `@cpd_session` of the chat slot of `cpd:main` (empty while the placeholder holds it).
fn chat_slot_session(env: &TestEnv) -> String {
    let out = env.tmux(&["list-panes", "-t", "=cpd:main", "-F", "#{@cpd_session}\t#{@cpd_view}"]);
    let text = String::from_utf8(out.stdout).unwrap();
    for line in text.lines() {
        let (session, view) = line.split_once('\t').expect("session and view");
        if !session.is_empty() || view == "chat" {
            return session.to_owned();
        }
    }
    panic!("no chat slot in {text:?}");
}

/// The recorded `display-message` calls.
fn display_messages(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == "display-message")).collect()
}

#[test]
fn a_click_opens_the_incident_lead() {
    let env = fixture(Incident::On);

    let run = action(&env);
    assert_eq!((run.exit, run.stdout.as_str(), run.stderr.as_str()), (0, "", ""), "{run:?}");

    assert!(messages(&env).is_empty(), "the Lead is opened, not woken: {:?}", messages(&env));
    assert_eq!(chat_slot_session(&env), LEAD, "the incident's Lead is in the chat slot");
    env.tmux(&["kill-session", "-t", "cpd"]);
}

#[test]
fn a_click_with_no_incident_reports_on_the_client() {
    let env = fixture(Incident::Off);

    let run = action(&env);
    assert_eq!((run.exit, run.stdout.as_str(), run.stderr.as_str()), (0, "", ""), "{run:?}");

    assert_eq!(chat_slot_session(&env), "", "nothing is opened");
    let displays = display_messages(&env);
    let call = displays.iter().find(|c| c.iter().any(|a| a.contains(NO_INCIDENT))).unwrap_or_else(|| panic!("no {NO_INCIDENT:?}: {displays:?}"));
    assert!(call.windows(2).any(|w| w == ["-c", CLIENT]), "{call:?}");
    env.tmux(&["kill-session", "-t", "cpd"]);
}

#[test]
fn a_click_with_no_lead_reports_on_the_client() {
    let env = fixture(Incident::NoLead);

    let run = action(&env);
    assert_eq!((run.exit, run.stdout.as_str(), run.stderr.as_str()), (0, "", ""), "{run:?}");

    assert_eq!(chat_slot_session(&env), "", "nothing is opened");
    let displays = display_messages(&env);
    let call = displays.iter().find(|c| c.iter().any(|a| a.contains(NO_LEAD))).unwrap_or_else(|| panic!("no {NO_LEAD:?}: {displays:?}"));
    assert!(call.windows(2).any(|w| w == ["-c", CLIENT]), "{call:?}");
    env.tmux(&["kill-session", "-t", "cpd"]);
}
