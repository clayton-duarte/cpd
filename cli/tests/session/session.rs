//! Sessions (contract §3.6, §6.4, §6.7; commands.md §10 with §19 amendments
//! 1–3), every test in this process (`Fx`, on `Fx::with_ports`): the cockpit is `Fx::cockpit` on the fake tmux, a pane's
//! program is what the fake server recorded for it, a pane whose program ends is `Fx::pane_exit`, the engine's tick is
//! `Fx::engine_tick`, a refusing `tmux` is a rule of the `FakeRunner`. The wrapper's tests (a `session run` in a pane,
//! `fork` waiting for its source, the user's quit) run the wrapper on a thread of the test, on virtual time
//! (`common::wrapper`): its pi is a pid of the `FakeSpawn` and its Enter is typed by the test.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::process::Output;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use common::fx::{Fx, PaneRow};
use common::session::{Cpd, events, home, setup, show};
use common::wrapper::{Pane, Wrapper, installed_cpd};
use common::{Run, TestEnv, refused};
use cpd_core::host::runner::Reply;
use cpd_core::host::spawn::{Launch, Pid};
use cpd_core::ids::{PlanId, SessionId};
use serde_json::{Value, json};

const LEAD1: &str = "lead1";
const LEAD2: &str = "lead2";
const MANAGER: &str = "manager1";
const WORKER: &str = "worker1";

fn sid(id: &str) -> SessionId {
    SessionId::parse(id).unwrap()
}

// ---------- tmux ----------

/// A tmux server a test reads: the fake of an [`Fx`] on fake ports.
trait Server {
    fn tmux(&self, args: &[&str]) -> Output;
}

impl Server for Fx {
    fn tmux(&self, args: &[&str]) -> Output {
        Fx::tmux(self, args)
    }
}

fn panes(env: &impl Server) -> Vec<PaneRow> {
    let out = env.tmux(&[
        "list-panes",
        "-a",
        "-F",
        "#{pane_id}\t#{window_id}\t#{window_name}\t#{@cpd_session}\t#{@cpd_view}\t#{@cpd_history}",
    ]);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            PaneRow {
                pane: f[0].into(),
                window: f[1].into(),
                window_name: f[2].into(),
                session: f[3].into(),
                view: f[4].into(),
                history: f[5].into(),
            }
        })
        .collect()
}

/// The program the server recorded for `pane`: the argv it was started or respawned with (the fake runs no pane program).
fn pane_argv(fx: &Fx, pane: &str) -> Vec<String> {
    let state = fx.tmux_state().expect("a server");
    state
        .panes()
        .into_iter()
        .find(|p| p.id == pane)
        .unwrap_or_else(|| panic!("no pane {pane}"))
        .argv
}

/// The chat slot: the pane of `main` with a Session, the placeholder or a history pane.
fn slot(env: &impl Server) -> PaneRow {
    let found: Vec<PaneRow> = panes(env)
        .into_iter()
        .filter(|p| {
            p.window_name == "main"
                && (!p.session.is_empty() || p.view == "chat" || p.view == "history")
        })
        .collect();
    assert_eq!(found.len(), 1, "one chat slot: {found:?}");
    found[0].clone()
}

/// At most one placeholder exists, and only in the slot (amendment 2).
fn assert_placeholders(env: &impl Server, expected: usize) {
    let placeholders: Vec<PaneRow> = panes(env)
        .into_iter()
        .filter(|p| p.view == "chat")
        .collect();
    assert_eq!(placeholders.len(), expected, "{placeholders:?}");
    for p in placeholders {
        assert_eq!(
            p.window_name, "main",
            "a placeholder outside the slot: {p:?}"
        );
    }
}

fn window_names(env: &impl Server) -> Vec<String> {
    let out = env.tmux(&["list-windows", "-t", "=cpd", "-F", "#{window_name}"]);
    let mut names: Vec<String> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

/// Every pane of the server, dead ones with their screen (where a dead `cpd
/// session run` printed its error): the evidence of what the chat slot held.
fn panes_report(env: &impl Server) -> String {
    let format = "#{pane_id} #{session_name}:#{window_name} dead=#{pane_dead} status=#{pane_dead_status} view=#{@cpd_view} session=#{@cpd_session}";
    let listed =
        String::from_utf8_lossy(&env.tmux(&["list-panes", "-a", "-F", format]).stdout).into_owned();
    let mut report = String::new();
    for line in listed.lines() {
        report.push_str(&format!("{line}\n"));
        if line.contains(" dead=1 ") {
            let pane = line.split(' ').next().unwrap();
            let screen = String::from_utf8_lossy(
                &env.tmux(&["capture-pane", "-p", "-J", "-t", pane]).stdout,
            )
            .into_owned();
            for text in screen.lines().filter(|text| !text.trim().is_empty()) {
                report.push_str(&format!("  | {text}\n"));
            }
        }
    }
    report
}

/// `cpd <args>`'s success fields, where no pane may die: it panics with
/// [`panes_report`] when a pane is dead before the command, or the command fails.
fn ok_with_panes<E: Server + Cpd>(env: &E, args: &[&str]) -> Value {
    let before = panes_report(env);
    assert!(
        !before.contains(" dead=1 "),
        "a pane died before cpd {args:?}:\n{before}"
    );
    let run = env.cpd(args);
    if run.exit != 0 {
        panic!(
            "cpd {args:?} failed: {run:?}\npanes:\n{}",
            panes_report(env)
        );
    }
    run.ok()
}

// ---------- list, show ----------

#[test]
fn list_filters_and_derives_place_and_leads() {
    let fx = Fx::new();
    setup(&fx.env);
    let ids = |run: Run| -> Vec<String> {
        run.ok()["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap().to_owned())
            .collect()
    };

    let all = fx.cpd(&["session", "list"]).ok();
    assert_eq!(all["events_cursor"], 1);
    let sessions = all["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 4);
    let lead1 = sessions.iter().find(|s| s["id"] == LEAD1).unwrap();
    assert_eq!(
        *lead1,
        json!({"id": "lead1", "role": "lead", "context": "ctx", "plan": "pay", "attempt": null, "name": "Lead one",
               "state": "idle", "state_label": null, "state_at": "2020-01-01T00:00:01Z", "archived_at": null, "asleep_at": null, "place": "none", "tmux_pane": null, "file": null,
               "forked_from": null, "plan_lead": true, "tier": "O3",
               "cwd": fx.env.root().display().to_string(), "title": null, "titled_at": null, "by_user": false, "runner": "pi"})
    );
    let manager = sessions.iter().find(|s| s["id"] == MANAGER).unwrap();
    assert_eq!(
        (
            &manager["role"],
            &manager["plan"],
            &manager["attempt"],
            &manager["plan_lead"],
            &manager["tier"]
        ),
        (
            &json!("manager"),
            &Value::Null,
            &Value::Null,
            &json!(false),
            &json!("O2")
        )
    );
    assert!(
        lead1.get("grounded_lead").is_none(),
        "the grounded Lead is retired"
    );

    assert_eq!(
        ids(fx.cpd(&["session", "list", "--plan", "pay"])),
        [LEAD1, WORKER]
    );
    assert_eq!(
        ids(fx.cpd(&["session", "list", "--context", "ctx", "--role", "worker"])),
        [WORKER]
    );
    assert_eq!(
        ids(fx.cpd(&["session", "list", "--state", "idle", "--state", "working"])),
        [LEAD1, WORKER]
    );
    assert_eq!(
        ids(fx.cpd(&["session", "list", "--role", "lead", "--state", "stopped"])),
        [LEAD2]
    );
    assert_eq!(
        ids(fx.cpd(&["session", "list", "--role", "manager"])),
        [MANAGER]
    );
    assert!(ids(fx.cpd(&["session", "list", "--context", "elsewhere"])).is_empty());
    // Filters only: CPD_PLAN does not select anything.
    assert_eq!(
        fx.cpd_with(fx.env.root(), &["session", "list"], &[("CPD_PLAN", "pay")])
            .ok()["sessions"]
            .as_array()
            .unwrap()
            .len(),
        4
    );

    refused(
        &fx.cpd(&["session", "list", "--role", "reviewer"]),
        "usage",
        2,
    );
    refused(&fx.cpd(&["session", "list", "--state", "busy"]), "usage", 2);
    refused(
        &fx.cpd(&["session", "list", "--plan", "Bad"]),
        "invalid_plan_id",
        2,
    );
}

/// sql-F5, commands-F4: the Plan-Lead lookup is a join: a Session that leads two
/// Plans is still one row, and a Session that leads none is not a Plan's Lead.
#[test]
fn list_gives_a_session_leading_two_plans_one_row() {
    let fx = Fx::new();
    setup(&fx.env);
    fx.env.sql("INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('again', 'ctx', 'Again', 'grounding', 'lead1', 't')",
    );
    let sessions = fx.cpd(&["session", "list"]).ok()["sessions"].clone();
    let sessions = sessions.as_array().unwrap();
    let leads: Vec<(&str, bool)> = sessions
        .iter()
        .map(|s| (s["id"].as_str().unwrap(), s["plan_lead"].as_bool().unwrap()))
        .collect();
    assert_eq!(
        leads,
        [
            (MANAGER, false),
            (LEAD1, true),
            (LEAD2, true),
            (WORKER, false)
        ]
    );
}

#[test]
fn show_adds_created_at_registered_event_and_open_escalations() {
    let fx = Fx::new();
    setup(&fx.env);
    let out = fx.cpd(&["session", "show", LEAD1]).ok();
    assert_eq!(out["session"]["created_at"], "t1");
    assert_eq!(out["session"]["registered_event"], 1);
    assert_eq!(out["session"]["plan_lead"], true);
    assert_eq!(out["open_escalations"], json!(["api-a1-q1"]));

    let worker = fx
        .cpd_with(
            fx.env.root(),
            &["session", "show"],
            &[("CPD_SESSION", WORKER)],
        )
        .ok();
    assert_eq!(worker["session"]["id"], WORKER);
    assert_eq!(worker["session"]["attempt"], "api-a1");
    assert_eq!(worker["session"]["registered_event"], Value::Null);
    assert!(
        worker.get("open_escalations").is_none(),
        "only a Plan's Lead has them"
    );

    refused(&fx.cpd(&["session", "show"]), "session_required", 2);
    let error = refused(
        &fx.cpd(&["session", "show", "nobody"]),
        "session_not_found",
        4,
    );
    assert_eq!(error["details"]["session"], "nobody");
    refused(
        &fx.cpd(&["session", "show", "bad id"]),
        "invalid_session_id",
        2,
    );
}

// ---------- state, rename ----------

#[test]
fn state_follows_section_3_6() {
    let fx = Fx::new();
    setup(&fx.env);
    let state = |args: &[&str]| {
        let mut argv = vec!["session", "state"];
        argv.extend(args);
        argv.extend(["--session", LEAD1]);
        fx.cpd(&argv)
    };

    let out = state(&["working"]).ok();
    assert_eq!(
        out,
        json!({"ok": true, "session": "lead1", "state": "working", "label": null, "from": "idle"})
    );
    let out = state(&["blocked", "--label", "cpd: Worker question"]).ok();
    assert_eq!(
        (&out["from"], &out["label"]),
        (&json!("working"), &json!("cpd: Worker question"))
    );
    assert_eq!(show(&fx, LEAD1)["state_label"], "cpd: Worker question");

    // Unchanged: nothing written, no Event.
    let before = events(&fx, "session-state").len();
    let txns: i64 = fx.env.query("SELECT count(*) FROM txns");
    assert_eq!(
        state(&["blocked", "--label", "cpd: Worker question"]).ok()["already"],
        true
    );
    assert_eq!(events(&fx, "session-state").len(), before);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns);

    // A new label is a change.
    assert_eq!(
        state(&["blocked", "--label", "cpd: plan review"]).ok()["from"],
        "blocked"
    );
    assert_eq!(state(&["idle"]).ok()["from"], "blocked");
    assert_eq!(state(&["stopped"]).ok()["from"], "idle");
    let error = refused(&state(&["working"]), "illegal_transition", 3);
    assert_eq!(
        (&error["details"]["from"], &error["details"]["to"]),
        (&json!("stopped"), &json!("working"))
    );
    refused(
        &state(&["blocked", "--label", "x"]),
        "illegal_transition",
        3,
    );
    assert_eq!(state(&["idle"]).ok()["from"], "stopped");

    let all = events(&fx, "session-state");
    assert_eq!(all.len(), 6);
    let first = &all[0];
    assert_eq!(
        (&first["session"], &first["plan"], &first["context"]),
        (&json!("lead1"), &json!("pay"), &json!("ctx"))
    );
    assert_eq!(
        first["data"],
        json!({"from": "idle", "to": "working", "label": null})
    );
    assert_eq!(
        all[1]["data"],
        json!({"from": "working", "to": "blocked", "label": "cpd: Worker question"})
    );
    assert_eq!(show(&fx, LEAD1)["state_label"], Value::Null);

    // A Manager has no Plan: its Event has none either.
    fx.cpd_with(
        fx.env.root(),
        &["session", "state", "idle"],
        &[("CPD_SESSION", MANAGER)],
    )
    .ok();
    assert_eq!(
        events(&fx, "session-state").last().unwrap()["plan"],
        Value::Null
    );

    refused(&state(&["blocked"]), "usage", 2);
    refused(&state(&["idle", "--label", "x"]), "usage", 2);
    refused(&state(&["blocked", "--label", &"x".repeat(81)]), "usage", 2);
    refused(&state(&["asleep"]), "usage", 2);
    refused(
        &fx.cpd(&["session", "state", "idle"]),
        "session_required",
        2,
    );
    refused(
        &fx.cpd(&["session", "state", "idle", "--session", "nobody"]),
        "session_not_found",
        4,
    );
}

#[test]
fn state_waiting_takes_a_label_and_follows_section_3_6() {
    let fx = Fx::new();
    setup(&fx.env);
    let state = |args: &[&str]| {
        let mut argv = vec!["session", "state"];
        argv.extend(args);
        argv.extend(["--session", LEAD1]);
        fx.cpd(&argv)
    };

    // `waiting` needs a label, as `blocked` does; a refusal writes nothing.
    let txns: i64 = fx.env.query("SELECT count(*) FROM txns");
    let error = refused(&state(&["waiting"]), "usage", 2);
    assert_eq!(error["details"]["state"], "waiting");
    refused(&state(&["waiting", "--label", &"x".repeat(81)]), "usage", 2);
    refused(&state(&["idle", "--label", "x"]), "usage", 2);
    refused(&state(&["waiting", "--label", "x", "--reply"]), "usage", 2);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns);

    // `idle` and `working` go to `waiting` and come back; a new label is a change; the same one writes nothing.
    let out = state(&["waiting", "--label", "cpd: Worker question"]).ok();
    assert_eq!(
        out,
        json!({"ok": true, "session": "lead1", "state": "waiting", "label": "cpd: Worker question", "from": "idle"})
    );
    assert_eq!(
        (
            show(&fx, LEAD1)["state"].clone(),
            show(&fx, LEAD1)["state_label"].clone()
        ),
        (json!("waiting"), json!("cpd: Worker question"))
    );
    let before = events(&fx, "session-state").len();
    assert_eq!(
        state(&["waiting", "--label", "cpd: Worker question"]).ok()["already"],
        true
    );
    assert_eq!(events(&fx, "session-state").len(), before);
    assert_eq!(
        state(&["waiting", "--label", "cpd: other"]).ok()["from"],
        "waiting"
    );
    assert_eq!(state(&["working"]).ok()["from"], "waiting");
    assert_eq!(show(&fx, LEAD1)["state_label"], Value::Null);
    assert_eq!(
        state(&["waiting", "--label", "cpd: Worker question"]).ok()["from"],
        "working"
    );
    assert_eq!(state(&["idle"]).ok()["from"], "waiting");

    // `waiting` goes to `blocked` (a human reason opens) and `blocked` back to `waiting` (it closes, the wait stays).
    state(&["waiting", "--label", "cpd: Worker question"]).ok();
    assert_eq!(
        state(&["blocked", "--label", "dialog"]).ok()["from"],
        "waiting"
    );
    assert_eq!(
        state(&["waiting", "--label", "cpd: Worker question"]).ok()["from"],
        "blocked"
    );
    assert_eq!(state(&["stopped"]).ok()["from"], "waiting");
    refused(
        &state(&["waiting", "--label", "x"]),
        "illegal_transition",
        3,
    );

    let last = events(&fx, "session-state")
        .into_iter()
        .rev()
        .nth(1)
        .unwrap();
    assert_eq!(
        last["data"],
        json!({"from": "blocked", "to": "waiting", "label": "cpd: Worker question"})
    );
}

#[test]
fn state_idle_reply_writes_data_reply_and_only_idle_accepts_it() {
    let fx = Fx::new();
    setup(&fx.env);
    let state = |args: &[&str]| {
        let mut argv = vec!["session", "state"];
        argv.extend(args);
        argv.extend(["--session", LEAD1]);
        fx.cpd(&argv)
    };

    // Only `idle` takes `--reply`; a refusal writes nothing.
    let txns: i64 = fx.env.query("SELECT count(*) FROM txns");
    for other in [
        &["working", "--reply"][..],
        &["blocked", "--label", "x", "--reply"],
        &["stopped", "--reply"],
    ] {
        refused(&state(other), "usage", 2);
    }
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns);

    state(&["working"]).ok();
    state(&["idle", "--reply"]).ok();
    let all = events(&fx, "session-state");
    assert_eq!(
        all.last().unwrap()["data"],
        json!({"from": "working", "to": "idle", "label": null, "reply": true})
    );

    // Without `--reply` the Event has no `reply`; an unchanged state writes nothing whatever `--reply`.
    state(&["working"]).ok();
    state(&["idle"]).ok();
    assert_eq!(
        events(&fx, "session-state").last().unwrap()["data"],
        json!({"from": "working", "to": "idle", "label": null})
    );
    let (events_before, txns) = (
        events(&fx, "session-state").len(),
        fx.env.query::<i64>("SELECT count(*) FROM txns"),
    );
    assert_eq!(state(&["idle", "--reply"]).ok()["already"], true);
    assert_eq!(events(&fx, "session-state").len(), events_before);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns);
}

