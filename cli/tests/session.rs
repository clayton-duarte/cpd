//! Sessions (contract §3.6, §6.4, §6.7; commands.md §10 with §19 amendments
//! 1–3), run as the built binary against a private tmux server, with fake
//! `pi` and `cpd` scripts on `PATH`.

mod common;

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use common::{CPD, Run, SLEEP_SECONDS, TestEnv, backdate, refused, wait_until};
use cpd_core::error::CpdError;
use cpd_core::events::{Kind, NewEvent, append};
use cpd_core::home::CpdHome;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

const LEAD1: &str = "lead1";
const LEAD2: &str = "lead2";
const MANAGER: &str = "manager1";
const WORKER: &str = "worker1";

fn home(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

fn store(env: &TestEnv) -> Store {
    Store::open(&home(env), 5000).unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn sid(id: &str) -> SessionId {
    SessionId::parse(id).unwrap()
}

/// Context `ctx` with its Manager `manager1`; Plans `pay` (Lead `lead1`, idle;
/// Job `api` with Attempt `api-a1`, Worker `worker1` and an open Escalation)
/// and `other` (Lead `lead2`, stopped). Every Session's cwd is the env's root.
fn setup(env: &TestEnv) {
    env.write_tmux_config("[timeouts]\nfork_quiet_seconds = 1\n"); // a 1 s quiet time for forks
    let cwd = env.root().display().to_string();
    let mut store = store(env);
    store
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                     INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at) VALUES
                       ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead one', '{cwd}', 'O3', 'idle', 't1', 't1'),
                       ('lead2', 'lead', 'ctx', 'other', NULL, 'Lead two', '{cwd}', 'O3', 'stopped', '2020-01-01T00:00:00Z', 't2'),
                       ('manager1', 'manager', 'ctx', NULL, NULL, 'Manager', '{cwd}', 'O5', 'stopped', '2020-01-01T00:00:00Z', 't0'),
                       ('worker1', 'worker', 'ctx', 'pay', 'api-a1', 'pay api a1', '{cwd}', 'S2', 'working', 't3', 't3');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
                       ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't'),
                       ('other', 'ctx', 'Other', 'grounding', 'lead2', 't');
                     INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at) VALUES
                       ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay', {id}, 't'),
                       ('other', 'r', '/r', 'main', 'waiting', 'cpd/other', {id}, 't');
                     INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at)
                       VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', {id}, 't');
                     INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
                       VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 'worker1', 't');
                     INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
                       VALUES ('pay', 'api-a1-q1', 'api-a1', 1, 'Which?', 'open', 1, 't');",
                    fp = "a".repeat(64),
                    id = tx.id()
                ),
            )?;
            append(
                tx,
                NewEvent::new(
                    Kind::SessionRegistered,
                    "Lead one registered.",
                    json!({"role": "lead", "name": "Lead one", "forked_from": null, "tier": "O3"}),
                )
                .plan(PlanId::parse("pay").unwrap())
                .session(sid(LEAD1)),
            )?;
            Ok(())
        })
        .unwrap();
    for plan in ["pay", "other"] {
        let path = home(env).lead_message(&PlanId::parse(plan).unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("Plan {plan}.")).unwrap();
    }
}

