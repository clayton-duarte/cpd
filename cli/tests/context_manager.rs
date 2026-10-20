//! `cpd context manager` (commands.md §3.10, contract §3.1 "The Context's
//! Manager"; B3-A1, B3-A2) and `session open` of a Manager (§10.4), run as the
//! built binary against a private tmux server, with fake `pi` and `cpd`
//! scripts on `PATH`.

mod common;

use std::fs;

use common::{CPD, SLEEP_SECONDS, TestEnv, refused, wait_until};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::{Value, json};

fn store(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn query<T: rusqlite::types::FromSql>(env: &TestEnv, sql: &str) -> T {
    store(env).read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

/// A fake `pi` that creates its session file (when given `--session-id`), records
/// its environment per Session, and sleeps.
fn fake_pi(env: &TestEnv) {
    env.fake_pi(&format!(
        "id=''; prev=''\nfor a in \"$@\"; do [ \"$prev\" = --session-id ] && id=$a; prev=$a; done\n\
         if [ -n \"$id\" ]; then mkdir -p \"$HOME/.pi/agent/sessions/g\"; \
         printf '{{\"type\":\"session\"}}\\n' > \"$HOME/.pi/agent/sessions/g/2026_$id.jsonl\"; fi\n\
         env > \"$HOME/pi-$CPD_SESSION.$$.env\"\nexec sleep {SLEEP_SECONDS}"
    ));
}

/// Whether the captured environment `dump` has the line `var` (`NAME=value`). A failure says nothing of the environment.
fn has_var(dump: &str, var: &str) -> bool {
    dump.lines().any(|l| l == var)
}

/// The names of the variables in `dump` that start with `prefix`: what a failing assertion may print (the
/// environment holds the caller's tokens).
fn var_names<'a>(dump: &'a str, prefix: &str) -> Vec<&'a str> {
    dump.lines().filter_map(|l| l.split_once('=')).map(|(name, _)| name).filter(|name| name.starts_with(prefix)).collect()
}

/// The cockpit: session `cpd`, window `main` with a tree pane and the placeholder in the chat slot.
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

/// The env with its config, the fakes, Context `ctx` with a note, and no cockpit yet.
fn setup(env: &TestEnv) {
    env.write_tmux_config("");
    env.fake_cpd_views();
    fake_pi(env);
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    env.cpd(&["context", "note", "Mind the ledger.", "--context", "ctx"]).ok();
}

/// `(pane, window name, @cpd_session, @cpd_view)` of every pane.
fn panes(env: &TestEnv) -> Vec<[String; 4]> {
    let out = env.tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{window_name}\t#{@cpd_session}\t#{@cpd_view}"]);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let f: Vec<String> = l.split('\t').map(str::to_owned).collect();
            [f[0].clone(), f[1].clone(), f[2].clone(), f[3].clone()]
        })
        .collect()
}

/// The chat slot: the pane of `main` with a Session or the placeholder.
fn slot(env: &TestEnv) -> [String; 4] {
    let found: Vec<_> = panes(env).into_iter().filter(|p| p[1] == "main" && (!p[2].is_empty() || p[3] == "chat")).collect();
    assert_eq!(found.len(), 1, "one chat slot: {found:?}");
    found[0].clone()
}

fn window_names(env: &TestEnv) -> Vec<String> {
    let out = env.tmux(&["list-windows", "-t", "=cpd", "-F", "#{window_name}"]);
    let mut names: Vec<String> = String::from_utf8(out.stdout).unwrap().lines().map(str::to_owned).collect();
    names.sort();
    names
}

/// The environment files the fake pi wrote for `session`, once there are `count` of them.
fn pi_envs(env: &TestEnv, session: &str, count: usize) -> Vec<String> {
    let prefix = format!("pi-{session}.");
    let read = || {
        let mut found: Vec<_> = fs::read_dir(env.home())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(&prefix))
            .filter_map(|p| fs::read_to_string(p).ok().filter(|t| t.contains("CPD_SESSION")))
            .collect();
        found.sort();
        found
    };
    wait_until("the fake pi's environment", || read().len() >= count);
    read()
}