#[test]
fn rename_appends_session_renamed_once() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    let out = fx
        .cpd(&["session", "rename", "cpd rust lead", "--session", LEAD1])
        .ok();
    assert_eq!(
        out,
        json!({"ok": true, "session": "lead1", "name": "cpd rust lead"})
    );
    assert_eq!(show(&fx, LEAD1)["name"], "cpd rust lead");
    let again = fx
        .cpd_with(
            fx.env.root(),
            &["session", "rename", "cpd rust lead"],
            &[("CPD_SESSION", LEAD1)],
        )
        .ok();
    assert_eq!(again["already"], true);
    let renamed = events(&fx, "session-renamed");
    assert_eq!(renamed.len(), 1);
    assert_eq!(
        (&renamed[0]["session"], &renamed[0]["data"]),
        (&json!("lead1"), &json!({"name": "cpd rust lead"}))
    );

    refused(
        &fx.cpd(&["session", "rename", "", "--session", LEAD1]),
        "invalid_name",
        2,
    );
    refused(
        &fx.cpd(&["session", "rename", "a\tb", "--session", LEAD1]),
        "invalid_name",
        2,
    );
    refused(&fx.cpd(&["session", "rename", "x"]), "session_required", 2);
    refused(
        &fx.cpd(&["session", "rename", "x", "--session", "nobody"]),
        "session_not_found",
        4,
    );
}

// ---------- fork ----------

#[test]
fn fork_refusals_register_nothing() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    let message = fx.env.root().join("message.md");
    fs::write(&message, "Hello.").unwrap();
    let message = message.to_str().unwrap();

    let error = refused(&fx.cpd(&["session", "fork", WORKER]), "not_a_lead", 3);
    assert_eq!(error["details"]["role"], "worker");
    let error = refused(&fx.cpd(&["session", "fork", MANAGER]), "not_a_lead", 3);
    assert_eq!(error["details"]["role"], "manager");
    refused(
        &fx.cpd(&["session", "fork", "nobody"]),
        "session_not_found",
        4,
    );
    refused(
        &fx.cpd(&["session", "fork", LEAD1, "--plan", "nope"]),
        "plan_not_found",
        4,
    );
    refused(
        &fx.cpd(&[
            "session",
            "fork",
            LEAD1,
            "--message-file",
            "/nonexistent/m.md",
        ]),
        "file_unreadable",
        2,
    );
    refused(
        &fx.cpd(&["session", "fork", LEAD1, "--name", ""]),
        "invalid_name",
        2,
    );
    // No tmux server, so no cockpit.
    let error = refused(
        &fx.cpd(&["session", "fork", LEAD1, "--message-file", message]),
        "no_cockpit",
        3,
    );
    assert_eq!(error["details"]["next"], "cpd tui");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM sessions"), 4);
}

#[test]
fn fork_registers_a_stopped_lead_in_a_parked_window() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    let message = fx.env.root().join("message.md");
    fs::write(&message, "Look at the tests.").unwrap();

    let out = fx
        .cpd(&[
            "session",
            "fork",
            LEAD2,
            "--plan",
            "pay",
            "--message-file",
            message.to_str().unwrap(),
        ])
        .ok();
    let new = out["session"]["id"].as_str().unwrap().to_owned();
    assert!(
        SessionId::parse(&new).is_ok() && new.len() == 36,
        "a UUID: {new}"
    );
    assert_eq!(out["session"]["name"], "Lead two (fork)");
    assert_eq!(
        (&out["session"]["plan"], &out["session"]["forked_from"]),
        (&json!("pay"), &json!(LEAD2))
    );

    let row = show(&fx, &new);
    assert_eq!(
        (
            &row["role"],
            &row["context"],
            &row["state"],
            &row["tier"],
            &row["forked_from"],
            &row["place"]
        ),
        (
            &json!("lead"),
            &json!("ctx"),
            &json!("stopped"),
            &json!("O2"),
            &json!(LEAD2),
            &json!("parked")
        )
    );
    assert_eq!(row["cwd"], fx.env.root().display().to_string());
    assert_eq!(row["tmux_pane"], out["session"]["pane"]);
    assert_eq!(
        row["plan_lead"], false,
        "a side session: the Plan's Lead is unchanged"
    );
    let copied = home(&fx.env).session_message(&PlanId::parse("pay").unwrap(), &sid(&new));
    assert_eq!(fs::read_to_string(copied).unwrap(), "Look at the tests.");

    let pane = panes(&fx)
        .into_iter()
        .find(|p| p.pane == out["session"]["pane"])
        .unwrap();
    assert_eq!(
        (
            pane.window.as_str(),
            pane.window_name.as_str(),
            pane.session.as_str()
        ),
        (
            out["session"]["window"].as_str().unwrap(),
            "park-pay",
            new.as_str()
        )
    );
    // The parked pane's program is `cpd session run <id>` (the fake server records it: it starts nothing).
    let argv = pane_argv(&fx, &pane.pane);
    assert_eq!(argv[argv.len() - 3..], ["session", "run", new.as_str()]);

    let registered = events(&fx, "session-registered");
    let registered = registered.last().unwrap();
    assert_eq!(
        (&registered["session"], &registered["plan"]),
        (&json!(new), &json!("pay"))
    );
    assert_eq!(
        registered["data"],
        json!({"role": "lead", "name": "Lead two (fork)", "forked_from": LEAD2, "tier": "O2"})
    );
    let moved = events(&fx, "session-moved");
    assert_eq!(moved.len(), 1);
    assert_eq!(
        moved[0]["data"],
        json!({"pane": out["session"]["pane"], "window": out["session"]["window"], "place": "parked"})
    );

    // The default Plan is the source's; each call registers a new Session.
    let again = fx.cpd(&["session", "fork", LEAD1]).ok();
    assert_eq!(
        (&again["session"]["plan"], &again["session"]["name"]),
        (&json!("pay"), &json!("Lead one (fork)"))
    );
    assert_ne!(again["session"]["id"], json!(new));
}

/// G11: a `tmux` that refuses `set -p … @cpd_session` and passes everything else to the fake server.
fn tmux_refusing_the_session_mark(fx: &Fx) {
    let _ = (*fx.fakes.runner).clone().on(
        "tmux",
        |call| {
            call.args.iter().any(|a| a == "set") && call.args.iter().any(|a| a == "@cpd_session")
        },
        Reply::exit(1, "", "refused\n"),
    );
}

/// One signal of [`GoSignals`]: `(pane, id, tmux_pane)`.
type GoSignal = (String, String, Option<String>);

/// What the store knew of each Session as its pane got the go signal: `set -p -t <pane> @cpd_session <id>` (the signal
/// the wrapper's `session run` waits for, then its pi reports). Recorded as the call is about to be answered, from the
/// store's own file on a second read-only connection: `(pane, id, tmux_pane of the Session's row)`.
struct GoSignals(Arc<Mutex<Vec<GoSignal>>>);