/// Runs `cpd <args>` with extra environment variables.
fn cpd_with(env: &TestEnv, vars: &[(&str, &str)], args: &[&str]) -> Run {
    let mut cmd = env.command(CPD);
    for (k, v) in vars {
        cmd.env(k, v);
    }
    let out = cmd.args(args).output().unwrap();
    Run {
        exit: out.status.code().unwrap(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn exec_sql(env: &TestEnv, sql: &str) {
    store(env).write("test", |tx| exec(tx, sql)).unwrap();
}

fn query<T: rusqlite::types::FromSql>(env: &TestEnv, sql: &str) -> T {
    store(env).read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

fn show(env: &TestEnv, id: &str) -> Value {
    env.cpd(&["session", "show", id]).ok()["session"].clone()
}

// ---------- tmux ----------

/// A fake `pi` that creates its session file (when given `--session-id`) and
/// then runs `then`.
fn fake_pi(env: &TestEnv, then: &str) {
    env.fake_pi(&format!(
        "id=''; prev=''\nfor a in \"$@\"; do [ \"$prev\" = --session-id ] && id=$a; prev=$a; done\n\
         if [ -n \"$id\" ]; then mkdir -p \"$HOME/.pi/agent/sessions/g\"; \
         printf '{{\"type\":\"session\"}}\\n' > \"$HOME/.pi/agent/sessions/g/2026_$id.jsonl\"; fi\n{then}"
    ));
}

/// The cockpit: session `cpd`, window `main` with a tree pane and the
/// placeholder in the chat slot.
fn cockpit(env: &TestEnv) {
    let ok = |args: &[&str]| {
        let out = env.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let tree = ok(&["new-session", "-d", "-s", "cpd", "-n", "main", "-x", "215", "-y", "62", "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    ok(&["set", "-p", "-t", &tree, "@cpd_view", "tree"]);
    let chat = ok(&["split-window", "-d", "-t", &tree, "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    ok(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
}

#[derive(Debug, Clone, PartialEq)]
struct P {
    pane: String,
    window: String,
    window_name: String,
    session: String,
    view: String,
    history: String,
}

fn panes(env: &TestEnv) -> Vec<P> {
    let out = env.tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{window_id}\t#{window_name}\t#{@cpd_session}\t#{@cpd_view}\t#{@cpd_history}"]);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            P { pane: f[0].into(), window: f[1].into(), window_name: f[2].into(), session: f[3].into(), view: f[4].into(), history: f[5].into() }
        })
        .collect()
}

/// The chat slot: the pane of `main` with a Session, the placeholder or a history pane.
fn slot(env: &TestEnv) -> P {
    let found: Vec<P> =
        panes(env).into_iter().filter(|p| p.window_name == "main" && (!p.session.is_empty() || p.view == "chat" || p.view == "history")).collect();
    assert_eq!(found.len(), 1, "one chat slot: {found:?}");
    found[0].clone()
}

/// At most one placeholder exists, and only in the slot (amendment 2).
fn assert_placeholders(env: &TestEnv, expected: usize) {
    let placeholders: Vec<P> = panes(env).into_iter().filter(|p| p.view == "chat").collect();
    assert_eq!(placeholders.len(), expected, "{placeholders:?}");
    for p in placeholders {
        assert_eq!(p.window_name, "main", "a placeholder outside the slot: {p:?}");
    }
}

fn window_names(env: &TestEnv) -> Vec<String> {
    let out = env.tmux(&["list-windows", "-t", "=cpd", "-F", "#{window_name}"]);
    let mut names: Vec<String> = String::from_utf8(out.stdout).unwrap().lines().map(str::to_owned).collect();
    names.sort();
    names
}

fn pi_calls_with(env: &TestEnv, flag: &str) -> Vec<Vec<String>> {
    env.calls("pi").into_iter().filter(|c| c.iter().any(|a| a == flag)).collect()
}

// ---------- list, show ----------

#[test]
fn list_filters_and_derives_place_and_leads() {
    let env = TestEnv::new();
    setup(&env);
    let ids = |run: Run| -> Vec<String> {
        run.ok()["sessions"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_owned()).collect()
    };

    let all = env.cpd(&["session", "list"]).ok();
    assert_eq!(all["events_cursor"], 1);
    let sessions = all["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 4);
    let lead1 = sessions.iter().find(|s| s["id"] == LEAD1).unwrap();
    assert_eq!(
        *lead1,
        json!({"id": "lead1", "role": "lead", "context": "ctx", "plan": "pay", "attempt": null, "name": "Lead one",
               "state": "idle", "state_label": null, "state_at": "t1", "archived_at": null, "place": "none", "tmux_pane": null, "file": null,
               "forked_from": null, "plan_lead": true, "tier": "O3",
               "cwd": env.root().display().to_string()})
    );
    let manager = sessions.iter().find(|s| s["id"] == MANAGER).unwrap();
    assert_eq!(
        (&manager["role"], &manager["plan"], &manager["attempt"], &manager["plan_lead"], &manager["tier"]),
        (&json!("manager"), &Value::Null, &Value::Null, &json!(false), &json!("O5"))
    );
    assert!(lead1.get("grounded_lead").is_none(), "the grounded Lead is retired");

    assert_eq!(ids(env.cpd(&["session", "list", "--plan", "pay"])), [LEAD1, WORKER]);
    assert_eq!(ids(env.cpd(&["session", "list", "--context", "ctx", "--role", "worker"])), [WORKER]);
    assert_eq!(ids(env.cpd(&["session", "list", "--state", "idle", "--state", "working"])), [LEAD1, WORKER]);
    assert_eq!(ids(env.cpd(&["session", "list", "--role", "lead", "--state", "stopped"])), [LEAD2]);
    assert_eq!(ids(env.cpd(&["session", "list", "--role", "manager"])), [MANAGER]);
    assert!(ids(env.cpd(&["session", "list", "--context", "elsewhere"])).is_empty());
    // Filters only: CPD_PLAN does not select anything.
    assert_eq!(cpd_with(&env, &[("CPD_PLAN", "pay")], &["session", "list"]).ok()["sessions"].as_array().unwrap().len(), 4);

    refused(&env.cpd(&["session", "list", "--role", "reviewer"]), "usage", 2);
    refused(&env.cpd(&["session", "list", "--state", "busy"]), "usage", 2);
    refused(&env.cpd(&["session", "list", "--plan", "Bad"]), "invalid_plan_id", 2);
}

/// sql-F5, commands-F4: the Plan-Lead lookup is a join: a Session that leads two
/// Plans is still one row, and a Session that leads none is not a Plan's Lead.
#[test]
fn list_gives_a_session_leading_two_plans_one_row() {
    let env = TestEnv::new();
    setup(&env);
    exec_sql(
        &env,
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('again', 'ctx', 'Again', 'grounding', 'lead1', 't')",
    );
    let sessions = env.cpd(&["session", "list"]).ok()["sessions"].clone();
    let sessions = sessions.as_array().unwrap();
    let leads: Vec<(&str, bool)> =
        sessions.iter().map(|s| (s["id"].as_str().unwrap(), s["plan_lead"].as_bool().unwrap())).collect();
    assert_eq!(leads, [(MANAGER, false), (LEAD1, true), (LEAD2, true), (WORKER, false)]);
}

#[test]
fn show_adds_created_at_registered_event_and_open_escalations() {
    let env = TestEnv::new();
    setup(&env);
    let out = env.cpd(&["session", "show", LEAD1]).ok();
    assert_eq!(out["session"]["created_at"], "t1");
    assert_eq!(out["session"]["registered_event"], 1);
    assert_eq!(out["session"]["plan_lead"], true);
    assert_eq!(out["open_escalations"], json!(["api-a1-q1"]));

    let worker = cpd_with(&env, &[("CPD_SESSION", WORKER)], &["session", "show"]).ok();
    assert_eq!(worker["session"]["id"], WORKER);
    assert_eq!(worker["session"]["attempt"], "api-a1");
    assert_eq!(worker["session"]["registered_event"], Value::Null);
    assert!(worker.get("open_escalations").is_none(), "only a Plan's Lead has them");

    refused(&env.cpd(&["session", "show"]), "session_required", 2);
    let error = refused(&env.cpd(&["session", "show", "nobody"]), "session_not_found", 4);
    assert_eq!(error["details"]["session"], "nobody");
    refused(&env.cpd(&["session", "show", "bad id"]), "invalid_session_id", 2);
}

// ---------- state, rename ----------

#[test]
fn state_follows_section_3_6() {
    let env = TestEnv::new();
    setup(&env);
    let state = |args: &[&str]| {
        let mut argv = vec!["session", "state"];
        argv.extend(args);
        argv.extend(["--session", LEAD1]);
        env.cpd(&argv)
    };

    let out = state(&["working"]).ok();
    assert_eq!(out, json!({"ok": true, "session": "lead1", "state": "working", "label": null, "from": "idle"}));
    let out = state(&["blocked", "--label", "cpd: Worker question"]).ok();
    assert_eq!((&out["from"], &out["label"]), (&json!("working"), &json!("cpd: Worker question")));
    assert_eq!(show(&env, LEAD1)["state_label"], "cpd: Worker question");

    // Unchanged: nothing written, no Event.
    let before = events(&env, "session-state").len();
    let txns: i64 = query(&env, "SELECT count(*) FROM txns");
    assert_eq!(state(&["blocked", "--label", "cpd: Worker question"]).ok()["already"], true);
    assert_eq!(events(&env, "session-state").len(), before);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM txns"), txns);

    // A new label is a change.
    assert_eq!(state(&["blocked", "--label", "cpd: plan review"]).ok()["from"], "blocked");
    assert_eq!(state(&["idle"]).ok()["from"], "blocked");
    assert_eq!(state(&["stopped"]).ok()["from"], "idle");
    let error = refused(&state(&["working"]), "illegal_transition", 3);
    assert_eq!((&error["details"]["from"], &error["details"]["to"]), (&json!("stopped"), &json!("working")));
    refused(&state(&["blocked", "--label", "x"]), "illegal_transition", 3);
    assert_eq!(state(&["idle"]).ok()["from"], "stopped");

    let all = events(&env, "session-state");
    assert_eq!(all.len(), 6);
    let first = &all[0];
    assert_eq!((&first["session"], &first["plan"], &first["context"]), (&json!("lead1"), &json!("pay"), &json!("ctx")));
    assert_eq!(first["data"], json!({"from": "idle", "to": "working", "label": null}));
    assert_eq!(all[1]["data"], json!({"from": "working", "to": "blocked", "label": "cpd: Worker question"}));
    assert_eq!(show(&env, LEAD1)["state_label"], Value::Null);

    // A Manager has no Plan: its Event has none either.
    cpd_with(&env, &[("CPD_SESSION", MANAGER)], &["session", "state", "idle"]).ok();
    assert_eq!(events(&env, "session-state").last().unwrap()["plan"], Value::Null);

    refused(&state(&["blocked"]), "usage", 2);
    refused(&state(&["idle", "--label", "x"]), "usage", 2);
    refused(&state(&["blocked", "--label", &"x".repeat(81)]), "usage", 2);
    refused(&state(&["asleep"]), "usage", 2);
    refused(&env.cpd(&["session", "state", "idle"]), "session_required", 2);
    refused(&env.cpd(&["session", "state", "idle", "--session", "nobody"]), "session_not_found", 4);
}

#[test]
fn state_idle_reply_writes_data_reply_and_only_idle_accepts_it() {
    let env = TestEnv::new();
    setup(&env);
    let state = |args: &[&str]| {
        let mut argv = vec!["session", "state"];
        argv.extend(args);
        argv.extend(["--session", LEAD1]);
        env.cpd(&argv)
    };

    // Only `idle` takes `--reply`; a refusal writes nothing.
    let txns: i64 = query(&env, "SELECT count(*) FROM txns");
    for other in [&["working", "--reply"][..], &["blocked", "--label", "x", "--reply"], &["stopped", "--reply"]] {
        refused(&state(other), "usage", 2);
    }
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM txns"), txns);

    state(&["working"]).ok();
    state(&["idle", "--reply"]).ok();
    let all = events(&env, "session-state");
    assert_eq!(all.last().unwrap()["data"], json!({"from": "working", "to": "idle", "label": null, "reply": true}));

    // Without `--reply` the Event has no `reply`; an unchanged state writes nothing whatever `--reply`.
    state(&["working"]).ok();
    state(&["idle"]).ok();
    assert_eq!(events(&env, "session-state").last().unwrap()["data"], json!({"from": "working", "to": "idle", "label": null}));
    let (events_before, txns) = (events(&env, "session-state").len(), query::<i64>(&env, "SELECT count(*) FROM txns"));
    assert_eq!(state(&["idle", "--reply"]).ok()["already"], true);
    assert_eq!(events(&env, "session-state").len(), events_before);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM txns"), txns);
}

#[test]
fn rename_appends_session_renamed_once() {
    let env = TestEnv::new();
    setup(&env);
    let out = env.cpd(&["session", "rename", "cpd rust lead", "--session", LEAD1]).ok();
    assert_eq!(out, json!({"ok": true, "session": "lead1", "name": "cpd rust lead"}));
    assert_eq!(show(&env, LEAD1)["name"], "cpd rust lead");
    let again = cpd_with(&env, &[("CPD_SESSION", LEAD1)], &["session", "rename", "cpd rust lead"]).ok();
    assert_eq!(again["already"], true);
    let renamed = events(&env, "session-renamed");
    assert_eq!(renamed.len(), 1);
    assert_eq!((&renamed[0]["session"], &renamed[0]["data"]), (&json!("lead1"), &json!({"name": "cpd rust lead"})));

    refused(&env.cpd(&["session", "rename", "", "--session", LEAD1]), "invalid_name", 2);
    refused(&env.cpd(&["session", "rename", "a\tb", "--session", LEAD1]), "invalid_name", 2);
    refused(&env.cpd(&["session", "rename", "x"]), "session_required", 2);
    refused(&env.cpd(&["session", "rename", "x", "--session", "nobody"]), "session_not_found", 4);
}

// ---------- fork ----------

#[test]
fn fork_refusals_register_nothing() {
    let env = TestEnv::new();
    setup(&env);
    let message = env.root().join("message.md");
    fs::write(&message, "Hello.").unwrap();
    let message = message.to_str().unwrap();

    let error = refused(&env.cpd(&["session", "fork", WORKER]), "not_a_lead", 3);
    assert_eq!(error["details"]["role"], "worker");
    let error = refused(&env.cpd(&["session", "fork", MANAGER]), "not_a_lead", 3);
    assert_eq!(error["details"]["role"], "manager");
    refused(&env.cpd(&["session", "fork", "nobody"]), "session_not_found", 4);
    refused(&env.cpd(&["session", "fork", LEAD1, "--plan", "nope"]), "plan_not_found", 4);
    refused(&env.cpd(&["session", "fork", LEAD1, "--message-file", "/nonexistent/m.md"]), "file_unreadable", 2);
    refused(&env.cpd(&["session", "fork", LEAD1, "--name", ""]), "invalid_name", 2);
    // No tmux server, so no cockpit.
    let error = refused(&env.cpd(&["session", "fork", LEAD1, "--message-file", message]), "no_cockpit", 3);
    assert_eq!(error["details"]["next"], "cpd tui");
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM sessions"), 4);
}

#[test]
fn fork_registers_a_stopped_lead_in_a_parked_window() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    let message = env.root().join("message.md");
    fs::write(&message, "Look at the tests.").unwrap();

    let out = env.cpd(&["session", "fork", LEAD2, "--plan", "pay", "--message-file", message.to_str().unwrap()]).ok();
    let new = out["session"]["id"].as_str().unwrap().to_owned();
    assert!(SessionId::parse(&new).is_ok() && new.len() == 36, "a UUID: {new}");
    assert_eq!(out["session"]["name"], "Lead two (fork)");
    assert_eq!((&out["session"]["plan"], &out["session"]["forked_from"]), (&json!("pay"), &json!(LEAD2)));

    let row = show(&env, &new);
    assert_eq!(
        (&row["role"], &row["context"], &row["state"], &row["tier"], &row["forked_from"], &row["place"]),
        (&json!("lead"), &json!("ctx"), &json!("stopped"), &json!("O3"), &json!(LEAD2), &json!("parked"))
    );
    assert_eq!(row["cwd"], env.root().display().to_string());
    assert_eq!(row["tmux_pane"], out["session"]["pane"]);
    assert_eq!(row["plan_lead"], false, "a side session: the Plan's Lead is unchanged");
    let copied = home(&env).session_message(&PlanId::parse("pay").unwrap(), &sid(&new));
    assert_eq!(fs::read_to_string(copied).unwrap(), "Look at the tests.");

    let pane = panes(&env).into_iter().find(|p| p.pane == out["session"]["pane"]).unwrap();
    assert_eq!((pane.window.as_str(), pane.window_name.as_str(), pane.session.as_str()), (out["session"]["window"].as_str().unwrap(), "park-pay", new.as_str()));
    assert!(env.calls("cpd").contains(&vec!["session".to_owned(), "run".to_owned(), new.clone()]));

    let registered = events(&env, "session-registered");
    let registered = registered.last().unwrap();
    assert_eq!((&registered["session"], &registered["plan"]), (&json!(new), &json!("pay")));
    assert_eq!(registered["data"], json!({"role": "lead", "name": "Lead two (fork)", "forked_from": LEAD2, "tier": "O3"}));
    let moved = events(&env, "session-moved");
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0]["data"], json!({"pane": out["session"]["pane"], "window": out["session"]["window"], "place": "parked"}));

    // The default Plan is the source's; each call registers a new Session.
    let again = env.cpd(&["session", "fork", LEAD1]).ok();
    assert_eq!((&again["session"]["plan"], &again["session"]["name"]), (&json!("pay"), &json!("Lead one (fork)")));
    assert_ne!(again["session"]["id"], json!(new));
}

/// G11: a `tmux` that refuses `set -p … @cpd_session` and passes everything else to the real one.
fn tmux_refusing_the_session_mark(env: &TestEnv) {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let real = std::env::split_paths(&path).map(|d| d.join("tmux")).find(|p| p.is_file()).expect("tmux on PATH");
    env.fake("tmux", &format!("case \" $* \" in *\" set \"*\" @cpd_session \"*) echo refused >&2; exit 1;; esac\nexec '{}' \"$@\"", real.display()));
}

/// G11: a step after `new-window` fails: the new window is killed and the original error returned.
#[test]
fn fork_and_open_leave_no_window_when_a_step_after_new_window_fails() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    tmux_refusing_the_session_mark(&env);
    let before = window_names(&env);
    let panes_before = panes(&env);

    let run = env.cpd(&["session", "fork", LEAD1, "--plan", "pay"]);
    assert_eq!(run.error()["code"], "tmux_failed", "{run:?}");
    assert_eq!(window_names(&env), before, "fork left a window");

    let run = env.cpd(&["session", "open", LEAD1]);
    assert_eq!(run.error()["code"], "tmux_failed", "{run:?}");
    assert_eq!(window_names(&env), before, "open left a window");
    assert_eq!(panes(&env), panes_before);
}

/// Fork when idle (§6.7): the fork waits while the source has no file, then
/// while it is busy, and forks once it is idle; pi exiting offers a resume.
#[test]
fn fork_waits_until_the_source_is_idle_then_forks_and_resumes() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, "exit 1");
    cockpit(&env);
    env.cpd(&["session", "state", "working", "--session", LEAD1]).ok();
    let message = env.root().join("message.md");
    fs::write(&message, "Review the tests.").unwrap();

    let out = env.cpd(&["session", "fork", LEAD1, "--name", "side", "--message-file", message.to_str().unwrap()]).ok();
    let new = out["session"]["id"].as_str().unwrap().to_owned();
    let pane = out["session"]["pane"].as_str().unwrap().to_owned();
    let capture = || String::from_utf8(env.tmux(&["capture-pane", "-p", "-t", &pane]).stdout).unwrap();
    wait_until("the waiting line", || capture().contains("Waiting for the source Lead to finish its turn, then forking it as \"side\""));

    // No file yet, and busy.
    // time-claim: the fork does not happen while the source is busy and has no file, over a whole 2 s poll of the
    // source; no per-poll signal from `session run` (it only reads the store and the file).
    std::thread::sleep(Duration::from_secs(3));
    assert!(env.calls("pi").is_empty());
    // A file with a finished turn, but still busy.
    let sessions = env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    let source_file: PathBuf = sessions.join("2026_lead1.jsonl");
    fs::write(
        &source_file,
        "{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n",
    )
    .unwrap();
    // Quiet for long enough and finished: only the busy source is in the way.
    backdate(&source_file, Duration::from_secs(3600));
    // time-claim: the fork does not happen while the source is busy, over a whole 2 s poll of the source;
    // no per-poll signal from `session run`.
    std::thread::sleep(Duration::from_secs(3));
    assert!(env.calls("pi").is_empty(), "the source is still working");

    env.cpd(&["session", "state", "idle", "--session", LEAD1]).ok();
    wait_until("the fork", || !pi_calls_with(&env, "--fork").is_empty());
    let fork = &pi_calls_with(&env, "--fork")[0];
    let after = |flag: &str| fork[fork.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(after("--fork"), source_file.display().to_string());
    assert_eq!(after("--session-id"), new);
    assert_eq!(after("-n"), "side");
    assert_eq!(after("--model"), "github-copilot/claude-opus-5.5:high");
    let copied = home(&env).session_message(&PlanId::parse("pay").unwrap(), &sid(&new));
    assert_eq!(fork[fork.len() - 2..], ["--".to_owned(), format!("@{}", copied.display())]);

    // pi created the fork's file: it is recorded; pi exited: the pane offers a resume.
    let fork_file = sessions.join(format!("2026_{new}.jsonl")).display().to_string();
    wait_until("the recorded file", || show(&env, &new)["file"] == json!(fork_file));
    wait_until("the resume prompt", || capture().contains("pi exited. Press Enter to resume side here."));
    assert_eq!(show(&env, &new)["state"], "stopped");
    env.tmux(&["send-keys", "-t", &pane, "Enter"]);
    wait_until("the resume", || !pi_calls_with(&env, "--session").is_empty());
    let resume = &pi_calls_with(&env, "--session")[0];
    assert_eq!(resume[..2], ["--session".to_owned(), fork_file]);
    assert!(!resume.iter().any(|a| a == "--fork" || a == "--"), "{resume:?}");
}

/// A fork whose source is `stopped` with no session file can never see it idle: it says so,
/// offers Enter to retry, and forks once the source has a finished turn (hs-d196).
#[test]
fn fork_stops_waiting_for_a_stopped_source_without_a_file_and_retries_on_enter() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, "exit 1");
    cockpit(&env);
    let message = env.root().join("message.md");
    fs::write(&message, "Review the tests.").unwrap();

    let out = env.cpd(&["session", "fork", LEAD2, "--name", "side", "--message-file", message.to_str().unwrap()]).ok();
    let pane = out["session"]["pane"].as_str().unwrap().to_owned();
    let capture = || String::from_utf8(env.tmux(&["capture-pane", "-p", "-t", &pane]).stdout).unwrap();
    wait_until("the refusal", || capture().contains("Press Enter to retry."));
    assert!(capture().contains("is stopped and has no session file"), "{}", capture());
    assert!(env.calls("pi").is_empty());

    // The source gets a file with a finished turn; Enter retries and forks.
    let sessions = env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    let source_file: PathBuf = sessions.join("2026_lead2.jsonl");
    fs::write(
        &source_file,
        "{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n",
    )
    .unwrap();
    backdate(&source_file, Duration::from_secs(3600));
    env.tmux(&["send-keys", "-t", &pane, "Enter"]);
    wait_until("the fork", || !pi_calls_with(&env, "--fork").is_empty());
    let fork = &pi_calls_with(&env, "--fork")[0];
    assert_eq!(fork[fork.iter().position(|a| a == "--fork").unwrap() + 1], source_file.display().to_string());
}

/// A fake pi body that copies the file named by its last argument (`@<path>`) to `$CPD_HOME/read.copy`.
/// The copy appears whole or not at all (cp to a temp name, then rename): a test waits for `read.copy` to exist.
fn copy_last_file() -> String {
    format!("for a in \"$@\"; do last=$a; done\ncp \"${{last#@}}\" \"$CPD_HOME/read.tmp\"\nmv \"$CPD_HOME/read.tmp\" \"$CPD_HOME/read.copy\"\nexec sleep {SLEEP_SECONDS}")
}

/// Bigger than one argv element may be (MAX_ARG_STRLEN, 131072 bytes; hs-n111).
fn big_message() -> String {
    "A line of a first message that is longer than one argument may be.\n".repeat(140_000 / 66 + 1)
}

/// A Lead's first message goes to pi as `@<absolute path>`, so one over 128 KiB starts pi.
#[test]
fn a_fresh_lead_with_a_first_message_over_128_kib_starts_pi_with_its_file() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &copy_last_file());
    cockpit(&env);
    let path = home(&env).lead_message(&PlanId::parse("pay").unwrap());
    let message = big_message();
    assert!(message.len() > 131_072);
    fs::write(&path, &message).unwrap();

    env.cpd(&["session", "open", LEAD1]).ok();
    wait_until("lead1's pi", || !env.calls("pi").is_empty());
    let call = &env.calls("pi")[0];
    assert_eq!(call[call.len() - 2..], ["--".to_owned(), format!("@{}", path.display())]);
    wait_until("the copy", || env.cpd_home().join("read.copy").exists());
    assert!(fs::read_to_string(env.cpd_home().join("read.copy")).unwrap() == message, "pi read the whole file");
}