fn manager_ids(env: &TestEnv) -> Vec<String> {
    let out = env.cpd(&["session", "list", "--role", "manager"]).ok();
    out["sessions"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn the_first_call_registers_the_manager_and_starts_pi_in_its_window() {
    let env = TestEnv::new();
    setup(&env);
    cockpit(&env);
    let placeholder = slot(&env);

    let out = env.cpd(&["context", "manager", "--context", "ctx"]).ok();
    assert_eq!(out["started"], true);
    let manager = &out["manager"];
    let id = manager["session"].as_str().unwrap().to_owned();
    assert_eq!((&manager["name"], &manager["state"]), (&json!("ctx-manager"), &json!("stopped")));

    // The Session row.
    let row = env.cpd(&["session", "show", &id]).ok()["session"].clone();
    assert_eq!(
        (&row["role"], &row["plan"], &row["context"], &row["tier"], &row["forked_from"], &row["state"]),
        (&json!("manager"), &Value::Null, &json!("ctx"), &json!("O5"), &Value::Null, &json!("stopped"))
    );
    assert_eq!(row["cwd"], env.root().canonicalize().unwrap().display().to_string());
    assert_eq!(row["tmux_pane"], manager["pane"]);
    assert_eq!(row["place"], "parked");

    // Its window, parked, with @cpd_session.
    let pane = manager["pane"].as_str().unwrap();
    let mine = panes(&env).into_iter().find(|p| p[0] == pane).unwrap();
    assert_eq!((mine[1].as_str(), mine[2].as_str()), ("ctx-ctx", id.as_str()));
    assert_eq!(window_names(&env), ["ctx-ctx", "main"]);

    // The Events.
    let registered = events(&env, "session-registered");
    assert_eq!(registered.len(), 1);
    assert_eq!(registered[0]["session"], json!(id));
    assert_eq!(registered[0]["data"], json!({"role": "manager", "name": "ctx-manager", "forked_from": null, "tier": "O5"}));
    let moved = events(&env, "session-moved");
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0]["data"]["place"], "parked");
    assert_eq!(moved[0]["data"]["pane"], json!(pane));

    // pi: a fresh start at the Manager's Tier, with the first message, and the Manager's environment.
    let envs = pi_envs(&env, &id, 1);
    let vars = &envs[0];
    assert!(has_var(vars, &format!("CPD_SESSION={id}")), "CPD_SESSION is not the Manager's Session");
    assert!(has_var(vars, &format!("CPD_HOME={}", env.cpd_home().display())), "CPD_HOME is not this env's");
    assert_eq!(var_names(vars, "CPD_PLAN"), Vec::<&str>::new(), "a Manager has no Plan");
    assert_eq!(var_names(vars, "PLANNOTATOR_"), ["PLANNOTATOR_UPDATE_CHECK"], "the only plannotator variable of a Manager");
    let calls = env.calls("pi");
    assert_eq!(calls.len(), 1);
    let argv = &calls[0];
    let at = |flag: &str| argv.iter().position(|a| a == flag).unwrap_or_else(|| panic!("{flag} in {argv:?}"));
    assert!(argv[at("--model") + 1].ends_with(":max"), "O5's level: {argv:?}");
    assert_eq!(argv[at("--session-id") + 1], id);
    assert_eq!(argv[at("-n") + 1], "ctx-manager");
    // The first message goes as its file, `@<path>` (hs-n111), as a Lead's does.
    let path = env.cpd_home().join("run/ctx/manager-message.md");
    assert_eq!(argv[at("--") + 1..], [format!("@{}", path.display())], "the file pi was given");
    let first = fs::read_to_string(&path).unwrap();
    assert!(first.starts_with("You are the Manager of Context ctx \"Payments\". Follow the cpd-manager skill.\n\n"), "{first}");
    assert!(first.contains("Mind the ledger."), "every record: {first}");

    // The chat slot did not move (hs-d103).
    assert_eq!(slot(&env), placeholder);
}

#[test]
fn a_managers_pi_gets_no_plannotator_variable_from_its_callers() {
    let env = TestEnv::new();
    setup(&env);
    cockpit(&env);
    // The tmux server's global environment is what every new pane starts from.
    for (name, value) in [("PLANNOTATOR_DATA_DIR", "/tmp/x"), ("PLANNOTATOR_PORT", "1"), ("PLANNOTATOR_REMOTE", "1")] {
        let out = env.tmux(&["set-environment", "-g", name, value]);
        assert!(out.status.success(), "tmux set-environment {name}");
    }
    // The caller of `cpd context manager` has them too.
    let out = env
        .command(CPD)
        .args(["context", "manager", "--context", "ctx"])
        .env("PLANNOTATOR_DATA_DIR", "/tmp/x")
        .env("PLANNOTATOR_PORT", "1")
        .env("PLANNOTATOR_REMOTE", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "cpd context manager failed: {}", String::from_utf8_lossy(&out.stderr));
    let out: Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = out["manager"]["session"].as_str().unwrap().to_owned();

    let envs = pi_envs(&env, &id, 1);
    assert_eq!(var_names(&envs[0], "PLANNOTATOR_"), ["PLANNOTATOR_UPDATE_CHECK"]);
}

#[test]
fn the_manager_tier_is_the_config_key() {
    let env = TestEnv::new();
    setup(&env);
    env.write_tmux_config("manager_tier = \"O3\"");
    cockpit(&env);
    let out = env.cpd(&["context", "manager", "--context", "ctx"]).ok();
    let id = out["manager"]["session"].as_str().unwrap();
    assert_eq!(env.cpd(&["session", "show", id]).ok()["session"]["tier"], "O3");
}