impl GoSignals {
    fn record(fx: &Fx) -> GoSignals {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let db = fx.env.cpd_home().join("cpd.db");
        // A matcher that never matches: the rule only looks, the fake server goes on to answer.
        let _ = (*fx.fakes.runner).clone().on(
            "tmux",
            move |call| {
                let a = &call.args;
                if let [.., t, pane, key, id] = a.as_slice()
                    && t == "-t"
                    && key == "@cpd_session"
                    && !id.is_empty()
                {
                    let conn = rusqlite::Connection::open_with_flags(
                        &db,
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                    )
                    .expect("open the store");
                    let row = conn
                        .query_row("SELECT tmux_pane FROM sessions WHERE id = ?1", [id], |r| {
                            r.get::<_, Option<String>>(0)
                        })
                        .expect("the Session's row");
                    log.lock().unwrap_or_else(PoisonError::into_inner).push((
                        pane.clone(),
                        id.clone(),
                        row,
                    ));
                }
                false
            },
            Reply::ok(""),
        );
        GoSignals(seen)
    }

    /// The pane `session` was given the go signal on, and the `tmux_pane` the store had for it at that moment.
    fn pane_and_store(&self, session: &str) -> (String, Value) {
        let seen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (pane, _, row) = seen
            .iter()
            .find(|(_, id, _)| id == session)
            .unwrap_or_else(|| panic!("{session} never got @cpd_session: {seen:?}"));
        (pane.clone(), json!(row))
    }

    /// The pane gets `@cpd_session` only once the store records the pane (contract §6.4): the signal is what lets the
    /// pane's `session run` go on and its pi report, and a report that lands before the store has the pane writes a
    /// row with no pane that the engine then stops (hs-d238).
    fn assert_the_store_had_the_pane(&self, session: &str) {
        let (pane, recorded) = self.pane_and_store(session);
        assert_eq!(
            recorded,
            json!(pane),
            "pane {pane} got @cpd_session {session} while the store's tmux_pane for it was {recorded}"
        );
    }
}

/// `session open` of a Lead with no pane, into the chat slot.
#[test]
fn open_records_the_pane_before_it_gets_the_go_signal() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    let signals = GoSignals::record(&fx);

    fx.cpd(&["session", "open", LEAD1]).ok();
    signals.assert_the_store_had_the_pane(LEAD1);
}

#[test]
fn fork_records_the_pane_before_it_gets_the_go_signal() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    let signals = GoSignals::record(&fx);

    let out = fx.cpd(&["session", "fork", LEAD1, "--plan", "pay"]).ok();
    signals.assert_the_store_had_the_pane(out["session"]["id"].as_str().unwrap());
}

/// G11: a step after `new-window` fails: the new window is killed and the original error returned.
#[test]
fn fork_and_open_leave_no_window_when_a_step_after_new_window_fails() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    tmux_refusing_the_session_mark(&fx);
    let before = window_names(&fx);
    let panes_before = panes(&fx);

    let run = fx.cpd(&["session", "fork", LEAD1, "--plan", "pay"]);
    assert_eq!(run.error()["code"], "tmux_failed", "{run:?}");
    assert_eq!(window_names(&fx), before, "fork left a window");

    let run = fx.cpd(&["session", "open", LEAD1]);
    assert_eq!(run.error()["code"], "tmux_failed", "{run:?}");
    assert_eq!(window_names(&fx), before, "open left a window");
    assert_eq!(panes(&fx), panes_before);
}

// ---------- the wrapper of a fork: `session run` waits for its source ----------

/// A finished turn (the last `message` is an assistant's `stop`), what pi's session file of a Lead at its idle ends with.
const FINISHED_TURN: &str = "{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n";

/// The pane of `Fx::cockpit` that a `session open` or `session fork` made, with a `session run <id>` started in it: the
/// wrapper runs on a thread of the test, stepped by it. Drops the wrapper, then the pane.
struct Running {
    w: Wrapper,
    pane: Pane,
}

impl Running {
    /// `session run <id>` in `pane`, a pane of the fake server a command made.
    fn attach(fx: &Fx, pane: &str, id: &str) -> Running {
        let pane = Pane::at(fx, pane, &installed_cpd(fx));
        let w = pane.run(&["session", "run", id]);
        Running { w, pane }
    }

    /// A new window of the cockpit for `id`, the row has its pane (as when cpd started it), and its first pi started.
    fn start(fx: &Fx, id: &str) -> Running {
        let pane = Pane::new(fx, id, &installed_cpd(fx));
        fx.env.sql(&format!(
            "UPDATE sessions SET tmux_pane = '{}' WHERE id = '{id}'",
            pane.pane
        ));
        let w = pane.run(&["session", "run", id]);
        let mut running = Running { w, pane };
        running.until_pis(1);
        running
    }

    /// Steps the wrapper until `n` pis have started.
    fn until_pis(&mut self, n: usize) {
        let pane = &self.pane;
        self.w
            .until(&format!("pi number {n}"), || pane.spawn.pis().len() >= n);
    }

    /// Steps the wrapper until the pane shows `text`.
    fn until_screen(&mut self, text: &str) {
        let pane = &self.pane;
        self.w.until(&format!("{text:?} on the screen"), || {
            pane.screen().contains(text)
        });
    }

    /// The `Launch` of the pi number `i` (from 0).
    fn pi(&self, i: usize) -> Launch {
        self.pane
            .spawn
            .pis()
            .get(i)
            .cloned()
            .unwrap_or_else(|| panic!("pi {i} did not start"))
    }

    /// The last pi ends with `status`, as the User's `/quit` (0) or a crash (1) would.
    fn pi_exits(&self, fx: &Fx, status: i32) {
        let pi: Pid = *self.pane.spawn.pids().last().expect("a pi runs");
        fx.fakes.spawn.exit(pi, status);
    }
}

/// The argument after `flag` in a pi's argv.
fn after(argv: &[String], flag: &str) -> String {
    argv[argv
        .iter()
        .position(|a| a == flag)
        .unwrap_or_else(|| panic!("no {flag} in {argv:?}"))
        + 1]
    .clone()
}

/// Fork when idle (§6.7): the fork waits while the source has no file, then
/// while it is busy, and forks once it is idle; pi exiting offers a resume.
#[test]
fn fork_waits_until_the_source_is_idle_then_forks_and_resumes() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    fx.cpd(&["session", "state", "working", "--session", LEAD1])
        .ok();
    let message = fx.env.root().join("message.md");
    fs::write(&message, "Review the tests.").unwrap();

    let out = fx
        .cpd(&[
            "session",
            "fork",
            LEAD1,
            "--name",
            "side",
            "--message-file",
            message.to_str().unwrap(),
        ])
        .ok();
    let new = out["session"]["id"].as_str().unwrap().to_owned();
    let mut fork = Running::attach(&fx, out["session"]["pane"].as_str().unwrap(), &new);
    fork.until_screen(
        "Waiting for the source Lead to finish its turn, then forking it as \"side\"",
    );

    // No file yet, and busy: three polls of the source (2 s each, on the wrapper's clock), and no pi.
    fork.w.steps(3);
    assert!(fork.pane.spawn.pis().is_empty());
    // A file with a finished turn, but still busy.
    let sessions = fx.env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    let source_file: PathBuf = sessions.join("2026_lead1.jsonl");
    fs::write(&source_file, FINISHED_TURN).unwrap();
    // Quiet for long enough and finished: only the busy source is in the way.
    fx.backdate(&source_file, Duration::from_secs(3600));
    fork.w.steps(3);
    assert!(
        fork.pane.spawn.pis().is_empty(),
        "the source is still working"
    );

    fx.cpd(&["session", "state", "idle", "--session", LEAD1])
        .ok();
    fork.until_pis(1);
    let pi = fork.pi(0).args;
    assert_eq!(after(&pi, "--fork"), source_file.display().to_string());
    assert_eq!(after(&pi, "--session-id"), new);
    assert_eq!(after(&pi, "-n"), "side");
    assert_eq!(
        after(&pi, "--model"),
        "github-copilot/claude-opus-5.5:medium"
    );
    let copied = home(&fx.env).session_message(&PlanId::parse("pay").unwrap(), &sid(&new));
    assert_eq!(
        pi[pi.len() - 2..],
        ["--".to_owned(), format!("@{}", copied.display())]
    );

    // pi created the fork's file: it is recorded; pi exited: the pane offers a resume.
    let fork_file = sessions.join(format!("2026_{new}.jsonl"));
    fs::write(&fork_file, "{\"type\":\"session\"}\n").unwrap();
    let fork_file = fork_file.display().to_string();
    fork.w.until("the recorded file", || {
        fx.env
            .query::<Option<String>>(&format!("SELECT file FROM sessions WHERE id = '{new}'"))
            .as_deref()
            == Some(fork_file.as_str())
    });
    assert_eq!(show(&fx, &new)["file"], json!(fork_file));
    fork.pi_exits(&fx, 1);
    fork.until_screen("pi exited. Press Enter to resume side here.");
    assert_eq!(show(&fx, &new)["state"], "stopped");
    fork.w.enter();
    fork.until_pis(2);
    let resume = fork.pi(1).args;
    // hs-d506: a resume names the row's Tier's model (`--model`) before the file.
    assert_eq!(
        resume[..4],
        [
            "--model".to_owned(),
            "github-copilot/claude-opus-5.5:medium".to_owned(),
            "--session".to_owned(),
            fork_file
        ]
    );
    assert!(
        !resume.iter().any(|a| a == "--fork" || a == "--"),
        "{resume:?}"
    );
}

/// A fork whose source is `stopped` with no session file can never see it idle: it says so,
/// offers Enter to retry, and forks once the source has a finished turn (hs-d196).
#[test]
fn fork_stops_waiting_for_a_stopped_source_without_a_file_and_retries_on_enter() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    let message = fx.env.root().join("message.md");
    fs::write(&message, "Review the tests.").unwrap();

    let out = fx
        .cpd(&[
            "session",
            "fork",
            LEAD2,
            "--name",
            "side",
            "--message-file",
            message.to_str().unwrap(),
        ])
        .ok();
    let new = out["session"]["id"].as_str().unwrap().to_owned();
    let mut fork = Running::attach(&fx, out["session"]["pane"].as_str().unwrap(), &new);
    fork.until_screen("Press Enter to retry.");
    assert!(
        fork.pane
            .screen()
            .contains("is stopped and has no session file"),
        "{}",
        fork.pane.screen()
    );
    assert!(fork.pane.spawn.pis().is_empty());

    // The source gets a file with a finished turn; Enter retries and forks.
    let sessions = fx.env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    let source_file: PathBuf = sessions.join("2026_lead2.jsonl");
    fs::write(&source_file, FINISHED_TURN).unwrap();
    fx.backdate(&source_file, Duration::from_secs(3600));
    fork.w.enter();
    fork.until_pis(1);
    assert_eq!(
        after(&fork.pi(0).args, "--fork"),
        source_file.display().to_string()
    );
}

/// Bigger than one argv element may be (MAX_ARG_STRLEN, 131072 bytes; hs-n111).
const MAX_ARG_STRLEN: usize = 131_072;

fn big_message() -> String {
    "A line of a first message that is longer than one argument may be.\n".repeat(140_000 / 66 + 1)
}

/// What cpd owes the kernel's limit (E2BIG at `exec`, which is not ours): pi's argv is `--` and `@<file>`, every
/// element under [`MAX_ARG_STRLEN`], and the file holds the whole text.
fn assert_pi_reads_the_message_from(argv: &[String], file: &std::path::Path, message: &str) {
    assert_eq!(
        argv[argv.len() - 2..],
        ["--".to_owned(), format!("@{}", file.display())]
    );
    assert!(
        argv.iter().all(|a| a.len() < MAX_ARG_STRLEN),
        "an argument of pi is over {MAX_ARG_STRLEN} bytes: {argv:?}"
    );
    assert!(
        fs::read_to_string(file).unwrap() == message,
        "pi reads the whole file"
    );
}

/// A Lead's first message goes to pi as `@<absolute path>`, so one over 128 KiB starts pi.
#[test]
fn a_fresh_lead_with_a_first_message_over_128_kib_starts_pi_with_its_file() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    let path = home(&fx.env).lead_message(&PlanId::parse("pay").unwrap());
    let message = big_message();
    assert!(message.len() > MAX_ARG_STRLEN);
    fs::write(&path, &message).unwrap();

    let out = fx.cpd(&["session", "open", LEAD1]).ok();
    let mut lead = Running::attach(&fx, out["pane"].as_str().unwrap(), LEAD1);
    lead.until_pis(1);
    assert_pi_reads_the_message_from(&lead.pi(0).args, &path, &message);
}