/// The same for a fork with `--message-file` (`run/<plan>/messages/<session>.md`).
#[test]
fn a_fork_with_a_message_file_over_128_kib_starts_pi_with_the_copied_file() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &copy_last_file());
    cockpit(&env);
    pi_file(&env, LEAD2, true);
    let message = big_message();
    let file = env.root().join("message.md");
    fs::write(&file, &message).unwrap();

    let out = env.cpd(&["session", "fork", LEAD2, "--plan", "pay", "--message-file", file.to_str().unwrap()]).ok();
    let new = out["session"]["id"].as_str().unwrap().to_owned();
    let copied = home(&env).session_message(&PlanId::parse("pay").unwrap(), &sid(&new));
    wait_until("the fork", || !pi_calls_with(&env, "--fork").is_empty());
    let call = &pi_calls_with(&env, "--fork")[0];
    assert_eq!(call[call.len() - 2..], ["--".to_owned(), format!("@{}", copied.display())]);
    wait_until("the copy", || env.cpd_home().join("read.copy").exists());
    assert!(fs::read_to_string(env.cpd_home().join("read.copy")).unwrap() == message, "pi read the whole file");
}

// ---------- open, park ----------

#[test]
fn open_and_park_keep_at_most_one_placeholder_only_in_the_slot() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    let placeholder = slot(&env);
    assert_placeholders(&env, 1);

    // A stopped Lead with no pane: started in a park window, swapped in; the placeholder dies with that window.
    let out = env.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!(out["displaced"], Value::Null);
    let lead1_pane = out["pane"].as_str().unwrap().to_owned();
    let s = slot(&env);
    assert_eq!((s.pane.as_str(), s.session.as_str(), s.window.as_str()), (lead1_pane.as_str(), LEAD1, placeholder.window.as_str()));
    assert_placeholders(&env, 0);
    assert_eq!(window_names(&env), ["main"]);
    assert_eq!(show(&env, LEAD1)["place"], "chat");
    assert_eq!(show(&env, LEAD1)["tmux_pane"], json!(lead1_pane));
    wait_until("lead1's pi", || env.calls("pi").iter().any(|c| c.contains(&LEAD1.to_owned())));
    let message = home(&env).lead_message(&PlanId::parse("pay").unwrap());
    assert!(env.calls("pi")[0].ends_with(&["--".to_owned(), format!("@{}", message.display())]), "a fresh Plan Lead gets lead-message.md");

    assert_eq!(env.cpd(&["session", "open", LEAD1]).ok()["already"], true);

    // Another Lead displaces it into a park window named for it.
    let out = env.cpd(&["session", "open", LEAD2]).ok();
    assert_eq!(out["displaced"], LEAD1);
    assert_eq!(slot(&env).session, LEAD2);
    let lead1 = panes(&env).into_iter().find(|p| p.session == LEAD1).unwrap();
    assert_eq!((lead1.pane.as_str(), lead1.window_name.as_str()), (lead1_pane.as_str(), "park-pay"));
    assert_eq!(window_names(&env), ["main", "park-pay"]);
    assert_eq!((show(&env, LEAD1)["place"].clone(), show(&env, LEAD2)["place"].clone()), (json!("parked"), json!("chat")));
    assert_placeholders(&env, 0);

    // Park: a placeholder takes the slot; the Lead is alone in a new park window.
    let out = env.cpd(&["session", "park"]).ok();
    assert_eq!(out["session"], LEAD2);
    let s = slot(&env);
    assert_eq!((s.view.as_str(), s.session.as_str()), ("chat", ""));
    assert_placeholders(&env, 1);
    assert!(env.calls("cpd").contains(&vec!["tui".to_owned(), "view".to_owned(), "chat".to_owned()]));
    let lead2 = panes(&env).into_iter().find(|p| p.session == LEAD2).unwrap();
    assert_eq!((lead2.window.as_str(), lead2.window_name.as_str()), (out["window"].as_str().unwrap(), "park-other"));
    assert_eq!(window_names(&env), ["main", "park-other", "park-pay"]);
    assert_eq!(show(&env, LEAD2)["place"], "parked");

    // Nothing in the slot to park, or not in the slot: nothing changes.
    assert_eq!(env.cpd(&["session", "park"]).ok(), json!({"ok": true, "session": null, "already": true}));
    assert_eq!(env.cpd(&["session", "park", LEAD1]).ok()["already"], true);
    assert_placeholders(&env, 1);

    // Opening a parked Lead uses its live pane, and kills the placeholder.
    let out = env.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!((&out["pane"], &out["displaced"]), (&json!(lead1_pane), &Value::Null));
    assert_eq!(slot(&env).pane, lead1_pane);
    assert_placeholders(&env, 0);
    assert_eq!(window_names(&env), ["main", "park-other"]);
    let lead1_runs = env.calls("cpd").iter().filter(|c| c[..2] == ["session", "run"] && c[2] == LEAD1).count();
    assert_eq!(lead1_runs, 1, "lead1 was started once");

    let moved: Vec<(String, String)> = events(&env, "session-moved")
        .iter()
        .map(|e| (e["session"].as_str().unwrap().to_owned(), e["data"]["place"].as_str().unwrap().to_owned()))
        .collect();
    let expected = [(LEAD1, "chat"), (LEAD2, "chat"), (LEAD1, "parked"), (LEAD2, "parked"), (LEAD1, "chat")];
    assert_eq!(moved, expected.map(|(s, p)| (s.to_owned(), p.to_owned())));
}

