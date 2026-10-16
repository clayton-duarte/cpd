//! Sessions (contract §3.6, §6.4, §6.7; commands.md §10 with §19 amendments
//! 1–3), run as the built binary against a private tmux server, with fake
//! `pi` and `cpd` scripts on `PATH`.

mod common;

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::{CPD, Run, SLEEP_SECONDS, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::events::{Kind, NewEvent, append};
use cpd_core::home::CpdHome;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

const LEAD1: &str = "lead1";
const LEAD2: &str = "lead2";
const GROUNDED: &str = "grounded";
const WORKER: &str = "worker1";

fn home(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

fn store(env: &TestEnv) -> Store {
    Store::open(&home(env), 5000).unwrap()
}

/// `config.toml` on this env's private tmux server, with a 1 s quiet time for forks.
fn write_config(env: &TestEnv) {
    let config = format!(
        "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[timeouts]\nfork_quiet_seconds = 1\n[tmux]\nsocket = \"{}\"\n",
        env.tmux_socket()
    );
    fs::write(env.cpd_home().join("config.toml"), config).unwrap();
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn sid(id: &str) -> SessionId {
    SessionId::parse(id).unwrap()
}

/// Context `ctx` with grounded Lead `grounded`; Plans `pay` (Lead `lead1`, idle;
/// Job `api` with Attempt `api-a1`, Worker `worker1` and an open Escalation)
/// and `other` (Lead `lead2`, stopped). Every Session's cwd is the env's root.
fn setup(env: &TestEnv) {
    write_config(env);
    let cwd = env.root().display().to_string();
    let mut store = store(env);
    store
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO contexts (id, title, created_at, created_on, lead_session, grounded_seq)
                       VALUES ('ctx', 'Ctx', 't', 'm1', 'grounded', 0);
                     INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at) VALUES
                       ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead one', '{cwd}', 'O3', 'idle', 't1', 't1'),
                       ('lead2', 'lead', 'ctx', 'other', NULL, 'Lead two', '{cwd}', 'O3', 'stopped', 't2', 't2'),
                       ('grounded', 'lead', 'ctx', NULL, NULL, 'Grounded', '{cwd}', 'O3', 'stopped', 't0', 't0'),
                       ('worker1', 'worker', 'ctx', 'pay', 'api-a1', 'pay api a1', '{cwd}', 'S2', 'working', 't3', 't3');
                     INSERT INTO plans (id, context, title, repo, base, lead_branch, phase, lead_session, created_at) VALUES
                       ('pay', 'ctx', 'Pay', '/r', 'main', 'cpd/pay', 'grounding', 'lead1', 't'),
                       ('other', 'ctx', 'Other', '/r', 'main', 'cpd/other', 'grounding', 'lead2', 't');
                     INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at)
                       VALUES ('pay', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', {id}, 't');
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

fn refused(run: &Run, code: &str, exit: i32) -> Value {
    let error = run.error();
    assert_eq!(error["code"], code, "{run:?}");
    assert_eq!(run.exit, exit, "{run:?}");
    error
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

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---------- tmux ----------

/// A fake `cpd` on `PATH` for the panes: `cpd tui view chat` (the TUI Job's
/// placeholder view) just sleeps; everything else is the built binary.
fn fake_cpd(env: &TestEnv) {
    env.fake("cpd", &format!("case \"$1 $2 $3\" in \"tui view chat\") exec sleep {SLEEP_SECONDS};; esac\nexec '{CPD}' \"$@\""));
}

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
}

fn panes(env: &TestEnv) -> Vec<P> {
    let out = env.tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{window_id}\t#{window_name}\t#{@cpd_session}\t#{@cpd_view}"]);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            P { pane: f[0].into(), window: f[1].into(), window_name: f[2].into(), session: f[3].into(), view: f[4].into() }
        })
        .collect()
}

/// The chat slot: the pane of `main` with a Session or the placeholder.
fn slot(env: &TestEnv) -> P {
    let found: Vec<P> =
        panes(env).into_iter().filter(|p| p.window_name == "main" && (!p.session.is_empty() || p.view == "chat")).collect();
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
               "state": "idle", "state_label": null, "state_at": "t1", "place": "none", "tmux_pane": null, "file": null,
               "forked_from": null, "plan_lead": true, "grounded_lead": false, "tier": "O3",
               "cwd": env.root().display().to_string()})
    );
    let grounded = sessions.iter().find(|s| s["id"] == GROUNDED).unwrap();
    assert_eq!((&grounded["plan_lead"], &grounded["grounded_lead"]), (&json!(false), &json!(true)));

    assert_eq!(ids(env.cpd(&["session", "list", "--plan", "pay"])), [LEAD1, WORKER]);
    assert_eq!(ids(env.cpd(&["session", "list", "--context", "ctx", "--role", "worker"])), [WORKER]);
    assert_eq!(ids(env.cpd(&["session", "list", "--state", "idle", "--state", "working"])), [LEAD1, WORKER]);
    assert_eq!(ids(env.cpd(&["session", "list", "--role", "lead", "--state", "stopped"])), [GROUNDED, LEAD2]);
    assert!(ids(env.cpd(&["session", "list", "--context", "elsewhere"])).is_empty());
    // Filters only: CPD_PLAN does not select anything.
    assert_eq!(cpd_with(&env, &[("CPD_PLAN", "pay")], &["session", "list"]).ok()["sessions"].as_array().unwrap().len(), 4);

    refused(&env.cpd(&["session", "list", "--role", "reviewer"]), "usage", 2);
    refused(&env.cpd(&["session", "list", "--state", "busy"]), "usage", 2);
    refused(&env.cpd(&["session", "list", "--plan", "Bad"]), "invalid_plan_id", 2);
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

    // The grounded Lead has no Plan: its Event has none either.
    cpd_with(&env, &[("CPD_SESSION", GROUNDED)], &["session", "state", "idle"]).ok();
    assert_eq!(events(&env, "session-state").last().unwrap()["plan"], Value::Null);

    refused(&state(&["blocked"]), "usage", 2);
    refused(&state(&["idle", "--label", "x"]), "usage", 2);
    refused(&state(&["blocked", "--label", &"x".repeat(81)]), "usage", 2);
    refused(&state(&["asleep"]), "usage", 2);
    refused(&env.cpd(&["session", "state", "idle"]), "session_required", 2);
    refused(&env.cpd(&["session", "state", "idle", "--session", "nobody"]), "session_not_found", 4);
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
    refused(&env.cpd(&["session", "fork", GROUNDED]), "plan_required", 2);
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
    fake_cpd(&env);
    fake_pi(&env, &format!("exec sleep {SLEEP_SECONDS}"));
    cockpit(&env);
    let message = env.root().join("message.md");
    fs::write(&message, "Look at the tests.").unwrap();

    let out = env.cpd(&["session", "fork", GROUNDED, "--plan", "pay", "--message-file", message.to_str().unwrap()]).ok();
    let new = out["session"]["id"].as_str().unwrap().to_owned();
    assert!(SessionId::parse(&new).is_ok() && new.len() == 36, "a UUID: {new}");
    assert_eq!(out["session"]["name"], "Grounded (fork)");
    assert_eq!((&out["session"]["plan"], &out["session"]["forked_from"]), (&json!("pay"), &json!(GROUNDED)));

    let row = show(&env, &new);
    assert_eq!(
        (&row["role"], &row["context"], &row["state"], &row["tier"], &row["forked_from"], &row["place"]),
        (&json!("lead"), &json!("ctx"), &json!("stopped"), &json!("O3"), &json!(GROUNDED), &json!("parked"))
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
    assert_eq!(registered["data"], json!({"role": "lead", "name": "Grounded (fork)", "forked_from": GROUNDED, "tier": "O3"}));
    let moved = events(&env, "session-moved");
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0]["data"], json!({"pane": out["session"]["pane"], "window": out["session"]["window"], "place": "parked"}));

    // The default Plan is the source's; each call registers a new Session.
    let again = env.cpd(&["session", "fork", LEAD1]).ok();
    assert_eq!((&again["session"]["plan"], &again["session"]["name"]), (&json!("pay"), &json!("Lead one (fork)")));
    assert_ne!(again["session"]["id"], json!(new));
}