/// The same for a fork with `--message-file` (`run/<plan>/messages/<session>.md`).
#[test]
fn a_fork_with_a_message_file_over_128_kib_starts_pi_with_the_copied_file() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    pi_file(&fx.env, LEAD2, true);
    // Quiet for `fork_quiet_seconds` already: this test is about the message, not the wait.
    fx.backdate(
        &fx.env
            .home()
            .join(format!(".pi/agent/sessions/g/2026_{LEAD2}.jsonl")),
        Duration::from_secs(3600),
    );
    let message = big_message();
    let file = fx.env.root().join("message.md");
    fs::write(&file, &message).unwrap();

    let out = fx
        .cpd(&[
            "session",
            "fork",
            LEAD2,
            "--plan",
            "pay",
            "--message-file",
            file.to_str().unwrap(),
        ])
        .ok();
    let new = out["session"]["id"].as_str().unwrap().to_owned();
    let copied = home(&fx.env).session_message(&PlanId::parse("pay").unwrap(), &sid(&new));
    let mut fork = Running::attach(&fx, out["session"]["pane"].as_str().unwrap(), &new);
    fork.until_pis(1);
    assert!(
        fork.pi(0).args.iter().any(|a| a == "--fork"),
        "{:?}",
        fork.pi(0)
    );
    assert_pi_reads_the_message_from(&fork.pi(0).args, &copied, &message);
}

// ---------- open, park ----------

#[test]
fn open_writes_session_moved_before_the_rename_so_a_failing_rename_keeps_the_swap_recorded() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    fx.cpd(&["session", "open", LEAD1]).ok();

    // A tmux that fails `rename-window` and answers everything else as the fake server does.
    let _ = (*fx.fakes.runner).clone().on(
        "tmux",
        |call| call.args.iter().any(|a| a == "rename-window"),
        Reply::exit(1, "", "rename-window: refused\n"),
    );

    let run = fx.cpd(&["session", "open", LEAD2]);
    refused(&run, "tmux_failed", 1);
    assert_eq!(slot(&fx).session, LEAD2, "the swap happened");
    let moved: Vec<(String, String)> = events(&fx, "session-moved")
        .iter()
        .map(|e| {
            (
                e["session"].as_str().unwrap().to_owned(),
                e["data"]["place"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let expected = [(LEAD1, "chat"), (LEAD2, "chat"), (LEAD1, "parked")];
    assert_eq!(moved, expected.map(|(s, p)| (s.to_owned(), p.to_owned())));
    assert_eq!(
        (
            show(&fx, LEAD1)["place"].clone(),
            show(&fx, LEAD2)["place"].clone()
        ),
        (json!("parked"), json!("chat"))
    );
}

#[test]
fn open_and_park_take_a_context_s_manager_and_park_it_in_its_context_window() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    // `context manager` wrote its first message.
    let message = home(&fx.env).manager_message(&cpd_core::ids::ContextId::parse("ctx").unwrap());
    fs::create_dir_all(message.parent().unwrap()).unwrap();
    fs::write(&message, "You are the Manager.").unwrap();

    // A stopped Manager with no pane is resumed in a new window and swapped in.
    let out = ok_with_panes(&fx, &["session", "open", MANAGER]);
    assert_eq!(out["displaced"], Value::Null);
    assert_eq!(slot(&fx).session, MANAGER);
    assert_placeholders(&fx, 0);
    // Its pane runs `cpd session run`: the wrapper reads the message and starts pi with `@<message>` (the wrapper's tests).
    let pane = slot(&fx).pane;
    let argv = pane_argv(&fx, &pane);
    assert_eq!(argv[argv.len() - 3..], ["session", "run", MANAGER]);
    assert!(message.is_file());
    assert_eq!(show(&fx, MANAGER)["place"], "chat");

    // A Lead displaces it into `ctx-<context>`.
    assert_eq!(
        ok_with_panes(&fx, &["session", "open", LEAD1])["displaced"],
        MANAGER
    );
    let manager = panes(&fx)
        .into_iter()
        .find(|p| p.session == MANAGER)
        .unwrap();
    assert_eq!(manager.window_name, "ctx-ctx");
    assert_eq!(window_names(&fx), ["ctx-ctx", "main"]);
    assert_eq!(show(&fx, MANAGER)["place"], "parked");

    // Parking the Lead, then opening the Manager again uses its live pane.
    ok_with_panes(&fx, &["session", "park"]);
    let out = ok_with_panes(&fx, &["session", "open", MANAGER]);
    assert_eq!(out["pane"], json!(manager.pane));
    ok_with_panes(&fx, &["session", "park", MANAGER]);
    assert_eq!(
        panes(&fx)
            .into_iter()
            .find(|p| p.session == MANAGER)
            .unwrap()
            .window_name,
        "ctx-ctx"
    );
}

#[test]
fn open_and_park_refusals() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    // No cockpit yet.
    let error = refused(&fx.cpd(&["session", "open", LEAD1]), "no_cockpit", 3);
    assert_eq!(error["details"]["next"], "cpd tui");
    refused(&fx.cpd(&["session", "park"]), "no_cockpit", 3);
    refused(&fx.cpd(&["session", "park", LEAD1]), "no_cockpit", 3);

    fx.cockpit();
    refused(&fx.cpd(&["session", "open", WORKER]), "not_a_lead", 3);
    refused(
        &fx.cpd(&["session", "open", "nobody"]),
        "session_not_found",
        4,
    );
    refused(&fx.cpd(&["session", "park", WORKER]), "not_a_lead", 3);
    refused(
        &fx.cpd(&["session", "park", "nobody"]),
        "session_not_found",
        4,
    );
    assert_placeholders(&fx, 1);
    assert_eq!(window_names(&fx), ["main"]);
}

// ---------- history ----------

/// Lead `lead2`, archived, with a readable session file.
fn archived_lead2(env: &TestEnv) {
    let file = env.root().join("lead2.jsonl");
    fs::write(&file, "{\"type\":\"session\"}\n").unwrap();
    env.sql(&format!(
        "UPDATE sessions SET file = '{}', archived_at = 't9' WHERE id = 'lead2'",
        file.display()
    ));
}

fn history_panes(env: &impl Server) -> Vec<PaneRow> {
    panes(env)
        .into_iter()
        .filter(|p| p.view == "history")
        .collect()
}

#[test]
fn history_parks_the_lead_in_the_slot_and_open_kills_the_history_pane() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    archived_lead2(&fx.env);
    let lead1_pane = fx.cpd(&["session", "open", LEAD1]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(window_names(&fx), ["main"]);

    // The Lead in the slot is parked; the history pane takes the slot.
    let out = fx.cpd(&["session", "history", LEAD2]).ok();
    assert_eq!(out["session"], LEAD2);
    assert_eq!(out["parked"], LEAD1);
    let s = slot(&fx);
    assert_eq!(
        (s.view.as_str(), s.history.as_str(), s.session.as_str()),
        ("history", LEAD2, "")
    );
    assert_eq!(out["pane"], json!(s.pane));
    assert_eq!(s.window_name, "main");
    // The history pane's program is `cpd tui view history <id>`.
    let argv = pane_argv(&fx, &s.pane);
    assert_eq!(argv[argv.len() - 4..], ["tui", "view", "history", LEAD2]);
    let lead1 = panes(&fx).into_iter().find(|p| p.session == LEAD1).unwrap();
    assert_eq!(
        (lead1.pane.as_str(), lead1.window_name.as_str()),
        (lead1_pane.as_str(), "park-pay")
    );
    assert_eq!(show(&fx, LEAD1)["place"], "parked");
    assert_eq!(history_panes(&fx).len(), 1);
    assert_placeholders(&fx, 0);
    assert_eq!(window_names(&fx), ["main", "park-pay"]);

    // A rerun respawns the same pane: still one history pane, nothing parked.
    let again = fx.cpd(&["session", "history", LEAD2]).ok();
    assert_eq!(
        (&again["pane"], &again["parked"]),
        (&out["pane"], &Value::Null)
    );
    assert_eq!(history_panes(&fx).len(), 1);
    assert_eq!(window_names(&fx), ["main", "park-pay"]);

    // There is no Lead to park in a history pane.
    assert_eq!(
        fx.cpd(&["session", "park"]).ok(),
        json!({"ok": true, "session": null, "already": true})
    );
    assert_eq!(fx.cpd(&["session", "park", LEAD1]).ok()["already"], true);
    assert_eq!(history_panes(&fx).len(), 1);

    // A Lead taking the slot kills the history pane with its window.
    let out = fx.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!(
        (&out["pane"], &out["displaced"]),
        (&json!(lead1_pane), &Value::Null)
    );
    assert_eq!(slot(&fx).session, LEAD1);
    assert!(history_panes(&fx).is_empty());
    assert_placeholders(&fx, 0);
    assert_eq!(window_names(&fx), ["main"]);

    let moved: Vec<(String, String)> = events(&fx, "session-moved")
        .iter()
        .map(|e| {
            (
                e["session"].as_str().unwrap().to_owned(),
                e["data"]["place"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let expected = [(LEAD1, "chat"), (LEAD1, "parked"), (LEAD1, "chat")];
    assert_eq!(moved, expected.map(|(s, p)| (s.to_owned(), p.to_owned())));
}

#[test]
fn history_replaces_the_placeholder_in_place_and_park_brings_it_back() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    archived_lead2(&fx.env);
    let placeholder = slot(&fx);

    let out = fx.cpd(&["session", "history", LEAD2]).ok();
    assert_eq!(out["parked"], Value::Null);
    let s = slot(&fx);
    assert_eq!(
        (s.pane.as_str(), s.view.as_str(), s.history.as_str()),
        (placeholder.pane.as_str(), "history", LEAD2)
    );
    assert_placeholders(&fx, 0);
    assert_eq!(window_names(&fx), ["main"]);
    assert!(events(&fx, "session-moved").is_empty());

    // The history of another Session retargets the same pane.
    fx.env.sql(&format!(
        "UPDATE sessions SET file = '{}' WHERE id = 'lead1'",
        fx.env.root().join("lead2.jsonl").display()
    ));
    fx.cpd(&["session", "history", LEAD1]).ok();
    let s = slot(&fx);
    assert_eq!(
        (s.pane.as_str(), s.history.as_str()),
        (placeholder.pane.as_str(), LEAD1)
    );
    assert_eq!(history_panes(&fx).len(), 1);
}

#[test]
fn history_refusals_change_nothing() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    refused(
        &fx.cpd(&["session", "history", "nobody"]),
        "session_not_found",
        4,
    );
    refused(&fx.cpd(&["session", "history", WORKER]), "not_a_lead", 3);
    refused(&fx.cpd(&["session", "history", MANAGER]), "not_a_lead", 3);
    fx.env
        .sql("UPDATE sessions SET file = '/nonexistent/lead2.jsonl' WHERE id = 'lead2'");
    refused(
        &fx.cpd(&["session", "history", LEAD2]),
        "file_unreadable",
        2,
    );
    archived_lead2(&fx.env);
    // No cockpit yet.
    let error = refused(&fx.cpd(&["session", "history", LEAD2]), "no_cockpit", 3);
    assert_eq!(error["details"]["next"], "cpd tui");

    fx.cockpit();
    fx.cpd(&["session", "open", LEAD1]).ok();
    refused(&fx.cpd(&["session", "history", WORKER]), "not_a_lead", 3);
    refused(&fx.cpd(&["session", "history", MANAGER]), "not_a_lead", 3);
    // The refused ones parked nothing.
    assert_eq!(slot(&fx).session, LEAD1);
    assert!(history_panes(&fx).is_empty());
    assert_eq!(events(&fx, "session-moved").len(), 1);
}

// ---------- close ----------

#[test]
fn close_refusals_and_nothing_to_close() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    refused(&fx.cpd(&["session", "close", WORKER]), "not_a_lead", 3);
    refused(
        &fx.cpd(&["session", "close", "nobody"]),
        "session_not_found",
        4,
    );
    // A Lead with no pane: nothing to do.
    fx.cockpit();
    assert_eq!(
        fx.cpd(&["session", "close", LEAD2]).ok(),
        json!({"ok": true, "session": LEAD2, "pane": null, "already": true})
    );
    // A pane that is gone, with E2 not having recorded it: nothing is written.
    fx.env
        .sql("UPDATE sessions SET tmux_pane = '%99' WHERE id = 'lead2'");
    assert_eq!(fx.cpd(&["session", "close", LEAD2]).ok()["already"], true);
    assert_eq!(
        fx.env
            .query::<String>("SELECT tmux_pane FROM sessions WHERE id = 'lead2'"),
        "%99"
    );
    assert_eq!(events(&fx, "session-moved").len(), 0);
    assert_eq!(events(&fx, "session-state").len(), 0);
}

#[test]
fn close_refuses_the_lead_in_the_chat_slot_and_leaves_everything() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    let pane = fx.cpd(&["session", "open", LEAD1]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    let error = refused(&fx.cpd(&["session", "close", LEAD1]), "in_chat_slot", 3);
    assert_eq!(error["details"]["next"], "cpd session park");
    assert_eq!(slot(&fx).pane, pane);
    assert_eq!(show(&fx, LEAD1)["tmux_pane"], json!(pane));
    assert_eq!(events(&fx, "session-moved").len(), 1);
}

#[test]
fn close_kills_a_parked_pane_and_writes_what_e2_writes() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    // lead2 displaces lead1 into a park window; lead1 is made `idle`, as pi would report.
    let lead1_pane = fx.cpd(&["session", "open", LEAD1]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.cpd(&["session", "open", LEAD2]).ok();
    assert_eq!(show(&fx, LEAD1)["place"], "parked");
    assert_eq!(window_names(&fx), ["main", "park-pay"]);
    fx.env
        .sql("UPDATE sessions SET state = 'idle' WHERE id = 'lead1'");

    let out = fx.cpd(&["session", "close", LEAD1]).ok();
    assert_eq!(
        out,
        json!({"ok": true, "session": LEAD1, "pane": lead1_pane})
    );
    // The pane is gone with its window; the file of pi stays (not touched); the slot is unchanged.
    assert!(panes(&fx).iter().all(|p| p.session != LEAD1));
    assert_eq!(window_names(&fx), ["main"]);
    assert_eq!(slot(&fx).session, LEAD2);
    let lead1 = show(&fx, LEAD1);
    assert_eq!(
        (&lead1["state"], &lead1["tmux_pane"], &lead1["place"]),
        (&json!("stopped"), &Value::Null, &json!("none"))
    );
    let state: Vec<Value> = events(&fx, "session-state")
        .into_iter()
        .filter(|e| e["session"] == LEAD1)
        .collect();
    assert_eq!(state.len(), 1);
    assert_eq!(
        state[0]["data"],
        json!({"from": "idle", "to": "stopped", "label": null})
    );
    let moved = events(&fx, "session-moved");
    let last = moved.last().unwrap();
    assert_eq!(
        (&last["session"], &last["data"]),
        (
            &json!(LEAD1),
            &json!({"pane": null, "window": null, "place": "none"})
        )
    );
    let after = moved.len();

    // Idempotent: nothing more is written.
    assert_eq!(fx.cpd(&["session", "close", LEAD1]).ok()["already"], true);
    assert_eq!(events(&fx, "session-moved").len(), after);

    // Open resumes it: a new pane, in the slot.
    let out = fx.cpd(&["session", "open", LEAD1]).ok();
    assert_ne!(out["pane"], json!(lead1_pane));
    assert_eq!(out["displaced"], LEAD2);
}

#[test]
fn close_of_a_stopped_lead_with_a_pane_appends_only_session_moved() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    // lead2 is `stopped` (the fake pi reports nothing) and parked by lead1.
    fx.cpd(&["session", "open", LEAD2]).ok();
    fx.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!(
        (
            show(&fx, LEAD2)["state"].clone(),
            show(&fx, LEAD2)["place"].clone()
        ),
        (json!("stopped"), json!("parked"))
    );
    let states = events(&fx, "session-state").len();

    fx.cpd(&["session", "close", LEAD2]).ok();
    assert_eq!(events(&fx, "session-state").len(), states);
    let lead2 = show(&fx, LEAD2);
    assert_eq!(
        (&lead2["tmux_pane"], &lead2["place"]),
        (&Value::Null, &json!("none"))
    );
    assert!(panes(&fx).iter().all(|p| p.session != LEAD2));
}

#[test]
fn rename_works_on_an_archived_session() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.env
        .sql("UPDATE sessions SET archived_at = 't9' WHERE id = 'lead2'");
    let out = fx
        .cpd(&[
            "session",
            "rename",
            "Archived and renamed",
            "--session",
            "lead2",
        ])
        .ok();
    assert_eq!(
        out,
        json!({"ok": true, "session": "lead2", "name": "Archived and renamed"})
    );
    let row = show(&fx, "lead2");
    assert_eq!(
        (&row["name"], &row["archived_at"]),
        (&json!("Archived and renamed"), &json!("t9"))
    );
    assert_eq!(events(&fx, "session-renamed").len(), 1);
}