#[test]
fn open_writes_session_moved_before_the_rename_so_a_failing_rename_keeps_the_swap_recorded() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    env.cpd(&["session", "open", LEAD1]).ok();

    // A tmux that fails `rename-window` and runs everything else on the real one.
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|d| d.join("tmux"))
        .find(|p| p.is_file())
        .expect("a tmux on PATH");
    env.fake("tmux", &format!("case \"$*\" in *rename-window*) echo 'rename-window: refused' >&2; exit 1;; esac\nexec '{}' \"$@\"", real.display()));

    let run = env.cpd(&["session", "open", LEAD2]);
    refused(&run, "tmux_failed", 1);
    assert_eq!(slot(&env).session, LEAD2, "the swap happened");
    let moved: Vec<(String, String)> = events(&env, "session-moved")
        .iter()
        .map(|e| (e["session"].as_str().unwrap().to_owned(), e["data"]["place"].as_str().unwrap().to_owned()))
        .collect();
    let expected = [(LEAD1, "chat"), (LEAD2, "chat"), (LEAD1, "parked")];
    assert_eq!(moved, expected.map(|(s, p)| (s.to_owned(), p.to_owned())));
    assert_eq!((show(&env, LEAD1)["place"].clone(), show(&env, LEAD2)["place"].clone()), (json!("parked"), json!("chat")));
}

#[test]
fn open_and_park_take_a_context_s_manager_and_park_it_in_its_context_window() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    // `context manager` wrote its first message.
    let message = home(&env).manager_message(&cpd_core::ids::ContextId::parse("ctx").unwrap());
    fs::create_dir_all(message.parent().unwrap()).unwrap();
    fs::write(&message, "You are the Manager.").unwrap();

    // A stopped Manager with no pane is resumed in a new window and swapped in.
    let out = env.cpd(&["session", "open", MANAGER]).ok();
    assert_eq!(out["displaced"], Value::Null);
    assert_eq!(slot(&env).session, MANAGER);
    assert_placeholders(&env, 0);
    let arg = format!("@{}", message.display());
    wait_until("the Manager's pi", || env.calls("pi").iter().any(|c| c.contains(&arg)));
    assert_eq!(show(&env, MANAGER)["place"], "chat");

    // A Lead displaces it into `ctx-<context>`.
    assert_eq!(env.cpd(&["session", "open", LEAD1]).ok()["displaced"], MANAGER);
    let manager = panes(&env).into_iter().find(|p| p.session == MANAGER).unwrap();
    assert_eq!(manager.window_name, "ctx-ctx");
    assert_eq!(window_names(&env), ["ctx-ctx", "main"]);
    assert_eq!(show(&env, MANAGER)["place"], "parked");

    // Parking the Lead, then opening the Manager again uses its live pane.
    env.cpd(&["session", "park"]).ok();
    let out = env.cpd(&["session", "open", MANAGER]).ok();
    assert_eq!(out["pane"], json!(manager.pane));
    env.cpd(&["session", "park", MANAGER]).ok();
    assert_eq!(panes(&env).into_iter().find(|p| p.session == MANAGER).unwrap().window_name, "ctx-ctx");
}

#[test]
fn open_and_park_refusals() {
    let env = TestEnv::new();
    setup(&env);
    // No cockpit yet.
    let error = refused(&env.cpd(&["session", "open", LEAD1]), "no_cockpit", 3);
    assert_eq!(error["details"]["next"], "cpd tui");
    refused(&env.cpd(&["session", "park"]), "no_cockpit", 3);
    refused(&env.cpd(&["session", "park", LEAD1]), "no_cockpit", 3);

    env.fake_cpd_views();
    cockpit(&env);
    refused(&env.cpd(&["session", "open", WORKER]), "not_a_lead", 3);
    refused(&env.cpd(&["session", "open", "nobody"]), "session_not_found", 4);
    refused(&env.cpd(&["session", "park", WORKER]), "not_a_lead", 3);
    refused(&env.cpd(&["session", "park", "nobody"]), "session_not_found", 4);
    assert_placeholders(&env, 1);
    assert_eq!(window_names(&env), ["main"]);
}

// ---------- history ----------

/// Lead `lead2`, archived, with a readable session file.
fn archived_lead2(env: &TestEnv) {
    let file = env.root().join("lead2.jsonl");
    fs::write(&file, "{\"type\":\"session\"}\n").unwrap();
    exec_sql(
        env,
        &format!("UPDATE sessions SET file = '{}', archived_at = 't9' WHERE id = 'lead2'", file.display()),
    );
}

fn history_panes(env: &TestEnv) -> Vec<P> {
    panes(env).into_iter().filter(|p| p.view == "history").collect()
}

#[test]
fn history_parks_the_lead_in_the_slot_and_open_kills_the_history_pane() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    archived_lead2(&env);
    let lead1_pane = env.cpd(&["session", "open", LEAD1]).ok()["pane"].as_str().unwrap().to_owned();
    assert_eq!(window_names(&env), ["main"]);

    // The Lead in the slot is parked; the history pane takes the slot.
    let out = env.cpd(&["session", "history", LEAD2]).ok();
    assert_eq!(out["session"], LEAD2);
    assert_eq!(out["parked"], LEAD1);
    let s = slot(&env);
    assert_eq!((s.view.as_str(), s.history.as_str(), s.session.as_str()), ("history", LEAD2, ""));
    assert_eq!(out["pane"], json!(s.pane));
    assert_eq!(s.window_name, "main");
    wait_until("the history view's start", || {
        env.calls("cpd").contains(&["tui", "view", "history", LEAD2].map(str::to_owned).to_vec())
    });
    let lead1 = panes(&env).into_iter().find(|p| p.session == LEAD1).unwrap();
    assert_eq!((lead1.pane.as_str(), lead1.window_name.as_str()), (lead1_pane.as_str(), "park-pay"));
    assert_eq!(show(&env, LEAD1)["place"], "parked");
    assert_eq!(history_panes(&env).len(), 1);
    assert_placeholders(&env, 0);
    assert_eq!(window_names(&env), ["main", "park-pay"]);

    // A rerun respawns the same pane: still one history pane, nothing parked.
    let again = env.cpd(&["session", "history", LEAD2]).ok();
    assert_eq!((&again["pane"], &again["parked"]), (&out["pane"], &Value::Null));
    assert_eq!(history_panes(&env).len(), 1);
    assert_eq!(window_names(&env), ["main", "park-pay"]);

    // There is no Lead to park in a history pane.
    assert_eq!(env.cpd(&["session", "park"]).ok(), json!({"ok": true, "session": null, "already": true}));
    assert_eq!(env.cpd(&["session", "park", LEAD1]).ok()["already"], true);
    assert_eq!(history_panes(&env).len(), 1);

    // A Lead taking the slot kills the history pane with its window.
    let out = env.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!((&out["pane"], &out["displaced"]), (&json!(lead1_pane), &Value::Null));
    assert_eq!(slot(&env).session, LEAD1);
    assert!(history_panes(&env).is_empty());
    assert_placeholders(&env, 0);
    assert_eq!(window_names(&env), ["main"]);

    let moved: Vec<(String, String)> = events(&env, "session-moved")
        .iter()
        .map(|e| (e["session"].as_str().unwrap().to_owned(), e["data"]["place"].as_str().unwrap().to_owned()))
        .collect();
    let expected = [(LEAD1, "chat"), (LEAD1, "parked"), (LEAD1, "chat")];
    assert_eq!(moved, expected.map(|(s, p)| (s.to_owned(), p.to_owned())));
}

#[test]
fn history_replaces_the_placeholder_in_place_and_park_brings_it_back() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    cockpit(&env);
    archived_lead2(&env);
    let placeholder = slot(&env);

    let out = env.cpd(&["session", "history", LEAD2]).ok();
    assert_eq!(out["parked"], Value::Null);
    let s = slot(&env);
    assert_eq!((s.pane.as_str(), s.view.as_str(), s.history.as_str()), (placeholder.pane.as_str(), "history", LEAD2));
    assert_placeholders(&env, 0);
    assert_eq!(window_names(&env), ["main"]);
    assert!(events(&env, "session-moved").is_empty());

    // The history of another Session retargets the same pane.
    exec_sql(&env, &format!("UPDATE sessions SET file = '{}' WHERE id = 'lead1'", env.root().join("lead2.jsonl").display()));
    env.cpd(&["session", "history", LEAD1]).ok();
    let s = slot(&env);
    assert_eq!((s.pane.as_str(), s.history.as_str()), (placeholder.pane.as_str(), LEAD1));
    assert_eq!(history_panes(&env).len(), 1);
}

#[test]
fn history_refusals_change_nothing() {
    let env = TestEnv::new();
    setup(&env);
    refused(&env.cpd(&["session", "history", "nobody"]), "session_not_found", 4);
    refused(&env.cpd(&["session", "history", WORKER]), "not_a_lead", 3);
    refused(&env.cpd(&["session", "history", MANAGER]), "not_a_lead", 3);
    exec_sql(&env, "UPDATE sessions SET file = '/nonexistent/lead2.jsonl' WHERE id = 'lead2'");
    refused(&env.cpd(&["session", "history", LEAD2]), "file_unreadable", 2);
    archived_lead2(&env);
    // No cockpit yet.
    let error = refused(&env.cpd(&["session", "history", LEAD2]), "no_cockpit", 3);
    assert_eq!(error["details"]["next"], "cpd tui");

    env.fake_cpd_views();
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    env.cpd(&["session", "open", LEAD1]).ok();
    refused(&env.cpd(&["session", "history", WORKER]), "not_a_lead", 3);
    refused(&env.cpd(&["session", "history", MANAGER]), "not_a_lead", 3);
    // The refused ones parked nothing.
    assert_eq!(slot(&env).session, LEAD1);
    assert!(history_panes(&env).is_empty());
    assert_eq!(events(&env, "session-moved").len(), 1);
}

// ---------- close ----------