/// Fork when idle (§6.7): the fork waits while the source has no file, then
/// while it is busy, and forks once it is idle; pi exiting offers a resume.
#[test]
fn fork_waits_until_the_source_is_idle_then_forks_and_resumes() {
    let env = TestEnv::new();
    setup(&env);
    fake_cpd(&env);
    fake_pi(&env, "exit 0");
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
    assert_eq!(fork[fork.len() - 2..], ["--".to_owned(), "Review the tests.".to_owned()]);

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

// ---------- open, park ----------

#[test]
fn open_and_park_keep_at_most_one_placeholder_only_in_the_slot() {
    let env = TestEnv::new();
    setup(&env);
    fake_cpd(&env);
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
    assert!(env.calls("pi")[0].ends_with(&["--".to_owned(), "Plan pay.".to_owned()]), "a fresh Plan Lead gets lead-message.md");

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
fn open_and_park_refusals() {
    let env = TestEnv::new();
    setup(&env);
    // No cockpit yet.
    let error = refused(&env.cpd(&["session", "open", LEAD1]), "no_cockpit", 3);
    assert_eq!(error["details"]["next"], "cpd tui");
    refused(&env.cpd(&["session", "park"]), "no_cockpit", 3);
    refused(&env.cpd(&["session", "park", LEAD1]), "no_cockpit", 3);

    fake_cpd(&env);
    cockpit(&env);
    refused(&env.cpd(&["session", "open", WORKER]), "not_a_lead", 3);
    refused(&env.cpd(&["session", "open", GROUNDED]), "grounded_lead", 3);
    refused(&env.cpd(&["session", "open", "nobody"]), "session_not_found", 4);
    refused(&env.cpd(&["session", "park", WORKER]), "not_a_lead", 3);
    refused(&env.cpd(&["session", "park", "nobody"]), "session_not_found", 4);
    assert_placeholders(&env, 1);
    assert_eq!(window_names(&env), ["main"]);
}

// ---------- close ----------

#[test]
fn close_refusals_and_nothing_to_close() {
    let env = TestEnv::new();
    setup(&env);
    refused(&env.cpd(&["session", "close", WORKER]), "not_a_lead", 3);
    refused(&env.cpd(&["session", "close", "nobody"]), "session_not_found", 4);
    // A Lead with no pane: nothing to do.
    fake_cpd(&env);
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
    fake_cpd(&env);
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
    fake_cpd(&env);
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
    fake_cpd(&env);
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

// ---------- dead Lead panes (contract §3.6, §3.7 E2; commands.md §10.4) ----------

/// Like [`fake_cpd`], but `cpd session run` reads no input, so it returns when its pi
/// exits and the pane dies (`remain-on-exit`).
fn fake_cpd_returning(env: &TestEnv) {
    env.fake(
        "cpd",
        &format!(
            "case \"$1 $2 $3\" in \"tui view chat\") exec sleep {SLEEP_SECONDS};; \"session run \"*) exec '{CPD}' \"$@\" </dev/null;; esac\nexec '{CPD}' \"$@\""
        ),
    );
}

/// The cockpit with `lead2`'s pi alive and every other Lead's pi exiting at once.
fn dying_leads(env: &TestEnv) {
    setup(env);
    fake_cpd_returning(env);
    fake_pi(env, &format!("[ \"$CPD_SESSION\" = lead2 ] && exec sleep {SLEEP_SECONDS}\nexit 0"));
    cockpit(env);
    assert!(env.tmux(&["set", "-g", "remain-on-exit", "on"]).status.success());
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
    wait_until("lead1's pane to die", || is_dead(&env, &lead1_pane));
    assert_eq!(window_names(&env), ["main", "park-pay"]);

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

/// A fake `pi` that records `PLANNOTATOR_PORT` per Session and exits, and the
/// session files of both Plans' Leads, so `session run` resumes them.
fn port_fixture(env: &TestEnv) {
    setup(env);
    fake_cpd(env);
    fake_pi(env, "echo \"${PLANNOTATOR_PORT-unset}\" >> \"$HOME/ports.$CPD_SESSION\"\nexit 0");
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
    let command = format!("cpd session run {id}");
    let out = env.tmux(&["new-window", "-d", "-t", "=cpd:", "-P", "-F", "#{pane_id}", "-e", "PLANNOTATOR_PORT=1", &command]);
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