#[test]
fn a_second_call_changes_nothing() {
    let env = TestEnv::new();
    setup(&env);
    cockpit(&env);
    let first = env.cpd(&["context", "manager", "--context", "ctx"]).ok();
    let id = first["manager"]["session"].as_str().unwrap().to_owned();
    pi_envs(&env, &id, 1);
    let slot_before = slot(&env);
    let events_before: i64 = query(&env, "SELECT count(*) FROM events");
    let panes_before = panes(&env);

    let second = env.cpd(&["context", "manager", "--context", "ctx"]).ok();
    assert_eq!(second["started"], false);
    assert_eq!(second["manager"], first["manager"]);
    assert_eq!(manager_ids(&env), [id]);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM events"), events_before);
    assert_eq!(panes(&env), panes_before);
    assert_eq!(slot(&env), slot_before);
    assert_eq!(env.calls("pi").len(), 1);
}

#[test]
fn a_stopped_manager_without_a_pane_is_started_again_on_its_file() {
    let env = TestEnv::new();
    setup(&env);
    cockpit(&env);
    let first = env.cpd(&["context", "manager", "--context", "ctx"]).ok();
    let id = first["manager"]["session"].as_str().unwrap().to_owned();
    pi_envs(&env, &id, 1);
    let file = env.home().join(format!(".pi/agent/sessions/g/2026_{id}.jsonl"));
    assert!(file.exists());

    // Close its pane: stopped, no pane.
    env.cpd(&["session", "close", &id]).ok();
    let row = env.cpd(&["session", "show", &id]).ok()["session"].clone();
    assert_eq!((&row["state"], &row["tmux_pane"]), (&json!("stopped"), &Value::Null));
    assert_eq!(window_names(&env), ["main"]);
    let slot_before = slot(&env);

    let again = env.cpd(&["context", "manager", "--context", "ctx"]).ok();
    assert_eq!(again["started"], true);
    assert_eq!(again["manager"]["session"], json!(id));
    assert_ne!(again["manager"]["pane"], first["manager"]["pane"]);
    assert_eq!(manager_ids(&env), [id.clone()], "no second Manager");
    assert_eq!(events(&env, "session-registered").len(), 1);
    // `session-moved` for: parked, its pane closed, parked again.
    assert_eq!(events(&env, "session-moved").len(), 3);
    assert_eq!(window_names(&env), ["ctx-ctx", "main"]);
    assert_eq!(slot(&env), slot_before);

    // pi resumes the Session's file: no model, no first message.
    pi_envs(&env, &id, 2);
    let calls = env.calls("pi");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1][..2], ["--session".to_owned(), file.display().to_string()], "{calls:?}");
    assert!(!calls[1].contains(&"--".to_owned()), "no first message: {calls:?}");

    // And a third call changes nothing.
    assert_eq!(env.cpd(&["context", "manager", "--context", "ctx"]).ok()["started"], false);
}

#[test]
fn refusals_register_nothing() {
    let env = TestEnv::new();
    setup(&env);

    // No cockpit: refused before the Session is registered.
    let error = refused(&env.cpd(&["context", "manager", "--context", "ctx"]), "no_cockpit", 3);
    assert_eq!(error["details"]["next"], "cpd tui");
    assert!(manager_ids(&env).is_empty());

    cockpit(&env);
    refused(&env.cpd(&["context", "manager", "--context", "nobody"]), "context_not_found", 4);
    refused(&env.cpd(&["context", "manager"]), "context_required", 2);

    env.cpd(&["context", "archive", "ctx"]).ok();
    let error = refused(&env.cpd(&["context", "manager", "--context", "ctx"]), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd context unarchive ctx");

    assert!(manager_ids(&env).is_empty());
    assert_eq!(window_names(&env), ["main"]);
    assert!(env.calls("pi").is_empty());
}

#[test]
fn session_open_brings_the_manager_into_the_chat_slot() {
    let env = TestEnv::new();
    setup(&env);
    cockpit(&env);
    let placeholder = slot(&env);
    let started = env.cpd(&["context", "manager", "--context", "ctx"]).ok();
    let id = started["manager"]["session"].as_str().unwrap().to_owned();
    assert_eq!(slot(&env), placeholder, "context manager never moves the slot");

    let out = env.cpd(&["session", "open", &id]).ok();
    assert_eq!((&out["session"], &out["displaced"], &out["pane"]), (&json!(id), &Value::Null, &started["manager"]["pane"]));
    let now = slot(&env);
    assert_eq!((now[0].as_str(), now[2].as_str()), (started["manager"]["pane"].as_str().unwrap(), id.as_str()));
    assert!(panes(&env).iter().all(|p| p[3] != "chat"), "the placeholder died with its window");
    assert_eq!(window_names(&env), ["main"]);
    assert_eq!(env.cpd(&["session", "show", &id]).ok()["session"]["place"], "chat");
    assert_eq!(env.cpd(&["session", "open", &id]).ok()["already"], true);

    // Parking it puts it back in `ctx-<context>`, alone, with a placeholder in the slot.
    env.cpd(&["session", "park"]).ok();
    assert_eq!(slot(&env)[3], "chat");
    let parked = panes(&env).into_iter().find(|p| p[2] == id).unwrap();
    assert_eq!(parked[1], "ctx-ctx");
    pi_envs(&env, &id, 1);
    assert_eq!(env.calls("pi").len(), 1, "it was started once");
}