#[test]
fn close_refusals_and_nothing_to_close() {
    let env = TestEnv::new();
    setup(&env);
    refused(&env.cpd(&["session", "close", WORKER]), "not_a_lead", 3);
    refused(&env.cpd(&["session", "close", "nobody"]), "session_not_found", 4);
    // A Lead with no pane: nothing to do.
    env.fake_cpd_views();
    cockpit(&env);
    assert_eq!(env.cpd(&["session", "close", LEAD2]).ok(), json!({"ok": true, "session": LEAD2, "pane": null, "already": true}));
    // A pane that is gone, with E2 not having recorded it: nothing is written.
    exec_sql(&env, "UPDATE sessions SET tmux_pane = '%99' WHERE id = 'lead2'");
    assert_eq!(env.cpd(&["session", "close", LEAD2]).ok()["already"], true);
    assert_eq!(query::<String>(&env, "SELECT tmux_pane FROM sessions WHERE id = 'lead2'"), "%99");
    assert_eq!(events(&env, "session-moved").len(), 0);
    assert_eq!(events(&env, "session-state").len(), 0);
}

#[test]
fn close_refuses_the_lead_in_the_chat_slot_and_leaves_everything() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    let pane = env.cpd(&["session", "open", LEAD1]).ok()["pane"].as_str().unwrap().to_owned();
    let error = refused(&env.cpd(&["session", "close", LEAD1]), "in_chat_slot", 3);
    assert_eq!(error["details"]["next"], "cpd session park");
    assert_eq!(slot(&env).pane, pane);
    assert_eq!(show(&env, LEAD1)["tmux_pane"], json!(pane));
    assert_eq!(events(&env, "session-moved").len(), 1);
}

#[test]
fn close_kills_a_parked_pane_and_writes_what_e2_writes() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    // lead2 displaces lead1 into a park window; lead1 is made `idle`, as pi would report.
    let lead1_pane = env.cpd(&["session", "open", LEAD1]).ok()["pane"].as_str().unwrap().to_owned();
    env.cpd(&["session", "open", LEAD2]).ok();
    assert_eq!(show(&env, LEAD1)["place"], "parked");
    assert_eq!(window_names(&env), ["main", "park-pay"]);
    exec_sql(&env, "UPDATE sessions SET state = 'idle' WHERE id = 'lead1'");

    let out = env.cpd(&["session", "close", LEAD1]).ok();
    assert_eq!(out, json!({"ok": true, "session": LEAD1, "pane": lead1_pane}));
    // The pane is gone with its window; the file of pi stays (not touched); the slot is unchanged.
    assert!(panes(&env).iter().all(|p| p.session != LEAD1));
    assert_eq!(window_names(&env), ["main"]);
    assert_eq!(slot(&env).session, LEAD2);
    let lead1 = show(&env, LEAD1);
    assert_eq!((&lead1["state"], &lead1["tmux_pane"], &lead1["place"]), (&json!("stopped"), &Value::Null, &json!("none")));
    let state: Vec<Value> = events(&env, "session-state").into_iter().filter(|e| e["session"] == LEAD1).collect();
    assert_eq!(state.len(), 1);
    assert_eq!(state[0]["data"], json!({"from": "idle", "to": "stopped", "label": null}));
    let moved = events(&env, "session-moved");
    let last = moved.last().unwrap();
    assert_eq!((&last["session"], &last["data"]), (&json!(LEAD1), &json!({"pane": null, "window": null, "place": "none"})));
    let after = moved.len();

    // Idempotent: nothing more is written.
    assert_eq!(env.cpd(&["session", "close", LEAD1]).ok()["already"], true);
    assert_eq!(events(&env, "session-moved").len(), after);

    // Open resumes it: a new pane, in the slot.
    let out = env.cpd(&["session", "open", LEAD1]).ok();
    assert_ne!(out["pane"], json!(lead1_pane));
    assert_eq!(out["displaced"], LEAD2);
}

#[test]
fn close_of_a_stopped_lead_with_a_pane_appends_only_session_moved() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    // lead2 is `stopped` (the fake pi reports nothing) and parked by lead1.
    env.cpd(&["session", "open", LEAD2]).ok();
    env.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!((show(&env, LEAD2)["state"].clone(), show(&env, LEAD2)["place"].clone()), (json!("stopped"), json!("parked")));
    let states = events(&env, "session-state").len();

    env.cpd(&["session", "close", LEAD2]).ok();
    assert_eq!(events(&env, "session-state").len(), states);
    let lead2 = show(&env, LEAD2);
    assert_eq!((&lead2["tmux_pane"], &lead2["place"]), (&Value::Null, &json!("none")));
    assert!(panes(&env).iter().all(|p| p.session != LEAD2));
}

#[test]
fn rename_works_on_an_archived_session() {
    let env = TestEnv::new();
    setup(&env);
    exec_sql(&env, "UPDATE sessions SET archived_at = 't9' WHERE id = 'lead2'");
    let out = env.cpd(&["session", "rename", "Archived and renamed", "--session", "lead2"]).ok();
    assert_eq!(out, json!({"ok": true, "session": "lead2", "name": "Archived and renamed"}));
    let row = show(&env, "lead2");
    assert_eq!((&row["name"], &row["archived_at"]), (&json!("Archived and renamed"), &json!("t9")));
    assert_eq!(events(&env, "session-renamed").len(), 1);
}

#[test]
fn rename_sets_the_title_of_a_forks_live_pane_to_its_name() {
    let env = TestEnv::new();
    let fork = parked_fork(&env);
    let pane = panes(&env).into_iter().find(|p| p.session == fork).unwrap().pane;
    let title = |env: &TestEnv| {
        let out = env.tmux(&["show", "-pqv", "-t", &pane, "@cpd_title"]);
        String::from_utf8(out.stdout).unwrap().trim_end_matches('\n').to_owned()
    };
    assert_eq!(title(&env), "Lead one (fork)");
    env.cpd(&["session", "rename", "side quest", "--session", &fork]).ok();
    assert_eq!(title(&env), "side quest");

    // A Session with no pane has nothing to set.
    env.cpd(&["session", "rename", "Lead two renamed", "--session", "lead2"]).ok();
}

#[test]
fn open_gives_a_new_pane_the_plans_title_for_the_plans_lead_and_the_name_for_a_fork() {
    let env = TestEnv::new();
    setup(&env);
    env.fake_cpd_views();
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    let title = |env: &TestEnv, session: &str| {
        let pane = panes(env).into_iter().find(|p| p.session == session).unwrap().pane;
        let out = env.tmux(&["show", "-pqv", "-t", &pane, "@cpd_title"]);
        String::from_utf8(out.stdout).unwrap().trim_end_matches('\n').to_owned()
    };
    env.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!(title(&env, LEAD1), "Pay");
    let fork = env.cpd(&["session", "fork", LEAD1, "--name", "side"]).ok()["session"]["id"].as_str().unwrap().to_owned();
    assert_eq!(title(&env, &fork), "side");
}

// ---------- archive ----------

/// A parked side fork of `lead1` (stopped, with a pane): fake pi and cpd, and the cockpit.
fn parked_fork(env: &TestEnv) -> String {
    setup(env);
    env.fake_cpd_views();
    fake_pi(env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(env);
    env.cpd(&["session", "fork", LEAD1]).ok()["session"]["id"].as_str().unwrap().to_owned()
}

fn archived_at(env: &TestEnv, id: &str) -> Value {
    show(env, id)["archived_at"].clone()
}

#[test]
fn archive_kills_the_pane_of_an_idle_parked_fork_and_appends_one_event() {
    let env = TestEnv::new();
    let fork = parked_fork(&env);
    exec_sql(&env, &format!("UPDATE sessions SET state = 'idle' WHERE id = '{fork}'"));
    let pane = show(&env, &fork)["tmux_pane"].as_str().unwrap().to_owned();
    assert_eq!(archived_at(&env, &fork), Value::Null);

    let out = env.cpd(&["session", "archive", &fork]).ok();
    assert_eq!(out, json!({"ok": true, "archived": [{"kind": "session", "id": fork}], "closed": [pane]}));
    assert!(panes(&env).iter().all(|p| p.session != fork));
    let row = show(&env, &fork);
    assert!(row["archived_at"].is_string(), "{row}");
    assert_eq!((&row["state"], &row["tmux_pane"], &row["place"]), (&json!("stopped"), &Value::Null, &json!("none")));
    let archived = events(&env, "session-archived");
    assert_eq!(archived.len(), 1);
    assert_eq!((&archived[0]["session"], &archived[0]["data"]), (&json!(fork), &json!({"archived": true})));

    // `session list` shows it; the Plan's Lead and the other Sessions are not archived.
    let list = env.cpd(&["session", "list"]).ok();
    let at = |id: &str| list["sessions"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap()["archived_at"].clone();
    assert_eq!(at(&fork), row["archived_at"]);
    assert_eq!(at(LEAD1), Value::Null);

    // Idempotent: nothing more is written, nothing closed.
    let again = env.cpd(&["session", "archive", &fork]).ok();
    assert_eq!(again, json!({"ok": true, "archived": [], "closed": [], "already": true}));
    assert_eq!(events(&env, "session-archived").len(), 1);
}

#[test]
fn archive_of_a_lead_with_no_pane_closes_nothing() {
    let env = TestEnv::new();
    setup(&env);
    let fork = "forkless";
    exec_sql(
        &env,
        &format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, forked_from)
               VALUES ('{fork}', 'lead', 'ctx', 'pay', 'Side', '/', 'O3', 'stopped', 't', 't', 'lead1')"
        ),
    );
    let out = env.cpd(&["session", "archive", fork]).ok();
    assert_eq!(out, json!({"ok": true, "archived": [{"kind": "session", "id": fork}], "closed": []}));
    assert_eq!(events(&env, "session-archived").len(), 1);
}

#[test]
fn archive_refuses_a_working_or_blocked_session_and_changes_nothing() {
    let env = TestEnv::new();
    let fork = parked_fork(&env);
    let pane = show(&env, &fork)["tmux_pane"].as_str().unwrap().to_owned();
    let moved = events(&env, "session-moved").len();
    for state in ["working", "blocked"] {
        exec_sql(&env, &format!("UPDATE sessions SET state = '{state}', state_label = {} WHERE id = '{fork}'", if state == "blocked" { "'why'" } else { "NULL" }));
        let error = refused(&env.cpd(&["session", "archive", &fork]), "archive_blocked", 3);
        let blockers = error["details"]["blockers"].as_array().unwrap();
        assert_eq!(blockers.len(), 1, "{error}");
        assert_eq!((&blockers[0]["kind"], &blockers[0]["id"]), (&json!("session"), &json!(fork)));
        assert!(blockers[0]["reason"].as_str().unwrap().contains(state), "{error}");
        assert_eq!(archived_at(&env, &fork), Value::Null);
        assert_eq!(show(&env, &fork)["tmux_pane"], json!(pane));
        assert!(panes(&env).iter().any(|p| p.session == fork));
    }
    assert_eq!(events(&env, "session-archived").len(), 0);
    assert_eq!(events(&env, "session-moved").len(), moved);
}