/// A pane's `@cpd_title`, as the fake server holds it.
fn pane_title(fx: &Fx, session: &str) -> String {
    let pane = panes(fx)
        .into_iter()
        .find(|p| p.session == session)
        .unwrap()
        .pane;
    let out = fx.tmux(&["show", "-pqv", "-t", &pane, "@cpd_title"]);
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .to_owned()
}

#[test]
fn rename_sets_the_title_of_a_forks_live_pane_to_its_name() {
    let fx = Fx::with_ports();
    let fork = parked_fork(&fx);
    assert_eq!(pane_title(&fx, &fork), "Lead One (fork)");
    fx.cpd(&["session", "rename", "side quest", "--session", &fork])
        .ok();
    assert_eq!(pane_title(&fx, &fork), "Side Quest");

    // A Session with no pane has nothing to set.
    fx.cpd(&[
        "session",
        "rename",
        "Lead two renamed",
        "--session",
        "lead2",
    ])
    .ok();
}

#[test]
fn open_gives_a_new_pane_the_plans_title_for_the_plans_lead_and_the_name_for_a_fork() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    fx.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!(pane_title(&fx, LEAD1), "Pay");
    let fork = fx.cpd(&["session", "fork", LEAD1, "--name", "side"]).ok()["session"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(pane_title(&fx, &fork), "Side");
}

#[test]
fn open_resumes_a_titled_lead_with_its_dynamic_title_and_a_rename_keeps_it() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    fx.env.sql(&format!(
        "UPDATE sessions SET title = 'Dynamic subject', titled_at = 't' WHERE id = '{LEAD1}'"
    ));
    // The Lead has no pane: opening resumes it in a new window, with the title.
    fx.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!(pane_title(&fx, LEAD1), "Dynamic Subject");
    // A rename writes the name and leaves the header.
    fx.cpd(&["session", "rename", "Lead one renamed", "--session", LEAD1])
        .ok();
    assert_eq!(pane_title(&fx, LEAD1), "Dynamic Subject");
    assert_eq!(show(&fx, LEAD1)["name"], "Lead one renamed");
}

// ---------- archive ----------

/// A parked side fork of `lead1` (stopped, with a pane) in the cockpit of the fake server.
fn parked_fork(fx: &Fx) -> String {
    setup(&fx.env);
    fx.cockpit();
    fx.cpd(&["session", "fork", LEAD1]).ok()["session"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn archived_at(env: &impl Cpd, id: &str) -> Value {
    show(env, id)["archived_at"].clone()
}

#[test]
fn archive_kills_the_pane_of_an_idle_parked_fork_and_appends_one_event() {
    let fx = Fx::with_ports();
    let fork = parked_fork(&fx);
    fx.env.sql(&format!(
        "UPDATE sessions SET state = 'idle' WHERE id = '{fork}'"
    ));
    let pane = show(&fx, &fork)["tmux_pane"].as_str().unwrap().to_owned();
    assert_eq!(archived_at(&fx, &fork), Value::Null);

    let out = fx.cpd(&["session", "archive", &fork]).ok();
    assert_eq!(
        out,
        json!({"ok": true, "archived": [{"kind": "session", "id": fork}], "closed": [pane]})
    );
    assert!(panes(&fx).iter().all(|p| p.session != fork));
    let row = show(&fx, &fork);
    assert!(row["archived_at"].is_string(), "{row}");
    assert_eq!(
        (&row["state"], &row["tmux_pane"], &row["place"]),
        (&json!("stopped"), &Value::Null, &json!("none"))
    );
    let archived = events(&fx, "session-archived");
    assert_eq!(archived.len(), 1);
    assert_eq!(
        (&archived[0]["session"], &archived[0]["data"]),
        (&json!(fork), &json!({"archived": true}))
    );

    // `session list --archived` shows it; the Plan's Lead and the other Sessions are not archived.
    let list = fx.cpd(&["session", "list", "--archived"]).ok();
    let at = |id: &str| {
        list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap()["archived_at"]
            .clone()
    };
    assert_eq!(at(&fork), row["archived_at"]);
    assert_eq!(at(LEAD1), Value::Null);

    // Idempotent: nothing more is written, nothing closed.
    let again = fx.cpd(&["session", "archive", &fork]).ok();
    assert_eq!(
        again,
        json!({"ok": true, "archived": [], "closed": [], "already": true})
    );
    assert_eq!(events(&fx, "session-archived").len(), 1);
}

#[test]
fn archive_of_a_lead_with_no_pane_closes_nothing() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    let fork = "forkless";
    fx.env.sql(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, forked_from)
               VALUES ('{fork}', 'lead', 'ctx', 'pay', 'Side', '/', 'O3', 'stopped', '2020-01-01T00:00:00Z', 't', 'lead1')"
        ),
    );
    let out = fx.cpd(&["session", "archive", fork]).ok();
    assert_eq!(
        out,
        json!({"ok": true, "archived": [{"kind": "session", "id": fork}], "closed": []})
    );
    assert_eq!(events(&fx, "session-archived").len(), 1);
}

#[test]
fn archive_refuses_a_working_waiting_or_blocked_session_and_changes_nothing() {
    let fx = Fx::with_ports();
    let fork = parked_fork(&fx);
    let pane = show(&fx, &fork)["tmux_pane"].as_str().unwrap().to_owned();
    let moved = events(&fx, "session-moved").len();
    for state in ["working", "waiting", "blocked"] {
        fx.env.sql(&format!(
            "UPDATE sessions SET state = '{state}', state_label = {} WHERE id = '{fork}'",
            if state == "working" { "NULL" } else { "'why'" }
        ));
        let error = refused(
            &fx.cpd(&["session", "archive", &fork]),
            "archive_blocked",
            3,
        );
        let blockers = error["details"]["blockers"].as_array().unwrap();
        assert_eq!(blockers.len(), 1, "{error}");
        assert_eq!(
            (&blockers[0]["kind"], &blockers[0]["id"]),
            (&json!("session"), &json!(fork))
        );
        assert!(
            blockers[0]["reason"].as_str().unwrap().contains(state),
            "{error}"
        );
        assert_eq!(archived_at(&fx, &fork), Value::Null);
        assert_eq!(show(&fx, &fork)["tmux_pane"], json!(pane));
        assert!(panes(&fx).iter().any(|p| p.session == fork));
    }
    assert_eq!(events(&fx, "session-archived").len(), 0);
    assert_eq!(events(&fx, "session-moved").len(), moved);
}