#[test]
fn archive_refuses_the_chat_slot_a_plan_lead_a_worker_and_an_unknown_session() {
    let env = TestEnv::new();
    let fork = parked_fork(&env);
    exec_sql(&env, &format!("UPDATE sessions SET state = 'idle' WHERE id = '{fork}'"));
    let pane = env.cpd(&["session", "open", &fork]).ok()["pane"].as_str().unwrap().to_owned();
    let error = refused(&env.cpd(&["session", "archive", &fork]), "in_chat_slot", 3);
    assert_eq!(error["details"]["next"], "cpd session park");
    assert_eq!(slot(&env).pane, pane);
    assert_eq!(archived_at(&env, &fork), Value::Null);

    let error = refused(&env.cpd(&["session", "archive", LEAD1]), "archive_blocked", 3);
    assert_eq!(error["details"]["next"], "cpd plan archive --plan pay");
    assert_eq!(archived_at(&env, LEAD1), Value::Null);

    refused(&env.cpd(&["session", "archive", WORKER]), "not_a_lead", 3);
    refused(&env.cpd(&["session", "archive", "nobody"]), "session_not_found", 4);
    assert_eq!(events(&env, "session-archived").len(), 0);
}

#[test]
fn unarchive_clears_archived_at_with_one_event_and_touches_no_pane() {
    let env = TestEnv::new();
    let fork = parked_fork(&env);
    env.cpd(&["session", "archive", &fork]).ok();
    let before = panes(&env);

    let out = env.cpd(&["session", "unarchive", &fork]).ok();
    assert_eq!(out, json!({"ok": true, "archived": [{"kind": "session", "id": fork}], "closed": []}));
    assert_eq!(archived_at(&env, &fork), Value::Null);
    assert_eq!(panes(&env), before);
    let archived = events(&env, "session-archived");
    assert_eq!(archived.len(), 2);
    assert_eq!(archived[1]["data"], json!({"archived": false}));

    // Idempotent.
    let again = env.cpd(&["session", "unarchive", &fork]).ok();
    assert_eq!(again, json!({"ok": true, "archived": [], "closed": [], "already": true}));
    assert_eq!(events(&env, "session-archived").len(), 2);

    refused(&env.cpd(&["session", "unarchive", WORKER]), "not_a_lead", 3);
    refused(&env.cpd(&["session", "unarchive", "nobody"]), "session_not_found", 4);
}

#[test]
fn unarchive_is_refused_while_its_plan_or_context_is_archived() {
    let env = TestEnv::new();
    setup(&env);
    exec_sql(&env, "UPDATE sessions SET archived_at = 't' WHERE id = 'lead2'");
    exec_sql(&env, "UPDATE plans SET archived_at = 't' WHERE id = 'other'");
    let error = refused(&env.cpd(&["session", "unarchive", LEAD2]), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd plan unarchive --plan other");
    exec_sql(&env, "UPDATE contexts SET archived_at = 't' WHERE id = 'ctx'");
    let error = refused(&env.cpd(&["session", "unarchive", LEAD2]), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd context unarchive ctx");
    assert_eq!(archived_at(&env, LEAD2), json!("t"));
    assert_eq!(events(&env, "session-archived").len(), 0);
}

/// No tmux server runs here: an `archived` refusal, not `no_cockpit`, shows that nothing asked tmux first.
#[test]
fn open_and_fork_of_an_archived_session_are_refused_before_any_tmux() {
    let env = TestEnv::new();
    setup(&env);
    exec_sql(&env, "UPDATE sessions SET archived_at = 't' WHERE id = 'lead2'");
    for args in [["session", "open", LEAD2], ["session", "fork", LEAD2]] {
        let error = refused(&env.cpd(&args), "archived", 3);
        assert_eq!(error["details"]["next"], "cpd session unarchive lead2");
    }
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM sessions"), 4);
    assert_eq!(events(&env, "session-registered").len(), 1);
}

// ---------- dead Lead panes (contract §3.6, §3.7 E2; commands.md §10.4) ----------

/// Like [`fake_cpd`], but `cpd session run` reads no input, so it returns when its pi
/// exits and the pane dies (`remain-on-exit`).
fn fake_cpd_returning(env: &TestEnv) {
    env.fake(
        "cpd",
        &format!(
            "case \"$1 $2 $3\" in \"tui view chat\"|\"tui view history\") exec sleep {SLEEP_SECONDS};; \"session run \"*) exec '{CPD}' \"$@\" </dev/null;; esac\nexec '{CPD}' \"$@\""
        ),
    );
}

/// The cockpit with `lead2`'s pi alive and every other Lead's pi alive until [`release_leads`]
/// (a pi that exits at once would race the test's own `open`s).
fn dying_leads(env: &TestEnv) {
    setup(env);
    fake_cpd_returning(env);
    let release = env.home().join("release-leads");
    fake_pi(
        env,
        &format!(
            "[ \"$CPD_SESSION\" = lead2 ] && exec sleep {SLEEP_SECONDS}\nwhile [ ! -e '{}' ]; do sleep 0.1; done\nexit 1",
            release.display()
        ),
    );
    cockpit(env);
    assert!(env.tmux(&["set", "-g", "remain-on-exit", "on"]).status.success());
}

/// Lets every Lead's pi but `lead2`'s exit, so its pane dies.
fn release_leads(env: &TestEnv) {
    fs::write(env.home().join("release-leads"), "").unwrap();
}

fn is_dead(env: &TestEnv, pane: &str) -> bool {
    let out = env.tmux(&["list-panes", "-a", "-F", "#{pane_id} #{pane_dead}"]);
    String::from_utf8(out.stdout).unwrap().lines().any(|l| l == format!("{pane} 1"))
}

/// `cpd watch` with a one-second tick, killed when dropped.
struct Engine(std::process::Child);

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn engine(env: &TestEnv) -> Engine {
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    let path = env.cpd_home().join("config.toml");
    let config = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{config}[watch]\ntick_seconds = 1\n")).unwrap();
    let child = env.command(CPD).arg("watch").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    Engine(child)
}

#[test]
fn e2_kills_the_window_of_a_dead_parked_lead() {
    let env = TestEnv::new();
    dying_leads(&env);
    let lead1_pane = env.cpd(&["session", "open", LEAD1]).ok()["pane"].as_str().unwrap().to_owned();
    env.cpd(&["session", "open", LEAD2]).ok();
    assert_eq!(window_names(&env), ["main", "park-pay"]);
    release_leads(&env);
    wait_until("lead1's pane to die", || is_dead(&env, &lead1_pane));

    let _engine = engine(&env);
    wait_until("E2 to kill the window", || window_names(&env) == ["main"]);
    wait_until("E2 to record the pane gone", || show(&env, LEAD1)["tmux_pane"].is_null());
    assert_eq!((show(&env, LEAD1)["state"].clone(), show(&env, LEAD1)["place"].clone()), (json!("stopped"), json!("none")));
    assert_eq!(slot(&env).session, LEAD2, "the chat slot is untouched");
}

#[test]
fn open_leaves_one_window_for_a_lead_with_a_dead_pane() {
    let env = TestEnv::new();
    dying_leads(&env);
    let lead1_pane = env.cpd(&["session", "open", LEAD1]).ok()["pane"].as_str().unwrap().to_owned();
    env.cpd(&["session", "open", LEAD2]).ok();
    assert_eq!(window_names(&env), ["main", "park-pay"]);
    release_leads(&env);
    wait_until("lead1's pane to die", || is_dead(&env, &lead1_pane));

    // No engine runs: `open` itself kills the dead pane before it makes a window.
    let out = env.cpd(&["session", "open", LEAD1]).ok();
    assert_ne!(out["pane"], json!(lead1_pane));
    assert_eq!(out["displaced"], LEAD2);
    assert_eq!(window_names(&env), ["main", "park-other"]);
    let lead1: Vec<P> = panes(&env).into_iter().filter(|p| p.session == LEAD1).collect();
    assert_eq!(lead1.len(), 1, "{lead1:?}");
    assert_eq!(lead1[0].window_name, "main");
}

#[test]
fn e2_replaces_a_dead_lead_in_the_chat_slot_by_the_placeholder() {
    let env = TestEnv::new();
    dying_leads(&env);
    let lead1_pane = env.cpd(&["session", "open", LEAD1]).ok()["pane"].as_str().unwrap().to_owned();
    release_leads(&env);
    wait_until("lead1's pane to die", || is_dead(&env, &lead1_pane));
    assert_eq!(slot(&env).session, LEAD1);

    let _engine = engine(&env);
    wait_until("the placeholder", || slot(&env).session.is_empty());
    let s = slot(&env);
    assert_eq!((s.pane.as_str(), s.view.as_str(), s.window_name.as_str()), (lead1_pane.as_str(), "chat", "main"));
    wait_until("the placeholder to run", || !is_dead(&env, &lead1_pane));
    assert_placeholders(&env, 1);
    assert_eq!(window_names(&env), ["main"]);
    wait_until("E2 to record the pane gone", || show(&env, LEAD1)["tmux_pane"].is_null());
    assert_eq!(show(&env, LEAD1)["place"], "none");
}

#[test]
fn open_replaces_a_dead_lead_in_the_chat_slot() {
    let env = TestEnv::new();
    dying_leads(&env);
    let lead1_pane = env.cpd(&["session", "open", LEAD1]).ok()["pane"].as_str().unwrap().to_owned();
    release_leads(&env);
    wait_until("lead1's pane to die", || is_dead(&env, &lead1_pane));

    // No engine runs, and the slot holds a dead pane of this very Lead: it is not "already" there.
    let out = env.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!(out["displaced"], Value::Null);
    assert_ne!(out["pane"], json!(lead1_pane));
    assert_eq!(window_names(&env), ["main"]);
    let lead1: Vec<P> = panes(&env).into_iter().filter(|p| p.session == LEAD1).collect();
    assert_eq!(lead1.len(), 1, "{lead1:?}");
    assert_eq!(slot(&env).pane, out["pane"].as_str().unwrap());
}

// ---------- run ----------

#[test]
fn run_refusals() {
    let env = TestEnv::new();
    setup(&env);
    refused(&env.cpd(&["session", "run", WORKER]), "not_a_lead", 3);
    refused(&env.cpd(&["session", "run", "nobody"]), "session_not_found", 4);
    // Outside any pane of cpd's server.
    let error = refused(&env.cpd(&["session", "run", LEAD1]), "wrong_pane", 3);
    assert_eq!(error["details"]["session"], LEAD1);
    assert!(env.calls("pi").is_empty());

    // In a pane of cpd's server that runs another Session.
    cockpit(&env);
    let out = env.tmux(&["new-window", "-d", "-t", "=cpd:", "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    env.tmux(&["set", "-p", "-t", &pane, "@cpd_session", LEAD2]);
    let server = String::from_utf8(env.tmux(&["display", "-p", "-t", &pane, "#{socket_path},1,0"]).stdout).unwrap();
    let run = cpd_with(&env, &[("TMUX", server.trim()), ("TMUX_PANE", &pane)], &["session", "run", LEAD1]);
    let error = refused(&run, "wrong_pane", 3);
    assert_eq!((&error["details"]["pane"], &error["details"]["found"]), (&json!(pane), &json!(LEAD2)));
    assert!(env.calls("pi").is_empty());
}

// ---------- run: the Plan's plannotator port ----------

/// A fake `pi` that records `PLANNOTATOR_PORT` per Session and exits with status 1 (not a quit), and the
/// session files of both Plans' Leads, so `session run` resumes them.
fn port_fixture(env: &TestEnv) {
    setup(env);
    env.fake_cpd_views();
    fake_pi(env, "echo \"${PLANNOTATOR_PORT-unset}\" >> \"$HOME/ports.$CPD_SESSION\"\nexit 1");
    cockpit(env);
    let sessions = env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    for lead in [LEAD1, LEAD2] {
        fs::write(sessions.join(format!("2026_{lead}.jsonl")), "{\"type\":\"session\"}\n").unwrap();
    }
}

/// A new window running `cpd session run <id>`, with `PLANNOTATOR_PORT=1` in its
/// environment (the port every Lead pane inherits from cpd's environment); returns its pane.
fn run_lead(env: &TestEnv, id: &str) -> String {
    run_lead_with(env, id, &[])
}

/// [`run_lead`] with more `NAME=value` variables in the pane's environment.
fn run_lead_with(env: &TestEnv, id: &str, vars: &[&str]) -> String {
    let command = format!("cpd session run {id}");
    let mut args = vec!["new-window", "-d", "-t", "=cpd:", "-P", "-F", "#{pane_id}", "-e", "PLANNOTATOR_PORT=1"];
    for var in vars {
        args.extend(["-e", var]);
    }
    args.push(&command);
    let out = env.tmux(&args);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    env.tmux(&["set", "-p", "-t", &pane, "@cpd_session", id]);
    pane
}

fn capture(env: &TestEnv, pane: &str) -> String {
    String::from_utf8(env.tmux(&["capture-pane", "-p", "-t", pane]).stdout).unwrap()
}

fn recorded_ports(env: &TestEnv, id: &str) -> Vec<String> {
    fs::read_to_string(env.home().join(format!("ports.{id}"))).map(|t| t.lines().map(str::to_owned).collect()).unwrap_or_default()
}

#[test]
fn run_gives_each_plan_its_port_and_keeps_it_across_a_restart() {
    let env = TestEnv::new();
    port_fixture(&env);

    let pane1 = run_lead(&env, LEAD1);
    wait_until("the first Lead's pi", || recorded_ports(&env, LEAD1).len() == 1);
    let pane2 = run_lead(&env, LEAD2);
    wait_until("the second Lead's pi", || recorded_ports(&env, LEAD2).len() == 1);
    assert_eq!(recorded_ports(&env, LEAD1), ["19432"], "the Plan's port replaces the inherited one");
    assert_eq!(recorded_ports(&env, LEAD2), ["19433"]);
    let home = home(&env);
    let file = |plan: &str| fs::read_to_string(home.plannotator_port(&PlanId::parse(plan).unwrap())).unwrap();
    assert_eq!((file("pay").as_str(), file("other").as_str()), ("19432\n", "19433\n"));

    // The second Lead is restarted first: it keeps its port; so does the first.
    for (pane, id) in [(&pane2, LEAD2), (&pane1, LEAD1)] {
        wait_until("the resume prompt", || capture(&env, pane).contains("Press Enter to resume"));
        env.tmux(&["send-keys", "-t", pane, "Enter"]);
        wait_until("the restart", || recorded_ports(&env, id).len() == 2);
    }
    assert_eq!(recorded_ports(&env, LEAD1), ["19432", "19432"]);
    assert_eq!(recorded_ports(&env, LEAD2), ["19433", "19433"]);
}

#[test]
fn run_gives_the_lead_remote_and_the_review_host() {
    let env = TestEnv::new();
    port_fixture(&env);
    fake_pi(&env, "echo \"$PLANNOTATOR_REMOTE ${PLANNOTATOR_URL_HOST-unset}\" >> \"$HOME/hosts.$CPD_SESSION\"\nexit 1");
    let hosts = |id: &str| fs::read_to_string(env.home().join(format!("hosts.{id}"))).unwrap_or_default();

    // The environment's `PLANNOTATOR_URL_HOST` is the override; the `PLANNOTATOR_REMOTE` it holds is replaced.
    run_lead_with(&env, LEAD1, &["PLANNOTATOR_URL_HOST=review.test", "PLANNOTATOR_REMOTE=0"]);
    wait_until("the first Lead's pi", || !hosts(LEAD1).is_empty());
    assert_eq!(hosts(LEAD1), "1 review.test\n");

    // With no override the host is detected: an address or `localhost`, never unset.
    run_lead(&env, LEAD2);
    wait_until("the second Lead's pi", || !hosts(LEAD2).is_empty());
    let hosts2 = hosts(LEAD2);
    let (remote, host) = hosts2.trim_end().split_once(' ').unwrap();
    assert_eq!(remote, "1");
    assert!(host == "localhost" || host.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| !ip.is_loopback()), "{host}");
}

#[test]
fn run_with_the_range_full_says_no_port_and_waits_for_enter() {
    let env = TestEnv::new();
    port_fixture(&env);
    // One port in the range: the first Lead holds it.
    let config = fs::read_to_string(env.cpd_home().join("config.toml")).unwrap();
    assert!(config.contains("lead_pi_packages = []\n"));
    let config = config.replacen("lead_pi_packages = []\n", "lead_pi_packages = []\nplannotator_ports = \"19432-19432\"\n", 1);
    fs::write(env.cpd_home().join("config.toml"), config).unwrap();

    run_lead(&env, LEAD1);
    wait_until("the first Lead's pi", || recorded_ports(&env, LEAD1).len() == 1);
    assert_eq!(recorded_ports(&env, LEAD1), ["19432"]);

    let pane = run_lead(&env, LEAD2);
    wait_until("no_port", || capture(&env, &pane).contains("Press Enter to retry."));
    let shown = capture(&env, &pane);
    assert!(shown.contains("no_port"), "{shown}");
    assert!(shown.contains("held by pay (19432)"), "{shown}");
    assert!(recorded_ports(&env, LEAD2).is_empty(), "pi is not started");

    // Still full: Enter shows it again. The holder lets go: Enter starts pi with the port.
    env.tmux(&["send-keys", "-t", &pane, "Enter"]);
    wait_until("the message again", || capture(&env, &pane).matches("Press Enter to retry.").count() == 2);
    assert!(recorded_ports(&env, LEAD2).is_empty());
    fs::remove_file(home(&env).plannotator_port(&PlanId::parse("pay").unwrap())).unwrap();
    env.tmux(&["send-keys", "-t", &pane, "Enter"]);
    wait_until("pi with the port", || recorded_ports(&env, LEAD2) == ["19432"]);
}

// ---------- run: the User's quit archives (commands.md §10.8) ----------

/// A fake `pi` that waits for [`quit`], then exits with the status given there; on `SIGTERM` it
/// exits 0, as pi 1.2.0 does (`interactive-mode.js`: `shutdown({fromSignal})` ends in `process.exit(0)`).
fn quitting_pi(env: &TestEnv) {
    fake_pi(
        env,
        "trap 'exit 0' TERM\nwhile [ ! -e \"$HOME/quit.$CPD_SESSION\" ]; do sleep 0.1; done\nexit $(cat \"$HOME/quit.$CPD_SESSION\")",
    );
}

/// Makes `id`'s pi exit with `status`.
fn quit(env: &TestEnv, id: &str, status: u8) {
    let tmp = env.home().join(format!("quit.{id}.tmp"));
    fs::write(&tmp, status.to_string()).unwrap();
    fs::rename(tmp, env.home().join(format!("quit.{id}"))).unwrap();
}

/// pi's session file for `id`, so `session run` resumes it; `finished` ends it on a finished turn.
fn pi_file(env: &TestEnv, id: &str, finished: bool) {
    let dir = env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&dir).unwrap();
    let turn = if finished { "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n" } else { "" };
    fs::write(dir.join(format!("2026_{id}.jsonl")), format!("{{\"type\":\"session\"}}\n{turn}")).unwrap();
}

/// The fixture of the quit tests: a side fork `side` of `lead1` and `side2` of `lead2`, both idle
/// with a session file, a quitting pi and the cockpit.
fn quit_fixture(env: &TestEnv) {
    setup(env);
    env.fake_cpd_views();
    quitting_pi(env);
    cockpit(env);
    exec_sql(
        env,
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, forked_from) VALUES
           ('side', 'lead', 'ctx', 'pay', 'Side', '/', 'O3', 'idle', 't', 't', 'lead1'),
           ('side2', 'lead', 'ctx', 'other', 'Side two', '/', 'O3', 'idle', 't', 't', 'lead2')",
    );
    for id in ["side", "side2", LEAD1, LEAD2] {
        pi_file(env, id, false);
    }
}

/// [`run_lead`], and the Session's row has the pane, as when cpd started it.
fn run_lead_with_pane(env: &TestEnv, id: &str) -> String {
    let pane = run_lead(env, id);
    exec_sql(env, &format!("UPDATE sessions SET tmux_pane = '{pane}' WHERE id = '{id}'"));
    pane
}

fn session_archived(env: &TestEnv, id: &str) -> bool {
    show(env, id)["archived_at"].is_string()
}

fn pane_exists(env: &TestEnv, pane: &str) -> bool {
    panes(env).iter().any(|p| p.pane == pane)
}

#[test]
fn quitting_pi_archives_a_side_fork_and_closes_its_pane() {
    let env = TestEnv::new();
    quit_fixture(&env);
    let pane = run_lead_with_pane(&env, "side");
    wait_until("pi", || !env.calls("pi").is_empty());
    quit(&env, "side", 0);
    wait_until("the archive", || session_archived(&env, "side"));
    wait_until("its pane to go", || !pane_exists(&env, &pane));
    let row = show(&env, "side");
    assert_eq!((&row["state"], &row["tmux_pane"], &row["place"]), (&json!("stopped"), &Value::Null, &json!("none")));
    assert_eq!(events(&env, "session-archived").len(), 1);
    // Nothing else was archived.
    for other in [LEAD1, LEAD2, "side2"] {
        assert!(!session_archived(&env, other), "{other}");
    }
    assert_eq!(query::<Option<String>>(&env, "SELECT archived_at FROM plans WHERE id = 'pay'"), None);
}

#[test]
fn quitting_a_plan_s_own_lead_archives_the_plan_with_its_fork() {
    let env = TestEnv::new();
    quit_fixture(&env);
    let lead = run_lead_with_pane(&env, LEAD2);
    let fork = run_lead_with_pane(&env, "side2");
    wait_until("both pi", || env.calls("pi").len() == 2);
    quit(&env, LEAD2, 0);
    wait_until("the archive", || session_archived(&env, LEAD2));
    wait_until("both panes to go", || !pane_exists(&env, &lead) && !pane_exists(&env, &fork));
    assert!(session_archived(&env, "side2"));
    assert!(query::<Option<String>>(&env, "SELECT archived_at FROM plans WHERE id = 'other'").is_some());
    assert_eq!(events(&env, "plan-archived").len(), 1);
    assert_eq!(events(&env, "session-archived").len(), 2);
    // Another Plan is untouched.
    assert!(!session_archived(&env, LEAD1));
    assert_eq!(query::<Option<String>>(&env, "SELECT archived_at FROM plans WHERE id = 'pay'"), None);
}

#[test]
fn quitting_the_lead_in_the_chat_slot_parks_it_first_and_the_placeholder_holds_the_slot() {
    let env = TestEnv::new();
    quit_fixture(&env);
    env.cpd(&["session", "open", "side"]).ok();
    let pane = slot(&env).pane;
    assert_eq!(slot(&env).session, "side");
    wait_until("pi", || !env.calls("pi").is_empty());
    quit(&env, "side", 0);
    wait_until("the archive", || session_archived(&env, "side"));
    wait_until("its pane to go", || !pane_exists(&env, &pane));
    let held = slot(&env);
    assert_eq!((held.session.as_str(), held.view.as_str()), ("", "chat"), "{held:?}");
    assert_placeholders(&env, 1);
    assert!(panes(&env).iter().all(|p| p.session != "side"));
    assert_eq!(window_names(&env), ["main"], "the Lead's park window went with its pane");
    // Parked first: `session-moved` to `parked`, then to none.
    let places: Vec<Value> = events(&env, "session-moved").iter().filter(|e| e["session"] == "side").map(|e| e["data"]["place"].clone()).collect();
    assert_eq!(places, [json!("chat"), json!("parked"), json!("none")]);
}

#[test]
fn a_pi_that_exits_with_status_1_is_not_archived() {
    let env = TestEnv::new();
    quit_fixture(&env);
    let pane = run_lead_with_pane(&env, "side");
    wait_until("pi", || !env.calls("pi").is_empty());
    quit(&env, "side", 1);
    wait_until("the resume prompt", || capture(&env, &pane).contains("pi exited. Press Enter to resume Side here."));
    assert!(!session_archived(&env, "side"));
    assert_eq!(show(&env, "side")["state"], "stopped");
    assert!(pane_exists(&env, &pane));
    assert_eq!(events(&env, "session-archived").len(), 0);
}

#[test]
fn a_refused_archive_prints_the_refusal_and_offers_the_resume_in_place() {
    let env = TestEnv::new();
    quit_fixture(&env);
    // `pay` has a running Job; its Lead holds the chat slot.
    env.cpd(&["session", "open", LEAD1]).ok();
    let pane = slot(&env).pane;
    wait_until("pi", || !env.calls("pi").is_empty());
    quit(&env, LEAD1, 0);
    wait_until("the resume prompt", || capture(&env, &pane).contains("pi exited. Press Enter to resume Lead one here."));
    let shown = capture(&env, &pane);
    assert!(shown.contains("archive_blocked"), "{shown}");
    assert!(shown.contains("Job api of Plan pay is running."), "{shown}");
    // Nothing was archived, and the Lead was not parked.
    assert!(!session_archived(&env, LEAD1));
    assert_eq!(query::<Option<String>>(&env, "SELECT archived_at FROM plans WHERE id = 'pay'"), None);
    assert_eq!(events(&env, "session-archived").len() + events(&env, "plan-archived").len(), 0);
    assert_eq!((slot(&env).pane, slot(&env).session), (pane, LEAD1.to_owned()));
}

#[test]
fn a_reload_is_not_a_quit_even_though_pi_exits_0_on_sigterm() {
    let env = TestEnv::new();
    quit_fixture(&env);
    // An idle Lead at a finished turn, parked: the restart for a new pi is due at once.
    pi_file(&env, "side", true);
    backdate(&env.home().join(".pi/agent/sessions/g/2026_side.jsonl"), Duration::from_secs(3600));
    exec_sql(&env, "UPDATE sessions SET state_at = '2020-01-01T00:00:00Z' WHERE id = 'side'");
    // pi's install, as the wrapper watches it (a cpd install does not restart pi, hs-d210).
    let current = env.home().join(".pi/agent/install/current-version");
    fs::create_dir_all(current.parent().unwrap()).unwrap();
    fs::write(&current, "1.2.0\n").unwrap();
    let pane = run_lead_with_pane(&env, "side");
    wait_until("pi", || env.calls("pi").len() == 1);

    // A new pi, as pi's own update installs it.
    let next = current.with_extension("tmp");
    fs::write(&next, "1.3.0\n").unwrap();
    fs::rename(&next, &current).unwrap();
    wait_until("pi, started again", || env.calls("pi").len() == 2);
    assert!(pane_exists(&env, &pane));
    assert!(!session_archived(&env, "side"));
    assert_eq!(events(&env, "session-archived").len(), 0);
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
    exec_sql(
        env,
        &format!(
            "INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('cafe', 'ctx', 'Cafe', 'grounding', '{CAFE_LEAD}', 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
               ('{UUID_A}', 'lead', 'ctx', 'pay', 'A', '/', 'O3', 'stopped', 't', 't4'),
               ('{UUID_B}', 'lead', 'ctx', 'pay', 'B', '/', 'O3', 'stopped', 't', 't5'),
               ('{CAFE_LEAD}', 'lead', 'ctx', 'cafe', 'Cafe lead', '/', 'O3', 'stopped', 't', 't6'),
               ('{CAFE_PREFIXED}', 'lead', 'ctx', 'cafe', 'Cafe prefixed', '/', 'O3', 'stopped', 't', 't7')"
        ),
    );
}

fn resolved(env: &TestEnv, value: &str) -> String {
    env.cpd(&["session", "show", value]).ok()["session"]["id"].as_str().unwrap().to_owned()
}

#[test]
fn every_short_form_resolves_to_the_full_id() {
    let env = TestEnv::new();
    setup_short(&env);
    // 1: the full id.
    assert_eq!(resolved(&env, UUID_A), UUID_A);
    assert_eq!(resolved(&env, LEAD2), LEAD2);
    // 2: a Plan id is that Plan's Lead.
    assert_eq!(resolved(&env, "pay"), LEAD1);
    assert_eq!(resolved(&env, "other"), LEAD2);
    // 3: <plan>.<prefix> and <plan>·<prefix>, within the Plan.
    assert_eq!(resolved(&env, "pay.a1b2c3d4-0000-4000-8000-000000000001"), UUID_A);
    assert_eq!(resolved(&env, "pay·worker"), WORKER);
    assert_eq!(resolved(&env, "pay.w"), WORKER);
    assert_eq!(resolved(&env, "other.l"), LEAD2);
    // 4: a bare prefix of 4 or more characters, of any Plan or none.
    assert_eq!(resolved(&env, "work"), WORKER);
    assert_eq!(resolved(&env, "a1b2c3d4-0000-4000-8000-000000000002"), UUID_B);
    assert_eq!(resolved(&env, "mana"), MANAGER);
}

#[test]
fn a_plan_id_wins_over_a_prefix_that_is_also_a_plan_id() {
    let env = TestEnv::new();
    setup_short(&env);
    // `cafe` is the id of a Plan and the 4-character prefix of another Session.
    assert_eq!(resolved(&env, "cafe"), CAFE_LEAD);
    // The longer prefix is not a Plan id.
    assert_eq!(resolved(&env, "cafe0"), CAFE_PREFIXED);
}

#[test]
fn an_ambiguous_prefix_is_refused_with_its_candidates() {
    let env = TestEnv::new();
    setup_short(&env);
    for value in ["a1b2", "a1b2c3d4", "pay.a1b2"] {
        let error = refused(&env.cpd(&["session", "show", value]), "session_ambiguous", 3);
        assert_eq!(error["details"]["candidates"], json!([UUID_A, UUID_B]), "{value}");
    }
    let error = refused(&env.cpd(&["session", "show", "lead"]), "session_ambiguous", 3);
    assert_eq!(error["details"]["candidates"], json!([LEAD1, LEAD2]));
    // Every command that takes a Session refuses it; nothing is written.
    refused(&env.cpd(&["session", "rename", "x", "--session", "a1b2"]), "session_ambiguous", 3);
    refused(&env.cpd(&["session", "archive", "a1b2"]), "session_ambiguous", 3);
    refused(&env.cpd(&["session", "open", "a1b2"]), "session_ambiguous", 3);
    refused(&env.cpd(&["session", "send", "a1b2", "hi"]), "session_ambiguous", 3);
    assert_eq!(events(&env, "session-renamed").len(), 0);
    assert_eq!(events(&env, "session-message").len(), 0);
}

#[test]
fn a_prefix_of_fewer_than_4_characters_is_not_accepted() {
    let env = TestEnv::new();
    setup_short(&env);
    for value in ["wor", "a1b", "man", "w"] {
        refused(&env.cpd(&["session", "show", value]), "session_not_found", 4);
    }
    // A Plan id of 2 or 3 characters still resolves, and so does a scoped prefix of 1.
    assert_eq!(resolved(&env, "pay"), LEAD1);
    assert_eq!(resolved(&env, "pay.w"), WORKER);
}

#[test]
fn unmatched_values_are_session_not_found_and_malformed_ones_usage_errors() {
    let env = TestEnv::new();
    setup_short(&env);
    for value in ["nobody", "Lead1", "WORK", "pay.zzz", "other.worker", "nosuch.lead", "pay·zzz"] {
        let error = refused(&env.cpd(&["session", "show", value]), "session_not_found", 4);
        // The reference is echoed in its canonical `.` form.
        assert_eq!(error["details"]["session"], value.replace('·', "."));
    }
    for value in ["pay.", "pay·", ".lead", "Pay.lead", "pay.-a", "a/b"] {
        refused(&env.cpd(&["session", "show", value]), "invalid_session_id", 2);
    }
}

#[test]
fn every_session_command_accepts_the_short_forms() {
    let env = TestEnv::new();
    setup_short(&env);
    env.fake_cpd_views();
    cockpit(&env);
    // `--session` and `CPD_SESSION`.
    let out = env.cpd(&["session", "rename", "renamed", "--session", "pay"]).ok();
    assert_eq!(out, json!({"ok": true, "session": LEAD1, "name": "renamed"}));
    let out = cpd_with(&env, &[("CPD_SESSION", "other")], &["session", "rename", "again"]).ok();
    assert_eq!(out["session"], LEAD2);
    let out = cpd_with(&env, &[("CPD_SESSION", "other")], &["session", "show"]).ok();
    assert_eq!(out["session"]["id"], LEAD2);
    // send, to a Lead by its Plan id and to the Manager by a bare prefix.
    assert_eq!(env.cpd(&["session", "send", "pay", "hi"]).ok()["session"], LEAD1);
    assert_eq!(env.cpd(&["session", "send", "mana", "hi"]).ok()["session"], MANAGER);
    // close (no pane: nothing to do), archive and unarchive.
    assert_eq!(env.cpd(&["session", "close", "other"]).ok(), json!({"ok": true, "session": LEAD2, "pane": null, "already": true}));
    let out = env.cpd(&["session", "archive", "pay.a1b2c3d4-0000-4000-8000-000000000001"]).ok();
    assert_eq!(out["archived"], json!([{"kind": "session", "id": UUID_A}]));
    assert_eq!(archived_at(&env, UUID_A).is_string(), true);
    // An archived Session can be named by a bare prefix to unarchive it.
    let out = env.cpd(&["session", "unarchive", "pay.a1b2c3d4-0000-4000-8000-000000000001"]).ok();
    assert_eq!(out["archived"], json!([{"kind": "session", "id": UUID_A}]));
    // open and park name a Session that does not exist.
    refused(&env.cpd(&["session", "open", "nobody"]), "session_not_found", 4);
    refused(&env.cpd(&["session", "fork", "nobody"]), "session_not_found", 4);
    refused(&env.cpd(&["session", "park", "nobody"]), "session_not_found", 4);
    refused(&env.cpd(&["session", "history", "nobody"]), "session_not_found", 4);
}