#[test]
fn archive_refuses_the_chat_slot_a_plan_lead_a_worker_and_an_unknown_session() {
    let fx = Fx::with_ports();
    let fork = parked_fork(&fx);
    fx.env.sql(&format!(
        "UPDATE sessions SET state = 'idle' WHERE id = '{fork}'"
    ));
    let pane = fx.cpd(&["session", "open", &fork]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    let error = refused(&fx.cpd(&["session", "archive", &fork]), "in_chat_slot", 3);
    assert_eq!(error["details"]["next"], "cpd session park");
    assert_eq!(slot(&fx).pane, pane);
    assert_eq!(archived_at(&fx, &fork), Value::Null);

    let error = refused(
        &fx.cpd(&["session", "archive", LEAD1]),
        "archive_blocked",
        3,
    );
    assert_eq!(error["details"]["next"], "cpd plan archive --plan pay");
    assert_eq!(archived_at(&fx, LEAD1), Value::Null);

    refused(&fx.cpd(&["session", "archive", WORKER]), "not_a_lead", 3);
    refused(
        &fx.cpd(&["session", "archive", "nobody"]),
        "session_not_found",
        4,
    );
    assert_eq!(events(&fx, "session-archived").len(), 0);
}

#[test]
fn unarchive_clears_archived_at_with_one_event_and_touches_no_pane() {
    let fx = Fx::with_ports();
    let fork = parked_fork(&fx);
    fx.cpd(&["session", "archive", &fork]).ok();
    let before = panes(&fx);

    let out = fx.cpd(&["session", "unarchive", &fork]).ok();
    assert_eq!(
        out,
        json!({"ok": true, "archived": [{"kind": "session", "id": fork}], "closed": []})
    );
    assert_eq!(archived_at(&fx, &fork), Value::Null);
    assert_eq!(panes(&fx), before);
    let archived = events(&fx, "session-archived");
    assert_eq!(archived.len(), 2);
    assert_eq!(archived[1]["data"], json!({"archived": false}));

    // Idempotent.
    let again = fx.cpd(&["session", "unarchive", &fork]).ok();
    assert_eq!(
        again,
        json!({"ok": true, "archived": [], "closed": [], "already": true})
    );
    assert_eq!(events(&fx, "session-archived").len(), 2);

    refused(&fx.cpd(&["session", "unarchive", WORKER]), "not_a_lead", 3);
    refused(
        &fx.cpd(&["session", "unarchive", "nobody"]),
        "session_not_found",
        4,
    );
}

#[test]
fn unarchive_is_refused_while_its_plan_or_context_is_archived() {
    let fx = Fx::new();
    setup(&fx.env);
    fx.env
        .sql("UPDATE sessions SET archived_at = 't' WHERE id = 'lead2'");
    fx.env
        .sql("UPDATE plans SET archived_at = 't' WHERE id = 'other'");
    let error = refused(&fx.cpd(&["session", "unarchive", LEAD2]), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd plan unarchive --plan other");
    fx.env
        .sql("UPDATE contexts SET archived_at = 't' WHERE id = 'ctx'");
    let error = refused(&fx.cpd(&["session", "unarchive", LEAD2]), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd context unarchive ctx");
    assert_eq!(archived_at(&fx, LEAD2), json!("t"));
    assert_eq!(events(&fx, "session-archived").len(), 0);
}

/// No tmux server runs here: an `archived` refusal, not `no_cockpit`, shows that nothing asked tmux first.
#[test]
fn open_and_fork_of_an_archived_session_are_refused_before_any_tmux() {
    let fx = Fx::new();
    setup(&fx.env);
    fx.env
        .sql("UPDATE sessions SET archived_at = 't' WHERE id = 'lead2'");
    for args in [["session", "open", LEAD2], ["session", "fork", LEAD2]] {
        let error = refused(&fx.cpd(&args), "archived", 3);
        assert_eq!(error["details"]["next"], "cpd session unarchive lead2");
    }
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM sessions"), 4);
    assert_eq!(events(&fx, "session-registered").len(), 1);
}

// ---------- dead Lead panes (contract §3.6, §3.7 E2; commands.md §10.4) ----------

/// The cockpit of the fake server, in a store with the Leads (`setup`): every pane the tests open runs nothing, and a
/// pane whose program ends is made so by [`Fx::pane_exit`] (the fake runs no program).
fn dying_leads(fx: &Fx) {
    setup(&fx.env);
    fx.cockpit();
}

#[test]
fn e2_kills_the_window_of_a_dead_parked_lead() {
    let fx = Fx::with_ports();
    dying_leads(&fx);
    let lead1_pane = fx.cpd(&["session", "open", LEAD1]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.cpd(&["session", "open", LEAD2]).ok();
    assert_eq!(window_names(&fx), ["main", "park-pay"]);
    fx.pane_exit(&lead1_pane, 1);
    assert!(fx.pane_dead(&lead1_pane));

    fx.engine_tick();
    // The dead window is killed (its pane is gone), and E2 restarts the Lead in a new parked window.
    assert!(
        panes(&fx).iter().all(|p| p.pane != lead1_pane),
        "the dead pane's window is killed"
    );
    assert_restarted_parked(&fx, LEAD1, &lead1_pane, "park-pay");
    assert_eq!(window_names(&fx), ["main", "park-pay"]);
    assert_eq!(slot(&fx).session, LEAD2, "the chat slot is untouched");
}

/// The `session-moved` Events of `id`, as `(place, pane)`, in order.
fn moves_of(env: &impl Cpd, id: &str) -> Vec<(String, Value)> {
    events(env, "session-moved")
        .into_iter()
        .filter(|e| e["session"] == id)
        .map(|e| {
            (
                e["data"]["place"].as_str().unwrap().to_owned(),
                e["data"]["pane"].clone(),
            )
        })
        .collect()
}

/// `id` runs in a new pane of the parked window `window`, started by `cpd session run <id>`; the store records it, and
/// `session-moved` `parked` follows the `none` of the dead pane.
fn assert_restarted_parked(fx: &Fx, id: &str, old_pane: &str, window: &str) {
    let mine: Vec<PaneRow> = panes(fx).into_iter().filter(|p| p.session == id).collect();
    assert_eq!(mine.len(), 1, "one pane of {id}: {mine:?}");
    let pane = &mine[0];
    assert_ne!(pane.pane, old_pane, "a new pane");
    assert_eq!(pane.window_name, window);
    let argv = pane_argv(fx, &pane.pane);
    assert_eq!(argv[argv.len() - 3..], ["session", "run", id]);
    assert!(!fx.pane_dead(&pane.pane));
    let row = show(fx, id);
    assert_eq!(
        (&row["tmux_pane"], &row["place"]),
        (&json!(pane.pane), &json!("parked"))
    );
    let moves = moves_of(fx, id);
    let [.., none, parked] = moves.as_slice() else {
        panic!("no restart in the moves of {id}: {moves:?}")
    };
    assert_eq!(none, &("none".to_owned(), Value::Null));
    assert_eq!(parked, &("parked".to_owned(), json!(pane.pane)));
}

/// The `error` Events about a Session or `session_restart_loop`: the fixture's own errors (an Attempt without a worktree, E5's
/// spawn) are not what these tests are about.
fn session_errors(fx: &Fx) -> Vec<Value> {
    events(fx, "error")
        .into_iter()
        .filter(|e| !e["session"].is_null() || e["data"]["code"] == "session_restart_loop")
        .collect()
}

/// `lead1` (idle) parked in `park-pay` and `lead2` (made `working`) parked in `park-other`, the chat slot the placeholder.
fn two_parked_leads(fx: &Fx) -> (String, String) {
    dying_leads(fx);
    fx.env
        .sql("UPDATE sessions SET state = 'working' WHERE id = 'lead2'");
    let one = fx.cpd(&["session", "open", LEAD1]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    let two = fx.cpd(&["session", "open", LEAD2]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.cpd(&["session", "park"]).ok();
    assert_eq!(window_names(fx), ["main", "park-other", "park-pay"]);
    (one, two)
}

#[test]
fn e2_restarts_a_lead_whose_pane_died_while_it_was_working_in_a_parked_window() {
    let fx = Fx::with_ports();
    let (one, _) = two_parked_leads(&fx);
    fx.env
        .sql("UPDATE sessions SET state = 'working' WHERE id = 'lead1'");
    fx.pane_exit(&one, 1);

    fx.engine_tick();
    assert_restarted_parked(&fx, LEAD1, &one, "park-pay");
    assert_eq!(window_names(&fx), ["main", "park-other", "park-pay"]);
    assert_placeholders(&fx, 1);
    assert!(slot(&fx).session.is_empty(), "the chat slot never changes");
    // The other Lead's pane lives: nothing was done to it.
    let places: Vec<String> = moves_of(&fx, LEAD2)
        .into_iter()
        .map(|(place, _)| place)
        .collect();
    assert_eq!(places, ["chat", "parked"]);

    // Idempotent: the new pane lives, a tick writes nothing.
    let moved = events(&fx, "session-moved").len();
    fx.engine_tick();
    assert_eq!(events(&fx, "session-moved").len(), moved);
}

#[test]
fn e2_restarts_a_dead_manager_in_its_context_window() {
    let fx = Fx::with_ports();
    dying_leads(&fx);
    let message = home(&fx.env).manager_message(&cpd_core::ids::ContextId::parse("ctx").unwrap());
    fs::create_dir_all(message.parent().unwrap()).unwrap();
    fs::write(&message, "You are the Manager.").unwrap();
    fx.env
        .sql("UPDATE sessions SET state = 'idle' WHERE id = 'manager1'");
    let old = fx.cpd(&["session", "open", MANAGER]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!(window_names(&fx), ["ctx-ctx", "main"]);
    fx.pane_exit(&old, 1);

    fx.engine_tick();
    assert_restarted_parked(&fx, MANAGER, &old, "ctx-ctx");
    assert_eq!(window_names(&fx), ["ctx-ctx", "main"]);
    assert_eq!(slot(&fx).session, LEAD1, "the chat slot is untouched");
}

#[test]
fn e2_does_not_restart_a_lead_that_stopped_itself_or_was_closed() {
    let fx = Fx::with_ports();
    let (one, two) = two_parked_leads(&fx);
    // lead2 died working: restarted (the control). lead1 wrote `stopped` first, as `/quit` does.
    fx.cpd(&["session", "state", "stopped", "--session", LEAD1])
        .ok();
    fx.pane_exit(&one, 0);
    fx.pane_exit(&two, 1);
    fx.engine_tick();
    assert_restarted_parked(&fx, LEAD2, &two, "park-other");
    assert_eq!(window_names(&fx), ["main", "park-other"]);
    assert!(show(&fx, LEAD1)["tmux_pane"].is_null());
    assert_eq!(
        (
            show(&fx, LEAD1)["state"].clone(),
            show(&fx, LEAD1)["place"].clone()
        ),
        (json!("stopped"), json!("none"))
    );
    assert_eq!(moves_of(&fx, LEAD1).last().unwrap().0, "none");

    // `session close` writes `stopped` and kills the pane: the engine restarts nothing.
    fx.env
        .sql("UPDATE sessions SET state = 'idle' WHERE id = 'lead1'");
    fx.cpd(&["session", "open", LEAD1]).ok();
    fx.cpd(&["session", "park"]).ok();
    assert_eq!(window_names(&fx), ["main", "park-other", "park-pay"]);
    fx.cpd(&["session", "close", LEAD1]).ok();
    let moved = moves_of(&fx, LEAD1);
    fx.engine_tick();
    assert_eq!(window_names(&fx), ["main", "park-other"]);
    assert_eq!(moves_of(&fx, LEAD1), moved);
    assert!(show(&fx, LEAD1)["tmux_pane"].is_null());
}

#[test]
fn e2_does_not_restart_an_archived_lead_or_the_lead_of_a_done_plan() {
    let fx = Fx::with_ports();
    dying_leads(&fx);
    let message = home(&fx.env).manager_message(&cpd_core::ids::ContextId::parse("ctx").unwrap());
    fs::create_dir_all(message.parent().unwrap()).unwrap();
    fs::write(&message, "You are the Manager.").unwrap();
    // The Manager is the control: it is restarted; both Leads are not.
    fx.env
        .sql("UPDATE sessions SET state = 'idle' WHERE id IN ('manager1', 'lead2')");
    let manager = fx.cpd(&["session", "open", MANAGER]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    let one = fx.cpd(&["session", "open", LEAD1]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    let two = fx.cpd(&["session", "open", LEAD2]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.cpd(&["session", "park"]).ok();
    assert_eq!(
        window_names(&fx),
        ["ctx-ctx", "main", "park-other", "park-pay"]
    );
    fx.env
        .sql("UPDATE sessions SET archived_at = 't9' WHERE id = 'lead1'");
    for phase in ["plan", "dispatch", "deploy"] {
        fx.env.sql(&format!(
            "UPDATE plans SET phase = '{phase}', locked_at = {} WHERE id = 'other'",
            if phase == "plan" { "NULL" } else { "'t'" }
        ));
    }
    fx.env
        .sql("UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'other'");
    for pane in [&manager, &one, &two] {
        fx.pane_exit(pane, 1);
    }

    fx.engine_tick();
    assert_restarted_parked(&fx, MANAGER, &manager, "ctx-ctx");
    assert_eq!(window_names(&fx), ["ctx-ctx", "main"]);
    for id in [LEAD1, LEAD2] {
        assert!(show(&fx, id)["tmux_pane"].is_null(), "{id}");
        assert_eq!(moves_of(&fx, id).last().unwrap().0, "none", "{id}");
    }
}

#[test]
fn e2_leaves_a_lead_stopped_and_raises_session_restart_loop_once_when_its_restart_died_within_ten_minutes()
 {
    let fx = Fx::with_ports();
    dying_leads(&fx);
    let old = fx.cpd(&["session", "open", LEAD1]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.cpd(&["session", "open", LEAD2]).ok();
    fx.pane_exit(&old, 1);
    fx.engine_tick();
    assert_restarted_parked(&fx, LEAD1, &old, "park-pay");
    let restarted = show(&fx, LEAD1)["tmux_pane"].as_str().unwrap().to_owned();
    assert!(session_errors(&fx).is_empty(), "{:?}", session_errors(&fx));

    // The restart died nine minutes later (the fake clock): the engine does not start it again.
    fx.env
        .sql("UPDATE sessions SET state = 'working' WHERE id = 'lead1'");
    fx.advance(Duration::from_secs(9 * 60));
    fx.pane_exit(&restarted, 1);
    fx.engine_tick();
    assert_eq!(window_names(&fx), ["main"]);
    assert!(panes(&fx).iter().all(|p| p.session != LEAD1));
    let lead1 = show(&fx, LEAD1);
    assert_eq!(
        (&lead1["state"], &lead1["tmux_pane"], &lead1["place"]),
        (&json!("stopped"), &Value::Null, &json!("none"))
    );
    let errors = session_errors(&fx);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(
        (&errors[0]["data"]["code"], &errors[0]["session"]),
        (&json!("session_restart_loop"), &json!(LEAD1))
    );

    // Once: later ticks raise nothing more and start nothing.
    fx.engine_tick();
    fx.engine_tick();
    assert_eq!(session_errors(&fx).len(), 1);
    assert_eq!(window_names(&fx), ["main"]);
    assert_eq!(slot(&fx).session, LEAD2, "the chat slot is untouched");
}

#[test]
fn e2_restarts_a_lead_again_when_its_last_restart_is_ten_minutes_old() {
    let fx = Fx::with_ports();
    dying_leads(&fx);
    let old = fx.cpd(&["session", "open", LEAD1]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.cpd(&["session", "open", LEAD2]).ok();
    fx.pane_exit(&old, 1);
    fx.engine_tick();
    let restarted = show(&fx, LEAD1)["tmux_pane"].as_str().unwrap().to_owned();

    fx.env
        .sql("UPDATE sessions SET state = 'working' WHERE id = 'lead1'");
    fx.advance(Duration::from_secs(10 * 60));
    fx.pane_exit(&restarted, 1);
    fx.engine_tick();
    assert_restarted_parked(&fx, LEAD1, &restarted, "park-pay");
    assert_eq!(window_names(&fx), ["main", "park-pay"]);
    assert!(session_errors(&fx).is_empty(), "{:?}", session_errors(&fx));
    assert_eq!(slot(&fx).session, LEAD2, "the chat slot is untouched");
}

#[test]
fn open_leaves_one_window_for_a_lead_with_a_dead_pane() {
    let fx = Fx::with_ports();
    dying_leads(&fx);
    let lead1_pane = fx.cpd(&["session", "open", LEAD1]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.cpd(&["session", "open", LEAD2]).ok();
    assert_eq!(window_names(&fx), ["main", "park-pay"]);
    fx.pane_exit(&lead1_pane, 1);

    // No engine runs: `open` itself kills the dead pane before it makes a window.
    let out = fx.cpd(&["session", "open", LEAD1]).ok();
    assert_ne!(out["pane"], json!(lead1_pane));
    assert_eq!(out["displaced"], LEAD2);
    assert_eq!(window_names(&fx), ["main", "park-other"]);
    let lead1: Vec<PaneRow> = panes(&fx)
        .into_iter()
        .filter(|p| p.session == LEAD1)
        .collect();
    assert_eq!(lead1.len(), 1, "{lead1:?}");
    assert_eq!(lead1[0].window_name, "main");
}

#[test]
fn e2_replaces_a_dead_lead_in_the_chat_slot_by_the_placeholder() {
    let fx = Fx::with_ports();
    dying_leads(&fx);
    let lead1_pane = fx.cpd(&["session", "open", LEAD1]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.pane_exit(&lead1_pane, 1);
    assert!(fx.pane_dead(&lead1_pane));
    assert_eq!(slot(&fx).session, LEAD1);

    fx.engine_tick();
    let s = slot(&fx);
    assert!(s.session.is_empty(), "{s:?}");
    assert_eq!(
        (s.pane.as_str(), s.view.as_str(), s.window_name.as_str()),
        (lead1_pane.as_str(), "chat", "main")
    );
    assert!(!fx.pane_dead(&lead1_pane), "the placeholder runs");
    assert_placeholders(&fx, 1);
    // And E2 restarts the Lead in a parked window, never in the chat slot.
    assert_restarted_parked(&fx, LEAD1, &lead1_pane, "park-pay");
    assert_eq!(window_names(&fx), ["main", "park-pay"]);
    assert_eq!(slot(&fx).pane, lead1_pane);
}

#[test]
fn open_replaces_a_dead_lead_in_the_chat_slot() {
    let fx = Fx::with_ports();
    dying_leads(&fx);
    let lead1_pane = fx.cpd(&["session", "open", LEAD1]).ok()["pane"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.pane_exit(&lead1_pane, 1);

    // No engine runs, and the slot holds a dead pane of this very Lead: it is not "already" there.
    let out = fx.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!(out["displaced"], Value::Null);
    assert_ne!(out["pane"], json!(lead1_pane));
    assert_eq!(window_names(&fx), ["main"]);
    let lead1: Vec<PaneRow> = panes(&fx)
        .into_iter()
        .filter(|p| p.session == LEAD1)
        .collect();
    assert_eq!(lead1.len(), 1, "{lead1:?}");
    assert_eq!(slot(&fx).pane, out["pane"].as_str().unwrap());
}

// ---------- run ----------

#[test]
fn run_refusals() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    refused(&fx.cpd(&["session", "run", WORKER]), "not_a_lead", 3);
    refused(
        &fx.cpd(&["session", "run", "nobody"]),
        "session_not_found",
        4,
    );
    // Outside any pane of cpd's server.
    let error = refused(&fx.cpd(&["session", "run", LEAD1]), "wrong_pane", 3);
    assert_eq!(error["details"]["session"], LEAD1);
    assert!(fx.fakes.spawn.launches().is_empty(), "pi was started");

    // In a pane of cpd's server that runs another Session.
    fx.cockpit();
    let out = fx.tmux(&[
        "new-window",
        "-d",
        "-t",
        "=cpd:",
        "-P",
        "-F",
        "#{pane_id}",
        "cpd session run lead2",
    ]);
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    fx.tmux(&["set", "-p", "-t", &pane, "@cpd_session", LEAD2]);
    let server = format!(
        "{}/tmux-0/{},1,0",
        fx.env.tmux_tmpdir().display(),
        fx.env.tmux_socket()
    );
    let run = fx.cpd_with(
        fx.env.root(),
        &["session", "run", LEAD1],
        &[("TMUX", &server), ("TMUX_PANE", &pane)],
    );
    let error = refused(&run, "wrong_pane", 3);
    assert_eq!(
        (&error["details"]["pane"], &error["details"]["found"]),
        (&json!(pane), &json!(LEAD2))
    );
    assert!(fx.fakes.spawn.launches().is_empty(), "pi was started");
}

// ---------- run: the User's quit archives (commands.md §10.8) ----------

/// pi's session file for `id`, so `session run` resumes it; `finished` ends it on a finished turn.
fn pi_file(env: &TestEnv, id: &str, finished: bool) {
    let dir = env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&dir).unwrap();
    let turn = if finished {
        "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n"
    } else {
        ""
    };
    fs::write(
        dir.join(format!("2026_{id}.jsonl")),
        format!("{{\"type\":\"session\"}}\n{turn}"),
    )
    .unwrap();
}

/// The fixture of the quit tests: a side fork `side` of `lead1` and `side2` of `lead2`, both idle
/// with a session file, and the cockpit. The pi of a pane ends when the test says (`Running::pi_exits`), and exits 0
/// at `SIGTERM`, as pi 1.2.0 does (`interactive-mode.js`: `shutdown({fromSignal})` ends in `process.exit(0)`).
fn quit_fixture(fx: &Fx) {
    setup(&fx.env);
    fx.cockpit();
    fx.env.sql("INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, forked_from) VALUES
           ('side', 'lead', 'ctx', 'pay', 'Side', '/', 'O3', 'idle', '2020-01-01T00:00:00Z', 't', 'lead1'),
           ('side2', 'lead', 'ctx', 'other', 'Side two', '/', 'O3', 'idle', '2020-01-01T00:00:00Z', 't', 'lead2')",
    );
    for id in ["side", "side2", LEAD1, LEAD2] {
        pi_file(&fx.env, id, false);
    }
}

fn session_archived(fx: &Fx, id: &str) -> bool {
    show(fx, id)["archived_at"].is_string()
}

fn pane_exists(fx: &Fx, pane: &str) -> bool {
    panes(fx).iter().any(|p| p.pane == pane)
}

#[test]
fn quitting_pi_archives_a_side_fork_and_closes_its_pane() {
    let fx = Fx::with_ports();
    quit_fixture(&fx);
    let mut side = Running::start(&fx, "side");
    let pane = side.pane.pane.clone();
    side.pi_exits(&fx, 0);
    side.w.finish();
    assert!(session_archived(&fx, "side"));
    assert!(!pane_exists(&fx, &pane));
    let row = show(&fx, "side");
    assert_eq!(
        (&row["state"], &row["tmux_pane"], &row["place"]),
        (&json!("stopped"), &Value::Null, &json!("none"))
    );
    assert_eq!(events(&fx, "session-archived").len(), 1);
    // Nothing else was archived.
    for other in [LEAD1, LEAD2, "side2"] {
        assert!(!session_archived(&fx, other), "{other}");
    }
    assert_eq!(
        fx.env
            .query::<Option<String>>("SELECT archived_at FROM plans WHERE id = 'pay'"),
        None
    );
}

#[test]
fn quitting_a_plan_s_own_lead_archives_the_plan_with_its_fork() {
    let fx = Fx::with_ports();
    quit_fixture(&fx);
    let mut lead = Running::start(&fx, LEAD2);
    let fork = Running::start(&fx, "side2");
    let (lead_pane, fork_pane) = (lead.pane.pane.clone(), fork.pane.pane.clone());
    lead.pi_exits(&fx, 0);
    lead.w.finish();
    assert!(session_archived(&fx, LEAD2));
    assert!(!pane_exists(&fx, &lead_pane) && !pane_exists(&fx, &fork_pane));
    assert!(session_archived(&fx, "side2"));
    assert!(
        fx.env
            .query::<Option<String>>("SELECT archived_at FROM plans WHERE id = 'other'")
            .is_some()
    );
    assert_eq!(events(&fx, "plan-archived").len(), 1);
    assert_eq!(events(&fx, "session-archived").len(), 2);
    // Another Plan is untouched.
    assert!(!session_archived(&fx, LEAD1));
    assert_eq!(
        fx.env
            .query::<Option<String>>("SELECT archived_at FROM plans WHERE id = 'pay'"),
        None
    );
}

#[test]
fn quitting_the_lead_in_the_chat_slot_parks_it_first_and_the_placeholder_holds_the_slot() {
    let fx = Fx::with_ports();
    quit_fixture(&fx);
    fx.cpd(&["session", "open", "side"]).ok();
    let pane = slot(&fx).pane;
    assert_eq!(slot(&fx).session, "side");
    let mut side = Running::attach(&fx, &pane, "side");
    side.until_pis(1);
    side.pi_exits(&fx, 0);
    side.w.finish();
    assert!(session_archived(&fx, "side"));
    assert!(!pane_exists(&fx, &pane));
    let held = slot(&fx);
    assert_eq!(
        (held.session.as_str(), held.view.as_str()),
        ("", "chat"),
        "{held:?}"
    );
    assert_placeholders(&fx, 1);
    assert!(panes(&fx).iter().all(|p| p.session != "side"));
    assert_eq!(
        window_names(&fx),
        ["main"],
        "the Lead's park window went with its pane"
    );
    // Parked first: `session-moved` to `parked`, then to none.
    let places: Vec<Value> = events(&fx, "session-moved")
        .iter()
        .filter(|e| e["session"] == "side")
        .map(|e| e["data"]["place"].clone())
        .collect();
    assert_eq!(places, [json!("chat"), json!("parked"), json!("none")]);
}

#[test]
fn a_pi_that_exits_with_status_1_is_not_archived() {
    let fx = Fx::with_ports();
    quit_fixture(&fx);
    let mut side = Running::start(&fx, "side");
    side.pi_exits(&fx, 1);
    side.until_screen("pi exited. Press Enter to resume Side here.");
    assert!(!session_archived(&fx, "side"));
    assert_eq!(show(&fx, "side")["state"], "stopped");
    assert!(pane_exists(&fx, &side.pane.pane));
    assert_eq!(events(&fx, "session-archived").len(), 0);
}

#[test]
fn a_refused_archive_prints_the_refusal_and_offers_the_resume_in_place() {
    let fx = Fx::with_ports();
    quit_fixture(&fx);
    // `pay` has a running Job; its Lead holds the chat slot.
    fx.cpd(&["session", "open", LEAD1]).ok();
    let pane = slot(&fx).pane;
    let mut lead = Running::attach(&fx, &pane, LEAD1);
    lead.until_pis(1);
    lead.pi_exits(&fx, 0);
    lead.until_screen("pi exited. Press Enter to resume Lead one here.");
    let shown = lead.pane.screen();
    assert!(shown.contains("archive_blocked"), "{shown}");
    assert!(shown.contains("Job api of Plan pay is running."), "{shown}");
    // Nothing was archived, and the Lead was not parked.
    assert!(!session_archived(&fx, LEAD1));
    assert_eq!(
        fx.env
            .query::<Option<String>>("SELECT archived_at FROM plans WHERE id = 'pay'"),
        None
    );
    assert_eq!(
        events(&fx, "session-archived").len() + events(&fx, "plan-archived").len(),
        0
    );
    assert_eq!(
        (slot(&fx).pane, slot(&fx).session),
        (pane, LEAD1.to_owned())
    );
}

#[test]
fn a_reload_is_not_a_quit_even_though_pi_exits_0_on_sigterm() {
    let fx = Fx::with_ports();
    quit_fixture(&fx);
    // An idle Lead at a finished turn, parked: the restart for a new pi is due at once.
    pi_file(&fx.env, "side", true);
    fx.backdate(
        &fx.env.home().join(".pi/agent/sessions/g/2026_side.jsonl"),
        Duration::from_secs(3600),
    );
    fx.env
        .sql("UPDATE sessions SET state_at = '2020-01-01T00:00:00Z' WHERE id = 'side'");
    // pi's install, as the wrapper watches it (a cpd install does not restart pi, hs-d210).
    let current = fx.env.home().join(".pi/agent/install/current-version");
    fs::create_dir_all(current.parent().unwrap()).unwrap();
    fs::write(&current, "1.2.0\n").unwrap();
    let mut side = Running::start(&fx, "side");
    side.pane.spawn.exit_on_term(0);
    let (pane, old_pi) = (side.pane.pane.clone(), side.pane.spawn.pids()[0]);

    // A new pi, as pi's own update installs it: the wrapper stops pi (SIGTERM, which pi answers with exit 0) and
    // restarts itself, and the new process starts pi again.
    let next = current.with_extension("tmp");
    fs::write(&next, "1.3.0\n").unwrap();
    fs::rename(&next, &current).unwrap();
    side.w.finish();
    let restart = side.w.exec().clone();
    assert_eq!(
        side.pane.spawn.signals(),
        [(old_pi, cpd_core::host::spawn::Signal::Term)],
        "pi got SIGTERM once"
    );
    side.w = side.pane.resume(&restart);
    side.until_pis(2);
    assert!(pane_exists(&fx, &pane));
    assert!(!session_archived(&fx, "side"));
    assert_eq!(events(&fx, "session-archived").len(), 0);
}

// ---------- short ids (commands.md §1.3, hs-a3) ----------

const UUID_A: &str = "a1b2c3d4-0000-4000-8000-000000000001";
const UUID_B: &str = "a1b2c3d4-0000-4000-8000-000000000002";
const CAFE_LEAD: &str = "beef0000-0000-4000-8000-000000000003";
const CAFE_PREFIXED: &str = "cafe0001-0000-4000-8000-000000000004";

/// `setup` plus: Plan `cafe` (Lead `beef0000-…`), two Sessions of Plan `pay`
/// whose ids share the prefix `a1b2c3d4`, and a side Session of Plan `cafe` whose id
/// starts with `cafe`, which is also a Plan id.
fn setup_short(env: &TestEnv) {
    setup(env);
    env.sql(&format!(
            "INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('cafe', 'ctx', 'Cafe', 'grounding', '{CAFE_LEAD}', 't');
             INSERT INTO sessions (id, role, context, plan, name, forked_from, cwd, tier, state, state_at, created_at) VALUES
               ('{UUID_A}', 'lead', 'ctx', 'pay', 'A', '{LEAD1}', '/', 'O3', 'stopped', '2020-01-01T00:00:00Z', 't4'),
               ('{UUID_B}', 'lead', 'ctx', 'pay', 'B', '{LEAD1}', '/', 'O3', 'stopped', '2020-01-01T00:00:00Z', 't5'),
               ('{CAFE_LEAD}', 'lead', 'ctx', 'cafe', 'Cafe lead', NULL, '/', 'O3', 'stopped', '2020-01-01T00:00:00Z', 't6'),
               ('{CAFE_PREFIXED}', 'lead', 'ctx', 'cafe', 'Cafe prefixed', '{CAFE_LEAD}', '/', 'O3', 'stopped', '2020-01-01T00:00:00Z', 't7')"
        ),
    );
}

fn resolved(fx: &Fx, value: &str) -> String {
    fx.cpd(&["session", "show", value]).ok()["session"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn every_short_form_resolves_to_the_full_id() {
    let fx = Fx::new();
    setup_short(&fx.env);
    // 1: the full id.
    assert_eq!(resolved(&fx, UUID_A), UUID_A);
    assert_eq!(resolved(&fx, LEAD2), LEAD2);
    // 2: a Plan id is that Plan's Lead.
    assert_eq!(resolved(&fx, "pay"), LEAD1);
    assert_eq!(resolved(&fx, "other"), LEAD2);
    // 3: <plan>.<prefix> and <plan>·<prefix>, within the Plan.
    assert_eq!(
        resolved(&fx, "pay.a1b2c3d4-0000-4000-8000-000000000001"),
        UUID_A
    );
    assert_eq!(resolved(&fx, "pay·worker"), WORKER);
    assert_eq!(resolved(&fx, "pay.w"), WORKER);
    assert_eq!(resolved(&fx, "other.l"), LEAD2);
    // 4: a bare prefix of 4 or more characters, of any Plan or none.
    assert_eq!(resolved(&fx, "work"), WORKER);
    assert_eq!(
        resolved(&fx, "a1b2c3d4-0000-4000-8000-000000000002"),
        UUID_B
    );
    assert_eq!(resolved(&fx, "mana"), MANAGER);
}

#[test]
fn a_plan_id_wins_over_a_prefix_that_is_also_a_plan_id() {
    let fx = Fx::new();
    setup_short(&fx.env);
    // `cafe` is the id of a Plan and the 4-character prefix of another Session.
    assert_eq!(resolved(&fx, "cafe"), CAFE_LEAD);
    // The longer prefix is not a Plan id.
    assert_eq!(resolved(&fx, "cafe0"), CAFE_PREFIXED);
}

#[test]
fn an_ambiguous_prefix_is_refused_with_its_candidates() {
    let fx = Fx::new();
    setup_short(&fx.env);
    for value in ["a1b2", "a1b2c3d4", "pay.a1b2"] {
        let error = refused(&fx.cpd(&["session", "show", value]), "session_ambiguous", 3);
        assert_eq!(
            error["details"]["candidates"],
            json!([UUID_A, UUID_B]),
            "{value}"
        );
    }
    let error = refused(
        &fx.cpd(&["session", "show", "lead"]),
        "session_ambiguous",
        3,
    );
    assert_eq!(error["details"]["candidates"], json!([LEAD1, LEAD2]));
    // Every command that takes a Session refuses it; nothing is written.
    refused(
        &fx.cpd(&["session", "rename", "x", "--session", "a1b2"]),
        "session_ambiguous",
        3,
    );
    refused(
        &fx.cpd(&["session", "archive", "a1b2"]),
        "session_ambiguous",
        3,
    );
    refused(
        &fx.cpd(&["session", "open", "a1b2"]),
        "session_ambiguous",
        3,
    );
    refused(
        &fx.cpd(&["session", "send", "a1b2", "hi", "--unblocks", "u"]),
        "session_ambiguous",
        3,
    );
    assert_eq!(events(&fx, "session-renamed").len(), 0);
    assert_eq!(events(&fx, "session-message").len(), 0);
}

#[test]
fn a_prefix_of_fewer_than_4_characters_is_not_accepted() {
    let fx = Fx::new();
    setup_short(&fx.env);
    for value in ["wor", "a1b", "man", "w"] {
        refused(&fx.cpd(&["session", "show", value]), "session_not_found", 4);
    }
    // A Plan id of 2 or 3 characters still resolves, and so does a scoped prefix of 1.
    assert_eq!(resolved(&fx, "pay"), LEAD1);
    assert_eq!(resolved(&fx, "pay.w"), WORKER);
}

#[test]
fn unmatched_values_are_session_not_found_and_malformed_ones_usage_errors() {
    let fx = Fx::new();
    setup_short(&fx.env);
    for value in [
        "nobody",
        "Lead1",
        "WORK",
        "pay.zzz",
        "other.worker",
        "nosuch.lead",
        "pay·zzz",
    ] {
        let error = refused(&fx.cpd(&["session", "show", value]), "session_not_found", 4);
        // The reference is echoed in its canonical `.` form.
        assert_eq!(error["details"]["session"], value.replace('·', "."));
    }
    for value in ["pay.", "pay·", ".lead", "Pay.lead", "pay.-a", "a/b"] {
        refused(
            &fx.cpd(&["session", "show", value]),
            "invalid_session_id",
            2,
        );
    }
}

#[test]
fn every_session_command_accepts_the_short_forms() {
    let fx = Fx::with_ports();
    setup_short(&fx.env);
    fx.cockpit();
    // `--session` and `CPD_SESSION`.
    let out = fx
        .cpd(&["session", "rename", "renamed", "--session", "pay"])
        .ok();
    assert_eq!(
        out,
        json!({"ok": true, "session": LEAD1, "name": "renamed"})
    );
    let out = fx
        .cpd_with(
            fx.env.root(),
            &["session", "rename", "again"],
            &[("CPD_SESSION", "other")],
        )
        .ok();
    assert_eq!(out["session"], LEAD2);
    let out = fx
        .cpd_with(
            fx.env.root(),
            &["session", "show"],
            &[("CPD_SESSION", "other")],
        )
        .ok();
    assert_eq!(out["session"]["id"], LEAD2);
    // send, to a Lead by its Plan id and to the Manager by a bare prefix.
    assert_eq!(
        fx.cpd(&["session", "send", "pay", "hi", "--unblocks", "u"])
            .ok()["session"],
        LEAD1
    );
    assert_eq!(
        fx.cpd(&["session", "send", "mana", "hi", "--unblocks", "u"])
            .ok()["session"],
        MANAGER
    );
    // close (no pane: nothing to do), archive and unarchive.
    assert_eq!(
        fx.cpd(&["session", "close", "other"]).ok(),
        json!({"ok": true, "session": LEAD2, "pane": null, "already": true})
    );
    let out = fx
        .cpd(&[
            "session",
            "archive",
            "pay.a1b2c3d4-0000-4000-8000-000000000001",
        ])
        .ok();
    assert_eq!(out["archived"], json!([{"kind": "session", "id": UUID_A}]));
    assert!(archived_at(&fx, UUID_A).is_string());
    // An archived Session can be named by a bare prefix to unarchive it.
    let out = fx
        .cpd(&[
            "session",
            "unarchive",
            "pay.a1b2c3d4-0000-4000-8000-000000000001",
        ])
        .ok();
    assert_eq!(out["archived"], json!([{"kind": "session", "id": UUID_A}]));
    // open and park name a Session that does not exist.
    refused(
        &fx.cpd(&["session", "open", "nobody"]),
        "session_not_found",
        4,
    );
    refused(
        &fx.cpd(&["session", "fork", "nobody"]),
        "session_not_found",
        4,
    );
    refused(
        &fx.cpd(&["session", "park", "nobody"]),
        "session_not_found",
        4,
    );
    refused(
        &fx.cpd(&["session", "history", "nobody"]),
        "session_not_found",
        4,
    );
}
